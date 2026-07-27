//! Goal-driven agent workspaces — the `jmux goal` primitive.
//!
//! A goal run launches a master agent (via a configurable *runner*: agent CLI
//! + model + effort) in a fresh workspace seeded with the goal file plus an
//! operating-contract guidance template. Completion is **file-based**: the
//! agent writes `docs/roadmap/iteration-N.md` with front-matter
//! `status: done|blocked`, which the driver ticker polls for; the
//! `jmux goal complete` socket call is only a fast-path notification. See
//! docs/roadmap/DESIGN-goal-graph.md for the full design.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::app::{lock_or_recover, AppState, SharedState};
use crate::model::claude_state::{classify, has_selection_menu, ClaudeState};
use crate::settings::GoalRunner;

/// Driver tick interval (same cadence as the sub-agent monitor).
const TICK_MS: u64 = 2000;

/// Minimum seconds between escalation notifications for one goal.
const ESCALATION_MIN_SECS: u64 = 60;

/// What the driver types into an idle master to keep it working. Sent with a
/// trailing carriage return only after a live claude process is verified on
/// the panel — an idle *shell* must never receive this.
const NUDGE_TEXT: &str = "Continue working toward the goal. When it is complete (or you cannot proceed), write the iteration file exactly as instructed, then run: jmux goal complete";

const DEFAULT_GUIDANCE: &str = include_str!("guidance_default.md");

/// Lifecycle state of a goal run.
#[derive(Debug, Clone, PartialEq)]
pub enum GoalStatus {
    Running,
    /// Still running, but the driver has given up nudging (budget exhausted
    /// or a selection menu is blocking) and has escalated to the human.
    NeedsAttention(String),
    Done,
    Blocked(String),
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Running => "running",
            GoalStatus::NeedsAttention(_) => "needs-attention",
            GoalStatus::Done => "done",
            GoalStatus::Blocked(_) => "blocked",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            GoalStatus::NeedsAttention(d) | GoalStatus::Blocked(d) => Some(d),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, GoalStatus::Done | GoalStatus::Blocked(_))
    }
}

/// One registered goal run. Lives in `SharedState::goals`, keyed by
/// workspace id; mutated by the driver ticker (GTK main loop) and the
/// `goal.*` socket handlers (tokio blocking threads).
#[derive(Debug, Clone)]
pub struct GoalRun {
    pub workspace_id: Uuid,
    pub panel_id: Uuid,
    pub session_id: String,
    pub goal_name: String,
    pub goal_path: String,
    pub cwd: String,
    pub iteration: u32,
    pub max_iterations: u32,
    pub runner_name: String,
    pub runner: GoalRunner,
    pub status: GoalStatus,
    pub nudges_sent: u32,
    pub idle_ticks: u32,
    pub started_epoch: u64,
    pub wall_clock_minutes: u32,
    pub last_escalation_epoch: u64,
}

impl GoalRun {
    /// Repo-relative iteration file path for iteration `n`.
    pub fn output_rel(n: u32) -> String {
        format!("docs/roadmap/iteration-{n}.md")
    }

    pub fn output_abs(&self, n: u32) -> PathBuf {
        Path::new(&self.cwd).join(Self::output_rel(n))
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "workspace_id": self.workspace_id.to_string(),
            "panel_id": self.panel_id.to_string(),
            "session_id": self.session_id,
            "goal": self.goal_name,
            "goal_path": self.goal_path,
            "cwd": self.cwd,
            "iteration": self.iteration,
            "max_iterations": self.max_iterations,
            "runner": self.runner_name,
            "status": self.status.as_str(),
            "detail": self.status.detail(),
            "output": Self::output_rel(self.iteration),
            "nudges_sent": self.nudges_sent,
            "started_epoch": self.started_epoch,
        })
    }
}

pub fn epoch_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// POSIX single-quote shell escaping.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The guidance template: user override at
/// `~/.config/jmux/goal-guidance.md`, else the shipped default.
fn guidance_template() -> String {
    if let Some(cfg) = dirs::config_dir() {
        let p = cfg.join("jmux/goal-guidance.md");
        if let Ok(s) = std::fs::read_to_string(&p) {
            return s;
        }
    }
    DEFAULT_GUIDANCE.to_string()
}

/// Compose the master agent's seed prompt for one iteration.
pub fn compose_seed(
    goal_name: &str,
    goal_text: &str,
    iteration: u32,
    feedback_ref: Option<&str>,
) -> String {
    let feedback = match feedback_ref {
        Some(rel) => format!(
            "\nA previous iteration exists: read `{rel}` (especially section 4, \
             Feedback) and address it before anything else.\n"
        ),
        None => String::new(),
    };
    guidance_template()
        .replace("{goal_name}", goal_name)
        .replace("{iteration}", &iteration.to_string())
        .replace("{output_path}", &GoalRun::output_rel(iteration))
        .replace("{feedback}", &feedback)
        .replace("{goal_text}", goal_text)
}

/// Build the launch command for a runner. `seed_file` always contains the
/// seed on disk (audit trail + `{seed_file}` templates); claude runners get
/// the seed inline as a quoted argv.
pub fn launch_command(
    runner: &GoalRunner,
    session_id: &str,
    seed: &str,
    seed_file: &Path,
    permission_mode: &str,
) -> String {
    if runner.agent == "custom" && !runner.command_template.is_empty() {
        return runner
            .command_template
            .replace("{sid}", session_id)
            .replace("{model}", &runner.model)
            .replace("{effort}", &runner.effort)
            .replace("{seed_file}", &shell_quote(&seed_file.to_string_lossy()))
            .replace("{seed}", &shell_quote(seed));
    }
    // claude adapter
    let mut parts: Vec<String> = vec![
        "claude".into(),
        "--session-id".into(),
        session_id.into(),
    ];
    if !runner.model.is_empty() {
        parts.push("--model".into());
        parts.push(shell_quote(&runner.model));
    }
    if !runner.effort.is_empty() {
        parts.push("--effort".into());
        parts.push(shell_quote(&runner.effort));
    }
    // "supervised" = stock interactive prompting (no flag).
    if !permission_mode.is_empty() && permission_mode != "supervised" {
        parts.push("--permission-mode".into());
        parts.push(shell_quote(permission_mode));
    }
    for a in &runner.extra_args {
        parts.push(shell_quote(a));
    }
    parts.push(shell_quote(seed));
    parts.join(" ")
}

/// Whether ClaudeState screen classification (and nudging) applies to this
/// runner. Custom runners are poll-only unless they opt in.
pub fn state_detection_is_claude(runner: &GoalRunner) -> bool {
    match runner.state_detection.as_str() {
        "claude" => true,
        "none" => false,
        _ => runner.agent != "custom",
    }
}

/// Directory where seed prompts are kept (out of the repo):
/// `~/.local/share/jmux/goal-seeds/<workspace>/`.
pub fn seed_dir(workspace_id: Uuid) -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("jmux/goal-seeds")
        .join(workspace_id.to_string())
}

/// Next iteration number: one past the highest existing
/// `iteration-<n>.md` in `<cwd>/docs/roadmap/`.
pub fn next_iteration_number(cwd: &str) -> u32 {
    let dir = Path::new(cwd).join("docs/roadmap");
    let mut max = 0u32;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(n) = name
                .strip_prefix("iteration-")
                .and_then(|s| s.strip_suffix(".md"))
                .and_then(|s| s.parse::<u32>().ok())
            {
                max = max.max(n);
            }
        }
    }
    max + 1
}

/// Parsed front matter of an iteration file.
pub struct IterationOutcome {
    pub status: String,
}

/// Parse `status:` out of an iteration file's front matter. Returns `None`
/// when the file is missing or has no front-matter status line (an existing
/// file without one is a protocol violation the caller escalates).
pub fn parse_iteration_file(path: &Path) -> Option<Option<IterationOutcome>> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Some(None);
    }
    for line in lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(v) = t.strip_prefix("status:") {
            return Some(Some(IterationOutcome {
                status: v.trim().to_ascii_lowercase(),
            }));
        }
    }
    Some(None)
}

/// Install the goal auto-driver on the GTK main loop. Cheap when no goals
/// are registered (one map check per tick).
pub fn start_driver(state: Rc<AppState>) {
    glib::timeout_add_local(std::time::Duration::from_millis(TICK_MS), move || {
        tick(&state);
        glib::ControlFlow::Continue
    });
}

/// One driver pass over every non-terminal goal run.
fn tick(state: &Rc<AppState>) {
    let goal_ids: Vec<Uuid> = {
        let goals = lock_or_recover(&state.shared.goals);
        goals
            .iter()
            .filter(|(_, run)| !run.status.is_terminal())
            .map(|(id, _)| *id)
            .collect()
    };
    if goal_ids.is_empty() {
        return;
    }
    let settings = crate::settings::load().goal;
    for ws_id in goal_ids {
        drive_one(state, ws_id, &settings);
    }
}

/// Advance one goal run by one tick.
fn drive_one(state: &Rc<AppState>, ws_id: Uuid, settings: &crate::settings::GoalSettings) {
    let Some(run) = lock_or_recover(&state.shared.goals).get(&ws_id).cloned() else {
        return;
    };

    // Workspace closed by the user → drop the run silently.
    {
        let tm = lock_or_recover(&state.shared.tab_manager);
        if tm.workspace(ws_id).is_none() {
            drop(tm);
            lock_or_recover(&state.shared.goals).remove(&ws_id);
            return;
        }
    }

    // 1) File check — the source of truth.
    match parse_iteration_file(&run.output_abs(run.iteration)) {
        Some(Some(outcome)) => {
            on_iteration_file(state, run, outcome, settings);
            return;
        }
        Some(None) => {
            // File exists but has no parseable front-matter status: protocol
            // violation. Escalate rather than hang.
            escalate(
                state,
                ws_id,
                GoalStatus::NeedsAttention("iteration file missing front-matter status".into()),
                &format!(
                    "goal '{}': {} exists but has no `status:` front matter",
                    run.goal_name,
                    GoalRun::output_rel(run.iteration)
                ),
            );
            return;
        }
        None => {}
    }

    // 2) Wall-clock cap.
    if run.wall_clock_minutes > 0
        && epoch_now().saturating_sub(run.started_epoch) > u64::from(run.wall_clock_minutes) * 60
    {
        escalate(
            state,
            ws_id,
            GoalStatus::Blocked("timeout".into()),
            &format!(
                "goal '{}' hit its {}-minute wall clock without an iteration file",
                run.goal_name, run.wall_clock_minutes
            ),
        );
        return;
    }

    // 3) Screen-state driving (claude runners only; others are poll-only).
    if !state_detection_is_claude(&run.runner) {
        return;
    }
    if state.shared.is_hibernated(&run.panel_id) {
        return;
    }
    let (text, raw_title) = {
        let text = state
            .terminal_cache
            .borrow()
            .get(&run.panel_id)
            .and_then(|s| s.read_screen_text());
        let Some(text) = text else {
            // Surface not spawned/ready yet — don't count this as idle.
            return;
        };
        let title = {
            let tm = lock_or_recover(&state.shared.tab_manager);
            tm.workspace(ws_id)
                .and_then(|ws| ws.panel(run.panel_id))
                .and_then(|p| p.title.clone())
                .unwrap_or_default()
        };
        (text, title)
    };

    // A hard selection menu (permission prompt, model picker…) blocks the
    // turn; nudging types into the menu. Escalate instead.
    if has_selection_menu(&text) {
        escalate(
            state,
            ws_id,
            GoalStatus::NeedsAttention("selection menu on screen".into()),
            &format!(
                "goal '{}' is waiting on a selection menu — open the workspace to answer it",
                run.goal_name
            ),
        );
        return;
    }

    match classify(&text, &raw_title) {
        Some(ClaudeState::Working) | Some(ClaudeState::Waiting) => {
            set_run(state, ws_id, |r| {
                r.idle_ticks = 0;
                // A turn is visibly running again — clear a stale escalation.
                if matches!(r.status, GoalStatus::NeedsAttention(_)) {
                    r.status = GoalStatus::Running;
                }
            });
        }
        // Idle, or the soft "last response ends in ?" heuristic — for an
        // autonomous run both mean the master stopped; count toward a nudge.
        Some(ClaudeState::NeedsInput) | None => {
            let idle = run.idle_ticks + 1;
            if idle < settings.idle_ticks_before_nudge {
                set_run(state, ws_id, |r| r.idle_ticks = idle);
                return;
            }
            // Debounce passed. Verify a live claude process before typing
            // anything — an idle shell would *execute* the nudge.
            let live = crate::session::claude_resume::all_local_claude_cwds();
            if !live.contains_key(&run.panel_id) {
                escalate(
                    state,
                    ws_id,
                    GoalStatus::Blocked("agent process exited".into()),
                    &format!(
                        "goal '{}': the agent process exited without writing {}",
                        run.goal_name,
                        GoalRun::output_rel(run.iteration)
                    ),
                );
                return;
            }
            if run.nudges_sent >= settings.nudge_budget {
                escalate(
                    state,
                    ws_id,
                    GoalStatus::NeedsAttention(format!(
                        "idle after {} nudges",
                        run.nudges_sent
                    )),
                    &format!(
                        "goal '{}' is idle after {} nudges — it may be stuck",
                        run.goal_name, run.nudges_sent
                    ),
                );
                return;
            }
            let sent = state.send_input_to_panel(run.panel_id, &format!("{NUDGE_TEXT}\r"));
            if sent {
                tracing::info!(goal = %run.goal_name, nudge = run.nudges_sent + 1, "goal driver nudge");
                set_run(state, ws_id, |r| {
                    r.nudges_sent += 1;
                    r.idle_ticks = 0;
                });
            }
        }
    }
}

/// Handle a parsed iteration file: finish, iterate, or escalate.
fn on_iteration_file(
    state: &Rc<AppState>,
    run: GoalRun,
    outcome: IterationOutcome,
    _settings: &crate::settings::GoalSettings,
) {
    let ws_id = run.workspace_id;
    match outcome.status.as_str() {
        "done" => {
            set_run(state, ws_id, |r| r.status = GoalStatus::Done);
            notify(
                state,
                ws_id,
                "Goal complete",
                &format!(
                    "'{}' finished: {}",
                    run.goal_name,
                    GoalRun::output_rel(run.iteration)
                ),
                true,
            );
        }
        "blocked" if run.iteration < run.started_iteration_cap() => {
            // Feed the feedback forward and start the next iteration
            // in-session. (Fresh-session relaunch lands with headless spawn —
            // see the design doc.)
            let next = run.iteration + 1;
            let prev_rel = GoalRun::output_rel(run.iteration);
            let next_rel = GoalRun::output_rel(next);
            let prompt = format!(
                "Iteration {} reported status: blocked. Begin iteration {}: re-read \
                 `{}` (especially section 4), address the blockers with reasonable \
                 assumptions, continue working the goal, and when finished write \
                 `{}` in the same four-section format with front-matter status \
                 done or blocked. Then run: jmux goal complete",
                run.iteration, next, prev_rel, next_rel
            );
            let sent = state.send_input_to_panel(run.panel_id, &format!("{prompt}\r"));
            if sent {
                set_run(state, ws_id, |r| {
                    r.iteration = next;
                    r.idle_ticks = 0;
                    r.nudges_sent = 0;
                    r.status = GoalStatus::Running;
                });
                notify(
                    state,
                    ws_id,
                    "Goal iterating",
                    &format!("'{}' starting iteration {next}", run.goal_name),
                    false,
                );
            }
        }
        "blocked" => {
            escalate(
                state,
                ws_id,
                GoalStatus::Blocked("agent reported blocked".into()),
                &format!(
                    "goal '{}' is blocked — see {} section 4",
                    run.goal_name,
                    GoalRun::output_rel(run.iteration)
                ),
            );
        }
        other => {
            escalate(
                state,
                ws_id,
                GoalStatus::NeedsAttention(format!("unknown status '{other}'")),
                &format!(
                    "goal '{}' wrote an unknown status '{other}' in {}",
                    run.goal_name,
                    GoalRun::output_rel(run.iteration)
                ),
            );
        }
    }
}

impl GoalRun {
    /// The iteration ceiling for auto-continuation.
    fn started_iteration_cap(&self) -> u32 {
        self.max_iterations.max(1)
    }
}

/// Mutate a registry entry in place (no-op if it was removed).
fn set_run(state: &Rc<AppState>, ws_id: Uuid, f: impl FnOnce(&mut GoalRun)) {
    if let Some(run) = lock_or_recover(&state.shared.goals).get_mut(&ws_id) {
        f(run);
    }
}

/// Set a status and send a (rate-limited) escalation notification.
fn escalate(state: &Rc<AppState>, ws_id: Uuid, status: GoalStatus, message: &str) {
    let now = epoch_now();
    let mut should_notify = false;
    {
        let mut goals = lock_or_recover(&state.shared.goals);
        if let Some(run) = goals.get_mut(&ws_id) {
            // Re-escalating the same state repeatedly is the flood case the
            // rate limit exists for.
            if run.status != status || now.saturating_sub(run.last_escalation_epoch) >= ESCALATION_MIN_SECS
            {
                should_notify = now.saturating_sub(run.last_escalation_epoch) >= ESCALATION_MIN_SECS;
                run.status = status;
                if should_notify {
                    run.last_escalation_epoch = now;
                }
            }
        }
    }
    if should_notify {
        notify(state, ws_id, "Goal needs attention", message, true);
    }
}

/// Add a jmux notification attached to the goal's workspace.
fn notify(state: &Rc<AppState>, ws_id: Uuid, title: &str, body: &str, desktop: bool) {
    lock_or_recover(&state.shared.notifications).add(title, body, Some(ws_id), None, desktop);
    state.shared.notify_metadata_refresh();
}

/// Register a run (socket-thread entry point).
pub fn register(shared: &Arc<SharedState>, run: GoalRun) {
    lock_or_recover(&shared.goals).insert(run.workspace_id, run);
}

pub type GoalRegistry = HashMap<Uuid, GoalRun>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
        assert_eq!(shell_quote("plain"), "'plain'");
    }

    #[test]
    fn compose_seed_replaces_placeholders() {
        let seed = compose_seed("mapsite", "Build a map.", 2, Some("docs/roadmap/iteration-1.md"));
        assert!(seed.contains("mapsite"));
        assert!(seed.contains("Build a map."));
        assert!(seed.contains("docs/roadmap/iteration-2.md"));
        assert!(seed.contains("iteration-1.md"));
        assert!(!seed.contains("{goal_text}"));
        assert!(!seed.contains("{output_path}"));
    }

    #[test]
    fn launch_command_claude_defaults() {
        let runner = GoalRunner::default();
        let cmd = launch_command(&runner, "abc", "do it", Path::new("/tmp/s.md"), "acceptEdits");
        assert!(cmd.starts_with("claude --session-id abc"));
        assert!(cmd.contains("--permission-mode 'acceptEdits'"));
        assert!(cmd.ends_with("'do it'"));
        assert!(!cmd.contains("--model"));
    }

    #[test]
    fn launch_command_custom_template() {
        let runner = GoalRunner {
            agent: "custom".into(),
            model: "gpt-sol".into(),
            command_template: "codex exec --model {model} {seed_file}".into(),
            ..Default::default()
        };
        let cmd = launch_command(&runner, "abc", "seed", Path::new("/tmp/s.md"), "acceptEdits");
        assert_eq!(cmd, "codex exec --model gpt-sol '/tmp/s.md'");
    }

    #[test]
    fn parse_iteration_front_matter() {
        let dir = std::env::temp_dir().join(format!("jmux-goal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("iteration-1.md");
        std::fs::write(&p, "---\nstatus: Done\niteration: 1\n---\n\n## 1. Goal\n").unwrap();
        let outcome = parse_iteration_file(&p).unwrap().unwrap();
        assert_eq!(outcome.status, "done");
        std::fs::write(&p, "no front matter").unwrap();
        assert!(parse_iteration_file(&p).unwrap().is_none());
        assert!(parse_iteration_file(&dir.join("missing.md")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_iteration_scans_existing() {
        let dir = std::env::temp_dir().join(format!("jmux-goal-scan-{}", std::process::id()));
        let roadmap = dir.join("docs/roadmap");
        std::fs::create_dir_all(&roadmap).unwrap();
        assert_eq!(next_iteration_number(dir.to_str().unwrap()), 1);
        std::fs::write(roadmap.join("iteration-3.md"), "x").unwrap();
        assert_eq!(next_iteration_number(dir.to_str().unwrap()), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
