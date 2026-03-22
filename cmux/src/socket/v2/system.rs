//! System V2 handlers (capabilities, identify, tree).

use std::sync::Arc;

use serde_json::Value;

use crate::app::{lock_or_recover, SharedState};

use super::Response;

pub(super) fn handle_capabilities(id: Value) -> Response {
    let mut methods: Vec<&str> = vec![
        "system.ping",
        "system.capabilities",
        "system.identify",
        "system.tree",
        "workspace.list",
        "workspace.new",
        "workspace.create",
        "workspace.create_ssh",
        "workspace.remote.status",
        "workspace.select",
        "workspace.next",
        "workspace.previous",
        "workspace.last",
        "workspace.latest_unread",
        "workspace.close",
        "workspace.current",
        "workspace.rename",
        "workspace.reorder",
        "workspace.set_status",
        "workspace.report_git_branch",
        "workspace.set_progress",
        "workspace.clear_progress",
        "workspace.append_log",
        "workspace.clear_status",
        "workspace.list_status",
        "workspace.clear_log",
        "workspace.list_log",
        "workspace.report_meta",
        "workspace.clear_meta",
        "workspace.list_meta",
        "workspace.report_meta_block",
        "workspace.clear_meta_block",
        "workspace.list_meta_blocks",
        "workspace.action",
        "workspace.report_pr",
        "workspace.move_to_window",
        "app.focus_override.set",
        "app.simulate_active",
        "pane.new",
        "pane.list",
        "pane.focus",
        "pane.close",
        "pane.last",
        "pane.swap",
        "pane.create",
        "pane.resize",
        "pane.focus_direction",
        "pane.break",
        "pane.join",
        "surface.send_input",
        "surface.send_text",
        "surface.list",
        "surface.current",
        "surface.focus",
        "surface.split",
        "surface.close",
        "surface.action",
        "surface.health",
        "surface.send_key",
        "surface.read_text",
        "surface.refresh",
        "surface.clear_history",
        "surface.trigger_flash",
        "surface.move",
        "surface.reorder",
        "surface.create",
        "surface.drag_to_split",
        "tab.action",
        "pane.surfaces",
        "pane.equalize",
        "workspace.report_pwd",
        "workspace.report_ports",
        "workspace.clear_ports",
        "workspace.report_tty",
        "workspace.ports_kick",
        "settings.open",
        "notification.create",
        "notification.create_for_surface",
        "notification.create_for_target",
        "notification.list",
        "notification.clear",
        "markdown.open",
        "window.new",
        "window.list",
        "window.current",
        "window.focus",
        "window.close",
    ];
    methods.extend_from_slice(&crate::socket::browser::method_names());
    Response::success(id, serde_json::json!({"methods": methods}))
}

pub(super) fn handle_system_identify(id: Value) -> Response {
    Response::success(
        id,
        serde_json::json!({
            "app": "cmux",
            "platform": "linux",
            "version": env!("CARGO_PKG_VERSION"),
        }),
    )
}

pub(super) fn handle_system_tree(id: Value, state: &Arc<SharedState>) -> Response {
    let tm = lock_or_recover(&state.tab_manager);
    let workspaces: Vec<Value> = tm
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            serde_json::json!({
                "index": i,
                "id": ws.id.to_string(),
                "title": ws.display_title(),
                "selected": tm.selected_index() == Some(i),
                "layout": ws.layout.to_json_tree(&ws.panels),
            })
        })
        .collect();

    Response::success(id, serde_json::json!({"workspaces": workspaces}))
}
