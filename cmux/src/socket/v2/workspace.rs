//! Workspace handler functions for the v2 socket protocol.

use std::sync::Arc;

use serde_json::Value;
use uuid::Uuid;

use crate::app::{lock_or_recover, SharedState, UiEvent};
use crate::model::workspace::truncate_str;
use crate::model::Workspace;

use super::helpers::*;
use super::{
    Response, MAX_BRANCH_LEN, MAX_DIRECTORY_LEN, MAX_METHOD_LEN, MAX_NAME_LEN, MAX_STATUS_LEN,
    MAX_TITLE_LEN, MAX_URL_LEN,
};

// -----------------------------------------------------------------------
// workspace.list
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_list(id: Value, state: &Arc<SharedState>) -> Response {
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

// -----------------------------------------------------------------------
// workspace.new / workspace.create
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_new(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    create_workspace(id, params, state, false)
}

pub(super) fn handle_workspace_create(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    create_workspace(id, params, state, true)
}

pub(super) fn create_workspace(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
    preserve_selection: bool,
) -> Response {
    let directory = params
        .get("directory")
        .or_else(|| params.get("cwd"))
        .and_then(|v| v.as_str())
        .map(|s| truncate_str(s, MAX_DIRECTORY_LEN));
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| truncate_str(s, MAX_TITLE_LEN));

    let mut ws = if let Some(dir) = directory {
        Workspace::with_directory(dir)
    } else {
        Workspace::new()
    };

    if let Some(t) = title {
        ws.custom_title = Some(t.to_string());
    }

    // Set command on the first panel if provided
    let command = params
        .get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(ref cmd) = command {
        if let Some(panel) = ws.panels.values_mut().next() {
            panel.command = Some(cmd.clone());
        }
    }

    let ws_id = ws.id;
    let mut tab_manager = lock_or_recover(&state.tab_manager);
    let previously_selected = if preserve_selection {
        tab_manager.selected_id()
    } else {
        None
    };
    let placement = crate::settings::load().new_workspace_placement;
    tab_manager.add_workspace_with_placement(ws, placement);
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

// -----------------------------------------------------------------------
// workspace.create_ssh
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_create_ssh(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let destination = match params.get("destination").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => return Response::error(id, "invalid_params", "destination is required"),
    };

    let port = params
        .get("port")
        .and_then(|v| v.as_u64())
        .map(|p| p as u16);
    let identity = params
        .get("identity")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Build SSH command (shell-escape user-supplied values to prevent injection)
    let mut ssh_cmd = "ssh".to_string();
    if let Some(p) = port {
        ssh_cmd += &format!(" -p {}", p);
    }
    if let Some(ref i) = identity {
        ssh_cmd += &format!(" -i {}", shell_escape::escape(i.into()));
    }
    ssh_cmd += &format!(" {}", shell_escape::escape(destination.into()));

    // Build remote config
    let remote_config = crate::remote::session::RemoteConfig {
        destination: destination.to_string(),
        port,
        identity,
        ssh_options: Vec::new(),
        remote_daemon_path: None,
    };

    // Create workspace with SSH command and title
    let mut create_params = params.clone();
    create_params["command"] = serde_json::json!(ssh_cmd);
    if create_params
        .get("title")
        .and_then(|v| v.as_str())
        .is_none()
    {
        create_params["title"] = serde_json::json!(destination);
    }

    let response = create_workspace(id, &create_params, state, false);

    // Store remote config on the workspace
    if response.ok {
        if let Some(ws_id_str) = response
            .result
            .as_ref()
            .and_then(|r| r.get("workspace_id"))
            .and_then(|v| v.as_str())
        {
            if let Ok(ws_id) = Uuid::parse_str(ws_id_str) {
                let mut tm = lock_or_recover(&state.tab_manager);
                if let Some(ws) = tm.workspace_mut(ws_id) {
                    ws.remote_config = Some(remote_config);
                }
            }
        }
    }

    response
}

// -----------------------------------------------------------------------
// workspace.remote_status
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_remote_status(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = parse_workspace_param(params).ok().flatten();

    let tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace(wid)
    } else {
        tm.selected()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace found");
    };

    let config = ws.remote_config.as_ref().map(|c| {
        serde_json::json!({
            "destination": c.destination,
            "port": c.port,
            "identity": c.identity,
        })
    });

    let state_json = ws.remote_state.as_ref().map(|s| match s {
        crate::remote::session::RemoteState::Disconnected => serde_json::json!("disconnected"),
        crate::remote::session::RemoteState::Connecting => serde_json::json!("connecting"),
        crate::remote::session::RemoteState::Connected {
            proxy_port,
            daemon_version,
        } => {
            serde_json::json!({
                "state": "connected",
                "proxy_port": proxy_port,
                "daemon_version": daemon_version,
            })
        }
        crate::remote::session::RemoteState::Error(msg) => serde_json::json!({
            "state": "error",
            "message": msg,
        }),
    });

    Response::success(
        id,
        serde_json::json!({
            "is_remote": ws.remote_config.is_some(),
            "config": config,
            "remote_state": state_json,
        }),
    )
}

// -----------------------------------------------------------------------
// workspace.select
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_select(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

// -----------------------------------------------------------------------
// workspace.next / workspace.previous / workspace.last
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_next(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

pub(super) fn handle_workspace_previous(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

pub(super) fn handle_workspace_last(id: Value, state: &Arc<SharedState>) -> Response {
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

// -----------------------------------------------------------------------
// workspace.latest_unread
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_latest_unread(id: Value, state: &Arc<SharedState>) -> Response {
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

// -----------------------------------------------------------------------
// workspace.close
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_close(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

// -----------------------------------------------------------------------
// workspace.set_status / workspace.clear_status / workspace.list_status
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_set_status(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let key = params.get("key").and_then(|v| v.as_str());
    let value = params.get("value").and_then(|v| v.as_str());
    let icon = params.get("icon").and_then(|v| v.as_str());
    let color = params.get("color").and_then(|v| v.as_str());
    let url = params.get("url").and_then(|v| v.as_str());

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
            ws.set_status_with_url(key, value, icon, color, url);
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

pub(super) fn handle_workspace_clear_status(
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

pub(super) fn handle_workspace_list_status(
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
                    "url": e.url,
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
// workspace.report_git
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_report_git(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let branch = params.get("branch").and_then(|v| v.as_str());
    let is_dirty = params
        .get("is_dirty")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let surface_id = params
        .get("surface")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let Some(branch) = branch else {
        return Response::error(id, "invalid_params", "Provide 'branch'");
    };

    let git_branch = if branch.is_empty() {
        None
    } else {
        Some(crate::model::panel::GitBranch {
            branch: truncate_str(branch, MAX_BRANCH_LEN).to_string(),
            is_dirty,
        })
    };

    let updated = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let ws = if let Some(wid) = ws_id {
            tm.workspace_mut(wid)
        } else {
            tm.selected_mut()
        };

        if let Some(ws) = ws {
            // Always update workspace-level branch
            ws.git_branch = git_branch.clone();
            // Also update per-panel branch when surface ID is provided
            if let Some(pid) = surface_id {
                if let Some(panel) = ws.panels.get_mut(&pid) {
                    panel.git_branch = git_branch;
                }
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

// -----------------------------------------------------------------------
// workspace.set_progress / workspace.clear_progress
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_set_progress(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

pub(super) fn handle_workspace_clear_progress(
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

// -----------------------------------------------------------------------
// workspace.append_log / workspace.clear_log / workspace.list_log
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_append_log(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

pub(super) fn handle_workspace_clear_log(
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

pub(super) fn handle_workspace_list_log(
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
// Metadata entry handlers
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_report_meta(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let key = params.get("key").and_then(|v| v.as_str());
    let value = params.get("value").and_then(|v| v.as_str());
    let (Some(key), Some(value)) = (key, value) else {
        return Response::error(id, "invalid_params", "Provide 'key' and 'value'");
    };

    let icon = params.get("icon").and_then(|v| v.as_str());
    let color = params.get("color").and_then(|v| v.as_str());
    let url = params.get("url").and_then(|v| v.as_str());
    let priority = params.get("priority").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let format = match params.get("format").and_then(|v| v.as_str()) {
        Some("markdown") => crate::model::workspace::MetadataFormat::Markdown,
        _ => crate::model::workspace::MetadataFormat::Plain,
    };

    let updated = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let ws = if let Some(wid) = ws_id {
            tm.workspace_mut(wid)
        } else {
            tm.selected_mut()
        };
        if let Some(ws) = ws {
            ws.set_metadata(key, value, icon, color, url, priority, format);
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

pub(super) fn handle_workspace_clear_meta(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let key = params.get("key").and_then(|v| v.as_str());

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace_mut(wid)
    } else {
        tm.selected_mut()
    };
    if let Some(ws) = ws {
        if let Some(key) = key {
            ws.clear_metadata(key);
        } else {
            ws.metadata_entries.clear();
        }
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

pub(super) fn handle_workspace_list_meta(
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
        let mut entries: Vec<&crate::model::workspace::MetadataEntry> =
            ws.metadata_entries.iter().collect();
        entries.sort_by(|a, b| {
            b.priority.cmp(&a.priority).then(
                a.timestamp
                    .partial_cmp(&b.timestamp)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        let entries: Vec<Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "value": e.value,
                    "icon": e.icon,
                    "color": e.color,
                    "url": e.url,
                    "priority": e.priority,
                    "format": e.format,
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
// Metadata block handlers
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_report_meta_block(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let key = params.get("key").and_then(|v| v.as_str());
    let content = params.get("content").and_then(|v| v.as_str());
    let (Some(key), Some(content)) = (key, content) else {
        return Response::error(id, "invalid_params", "Provide 'key' and 'content'");
    };
    let priority = params.get("priority").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

    let updated = {
        let mut tm = lock_or_recover(&state.tab_manager);
        let ws = if let Some(wid) = ws_id {
            tm.workspace_mut(wid)
        } else {
            tm.selected_mut()
        };
        if let Some(ws) = ws {
            ws.set_metadata_block(key, content, priority);
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

pub(super) fn handle_workspace_clear_meta_block(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let key = params.get("key").and_then(|v| v.as_str());

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace_mut(wid)
    } else {
        tm.selected_mut()
    };
    if let Some(ws) = ws {
        if let Some(key) = key {
            ws.clear_metadata_block(key);
        } else {
            ws.metadata_blocks.clear();
        }
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

pub(super) fn handle_workspace_list_meta_blocks(
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
        let mut blocks: Vec<&crate::model::workspace::MetadataBlock> =
            ws.metadata_blocks.iter().collect();
        blocks.sort_by(|a, b| {
            b.priority.cmp(&a.priority).then(
                a.timestamp
                    .partial_cmp(&b.timestamp)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        let blocks: Vec<Value> = blocks
            .iter()
            .map(|b| {
                serde_json::json!({
                    "key": b.key,
                    "content": b.content,
                    "priority": b.priority,
                    "timestamp": b.timestamp,
                })
            })
            .collect();
        Response::success(id, serde_json::json!({"blocks": blocks}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

// -----------------------------------------------------------------------
// workspace.current
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_current(id: Value, state: &Arc<SharedState>) -> Response {
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

// -----------------------------------------------------------------------
// workspace.rename
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_rename(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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
            ws.custom_title = Some(truncate_str(title, MAX_TITLE_LEN).to_string());
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

// -----------------------------------------------------------------------
// workspace.reorder
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_reorder(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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
// workspace.action
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_action(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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
            ws.custom_color = Some(truncate_str(color, MAX_STATUS_LEN).to_string());
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
        "rename" => {
            let title = params.get("title").and_then(|v| v.as_str());
            let Some(title) = title else {
                return Response::error(
                    id,
                    "invalid_params",
                    "rename requires 'title' param",
                );
            };
            ws.custom_title = Some(truncate_str(title, MAX_METHOD_LEN).to_string());
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"ok": true}));
        }
        "move_up" => {
            let idx = tm.workspace_index(ws_id).unwrap_or(0);
            drop(tm);
            let new_idx = idx.saturating_sub(1);
            let mut tm = lock_or_recover(&state.tab_manager);
            tm.move_workspace(idx, new_idx);
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"index": new_idx}));
        }
        "move_down" => {
            let idx = tm.workspace_index(ws_id).unwrap_or(0);
            let len = tm.len();
            drop(tm);
            let new_idx = (idx + 1).min(len - 1);
            let mut tm = lock_or_recover(&state.tab_manager);
            tm.move_workspace(idx, new_idx);
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"index": new_idx}));
        }
        "move_top" => {
            let idx = tm.workspace_index(ws_id).unwrap_or(0);
            drop(tm);
            let mut tm = lock_or_recover(&state.tab_manager);
            tm.move_workspace(idx, 0);
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"index": 0}));
        }
        "close_others" => {
            drop(tm);
            let mut tm = lock_or_recover(&state.tab_manager);
            let count = tm.close_others(ws_id);
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"closed": count}));
        }
        "close_above" => {
            drop(tm);
            let mut tm = lock_or_recover(&state.tab_manager);
            let count = tm.close_above(ws_id);
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"closed": count}));
        }
        "close_below" => {
            drop(tm);
            let mut tm = lock_or_recover(&state.tab_manager);
            let count = tm.close_below(ws_id);
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"closed": count}));
        }
        _ => {
            return Response::error(
                id,
                "invalid_params",
                "Unknown action. Use: pin, unpin, toggle_pin, mark_read, mark_unread, clear_name, set_color, clear_color, rename, move_up, move_down, move_top, close_others, close_above, close_below",
            )
        }
    }

    let pinned = ws.is_pinned;
    drop(tm);
    state.notify_ui_refresh();

    Response::success(id, serde_json::json!({"is_pinned": pinned}))
}

// -----------------------------------------------------------------------
// workspace.report_pwd
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_report_pwd(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let directory = match params.get("directory").and_then(|v| v.as_str()) {
        Some(d) => d.to_string(),
        None => return Response::error(id, "invalid_params", "Provide 'directory'"),
    };

    let panel_id = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let mut tm = lock_or_recover(&state.tab_manager);

    // Find the workspace: by panel, by workspace ID, or selected
    let ws = if let Some(pid) = panel_id {
        tm.find_workspace_with_panel_mut(pid)
    } else if let Some(wid) = ws_id {
        tm.workspace_mut(wid)
    } else {
        tm.selected_mut()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace");
    };

    // Set panel directory if a panel was specified
    if let Some(pid) = panel_id {
        if let Some(panel) = ws.panels.get_mut(&pid) {
            panel.directory = Some(directory.clone());
        }
        // If this is the focused panel, also update workspace directory
        if ws.focused_panel_id == Some(pid) {
            ws.current_directory = directory;
        }
    } else {
        // No panel specified — update workspace directory
        ws.current_directory = directory;
    }

    drop(tm);
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}

// -----------------------------------------------------------------------
// workspace.report_ports / workspace.clear_ports
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_report_ports(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ports: Vec<u16> = match params.get("ports").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_u64().and_then(|n| u16::try_from(n).ok()))
            .collect(),
        None => return Response::error(id, "invalid_params", "Provide 'ports' array"),
    };

    let panel_id = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(pid) = panel_id {
        tm.find_workspace_with_panel_mut(pid)
    } else {
        tm.selected_mut()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace");
    };

    let target_panel_id = panel_id.or(ws.focused_panel_id);
    if let Some(pid) = target_panel_id {
        if let Some(panel) = ws.panels.get_mut(&pid) {
            panel.listening_ports = ports;
        }
    }

    drop(tm);
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}

pub(super) fn handle_workspace_clear_ports(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(pid) = panel_id {
        tm.find_workspace_with_panel_mut(pid)
    } else {
        tm.selected_mut()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace");
    };

    let target_panel_id = panel_id.or(ws.focused_panel_id);
    if let Some(pid) = target_panel_id {
        if let Some(panel) = ws.panels.get_mut(&pid) {
            panel.listening_ports.clear();
        }
    }

    drop(tm);
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}

// -----------------------------------------------------------------------
// workspace.report_tty
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_report_tty(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let tty = match params.get("tty").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return Response::error(id, "invalid_params", "Provide 'tty'"),
    };

    let panel_id = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(pid) = panel_id {
        tm.find_workspace_with_panel_mut(pid)
    } else {
        tm.selected_mut()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace");
    };

    let target_panel_id = panel_id.or(ws.focused_panel_id);
    if let Some(pid) = target_panel_id {
        if let Some(panel) = ws.panels.get_mut(&pid) {
            panel.tty_name = Some(tty);
        }
    }

    drop(tm);
    Response::success(id, serde_json::json!({"ok": true}))
}

// -----------------------------------------------------------------------
// workspace.ports_kick (no-op for API parity)
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_ports_kick(id: Value) -> Response {
    Response::success(id, serde_json::json!({"ok": true}))
}

// -----------------------------------------------------------------------
// workspace.report_pr / workspace.report_pr_checks
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_report_pr(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

    ws.pr_status = status.map(|s| truncate_str(s, MAX_STATUS_LEN).to_string());
    ws.pr_url = url.map(|s| truncate_str(s, MAX_URL_LEN).to_string());

    drop(tm);
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"updated": true}))
}

pub(super) fn handle_workspace_report_pr_checks(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let checks = params.get("checks").and_then(|v| v.as_array());

    let Some(checks_arr) = checks else {
        return Response::error(id, "invalid_params", "Provide 'checks' array");
    };

    let parsed: Vec<crate::model::workspace::PrCheck> = checks_arr
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?;
            let conclusion = item
                .get("conclusion")
                .and_then(|v| v.as_str())
                .unwrap_or("PENDING");
            Some(crate::model::workspace::PrCheck {
                name: truncate_str(name, MAX_NAME_LEN).to_string(),
                conclusion: conclusion.to_uppercase(),
            })
        })
        .take(20) // Cap at 20 checks
        .collect();

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace_mut(wid)
    } else {
        tm.selected_mut()
    };

    let Some(ws) = ws else {
        return Response::error(id, "not_found", "No workspace");
    };

    ws.pr_checks = parsed;
    drop(tm);
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"updated": true}))
}

// -----------------------------------------------------------------------
// workspace.move_to_window
// -----------------------------------------------------------------------

pub(super) fn handle_workspace_move_to_window(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let ws_id = match parse_workspace_param(params) {
        Ok(v) => v,
        Err(()) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };
    let target_window = params
        .get("window")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let Some(target_window) = target_window else {
        return Response::error(id, "invalid_params", "Provide 'window' (UUID)");
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = if let Some(wid) = ws_id {
        tm.workspace_mut(wid)
    } else {
        tm.selected_mut()
    };

    if let Some(ws) = ws {
        ws.window_id = Some(target_window);
        drop(tm);
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Workspace not found")
    }
}

// -----------------------------------------------------------------------
// settings.open
// -----------------------------------------------------------------------

pub(super) fn handle_settings_open(id: Value, state: &Arc<SharedState>) -> Response {
    state.send_ui_event(UiEvent::OpenSettings);
    Response::success(id, serde_json::json!({"opened": true}))
}

// -----------------------------------------------------------------------
// app.focus_override.set
// -----------------------------------------------------------------------

pub(super) fn handle_app_focus_override(
    id: Value,
    params: &Value,
    _state: &Arc<SharedState>,
) -> Response {
    let _active = params
        .get("active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // Focus override is a macOS-specific feature (NSApp.isActive override).
    // On Linux/GTK this is a no-op but we accept the command for protocol parity.
    Response::success(id, serde_json::json!({"ok": true}))
}

// -----------------------------------------------------------------------
// app.simulate_active
// -----------------------------------------------------------------------

pub(super) fn handle_app_simulate_active(
    id: Value,
    _params: &Value,
    _state: &Arc<SharedState>,
) -> Response {
    // Simulate app activation — on Linux/GTK this is a no-op for protocol parity.
    Response::success(id, serde_json::json!({"ok": true}))
}
