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

const DECOMPOSE_GUIDANCE: &str = include_str!("decompose_guidance.md");

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
    /// Gate 1: require human approval of the proposal.
    pub review: bool,
    /// Gate 2: pause each node iteration for human review.
    pub review_iterations: bool,
    pub use_worktrees: bool,
    pub permission_mode: String,
    /// Default runner name for nodes that don't set one.
    pub runner: Option<String>,
    pub orchestrator_workspace: Option<Uuid>,
    pub group_id: Option<Uuid>,
    /// Only accept a proposal written after this mtime (revise cycles).
    pub min_proposal_mtime: Option<u64>,
    /// Scheduler-internal wait counter (not persisted meaningfully).
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
            use_worktrees: false,
            permission_mode: "acceptEdits".into(),
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
    pub fn dir(&self) -> PathBuf {
        Path::new(&self.base_cwd)
            .join("docs/roadmap")
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
        format!("docs/roadmap/{}/{}", self.name, node_id)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

pub type GraphRegistry = HashMap<String, GraphSpec>;

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
         Status: **{:?}** · concurrency {} · iterations/node {}\n\n\
         | node | status | deps | runner | iter |\n|---|---|---|---|---|\n",
        spec.name, spec.status, spec.max_concurrency, spec.max_iterations
    );
    for n in &spec.nodes {
        out.push_str(&format!(
            "| {} | {:?}{} | {} | {} | {} |\n",
            n.id,
            n.status,
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
                n.detail = Some("app restarted mid-run".into());
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
        for d in &n.deps {
            if !ids.contains(d.as_str()) {
                return Err(format!("node '{}' depends on unknown node '{d}'", n.id));
            }
            if d == &n.id {
                return Err(format!("node '{}' depends on itself", n.id));
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
            ..GraphNode::default()
        })
        .collect())
}

// ───────────────────────── operations (socket-callable) ─────────────────────────

/// Create a graph: sidebar group + orchestrator workspace + registry entry.
pub fn create_graph(
    shared: &Arc<SharedState>,
    name: &str,
    goal_path: &Path,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !valid_node_id(name) {
        return Err("graph name must be lowercase letters, digits, hyphens".into());
    }
    ensure_loaded(shared);
    if lock_or_recover(&shared.graphs).contains_key(name) {
        return Err(format!(
            "graph '{name}' already exists (jmux graph status {name}, or stop it first)"
        ));
    }
    let goal_text =
        std::fs::read_to_string(goal_path).map_err(|e| format!("cannot read goal file: {e}"))?;
    let parent = goal_path.parent().unwrap_or(Path::new("/"));
    let base_cwd = super::find_git_root(parent)
        .unwrap_or_else(|| parent.to_path_buf())
        .to_string_lossy()
        .to_string();

    let max_concurrency = params
        .get("max_concurrency")
        .and_then(|v| v.as_u64())
        .map(|n| (n as u32).clamp(1, 8))
        .unwrap_or(1);
    let mut spec = GraphSpec {
        name: name.to_string(),
        base_cwd,
        goal_path: goal_path.to_string_lossy().to_string(),
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
        use_worktrees: max_concurrency > 1,
        permission_mode: params
            .get("permission_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("acceptEdits")
            .to_string(),
        runner: params
            .get("runner")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        ..GraphSpec::default()
    };
    std::fs::create_dir_all(spec.dir()).map_err(|e| format!("cannot create graph dir: {e}"))?;

    // Sidebar group for the whole graph.
    let group_id = {
        let mut tm = lock_or_recover(&shared.tab_manager);
        Some(tm.create_group(name, None))
    };
    spec.group_id = group_id;

    // Orchestrator workspace.
    let settings = crate::settings::load().goal;
    let runners: Vec<String> = settings.runners.keys().cloned().collect();
    let seed = DECOMPOSE_GUIDANCE
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
        &spec.permission_mode,
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
        return Err(format!("graph is {:?}, not awaiting approval", spec.status));
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
        return Err(format!("graph is {:?}, not in review", spec.status));
    }
    let Some(orch_ws) = spec.orchestrator_workspace else {
        return Err(
            "no orchestrator workspace (app restarted?) — edit proposal.json directly, then approve"
                .into(),
        );
    };
    let panel_id = {
        let tm = lock_or_recover(&shared.tab_manager);
        tm.workspace(orch_ws)
            .and_then(|ws| ws.panels.keys().next().copied())
    };
    let Some(panel_id) = panel_id else {
        return Err("orchestrator workspace is gone — edit proposal.json directly".into());
    };
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
    for n in &mut spec.nodes {
        if n.status == NodeState::Interrupted {
            n.status = NodeState::Pending;
            n.detail = None;
        }
        // Merge-pending blocks are retried by the scheduler.
        if n.status == NodeState::Blocked
            && n.detail.as_deref().is_some_and(|d| d.starts_with("merge"))
        {
            n.status = NodeState::Review;
            n.detail = Some("retrying merge".into());
        }
    }
    spec.status = GraphState::Running;
    save_graph(spec);
    Ok(serde_json::json!({"graph": name, "status": "running"}))
}

pub fn stop_graph(shared: &Arc<SharedState>, name: &str) -> Result<serde_json::Value, String> {
    set_graph_state(shared, name, GraphState::Stopped)
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
}

fn process_graph(shared: &Arc<SharedState>, name: &str) {
    let Some(mut spec) = lock_or_recover(&shared.graphs).get(name).cloned() else {
        return;
    };
    let before = serde_json::to_string(&spec).unwrap_or_default();
    match spec.status {
        GraphState::Proposing => tick_proposing(shared, &mut spec),
        GraphState::Running => tick_running(shared, &mut spec),
        _ => return,
    }
    let after = serde_json::to_string(&spec).unwrap_or_default();
    if before != after {
        save_graph(&spec);
        lock_or_recover(&shared.graphs).insert(name.to_string(), spec);
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
                "Graph proposal overdue",
                &format!(
                    "graph '{}': no proposal after 15 min — check the orchestrator workspace",
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
                notify_graph(
                    shared,
                    spec,
                    "Graph proposal ready",
                    &format!(
                        "graph '{}' proposed — review docs/roadmap/{}/proposal.json, \
                         then: jmux graph approve {}",
                        spec.name, spec.name, spec.name
                    ),
                );
            } else {
                // --no-review: approve immediately.
                match parse_proposal(&path) {
                    Ok(nodes) => {
                        spec.nodes = nodes;
                        spec.status = GraphState::Running;
                        notify_graph(
                            shared,
                            spec,
                            "Graph running",
                            &format!("graph '{}' auto-approved ({} nodes)", spec.name, spec.nodes.len()),
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
                    "Graph proposal invalid",
                    &format!("graph '{}': {e}", spec.name),
                );
            }
        }
    }
}

fn tick_running(shared: &Arc<SharedState>, spec: &mut GraphSpec) {
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
            "Graph complete",
            &format!("graph '{}' finished — all {} nodes done", spec.name, spec.nodes.len()),
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
                    "Graph node failed to launch",
                    &format!("graph '{}' node '{}': {e}", spec.name, id),
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
            "Graph stalled",
            &format!(
                "graph '{}' cannot progress — blocked/interrupted nodes gate the rest \
                 (fix, then: jmux graph resume {})",
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
            node.detail = Some("no workspace recorded".into());
            continue;
        };
        let run = lock_or_recover(&shared.goals).get(&ws_id).cloned();
        let Some(run) = run else {
            node.status = NodeState::Interrupted;
            node.detail = Some("goal run vanished (workspace closed or app restarted)".into());
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
                        "Graph node awaiting review",
                        &format!(
                            "graph '{name}' node '{}' iteration {} ready — \
                             jmux goal continue|accept --workspace {ws_id}",
                            node.id, run.iteration
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
                    "Graph node blocked",
                    &format!("graph '{name}' node '{}': {d}", node.id),
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
        // Refuse to merge into a dirty base checkout.
        match git(&base, &["status", "--porcelain"]) {
            Ok(out) if out.trim().is_empty() => {
                match git(&base, &["merge", "--no-ff", "-m", &format!("graph({name}): merge {node_id}"), &branch]) {
                    Ok(_) => {}
                    Err(e) => {
                        let _ = git(&base, &["merge", "--abort"]);
                        node.status = NodeState::Blocked;
                        node.detail = Some(format!("merge conflict: {e}"));
                        notify_graph_name(
                            shared,
                            node.workspace,
                            "Graph merge conflict",
                            &format!(
                                "graph '{name}' node '{node_id}': branch {branch} conflicts — \
                                 resolve manually, then: jmux graph resume {name}"
                            ),
                        );
                        return;
                    }
                }
            }
            _ => {
                node.status = NodeState::Blocked;
                node.detail = Some(format!(
                    "merge-pending: base checkout is dirty — commit/stash it, then: jmux graph resume {name}"
                ));
                notify_graph_name(
                    shared,
                    node.workspace,
                    "Graph merge pending",
                    &format!("graph '{name}' node '{node_id}': base checkout dirty, merge deferred"),
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
        "Graph node done",
        &format!("graph '{name}' node '{node_id}' complete"),
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
        if !Path::new(&wt).exists() {
            if let Some(parent) = Path::new(&wt).parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create worktrees dir: {e}"))?;
            }
            git(
                &spec.base_cwd,
                &["worktree", "add", "-B", &branch, &wt, "HEAD"],
            )
            .map_err(|e| format!("git worktree add failed: {e}"))?;
        }
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
            permission_mode: spec.permission_mode.clone(),
            wall_clock_minutes: settings.wall_clock_minutes,
            title: Some(node_id.to_string()),
            graph: Some(GraphLink {
                graph: spec.name.clone(),
                node: node_id.to_string(),
            }),
            group_id: spec.group_id,
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
        assert_eq!(back.node_output_dir("a"), "docs/roadmap/g/a");
    }
}
