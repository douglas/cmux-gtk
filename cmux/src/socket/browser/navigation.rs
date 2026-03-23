//! Phase 1 + basic navigation browser command handlers.

use std::sync::Arc;

use serde_json::Value;

use crate::app::SharedState;
use crate::app::UiEvent;
use crate::ui::browser_panel::BrowserActionKind;

use super::helpers::*;
use super::Response;

pub(super) fn handle_navigate(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(url) = params.get("url").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'url'");
    };
    state.send_ui_event(UiEvent::BrowserAction {
        panel_id,
        action: BrowserActionKind::Navigate {
            url: url.to_string(),
        },
    });
    Response::success(id, serde_json::json!({"navigated": true}))
}

pub(super) fn handle_execute_js(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(script) = params.get("script").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'script'");
    };
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::Eval {
            script: script.to_string(),
            reply,
        },
        "execution_failed",
        "UI event channel closed",
    )
}

pub(super) fn handle_get_url(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::GetUrl { reply },
        "not_found",
        "UI event channel closed",
    )
}

pub(super) fn handle_get_text(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::GetText { reply },
        "not_found",
        "UI event channel closed",
    )
}

pub(super) fn handle_back(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action(&id, params, state, BrowserActionKind::GoBack)
}

pub(super) fn handle_forward(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action(&id, params, state, BrowserActionKind::GoForward)
}

pub(super) fn handle_reload(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action(&id, params, state, BrowserActionKind::Reload)
}

pub(super) fn handle_set_zoom(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let zoom = params["zoom"].as_f64().unwrap_or(1.0).clamp(0.25, 5.0);
    state.send_ui_event(UiEvent::BrowserAction {
        panel_id,
        action: BrowserActionKind::SetZoom { zoom },
    });
    Response::success(id, serde_json::json!({"zoom": zoom}))
}

pub(super) fn handle_screenshot(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::Eval {
            script: "document.documentElement.outerHTML.substring(0, 10000)".to_string(),
            reply,
        },
        "not_found",
        "UI event channel closed",
    )
}

// ---------------------------------------------------------------------------
// Wait commands
// ---------------------------------------------------------------------------

pub(super) fn handle_wait_for_selector(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let timeout_ms = params
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(5000);
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::WaitForSelector {
            selector,
            timeout_ms,
            reply,
        },
        "timeout",
        "UI event channel closed",
    )
}

pub(super) fn handle_wait_for_navigation(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let timeout_ms = params
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(10000);
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::WaitForNavigation { timeout_ms, reply },
        "timeout",
        "UI event channel closed",
    )
}

pub(super) fn handle_wait_for_load_state(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let timeout_ms = params
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(10000);
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::WaitForLoadState { timeout_ms, reply },
        "timeout",
        "UI event channel closed",
    )
}

pub(super) fn handle_wait_for_function(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(expression) = params.get("expression").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'expression'");
    };
    let timeout_ms = params
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(5000);
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::WaitForFunction {
            expression: expression.to_string(),
            timeout_ms,
            reply,
        },
        "timeout",
        "UI event channel closed",
    )
}

// ---------------------------------------------------------------------------
// Snapshot / title
// ---------------------------------------------------------------------------

pub(super) fn handle_snapshot(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "document.documentElement.outerHTML".to_string();
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::Eval { script: js, reply },
        "not_found",
        "UI event channel closed",
    )
}

pub(super) fn handle_title(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "document.title".to_string();
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::Eval { script: js, reply },
        "not_found",
        "UI event channel closed",
    )
}
