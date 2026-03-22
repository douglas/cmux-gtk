//! Window V2 handlers.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{SharedState, UiEvent};

use super::Response;

pub(super) fn handle_window_new(id: Value, state: &Arc<SharedState>) -> Response {
    state.send_ui_event(UiEvent::CreateWindow);
    Response::success(id, serde_json::json!({"created": true}))
}

pub(super) fn handle_window_list(id: Value, state: &Arc<SharedState>) -> Response {
    let wids = state.window_ids();
    let windows: Vec<Value> = wids
        .iter()
        .enumerate()
        .map(|(i, wid)| {
            serde_json::json!({
                "id": wid.to_string(),
                "focused": i == 0,
            })
        })
        .collect();
    Response::success(id, serde_json::json!({"windows": windows}))
}

pub(super) fn handle_window_current(id: Value, state: &Arc<SharedState>) -> Response {
    let wids = state.window_ids();
    if let Some(wid) = wids.first() {
        Response::success(
            id,
            serde_json::json!({"id": wid.to_string(), "focused": true}),
        )
    } else {
        Response::error(id, "no_window", "No windows available")
    }
}

pub(super) fn handle_window_focus(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let window_id = params
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok());
    if let Some(wid) = window_id {
        state.send_ui_event_to(&wid, UiEvent::Refresh);
        Response::success(id, serde_json::json!({"focused": true}))
    } else {
        // Focus the primary window
        state.send_ui_event(UiEvent::Refresh);
        Response::success(id, serde_json::json!({"focused": true}))
    }
}

pub(super) fn handle_window_close(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let window_id_str = params.get("id").and_then(|v| v.as_str());
    if let Some(wid_str) = window_id_str {
        if let Ok(wid) = uuid::Uuid::parse_str(wid_str) {
            state.remove_ui_event_sender(&wid);
            Response::success(id, serde_json::json!({"closed": true}))
        } else {
            Response::error(id, "invalid_id", "Invalid window ID")
        }
    } else {
        Response::error(id, "missing_param", "Missing 'id' parameter")
    }
}
