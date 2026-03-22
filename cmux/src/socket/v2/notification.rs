//! Notification V2 handlers.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{lock_or_recover, SharedState};

use super::helpers::parse_workspace_param;
use super::Response;

pub(super) fn handle_notification_create(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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
    let panel_id = match params
        .get("surface")
        .or_else(|| params.get("panel"))
        .and_then(|v| if v.is_null() { None } else { Some(v) })
    {
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

        let workspace = tm
            .workspace_mut(target_workspace_id)
            .expect("workspace validated above");
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

    // Auto-reorder: move notified workspace toward the top (after pinned items)
    let notif_settings = crate::settings::load().notifications;
    if notif_settings.reorder_on_notification {
        let mut tm = lock_or_recover(&state.tab_manager);
        if let Some(ws_idx) = tm.workspace_index(target_workspace_id) {
            // Find the first non-pinned index (skip pinned workspaces at top)
            let first_unpinned = tm.iter().position(|ws| !ws.is_pinned).unwrap_or(0);
            if ws_idx > first_unpinned {
                tm.move_workspace(ws_idx, first_unpinned);
            }
        }
    }

    // Play notification sound if enabled
    if notif_settings.sound_enabled {
        play_notification_sound(&notif_settings);
    }

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

fn play_notification_sound(settings: &crate::settings::NotificationSettings) {
    // Use custom command if set, otherwise fall back to paplay with a freedesktop sound
    if let Some(ref cmd) = settings.custom_command {
        let cmd = cmd.clone();
        std::thread::spawn(move || {
            let _ = std::process::Command::new("sh")
                .args(["-c", &cmd])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        });
    } else {
        std::thread::spawn(|| {
            // Try paplay (PulseAudio) with a freedesktop notification sound
            let _ = std::process::Command::new("paplay")
                .arg("/usr/share/sounds/freedesktop/stereo/message-new-instant.oga")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        });
    }
}

pub(super) fn handle_notification_list(id: Value, state: &Arc<SharedState>) -> Response {
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

pub(super) fn handle_notification_clear(id: Value, state: &Arc<SharedState>) -> Response {
    lock_or_recover(&state.notifications).clear();
    state.notify_ui_refresh();
    Response::success(id, serde_json::json!({"ok": true}))
}
