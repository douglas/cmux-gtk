//! `goal.*` socket methods — launch and track goal-driven agent workspaces.
//! See docs/roadmap/DESIGN-goal-graph.md.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use uuid::Uuid;

use super::Response;
use crate::app::{lock_or_recover, SharedState};
use crate::goal::{self, GoalRun, GoalStatus};
use crate::model::Workspace;

/// Walk up from `start` to the nearest directory containing `.git`.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join(".git").exists() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// Resolve the runner for a request: `runner` names a configured runner;
/// `agent`/`model`/`effort` params override on top (or define an ad-hoc
/// runner when no name is given).
fn resolve_runner(
    params: &Value,
    settings: &crate::settings::GoalSettings,
) -> Result<(String, crate::settings::GoalRunner), String> {
    let name = params
        .get("runner")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| settings.default_runner.clone());
    let mut runner = if name.is_empty() {
        crate::settings::GoalRunner::default()
    } else {
        settings
            .runners
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("Unknown runner '{name}' (configure it in settings goal.runners)"))?
    };
    if let Some(a) = params.get("agent").and_then(|v| v.as_str()) {
        runner.agent = a.to_string();
    }
    if let Some(m) = params.get("model").and_then(|v| v.as_str()) {
        runner.model = m.to_string();
    }
    if let Some(e) = params.get("effort").and_then(|v| v.as_str()) {
        runner.effort = e.to_string();
    }
    if runner.agent == "custom" && runner.command_template.is_empty() {
        return Err("custom runner needs a command_template".into());
    }
    if !matches!(runner.agent.as_str(), "" | "claude" | "custom") {
        return Err(format!(
            "unknown runner agent '{}' (expected claude or custom)",
            runner.agent
        ));
    }
    let display = if name.is_empty() { "claude".to_string() } else { name };
    Ok((display, runner))
}

pub(super) fn handle_goal_create(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(goal_path) = params.get("goal").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Missing 'goal' (path to a goal .md file)");
    };
    let goal_path = Path::new(goal_path);
    if !goal_path.is_absolute() {
        return Response::error(id, "invalid_params", "'goal' must be an absolute path");
    }
    let goal_text = match std::fs::read_to_string(goal_path) {
        Ok(t) => t,
        Err(e) => {
            return Response::error(id, "not_found", &format!("Cannot read goal file: {e}"));
        }
    };
    if goal_text.len() > 256 * 1024 {
        return Response::error(id, "invalid_params", "Goal file is too large (256 KiB max)");
    }

    // cwd: explicit param, else nearest git root above the goal file, else
    // the goal file's directory.
    let cwd: PathBuf = match params.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => PathBuf::from(c),
        None => {
            let parent = goal_path.parent().unwrap_or(Path::new("/"));
            find_git_root(parent).unwrap_or_else(|| parent.to_path_buf())
        }
    };
    if !cwd.is_dir() {
        return Response::error(id, "invalid_params", "Resolved cwd is not a directory");
    }
    let cwd_str = cwd.to_string_lossy().to_string();

    let settings = crate::settings::load().goal;
    let (runner_name, runner) = match resolve_runner(params, &settings) {
        Ok(r) => r,
        Err(e) => return Response::error(id, "invalid_params", &e),
    };

    let max_iterations = params
        .get("max_iterations")
        .and_then(|v| v.as_u64())
        .map(|n| (n as u32).clamp(1, 50))
        .unwrap_or(1);
    let permission_mode = params
        .get("permission_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("acceptEdits")
        .to_string();

    // Goal name: the file stem, or the parent directory when the stem is the
    // generic "goal".
    let goal_name = {
        let stem = goal_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("goal")
            .to_string();
        if stem == "goal" {
            goal_path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .map(str::to_string)
                .unwrap_or(stem)
        } else {
            stem
        }
    };

    // Ensure the roadmap dir exists so the agent's Write and our polling
    // agree on the location.
    let roadmap = cwd.join("docs/roadmap");
    if let Err(e) = std::fs::create_dir_all(&roadmap) {
        return Response::error(id, "internal", &format!("Cannot create {roadmap:?}: {e}"));
    }
    let iteration = goal::next_iteration_number(&cwd_str);
    let feedback_rel = (iteration > 1).then(|| GoalRun::output_rel(iteration - 1));
    let feedback_ref = feedback_rel
        .as_deref()
        .filter(|rel| cwd.join(rel).exists());

    let session_id = Uuid::new_v4().to_string();
    let seed = goal::compose_seed(&goal_name, &goal_text, iteration, feedback_ref);

    // Build the workspace first so the seed file can live under its id.
    let mut ws = Workspace::with_directory(&cwd_str);
    let ws_id = ws.id;
    ws.custom_title = Some(
        params
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("goal: {goal_name}")),
    );

    let seed_dir = goal::seed_dir(ws_id);
    if let Err(e) = std::fs::create_dir_all(&seed_dir) {
        return Response::error(id, "internal", &format!("Cannot create seed dir: {e}"));
    }
    let seed_file = seed_dir.join(format!("iteration-{iteration}.md"));
    if let Err(e) = std::fs::write(&seed_file, &seed) {
        return Response::error(id, "internal", &format!("Cannot write seed file: {e}"));
    }

    let command = goal::launch_command(&runner, &session_id, &seed, &seed_file, &permission_mode);
    let is_claude = runner.agent != "custom";

    let Some(panel_id) = ws.panels.keys().next().copied() else {
        return Response::error(id, "internal", "New workspace has no panel");
    };
    if let Some(panel) = ws.panels.get_mut(&panel_id) {
        panel.command = Some(command);
        if is_claude {
            // Stamp the session identity at launch: `command=`-launched panes
            // never run shell integration, so the reporting path that
            // normally fills agent_session_id does not apply here.
            panel.agent_session_id = Some(session_id.clone());
        }
    }
    // Mirror the master's Task-tool sub-agents beside it (claude only —
    // custom runners have no transcript to mirror).
    ws.subagent_monitor = is_claude;

    {
        let mut tm = lock_or_recover(&state.tab_manager);
        let placement = crate::settings::load().new_workspace_placement;
        tm.add_workspace_with_placement(ws, placement);
    }
    state.notify_ui_refresh();

    goal::register(
        state,
        GoalRun {
            workspace_id: ws_id,
            panel_id,
            session_id: session_id.clone(),
            goal_name: goal_name.clone(),
            goal_path: goal_path.to_string_lossy().to_string(),
            cwd: cwd_str,
            iteration,
            max_iterations,
            runner_name: runner_name.clone(),
            runner,
            status: GoalStatus::Running,
            nudges_sent: 0,
            idle_ticks: 0,
            started_epoch: goal::epoch_now(),
            wall_clock_minutes: settings.wall_clock_minutes,
            last_escalation_epoch: 0,
        },
    );

    Response::success(
        id,
        serde_json::json!({
            "workspace_id": ws_id.to_string(),
            "session_id": session_id,
            "goal": goal_name,
            "iteration": iteration,
            "output": GoalRun::output_rel(iteration),
            "runner": runner_name,
        }),
    )
}

/// Parse an optional workspace UUID out of `workspace`/`workspace_id`.
fn workspace_param(params: &Value) -> Result<Option<Uuid>, ()> {
    match params
        .get("workspace")
        .or_else(|| params.get("workspace_id"))
        .and_then(|v| v.as_str())
    {
        Some(s) => Uuid::parse_str(s).map(Some).map_err(|_| ()),
        None => Ok(None),
    }
}

pub(super) fn handle_goal_status(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_filter = match workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let goals = lock_or_recover(&state.goals);
    match ws_filter {
        Some(ws_id) => match goals.get(&ws_id) {
            Some(run) => Response::success(id, run.to_json()),
            None => Response::error(id, "not_found", "No goal registered for that workspace"),
        },
        None => {
            let all: Vec<_> = goals.values().map(GoalRun::to_json).collect();
            Response::success(id, serde_json::json!({ "goals": all }))
        }
    }
}

pub(super) fn handle_goal_complete(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match workspace_param(params) {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Response::error(
                id,
                "invalid_params",
                "Missing workspace (pass --workspace or run inside a jmux pane)",
            )
        }
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let Some(run) = lock_or_recover(&state.goals).get(&ws_id).cloned() else {
        return Response::error(id, "not_found", "No goal registered for that workspace");
    };

    // The file is the source of truth; the param is only a fallback for
    // agents that call complete before/without the front matter.
    let file_status = goal::parse_iteration_file(&run.output_abs(run.iteration))
        .flatten()
        .map(|o| o.status);
    let param_status = params
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase());
    let effective = file_status.clone().or(param_status).unwrap_or_default();

    let new_status = match effective.as_str() {
        "done" => GoalStatus::Done,
        "blocked" => GoalStatus::Blocked("agent reported blocked".into()),
        _ => {
            return Response::error(
                id,
                "invalid_params",
                &format!(
                    "No usable status: iteration file {} {} — write it with front-matter status done|blocked",
                    GoalRun::output_rel(run.iteration),
                    if file_status.is_none() { "is missing or has no front matter" } else { "has an unknown status" },
                ),
            );
        }
    };

    // Terminal "blocked" with iterations remaining is the driver's business
    // (it feeds feedback forward); the fast path only records terminal states
    // and lets the next driver tick handle continuation.
    let is_done = new_status == GoalStatus::Done;
    if is_done {
        if let Some(r) = lock_or_recover(&state.goals).get_mut(&ws_id) {
            r.status = GoalStatus::Done;
        }
        let mut notifications = lock_or_recover(&state.notifications);
        notifications.add(
            "Goal complete",
            &format!("'{}' finished: {}", run.goal_name, GoalRun::output_rel(run.iteration)),
            Some(ws_id),
            None,
            true,
        );
        drop(notifications);
        state.notify_metadata_refresh();
    }

    Response::success(
        id,
        serde_json::json!({
            "workspace_id": ws_id.to_string(),
            "status": if is_done { "done" } else { "blocked" },
            "iteration": run.iteration,
        }),
    )
}
