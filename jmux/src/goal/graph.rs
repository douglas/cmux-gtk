//! Graph orchestration — `jmux graph` (docs/roadmap/DESIGN-goal-graph.md).
//!
//! A graph decomposes a top-level goal into a DAG of sub-goals (nodes), each
//! executed as a goal run (`super::launch_goal`). The **orchestrator agent**
//! proposes the DAG (`proposal.json`); a human review gate approves it; the
//! deterministic **scheduler** below launches ready nodes up to the
//! concurrency cap, feeds upstream iteration reports forward, and (in
//! worktree mode) merges each node's branch back on completion.
//!
//! Ownership rules (single writer per file):
//! - the orchestrator writes only `proposal.json` / `proposal.md`;
//! - the scheduler owns `graph.json` (authoritative state) and generates
//!   `graph.md` (read-only rendering);
//! - humans edit `proposal.json` between proposal and approval — approve
//!   re-reads it, so edits always count.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::{lock_or_recover, AppState, SharedState};
use crate::model::Workspace;

use super::{GoalStatus, GraphLink};

/// The orchestrator's instructions as shipped. Read through
/// `super::Guidance::Orchestrator`, which prefers the user's edited copy at
/// `~/.config/jmux/graph-guidance.md`.
pub(super) const DEFAULT_DECOMPOSE_GUIDANCE: &str = include_str!("decompose_guidance.md");

/// Ticks (2 s each) the scheduler waits for an orchestrator proposal before
/// escalating (it keeps polling afterwards).
const PROPOSAL_WARN_TICKS: u32 = 450; // 15 min

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphState {
    Proposing,
    Proposed,
    Running,
    Complete,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeState {
    Pending,
    Running,
    Review,
    Done,
    Blocked,
    /// The app restarted (or the workspace vanished) while the node ran.
    /// `jmux graph resume` relaunches it.
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    /// Self-contained goal text for this node's agent.
    pub goal: String,
    pub deps: Vec<String>,
    /// Named runner (settings goal.runners); None = graph/default runner.
    pub runner: Option<String>,
    pub status: NodeState,
    pub workspace: Option<Uuid>,
    /// Final iteration number once done (dependents reference this file).
    pub iteration: u32,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub detail: Option<String>,
    /// Manual canvas position (drag-and-drop in the graph panel); None =
    /// auto-layout.
    pub pos: Option<(f64, f64)>,
}

impl Default for GraphNode {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            goal: String::new(),
            deps: Vec::new(),
            runner: None,
            status: NodeState::Pending,
            workspace: None,
            iteration: 0,
            branch: None,
            worktree: None,
            detail: None,
            pos: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphSpec {
    pub name: String,
    /// Repo root every node works against (worktrees hang off it).
    pub base_cwd: String,
    pub goal_path: String,
    pub status: GraphState,
    pub max_concurrency: u32,
    /// Per-node iteration cap.
    pub max_iterations: u32,
    /// Plan review (Gate 1): require human approval of the proposal.
    pub review: bool,
    /// Iteration review (Gate 2): pause each node iteration for a human.
    pub review_iterations: bool,
    /// Per-node git worktrees (on by default — the user's checkout is never
    /// the agent's workbench). Off only when the user opted out or the base
    /// is not a git checkout.
    pub use_worktrees: bool,
    /// Repo-relative directory this graph's state and node iteration
    /// reports live in. Resolved from `goal.output_dir` at create time and
    /// stored here, so a created graph keeps its location if the setting
    /// changes. Missing in graph.json files written before this field
    /// existed → the historical "docs/roadmap".
    pub output_dir: String,
    /// Per-invocation permission-mode override (`--full-auto` /
    /// `--supervised`). Empty = none was given, so each launch resolves its
    /// own runner's `permission_mode`, else `goal.permission_mode`. Graphs
    /// written before this meaning existed carry "acceptEdits", which still
    /// reads as an explicit override — the mode they actually ran with.
    pub permission_mode: String,
    /// Default runner name for nodes that don't set one.
    pub runner: Option<String>,
    pub orchestrator_workspace: Option<Uuid>,
    pub group_id: Option<Uuid>,
    /// Only accept a proposal written after this mtime (revise cycles).
    pub min_proposal_mtime: Option<u64>,
    /// Scheduler-internal wait counter. Not serialized: it changes every
    /// tick while waiting, and persisting it would rewrite graph.json 2×/s.
    #[serde(skip)]
    pub proposal_wait_ticks: u32,
    pub nodes: Vec<GraphNode>,
}

impl Default for GraphSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_cwd: String::new(),
            goal_path: String::new(),
            status: GraphState::Proposing,
            max_concurrency: 1,
            max_iterations: 4,
            review: true,
            review_iterations: false,
            use_worktrees: true,
            output_dir: crate::settings::DEFAULT_GOAL_OUTPUT_DIR.into(),
            permission_mode: String::new(),
            runner: None,
            orchestrator_workspace: None,
            group_id: None,
            min_proposal_mtime: None,
            proposal_wait_ticks: 0,
            nodes: Vec::new(),
        }
    }
}

impl GraphSpec {
    /// Repo-relative output root. Re-validated on read: graph.json is a
    /// human-editable file, and an absolute or `..` path here would write
    /// outside the repo.
    fn output_dir_rel(&self) -> String {
        super::validate_output_dir(&self.output_dir)
            .unwrap_or_else(|_| crate::settings::DEFAULT_GOAL_OUTPUT_DIR.to_string())
    }
    pub fn dir(&self) -> PathBuf {
        Path::new(&self.base_cwd)
            .join(self.output_dir_rel())
            .join(&self.name)
    }
    pub fn graph_json(&self) -> PathBuf {
        self.dir().join("graph.json")
    }
    pub fn proposal_json(&self) -> PathBuf {
        self.dir().join("proposal.json")
    }
    pub fn node(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: &str) -> Option<&mut GraphNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    /// Repo-relative iteration dir for a node.
    pub fn node_output_dir(&self, node_id: &str) -> String {
        format!("{}/{}/{}", self.output_dir_rel(), self.name, node_id)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

pub type GraphRegistry = HashMap<String, GraphSpec>;

/// Plain-language label for a graph state. The serialized identifiers
/// (graph.json, socket JSON) keep their exact names — this is display only.
pub fn graph_state_label(s: GraphState) -> &'static str {
    match s {
        GraphState::Proposing => "planning…",
        GraphState::Proposed => "plan review",
        GraphState::Running => "working",
        GraphState::Complete => "finished",
        GraphState::Paused => "paused",
        GraphState::Stopped => "stopped",
    }
}

/// Plain-language label for a node state (display only — see above).
pub fn node_state_label(s: NodeState) -> &'static str {
    match s {
        NodeState::Pending => "waiting its turn",
        NodeState::Running => "working",
        NodeState::Review => "waiting for your review",
        NodeState::Done => "finished",
        NodeState::Blocked => "needs you",
        NodeState::Interrupted => "interrupted by a restart",
    }
}

// ───────────────────────── persistence ─────────────────────────

/// Pointer file listing known graphs: name → graph dir.
fn pointer_file() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("jmux/graphs.json")
}

fn load_pointers() -> HashMap<String, String> {
    std::fs::read_to_string(pointer_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_pointers(p: &HashMap<String, String>) {
    let path = pointer_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(p) {
        let _ = std::fs::write(path, s);
    }
}

/// Persist a graph: registry entry is already updated by the caller; this
/// writes graph.json + regenerates graph.md.
fn save_graph(spec: &GraphSpec) {
    let _ = std::fs::create_dir_all(spec.dir());
    if let Ok(s) = serde_json::to_string_pretty(spec) {
        let _ = std::fs::write(spec.graph_json(), s);
    }
    let _ = std::fs::write(spec.dir().join("graph.md"), render_markdown(spec));
}

/// Generated, read-only rendering of the graph (the scheduler owns it).
fn render_markdown(spec: &GraphSpec) -> String {
    let mut out = format!(
        "# graph: {}\n\n_Generated by jmux — edit `proposal.json`, not this file._\n\n\
         Status: **{}** · concurrency {} · iterations/node {}\n\n\
         | node | status | deps | runner | iter |\n|---|---|---|---|---|\n",
        spec.name,
        graph_state_label(spec.status),
        spec.max_concurrency,
        spec.max_iterations
    );
    for n in &spec.nodes {
        out.push_str(&format!(
            "| {} | {}{} | {} | {} | {} |\n",
            n.id,
            node_state_label(n.status),
            n.detail
                .as_deref()
                .map(|d| format!(" ({d})"))
                .unwrap_or_default(),
            n.deps.join(", "),
            n.runner.as_deref().unwrap_or("default"),
            n.iteration,
        ));
    }
    for n in &spec.nodes {
        out.push_str(&format!("\n## {} — {}\n\n{}\n", n.id, n.title, n.goal));
    }
    out
}

/// One-time load of known graphs on the first scheduler tick after start.
/// Restart reconciliation: nodes that were Running/Review have no live goal
/// runs any more → Interrupted; a Running graph pauses until `graph resume`.
static LOADED: AtomicBool = AtomicBool::new(false);

fn ensure_loaded(shared: &Arc<SharedState>) {
    if LOADED.swap(true, Ordering::SeqCst) {
        return;
    }
    let pointers = load_pointers();
    let mut registry = lock_or_recover(&shared.graphs);
    for (name, dir) in pointers {
        let path = Path::new(&dir).join("graph.json");
        let Some(mut spec) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<GraphSpec>(&s).ok())
        else {
            continue;
        };
        if spec.name != name {
            continue;
        }
        let mut interrupted = false;
        for n in &mut spec.nodes {
            if matches!(n.status, NodeState::Running | NodeState::Review) {
                n.status = NodeState::Interrupted;
                n.detail = Some("interrupted by a restart".into());
                n.workspace = None;
                interrupted = true;
            }
        }
        if spec.status == GraphState::Running && interrupted {
            spec.status = GraphState::Paused;
        }
        // Orchestrator workspaces don't survive restarts.
        spec.orchestrator_workspace = None;
        save_graph(&spec);
        registry.insert(name, spec);
    }
}

// ───────────────────────── proposal parsing ─────────────────────────

#[derive(Debug, Deserialize)]
struct Proposal {
    nodes: Vec<ProposalNode>,
}

#[derive(Debug, Deserialize)]
struct ProposalNode {
    id: String,
    #[serde(default)]
    title: String,
    goal: String,
    #[serde(default)]
    deps: Vec<String>,
    #[serde(default)]
    runner: Option<String>,
    /// Manual canvas position (written back by the panel during review).
    #[serde(default)]
    pos: Option<(f64, f64)>,
}

fn valid_node_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Parse + validate a proposal file into graph nodes.
pub fn parse_proposal(path: &Path) -> Result<Vec<GraphNode>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read proposal: {e}"))?;
    let proposal: Proposal =
        serde_json::from_str(&text).map_err(|e| format!("proposal.json is invalid: {e}"))?;
    if proposal.nodes.is_empty() {
        return Err("proposal has no nodes".into());
    }
    if proposal.nodes.len() > 50 {
        return Err("proposal has too many nodes (max 50)".into());
    }
    let ids: HashSet<&str> = proposal.nodes.iter().map(|n| n.id.as_str()).collect();
    if ids.len() != proposal.nodes.len() {
        return Err("duplicate node ids".into());
    }
    for n in &proposal.nodes {
        if !valid_node_id(&n.id) {
            return Err(format!(
                "node id '{}' is invalid (lowercase letters, digits, hyphens)",
                n.id
            ));
        }
        if n.goal.trim().is_empty() {
            return Err(format!("node '{}' has an empty goal", n.id));
        }
        let mut seen_deps: HashSet<&str> = HashSet::new();
        for d in &n.deps {
            if !ids.contains(d.as_str()) {
                return Err(format!("node '{}' depends on unknown node '{d}'", n.id));
            }
            if d == &n.id {
                return Err(format!("node '{}' depends on itself", n.id));
            }
            // Duplicate deps would also break the Kahn indegree accounting.
            if !seen_deps.insert(d.as_str()) {
                return Err(format!("node '{}' lists dep '{d}' twice", n.id));
            }
        }
    }
    // Cycle check (Kahn).
    let mut indegree: HashMap<&str, usize> = HashMap::new();
    for n in &proposal.nodes {
        indegree.entry(n.id.as_str()).or_insert(0);
        for _ in &n.deps {
            *indegree.entry(n.id.as_str()).or_insert(0) += 0;
        }
    }
    for n in &proposal.nodes {
        *indegree.get_mut(n.id.as_str()).expect("id present") += n.deps.len();
    }
    let mut queue: Vec<&str> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut seen = 0usize;
    while let Some(id) = queue.pop() {
        seen += 1;
        for n in &proposal.nodes {
            if n.deps.iter().any(|d| d == id) {
                let e = indegree.get_mut(n.id.as_str()).expect("id present");
                *e -= 1;
                if *e == 0 {
                    queue.push(n.id.as_str());
                }
            }
        }
    }
    if seen != proposal.nodes.len() {
        return Err("dependency cycle detected".into());
    }

    Ok(proposal
        .nodes
        .into_iter()
        .map(|n| GraphNode {
            title: if n.title.is_empty() { n.id.clone() } else { n.title },
            id: n.id,
            goal: n.goal,
            deps: n.deps,
            runner: n.runner,
            pos: n.pos,
            ..GraphNode::default()
        })
        .collect())
}

// ───────────────────────── operations (socket-callable) ─────────────────────────

/// Per-node worktrees are on at every concurrency — the user's checkout is
/// never the agent's workbench. `use_worktrees: false` (`--no-worktrees`)
/// opts out, and a base that isn't a git checkout cannot have any.
fn resolve_use_worktrees(params: &serde_json::Value, base_is_git: bool) -> bool {
    params
        .get("use_worktrees")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
        && base_is_git
}

/// Create a graph: sidebar group + orchestrator workspace + registry entry.
/// `source` carries the top-level goal (a file the human wrote, or text jmux
/// wrote to its own data dir) plus the repo the nodes work in.
pub fn create_graph(
    shared: &Arc<SharedState>,
    name: &str,
    source: &super::GoalSource,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !valid_node_id(name) {
        return Err("graph name must be lowercase letters, digits, hyphens".into());
    }
    ensure_loaded(shared);
    // A stopped or completed graph releases its name; anything live must be
    // stopped explicitly first.
    {
        let mut registry = lock_or_recover(&shared.graphs);
        if let Some(existing) = registry.get(name) {
            if matches!(existing.status, GraphState::Stopped | GraphState::Complete) {
                registry.remove(name);
            } else {
                return Err(format!(
                    "graph '{name}' is already {} — jmux graph status {name} to \
                     inspect it, jmux graph stop {name} to replace it",
                    graph_state_label(existing.status)
                ));
            }
        }
    }
    let goal_text = source.text.clone();
    let base_cwd = source.cwd.to_string_lossy().to_string();
    let settings = crate::settings::load().goal;

    let max_concurrency = params
        .get("max_concurrency")
        .and_then(|v| v.as_u64())
        .map(|n| (n as u32).clamp(1, 8))
        .unwrap_or(1);
    let use_worktrees = resolve_use_worktrees(
        params,
        git(&base_cwd, &["rev-parse", "--is-inside-work-tree"]).is_ok(),
    );
    let mut spec = GraphSpec {
        name: name.to_string(),
        base_cwd,
        goal_path: source.path.to_string_lossy().to_string(),
        status: GraphState::Proposing,
        max_concurrency,
        max_iterations: params
            .get("max_iterations")
            .and_then(|v| v.as_u64())
            .map(|n| (n as u32).clamp(1, 20))
            .unwrap_or(4),
        review: params
            .get("review")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        review_iterations: params
            .get("review_iterations")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        use_worktrees,
        // Resolved once, at create: an already-created graph keeps its
        // location even if goal.output_dir changes later.
        output_dir: super::output_dir_rel(&settings),
        // Stored as given, not resolved: nodes may run under different
        // runners, each with its own configured mode.
        permission_mode: params
            .get("permission_mode")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        runner: params
            .get("runner")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        // Only accept a proposal written after this create — a stale
        // proposal.json left by a previous run of the same graph name must
        // not short-circuit the new orchestrator. (`graph approve` re-reads
        // the file unconditionally, so a human can still force the old one.)
        min_proposal_mtime: Some(super::epoch_now()),
        ..GraphSpec::default()
    };
    std::fs::create_dir_all(spec.dir()).map_err(|e| format!("cannot create graph dir: {e}"))?;

    // Sidebar group for the whole graph — reuse an existing group with this
    // name (re-creates would otherwise accumulate duplicate groups).
    let group_id = {
        let mut tm = lock_or_recover(&shared.tab_manager);
        let existing = tm.groups().iter().find(|g| g.name == name).map(|g| g.id);
        Some(existing.unwrap_or_else(|| tm.create_group(name, None)))
    };
    spec.group_id = group_id;

    // Orchestrator workspace.
    let runners: Vec<String> = settings.runners.keys().cloned().collect();
    let seed = super::Guidance::Orchestrator
        .text()
        .replace("{graph_name}", name)
        .replace(
            "{proposal_path}",
            &spec.proposal_json().to_string_lossy(),
        )
        .replace(
            "{proposal_md_path}",
            &spec.dir().join("proposal.md").to_string_lossy(),
        )
        .replace("{max_concurrency}", &spec.max_concurrency.to_string())
        .replace(
            "{runners}",
            &if runners.is_empty() {
                "(none configured)".to_string()
            } else {
                runners.join(", ")
            },
        )
        .replace("{goal_text}", &goal_text);

    let (orch_runner_name, orch_runner) =
        super::resolve_runner_by_name(spec.runner.as_deref().unwrap_or(""), &settings)?;
    let session_id = Uuid::new_v4().to_string();
    let seed_dir = super::seed_dir(Uuid::new_v4());
    std::fs::create_dir_all(&seed_dir).map_err(|e| format!("cannot create seed dir: {e}"))?;
    let seed_file = seed_dir.join("decompose.md");
    std::fs::write(&seed_file, &seed).map_err(|e| format!("cannot write seed: {e}"))?;
    let command = super::launch_command(
        &orch_runner,
        &session_id,
        &seed,
        &seed_file,
        &super::resolve_permission_mode(Some(&spec.permission_mode), &orch_runner, &settings),
    );

    let mut ws = Workspace::with_directory(&spec.base_cwd);
    let ws_id = ws.id;
    ws.custom_title = Some(format!("graph: {name}"));
    ws.group_id = group_id;
    ws.subagent_monitor = orch_runner.agent != "custom";
    if let Some(panel) = ws.panels.values_mut().next() {
        panel.command = Some(command);
        if orch_runner.agent != "custom" {
            panel.agent_session_id = Some(session_id);
        }
    }
    // Live graph control panel beside the orchestrator.
    ws.insert_panel(
        crate::model::Panel::new_graph(name),
        crate::model::panel::SplitOrientation::Horizontal,
    );
    {
        let mut tm = lock_or_recover(&shared.tab_manager);
        let placement = crate::settings::load().new_workspace_placement;
        tm.add_workspace_with_placement(ws, placement);
    }
    shared.notify_ui_refresh();
    spec.orchestrator_workspace = Some(ws_id);

    let mut pointers = load_pointers();
    pointers.insert(name.to_string(), spec.dir().to_string_lossy().to_string());
    save_pointers(&pointers);
    save_graph(&spec);
    let result = serde_json::json!({
        "graph": name,
        "status": "proposing",
        "orchestrator_workspace": ws_id.to_string(),
        "dir": spec.dir().to_string_lossy(),
        "runner": orch_runner_name,
    });
    lock_or_recover(&shared.graphs).insert(name.to_string(), spec);
    Ok(result)
}

/// Approve: re-read proposal.json (human edits count), build the node set,
/// start running.
pub fn approve_graph(shared: &Arc<SharedState>, name: &str) -> Result<serde_json::Value, String> {
    ensure_loaded(shared);
    let mut registry = lock_or_recover(&shared.graphs);
    let spec = registry
        .get_mut(name)
        .ok_or_else(|| format!("unknown graph '{name}'"))?;
    if !matches!(spec.status, GraphState::Proposing | GraphState::Proposed) {
        return Err(format!(
            "graph '{name}' is {} — there is no plan waiting for approval",
            graph_state_label(spec.status)
        ));
    }
    let nodes = parse_proposal(&spec.proposal_json())?;
    spec.nodes = nodes;
    spec.status = GraphState::Running;
    save_graph(spec);
    Ok(serde_json::json!({
        "graph": name,
        "status": "running",
        "nodes": spec.nodes.len(),
    }))
}

/// Ask the orchestrator to revise the proposal.
pub fn revise_graph(
    shared: &Arc<SharedState>,
    name: &str,
    note: &str,
) -> Result<serde_json::Value, String> {
    ensure_loaded(shared);
    let mut registry = lock_or_recover(&shared.graphs);
    let spec = registry
        .get_mut(name)
        .ok_or_else(|| format!("unknown graph '{name}'"))?;
    if !matches!(spec.status, GraphState::Proposing | GraphState::Proposed) {
        return Err(format!(
            "graph '{name}' is {} — its plan is not under review",
            graph_state_label(spec.status)
        ));
    }
    let Some(orch_ws) = spec.orchestrator_workspace else {
        return Err(
            "the workspace that wrote the plan is gone (jmux restarted?) — edit \
             proposal.json yourself, then approve"
                .into(),
        );
    };
    let panel_id = {
        let tm = lock_or_recover(&shared.tab_manager);
        tm.workspace(orch_ws)
            .and_then(|ws| ws.panels.keys().next().copied())
    };
    let Some(panel_id) = panel_id else {
        return Err("that workspace is gone — edit proposal.json yourself instead".into());
    };
    // Never type into a pane without a live agent — an idle shell would
    // execute the prompt.
    if !crate::session::claude_resume::all_local_claude_cwds().contains_key(&panel_id) {
        return Err(
            "the agent that wrote the plan is not running — edit proposal.json \
             yourself, then approve"
                .into(),
        );
    }
    let prompt = format!(
        "Revision requested: {note}\nUpdate {} (and the .md rendering) \
         accordingly — same JSON schema — then reply with a short summary and stop.",
        spec.proposal_json().to_string_lossy()
    );
    // Only accept a proposal newer than now.
    spec.min_proposal_mtime = Some(super::epoch_now());
    spec.status = GraphState::Proposing;
    spec.proposal_wait_ticks = 0;
    save_graph(spec);
    drop(registry);
    if !shared.send_ui_event(crate::app::UiEvent::SendInput {
        panel_id,
        text: format!("{prompt}\r"),
    }) {
        return Err("no UI event channel".into());
    }
    Ok(serde_json::json!({"graph": name, "status": "proposing"}))
}

/// Pause: stop launching new nodes (running nodes continue).
pub fn pause_graph(shared: &Arc<SharedState>, name: &str) -> Result<serde_json::Value, String> {
    set_graph_state(shared, name, GraphState::Paused)
}

/// Resume: relaunch interrupted nodes, retry pending merges, keep going.
pub fn resume_graph(shared: &Arc<SharedState>, name: &str) -> Result<serde_json::Value, String> {
    ensure_loaded(shared);
    let mut registry = lock_or_recover(&shared.graphs);
    let spec = registry
        .get_mut(name)
        .ok_or_else(|| format!("unknown graph '{name}'"))?;
    if spec.nodes.is_empty() {
        return Err(format!(
            "graph '{name}' has no approved plan yet — review the plan, then \
             run: jmux graph approve {name}"
        ));
    }
    let mut merge_retries: Vec<String> = Vec::new();
    for n in &mut spec.nodes {
        if n.status == NodeState::Interrupted {
            n.status = NodeState::Pending;
            n.detail = None;
        }
        // Merge-pending blocks are retried right now (nothing else in the
        // scheduler ever re-drives a blocked node).
        if n.status == NodeState::Blocked
            && n.detail.as_deref().is_some_and(|d| d.starts_with("merge"))
        {
            merge_retries.push(n.id.clone());
        }
    }
    for id in &merge_retries {
        finish_node(shared, spec, id);
    }
    spec.status = GraphState::Running;
    save_graph(spec);
    Ok(serde_json::json!({"graph": name, "status": "running"}))
}

pub fn stop_graph(shared: &Arc<SharedState>, name: &str) -> Result<serde_json::Value, String> {
    let result = set_graph_state(shared, name, GraphState::Stopped)?;
    // Terminate the graph's goal runs so the driver stops nudging agents of
    // a stopped graph toward goals nobody wants any more.
    {
        let mut goals = lock_or_recover(&shared.goals);
        for run in goals.values_mut() {
            if run.graph.as_ref().is_some_and(|l| l.graph == name) && !run.status.is_terminal() {
                run.status = super::GoalStatus::Blocked("graph stopped".into());
            }
        }
    }
    // Remove the graph's sidebar group if nothing lives in it any more —
    // stopped test runs otherwise leave zombie group headers behind.
    let group_id = lock_or_recover(&shared.graphs)
        .get(name)
        .and_then(|s| s.group_id);
    if let Some(gid) = group_id {
        let mut tm = lock_or_recover(&shared.tab_manager);
        let empty = !tm.iter().any(|ws| ws.group_id == Some(gid));
        if empty {
            tm.remove_group(gid);
            drop(tm);
            shared.notify_metadata_refresh();
        }
    }
    Ok(result)
}

fn set_graph_state(
    shared: &Arc<SharedState>,
    name: &str,
    state: GraphState,
) -> Result<serde_json::Value, String> {
    ensure_loaded(shared);
    let mut registry = lock_or_recover(&shared.graphs);
    let spec = registry
        .get_mut(name)
        .ok_or_else(|| format!("unknown graph '{name}'"))?;
    spec.status = state;
    save_graph(spec);
    Ok(spec.to_json())
}

pub fn graph_status(shared: &Arc<SharedState>, name: Option<&str>) -> Result<serde_json::Value, String> {
    ensure_loaded(shared);
    let registry = lock_or_recover(&shared.graphs);
    match name {
        Some(n) => registry
            .get(n)
            .map(GraphSpec::to_json)
            .ok_or_else(|| format!("unknown graph '{n}'")),
        None => Ok(serde_json::json!({
            "graphs": registry.values().map(GraphSpec::to_json).collect::<Vec<_>>()
        })),
    }
}

/// Write `path` WITHOUT advancing its mtime. The Proposing→Proposed gate
/// keys on orchestrator-written proposal mtimes (`min_proposal_mtime`);
/// panel edits (goal text, drag positions) must not trip it — a pre-revision
/// proposal touched by a drag would otherwise be accepted as the revision.
fn write_preserving_mtime(path: &Path, contents: &str) -> Result<(), String> {
    let prev = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    std::fs::write(path, contents).map_err(|e| format!("cannot write {path:?}: {e}"))?;
    if let Some(t) = prev {
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = f.set_modified(t);
        }
    }
    Ok(())
}

/// Persist a node's dragged canvas position. Approved nodes store it in
/// graph.json; during review it's written into proposal.json (and carried
/// across approve, which parses it back out).
pub fn update_node_position(
    shared: &Arc<SharedState>,
    name: &str,
    node_id: &str,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let mut registry = lock_or_recover(&shared.graphs);
    let spec = registry
        .get_mut(name)
        .ok_or_else(|| format!("unknown graph '{name}'"))?;
    if let Some(node) = spec.node_mut(node_id) {
        node.pos = Some((x, y));
        save_graph(spec);
        return Ok(());
    }
    // Review gate: the node lives only in proposal.json.
    let path = spec.proposal_json();
    drop(registry);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read proposal: {e}"))?;
    let mut v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("proposal.json is invalid: {e}"))?;
    let node = v
        .get_mut("nodes")
        .and_then(|n| n.as_array_mut())
        .and_then(|nodes| {
            nodes
                .iter_mut()
                .find(|n| n.get("id").and_then(|i| i.as_str()) == Some(node_id))
        })
        .ok_or_else(|| format!("node '{node_id}' not found"))?;
    node["pos"] = serde_json::json!([x, y]);
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    write_preserving_mtime(&path, &out)
}

/// Edit a proposed node's goal text in `proposal.json` (in-panel review,
/// pre-approval). Approve re-reads the file, so the edit always counts.
pub fn update_proposal_node(
    shared: &Arc<SharedState>,
    name: &str,
    node_id: &str,
    goal_text: &str,
) -> Result<(), String> {
    if goal_text.trim().is_empty() {
        return Err("goal text cannot be empty".into());
    }
    let path = {
        let registry = lock_or_recover(&shared.graphs);
        let spec = registry
            .get(name)
            .ok_or_else(|| format!("unknown graph '{name}'"))?;
        spec.proposal_json()
    };
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("cannot read proposal: {e}"))?;
    let mut v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("proposal.json is invalid: {e}"))?;
    let node = v
        .get_mut("nodes")
        .and_then(|n| n.as_array_mut())
        .and_then(|nodes| {
            nodes
                .iter_mut()
                .find(|n| n.get("id").and_then(|i| i.as_str()) == Some(node_id))
        })
        .ok_or_else(|| format!("node '{node_id}' not found in proposal"))?;
    node["goal"] = serde_json::Value::String(goal_text.to_string());
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    write_preserving_mtime(&path, &out)
}

/// Edit a node's goal text (UI detail card). Applies to future launches and
/// iterations — a running master keeps the seed it started with.
pub fn update_node_goal(
    shared: &Arc<SharedState>,
    name: &str,
    node_id: &str,
    goal_text: &str,
) -> Result<(), String> {
    if goal_text.trim().is_empty() {
        return Err("goal text cannot be empty".into());
    }
    let mut registry = lock_or_recover(&shared.graphs);
    let spec = registry
        .get_mut(name)
        .ok_or_else(|| format!("unknown graph '{name}'"))?;
    let node = spec
        .node_mut(node_id)
        .ok_or_else(|| format!("unknown node '{node_id}'"))?;
    node.goal = goal_text.to_string();
    save_graph(spec);
    Ok(())
}

/// Human accepted a node's current iteration (Gate 2): finish it now.
pub fn accept_node(shared: &Arc<SharedState>, ws_id: Uuid) -> Result<(), String> {
    let link = lock_or_recover(&shared.goals)
        .get(&ws_id)
        .and_then(|r| r.graph.clone());
    let Some(link) = link else { return Ok(()) };
    let mut registry = lock_or_recover(&shared.graphs);
    let Some(spec) = registry.get_mut(&link.graph) else {
        return Ok(());
    };
    finish_node(shared, spec, &link.node);
    save_graph(spec);
    Ok(())
}

/// Human asked for another iteration on a review node: back to Running.
pub fn continue_node(shared: &Arc<SharedState>, ws_id: Uuid) {
    let link = lock_or_recover(&shared.goals)
        .get(&ws_id)
        .and_then(|r| r.graph.clone());
    let Some(link) = link else { return };
    let mut registry = lock_or_recover(&shared.graphs);
    if let Some(spec) = registry.get_mut(&link.graph) {
        if let Some(node) = spec.node_mut(&link.node) {
            if node.status == NodeState::Review {
                node.status = NodeState::Running;
                node.detail = None;
                save_graph(spec);
            }
        }
    }
}

// ───────────────────────── scheduler ─────────────────────────

/// Main-loop entry point, called from the goal driver tick.
pub fn scheduler_tick(state: &Rc<AppState>) {
    let shared = &state.shared;
    ensure_loaded(shared);
    let names: Vec<String> = lock_or_recover(&shared.graphs).keys().cloned().collect();
    for name in names {
        process_graph(shared, &name);
    }
    ensure_orchestrators_spawned(state);
}

/// Start orchestrator agents that have not started.
///
/// `create_graph` selects the orchestrator workspace (the human just typed the
/// command and expects to land in it), but *selected* is not *mapped*: with the
/// quake window down, or before the window has come up, nothing is allocated and
/// the agent would never spawn. Node agents get this from the goal driver
/// (`ensure_agent_spawned`); the orchestrator is not a registered goal run, so it
/// is handled here. Idempotent — a running surface short-circuits.
fn ensure_orchestrators_spawned(state: &Rc<AppState>) {
    // Lock order: graphs → tab_manager. Take the ids, then release.
    let workspaces: Vec<Uuid> = {
        let registry = lock_or_recover(&state.shared.graphs);
        registry
            .values()
            .filter(|spec| {
                matches!(
                    spec.status,
                    GraphState::Proposing | GraphState::Proposed | GraphState::Running
                )
            })
            .filter_map(|spec| spec.orchestrator_workspace)
            .collect()
    };
    for ws_id in workspaces {
        let panel_id = {
            let tm = lock_or_recover(&state.shared.tab_manager);
            tm.workspace(ws_id).and_then(|ws| {
                ws.panels
                    .values()
                    .find(|p| {
                        p.panel_type == crate::model::PanelType::Terminal && p.command.is_some()
                    })
                    .map(|p| p.id)
            })
        };
        let Some(panel_id) = panel_id else { continue };
        if state
            .terminal_cache
            .borrow()
            .get(&panel_id)
            .is_some_and(|s| s.has_spawned())
        {
            continue;
        }
        state.spawn_panel_headless(panel_id);
    }
}

fn process_graph(shared: &Arc<SharedState>, name: &str) {
    // Mutate IN PLACE under the registry lock. A clone → mutate → write-back
    // window would silently revert any socket/UI mutation that landed while
    // the tick ran (approve, goal.accept, pause/stop, dragged node
    // positions) and then persist the stale state to graph.json. Handlers
    // block briefly on the mutex instead. Lock order here and everywhere:
    // graphs → goals → tab_manager.
    let mut registry = lock_or_recover(&shared.graphs);
    let Some(spec) = registry.get_mut(name) else {
        return;
    };
    let before = serde_json::to_string(&*spec).unwrap_or_default();
    match spec.status {
        GraphState::Proposing => tick_proposing(shared, spec),
        GraphState::Running => tick_running(shared, spec),
        _ => return,
    }
    let after = serde_json::to_string(&*spec).unwrap_or_default();
    if before != after {
        save_graph(spec);
    }
}

/// Poll for a (new-enough) valid proposal.
fn tick_proposing(shared: &Arc<SharedState>, spec: &mut GraphSpec) {
    let path = spec.proposal_json();
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let Some(mtime) = mtime else {
        spec.proposal_wait_ticks += 1;
        if spec.proposal_wait_ticks == PROPOSAL_WARN_TICKS {
            notify_graph(
                shared,
                spec,
                "Plan is taking a while",
                &format!(
                    "'{}' has no plan after 15 minutes — open its workspace to see \
                     what the agent is doing",
                    spec.name
                ),
            );
        }
        return;
    };
    if let Some(min) = spec.min_proposal_mtime {
        if mtime < min {
            return; // stale proposal from before the revise request
        }
    }
    match parse_proposal(&path) {
        Ok(_) => {
            if spec.review {
                spec.status = GraphState::Proposed;
                let proposal = spec.proposal_json().to_string_lossy().to_string();
                notify_graph(
                    shared,
                    spec,
                    "Plan ready for your review",
                    &format!(
                        "'{}' has a plan — review it in the graph panel (or edit \
                         {proposal}), then: jmux graph approve {}",
                        spec.name, spec.name
                    ),
                );
            } else {
                // --no-review: skip the plan review and start.
                match parse_proposal(&path) {
                    Ok(nodes) => {
                        spec.nodes = nodes;
                        spec.status = GraphState::Running;
                        notify_graph(
                            shared,
                            spec,
                            "Graph working",
                            &format!(
                                "'{}' started on its plan of {} nodes (plan review skipped)",
                                spec.name,
                                spec.nodes.len()
                            ),
                        );
                    }
                    Err(e) => {
                        tracing::warn!(graph = spec.name, "proposal invalid: {e}");
                    }
                }
            }
        }
        Err(e) => {
            // A partially-written or invalid file: keep waiting, warn once in a while.
            spec.proposal_wait_ticks += 1;
            if spec.proposal_wait_ticks % PROPOSAL_WARN_TICKS == 0 {
                notify_graph(
                    shared,
                    spec,
                    "Plan needs a fix",
                    &format!("the plan for '{}' cannot be used: {e}", spec.name),
                );
            }
        }
    }
}

fn tick_running(shared: &Arc<SharedState>, spec: &mut GraphSpec) {
    // A running graph with no nodes is a broken state (e.g. resumed before
    // approval) — never let it vacuously "complete"; send it back to
    // proposing so the normal flow re-engages.
    if spec.nodes.is_empty() {
        spec.status = GraphState::Proposing;
        return;
    }
    sync_node_states(shared, spec);

    // All done?
    if spec
        .nodes
        .iter()
        .all(|n| n.status == NodeState::Done)
    {
        spec.status = GraphState::Complete;
        notify_graph(
            shared,
            spec,
            "Graph finished",
            &format!("'{}' is done — all {} nodes finished", spec.name, spec.nodes.len()),
        );
        return;
    }

    // Launch ready nodes up to the cap.
    let running = spec
        .nodes
        .iter()
        .filter(|n| matches!(n.status, NodeState::Running | NodeState::Review))
        .count() as u32;
    let mut slots = spec.max_concurrency.saturating_sub(running);
    let done_ids: HashSet<String> = spec
        .nodes
        .iter()
        .filter(|n| n.status == NodeState::Done)
        .map(|n| n.id.clone())
        .collect();
    let ready_ids: Vec<String> = spec
        .nodes
        .iter()
        .filter(|n| n.status == NodeState::Pending && n.deps.iter().all(|d| done_ids.contains(d)))
        .map(|n| n.id.clone())
        .collect();
    for id in ready_ids {
        if slots == 0 {
            break;
        }
        match launch_node(shared, spec, &id) {
            Ok(()) => slots -= 1,
            Err(e) => {
                if let Some(n) = spec.node_mut(&id) {
                    n.status = NodeState::Blocked;
                    n.detail = Some(e.clone());
                }
                notify_graph(
                    shared,
                    spec,
                    "Graph needs you",
                    &format!("'{}' could not start node '{}': {e}", spec.name, id),
                );
            }
        }
    }

    // Stalled? (nothing running, nothing launchable, not everything done)
    let any_active = spec
        .nodes
        .iter()
        .any(|n| matches!(n.status, NodeState::Running | NodeState::Review | NodeState::Pending));
    let any_progressable = spec.nodes.iter().any(|n| {
        n.status == NodeState::Pending && n.deps.iter().all(|d| done_ids.contains(d))
    }) || spec
        .nodes
        .iter()
        .any(|n| matches!(n.status, NodeState::Running | NodeState::Review));
    if any_active && !any_progressable {
        spec.status = GraphState::Paused;
        notify_graph(
            shared,
            spec,
            "Graph needs you",
            &format!(
                "'{}' cannot continue — the nodes that are left are waiting on \
                 stopped ones; fix those, then: jmux graph resume {}",
                spec.name, spec.name
            ),
        );
    }
}

/// Reflect terminal goal-run states into node states.
fn sync_node_states(shared: &Arc<SharedState>, spec: &mut GraphSpec) {
    let review_iterations = spec.review_iterations;
    let name = spec.name.clone();
    let mut to_finish: Vec<String> = Vec::new();
    for node in &mut spec.nodes {
        if node.status != NodeState::Running {
            continue;
        }
        let Some(ws_id) = node.workspace else {
            node.status = NodeState::Interrupted;
            node.detail = Some("no workspace to watch".into());
            continue;
        };
        let run = lock_or_recover(&shared.goals).get(&ws_id).cloned();
        let Some(run) = run else {
            node.status = NodeState::Interrupted;
            node.detail = Some("its workspace was closed, or jmux restarted".into());
            continue;
        };
        node.iteration = run.iteration;
        match run.status {
            GoalStatus::Done => {
                if review_iterations {
                    node.status = NodeState::Review;
                    notify_graph_name(
                        shared,
                        Some(ws_id),
                        "Node waiting for your review",
                        &format!(
                            "'{name}' node '{}' finished iteration {} — accept it or \
                             ask for another: jmux goal accept|continue {name}/{}",
                            node.id, run.iteration, node.id
                        ),
                    );
                } else {
                    to_finish.push(node.id.clone());
                }
            }
            GoalStatus::Blocked(ref d) => {
                node.status = NodeState::Blocked;
                node.detail = Some(d.clone());
                notify_graph_name(
                    shared,
                    Some(ws_id),
                    "Node needs you",
                    &format!("'{name}' node '{}' stopped: {d}", node.id),
                );
            }
            _ => {}
        }
    }
    for id in to_finish {
        finish_node(shared, spec, &id);
    }
}

/// Node finished (agent done + optionally human-accepted): in worktree mode
/// commit its work and merge the branch back; then mark Done.
fn finish_node(shared: &Arc<SharedState>, spec: &mut GraphSpec, node_id: &str) {
    let name = spec.name.clone();
    let base = spec.base_cwd.clone();
    let own_dir = format!("{}/{}/", spec.output_dir_rel(), spec.name);
    let Some(node) = spec.node_mut(node_id) else { return };
    if node.status == NodeState::Done {
        return;
    }
    if let Some(ws_id) = node.workspace {
        if let Some(run) = lock_or_recover(&shared.goals).get(&ws_id) {
            node.iteration = run.iteration.max(node.iteration);
        }
    }
    if let (Some(worktree), Some(branch)) = (node.worktree.clone(), node.branch.clone()) {
        // Commit whatever the node's agent left uncommitted.
        let _ = git(&worktree, &["add", "-A"]);
        let _ = git(
            &worktree,
            &[
                "commit",
                "-m",
                &format!("graph({name}): node {node_id} iteration {}", node.iteration),
            ],
        );
        // Refuse to merge into a dirty base checkout — the graph's own
        // state files (graph.json/graph.md/proposal.*, which the scheduler
        // writes into the base) don't count as the user's work.
        // `-uall` lists untracked FILES: git otherwise collapses an untracked
        // directory to "docs/roadmap/", which no per-graph prefix can match.
        match git(&base, &["status", "--porcelain", "--untracked-files=all"]) {
            Ok(out) if base_is_clean(&out, &own_dir) => {
                match git(&base, &["merge", "--no-ff", "-m", &format!("graph({name}): merge {node_id}"), &branch]) {
                    Ok(_) => {}
                    Err(e) => {
                        let _ = git(&base, &["merge", "--abort"]);
                        node.status = NodeState::Blocked;
                        node.detail = Some(format!("merge conflict: {e}"));
                        notify_graph_name(
                            shared,
                            node.workspace,
                            "Node needs you",
                            &format!(
                                "'{name}' node '{node_id}' is finished, but its branch \
                                 {branch} conflicts with your checkout — resolve it \
                                 yourself, then: jmux graph resume {name}"
                            ),
                        );
                        return;
                    }
                }
            }
            _ => {
                node.status = NodeState::Blocked;
                // Detail keeps its "merge" prefix: `graph resume` retries
                // exactly the nodes whose detail starts with it.
                node.detail = Some(format!(
                    "merge waiting: your checkout has uncommitted changes — commit or \
                     stash them, then: jmux graph resume {name}"
                ));
                notify_graph_name(
                    shared,
                    node.workspace,
                    "Node waiting on you",
                    &format!(
                        "'{name}' node '{node_id}' is finished but cannot merge: your \
                         checkout has uncommitted changes — commit or stash them, \
                         then: jmux graph resume {name}"
                    ),
                );
                return;
            }
        }
    }
    node.status = NodeState::Done;
    node.detail = None;
    notify_graph_name(
        shared,
        node.workspace,
        "Node finished",
        &format!("'{name}' node '{node_id}' is done"),
    );
}

fn launch_node(shared: &Arc<SharedState>, spec: &mut GraphSpec, node_id: &str) -> Result<(), String> {
    let settings = crate::settings::load().goal;
    let node = spec
        .node(node_id)
        .cloned()
        .ok_or_else(|| format!("unknown node '{node_id}'"))?;

    // Working directory: base checkout, or a per-node worktree.
    let (cwd, branch, worktree) = if spec.use_worktrees {
        let wt = format!("{}-worktrees/{}", spec.base_cwd.trim_end_matches('/'), node_id);
        let branch = format!("graph/{}/{}", spec.name, node_id);
        if Path::new(&wt).exists() {
            // A previous run's worktree — recreate it fresh. Reusing the
            // stale branch/dirty tree would resurrect old work into the
            // eventual merge.
            let _ = git(&spec.base_cwd, &["worktree", "remove", "--force", &wt]);
            let _ = std::fs::remove_dir_all(&wt);
        }
        if let Some(parent) = Path::new(&wt).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create worktrees dir: {e}"))?;
        }
        git(
            &spec.base_cwd,
            &["worktree", "add", "-B", &branch, &wt, "HEAD"],
        )
        .map_err(|e| format!("git worktree add failed: {e}"))?;
        (wt.clone(), Some(branch), Some(wt))
    } else {
        (spec.base_cwd.clone(), None, None)
    };

    // Upstream hand-off: each dep's final iteration report, by reference.
    let upstream_refs: Vec<String> = node
        .deps
        .iter()
        .filter_map(|d| {
            let dep = spec.node(d)?;
            (dep.iteration > 0)
                .then(|| super::iteration_rel(&spec.node_output_dir(d), dep.iteration))
        })
        .collect();

    let runner_name = node
        .runner
        .clone()
        .or_else(|| spec.runner.clone())
        .unwrap_or_default();
    let (runner_display, runner) = super::resolve_runner_by_name(&runner_name, &settings)?;
    let permission_mode =
        super::resolve_permission_mode(Some(&spec.permission_mode), &runner, &settings);

    let result = super::launch_goal(
        shared,
        super::LaunchSpec {
            goal_name: format!("{}/{}", spec.name, node_id),
            goal_path: format!("graph:{}:{}", spec.name, node_id),
            goal_text: node.goal.clone(),
            cwd,
            output_dir_rel: spec.node_output_dir(node_id),
            upstream_refs,
            runner_name: runner_display,
            runner,
            max_iterations: spec.max_iterations,
            permission_mode,
            wall_clock_minutes: settings.wall_clock_minutes,
            title: Some(node_id.to_string()),
            graph: Some(GraphLink {
                graph: spec.name.clone(),
                node: node_id.to_string(),
            }),
            group_id: spec.group_id,
            // Scheduler-launched: the node's agent starts headlessly, so never
            // yank the user's view over to it (a K-wide graph would do it K
            // times, and again for every node that follows).
            select: false,
        },
    )?;
    let ws_id = result
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    if let Some(n) = spec.node_mut(node_id) {
        n.status = NodeState::Running;
        n.workspace = ws_id;
        n.branch = branch;
        n.worktree = worktree;
        n.detail = None;
    }
    Ok(())
}

/// Whether `git status --porcelain` output shows nothing but the graph's own
/// state directory (`own_dir`, repo-relative with a trailing slash).
fn base_is_clean(porcelain: &str, own_dir: &str) -> bool {
    porcelain.lines().all(|line| {
        let path = line.get(3..).unwrap_or("").trim();
        // Renames report "old -> new"; the destination is what would move.
        let path = path.rsplit(" -> ").next().unwrap_or(path);
        let path = path.trim_matches('"');
        path.is_empty() || path.starts_with(own_dir) || format!("{path}/") == own_dir
    })
}

/// Run a git command, returning stdout on success.
fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git spawn failed: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn notify_graph(shared: &Arc<SharedState>, spec: &GraphSpec, title: &str, body: &str) {
    notify_graph_name(shared, spec.orchestrator_workspace, title, body);
}

fn notify_graph_name(shared: &Arc<SharedState>, ws: Option<Uuid>, title: &str, body: &str) {
    lock_or_recover(&shared.notifications).add(title, body, ws, None, true);
    shared.notify_metadata_refresh();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_proposal(dir: &Path, json: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("proposal.json");
        std::fs::write(&p, json).unwrap();
        p
    }

    #[test]
    fn proposal_valid_dag_parses() {
        let dir = std::env::temp_dir().join(format!("jmux-graph-ok-{}", std::process::id()));
        let p = write_proposal(
            &dir,
            r#"{"nodes":[
                {"id":"a","goal":"do a","deps":[]},
                {"id":"b","goal":"do b","deps":["a"],"runner":"opus"},
                {"id":"c","title":"C","goal":"do c","deps":["a","b"]}
            ]}"#,
        );
        let nodes = parse_proposal(&p).unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[1].runner.as_deref(), Some("opus"));
        assert_eq!(nodes[2].title, "C");
        assert_eq!(nodes[0].title, "a"); // defaults to id
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn proposal_cycle_rejected() {
        let dir = std::env::temp_dir().join(format!("jmux-graph-cyc-{}", std::process::id()));
        let p = write_proposal(
            &dir,
            r#"{"nodes":[
                {"id":"a","goal":"x","deps":["b"]},
                {"id":"b","goal":"y","deps":["a"]}
            ]}"#,
        );
        assert!(parse_proposal(&p).unwrap_err().contains("cycle"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn proposal_bad_ids_rejected() {
        let dir = std::env::temp_dir().join(format!("jmux-graph-bad-{}", std::process::id()));
        let p = write_proposal(
            &dir,
            r#"{"nodes":[{"id":"../evil","goal":"x","deps":[]}]}"#,
        );
        assert!(parse_proposal(&p).is_err());
        let p = write_proposal(
            &dir,
            r#"{"nodes":[{"id":"a","goal":"x","deps":["missing"]}]}"#,
        );
        assert!(parse_proposal(&p).unwrap_err().contains("unknown node"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn graph_spec_roundtrip() {
        let spec = GraphSpec {
            name: "g".into(),
            base_cwd: "/tmp/repo".into(),
            output_dir: ".jmux/goals".into(),
            nodes: vec![GraphNode {
                id: "a".into(),
                goal: "x".into(),
                status: NodeState::Running,
                ..GraphNode::default()
            }],
            ..GraphSpec::default()
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: GraphSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nodes[0].status, NodeState::Running);
        assert_eq!(back.node_output_dir("a"), ".jmux/goals/g/a");
        assert_eq!(back.dir(), Path::new("/tmp/repo/.jmux/goals/g"));
    }

    #[test]
    fn graph_json_written_by_an_older_build_still_loads() {
        // No output_dir, no use_worktrees: the historical location and
        // behaviour must survive.
        let old = r#"{
            "name":"g","base_cwd":"/tmp/repo","goal_path":"/tmp/repo/goal.md",
            "status":"running","max_concurrency":1,"max_iterations":4,
            "review":true,"review_iterations":false,
            "permission_mode":"acceptEdits","runner":null,
            "orchestrator_workspace":null,"group_id":null,
            "min_proposal_mtime":null,
            "nodes":[{"id":"a","title":"A","goal":"x","deps":[],"runner":null,
                      "status":"done","workspace":null,"iteration":2,
                      "branch":null,"worktree":null,"detail":null,"pos":null}]
        }"#;
        let spec: GraphSpec = serde_json::from_str(old).unwrap();
        assert_eq!(spec.output_dir, crate::settings::DEFAULT_GOAL_OUTPUT_DIR);
        assert_eq!(spec.node_output_dir("a"), "docs/roadmap/g/a");
        assert_eq!(spec.nodes[0].status, NodeState::Done);
        // A missing use_worktrees takes the struct default (on) — an old
        // file that ran without them always carries the field explicitly.
        assert!(spec.use_worktrees);
    }

    #[test]
    fn output_dir_in_graph_json_is_re_validated() {
        // graph.json is hand-editable: an escaping path falls back rather
        // than writing outside the repo.
        for bad in ["/etc", "../../elsewhere", ""] {
            let spec = GraphSpec {
                name: "g".into(),
                base_cwd: "/tmp/repo".into(),
                output_dir: bad.into(),
                ..GraphSpec::default()
            };
            assert_eq!(spec.node_output_dir("a"), "docs/roadmap/g/a", "{bad}");
        }
    }

    #[test]
    fn worktrees_are_on_by_default_and_opt_out() {
        assert!(resolve_use_worktrees(&serde_json::json!({}), true));
        assert!(resolve_use_worktrees(
            &serde_json::json!({"max_concurrency": 1}),
            true
        ));
        assert!(!resolve_use_worktrees(
            &serde_json::json!({"use_worktrees": false}),
            true
        ));
        // No git checkout to hang a worktree off.
        assert!(!resolve_use_worktrees(&serde_json::json!({}), false));
    }

    #[test]
    fn base_dirtiness_ignores_the_graphs_own_files() {
        let own = "docs/roadmap/g/";
        assert!(base_is_clean("", own));
        assert!(base_is_clean(
            "?? docs/roadmap/g/graph.json\n M docs/roadmap/g/proposal.json\n",
            own
        ));
        assert!(base_is_clean("?? docs/roadmap/g\n", own));
        assert!(!base_is_clean(" M src/main.rs\n", own));
        assert!(!base_is_clean("?? docs/roadmap/other/graph.json\n", own));
        // Renames report the destination path.
        assert!(!base_is_clean("R  a.rs -> b.rs\n", own));
    }
}
