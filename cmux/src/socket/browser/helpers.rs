//! Shared helpers for browser automation handlers.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{SharedState, UiEvent};
use crate::ui::browser_panel::BrowserActionKind;

use super::Response;

/// Maximum length for user-supplied text in browser commands (1 MB).
pub(super) const MAX_BROWSER_INPUT_LEN: usize = 1024 * 1024;

/// Truncate a browser input string to prevent memory exhaustion.
pub(super) fn truncate_browser_input(s: &str) -> &str {
    crate::model::workspace::truncate_str(s, MAX_BROWSER_INPUT_LEN)
}

/// JSON-encode a value for embedding in JavaScript. Infallible for types
/// that are always representable as JSON (strings, numbers, bools).
pub(crate) fn js<T: serde::Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

pub(super) fn send_action(
    id: &Value,
    params: &Value,
    state: &Arc<SharedState>,
    action: BrowserActionKind,
) -> Response {
    let panel_id = match require_panel_id(id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.send_ui_event(UiEvent::BrowserAction { panel_id, action });
    Response::success(id.clone(), serde_json::json!({"ok": true}))
}

pub(super) fn send_action_with_reply(
    id: &Value,
    params: &Value,
    state: &Arc<SharedState>,
    make_action: impl FnOnce(tokio::sync::oneshot::Sender<Result<Value, String>>) -> BrowserActionKind,
    error_code: &str,
    error_msg: &str,
) -> Response {
    let panel_id = match require_panel_id(id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let action = make_action(tx);
    state.send_ui_event(UiEvent::BrowserAction { panel_id, action });
    match rx.blocking_recv() {
        Ok(Ok(value)) => Response::success(id.clone(), value),
        Ok(Err(e)) => Response::error(id.clone(), error_code, &e),
        Err(_) => Response::error(id.clone(), "timeout", error_msg),
    }
}

pub(super) fn send_eval_action(
    id: &Value,
    params: &Value,
    state: &Arc<SharedState>,
    js: String,
) -> Response {
    let panel_id = match require_panel_id(id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.send_ui_event(UiEvent::BrowserAction {
        panel_id,
        action: BrowserActionKind::Eval {
            script: js,
            reply: tx,
        },
    });
    match rx.blocking_recv() {
        Ok(Ok(val)) => {
            let s = val.as_str().unwrap_or("");
            if let Some(code) = s.strip_prefix("ERROR:") {
                Response::error(id.clone(), code, code)
            } else {
                Response::success(id.clone(), val)
            }
        }
        Ok(Err(e)) => Response::error(id.clone(), "execution_failed", &e),
        Err(_) => Response::error(id.clone(), "timeout", "UI event channel closed"),
    }
}

/// Extract selector param, resolving @eN refs.
pub(super) fn require_selector(id: &Value, params: &Value) -> Result<String, Response> {
    let sel = params
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Response::error(id.clone(), "invalid_params", "Provide 'selector'"))?;
    crate::ui::browser_panel::resolve_selector(sel)
        .ok_or_else(|| Response::error(id.clone(), "invalid_ref", "Element ref not found"))
}

// Re-export from v2 helpers for convenience.
pub(super) use super::super::v2::require_panel_id;
