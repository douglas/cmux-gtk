//! Graph control panel — live view + controls for a `jmux graph` run.
//!
//! Renders the in-app graph registry (no file reads): nodes as status chips
//! grouped by dependency depth, clickable to jump to the node's workspace,
//! with the gate actions (Approve / Revise / Open proposal) and per-node
//! Continue/Accept when an iteration awaits review. Rebuilt on a 2 s tick —
//! the same cadence as the scheduler that mutates the registry.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{lock_or_recover, AppState};
use crate::goal::graph::{GraphSpec, GraphState, NodeState};
use crate::goal::{self, GoalStatus};

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

    rebuild(&content, graph_name, state);

    // Live refresh while the widget is alive.
    let weak = content.downgrade();
    let state_tick = state.clone();
    let name_tick = graph_name.to_string();
    glib::timeout_add_local(std::time::Duration::from_millis(2000), move || {
        let Some(content) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        rebuild(&content, &name_tick, &state_tick);
        glib::ControlFlow::Continue
    });

    container.upcast()
}

fn rebuild(content: &gtk4::Box, graph_name: &str, state: &Rc<AppState>) {
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

    content.append(&header_row(&spec, state));
    content.append(&actions_row(&spec, state));

    // Nodes by dependency depth (level 0 = no deps).
    if spec.nodes.is_empty() {
        let label = gtk4::Label::new(Some(match spec.status {
            GraphState::Proposing => "Waiting for the orchestrator's proposal…",
            GraphState::Proposed => "Proposal ready — review it, then Approve.",
            _ => "No nodes.",
        }));
        label.add_css_class("dim-label");
        label.set_halign(gtk4::Align::Start);
        content.append(&label);
    }
    for level in levels(&spec) {
        let row = gtk4::FlowBox::new();
        row.set_selection_mode(gtk4::SelectionMode::None);
        row.set_column_spacing(6);
        row.set_row_spacing(6);
        row.set_max_children_per_line(6);
        for node_id in level {
            let Some(node) = spec.node(&node_id) else { continue };
            let chip = gtk4::Button::new();
            chip.add_css_class("graph-chip");
            chip.add_css_class(chip_class(node.status));
            let label = match node.status {
                NodeState::Done => format!("{} ✓", node.id),
                NodeState::Running => format!("{} ✻", node.id),
                NodeState::Review => format!("{} ⏸", node.id),
                NodeState::Blocked => format!("{} ⚠", node.id),
                NodeState::Interrupted => format!("{} ⏹", node.id),
                NodeState::Pending => format!("{} ○", node.id),
            };
            chip.set_label(&label);
            let mut tip = node.title.clone();
            if let Some(d) = &node.detail {
                tip.push_str(&format!("\n{d}"));
            }
            chip.set_tooltip_text(Some(&tip));
            // Click → jump to the node's workspace.
            if let Some(ws_id) = node.workspace {
                let state = state.clone();
                chip.connect_clicked(move |_| {
                    {
                        let mut tm = lock_or_recover(&state.shared.tab_manager);
                        let _ = tm.select_by_id(ws_id);
                    }
                    state.shared.notify_ui_refresh();
                });
            }
            row.append(&chip);
        }
        content.append(&row);
    }

    // Review verdicts (Gate 2).
    for node in spec.nodes.iter().filter(|n| n.status == NodeState::Review) {
        let Some(ws_id) = node.workspace else { continue };
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let label = gtk4::Label::new(Some(&format!(
            "{} — iteration {} awaits review",
            node.id, node.iteration
        )));
        label.set_halign(gtk4::Align::Start);
        label.set_hexpand(true);
        row.append(&label);

        let cont = gtk4::Button::with_label("Continue");
        cont.set_tooltip_text(Some(
            "Run another iteration (edit section 4 of the iteration file first to steer it)",
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
        row.append(&cont);

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
        row.append(&accept);
        content.append(&row);
    }
}

fn header_row(spec: &GraphSpec, _state: &Rc<AppState>) -> gtk4::Widget {
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
        open.set_tooltip_text(Some("Edit proposal.json in a notes pane — your edits count on Approve"));
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
fn levels(spec: &GraphSpec) -> Vec<Vec<String>> {
    use std::collections::HashMap;
    let mut depth: HashMap<&str, usize> = HashMap::new();
    fn depth_of<'a>(
        spec: &'a GraphSpec,
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
        let d = spec
            .node(id)
            .map(|n| {
                n.deps
                    .iter()
                    .map(|dep| depth_of(spec, dep.as_str(), depth, guard + 1) + 1)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        depth.insert(id, d);
        d
    }
    for n in &spec.nodes {
        depth_of(spec, n.id.as_str(), &mut depth, 0);
    }
    let max_depth = depth.values().copied().max().unwrap_or(0);
    let mut out: Vec<Vec<String>> = vec![Vec::new(); max_depth + 1];
    for n in &spec.nodes {
        let d = depth.get(n.id.as_str()).copied().unwrap_or(0);
        out[d].push(n.id.clone());
    }
    out.retain(|l| !l.is_empty());
    out
}
