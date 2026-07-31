//! Graph control panel — live view + controls for a `jmux graph` run.
//!
//! Renders the in-app graph registry (no file reads): nodes as status chips
//! grouped by dependency depth. Clicking a chip selects the node and opens a
//! **detail card** below the DAG: live status, per-iteration monitoring
//! (every `iteration-N.md` opens in the notes editor), an editable goal
//! text, jump-to-workspace, and the Gate 2 verdicts. Gate 1 actions
//! (Open proposal / Approve & Run / Revise) live in the header row.
//!
//! Rebuilt on a 2 s tick — the same cadence as the scheduler that mutates
//! the registry. The tick skips rebuilds while a TextView inside the panel
//! has focus, so typing in the goal editor is never clobbered.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{lock_or_recover, AppState};
use crate::goal::graph::{GraphNode, GraphSpec, GraphState, NodeState};
use crate::goal::{self, GoalStatus};

/// Which node's detail card is open (survives the 2 s rebuilds).
type Selection = Rc<RefCell<Option<String>>>;

pub fn create_graph_widget(
    graph_name: &str,
    state: &Rc<AppState>,
    is_attention_source: bool,
) -> gtk4::Widget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.add_css_class("pane-container");
    container.add_css_class("graph-panel");
    if is_attention_source {
        container.add_css_class("attention-panel");
    }

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    scroll.set_child(Some(&content));
    container.append(&scroll);

    let selection: Selection = Rc::new(RefCell::new(None));
    rebuild(&content, graph_name, state, &selection);

    // Live refresh while the widget is alive — but never while the user is
    // typing into a TextView inside this panel (goal editor).
    let weak = content.downgrade();
    let state_tick = state.clone();
    let name_tick = graph_name.to_string();
    let selection_tick = selection.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(2000), move || {
        let Some(content) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if text_editor_focused(&content) {
            return glib::ControlFlow::Continue;
        }
        rebuild(&content, &name_tick, &state_tick, &selection_tick);
        glib::ControlFlow::Continue
    });

    container.upcast()
}

/// True when the keyboard focus is a TextView inside `content`.
fn text_editor_focused(content: &gtk4::Box) -> bool {
    content
        .root()
        .and_then(|r| r.focus())
        .is_some_and(|f| f.is::<gtk4::TextView>() && f.is_ancestor(content))
}

fn rebuild(content: &gtk4::Box, graph_name: &str, state: &Rc<AppState>, selection: &Selection) {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }
    let spec = lock_or_recover(&state.shared.graphs)
        .get(graph_name)
        .cloned();
    let Some(spec) = spec else {
        let label = gtk4::Label::new(Some(&format!("graph '{graph_name}' is not loaded")));
        label.add_css_class("dim-label");
        content.append(&label);
        return;
    };

    content.append(&header_row(&spec));
    content.append(&actions_row(&spec, state));

    // Which nodes to render: approved nodes from graph.json, or — during the
    // review gate — the PROPOSED nodes parsed straight from proposal.json,
    // so the plan is reviewed (and edited) right here in the panel.
    let in_review = matches!(spec.status, GraphState::Proposing | GraphState::Proposed);
    let (nodes, proposal_mode) = if !spec.nodes.is_empty() {
        (spec.nodes.clone(), false)
    } else if in_review {
        match crate::goal::graph::parse_proposal(&spec.proposal_json()) {
            Ok(nodes) => (nodes, true),
            Err(e) => {
                let msg = if spec.proposal_json().exists() {
                    format!("Proposal has a problem: {e}")
                } else {
                    "The orchestrator (left pane) is decomposing the goal — \
                     its proposed plan will appear here for review."
                        .to_string()
                };
                let label = gtk4::Label::new(Some(&msg));
                label.add_css_class("dim-label");
                label.set_halign(gtk4::Align::Start);
                label.set_wrap(true);
                content.append(&label);
                (Vec::new(), true)
            }
        }
    } else {
        let label = gtk4::Label::new(Some("No nodes."));
        label.add_css_class("dim-label");
        label.set_halign(gtk4::Align::Start);
        content.append(&label);
        (Vec::new(), false)
    };

    if proposal_mode && !nodes.is_empty() {
        let hint = gtk4::Label::new(Some(
            "Proposed plan — click a node to inspect or edit its goal, then Approve & Run.",
        ));
        hint.add_css_class("dim-label");
        hint.set_halign(gtk4::Align::Start);
        hint.set_wrap(true);
        content.append(&hint);
    }

    // Drop a stale selection (node renamed away by a re-proposal).
    {
        let mut sel = selection.borrow_mut();
        if let Some(id) = sel.clone() {
            if !nodes.iter().any(|n| n.id == id) {
                *sel = None;
            }
        }
    }

    // Nodes by dependency depth (level 0 = no deps). Click selects.
    let selected_id = selection.borrow().clone();
    for level in levels(&nodes) {
        let row = gtk4::FlowBox::new();
        row.set_selection_mode(gtk4::SelectionMode::None);
        row.set_column_spacing(6);
        row.set_row_spacing(6);
        row.set_max_children_per_line(6);
        for node_id in level {
            let Some(node) = nodes.iter().find(|n| n.id == node_id) else { continue };
            let chip = gtk4::Button::new();
            chip.add_css_class("graph-chip");
            chip.add_css_class(if proposal_mode {
                "graph-chip-proposed"
            } else {
                chip_class(node.status)
            });
            if selected_id.as_deref() == Some(node.id.as_str()) {
                chip.add_css_class("graph-chip-selected");
            }
            chip.set_label(&format!("{} {}", node.id, glyph(node.status)));
            let mut tip = node.title.clone();
            if let Some(d) = &node.detail {
                tip.push_str(&format!("\n{d}"));
            }
            tip.push_str("\nClick for status, iterations, and editing");
            chip.set_tooltip_text(Some(&tip));
            // Click → toggle this node's detail card.
            {
                let selection = selection.clone();
                let state = state.clone();
                let name = graph_name.to_string();
                let id = node.id.clone();
                let weak = content.downgrade();
                chip.connect_clicked(move |_| {
                    {
                        let mut sel = selection.borrow_mut();
                        *sel = if sel.as_deref() == Some(id.as_str()) {
                            None
                        } else {
                            Some(id.clone())
                        };
                    }
                    if let Some(content) = weak.upgrade() {
                        rebuild(&content, &name, &state, &selection);
                    }
                });
            }
            row.append(&chip);
        }
        content.append(&row);
    }

    // Detail card for the selected node.
    if let Some(id) = selected_id {
        if let Some(node) = nodes.iter().find(|n| n.id == id) {
            content.append(&detail_card(&spec, node, state, graph_name, proposal_mode));
        }
    }
}

// ───────────────────────── node detail card ─────────────────────────

/// Status + per-iteration monitoring + goal editing for one node. In
/// `proposal_mode` (review gate) the card edits the PROPOSED node in
/// proposal.json and hides run-only sections.
fn detail_card(
    spec: &GraphSpec,
    node: &GraphNode,
    state: &Rc<AppState>,
    graph_name: &str,
    proposal_mode: bool,
) -> gtk4::Widget {
    let card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    card.add_css_class("graph-detail");

    // Title + status.
    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let title = gtk4::Label::new(Some(&format!("{} — {}", node.id, node.title)));
    title.add_css_class("heading");
    title.set_halign(gtk4::Align::Start);
    title.set_hexpand(true);
    head.append(&title);
    let status_text = if proposal_mode {
        "proposed".to_string()
    } else {
        format!("{} {:?}", glyph(node.status), node.status)
    };
    let status = gtk4::Label::new(Some(&status_text));
    status.add_css_class("graph-status");
    head.append(&status);
    card.append(&head);
    if let Some(d) = &node.detail {
        let detail = gtk4::Label::new(Some(d));
        detail.add_css_class("dim-label");
        detail.set_halign(gtk4::Align::Start);
        detail.set_wrap(true);
        card.append(&detail);
    }

    // Facts line: runner, deps, iteration, live driver state.
    let run = node
        .workspace
        .filter(|_| !proposal_mode)
        .and_then(|ws| lock_or_recover(&state.shared.goals).get(&ws).cloned());
    let mut facts = format!(
        "runner: {} · deps: {}",
        node.runner.as_deref().unwrap_or("default"),
        if node.deps.is_empty() {
            "none".to_string()
        } else {
            node.deps.join(", ")
        },
    );
    if !proposal_mode {
        facts.push_str(&format!(
            " · iteration {}/{}",
            node.iteration.max(run.as_ref().map(|r| r.iteration).unwrap_or(0)),
            spec.max_iterations,
        ));
    }
    if let Some(r) = &run {
        facts.push_str(&format!(
            " · driver: {}{} · nudges {}",
            r.status.as_str(),
            r.status.detail().map(|d| format!(" ({d})")).unwrap_or_default(),
            r.nudges_sent,
        ));
    }
    let facts_label = gtk4::Label::new(Some(&facts));
    facts_label.add_css_class("dim-label");
    facts_label.set_halign(gtk4::Align::Start);
    facts_label.set_wrap(true);
    card.append(&facts_label);

    // Actions: jump to workspace + Gate 2 verdicts (run mode only).
    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    if let Some(ws_id) = node.workspace.filter(|_| !proposal_mode) {
        let open = gtk4::Button::with_label("Open workspace");
        let state_c = state.clone();
        open.connect_clicked(move |_| {
            {
                let mut tm = lock_or_recover(&state_c.shared.tab_manager);
                let _ = tm.select_by_id(ws_id);
            }
            state_c.shared.notify_ui_refresh();
        });
        actions.append(&open);
    }
    if node.status == NodeState::Review && !proposal_mode {
        if let Some(ws_id) = node.workspace {
            let cont = gtk4::Button::with_label("Continue");
            cont.set_tooltip_text(Some(
                "Run another iteration — edit section 4 of the latest iteration file first to steer it",
            ));
            {
                let state = state.clone();
                cont.connect_clicked(move |_| {
                    let _ = goal::advance_iteration(
                        &state.shared,
                        ws_id,
                        "The reviewer asked for another iteration.",
                    );
                    goal::graph::continue_node(&state.shared, ws_id);
                });
            }
            actions.append(&cont);

            let accept = gtk4::Button::with_label("Accept");
            accept.add_css_class("suggested-action");
            accept.set_tooltip_text(Some("Accept as final — merge and unblock dependents"));
            {
                let state = state.clone();
                accept.connect_clicked(move |_| {
                    if let Some(run) = lock_or_recover(&state.shared.goals).get_mut(&ws_id) {
                        run.status = GoalStatus::Done;
                    }
                    let _ = goal::graph::accept_node(&state.shared, ws_id);
                });
            }
            actions.append(&accept);
        }
    }
    card.append(&actions);

    // Iterations: one row per existing iteration-N.md, each opening in the
    // notes editor (edit section 4 to steer the next loop). Run mode only —
    // proposed nodes have no iterations yet.
    let node_cwd = run
        .as_ref()
        .map(|r| r.cwd.clone())
        .or_else(|| node.worktree.clone())
        .unwrap_or_else(|| spec.base_cwd.clone());
    let out_dir = Path::new(&node_cwd).join(spec.node_output_dir(&node.id));
    let mut iterations: Vec<(u32, std::path::PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&out_dir) {
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(n) = name
                .strip_prefix("iteration-")
                .and_then(|s| s.strip_suffix(".md"))
                .and_then(|s| s.parse::<u32>().ok())
            {
                iterations.push((n, e.path()));
            }
        }
    }
    iterations.sort_by_key(|(n, _)| *n);
    if !iterations.is_empty() && !proposal_mode {
        let hdr = gtk4::Label::new(Some("Iterations"));
        hdr.add_css_class("dim-label");
        hdr.set_halign(gtk4::Align::Start);
        card.append(&hdr);
        for (n, path) in iterations {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
            let status = goal::parse_iteration_file(&path)
                .flatten()
                .map(|o| o.status)
                .unwrap_or_else(|| "?".into());
            let label = gtk4::Label::new(Some(&format!("iteration-{n}.md — {status}")));
            label.set_halign(gtk4::Align::Start);
            label.set_hexpand(true);
            row.append(&label);
            let open = gtk4::Button::with_label("Open");
            open.set_tooltip_text(Some("Edit in a notes pane (section 4 seeds the next loop)"));
            let state_c = state.clone();
            let path_s = path.to_string_lossy().to_string();
            open.connect_clicked(move |_| open_in_notes(&state_c, &path_s));
            row.append(&open);
            card.append(&row);
        }
    }

    // Goal editor: applies to future launches/iterations.
    let goal_hdr = gtk4::Label::new(Some(if proposal_mode {
        "Goal (edit before approving — Approve & Run uses what's saved here)"
    } else {
        "Goal (edits apply to future launches/iterations)"
    }));
    goal_hdr.add_css_class("dim-label");
    goal_hdr.set_halign(gtk4::Align::Start);
    card.append(&goal_hdr);
    let text = gtk4::TextView::new();
    text.set_wrap_mode(gtk4::WrapMode::WordChar);
    text.buffer().set_text(&node.goal);
    text.add_css_class("graph-goal-editor");
    let text_scroll = gtk4::ScrolledWindow::new();
    text_scroll.set_min_content_height(90);
    text_scroll.set_max_content_height(180);
    text_scroll.set_propagate_natural_height(true);
    text_scroll.set_child(Some(&text));
    card.append(&text_scroll);
    let save_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    save_row.set_halign(gtk4::Align::End);
    let save = gtk4::Button::with_label("Save goal");
    {
        let state = state.clone();
        let name = graph_name.to_string();
        let node_id = node.id.clone();
        let buffer = text.buffer();
        save.connect_clicked(move |btn| {
            let (start, end) = buffer.bounds();
            let goal_text = buffer.text(&start, &end, false).to_string();
            // Review gate edits go to proposal.json (what Approve reads);
            // post-approval edits go to the authoritative graph.json.
            let result = if proposal_mode {
                goal::graph::update_proposal_node(&state.shared, &name, &node_id, &goal_text)
            } else {
                goal::graph::update_node_goal(&state.shared, &name, &node_id, &goal_text)
            };
            match result {
                Ok(()) => {
                    let _ = state
                        .shared
                        .send_ui_event(crate::app::UiEvent::ShowToast(format!(
                            "Saved goal for node '{node_id}'"
                        )));
                    // Drop focus so the next tick may rebuild with fresh state.
                    if let Some(root) = btn.root() {
                        root.set_focus(None::<&gtk4::Widget>);
                    }
                }
                Err(e) => {
                    let _ = state
                        .shared
                        .send_ui_event(crate::app::UiEvent::ShowToast(format!("Save failed: {e}")));
                }
            }
        });
    }
    save_row.append(&save);
    card.append(&save_row);

    card.upcast()
}

// ───────────────────────── header + graph actions ─────────────────────────

fn header_row(spec: &GraphSpec) -> gtk4::Widget {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let title = gtk4::Label::new(Some(&format!("graph: {}", spec.name)));
    title.add_css_class("heading");
    title.set_halign(gtk4::Align::Start);
    row.append(&title);

    let status = gtk4::Label::new(Some(status_text(spec.status)));
    status.add_css_class("graph-status");
    status.add_css_class(match spec.status {
        GraphState::Running => "graph-status-running",
        GraphState::Complete => "graph-status-done",
        GraphState::Proposed | GraphState::Proposing => "graph-status-review",
        _ => "graph-status-paused",
    });
    row.append(&status);

    let running = spec
        .nodes
        .iter()
        .filter(|n| n.status == NodeState::Running)
        .count();
    let slots = gtk4::Label::new(Some(&format!("{running}/{} slots", spec.max_concurrency)));
    slots.add_css_class("dim-label");
    slots.set_hexpand(true);
    slots.set_halign(gtk4::Align::End);
    row.append(&slots);
    row.upcast()
}

fn actions_row(spec: &GraphSpec, state: &Rc<AppState>) -> gtk4::Widget {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let name = spec.name.clone();

    // Gate 1 actions while the proposal is under review.
    if matches!(spec.status, GraphState::Proposing | GraphState::Proposed) {
        let open = gtk4::Button::with_label("Open proposal");
        open.set_tooltip_text(Some(
            "Edit proposal.json in a notes pane — your edits count on Approve",
        ));
        {
            let state = state.clone();
            let path = spec.proposal_json().to_string_lossy().to_string();
            open.connect_clicked(move |_| open_in_notes(&state, &path));
        }
        row.append(&open);

        let approve = gtk4::Button::with_label("Approve & Run");
        approve.add_css_class("suggested-action");
        {
            let state = state.clone();
            let name = name.clone();
            approve.connect_clicked(move |_| {
                if let Err(e) = goal::graph::approve_graph(&state.shared, &name) {
                    toast(&state, &format!("Approve failed: {e}"));
                } else {
                    state.shared.notify_ui_refresh();
                }
            });
        }
        row.append(&approve);
    }

    match spec.status {
        GraphState::Running => {
            let pause = gtk4::Button::with_label("Pause");
            let state_c = state.clone();
            let name_c = name.clone();
            pause.connect_clicked(move |_| {
                let _ = goal::graph::pause_graph(&state_c.shared, &name_c);
            });
            row.append(&pause);
        }
        GraphState::Paused | GraphState::Stopped => {
            let resume = gtk4::Button::with_label("Resume");
            let state_c = state.clone();
            let name_c = name.clone();
            resume.connect_clicked(move |_| {
                let _ = goal::graph::resume_graph(&state_c.shared, &name_c);
            });
            row.append(&resume);
        }
        _ => {}
    }

    if spec.status != GraphState::Stopped && spec.status != GraphState::Complete {
        let stop = gtk4::Button::with_label("Stop");
        stop.add_css_class("destructive-action");
        let state_c = state.clone();
        let name_c = name.clone();
        stop.connect_clicked(move |_| {
            let _ = goal::graph::stop_graph(&state_c.shared, &name_c);
        });
        row.append(&stop);
    }
    row.upcast()
}

// ───────────────────────── helpers ─────────────────────────

/// Open a file in an editable notes pane inside the selected workspace.
fn open_in_notes(state: &Rc<AppState>, path: &str) {
    {
        let mut tm = lock_or_recover(&state.shared.tab_manager);
        if let Some(ws) = tm.selected_mut() {
            let already_open = ws.panels.values().any(|p| {
                p.panel_type == crate::model::PanelType::Notes
                    && p.markdown_file.as_deref() == Some(path)
            });
            if !already_open {
                ws.insert_panel(
                    crate::model::Panel::new_notes(path),
                    crate::model::panel::SplitOrientation::Horizontal,
                );
            }
        }
    }
    state.shared.notify_ui_refresh();
}

fn toast(state: &Rc<AppState>, msg: &str) {
    let _ = state
        .shared
        .send_ui_event(crate::app::UiEvent::ShowToast(msg.to_string()));
}

fn status_text(s: GraphState) -> &'static str {
    match s {
        GraphState::Proposing => "proposing…",
        GraphState::Proposed => "review gate",
        GraphState::Running => "running",
        GraphState::Complete => "complete",
        GraphState::Paused => "paused",
        GraphState::Stopped => "stopped",
    }
}

fn glyph(s: NodeState) -> &'static str {
    match s {
        NodeState::Done => "✓",
        NodeState::Running => "✻",
        NodeState::Review => "⏸",
        NodeState::Blocked => "⚠",
        NodeState::Interrupted => "⏹",
        NodeState::Pending => "○",
    }
}

fn chip_class(s: NodeState) -> &'static str {
    match s {
        NodeState::Pending => "graph-chip-pending",
        NodeState::Running => "graph-chip-running",
        NodeState::Review => "graph-chip-review",
        NodeState::Done => "graph-chip-done",
        NodeState::Blocked => "graph-chip-blocked",
        NodeState::Interrupted => "graph-chip-interrupted",
    }
}

/// Node ids grouped by dependency depth (level 0 first). Cycle-safe: the
/// proposal validator rejects cycles, but cap the depth walk anyway.
fn levels(nodes: &[GraphNode]) -> Vec<Vec<String>> {
    use std::collections::HashMap;
    let mut depth: HashMap<&str, usize> = HashMap::new();
    fn depth_of<'a>(
        nodes: &'a [GraphNode],
        id: &'a str,
        depth: &mut std::collections::HashMap<&'a str, usize>,
        guard: usize,
    ) -> usize {
        if guard > 64 {
            return 0;
        }
        if let Some(d) = depth.get(id) {
            return *d;
        }
        let d = nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| {
                n.deps
                    .iter()
                    .map(|dep| depth_of(nodes, dep.as_str(), depth, guard + 1) + 1)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        depth.insert(id, d);
        d
    }
    for n in nodes {
        depth_of(nodes, n.id.as_str(), &mut depth, 0);
    }
    let max_depth = depth.values().copied().max().unwrap_or(0);
    let mut out: Vec<Vec<String>> = vec![Vec::new(); max_depth + 1];
    for n in nodes {
        let d = depth.get(n.id.as_str()).copied().unwrap_or(0);
        out[d].push(n.id.clone());
    }
    out.retain(|l| !l.is_empty());
    out
}
