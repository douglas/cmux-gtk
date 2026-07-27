use std::sync::Arc;

use serde_json::Value;

use crate::app::{lock_or_recover, SharedState};
use crate::model::panel::SplitOrientation;
use crate::model::PanelType;

use super::helpers::*;
use super::Response;

pub(super) fn handle_pane_new(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let orientation = match params.get("orientation").and_then(|v| v.as_str()) {
        Some("horizontal") => SplitOrientation::Horizontal,
        Some("vertical") => SplitOrientation::Vertical,
        _ => SplitOrientation::Horizontal,
    };
    // Honour the requested panel type — the V1 `open_browser` shim sends
    // "panel_type", other callers send "type". Without this, `jmux browser`
    // opened a terminal instead of a browser. Defaults to a terminal.
    let panel_type = match params
        .get("panel_type")
        .or_else(|| params.get("type"))
        .and_then(|v| v.as_str())
    {
        Some("browser") => PanelType::Browser,
        _ => PanelType::Terminal,
    };
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut tm = lock_or_recover(&state.tab_manager);
    if let Some(ws) = tm.selected_mut() {
        let panel_id = ws.split(orientation, panel_type);
        if panel_type == PanelType::Browser {
            if let Some(panel) = ws.panels.get_mut(&panel_id) {
                panel.browser_url = url;
            }
        }
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"panel_id": panel_id.to_string()}))
    } else {
        Response::error(id, "not_found", "No workspace selected")
    }
}

/// Split the selected workspace and keep focus on the previously focused panel.
///
/// Params:
///   - `orientation` ("horizontal" | "vertical", default: "horizontal")
///
/// Unlike `pane.new`, this does NOT move focus to the new panel — the active
/// panel is unchanged after the split.
pub(super) fn handle_pane_split_off(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let orientation = match params.get("orientation").and_then(|v| v.as_str()) {
        Some("horizontal") => SplitOrientation::Horizontal,
        Some("vertical") => SplitOrientation::Vertical,
        _ => SplitOrientation::Horizontal,
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    if let Some(ws) = tm.selected_mut() {
        // Preserve focused panel before the split
        let focused_before = ws.focused_panel_id;
        let new_panel_id = ws.split(orientation, PanelType::Terminal);
        // Restore focus to the panel that was active before the split
        if let Some(pid) = focused_before {
            ws.focus_panel(pid);
        }
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"panel_id": new_panel_id.to_string()}))
    } else {
        Response::error(id, "not_found", "No workspace selected")
    }
}

pub(super) fn handle_pane_list(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace(wid)
    } else {
        tm.selected()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "Workspace not found");
    };

    let panels: Vec<Value> = ws
        .panel_ids()
        .iter()
        .map(|&pid| {
            let panel = ws.panels.get(&pid);
            let focused = ws.focused_panel_id == Some(pid);
            serde_json::json!({
                "id": pid.to_string(),
                "type": panel.map(|p| match p.panel_type {
                    crate::model::PanelType::Terminal => "terminal",
                    crate::model::PanelType::Browser => "browser",
                    crate::model::PanelType::Markdown => "markdown",
                    crate::model::PanelType::Diff => "diff",
                    crate::model::PanelType::Project => "project",
                    crate::model::PanelType::FilePreview => "file_preview",
                    crate::model::PanelType::Notes => "notes",
                    crate::model::PanelType::History => "history",
                    crate::model::PanelType::Vault => "vault",
                    crate::model::PanelType::AgentMonitor => "agent_monitor",
                    crate::model::PanelType::Graph => "graph",
                }).unwrap_or("unknown"),
                "title": panel.map(|p| p.display_title()).unwrap_or("?"),
                "directory": panel.and_then(|p| p.directory.as_deref()),
                "focused": focused,
            })
        })
        .collect();

    Response::success(id, serde_json::json!({"panels": panels}))
}

pub(super) fn handle_pane_focus(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match params
        .get("panel")
        .or_else(|| params.get("surface"))
        .or_else(|| params.get("id"))
        .and_then(|v| if v.is_null() { None } else { Some(v) })
    {
        Some(v) => {
            let Some(s) = v.as_str() else {
                return Response::error(id, "invalid_params", "panel/id must be a string");
            };
            match uuid::Uuid::parse_str(s) {
                Ok(uuid) => uuid,
                Err(_) => {
                    return Response::error(id, "invalid_params", "Invalid panel UUID format")
                }
            }
        }
        None => return Response::error(id, "invalid_params", "Provide 'panel' or 'id'"),
    };

    let focused = {
        let mut tm = lock_or_recover(&state.tab_manager);
        // First find which workspace contains this panel
        if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
            ws.focus_panel(panel_id)
        } else {
            false
        }
    };

    if focused {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"focused": true}))
    } else {
        Response::error(id, "not_found", "Panel not found")
    }
}

pub(super) fn handle_pane_close(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match params
        .get("panel")
        .or_else(|| params.get("surface"))
        .or_else(|| params.get("id"))
        .and_then(|v| if v.is_null() { None } else { Some(v) })
    {
        Some(v) => {
            let Some(s) = v.as_str() else {
                return Response::error(id, "invalid_params", "panel/id must be a string");
            };
            match uuid::Uuid::parse_str(s) {
                Ok(uuid) => Some(uuid),
                Err(_) => {
                    return Response::error(id, "invalid_params", "Invalid panel UUID format")
                }
            }
        }
        None => None,
    };

    let closed = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let target_panel_id = if let Some(pid) = panel_id {
            pid
        } else if let Some(ws) = tm.selected() {
            match ws.focused_panel_id {
                Some(pid) => pid,
                None => return Response::error(id, "not_found", "No focused panel"),
            }
        } else {
            return Response::error(id, "not_found", "No workspace selected");
        };

        tm.close_panel(target_panel_id)
    };

    if closed {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"closed": true}))
    } else {
        Response::error(id, "not_found", "Panel not found")
    }
}

pub(super) fn handle_pane_last(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace_mut(wid)
    } else {
        tm.selected_mut()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace");
    };

    let Some(prev_id) = ws.previous_focused_panel_id else {
        return Response::error(id, "not_found", "No previous panel");
    };

    if !ws.panels.contains_key(&prev_id) {
        return Response::error(id, "not_found", "Previous panel no longer exists");
    }

    ws.focus_panel(prev_id);
    drop(tm);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "panel_id": prev_id.to_string(),
            "focused": true,
        }),
    )
}

pub(super) fn handle_pane_swap(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let a_str = params
        .get("a")
        .or_else(|| params.get("panel_a"))
        .and_then(|v| v.as_str());
    let b_str = params
        .get("b")
        .or_else(|| params.get("panel_b"))
        .and_then(|v| v.as_str());

    let (Some(a_str), Some(b_str)) = (a_str, b_str) else {
        return Response::error(id, "invalid_params", "Provide 'a' and 'b' panel UUIDs");
    };

    let a = match uuid::Uuid::parse_str(a_str) {
        Ok(id) => id,
        Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID for 'a'"),
    };
    let b = match uuid::Uuid::parse_str(b_str) {
        Ok(id) => id,
        Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID for 'b'"),
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    let Some(ws) = tm.selected_mut() else {
        return Response::error(id, "not_found", "No workspace selected");
    };

    if !ws.panels.contains_key(&a) || !ws.panels.contains_key(&b) {
        return Response::error(id, "not_found", "One or both panels not found");
    }

    if ws.layout.swap_panels(a, b) {
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"swapped": true}))
    } else {
        Response::error(id, "not_found", "Panels not found in layout")
    }
}

pub(super) fn handle_pane_resize(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_str = params.get("panel").and_then(|v| v.as_str());
    let amount = params.get("amount").and_then(|v| v.as_f64());

    let Some(amount) = amount else {
        return Response::error(
            id,
            "invalid_params",
            "Provide 'amount' (e.g. 0.05 or -0.05)",
        );
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = tm.selected_mut();
    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace selected");
    };

    let panel_id = if let Some(s) = panel_str {
        match uuid::Uuid::parse_str(s) {
            Ok(id) => id,
            Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID"),
        }
    } else {
        let Some(pid) = ws.focused_panel_id else {
            return Response::error(id, "not_found", "No focused panel");
        };
        pid
    };

    if ws.layout.resize_panel(panel_id, amount) {
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"resized": true}))
    } else {
        Response::error(id, "not_found", "Panel not in any split")
    }
}

pub(super) fn handle_pane_focus_direction(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    use crate::model::panel::Direction;

    let dir_str = params.get("direction").and_then(|v| v.as_str());
    let Some(dir_str) = dir_str else {
        return Response::error(
            id,
            "invalid_params",
            "Provide 'direction': left, right, up, down",
        );
    };

    let direction = match dir_str {
        "left" => Direction::Left,
        "right" => Direction::Right,
        "up" => Direction::Up,
        "down" => Direction::Down,
        _ => {
            return Response::error(
                id,
                "invalid_params",
                "direction must be: left, right, up, down",
            )
        }
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    let Some(ws) = tm.selected_mut() else {
        return Response::error(id, "not_found", "No workspace selected");
    };
    let Some(current_id) = ws.focused_panel_id else {
        return Response::error(id, "not_found", "No focused panel");
    };

    let Some(neighbor_id) = ws.layout.neighbor(current_id, direction) else {
        return Response::error(id, "not_found", "No neighbor in that direction");
    };

    ws.focus_panel(neighbor_id);
    drop(tm);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "panel_id": neighbor_id.to_string(),
            "focused": true,
        }),
    )
}

pub(super) fn handle_pane_equalize(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace_mut(wid)
    } else {
        tm.selected_mut()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace");
    };

    ws.layout.equalize();
    drop(tm);
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"equalized": true}))
}

pub(super) fn handle_pane_break(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    use crate::model::Workspace;

    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let mut tm = lock_or_recover(&state.tab_manager);

    // Find the source workspace
    let source_ws = tm.find_workspace_with_panel_mut(panel_id);
    let Some(source_ws) = source_ws else {
        return Response::error(id, "not_found", "Panel not found in any workspace");
    };

    // Don't break if it's the only panel
    if source_ws.panels.len() <= 1 {
        return Response::error(
            id,
            "invalid_params",
            "Cannot break the only panel in a workspace",
        );
    }

    let source_ws_id = source_ws.id;
    let source_dir = source_ws.current_directory.clone();
    let panel = source_ws.detach_panel(panel_id);
    let Some(panel) = panel else {
        return Response::error(id, "not_found", "Panel not found");
    };

    // Auto-remove empty source workspace
    if tm.workspace(source_ws_id).is_some_and(|ws| ws.is_empty()) {
        tm.remove_by_id(source_ws_id);
    }

    // Create new workspace with the detached panel
    let mut new_ws = Workspace::new();
    // Remove the default panel that Workspace::new() creates
    let default_panel_id = new_ws.focused_panel_id;
    if let Some(dpid) = default_panel_id {
        new_ws.panels.remove(&dpid);
    }
    new_ws.current_directory = source_dir;
    new_ws.panels.insert(panel_id, panel);
    new_ws.layout = crate::model::panel::LayoutNode::single_pane(panel_id);
    new_ws.focused_panel_id = Some(panel_id);
    let new_ws_id = new_ws.id;
    tm.add_workspace(new_ws);

    drop(tm);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "panel_id": panel_id.to_string(),
            "workspace_id": new_ws_id.to_string(),
        }),
    )
}

pub(super) fn handle_pane_join(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .or_else(|| params.get("id"))
        .and_then(|v| v.as_str());

    let Some(panel_str) = panel_str else {
        return Response::error(id, "invalid_params", "Provide 'panel' UUID to join");
    };
    let panel_id = match uuid::Uuid::parse_str(panel_str) {
        Ok(pid) => pid,
        Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID"),
    };

    let orientation = match params.get("orientation").and_then(|v| v.as_str()) {
        Some("vertical") => SplitOrientation::Vertical,
        _ => SplitOrientation::Horizontal,
    };

    let mut tm = lock_or_recover(&state.tab_manager);

    let selected_ws_id = tm.selected_id();
    let Some(selected_ws_id) = selected_ws_id else {
        return Response::error(id, "not_found", "No workspace selected");
    };

    // Find the source workspace containing this panel
    let source_ws_id = tm.find_workspace_with_panel(panel_id).map(|ws| ws.id);
    let Some(source_ws_id) = source_ws_id else {
        return Response::error(id, "not_found", "Panel not found in any workspace");
    };

    // Can't join a panel into its own workspace
    if source_ws_id == selected_ws_id {
        return Response::error(
            id,
            "invalid_params",
            "Panel is already in the target workspace",
        );
    }

    // Detach from source
    let source_ws = tm
        .workspace_mut(source_ws_id)
        .expect("source workspace validated");
    let panel = source_ws.detach_panel(panel_id);
    let Some(panel) = panel else {
        return Response::error(id, "not_found", "Panel not found");
    };
    let source_empty = tm.workspace(source_ws_id).is_some_and(|ws| ws.is_empty());
    if source_empty {
        tm.remove_by_id(source_ws_id);
    }

    // Insert into target workspace
    let target_ws = tm
        .workspace_mut(selected_ws_id)
        .expect("target workspace validated");
    target_ws.insert_panel(panel, orientation);

    drop(tm);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "panel_id": panel_id.to_string(),
            "workspace_id": selected_ws_id.to_string(),
            "joined": true,
        }),
    )
}

pub(super) fn handle_pane_surfaces(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let tm = lock_or_recover(&state.tab_manager);
    let ws = tm.find_workspace_with_panel(panel_id);
    let Some(ws) = ws else {
        return Response::error(id, "not_found", "Panel not found in any workspace");
    };

    // Find the pane containing this panel and list all panel_ids in it
    let pane_panel_ids = if let Some(pane) = ws.layout.find_pane_with_panel_readonly(panel_id) {
        pane
    } else {
        vec![panel_id]
    };

    let surfaces: Vec<Value> = pane_panel_ids
        .iter()
        .map(|&pid| {
            let panel = ws.panels.get(&pid);
            serde_json::json!({
                "id": pid.to_string(),
                "type": panel.map(|p| match p.panel_type {
                    crate::model::PanelType::Terminal => "terminal",
                    crate::model::PanelType::Browser => "browser",
                    crate::model::PanelType::Markdown => "markdown",
                    crate::model::PanelType::Diff => "diff",
                    crate::model::PanelType::Project => "project",
                    crate::model::PanelType::FilePreview => "file_preview",
                    crate::model::PanelType::Notes => "notes",
                    crate::model::PanelType::History => "history",
                    crate::model::PanelType::Vault => "vault",
                    crate::model::PanelType::AgentMonitor => "agent_monitor",
                    crate::model::PanelType::Graph => "graph",
                }).unwrap_or("unknown"),
                "title": panel.map(|p| p.display_title()).unwrap_or("?"),
                "focused": ws.focused_panel_id == Some(pid),
            })
        })
        .collect();

    Response::success(id, serde_json::json!({"surfaces": surfaces}))
}

/// Move a panel to a specific workspace by UUID.
///
/// Params:
///   - `panel` (optional UUID): panel to move; defaults to the focused panel of
///     the currently selected workspace.
///   - `workspace` (required UUID): target workspace UUID.
pub(super) fn handle_panel_move_to_workspace(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let target_ws_str = params
        .get("workspace")
        .or_else(|| params.get("target_workspace"))
        .or_else(|| params.get("target_workspace_id"))
        .and_then(|v| v.as_str());

    let Some(target_ws_str) = target_ws_str else {
        return Response::error(id, "invalid_params", "Provide 'workspace' UUID");
    };

    let target_ws_id = match uuid::Uuid::parse_str(target_ws_str) {
        Ok(wid) => wid,
        Err(_) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let mut tm = lock_or_recover(&state.tab_manager);

    // Verify target exists
    if tm.workspace(target_ws_id).is_none() {
        return Response::error(id, "not_found", "Target workspace not found");
    }

    match tm.move_panel_to_workspace(panel_id, target_ws_id) {
        Some(wid) => {
            drop(tm);
            state.notify_ui_refresh();
            Response::success(
                id,
                serde_json::json!({
                    "panel_id": panel_id.to_string(),
                    "workspace_id": wid.to_string(),
                    "moved": true,
                }),
            )
        }
        None => Response::error(id, "invalid_params", "Cannot move panel (already in target or not found)"),
    }
}

/// Toggle the zoom state of a panel (expand to fill or restore split).
///
/// Params:
///   - `panel` (optional UUID): panel to zoom/unzoom; defaults to focused panel.
pub(super) fn handle_panel_toggle_zoom(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
        if ws.zoomed_panel_id == Some(panel_id) {
            ws.zoomed_panel_id = None;
        } else {
            ws.zoomed_panel_id = Some(panel_id);
        }
        let zoomed = ws.zoomed_panel_id.is_some();
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"zoomed": zoomed, "panel_id": panel_id.to_string()}))
    } else {
        Response::error(id, "not_found", "Panel not found in any workspace")
    }
}
