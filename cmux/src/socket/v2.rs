//! v2 JSON protocol dispatch.
//!
//! Request format:
//! ```json
//! {"id": "1", "method": "workspace.list", "params": {}}
//! ```
//!
//! Response format:
//! ```json
//! {"id": "1", "ok": true, "result": {...}}
//! ```

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::{lock_or_recover, SharedState, UiEvent};
use crate::model::panel::SplitOrientation;
use crate::model::PanelType;
use crate::model::Workspace;

/// V2 protocol request.
#[derive(Debug, Deserialize)]
pub struct Request {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// V2 protocol response.
#[derive(Debug, Serialize)]
pub struct Response {
    pub id: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

#[derive(Debug, Serialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
}

impl Response {
    fn success(id: Value, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: &str, message: &str) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(ErrorInfo {
                code: code.to_string(),
                message: message.to_string(),
            }),
        }
    }
}

/// Parse and dispatch a v2 request. Returns the response.
pub fn dispatch(json_line: &str, state: &Arc<SharedState>) -> Response {
    let req: Request = match serde_json::from_str(json_line) {
        Ok(r) => r,
        Err(e) => {
            return Response::error(Value::Null, "parse_error", &format!("Invalid JSON: {}", e));
        }
    };

    let id = req.id.clone();

    match req.method.as_str() {
        // System
        "system.ping" => Response::success(id, serde_json::json!({"pong": true})),
        "system.capabilities" => handle_capabilities(id),
        "system.identify" => handle_system_identify(id),
        "system.tree" => handle_system_tree(id, state),

        // Workspace commands
        "workspace.list" => handle_workspace_list(id, state),
        "workspace.new" => handle_workspace_new(id, &req.params, state),
        "workspace.create" => handle_workspace_create(id, &req.params, state),
        "workspace.select" => handle_workspace_select(id, &req.params, state),
        "workspace.next" => handle_workspace_next(id, &req.params, state),
        "workspace.previous" => handle_workspace_previous(id, &req.params, state),
        "workspace.last" => handle_workspace_last(id, state),
        "workspace.latest_unread" => handle_workspace_latest_unread(id, state),
        "workspace.close" => handle_workspace_close(id, &req.params, state),
        "workspace.set_status" => handle_workspace_set_status(id, &req.params, state),
        "workspace.report_git_branch" => handle_workspace_report_git(id, &req.params, state),
        "workspace.set_progress" => handle_workspace_set_progress(id, &req.params, state),
        "workspace.append_log" => handle_workspace_append_log(id, &req.params, state),
        "workspace.reorder" => handle_workspace_reorder(id, &req.params, state),
        "workspace.clear_status" => handle_workspace_clear_status(id, &req.params, state),
        "workspace.list_status" => handle_workspace_list_status(id, &req.params, state),
        "workspace.clear_progress" => handle_workspace_clear_progress(id, &req.params, state),
        "workspace.clear_log" => handle_workspace_clear_log(id, &req.params, state),
        "workspace.list_log" => handle_workspace_list_log(id, &req.params, state),

        // Workspace query commands
        "workspace.current" => handle_workspace_current(id, state),
        "workspace.rename" => handle_workspace_rename(id, &req.params, state),
        "workspace.action" => handle_workspace_action(id, &req.params, state),
        "workspace.report_pr" => handle_workspace_report_pr(id, &req.params, state),

        // Pane commands
        "pane.new" => handle_pane_new(id, &req.params, state),
        "pane.list" => handle_pane_list(id, &req.params, state),
        "pane.focus" => handle_pane_focus(id, &req.params, state),
        "pane.close" => handle_pane_close(id, &req.params, state),
        "pane.last" => handle_pane_last(id, &req.params, state),
        "pane.swap" => handle_pane_swap(id, &req.params, state),
        "pane.resize" => handle_pane_resize(id, &req.params, state),
        "pane.focus_direction" => handle_pane_focus_direction(id, &req.params, state),
        "pane.create" => handle_pane_new(id, &req.params, state),
        "pane.break" => handle_pane_break(id, &req.params, state),
        "pane.join" => handle_pane_join(id, &req.params, state),

        // Surface commands
        "surface.send_input" => handle_surface_send_input(id, &req.params, state),
        "surface.list" => handle_surface_list(id, &req.params, state),
        "surface.current" => handle_surface_current(id, state),
        "surface.focus" => handle_surface_focus(id, &req.params, state),
        "surface.split" => handle_pane_new(id, &req.params, state),
        "surface.close" => handle_pane_close(id, &req.params, state),
        "surface.action" => handle_surface_action(id, &req.params, state),
        "surface.health" => handle_surface_health(id, &req.params, state),
        "surface.send_key" => handle_surface_send_key(id, &req.params, state),
        "surface.read_text" => handle_surface_read_text(id, &req.params, state),
        "surface.refresh" => handle_surface_refresh(id, &req.params, state),
        "surface.clear_history" => handle_surface_clear_history(id, &req.params, state),
        "surface.trigger_flash" => handle_surface_trigger_flash(id, &req.params, state),

        // Settings
        "settings.open" => handle_settings_open(id, state),

        // Notification commands
        "notification.create" => handle_notification_create(id, &req.params, state),
        "notification.list" => handle_notification_list(id, state),
        "notification.clear" => handle_notification_clear(id, state),

        _ => Response::error(
            id,
            "unknown_method",
            &format!(
                "Unknown method: {}",
                crate::model::workspace::truncate_str(&req.method, 200)
            ),
        ),
    }
}

// -----------------------------------------------------------------------
// System handlers
// -----------------------------------------------------------------------

fn handle_capabilities(id: Value) -> Response {
    let methods = vec![
        "system.ping",
        "system.capabilities",
        "system.identify",
        "system.tree",
        "workspace.list",
        "workspace.new",
        "workspace.create",
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
        "workspace.action",
        "workspace.report_pr",
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
        "settings.open",
        "notification.create",
        "notification.list",
        "notification.clear",
    ];
    Response::success(id, serde_json::json!({"methods": methods}))
}

// -----------------------------------------------------------------------
// Workspace handlers
// -----------------------------------------------------------------------

fn handle_workspace_list(id: Value, state: &Arc<SharedState>) -> Response {
    let tm = lock_or_recover(&state.tab_manager);
    let workspaces: Vec<Value> = tm
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let selected = tm.selected_index() == Some(i);
            serde_json::json!({
                "index": i,
                "id": ws.id.to_string(),
                "title": ws.display_title(),
                "directory": ws.current_directory,
                "panel_count": ws.panels.len(),
                "unread_count": ws.unread_count,
                "latest_notification": ws.latest_notification,
                "attention_panel_id": ws.attention_panel_id.map(|id| id.to_string()),
                "selected": selected,
                "is_selected": selected,
            })
        })
        .collect();

    Response::success(id, serde_json::json!({"workspaces": workspaces}))
}

fn handle_workspace_new(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    create_workspace(id, params, state, false)
}

fn handle_workspace_create(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    create_workspace(id, params, state, true)
}

fn create_workspace(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
    preserve_selection: bool,
) -> Response {
    let directory = params
        .get("directory")
        .or_else(|| params.get("cwd"))
        .and_then(|v| v.as_str())
        .map(|s| crate::model::workspace::truncate_str(s, 4096));
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| crate::model::workspace::truncate_str(s, 1024));

    let mut ws = if let Some(dir) = directory {
        Workspace::with_directory(dir)
    } else {
        Workspace::new()
    };

    if let Some(t) = title {
        ws.custom_title = Some(t.to_string());
    }

    let ws_id = ws.id;
    let mut tab_manager = lock_or_recover(&state.tab_manager);
    let previously_selected = if preserve_selection {
        tab_manager.selected_id()
    } else {
        None
    };
    tab_manager.add_workspace(ws);
    if let Some(selected_id) = previously_selected {
        let _ = tab_manager.select_by_id(selected_id);
    }
    drop(tab_manager);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "workspace_id": ws_id.to_string(),
            "workspace": ws_id.to_string()
        }),
    )
}

fn handle_workspace_select(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let index = match parse_usize_param(&id, params, "index") {
        Ok(index) => index,
        Err(response) => return response,
    };
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let mut tm = lock_or_recover(&state.tab_manager);

    let selected = if let Some(idx) = index {
        tm.select(idx)
    } else if let Some(wid) = ws_id {
        tm.select_by_id(wid)
    } else {
        return Response::error(
            id,
            "invalid_params",
            "Provide 'index' or 'workspace'/'workspace_id'",
        );
    };

    if selected {
        let selected_workspace = tm.selected_id();
        drop(tm);
        if let Some(workspace_id) = selected_workspace {
            mark_workspace_read(state, workspace_id);
        }
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"selected": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_next(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let wrap = params.get("wrap").and_then(|v| v.as_bool()).unwrap_or(true);
    let selected_workspace = {
        let mut tm = lock_or_recover(&state.tab_manager);
        tm.select_next(wrap);
        tm.selected_id()
    };
    if let Some(workspace_id) = selected_workspace {
        mark_workspace_read(state, workspace_id);
    }
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}

fn handle_workspace_previous(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let wrap = params.get("wrap").and_then(|v| v.as_bool()).unwrap_or(true);
    let selected_workspace = {
        let mut tm = lock_or_recover(&state.tab_manager);
        tm.select_previous(wrap);
        tm.selected_id()
    };
    if let Some(workspace_id) = selected_workspace {
        mark_workspace_read(state, workspace_id);
    }
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}

fn handle_workspace_last(id: Value, state: &Arc<SharedState>) -> Response {
    let selected_workspace = {
        let mut tm = lock_or_recover(&state.tab_manager);
        tm.select_last();
        tm.selected_id()
    };
    if let Some(workspace_id) = selected_workspace {
        mark_workspace_read(state, workspace_id);
    }
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}

fn handle_workspace_latest_unread(id: Value, state: &Arc<SharedState>) -> Response {
    let selected_workspace = {
        let mut tm = lock_or_recover(&state.tab_manager);
        tm.select_latest_unread()
    };

    if let Some(workspace_id) = selected_workspace {
        mark_workspace_read(state, workspace_id);
        state.notify_ui_refresh();
        Response::success(
            id,
            serde_json::json!({
                "workspace_id": workspace_id.to_string(),
                "workspace": workspace_id.to_string(),
                "selected": true
            }),
        )
    } else {
        Response::error(id, "not_found", "No unread workspace")
    }
}

fn handle_workspace_close(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let index = match parse_usize_param(&id, params, "index") {
        Ok(index) => index,
        Err(response) => return response,
    };
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let removed = {
        let mut tm = lock_or_recover(&state.tab_manager);
        if let Some(idx) = index {
            tm.remove(idx).is_some()
        } else if let Some(wid) = ws_id {
            tm.remove_by_id(wid).is_some()
        } else if let Some(idx) = tm.selected_index() {
            tm.remove(idx).is_some()
        } else {
            false
        }
    };

    if removed {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"closed": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_set_status(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let key = params.get("key").and_then(|v| v.as_str());
    let value = params.get("value").and_then(|v| v.as_str());
    let icon = params.get("icon").and_then(|v| v.as_str());
    let color = params.get("color").and_then(|v| v.as_str());

    let (Some(key), Some(value)) = (key, value) else {
        return Response::error(id, "invalid_params", "Provide 'key' and 'value'");
    };

    let updated = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let ws = if let Some(wid) = ws_id {
            tm.workspace_mut(wid)
        } else {
            tm.selected_mut()
        };

        if let Some(ws) = ws {
            ws.set_status(key, value, icon, color);
            true
        } else {
            false
        }
    };

    if updated {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_report_git(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let branch = params.get("branch").and_then(|v| v.as_str());
    let is_dirty = params
        .get("is_dirty")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let Some(branch) = branch else {
        return Response::error(id, "invalid_params", "Provide 'branch'");
    };

    let updated = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let ws = if let Some(wid) = ws_id {
            tm.workspace_mut(wid)
        } else {
            tm.selected_mut()
        };

        if let Some(ws) = ws {
            ws.git_branch = Some(crate::model::panel::GitBranch {
                branch: crate::model::workspace::truncate_str(branch, 256).to_string(),
                is_dirty,
            });
            true
        } else {
            false
        }
    };

    if updated {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_set_progress(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let value = params.get("value").and_then(|v| v.as_f64());
    let label = params.get("label").and_then(|v| v.as_str());

    let updated = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let ws = if let Some(wid) = ws_id {
            tm.workspace_mut(wid)
        } else {
            tm.selected_mut()
        };

        if let Some(ws) = ws {
            if let Some(value) = value {
                ws.progress = Some(crate::model::workspace::Progress {
                    value,
                    label: label.map(|s| s.to_string()),
                });
            } else {
                ws.progress = None;
            }
            true
        } else {
            false
        }
    };

    if updated {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_append_log(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let message = params.get("message").and_then(|v| v.as_str());
    let level = params
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("info");
    let source = params.get("source").and_then(|v| v.as_str());

    let Some(message) = message else {
        return Response::error(id, "invalid_params", "Provide 'message'");
    };

    let updated = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let ws = if let Some(wid) = ws_id {
            tm.workspace_mut(wid)
        } else {
            tm.selected_mut()
        };

        if let Some(ws) = ws {
            ws.append_log(message, level, source);
            true
        } else {
            false
        }
    };

    if updated {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_clear_status(
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
    if let Some(ws) = ws {
        ws.status_entries.clear();
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_list_status(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.iter().find(|ws| ws.id == wid)
    } else {
        tm.selected()
    };
    if let Some(ws) = ws {
        let entries: Vec<Value> = ws
            .status_entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "value": e.value,
                    "icon": e.icon,
                    "color": e.color,
                    "timestamp": e.timestamp,
                })
            })
            .collect();
        Response::success(id, serde_json::json!({"entries": entries}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_clear_progress(
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
    if let Some(ws) = ws {
        ws.progress = None;
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_clear_log(
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
    if let Some(ws) = ws {
        ws.log_entries.clear();
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_list_log(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.iter().find(|ws| ws.id == wid)
    } else {
        tm.selected()
    };
    if let Some(ws) = ws {
        let entries: Vec<Value> = ws
            .log_entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "message": e.message,
                    "level": e.level,
                    "source": e.source,
                    "timestamp": e.timestamp,
                })
            })
            .collect();
        Response::success(id, serde_json::json!({"entries": entries}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

// -----------------------------------------------------------------------
// Pane handlers
// -----------------------------------------------------------------------

fn handle_pane_new(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let orientation = match params.get("orientation").and_then(|v| v.as_str()) {
        Some("horizontal") => SplitOrientation::Horizontal,
        Some("vertical") => SplitOrientation::Vertical,
        _ => SplitOrientation::Horizontal,
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    if let Some(ws) = tm.selected_mut() {
        let panel_id = ws.split(orientation, PanelType::Terminal);
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"panel_id": panel_id.to_string()}))
    } else {
        Response::error(id, "not_found", "No workspace selected")
    }
}

// -----------------------------------------------------------------------
// Surface handlers
// -----------------------------------------------------------------------

fn handle_surface_send_input(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(input) = params.get("input").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'input'");
    };
    // Limit input size to prevent unbounded memory growth via the channel
    let input = crate::model::workspace::truncate_str(input, 128 * 1024);

    let explicit_panel_id = match params.get("surface").or_else(|| params.get("panel")) {
        Some(v) => {
            let Some(s) = v.as_str() else {
                return Response::error(id, "invalid_params", "surface/panel must be a string");
            };
            match uuid::Uuid::parse_str(s) {
                Ok(uuid) => Some(uuid),
                Err(_) => {
                    return Response::error(
                        id,
                        "invalid_params",
                        "Invalid surface/panel UUID format",
                    )
                }
            }
        }
        None => None,
    };

    let panel_id = {
        let tab_manager = lock_or_recover(&state.tab_manager);
        if let Some(panel_id) = explicit_panel_id {
            if tab_manager.find_workspace_with_panel(panel_id).is_none() {
                return Response::error(id, "not_found", "Surface not found");
            }
            panel_id
        } else if let Some(workspace) = tab_manager.selected() {
            let Some(panel_id) = workspace
                .focused_panel_id
                .or_else(|| workspace.panel_ids().into_iter().next())
            else {
                return Response::error(id, "not_found", "No focused surface");
            };
            panel_id
        } else {
            return Response::error(id, "not_found", "No workspace selected");
        }
    };

    if !state.send_ui_event(UiEvent::SendInput {
        panel_id,
        text: input.to_string(),
    }) {
        return Response::error(id, "not_ready", "UI is not ready");
    }

    Response::success(
        id,
        serde_json::json!({
            "sent": true,
            "surface": panel_id.to_string(),
        }),
    )
}

// -----------------------------------------------------------------------
// Notification handlers
// -----------------------------------------------------------------------

fn handle_notification_create(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let title = crate::model::workspace::truncate_str(
        params
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("cmux"),
        1024,
    );
    let body = crate::model::workspace::truncate_str(
        params.get("body").and_then(|v| v.as_str()).unwrap_or(""),
        8192,
    );
    let workspace_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let panel_id = match params.get("surface").or_else(|| params.get("panel")) {
        Some(v) => {
            let Some(s) = v.as_str() else {
                return Response::error(id, "invalid_params", "surface/panel must be a string");
            };
            match uuid::Uuid::parse_str(s) {
                Ok(uuid) => Some(uuid),
                Err(_) => {
                    return Response::error(
                        id,
                        "invalid_params",
                        "Invalid surface/panel UUID format",
                    )
                }
            }
        }
        None => None,
    };
    let send_desktop = params
        .get("send_desktop")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let target = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let target_workspace_id = if let Some(workspace_id) = workspace_id {
            if tm.workspace(workspace_id).is_some() {
                Some(workspace_id)
            } else {
                return Response::error(id, "not_found", "Workspace not found");
            }
        } else if let Some(panel_id) = panel_id {
            tm.find_workspace_with_panel(panel_id).map(|ws| ws.id)
        } else {
            tm.selected_id()
        };

        let Some(target_workspace_id) = target_workspace_id else {
            return Response::error(id, "not_found", "No workspace selected");
        };

        let workspace = tm.workspace_mut(target_workspace_id).unwrap();
        let resolved_panel_id = panel_id.filter(|id| workspace.panels.contains_key(id));
        workspace.record_notification(title, body, resolved_panel_id);
        (target_workspace_id, resolved_panel_id)
    };

    let (target_workspace_id, resolved_panel_id) = target;
    lock_or_recover(&state.notifications).add(
        title,
        body,
        Some(target_workspace_id),
        resolved_panel_id,
        send_desktop,
    );
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "notified": true,
            "workspace": target_workspace_id.to_string(),
            "workspace_id": target_workspace_id.to_string(),
            "surface": resolved_panel_id.map(|panel_id| panel_id.to_string()),
        }),
    )
}

fn handle_notification_list(id: Value, state: &Arc<SharedState>) -> Response {
    let store = lock_or_recover(&state.notifications);
    let notifications: Vec<Value> = store
        .all()
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id.to_string(),
                "title": n.title,
                "body": n.body,
                "workspace_id": n.source_workspace_id.map(|id| id.to_string()),
                "panel_id": n.source_panel_id.map(|id| id.to_string()),
                "timestamp": n.timestamp,
                "is_read": n.is_read,
            })
        })
        .collect();
    Response::success(
        id,
        serde_json::json!({
            "notifications": notifications,
            "count": notifications.len(),
        }),
    )
}

fn handle_notification_clear(id: Value, state: &Arc<SharedState>) -> Response {
    lock_or_recover(&state.notifications).clear();
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}

// -----------------------------------------------------------------------
// Workspace query handlers
// -----------------------------------------------------------------------

fn handle_workspace_current(id: Value, state: &Arc<SharedState>) -> Response {
    let tm = lock_or_recover(&state.tab_manager);
    if let Some(ws) = tm.selected() {
        let index = tm.selected_index().unwrap_or(0);
        Response::success(
            id,
            serde_json::json!({
                "index": index,
                "id": ws.id.to_string(),
                "title": ws.display_title(),
                "directory": ws.current_directory,
                "panel_count": ws.panels.len(),
                "focused_panel_id": ws.focused_panel_id.map(|id| id.to_string()),
            }),
        )
    } else {
        Response::error(id, "not_found", "No workspace selected")
    }
}

fn handle_workspace_rename(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let title = params.get("title").and_then(|v| v.as_str());

    let Some(title) = title else {
        return Response::error(id, "invalid_params", "Provide 'title'");
    };

    let updated = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let ws = if let Some(wid) = ws_id {
            tm.workspace_mut(wid)
        } else {
            tm.selected_mut()
        };

        if let Some(ws) = ws {
            ws.custom_title = Some(
                crate::model::workspace::truncate_str(title, 1024).to_string(),
            );
            true
        } else {
            false
        }
    };

    if updated {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

fn handle_workspace_reorder(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let from = match parse_usize_param(&id, params, "from") {
        Ok(v) => v,
        Err(response) => return response,
    };
    let to = match parse_usize_param(&id, params, "to") {
        Ok(v) => v,
        Err(response) => return response,
    };

    let (Some(from), Some(to)) = (from, to) else {
        return Response::error(id, "invalid_params", "Provide 'from' and 'to'");
    };

    let moved = lock_or_recover(&state.tab_manager).move_workspace(from, to);
    if moved {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "invalid_params", "Invalid workspace indices")
    }
}

// -----------------------------------------------------------------------
// Pane list/focus/close handlers
// -----------------------------------------------------------------------

fn handle_pane_list(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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
                }).unwrap_or("unknown"),
                "title": panel.map(|p| p.display_title()).unwrap_or("?"),
                "directory": panel.and_then(|p| p.directory.as_deref()),
                "focused": focused,
            })
        })
        .collect();

    Response::success(id, serde_json::json!({"panels": panels}))
}

fn handle_pane_focus(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match params
        .get("panel")
        .or_else(|| params.get("surface"))
        .or_else(|| params.get("id"))
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

fn handle_pane_close(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match params
        .get("panel")
        .or_else(|| params.get("surface"))
        .or_else(|| params.get("id"))
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

        if let Some(ws) = tm.find_workspace_with_panel_mut(target_panel_id) {
            let removed = ws.remove_panel(target_panel_id);
            if removed && ws.is_empty() {
                let ws_id = ws.id;
                tm.remove_by_id(ws_id);
            }
            removed
        } else {
            false
        }
    };

    if closed {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"closed": true}))
    } else {
        Response::error(id, "not_found", "Panel not found")
    }
}

// -----------------------------------------------------------------------
// Surface list/current/focus handlers
// -----------------------------------------------------------------------

fn handle_surface_list(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    // Alias for pane.list
    handle_pane_list(id, params, state)
}

fn handle_surface_current(id: Value, state: &Arc<SharedState>) -> Response {
    let tm = lock_or_recover(&state.tab_manager);
    let Some(ws) = tm.selected() else {
        return Response::error(id, "not_found", "No workspace selected");
    };

    let Some(panel_id) = ws.focused_panel_id else {
        return Response::error(id, "not_found", "No focused surface");
    };

    let panel = ws.panels.get(&panel_id);
    Response::success(
        id,
        serde_json::json!({
            "id": panel_id.to_string(),
            "type": panel.map(|p| match p.panel_type {
                crate::model::PanelType::Terminal => "terminal",
                crate::model::PanelType::Browser => "browser",
            }).unwrap_or("unknown"),
            "title": panel.map(|p| p.display_title()).unwrap_or("?"),
            "directory": panel.and_then(|p| p.directory.as_deref()),
        }),
    )
}

fn handle_surface_focus(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    // Alias for pane.focus
    handle_pane_focus(id, params, state)
}

// -----------------------------------------------------------------------
// system.identify
// -----------------------------------------------------------------------

fn handle_system_identify(id: Value) -> Response {
    Response::success(
        id,
        serde_json::json!({
            "app": "cmux",
            "platform": "linux",
            "version": env!("CARGO_PKG_VERSION"),
        }),
    )
}

// -----------------------------------------------------------------------
// system.tree
// -----------------------------------------------------------------------

fn handle_system_tree(id: Value, state: &Arc<SharedState>) -> Response {
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

// -----------------------------------------------------------------------
// pane.last
// -----------------------------------------------------------------------

fn handle_pane_last(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

// -----------------------------------------------------------------------
// pane.swap
// -----------------------------------------------------------------------

fn handle_pane_swap(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let a_str = params.get("a").or_else(|| params.get("panel_a")).and_then(|v| v.as_str());
    let b_str = params.get("b").or_else(|| params.get("panel_b")).and_then(|v| v.as_str());

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

// -----------------------------------------------------------------------
// pane.resize
// -----------------------------------------------------------------------

fn handle_pane_resize(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_str = params.get("panel").and_then(|v| v.as_str());
    let amount = params.get("amount").and_then(|v| v.as_f64());

    let Some(amount) = amount else {
        return Response::error(id, "invalid_params", "Provide 'amount' (e.g. 0.05 or -0.05)");
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

// -----------------------------------------------------------------------
// pane.focus_direction
// -----------------------------------------------------------------------

fn handle_pane_focus_direction(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

// -----------------------------------------------------------------------
// workspace.action (pin/unpin/toggle_pin)
// -----------------------------------------------------------------------

fn handle_workspace_action(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let action = params.get("action").and_then(|v| v.as_str());

    let Some(action) = action else {
        return Response::error(id, "invalid_params", "Provide 'action'");
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

    let ws_id = ws.id;
    match action {
        "pin" => ws.is_pinned = true,
        "unpin" => ws.is_pinned = false,
        "toggle_pin" => ws.is_pinned = !ws.is_pinned,
        "mark_read" => {
            ws.mark_notifications_read();
            drop(tm);
            mark_workspace_read(state, ws_id);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"ok": true}));
        }
        "mark_unread" => {
            ws.unread_count = ws.unread_count.max(1);
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"ok": true}));
        }
        "clear_name" => {
            ws.custom_title = None;
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"ok": true}));
        }
        "set_color" => {
            let color = params.get("color").and_then(|v| v.as_str());
            let Some(color) = color else {
                return Response::error(
                    id,
                    "invalid_params",
                    "set_color requires 'color' param",
                );
            };
            ws.custom_color = Some(
                crate::model::workspace::truncate_str(color, 64).to_string(),
            );
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"ok": true}));
        }
        "clear_color" => {
            ws.custom_color = None;
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"ok": true}));
        }
        _ => {
            return Response::error(
                id,
                "invalid_params",
                "Unknown action. Use: pin, unpin, toggle_pin, mark_read, mark_unread, clear_name, set_color, clear_color",
            )
        }
    }

    let pinned = ws.is_pinned;
    drop(tm);
    state.notify_ui_refresh();

    Response::success(id, serde_json::json!({"is_pinned": pinned}))
}

// -----------------------------------------------------------------------
// settings.open
// -----------------------------------------------------------------------

fn handle_settings_open(id: Value, state: &Arc<SharedState>) -> Response {
    state.send_ui_event(UiEvent::OpenSettings);
    Response::success(id, serde_json::json!({"opened": true}))
}

// -----------------------------------------------------------------------
// surface.trigger_flash
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// workspace.report_pr
// -----------------------------------------------------------------------

fn handle_workspace_report_pr(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let status = params.get("status").and_then(|v| v.as_str());
    let url = params.get("url").and_then(|v| v.as_str());

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace_mut(wid)
    } else {
        tm.selected_mut()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace");
    };

    ws.pr_status = status.map(|s| {
        crate::model::workspace::truncate_str(s, 64).to_string()
    });
    ws.pr_url = url.map(|s| {
        crate::model::workspace::truncate_str(s, 1024).to_string()
    });

    drop(tm);
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"updated": true}))
}

// -----------------------------------------------------------------------
// surface.send_key
// -----------------------------------------------------------------------

fn handle_surface_send_key(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let key_name = params.get("key").and_then(|v| v.as_str());
    let mods_arr = params.get("mods").and_then(|v| v.as_array());

    let Some(key_name) = key_name else {
        return Response::error(id, "invalid_params", "Provide 'key' (e.g. 'c', 'Return', 'Escape')");
    };

    // Parse modifier names to ghostty mods bitmask
    let mut mods: u32 = 0;
    if let Some(arr) = mods_arr {
        for m in arr {
            if let Some(s) = m.as_str() {
                match s.to_lowercase().as_str() {
                    "ctrl" | "control" => {
                        mods |=
                            ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_CTRL as u32;
                    }
                    "shift" => {
                        mods |=
                            ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_SHIFT as u32;
                    }
                    "alt" => {
                        mods |=
                            ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_ALT as u32;
                    }
                    "super" | "meta" => {
                        mods |=
                            ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_SUPER as u32;
                    }
                    _ => {}
                }
            }
        }
    }

    // Convert key name to GDK keyval. Try the name directly first,
    // then try common aliases.
    let keyval = resolve_key_name(key_name);
    let Some(keyval) = keyval else {
        return Response::error(
            id,
            "invalid_params",
            &format!("Unknown key name: '{key_name}'"),
        );
    };

    // Resolve the panel
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str());
    let panel_id = if let Some(s) = panel_str {
        match uuid::Uuid::parse_str(s) {
            Ok(pid) => pid,
            Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID"),
        }
    } else {
        let tm = lock_or_recover(&state.tab_manager);
        let Some(ws) = tm.selected() else {
            return Response::error(id, "not_found", "No workspace selected");
        };
        let Some(pid) = ws.focused_panel_id else {
            return Response::error(id, "not_found", "No focused panel");
        };
        pid
    };

    state.send_ui_event(UiEvent::SendKey {
        panel_id,
        keyval,
        keycode: 0,
        mods,
    });
    Response::success(id, serde_json::json!({"sent": true}))
}

/// Resolve a key name string to a GDK keyval.
fn resolve_key_name(name: &str) -> Option<u32> {
    // Single character → use its unicode value as keyval
    let mut chars = name.chars();
    if let Some(ch) = chars.next() {
        if chars.next().is_none() && ch.is_ascii() {
            // Single ASCII char: GDK keyvals for ASCII match the codepoint
            // for a-z, 0-9, punctuation
            return Some(ch as u32);
        }
    }

    // Common key name aliases
    match name.to_lowercase().as_str() {
        "return" | "enter" => Some(0xff0d),
        "escape" | "esc" => Some(0xff1b),
        "tab" => Some(0xff09),
        "backspace" => Some(0xff08),
        "delete" | "del" => Some(0xffff),
        "space" => Some(0x0020),
        "up" | "arrow_up" => Some(0xff52),
        "down" | "arrow_down" => Some(0xff54),
        "left" | "arrow_left" => Some(0xff51),
        "right" | "arrow_right" => Some(0xff53),
        "home" => Some(0xff50),
        "end" => Some(0xff57),
        "page_up" | "pageup" => Some(0xff55),
        "page_down" | "pagedown" => Some(0xff56),
        "insert" => Some(0xff63),
        "f1" => Some(0xffbe),
        "f2" => Some(0xffbf),
        "f3" => Some(0xffc0),
        "f4" => Some(0xffc1),
        "f5" => Some(0xffc2),
        "f6" => Some(0xffc3),
        "f7" => Some(0xffc4),
        "f8" => Some(0xffc5),
        "f9" => Some(0xffc6),
        "f10" => Some(0xffc7),
        "f11" => Some(0xffc8),
        "f12" => Some(0xffc9),
        _ => None,
    }
}

// -----------------------------------------------------------------------
// surface.read_text
// -----------------------------------------------------------------------

fn handle_surface_read_text(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str());

    let panel_id = if let Some(s) = panel_str {
        match uuid::Uuid::parse_str(s) {
            Ok(pid) => pid,
            Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID"),
        }
    } else {
        let tm = lock_or_recover(&state.tab_manager);
        let Some(ws) = tm.selected() else {
            return Response::error(id, "not_found", "No workspace selected");
        };
        let Some(pid) = ws.focused_panel_id else {
            return Response::error(id, "not_found", "No focused panel");
        };
        pid
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    state.send_ui_event(UiEvent::ReadText {
        panel_id,
        reply: tx,
    });

    // Block waiting for the GTK thread to reply.
    // The socket handler runs on a tokio thread so this is safe.
    match rx.blocking_recv() {
        Ok(Some(text)) => Response::success(
            id,
            serde_json::json!({
                "text": text,
            }),
        ),
        Ok(None) => Response::error(id, "not_found", "Surface not ready or not found"),
        Err(_) => Response::error(id, "internal", "GTK thread did not reply"),
    }
}

// -----------------------------------------------------------------------
// pane.break — detach focused pane to a new workspace
// -----------------------------------------------------------------------

fn handle_pane_break(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

// -----------------------------------------------------------------------
// pane.join — move a pane into the current workspace
// -----------------------------------------------------------------------

fn handle_pane_join(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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
    let source_ws_id = tm
        .find_workspace_with_panel(panel_id)
        .map(|ws| ws.id);
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
    let source_ws = tm.workspace_mut(source_ws_id).unwrap();
    let panel = source_ws.detach_panel(panel_id);
    let Some(panel) = panel else {
        return Response::error(id, "not_found", "Panel not found");
    };
    let source_empty = tm
        .workspace(source_ws_id)
        .is_some_and(|ws| ws.is_empty());
    if source_empty {
        tm.remove_by_id(source_ws_id);
    }

    // Insert into target workspace
    let target_ws = tm.workspace_mut(selected_ws_id).unwrap();
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

// -----------------------------------------------------------------------
// surface.action — named actions on a surface
// -----------------------------------------------------------------------

fn handle_surface_action(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let action = params.get("action").and_then(|v| v.as_str());
    let Some(action) = action else {
        return Response::error(id, "invalid_params", "Provide 'action'");
    };

    match action {
        "toggle_zoom" => {
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
                Response::success(id, serde_json::json!({"zoomed": zoomed}))
            } else {
                Response::error(id, "not_found", "Panel not found")
            }
        }
        "clear_screen" => {
            state.send_ui_event(UiEvent::ClearHistory { panel_id });
            Response::success(id, serde_json::json!({"cleared": true}))
        }
        "refresh" => {
            state.send_ui_event(UiEvent::RefreshSurface { panel_id });
            Response::success(id, serde_json::json!({"refreshed": true}))
        }
        "flash" => {
            state.send_ui_event(UiEvent::TriggerFlash { panel_id });
            Response::success(id, serde_json::json!({"flashed": true}))
        }
        _ => Response::error(
            id,
            "invalid_params",
            "Unknown action. Use: toggle_zoom, clear_screen, refresh, flash",
        ),
    }
}

// -----------------------------------------------------------------------
// surface.health — report surface readiness
// -----------------------------------------------------------------------

fn handle_surface_health(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let tm = lock_or_recover(&state.tab_manager);
    let exists = tm.find_workspace_with_panel(panel_id).is_some();
    drop(tm);

    Response::success(
        id,
        serde_json::json!({
            "panel_id": panel_id.to_string(),
            "exists": exists,
            "healthy": exists,
        }),
    )
}

// -----------------------------------------------------------------------
// surface.refresh / surface.clear_history
// -----------------------------------------------------------------------

fn handle_surface_refresh(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = resolve_panel_id(&id, params, state);
    let panel_id = match panel_id {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    state.send_ui_event(UiEvent::RefreshSurface { panel_id });
    Response::success(id, serde_json::json!({"refreshed": true}))
}

fn handle_surface_clear_history(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = resolve_panel_id(&id, params, state);
    let panel_id = match panel_id {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    state.send_ui_event(UiEvent::ClearHistory { panel_id });
    Response::success(id, serde_json::json!({"cleared": true}))
}

fn handle_surface_trigger_flash(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str());

    let panel_id = if let Some(s) = panel_str {
        match uuid::Uuid::parse_str(s) {
            Ok(id) => id,
            Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID"),
        }
    } else {
        let tm = lock_or_recover(&state.tab_manager);
        let Some(ws) = tm.selected() else {
            return Response::error(id, "not_found", "No workspace selected");
        };
        let Some(pid) = ws.focused_panel_id else {
            return Response::error(id, "not_found", "No focused panel");
        };
        pid
    };

    state.send_ui_event(UiEvent::TriggerFlash { panel_id });
    Response::success(id, serde_json::json!({"flashed": true}))
}

/// Resolve a panel UUID from `panel` or `surface` params, falling back to the
/// focused panel in the selected workspace.
fn resolve_panel_id(
    id: &Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Result<uuid::Uuid, Response> {
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str());

    if let Some(s) = panel_str {
        uuid::Uuid::parse_str(s)
            .map_err(|_| Response::error(id.clone(), "invalid_params", "Invalid panel UUID"))
    } else {
        let tm = lock_or_recover(&state.tab_manager);
        let ws = tm
            .selected()
            .ok_or_else(|| Response::error(id.clone(), "not_found", "No workspace selected"))?;
        ws.focused_panel_id
            .ok_or_else(|| Response::error(id.clone(), "not_found", "No focused panel"))
    }
}

fn mark_workspace_read(state: &Arc<SharedState>, workspace_id: uuid::Uuid) {
    lock_or_recover(&state.notifications).mark_workspace_read(workspace_id);

    if let Some(workspace) = lock_or_recover(&state.tab_manager).workspace_mut(workspace_id) {
        workspace.mark_notifications_read();
    }
}

/// Parse a workspace UUID from `workspace` or `workspace_id` params.
/// Returns `Err(())` if the key exists but the value is not a valid UUID.
/// Returns `Ok(None)` if neither key is present.
fn parse_workspace_param(params: &Value) -> Result<Option<uuid::Uuid>, ()> {
    let val = params
        .get("workspace")
        .or_else(|| params.get("workspace_id"));
    match val {
        Some(v) if v.is_null() => Ok(None),
        Some(v) => match v.as_str().map(uuid::Uuid::parse_str) {
            Some(Ok(id)) => Ok(Some(id)),
            _ => Err(()),
        },
        None => Ok(None),
    }
}

fn parse_usize_param(id: &Value, params: &Value, key: &str) -> Result<Option<usize>, Response> {
    match params.get(key) {
        Some(v) => match v.as_u64() {
            Some(value) => usize::try_from(value).map(Some).map_err(|_| {
                Response::error(
                    id.clone(),
                    "invalid_params",
                    &format!("'{key}' is out of range"),
                )
            }),
            None => Err(Response::error(
                id.clone(),
                "invalid_params",
                &format!("'{key}' must be a non-negative integer"),
            )),
        },
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_create_updates_workspace_attention() {
        let state = Arc::new(SharedState::new());
        let (workspace_id, panel_id) = {
            let tab_manager = lock_or_recover(&state.tab_manager);
            let workspace = tab_manager.selected().unwrap();
            (workspace.id, workspace.focused_panel_id.unwrap())
        };

        let request = serde_json::json!({
            "id": 1,
            "method": "notification.create",
            "params": {
                "title": "Codex",
                "body": "Waiting for input",
                "workspace": workspace_id.to_string(),
                "surface": panel_id.to_string(),
                "send_desktop": false
            }
        });

        let response = dispatch(&request.to_string(), &state);
        assert!(response.ok);

        let tab_manager = lock_or_recover(&state.tab_manager);
        let workspace = tab_manager.workspace(workspace_id).unwrap();
        assert_eq!(workspace.unread_count, 1);
        assert_eq!(
            workspace.latest_notification.as_deref(),
            Some("Codex: Waiting for input")
        );
        assert_eq!(workspace.attention_panel_id, Some(panel_id));
    }

    #[test]
    fn test_workspace_latest_unread_selects_newest_workspace() {
        let state = Arc::new(SharedState::new());
        let workspace_one_id = lock_or_recover(&state.tab_manager).selected_id().unwrap();

        let new_workspace_request = serde_json::json!({
            "id": 1,
            "method": "workspace.new",
            "params": {
                "title": "Second"
            }
        });
        let response = dispatch(&new_workspace_request.to_string(), &state);
        assert!(response.ok);

        let workspace_two_id = lock_or_recover(&state.tab_manager).selected_id().unwrap();

        let first_notification = serde_json::json!({
            "id": 2,
            "method": "notification.create",
            "params": {
                "title": "Claude Code",
                "body": "Needs approval",
                "workspace": workspace_one_id.to_string(),
                "send_desktop": false
            }
        });
        assert!(dispatch(&first_notification.to_string(), &state).ok);

        std::thread::sleep(std::time::Duration::from_millis(1));

        let second_notification = serde_json::json!({
            "id": 3,
            "method": "notification.create",
            "params": {
                "title": "Codex",
                "body": "Waiting for input",
                "workspace": workspace_two_id.to_string(),
                "send_desktop": false
            }
        });
        assert!(dispatch(&second_notification.to_string(), &state).ok);

        let latest_unread = serde_json::json!({
            "id": 4,
            "method": "workspace.latest_unread",
            "params": {}
        });
        let response = dispatch(&latest_unread.to_string(), &state);
        assert!(response.ok);

        let tab_manager = lock_or_recover(&state.tab_manager);
        assert_eq!(tab_manager.selected_id(), Some(workspace_two_id));
        assert_eq!(
            tab_manager
                .workspace(workspace_two_id)
                .unwrap()
                .unread_count,
            0
        );
        assert_eq!(
            tab_manager
                .workspace(workspace_one_id)
                .unwrap()
                .unread_count,
            1
        );
    }

    #[test]
    fn test_surface_send_input_dispatches_ui_event() {
        let state = Arc::new(SharedState::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        state.install_ui_event_sender(tx);

        let panel_id = {
            let tab_manager = lock_or_recover(&state.tab_manager);
            tab_manager.selected().unwrap().focused_panel_id.unwrap()
        };

        let request = serde_json::json!({
            "id": 1,
            "method": "surface.send_input",
            "params": {
                "surface": panel_id.to_string(),
                "input": "ls\n"
            }
        });

        let response = dispatch(&request.to_string(), &state);
        assert!(response.ok);

        let event = rx.try_recv().expect("expected a UI event");
        match event {
            UiEvent::SendInput {
                panel_id: actual,
                text,
            } => {
                assert_eq!(actual, panel_id);
                assert_eq!(text, "ls\n");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn test_workspace_create_alias_and_legacy_response_field() {
        let state = Arc::new(SharedState::new());
        let selected_before = lock_or_recover(&state.tab_manager).selected_id();

        let response = dispatch(
            r#"{"id":1,"method":"workspace.create","params":{"title":"Legacy"}}"#,
            &state,
        );

        assert!(response.ok);
        let result = response.result.unwrap();
        let workspace_id = result
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .expect("legacy workspace_id should be present");
        assert_eq!(
            result.get("workspace").and_then(|v| v.as_str()),
            Some(workspace_id)
        );
        assert_eq!(
            lock_or_recover(&state.tab_manager).selected_id(),
            selected_before
        );
    }

    #[test]
    fn test_workspace_list_keeps_selected_alias() {
        let state = Arc::new(SharedState::new());

        let response = dispatch(r#"{"id":1,"method":"workspace.list","params":{}}"#, &state);

        assert!(response.ok);
        let result = response.result.unwrap();
        let workspaces = result["workspaces"].as_array().expect("workspaces array");
        let first = &workspaces[0];
        assert_eq!(first.get("selected").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            first.get("is_selected").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn test_workspace_select_accepts_legacy_workspace_id_param() {
        let state = Arc::new(SharedState::new());
        let workspace_id = lock_or_recover(&state.tab_manager).selected_id().unwrap();

        let response = dispatch(
            &serde_json::json!({
                "id": 1,
                "method": "workspace.select",
                "params": {
                    "workspace_id": workspace_id.to_string()
                }
            })
            .to_string(),
            &state,
        );

        assert!(response.ok);
        assert_eq!(
            lock_or_recover(&state.tab_manager).selected_id(),
            Some(workspace_id)
        );
    }

    #[test]
    fn test_workspace_create_accepts_legacy_cwd_param() {
        let state = Arc::new(SharedState::new());

        let response = dispatch(
            r#"{"id":1,"method":"workspace.create","params":{"cwd":"/tmp/cmux-legacy"}}"#,
            &state,
        );

        assert!(response.ok);
        let workspace_id = response.result.as_ref().unwrap()["workspace_id"]
            .as_str()
            .expect("workspace_id should be present");
        let workspace_id = uuid::Uuid::parse_str(workspace_id).expect("valid uuid");

        let tab_manager = lock_or_recover(&state.tab_manager);
        let workspace = tab_manager
            .workspace(workspace_id)
            .expect("workspace should exist");
        assert_eq!(workspace.current_directory, "/tmp/cmux-legacy");
    }
}
