//! `goal.*` socket methods — launch and track goal-driven agent workspaces.
//! See docs/roadmap/DESIGN-goal-graph.md.

use std::sync::Arc;

use serde_json::Value;
use uuid::Uuid;

use super::Response;
use crate::app::{lock_or_recover, SharedState};
use crate::goal::{self, GoalRun, GoalStatus};

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
    // `goal` (a .md file) or `goal_text` (inline) — see resolve_goal_source.
    let source = match goal::resolve_goal_source(params) {
        Ok(s) => s,
        Err((code, msg)) => return Response::error(id, code, &msg),
    };
    let cwd_str = source.cwd.to_string_lossy().to_string();

    let settings = crate::settings::load().goal;
    let (runner_name, runner) = match resolve_runner(params, &settings) {
        Ok(r) => r,
        Err(e) => return Response::error(id, "invalid_params", &e),
    };

    // Unset means the settings default (3), not 1: iterating on `blocked` is
    // the point of the loop.
    let max_iterations = params
        .get("max_iterations")
        .and_then(|v| v.as_u64())
        .map(|n| (n as u32).clamp(1, 50))
        .unwrap_or_else(|| settings.max_iterations.clamp(1, 50));
    // Absent = no per-invocation flag: the runner's mode, else the setting.
    let permission_mode = goal::resolve_permission_mode(
        params.get("permission_mode").and_then(|v| v.as_str()),
        &runner,
        &settings,
    );

    // Name: explicit override, else derived from the source (file stem or a
    // slug of the inline text).
    let goal_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| source.name.clone());

    let spec = goal::LaunchSpec {
        goal_name,
        goal_path: source.path.to_string_lossy().to_string(),
        goal_text: source.text,
        cwd: cwd_str,
        output_dir_rel: goal::output_dir_rel(&settings),
        upstream_refs: Vec::new(),
        runner_name,
        runner,
        max_iterations,
        permission_mode,
        wall_clock_minutes: settings.wall_clock_minutes,
        title: params
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        graph: None,
        group_id: None,
        // A human just asked for this run — land them in it.
        select: true,
    };
    match goal::launch_goal(state, spec) {
        Ok(result) => Response::success(id, result),
        Err(e) => Response::error(id, "internal", &e),
    }
}

/// The run a `goal.*` call addresses: a workspace UUID (scripts) or a goal
/// name, from `workspace` (legacy), `workspace_id`, `name` or `goal`.
fn target_param(params: &Value) -> Option<&str> {
    ["workspace", "workspace_id", "name", "goal"]
        .iter()
        .filter_map(|k| params.get(*k))
        .find_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The caller's own workspace — used only when nothing was named.
fn hint_param(params: &Value) -> Option<Uuid> {
    params
        .get("hint_workspace")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
}

/// Resolve the addressed run (name, UUID, this pane, or the sole active run).
fn resolve_target(params: &Value, state: &Arc<SharedState>) -> Result<Uuid, String> {
    let goals = lock_or_recover(&state.goals);
    goal::resolve_run(&goals, target_param(params), hint_param(params))
}

pub(super) fn handle_goal_status(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    // No target lists everything — the hint deliberately does not narrow it
    // (`jmux goal status` inside a goal pane still answers "what's running?").
    let target = target_param(params).map(str::to_string);
    let goals = lock_or_recover(&state.goals);
    match target {
        Some(t) => match goal::resolve_run(&goals, Some(&t), None) {
            Ok(ws_id) => match goals.get(&ws_id) {
                Some(run) => Response::success(id, run.to_json()),
                None => Response::error(id, "not_found", "No goal registered for that workspace"),
            },
            Err(e) => Response::error(id, "not_found", &e),
        },
        None => {
            let all: Vec<_> = goals.values().map(GoalRun::to_json).collect();
            Response::success(id, serde_json::json!({ "goals": all }))
        }
    }
}

pub(super) fn handle_goal_complete(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match resolve_target(params, state) {
        Ok(v) => v,
        Err(e) => return Response::error(id, "not_found", &e),
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
        "blocked" => GoalStatus::Blocked("the agent could not finish".into()),
        _ => {
            return Response::error(
                id,
                "invalid_params",
                &format!(
                    "No usable status: iteration file {} {} — write it with front-matter status done|blocked",
                    run.output_rel(run.iteration),
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
            "Goal finished",
            &format!("'{}' is done — read {}", run.goal_name, run.output_rel(run.iteration)),
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
            "goal": run.goal_name,
            "status": if is_done { "done" } else { "blocked" },
            "iteration": run.iteration,
        }),
    )
}

/// A steering note is pasted into the next-iteration prompt, which is typed
/// into the pane as ONE line — control characters (a bare newline submits the
/// prompt early) must never reach it.
fn sanitize_note(note: &str) -> String {
    let flat: String = note
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    flat.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(500)
        .collect()
}

/// Human verdict: run another iteration, seeded with the (possibly
/// human-edited) feedback section of the current iteration file. An optional
/// `note` steers it without editing the file.
pub(super) fn handle_goal_continue(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match resolve_target(params, state) {
        Ok(v) => v,
        Err(e) => return Response::error(id, "not_found", &e),
    };
    let note = params
        .get("note")
        .and_then(|v| v.as_str())
        .map(sanitize_note)
        .unwrap_or_default();
    let reason = if note.is_empty() {
        "The reviewer asked for another iteration.".to_string()
    } else {
        format!("The reviewer asked for another iteration, with this note: {note}")
    };
    match goal::advance_iteration(state, ws_id, &reason) {
        Ok(next) => {
            // Graph nodes in Review go back to Running.
            goal::graph::continue_node(state, ws_id);
            let name = lock_or_recover(&state.goals)
                .get(&ws_id)
                .map(|r| r.goal_name.clone())
                .unwrap_or_default();
            Response::success(
                id,
                serde_json::json!({
                    "workspace_id": ws_id.to_string(),
                    "goal": name,
                    "iteration": next,
                }),
            )
        }
        Err(e) => Response::error(id, "not_found", &e),
    }
}

/// Human verdict: accept the current iteration as final — mark the run Done
/// (the graph scheduler then merges/unblocks dependents).
pub(super) fn handle_goal_accept(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match resolve_target(params, state) {
        Ok(v) => v,
        Err(e) => return Response::error(id, "not_found", &e),
    };
    let mut goals = lock_or_recover(&state.goals);
    let Some(run) = goals.get_mut(&ws_id) else {
        return Response::error(id, "not_found", "No goal registered for that workspace");
    };
    run.status = GoalStatus::Done;
    let iteration = run.iteration;
    let name = run.goal_name.clone();
    drop(goals);
    // Graph nodes: finish immediately (merge + unblock dependents).
    if let Err(e) = goal::graph::accept_node(state, ws_id) {
        return Response::error(id, "internal", &e);
    }
    Response::success(
        id,
        serde_json::json!({
            "workspace_id": ws_id.to_string(),
            "goal": name,
            "iteration": iteration,
            "status": "done",
        }),
    )
}

/// Human stop: the driver stops driving this run. The workspace stays open.
pub(super) fn handle_goal_stop(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match resolve_target(params, state) {
        Ok(v) => v,
        Err(e) => return Response::error(id, "not_found", &e),
    };
    match goal::stop_run(state, ws_id) {
        Ok(v) => Response::success(id, v),
        Err(e) => Response::error(id, "not_found", &e),
    }
}
