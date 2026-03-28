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

mod helpers;
mod markdown;
mod notification;
mod pane;
mod surface;
mod system;
mod tab;
mod window;
mod workspace;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::SharedState;

// Re-export for use by other modules (browser.rs uses require_panel_id).
pub(crate) use helpers::require_panel_id;

/// V2 protocol request.
#[derive(Debug, Deserialize)]
pub struct Request {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// V2 protocol response — `ok: true` with `result` on success, `ok: false` with `error` on failure.
#[derive(Debug, Serialize)]
pub struct Response {
    pub id: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

/// Structured error returned in failed V2 responses.
#[derive(Debug, Serialize)]
pub struct ErrorInfo {
    /// Machine-readable error code (e.g., `"not_found"`, `"invalid_params"`).
    pub code: String,
    /// Human-readable description of what went wrong.
    pub message: String,
}

impl Response {
    pub(crate) fn success(id: Value, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub(crate) fn error(id: Value, code: &str, message: &str) -> Self {
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

// Truncation limits for user-supplied strings stored in model objects.
const MAX_DIRECTORY_LEN: usize = 4096;
const MAX_TITLE_LEN: usize = 1024;
const MAX_URL_LEN: usize = 1024;
const MAX_BRANCH_LEN: usize = 256;
const MAX_METHOD_LEN: usize = 200;
const MAX_NAME_LEN: usize = 128;
const MAX_STATUS_LEN: usize = 64;
const MAX_SURFACE_INPUT_LEN: usize = 128 * 1024;

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
        "system.capabilities" => system::handle_capabilities(id),
        "system.identify" => system::handle_system_identify(id),
        "system.tree" => system::handle_system_tree(id, state),

        // Workspace commands
        "workspace.list" => workspace::handle_workspace_list(id, state),
        "workspace.new" => workspace::handle_workspace_new(id, &req.params, state),
        "workspace.create" => workspace::handle_workspace_create(id, &req.params, state),
        "workspace.create_ssh" => workspace::handle_workspace_create_ssh(id, &req.params, state),
        "workspace.remote.status" => {
            workspace::handle_workspace_remote_status(id, &req.params, state)
        }
        "workspace.select" => workspace::handle_workspace_select(id, &req.params, state),
        "workspace.next" => workspace::handle_workspace_next(id, &req.params, state),
        "workspace.previous" => workspace::handle_workspace_previous(id, &req.params, state),
        "workspace.last" => workspace::handle_workspace_last(id, state),
        "workspace.latest_unread" => workspace::handle_workspace_latest_unread(id, state),
        "workspace.close" => workspace::handle_workspace_close(id, &req.params, state),
        "workspace.set_status" => workspace::handle_workspace_set_status(id, &req.params, state),
        "workspace.report_git_branch" => {
            workspace::handle_workspace_report_git(id, &req.params, state)
        }
        "workspace.set_progress" => {
            workspace::handle_workspace_set_progress(id, &req.params, state)
        }
        "workspace.append_log" => workspace::handle_workspace_append_log(id, &req.params, state),
        "workspace.reorder" => workspace::handle_workspace_reorder(id, &req.params, state),
        "workspace.clear_status" => {
            workspace::handle_workspace_clear_status(id, &req.params, state)
        }
        "workspace.list_status" => workspace::handle_workspace_list_status(id, &req.params, state),
        "workspace.clear_progress" => {
            workspace::handle_workspace_clear_progress(id, &req.params, state)
        }
        "workspace.clear_log" => workspace::handle_workspace_clear_log(id, &req.params, state),
        "workspace.list_log" => workspace::handle_workspace_list_log(id, &req.params, state),
        "workspace.report_meta" => workspace::handle_workspace_report_meta(id, &req.params, state),
        "workspace.clear_meta" => workspace::handle_workspace_clear_meta(id, &req.params, state),
        "workspace.list_meta" => workspace::handle_workspace_list_meta(id, &req.params, state),
        "workspace.report_meta_block" => {
            workspace::handle_workspace_report_meta_block(id, &req.params, state)
        }
        "workspace.clear_meta_block" => {
            workspace::handle_workspace_clear_meta_block(id, &req.params, state)
        }
        "workspace.list_meta_blocks" => {
            workspace::handle_workspace_list_meta_blocks(id, &req.params, state)
        }

        // Workspace query commands
        "workspace.current" => workspace::handle_workspace_current(id, state),
        "workspace.rename" => workspace::handle_workspace_rename(id, &req.params, state),
        "workspace.action" => workspace::handle_workspace_action(id, &req.params, state),
        "workspace.report_pr" => workspace::handle_workspace_report_pr(id, &req.params, state),
        "workspace.report_pr_checks" => {
            workspace::handle_workspace_report_pr_checks(id, &req.params, state)
        }
        "workspace.move_to_window" => {
            workspace::handle_workspace_move_to_window(id, &req.params, state)
        }

        // App commands
        "app.focus_override.set" => workspace::handle_app_focus_override(id, &req.params, state),
        "app.simulate_active" => workspace::handle_app_simulate_active(id, &req.params, state),

        // Pane commands
        "pane.new" => pane::handle_pane_new(id, &req.params, state),
        "pane.list" => pane::handle_pane_list(id, &req.params, state),
        "pane.focus" => pane::handle_pane_focus(id, &req.params, state),
        "pane.close" => pane::handle_pane_close(id, &req.params, state),
        "pane.last" => pane::handle_pane_last(id, &req.params, state),
        "pane.swap" => pane::handle_pane_swap(id, &req.params, state),
        "pane.resize" => pane::handle_pane_resize(id, &req.params, state),
        "pane.focus_direction" => pane::handle_pane_focus_direction(id, &req.params, state),
        "pane.create" => pane::handle_pane_new(id, &req.params, state),
        "pane.break" => pane::handle_pane_break(id, &req.params, state),
        "pane.join" => pane::handle_pane_join(id, &req.params, state),

        // Surface commands
        "surface.send_input" => surface::handle_surface_send_input(id, &req.params, state),
        "surface.send_text" => surface::handle_surface_send_input(id, &req.params, state),
        "surface.list" => surface::handle_surface_list(id, &req.params, state),
        "surface.current" => surface::handle_surface_current(id, state),
        "surface.focus" => surface::handle_surface_focus(id, &req.params, state),
        "surface.split" => pane::handle_pane_new(id, &req.params, state),
        "surface.close" => pane::handle_pane_close(id, &req.params, state),
        "surface.action" => surface::handle_surface_action(id, &req.params, state),
        "surface.health" => surface::handle_surface_health(id, &req.params, state),
        "surface.send_key" => surface::handle_surface_send_key(id, &req.params, state),
        "surface.read_text" => surface::handle_surface_read_text(id, &req.params, state),
        "surface.refresh" => surface::handle_surface_refresh(id, &req.params, state),
        "surface.clear_history" => surface::handle_surface_clear_history(id, &req.params, state),
        "surface.trigger_flash" => surface::handle_surface_trigger_flash(id, &req.params, state),
        "surface.move" => surface::handle_surface_move(id, &req.params, state),
        "surface.reorder" => surface::handle_surface_reorder(id, &req.params, state),
        "surface.create" => surface::handle_surface_create(id, &req.params, state),
        "surface.drag_to_split" => surface::handle_surface_drag_to_split(id, &req.params, state),

        // Tab actions
        "tab.action" => tab::handle_tab_action(id, &req.params, state),

        // Pane query
        "pane.surfaces" => pane::handle_pane_surfaces(id, &req.params, state),
        "pane.equalize" => pane::handle_pane_equalize(id, &req.params, state),

        // Workspace telemetry
        "workspace.report_pwd" => workspace::handle_workspace_report_pwd(id, &req.params, state),
        "workspace.report_ports" => {
            workspace::handle_workspace_report_ports(id, &req.params, state)
        }
        "workspace.clear_ports" => workspace::handle_workspace_clear_ports(id, &req.params, state),
        "workspace.report_tty" => workspace::handle_workspace_report_tty(id, &req.params, state),
        "workspace.ports_kick" => workspace::handle_workspace_ports_kick(id),

        // Settings
        "settings.open" => workspace::handle_settings_open(id, state),

        // Notification commands
        "notification.create" => notification::handle_notification_create(id, &req.params, state),
        "notification.create_for_surface" => {
            notification::handle_notification_create(id, &req.params, state)
        }
        "notification.create_for_target" => {
            notification::handle_notification_create(id, &req.params, state)
        }
        "notification.list" => notification::handle_notification_list(id, state),
        "notification.clear" => notification::handle_notification_clear(id, state),

        // Browser automation commands — delegated to socket::browser module
        #[cfg(feature = "webkit")]
        method if method.starts_with("browser.") => {
            match super::browser::dispatch(method, id.clone(), &req.params, state) {
                Some(resp) => resp,
                None => Response::error(id, "unknown_method", &format!("Unknown method: {method}")),
            }
        }
        #[cfg(not(feature = "webkit"))]
        method if method.starts_with("browser.") => Response::error(
            id,
            "not_compiled",
            "browser support not compiled (build with --features webkit)",
        ),

        // Markdown commands
        "markdown.open" => markdown::handle_markdown_open(id, &req.params, state),

        // Window commands
        "window.new" => window::handle_window_new(id, state),
        "window.list" => window::handle_window_list(id, state),
        "window.current" => window::handle_window_current(id, state),
        "window.focus" => window::handle_window_focus(id, &req.params, state),
        "window.close" => window::handle_window_close(id, &req.params, state),

        _ => Response::error(
            id,
            "unknown_method",
            &format!(
                "Unknown method: {}",
                crate::model::workspace::truncate_str(&req.method, MAX_METHOD_LEN)
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::lock_or_recover;

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
        state.install_ui_event_sender(uuid::Uuid::new_v4(), tx);

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
            crate::app::UiEvent::SendInput {
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
