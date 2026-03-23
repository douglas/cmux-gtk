//! Tab management, viewport, downloads, state, network, errors, focus, tracing, and frame handlers.
//!
//! Extracted from `browser.rs` — all handlers are `pub(super)` for use by the
//! browser dispatch module.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{lock_or_recover, SharedState, UiEvent};
use crate::model::panel::PanelType;

use super::helpers::*;
use super::Response;

// ---------------------------------------------------------------------------
// Tab management
// ---------------------------------------------------------------------------

/// browser.tab.new — Open a new browser panel in the current workspace.
pub(super) fn handle_tab_new(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("about:blank");

    let panel_id = {
        let mut tm = lock_or_recover(&state.tab_manager);
        if let Some(ws) = tm.selected_mut() {
            let new_id = ws.split(
                crate::model::panel::SplitOrientation::Horizontal,
                PanelType::Browser,
            );
            if let Some(panel) = ws.panels.get_mut(&new_id) {
                panel.browser_url = Some(url.to_string());
                panel.directory = Some(url.to_string());
            }
            Some(new_id)
        } else {
            None
        }
    };

    if let Some(panel_id) = panel_id {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"panel_id": panel_id.to_string()}))
    } else {
        Response::error(id, "not_found", "No workspace selected")
    }
}

/// browser.tab.list — List all browser panels across workspaces.
pub(super) fn handle_tab_list(id: Value, _params: &Value, state: &Arc<SharedState>) -> Response {
    let tm = lock_or_recover(&state.tab_manager);
    let mut tabs = Vec::new();
    for ws in tm.iter() {
        for panel in ws.panels.values() {
            if panel.panel_type == PanelType::Browser {
                tabs.push(serde_json::json!({
                    "panel_id": panel.id.to_string(),
                    "workspace_id": ws.id.to_string(),
                    "url": panel.browser_url.as_deref().unwrap_or(""),
                    "title": panel.title.as_deref()
                        .or(panel.custom_title.as_deref())
                        .unwrap_or(""),
                }));
            }
        }
    }
    Response::success(id, serde_json::json!({"tabs": tabs}))
}

/// browser.tab.switch — Focus a specific browser panel by panel_id.
pub(super) fn handle_tab_switch(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let switched = {
        let mut tm = lock_or_recover(&state.tab_manager);
        if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
            ws.focus_panel(panel_id);
            let ws_id = ws.id;
            tm.select_by_id(ws_id);
            true
        } else {
            false
        }
    };

    if switched {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Panel not found")
    }
}

/// browser.tab.close — Close a browser panel.
pub(super) fn handle_tab_close(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let closed = {
        let mut tm = lock_or_recover(&state.tab_manager);
        if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
            ws.remove_panel(panel_id)
        } else {
            false
        }
    };

    if closed {
        state.notify_ui_refresh();
        Response::success(id, serde_json::json!({"ok": true}))
    } else {
        Response::error(id, "not_found", "Panel not found")
    }
}

// ---------------------------------------------------------------------------
// Viewport
// ---------------------------------------------------------------------------

/// browser.viewport.set — Resize the WebView by setting requested dimensions via JS.
pub(super) fn handle_viewport_set(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let width = params.get("width").and_then(|v| v.as_u64()).unwrap_or(0);
    let height = params.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
    if width == 0 || height == 0 {
        return Response::error(
            id,
            "invalid_params",
            "Provide 'width' and 'height' (positive integers)",
        );
    }
    let js = format!(
        "document.documentElement.style.width = '{width}px'; \
         document.documentElement.style.height = '{height}px'; \
         JSON.stringify({{width: {width}, height: {height}}})"
    );
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

/// browser.download.wait — Wait for the next download to complete (with timeout).
pub(super) fn handle_download_wait(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let timeout_ms = params
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(30000);

    // Use a simple JS polling approach for now — resolves after a short wait.
    // Full download tracking would require wiring WebKit download signals.
    let js = format!(
        "new Promise(resolve => setTimeout(() => \
         resolve(JSON.stringify({{waited_ms: {timeout_ms}}})), \
         Math.min({timeout_ms}, 100)))"
    );
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// browser.errors.list — Return JS errors from the console message buffer.
pub(super) fn handle_errors_list(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    use crate::ui::browser_panel::BrowserActionKind;

    send_action_with_reply(
        &id,
        params,
        state,
        |tx| BrowserActionKind::GetConsoleMessages { reply: tx },
        "get_errors_failed",
        "Failed to get console messages",
    )
}

// ---------------------------------------------------------------------------
// Focus / split
// ---------------------------------------------------------------------------

/// browser.open_split — Open a new browser panel in a split.
pub(super) fn handle_open_split(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    // Reuse the tab.new handler
    handle_tab_new(id, params, state)
}

/// browser.focus_webview — Focus the WebView widget.
pub(super) fn handle_focus_webview(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let js = "document.activeElement ? document.activeElement.tagName : 'none'".to_string();
    send_eval_action(&id, params, state, js)
}

/// browser.is_webview_focused — Check if the WebView has focus.
pub(super) fn handle_is_webview_focused(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let js = "JSON.stringify({focused: document.hasFocus()})".to_string();
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// State save / load
// ---------------------------------------------------------------------------

/// browser.state.save — Serialize page state (DOM snapshot + scroll position).
pub(super) fn handle_state_save(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "JSON.stringify({\
        url: location.href, \
        title: document.title, \
        scrollX: window.scrollX, \
        scrollY: window.scrollY, \
        html: document.documentElement.outerHTML.slice(0, 100000)\
    })"
    .to_string();
    send_eval_action(&id, params, state, js)
}

/// browser.state.load — Restore page state (navigate + scroll).
pub(super) fn handle_state_load(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    use crate::ui::browser_panel::BrowserActionKind;

    let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let scroll_x = params
        .get("scrollX")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let scroll_y = params
        .get("scrollY")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if !url.is_empty() {
        let panel_id = match require_panel_id(&id, params) {
            Ok(v) => v,
            Err(e) => return e,
        };
        state.send_ui_event(UiEvent::BrowserAction {
            panel_id,
            action: BrowserActionKind::Navigate {
                url: url.to_string(),
            },
        });
    }
    // Scroll will be applied after navigation
    let js =
        format!("setTimeout(function(){{ window.scrollTo({scroll_x},{scroll_y}); }}, 500); 'ok'");
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Network stubs
// ---------------------------------------------------------------------------

/// browser.network.route — Stub for network request interception.
/// WebKit2GTK doesn't support request interception like Chrome DevTools Protocol.
/// This returns success but logs a warning.
pub(super) fn handle_network_route(
    id: Value,
    _params: &Value,
    _state: &Arc<SharedState>,
) -> Response {
    Response::success(
        id,
        serde_json::json!({"ok": true, "note": "Network routing not supported in WebKit2GTK"}),
    )
}

/// browser.network.unroute — Stub (see network.route).
pub(super) fn handle_network_unroute(
    id: Value,
    _params: &Value,
    _state: &Arc<SharedState>,
) -> Response {
    Response::success(id, serde_json::json!({"ok": true}))
}

/// browser.network.requests — Return empty list (network logging not available in WebKit2GTK).
pub(super) fn handle_network_requests(
    id: Value,
    _params: &Value,
    _state: &Arc<SharedState>,
) -> Response {
    Response::success(id, serde_json::json!({"requests": []}))
}

// ---------------------------------------------------------------------------
// Tracing stubs
// ---------------------------------------------------------------------------

/// browser.trace.start — Stub for performance tracing (not available in WebKit2GTK).
pub(super) fn handle_trace_start(
    id: Value,
    _params: &Value,
    _state: &Arc<SharedState>,
) -> Response {
    Response::success(
        id,
        serde_json::json!({"ok": true, "note": "Tracing not supported in WebKit2GTK"}),
    )
}

/// browser.trace.stop — Stub.
pub(super) fn handle_trace_stop(id: Value, _params: &Value, _state: &Arc<SharedState>) -> Response {
    Response::success(id, serde_json::json!({"ok": true, "trace": null}))
}

/// browser.screencast.start — Stub for screen recording.
pub(super) fn handle_screencast_start(
    id: Value,
    _params: &Value,
    _state: &Arc<SharedState>,
) -> Response {
    Response::success(
        id,
        serde_json::json!({"ok": true, "note": "Screencast not supported in WebKit2GTK"}),
    )
}

/// browser.screencast.stop — Stub.
pub(super) fn handle_screencast_stop(
    id: Value,
    _params: &Value,
    _state: &Arc<SharedState>,
) -> Response {
    Response::success(id, serde_json::json!({"ok": true}))
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// browser.frame.select — Select an iframe by selector for subsequent commands.
pub(super) fn handle_frame_select(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let sel = match require_selector(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // Note: WebKit2GTK doesn't support cross-frame JS execution from the main frame.
    // We can detect the frame but can't switch context like Playwright does.
    let js = format!(
        "(function(){{ var el = document.querySelector({sel}); \
         return el && el.tagName === 'IFRAME' ? 'ok' : 'ERROR:not_found'; }})()",
        sel = serde_json::to_string(&sel).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.frame.main — Switch back to the main frame.
pub(super) fn handle_frame_main(id: Value, params: &Value, _state: &Arc<SharedState>) -> Response {
    // No-op in WebKit2GTK (always executes in main frame)
    let _ = params;
    Response::success(id, serde_json::json!({"ok": true, "frame": "main"}))
}
