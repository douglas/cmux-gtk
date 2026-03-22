//! Tab action V2 handler.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{lock_or_recover, SharedState};

use super::helpers::resolve_panel_id;
use super::{Response, MAX_TITLE_LEN};

pub(super) fn handle_tab_action(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let action = params.get("action").and_then(|v| v.as_str());
    let Some(action) = action else {
        return Response::error(id, "invalid_params", "Provide 'action'");
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = tm.find_workspace_with_panel_mut(panel_id);
    let Some(ws) = ws else {
        return Response::error(id, "not_found", "Panel not found in any workspace");
    };

    match action {
        "rename" => {
            let title = params.get("title").and_then(|v| v.as_str());
            let Some(title) = title else {
                return Response::error(id, "invalid_params", "rename requires 'title'");
            };
            if let Some(panel) = ws.panels.get_mut(&panel_id) {
                panel.custom_title =
                    Some(crate::model::workspace::truncate_str(title, MAX_TITLE_LEN).to_string());
            }
        }
        "clear_name" => {
            if let Some(panel) = ws.panels.get_mut(&panel_id) {
                panel.custom_title = None;
            }
        }
        "close_left" | "close_right" | "close_others" => {
            let pane_ids = ws.layout.find_pane_with_panel_readonly(panel_id);
            let Some(pane_ids) = pane_ids else {
                return Response::error(id, "not_found", "Panel not in any pane");
            };
            let pos = pane_ids
                .iter()
                .position(|&pid| pid == panel_id)
                .unwrap_or(0);
            let to_close: Vec<uuid::Uuid> = match action {
                "close_left" => pane_ids[..pos].to_vec(),
                "close_right" => {
                    if pos + 1 < pane_ids.len() {
                        pane_ids[pos + 1..].to_vec()
                    } else {
                        vec![]
                    }
                }
                "close_others" => pane_ids
                    .iter()
                    .filter(|&&pid| pid != panel_id)
                    .copied()
                    .collect(),
                _ => vec![],
            };
            for pid in &to_close {
                ws.panels.remove(pid);
                ws.layout.remove_panel(*pid);
            }
            // Update focus
            if let Some(focused) = ws.focused_panel_id {
                if to_close.contains(&focused) {
                    ws.focused_panel_id = Some(panel_id);
                }
            }
            let ws_empty = ws.is_empty();
            let ws_id = ws.id;
            if ws_empty {
                tm.remove_by_id(ws_id);
            }
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(id, serde_json::json!({"closed": to_close.len()}));
        }
        "pin" => {
            if let Some(panel) = ws.panels.get_mut(&panel_id) {
                panel.is_pinned = true;
            }
        }
        "unpin" => {
            if let Some(panel) = ws.panels.get_mut(&panel_id) {
                panel.is_pinned = false;
            }
        }
        "mark_read" => {
            if let Some(panel) = ws.panels.get_mut(&panel_id) {
                panel.is_manually_unread = false;
            }
        }
        "mark_unread" => {
            if let Some(panel) = ws.panels.get_mut(&panel_id) {
                panel.is_manually_unread = true;
            }
        }
        "duplicate" => {
            let new_panel = crate::model::Panel::new_terminal();
            let new_id = new_panel.id;
            ws.panels.insert(new_id, new_panel);
            ws.layout.add_panel_to_pane(panel_id, new_id);
            ws.previous_focused_panel_id = ws.focused_panel_id;
            ws.focused_panel_id = Some(new_id);
            drop(tm);
            state.notify_ui_refresh();
            return Response::success(
                id,
                serde_json::json!({
                    "panel_id": new_id.to_string(),
                    "duplicated": true,
                }),
            );
        }
        _ => {
            return Response::error(
                id,
                "invalid_params",
                "Unknown action. Use: rename, clear_name, close_left, close_right, close_others, pin, unpin, mark_read, mark_unread, duplicate",
            );
        }
    }

    drop(tm);
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}
