//! Goal-driven agent workspaces — the `jmux goal` primitive.
//!
//! A goal run launches a master agent (via a configurable *runner*: agent CLI
//! + model + effort) in a fresh workspace seeded with the goal file plus an
//! operating-contract guidance template. Completion is **file-based**: the
//! agent writes `<goal.output_dir>/iteration-N.md` (default
//! `docs/roadmap/…`) with front-matter
//! `status: done|blocked`, which the driver ticker polls for; the
//! `jmux goal complete` socket call is only a fast-path notification. See
//! docs/roadmap/DESIGN-goal-graph.md for the full design.

pub mod graph;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::app::{lock_or_recover, AppState, SharedState};
use crate::model::claude_state::{classify, has_selection_menu, ClaudeState};
use crate::model::Workspace;
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

/// Ties a goal run to the graph node it executes.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphLink {
    pub graph: String,
    pub node: String,
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
    /// Repo-relative directory holding this run's iteration files: the
    /// configured `goal.output_dir` for bare goals, `<output_dir>/<graph>/
    /// <node>` for graph nodes.
    pub output_dir_rel: String,
    pub iteration: u32,
    pub max_iterations: u32,
    pub runner_name: String,
    pub runner: GoalRunner,
    pub status: GoalStatus,
    pub nudges_sent: u32,
    pub idle_ticks: u32,
    /// Consecutive ticks with no readable screen text while Running — the
    /// spawn watchdog (surfaces only spawn once mapped + visible).
    pub no_text_ticks: u32,
    pub started_epoch: u64,
    pub wall_clock_minutes: u32,
    pub last_escalation_epoch: u64,
    /// Set when this run executes a graph node.
    pub graph: Option<GraphLink>,
}

/// Repo-relative iteration file path for iteration `n` under `dir_rel`.
pub fn iteration_rel(dir_rel: &str, n: u32) -> String {
    format!("{dir_rel}/iteration-{n}.md")
}

/// Validate `goal.output_dir`: it names a directory INSIDE the repo, so an
/// absolute path or a `..` component is a configuration error, not a path to
/// normalize. Returns the cleaned relative path.
pub fn validate_output_dir(dir: &str) -> Result<String, String> {
    let dir = dir.trim();
    if dir.is_empty() {
        return Err("output_dir is empty".into());
    }
    let mut parts: Vec<&str> = Vec::new();
    for component in Path::new(dir).components() {
        match component {
            std::path::Component::Normal(s) => {
                parts.push(s.to_str().ok_or("output_dir is not valid UTF-8")?)
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(format!(
                    "output_dir '{dir}' must be a repo-relative path without '..'"
                ))
            }
        }
    }
    if parts.is_empty() {
        return Err(format!("output_dir '{dir}' names no directory"));
    }
    Ok(parts.join("/"))
}

/// The configured output directory, falling back to the default when the
/// setting is unusable — a bad setting must not break every launch.
pub fn output_dir_rel(settings: &crate::settings::GoalSettings) -> String {
    match validate_output_dir(&settings.output_dir) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("ignoring goal.output_dir: {e}");
            crate::settings::DEFAULT_GOAL_OUTPUT_DIR.to_string()
        }
    }
}

/// Permission modes jmux accepts. `supervised` is jmux's own name for stock
/// prompting (no `--permission-mode` flag at all); the rest are the claude
/// CLI's own `--permission-mode` choices.
pub const PERMISSION_MODES: &[&str] = &[
    "supervised",
    "acceptEdits",
    "plan",
    "auto",
    "manual",
    "dontAsk",
    "bypassPermissions",
];

/// Validate a permission mode. `allow_bypass` is true only for the
/// per-invocation request (`--full-auto`): every ambient source (settings,
/// runner config) is refused bypass, because edge payloads feed upstream
/// agent output into downstream prompts and a configured bypass would apply
/// to runs the user never opted in for. See DESIGN-goal-graph.md §Permissions.
pub fn validate_permission_mode(mode: &str, allow_bypass: bool) -> Result<String, String> {
    let mode = mode.trim();
    if !PERMISSION_MODES.contains(&mode) {
        return Err(format!(
            "unknown permission mode '{mode}' (expected one of {})",
            PERMISSION_MODES.join(", ")
        ));
    }
    if mode == "bypassPermissions" && !allow_bypass {
        return Err(
            "bypassPermissions is a per-invocation opt-in (`--full-auto`), never a configured \
             default"
                .into(),
        );
    }
    Ok(mode.to_string())
}

/// Resolve the permission mode for a launch. Precedence:
/// per-invocation request (`--full-auto`/`--supervised`, or an explicit
/// `permission_mode` socket param) > runner `permission_mode` >
/// `goal.permission_mode` > `acceptEdits`. A source that doesn't validate is
/// logged and skipped rather than aborting the launch — a bad setting must
/// not break every run, and the warning says which source was ignored.
pub fn resolve_permission_mode(
    requested: Option<&str>,
    runner: &GoalRunner,
    settings: &crate::settings::GoalSettings,
) -> String {
    let sources = [
        ("permission_mode request", requested.unwrap_or(""), true),
        ("runner permission_mode", runner.permission_mode.as_str(), false),
        ("goal.permission_mode", settings.permission_mode.as_str(), false),
    ];
    for (origin, value, allow_bypass) in sources {
        if value.trim().is_empty() {
            continue;
        }
        match validate_permission_mode(value, allow_bypass) {
            Ok(mode) => return mode,
            Err(e) => tracing::warn!("ignoring {origin}: {e}"),
        }
    }
    crate::settings::DEFAULT_GOAL_PERMISSION_MODE.to_string()
}

impl GoalRun {
    /// Repo-relative iteration file path for iteration `n` of this run.
    pub fn output_rel(&self, n: u32) -> String {
        iteration_rel(&self.output_dir_rel, n)
    }

    pub fn output_abs(&self, n: u32) -> PathBuf {
        Path::new(&self.cwd).join(self.output_rel(n))
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
            "output": self.output_rel(self.iteration),
            "nudges_sent": self.nudges_sent,
            "started_epoch": self.started_epoch,
            "graph": self.graph.as_ref().map(|g| g.graph.clone()),
            "node": self.graph.as_ref().map(|g| g.node.clone()),
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
/// `upstream_refs` are repo-relative iteration files of completed upstream
/// graph nodes, passed by reference (never inlined) to keep context small.
pub fn compose_seed(
    goal_name: &str,
    goal_text: &str,
    iteration: u32,
    output_dir_rel: &str,
    feedback_ref: Option<&str>,
    upstream_refs: &[String],
) -> String {
    let feedback = match feedback_ref {
        Some(rel) => format!(
            "\nA previous iteration exists: read `{rel}` (especially section 4, \
             Feedback) and address it before anything else.\n"
        ),
        None => String::new(),
    };
    let upstream = if upstream_refs.is_empty() {
        String::new()
    } else {
        let list = upstream_refs
            .iter()
            .map(|r| format!("- `{r}`"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\nThis goal depends on completed upstream work. Read these iteration \
             reports first (sections 3 and 4 carry the hand-off):\n{list}\n"
        )
    };
    guidance_template()
        .replace("{goal_name}", goal_name)
        .replace("{iteration}", &iteration.to_string())
        .replace("{output_path}", &iteration_rel(output_dir_rel, iteration))
        .replace("{feedback}", &feedback)
        .replace("{upstream}", &upstream)
        .replace("{goal_text}", goal_text)
}

/// Build the launch command for a runner. The seed is NEVER inlined into
/// the command string: panel commands must be a single line (the app
/// rejects commands containing control characters and falls back to a
/// plain shell — see `terminal_surface_for`), and the seed is a multi-line
/// document. Instead the command reads `seed_file` at spawn time via
/// `"$(cat '<seed_file>')"`.
pub fn launch_command(
    runner: &GoalRunner,
    session_id: &str,
    _seed: &str,
    seed_file: &Path,
    permission_mode: &str,
) -> String {
    let seed_arg = format!("\"$(cat {})\"", shell_quote(&seed_file.to_string_lossy()));
    if runner.agent == "custom" && !runner.command_template.is_empty() {
        return runner
            .command_template
            .replace("{sid}", session_id)
            .replace("{model}", &runner.model)
            .replace("{effort}", &runner.effort)
            .replace("{seed_file}", &shell_quote(&seed_file.to_string_lossy()))
            .replace("{seed}", &seed_arg);
    }
    // claude adapter
    let mut parts: Vec<String> = vec!["claude".into()];
    // Pre-approved tools (Claude Code's `--allowedTools`, verified against
    // `claude --help`: "Comma or space-separated list of tool names to
    // allow"). The flag is variadic — it consumes every following argument
    // up to the next flag — so it is emitted FIRST, terminated by
    // `--session-id`, which is always present. It must never sit in front of
    // the trailing seed argument, or the seed would be parsed as a tool
    // pattern instead of the prompt.
    let allowed: Vec<&String> = runner
        .allowed_tools
        .iter()
        .filter(|t| !t.trim().is_empty())
        .collect();
    if !allowed.is_empty() {
        parts.push("--allowedTools".into());
        parts.extend(allowed.into_iter().map(|t| shell_quote(t.trim())));
    }
    parts.push("--session-id".into());
    parts.push(session_id.into());
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
    parts.push(seed_arg);
    parts.join(" ")
}

/// Walk up from `start` to the nearest directory containing `.git`.
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join(".git").exists() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// Resolve a runner by configured name. Empty name = stock claude (or the
/// settings default when one is configured).
pub fn resolve_runner_by_name(
    name: &str,
    settings: &crate::settings::GoalSettings,
) -> Result<(String, GoalRunner), String> {
    let name = if name.is_empty() {
        settings.default_runner.as_str()
    } else {
        name
    };
    if name.is_empty() {
        return Ok(("claude".to_string(), GoalRunner::default()));
    }
    settings
        .runners
        .get(name)
        .cloned()
        .map(|r| (name.to_string(), r))
        .ok_or_else(|| format!("unknown runner '{name}' (configure it in settings goal.runners)"))
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
    data_dir().join("goal-seeds").join(workspace_id.to_string())
}

/// jmux's own data directory (`~/.local/share/jmux`).
fn data_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("jmux")
}

/// Where goal files written from inline text live. Never the user's repo —
/// `jmux goal "text"` must not add a file to the tree the agent then commits.
pub fn goal_text_dir() -> PathBuf {
    data_dir().join("goal-texts")
}

/// Largest goal document accepted (file or inline text).
const MAX_GOAL_BYTES: usize = 256 * 1024;

/// A run name derived from free goal text: the first few words, lowercase,
/// hyphen-joined. Always a valid graph/node id.
pub fn slugify_goal(text: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for raw in text.split_whitespace() {
        let word: String = raw
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if word.is_empty() {
            continue;
        }
        words.push(word);
        if words.len() == 6 {
            break;
        }
    }
    let mut slug = words.join("-");
    // ASCII-only by construction, so truncating on a byte index is safe.
    if slug.len() > 48 {
        slug.truncate(48);
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "goal".to_string()
    } else {
        slug.to_string()
    }
}

/// Where a launch request's goal came from: an existing markdown file, or
/// inline text (which jmux writes into its own data dir).
#[derive(Debug)]
pub struct GoalSource {
    /// Absolute goal file path — display origin, and what a graph records.
    pub path: PathBuf,
    pub text: String,
    /// Working directory for the run (a git root when derived).
    pub cwd: PathBuf,
    /// Name derived from the source: file stem, or a slug of the text.
    pub name: String,
    /// Slugified directory name of `cwd` — the graph-flavoured default name.
    pub repo_name: String,
    pub inline: bool,
}

/// Resolve the goal of a `goal.create` / `graph.create` request. Accepts
/// either `goal` (absolute path to a markdown file) or `goal_text` (literal
/// goal text); `cwd` overrides the working directory, `client_cwd` is the
/// caller's directory that inline text derives its git root from.
/// Err is `(socket error code, message)`.
pub fn resolve_goal_source(
    params: &serde_json::Value,
) -> Result<GoalSource, (&'static str, String)> {
    let str_param = |key: &str| {
        params
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let explicit_cwd = str_param("cwd").map(PathBuf::from);
    let client_cwd = str_param("client_cwd").map(PathBuf::from);

    let (path, text, cwd, name, inline) = match str_param("goal_text") {
        Some(text) => {
            if text.len() > MAX_GOAL_BYTES {
                return Err(("invalid_params", "Goal text is too large (256 KiB max)".into()));
            }
            let cwd = explicit_cwd
                .clone()
                .or_else(|| {
                    client_cwd
                        .clone()
                        .map(|c| find_git_root(&c).unwrap_or(c))
                })
                .ok_or((
                    "invalid_params",
                    "Inline goal text needs a working directory ('cwd' or 'client_cwd')"
                        .to_string(),
                ))?;
            let name = slugify_goal(text);
            let path = write_inline_goal(&name, text)
                .map_err(|e| ("internal", e))?;
            (path, text.to_string(), cwd, name, true)
        }
        None => {
            let Some(goal) = str_param("goal") else {
                return Err((
                    "invalid_params",
                    "Missing 'goal' (a goal .md file) or 'goal_text'".into(),
                ));
            };
            let goal_path = Path::new(goal);
            if !goal_path.is_absolute() {
                return Err(("invalid_params", "'goal' must be an absolute path".into()));
            }
            let text = std::fs::read_to_string(goal_path)
                .map_err(|e| ("not_found", format!("Cannot read goal file: {e}")))?;
            if text.len() > MAX_GOAL_BYTES {
                return Err(("invalid_params", "Goal file is too large (256 KiB max)".into()));
            }
            let parent = goal_path.parent().unwrap_or(Path::new("/"));
            let cwd = explicit_cwd.clone().unwrap_or_else(|| {
                find_git_root(parent).unwrap_or_else(|| parent.to_path_buf())
            });
            // File stem, or the parent directory when the stem is the
            // generic "goal".
            let stem = goal_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("goal")
                .to_string();
            let name = if stem == "goal" {
                goal_path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
                    .unwrap_or(stem)
            } else {
                stem
            };
            (goal_path.to_path_buf(), text, cwd, name, false)
        }
    };
    if !cwd.is_dir() {
        return Err(("invalid_params", "Resolved cwd is not a directory".into()));
    }
    let repo_name = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .map(slugify_goal)
        .unwrap_or_else(|| slugify_goal(&name));
    Ok(GoalSource {
        path,
        text,
        cwd,
        name,
        repo_name,
        inline,
    })
}

/// Write inline goal text to jmux's data dir; returns the file path.
fn write_inline_goal(slug: &str, text: &str) -> Result<PathBuf, String> {
    let dir = goal_text_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {dir:?}: {e}"))?;
    let path = dir.join(format!("{slug}-{}.md", epoch_now()));
    std::fs::write(&path, format!("{}\n", text.trim_end()))
        .map_err(|e| format!("cannot write goal file: {e}"))?;
    Ok(path)
}

/// Resolve a run reference to a workspace id. `target` is a workspace UUID
/// (scripts) or a goal name — `<graph>/<node>` for graph nodes, and a bare
/// node id when it is unambiguous. With no target the caller's own workspace
/// wins (`hint`), else the sole non-terminal run.
pub fn resolve_run(
    goals: &GoalRegistry,
    target: Option<&str>,
    hint: Option<Uuid>,
) -> Result<Uuid, String> {
    if let Some(t) = target.map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(id) = Uuid::parse_str(t) {
            return if goals.contains_key(&id) {
                Ok(id)
            } else {
                Err(format!("no goal run for workspace {id}"))
            };
        }
        let mut hits: Vec<(&Uuid, &GoalRun)> =
            goals.iter().filter(|(_, r)| r.goal_name == t).collect();
        if hits.is_empty() {
            hits = goals
                .iter()
                .filter(|(_, r)| r.goal_name.rsplit('/').next() == Some(t))
                .collect();
        }
        // A relaunched name matches its finished predecessor too — the live
        // run is what the human means.
        if hits.len() > 1 {
            let live: Vec<(&Uuid, &GoalRun)> = hits
                .iter()
                .copied()
                .filter(|(_, r)| !r.status.is_terminal())
                .collect();
            if live.len() == 1 {
                hits = live;
            }
        }
        return match hits.len() {
            0 => Err(format!(
                "no goal run named '{t}'{}",
                candidate_list(goals.values())
            )),
            1 => Ok(*hits[0].0),
            _ => Err(format!(
                "'{t}' matches several runs — name one exactly:{}",
                candidate_list(hits.iter().map(|(_, r)| *r))
            )),
        };
    }
    if let Some(h) = hint {
        if goals.contains_key(&h) {
            return Ok(h);
        }
    }
    let live: Vec<(&Uuid, &GoalRun)> = goals
        .iter()
        .filter(|(_, r)| !r.status.is_terminal())
        .collect();
    match live.len() {
        0 => Err("no active goal runs (jmux goal status lists everything)".into()),
        1 => Ok(*live[0].0),
        _ => Err(format!(
            "several goal runs are active — name one:{}",
            candidate_list(live.iter().map(|(_, r)| *r))
        )),
    }
}

/// "\n  - <name> (<status>)" per run, sorted (registry order is a HashMap's).
fn candidate_list<'a>(runs: impl Iterator<Item = &'a GoalRun>) -> String {
    let mut lines: Vec<String> = runs
        .map(|r| format!("\n  - {} ({})", r.goal_name, r.status.as_str()))
        .collect();
    lines.sort();
    lines.concat()
}

/// Human stop: mark the run Blocked so the driver stops driving it. The
/// workspace stays open on purpose — the agent's context is still there.
pub fn stop_run(shared: &Arc<SharedState>, ws_id: Uuid) -> Result<serde_json::Value, String> {
    let mut goals = lock_or_recover(&shared.goals);
    let run = goals
        .get_mut(&ws_id)
        .ok_or_else(|| "no goal registered for that workspace".to_string())?;
    run.status = GoalStatus::Blocked("stopped by user".into());
    Ok(serde_json::json!({
        "workspace_id": ws_id.to_string(),
        "goal": run.goal_name,
        "iteration": run.iteration,
        "status": "blocked",
        "detail": "stopped by user",
    }))
}

/// Next iteration number: one past the highest existing
/// `iteration-<n>.md` in `<cwd>/<dir_rel>/`.
pub fn next_iteration_number(cwd: &str, dir_rel: &str) -> u32 {
    let dir = Path::new(cwd).join(dir_rel);
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

/// One driver pass: graph scheduling first (it consumes terminal goal
/// states), then every non-terminal goal run.
fn tick(state: &Rc<AppState>) {
    graph::scheduler_tick(state);
    // Purge terminal runs whose workspace is gone — they're invisible to
    // the driver (filtered below) and to status queries by workspace, so
    // without this the registry only ever grows.
    {
        let live: std::collections::HashSet<Uuid> = {
            let tm = lock_or_recover(&state.shared.tab_manager);
            tm.iter().map(|ws| ws.id).collect()
        };
        lock_or_recover(&state.shared.goals)
            .retain(|ws_id, run| !run.status.is_terminal() || live.contains(ws_id));
    }
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
        ensure_agent_spawned(state, ws_id);
        drive_one(state, ws_id, &settings);
    }
}

/// Start a run's agent without showing its workspace.
///
/// A goal workspace launched in the background is never mapped, so its terminal
/// would never get the allocation that normally spawns the command. This starts
/// the pty + agent directly (see `AppState::spawn_panel_headless`); it is cheap
/// and idempotent once the child is running, and retries on the next tick while
/// no window is realized yet (app startup, restored session).
fn ensure_agent_spawned(state: &Rc<AppState>, ws_id: Uuid) {
    let Some(panel_id) = lock_or_recover(&state.shared.goals)
        .get(&ws_id)
        .map(|run| run.panel_id)
    else {
        return;
    };
    // A paused run stays paused: never (re)start a child the human froze.
    if state.shared.is_hibernated(&panel_id) {
        return;
    }
    if state
        .terminal_cache
        .borrow()
        .get(&panel_id)
        .is_some_and(|s| s.has_spawned())
    {
        return;
    }
    state.spawn_panel_headless(panel_id);
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
                GoalStatus::NeedsAttention("iteration report has no status line".into()),
                &format!(
                    "the agent for '{}' wrote {} without the `status: done|blocked` \
                     line jmux waits for — open its workspace",
                    run.goal_name,
                    run.output_rel(run.iteration)
                ),
            );
            return;
        }
        None => {}
    }

    // 2) Paused (hibernated) agent: the human froze it on purpose — no
    //    wall clock, no nudging, nothing until it's resumed. (Completion
    //    via the file check above still counts.)
    if state.shared.is_hibernated(&run.panel_id) {
        return;
    }

    // 3) Wall-clock cap.
    if run.wall_clock_minutes > 0
        && epoch_now().saturating_sub(run.started_epoch) > u64::from(run.wall_clock_minutes) * 60
    {
        escalate(
            state,
            ws_id,
            GoalStatus::Blocked("ran out of time".into()),
            &format!(
                "'{}' ran for {} minutes without finishing — jmux stopped driving it; \
                 open its workspace to take over",
                run.goal_name, run.wall_clock_minutes
            ),
        );
        return;
    }

    // 4) Screen-state driving (claude runners only; others are poll-only).
    if !state_detection_is_claude(&run.runner) {
        return;
    }
    let (text, raw_title) = {
        let text = state
            .terminal_cache
            .borrow()
            .get(&run.panel_id)
            .and_then(|s| s.read_screen_text());
        let Some(text) = text else {
            // No terminal to read: the run's surface does not exist. Runs are
            // spawned headlessly by `ensure_agent_spawned` (visibility is not
            // required), so this is now a genuine fault, not the normal
            // background case — but keep it as a safety net rather than a panic,
            // and give it a full minute in case a window is still coming up.
            let ticks = run.no_text_ticks + 1;
            set_run(state, ws_id, |r| r.no_text_ticks = ticks);
            if ticks == 30 {
                // The pty may be running even if the terminal is unreadable —
                // process liveness (a /proc walk keyed on JMUX_PANEL_ID) tells
                // "failed to start" apart from "started, can't read it".
                let agent_alive = crate::session::claude_resume::all_local_claude_cwds()
                    .contains_key(&run.panel_id);
                let detail = if agent_alive {
                    "running but its terminal cannot be read"
                } else {
                    "failed to start"
                };
                escalate(
                    state,
                    ws_id,
                    GoalStatus::NeedsAttention(detail.into()),
                    &format!(
                        "'{}' {detail} — jmux could not start or read its terminal, \
                         which should not happen; open its workspace and check the \
                         jmux log for the spawn error",
                        run.goal_name
                    ),
                );
            }
            return;
        };
        if run.no_text_ticks != 0 {
            set_run(state, ws_id, |r| {
                r.no_text_ticks = 0;
                if matches!(r.status, GoalStatus::NeedsAttention(_)) {
                    r.status = GoalStatus::Running;
                }
            });
        }
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
            GoalStatus::NeedsAttention("waiting on a prompt".into()),
            &format!(
                "the agent for '{}' is waiting for you to answer a prompt — \
                 open its workspace",
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
                    GoalStatus::Blocked("the agent exited".into()),
                    &format!(
                        "the agent for '{}' exited before writing its iteration \
                         report ({})",
                        run.goal_name,
                        run.output_rel(run.iteration)
                    ),
                );
                return;
            }
            if run.nudges_sent >= settings.nudge_budget {
                escalate(
                    state,
                    ws_id,
                    GoalStatus::NeedsAttention(format!(
                        "stalled after {} reminders",
                        run.nudges_sent
                    )),
                    &format!(
                        "the agent for '{}' stalled {} times — it may be stuck; \
                         open its workspace",
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
                "Goal finished",
                &format!(
                    "'{}' is done — read {}",
                    run.goal_name,
                    run.output_rel(run.iteration)
                ),
                true,
            );
        }
        "blocked" if run.iteration < run.started_iteration_cap() => {
            // Feed the feedback forward and start the next iteration
            // in-session. (Fresh-session relaunch lands with headless spawn —
            // see the design doc.)
            match advance_iteration(
                &state.shared,
                ws_id,
                &format!("Iteration {} reported status: blocked.", run.iteration),
            ) {
                Ok(next) => notify(
                    state,
                    ws_id,
                    "Goal working",
                    &format!(
                        "'{}' could not finish yet — starting iteration {next} of {}",
                        run.goal_name,
                        run.started_iteration_cap()
                    ),
                    false,
                ),
                Err(e) => tracing::warn!(goal = %run.goal_name, "auto-iteration failed: {e}"),
            }
        }
        "blocked" => {
            escalate(
                state,
                ws_id,
                GoalStatus::Blocked("the agent could not finish".into()),
                &format!(
                    "'{}' stopped — the agent could not finish; its reasons are in \
                     section 4 of {}",
                    run.goal_name,
                    run.output_rel(run.iteration)
                ),
            );
        }
        other => {
            escalate(
                state,
                ws_id,
                GoalStatus::NeedsAttention(format!("unrecognised status '{other}'")),
                &format!(
                    "'{}' wrote an unrecognised status '{other}' in {} — expected \
                     done or blocked",
                    run.goal_name,
                    run.output_rel(run.iteration)
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

/// Set a status and send a (rate-limited) escalation notification. The
/// notification is plain language; the precise state name goes to the log.
fn escalate(state: &Rc<AppState>, ws_id: Uuid, status: GoalStatus, message: &str) {
    let now = epoch_now();
    tracing::info!(
        workspace = %ws_id,
        state = status.as_str(),
        detail = status.detail().unwrap_or(""),
        "goal escalation"
    );
    let mut should_notify = false;
    {
        let mut goals = lock_or_recover(&state.shared.goals);
        if let Some(run) = goals.get_mut(&ws_id) {
            // Re-escalating the same state repeatedly is the flood case the
            // rate limit exists for. A transition into a TERMINAL state
            // always notifies, though — the run leaves the driver's tick
            // filter, so a suppressed notification would never re-fire.
            let rate_ok = now.saturating_sub(run.last_escalation_epoch) >= ESCALATION_MIN_SECS;
            let entering_terminal = status.is_terminal() && !run.status.is_terminal();
            if run.status != status || rate_ok {
                should_notify = rate_ok || entering_terminal;
                run.status = status;
                if should_notify {
                    run.last_escalation_epoch = now;
                }
            }
        }
    }
    if should_notify {
        notify(state, ws_id, "Goal needs you", message, true);
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

/// Send the "begin iteration N+1" prompt into a run's master (in-session)
/// and update the registry. Thread-safe: input is routed via the UI event
/// channel, so both the driver and socket handlers can call this. `reason`
/// is the first sentence of the prompt ("Iteration 2 reported blocked." /
/// "The reviewer asked for another iteration.").
pub fn advance_iteration(
    shared: &Arc<SharedState>,
    ws_id: Uuid,
    reason: &str,
) -> Result<u32, String> {
    let run = lock_or_recover(&shared.goals)
        .get(&ws_id)
        .cloned()
        .ok_or_else(|| "no goal registered for that workspace".to_string())?;
    // Guard: the current iteration must have actually finished — a
    // double-fired continue would bump the counter past the file the agent
    // was told to write and the driver would poll a file that never comes.
    if parse_iteration_file(&run.output_abs(run.iteration))
        .flatten()
        .is_none()
    {
        return Err(format!(
            "iteration {} has no finished iteration file yet",
            run.iteration
        ));
    }
    // Guard: never type into a pane without a live agent — an idle shell
    // would EXECUTE the prompt (same rule as the nudge path). Claude
    // runners only; custom runners have no process fingerprint to check.
    if state_detection_is_claude(&run.runner) {
        let live = crate::session::claude_resume::all_local_claude_cwds();
        if !live.contains_key(&run.panel_id) {
            return Err(
                "the agent process is not running in that pane — resume it first".into(),
            );
        }
    }
    let next = run.iteration + 1;
    let prev_rel = run.output_rel(run.iteration);
    let next_rel = run.output_rel(next);
    let prompt = format!(
        "{reason} Begin iteration {next}: re-read `{prev_rel}` (especially \
         section 4), address the feedback with reasonable assumptions, continue \
         working the goal, and when finished write `{next_rel}` in the same \
         four-section format with front-matter status done or blocked. Then \
         run: jmux goal complete"
    );
    if !shared.send_ui_event(crate::app::UiEvent::SendInput {
        panel_id: run.panel_id,
        text: format!("{prompt}\r"),
    }) {
        return Err("no UI event channel".into());
    }
    if let Some(r) = lock_or_recover(&shared.goals).get_mut(&ws_id) {
        r.iteration = next;
        r.idle_ticks = 0;
        r.nudges_sent = 0;
        r.no_text_ticks = 0;
        r.status = GoalStatus::Running;
    }
    Ok(next)
}

/// Everything needed to launch a goal run. Built by the `goal.create` socket
/// handler and by the graph scheduler (which is why launching is not inlined
/// in the handler).
pub struct LaunchSpec {
    pub goal_name: String,
    /// Display-only origin ("/path/to/goal.md" or "<graph>:<node>").
    pub goal_path: String,
    pub goal_text: String,
    pub cwd: String,
    pub output_dir_rel: String,
    pub upstream_refs: Vec<String>,
    pub runner_name: String,
    pub runner: GoalRunner,
    pub max_iterations: u32,
    pub permission_mode: String,
    pub wall_clock_minutes: u32,
    pub title: Option<String>,
    pub graph: Option<GraphLink>,
    /// Sidebar group the workspace joins (graph nodes).
    pub group_id: Option<Uuid>,
    /// Switch the user's view to the new workspace. True when a human just
    /// asked for this run (`jmux goal run`) and expects to land in it; false for
    /// scheduler-launched graph nodes, which start their agent headlessly and
    /// must not steal focus.
    pub select: bool,
}

/// Create the workspace, launch the runner, and register the run.
/// Returns the `goal.create` result payload.
pub fn launch_goal(
    shared: &Arc<SharedState>,
    spec: LaunchSpec,
) -> Result<serde_json::Value, String> {
    let cwd = Path::new(&spec.cwd);
    let out_dir = cwd.join(&spec.output_dir_rel);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create {out_dir:?}: {e}"))?;

    let iteration = next_iteration_number(&spec.cwd, &spec.output_dir_rel);
    let feedback_rel = (iteration > 1).then(|| iteration_rel(&spec.output_dir_rel, iteration - 1));
    let feedback_ref = feedback_rel.as_deref().filter(|rel| cwd.join(rel).exists());

    let session_id = Uuid::new_v4().to_string();
    let seed = compose_seed(
        &spec.goal_name,
        &spec.goal_text,
        iteration,
        &spec.output_dir_rel,
        feedback_ref,
        &spec.upstream_refs,
    );

    let mut ws = Workspace::with_directory(&spec.cwd);
    let ws_id = ws.id;
    ws.custom_title = Some(
        spec.title
            .clone()
            .unwrap_or_else(|| format!("goal: {}", spec.goal_name)),
    );

    let sd = seed_dir(ws_id);
    std::fs::create_dir_all(&sd).map_err(|e| format!("cannot create seed dir: {e}"))?;
    let seed_file = sd.join(format!("iteration-{iteration}.md"));
    std::fs::write(&seed_file, &seed).map_err(|e| format!("cannot write seed file: {e}"))?;

    let command = launch_command(
        &spec.runner,
        &session_id,
        &seed,
        &seed_file,
        &spec.permission_mode,
    );
    let is_claude = spec.runner.agent != "custom";

    let Some(panel_id) = ws.panels.keys().next().copied() else {
        return Err("new workspace has no panel".into());
    };
    if let Some(panel) = ws.panels.get_mut(&panel_id) {
        panel.command = Some(command);
        if is_claude {
            // Stamp the session identity at launch: `command=`-launched panes
            // never run shell integration, so the reporting path that
            // normally fills agent_session_id does not apply.
            panel.agent_session_id = Some(session_id.clone());
        }
    }
    // Mirror the master's Task-tool sub-agents beside it (claude only).
    ws.subagent_monitor = is_claude;
    if let Some(gid) = spec.group_id {
        ws.group_id = Some(gid);
    }

    {
        let mut tm = lock_or_recover(&shared.tab_manager);
        let placement = crate::settings::load().new_workspace_placement;
        if spec.select {
            tm.add_workspace_with_placement(ws, placement);
        } else {
            // Background run: the workspace takes its configured place in the
            // sidebar but the user stays where they are. Its agent is started by
            // the driver's headless spawn (`AppState::spawn_panel_headless`) —
            // it does not need to be visible.
            tm.add_workspace_keep_selection(ws, placement);
        }
    }
    shared.notify_ui_refresh();

    register(
        shared,
        GoalRun {
            workspace_id: ws_id,
            panel_id,
            session_id: session_id.clone(),
            goal_name: spec.goal_name.clone(),
            goal_path: spec.goal_path.clone(),
            cwd: spec.cwd.clone(),
            output_dir_rel: spec.output_dir_rel.clone(),
            iteration,
            max_iterations: spec.max_iterations,
            runner_name: spec.runner_name.clone(),
            runner: spec.runner.clone(),
            status: GoalStatus::Running,
            nudges_sent: 0,
            idle_ticks: 0,
            no_text_ticks: 0,
            started_epoch: epoch_now(),
            wall_clock_minutes: spec.wall_clock_minutes,
            last_escalation_epoch: 0,
            graph: spec.graph.clone(),
        },
    );

    Ok(serde_json::json!({
        "workspace_id": ws_id.to_string(),
        "session_id": session_id,
        "goal": spec.goal_name,
        "iteration": iteration,
        "output": iteration_rel(&spec.output_dir_rel, iteration),
        "runner": spec.runner_name,
    }))
}

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
        let seed = compose_seed(
            "mapsite",
            "Build a map.",
            2,
            "docs/roadmap",
            Some("docs/roadmap/iteration-1.md"),
            &["docs/roadmap/g/dep/iteration-3.md".to_string()],
        );
        assert!(seed.contains("mapsite"));
        assert!(seed.contains("Build a map."));
        assert!(seed.contains("docs/roadmap/iteration-2.md"));
        assert!(seed.contains("iteration-1.md"));
        assert!(seed.contains("g/dep/iteration-3.md"));
        assert!(!seed.contains("{goal_text}"));
        assert!(!seed.contains("{output_path}"));
        assert!(!seed.contains("{upstream}"));
    }

    #[test]
    fn launch_command_claude_defaults() {
        let runner = GoalRunner::default();
        let cmd = launch_command(&runner, "abc", "do it", Path::new("/tmp/s.md"), "acceptEdits");
        assert!(cmd.starts_with("claude --session-id abc"));
        assert!(cmd.contains("--permission-mode 'acceptEdits'"));
        // Seed is read from the seed file at spawn — never inlined (panel
        // commands must be a single line; the app rejects control chars).
        assert!(cmd.ends_with("\"$(cat '/tmp/s.md')\""));
        assert!(!cmd.contains("do it"));
        assert!(!cmd.contains("--model"));
        assert!(!cmd.contains('\n'));
    }

    #[test]
    fn launch_command_allowed_tools_are_quoted_and_never_eat_the_seed() {
        let runner = GoalRunner {
            allowed_tools: vec![
                "Bash(cargo test:*)".into(),
                "  ".into(), // blank entries are dropped, not quoted
                "Bash(git commit -m 'wip':*)".into(),
                " Read ".into(),
            ],
            ..Default::default()
        };
        let cmd = launch_command(&runner, "abc", "do it", Path::new("/tmp/s.md"), "acceptEdits");
        // `--allowedTools` is variadic: it must be terminated by a flag,
        // never by the trailing seed argument.
        assert!(
            cmd.starts_with(
                "claude --allowedTools 'Bash(cargo test:*)' 'Bash(git commit -m '\\''wip'\\'':*)' \
                 'Read' --session-id abc"
            ),
            "{cmd}"
        );
        assert!(cmd.ends_with("\"$(cat '/tmp/s.md')\""), "{cmd}");
        assert!(!cmd.contains('\n'));
        // No allowlist configured = no flag at all.
        let bare = launch_command(
            &GoalRunner::default(),
            "abc",
            "do it",
            Path::new("/tmp/s.md"),
            "acceptEdits",
        );
        assert!(!bare.contains("--allowedTools"), "{bare}");
    }

    #[test]
    fn permission_mode_precedence_and_bypass_containment() {
        let plain = GoalRunner::default();
        let mut settings = crate::settings::GoalSettings::default();
        assert_eq!(settings.permission_mode, "acceptEdits");

        // Nothing configured anywhere → the default.
        assert_eq!(resolve_permission_mode(None, &plain, &settings), "acceptEdits");

        // Settings < runner < per-invocation request.
        settings.permission_mode = "plan".into();
        assert_eq!(resolve_permission_mode(None, &plain, &settings), "plan");
        let runner = GoalRunner {
            permission_mode: "acceptEdits".into(),
            ..Default::default()
        };
        assert_eq!(resolve_permission_mode(None, &runner, &settings), "acceptEdits");
        assert_eq!(
            resolve_permission_mode(Some("supervised"), &runner, &settings),
            "supervised"
        );
        // An empty request is "no flag given", not a mode.
        assert_eq!(resolve_permission_mode(Some(""), &runner, &settings), "acceptEdits");

        // Bypass: allowed per invocation, refused from every ambient source.
        assert_eq!(
            resolve_permission_mode(Some("bypassPermissions"), &plain, &settings),
            "bypassPermissions"
        );
        let bypass_runner = GoalRunner {
            permission_mode: "bypassPermissions".into(),
            ..Default::default()
        };
        assert_eq!(resolve_permission_mode(None, &bypass_runner, &settings), "plan");
        settings.permission_mode = "bypassPermissions".into();
        assert_eq!(
            resolve_permission_mode(None, &bypass_runner, &settings),
            "acceptEdits"
        );
        assert!(validate_permission_mode("bypassPermissions", false).is_err());
        assert!(validate_permission_mode("bypassPermissions", true).is_ok());

        // Unknown values are ignored (with a warning), falling through.
        settings.permission_mode = "yolo".into();
        assert_eq!(resolve_permission_mode(Some("nope"), &plain, &settings), "acceptEdits");
        assert!(validate_permission_mode("acceptedits", false).is_err());
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
        let runner_inline = GoalRunner {
            agent: "custom".into(),
            command_template: "mytool {seed}".into(),
            ..Default::default()
        };
        let cmd = launch_command(&runner_inline, "abc", "multi\nline", Path::new("/tmp/s.md"), "");
        assert_eq!(cmd, "mytool \"$(cat '/tmp/s.md')\"");
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
    fn slugify_takes_the_first_words() {
        assert_eq!(
            slugify_goal("Add a --version flag to the CLI, with a test"),
            "add-a-version-flag-to-the"
        );
        assert_eq!(slugify_goal("  "), "goal");
        assert_eq!(slugify_goal("!!! ???"), "goal");
        let long = slugify_goal(
            "supercalifragilistic expialidocious enumeration of everything imaginable",
        );
        assert!(long.len() <= 48, "{long}");
        assert!(!long.ends_with('-'));
    }

    fn test_run(name: &str, status: GoalStatus) -> GoalRun {
        GoalRun {
            workspace_id: Uuid::new_v4(),
            panel_id: Uuid::new_v4(),
            session_id: String::new(),
            goal_name: name.to_string(),
            goal_path: String::new(),
            cwd: "/tmp".into(),
            output_dir_rel: "docs/roadmap".into(),
            iteration: 1,
            max_iterations: 1,
            runner_name: "claude".into(),
            runner: GoalRunner::default(),
            status,
            nudges_sent: 0,
            idle_ticks: 0,
            no_text_ticks: 0,
            started_epoch: 0,
            wall_clock_minutes: 0,
            last_escalation_epoch: 0,
            graph: None,
        }
    }

    fn registry(runs: Vec<GoalRun>) -> GoalRegistry {
        runs.into_iter().map(|r| (r.workspace_id, r)).collect()
    }

    #[test]
    fn resolve_run_by_name_uuid_and_sole_run() {
        let a = test_run("mapsite/map-core", GoalStatus::Running);
        let a_id = a.workspace_id;
        let reg = registry(vec![a]);
        // Exact name, bare node id, and UUID all land on the same run.
        assert_eq!(resolve_run(&reg, Some("mapsite/map-core"), None).unwrap(), a_id);
        assert_eq!(resolve_run(&reg, Some("map-core"), None).unwrap(), a_id);
        assert_eq!(
            resolve_run(&reg, Some(&a_id.to_string()), None).unwrap(),
            a_id
        );
        // No target: the only non-terminal run.
        assert_eq!(resolve_run(&reg, None, None).unwrap(), a_id);
        // Unknown name lists the candidates.
        let err = resolve_run(&reg, Some("nope"), None).unwrap_err();
        assert!(err.contains("mapsite/map-core"), "{err}");
        let err = resolve_run(&reg, Some(&Uuid::new_v4().to_string()), None).unwrap_err();
        assert!(err.contains("no goal run for workspace"), "{err}");
    }

    #[test]
    fn resolve_run_ambiguity_and_hint() {
        let a = test_run("g/one", GoalStatus::Running);
        let b = test_run("g/two", GoalStatus::Running);
        let (a_id, b_id) = (a.workspace_id, b.workspace_id);
        let reg = registry(vec![a, b]);
        let err = resolve_run(&reg, None, None).unwrap_err();
        assert!(err.contains("several goal runs are active"), "{err}");
        assert!(err.contains("g/one") && err.contains("g/two"), "{err}");
        // The caller's own workspace decides when nothing was named.
        assert_eq!(resolve_run(&reg, None, Some(b_id)).unwrap(), b_id);
        // A hint that isn't a goal run falls through to the ambiguity error.
        assert!(resolve_run(&reg, None, Some(Uuid::new_v4())).is_err());
        // A finished run loses to its live namesake.
        let done = test_run("g/one", GoalStatus::Done);
        let reg = registry(vec![
            test_run("g/two", GoalStatus::Running),
            done,
            {
                let mut r = test_run("g/one", GoalStatus::Running);
                r.workspace_id = a_id;
                r
            },
        ]);
        assert_eq!(resolve_run(&reg, Some("g/one"), None).unwrap(), a_id);
    }

    #[test]
    fn goal_source_file_vs_inline_text() {
        let dir = std::env::temp_dir().join(format!("jmux-goal-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("goal.md");
        std::fs::write(&file, "Build a thing.").unwrap();

        let src = resolve_goal_source(&serde_json::json!({
            "goal": file.to_string_lossy(),
        }))
        .unwrap();
        assert!(!src.inline);
        assert_eq!(src.text, "Build a thing.");
        // Stem "goal" → the parent directory names the run.
        assert!(src.name.starts_with("jmux-goal-src-"), "{}", src.name);
        assert_eq!(src.cwd, dir);

        let src = resolve_goal_source(&serde_json::json!({
            "goal_text": "Add a --version flag to the CLI",
            "client_cwd": dir.to_string_lossy(),
        }))
        .unwrap();
        assert!(src.inline);
        assert_eq!(src.name, "add-a-version-flag-to-the");
        // The goal file jmux wrote lives outside the working directory.
        assert!(src.path.starts_with(goal_text_dir()), "{:?}", src.path);
        assert!(!src.path.starts_with(&dir));
        assert_eq!(std::fs::read_to_string(&src.path).unwrap().trim(), src.text);
        let _ = std::fs::remove_file(&src.path);

        // Relative file path and a missing goal are both rejected.
        assert_eq!(
            resolve_goal_source(&serde_json::json!({"goal": "rel/goal.md"}))
                .unwrap_err()
                .0,
            "invalid_params"
        );
        assert_eq!(
            resolve_goal_source(&serde_json::json!({})).unwrap_err().0,
            "invalid_params"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_dir_must_stay_inside_the_repo() {
        assert_eq!(validate_output_dir("docs/roadmap").unwrap(), "docs/roadmap");
        assert_eq!(validate_output_dir(" .jmux/goals/ ").unwrap(), ".jmux/goals");
        assert_eq!(validate_output_dir("./docs/roadmap").unwrap(), "docs/roadmap");
        for bad in ["/tmp/out", "/", "../out", "docs/../../out", "", "  ", "."] {
            assert!(validate_output_dir(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn output_dir_setting_falls_back_when_unusable() {
        let mut settings = crate::settings::GoalSettings::default();
        assert_eq!(output_dir_rel(&settings), "docs/roadmap");
        settings.output_dir = ".jmux/goals".into();
        assert_eq!(output_dir_rel(&settings), ".jmux/goals");
        settings.output_dir = "/etc".into();
        assert_eq!(output_dir_rel(&settings), "docs/roadmap");
    }

    #[test]
    fn bare_goals_iterate_three_times_by_default() {
        assert_eq!(crate::settings::GoalSettings::default().max_iterations, 3);
    }

    #[test]
    fn next_iteration_scans_existing() {
        let dir = std::env::temp_dir().join(format!("jmux-goal-scan-{}", std::process::id()));
        let roadmap = dir.join("docs/roadmap");
        std::fs::create_dir_all(&roadmap).unwrap();
        assert_eq!(next_iteration_number(dir.to_str().unwrap(), "docs/roadmap"), 1);
        std::fs::write(roadmap.join("iteration-3.md"), "x").unwrap();
        assert_eq!(next_iteration_number(dir.to_str().unwrap(), "docs/roadmap"), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
