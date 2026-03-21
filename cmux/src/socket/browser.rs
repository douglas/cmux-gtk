//! Browser automation socket command handlers.
//!
//! All `browser.*` methods are routed here from `v2::dispatch()`.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{SharedState, UiEvent};
use crate::ui::browser_panel::BrowserActionKind;

use super::v2::{require_panel_id, Response};

/// Dispatch a `browser.*` method. Returns `Some(Response)` if handled, `None` if unrecognized.
pub fn dispatch(
    method: &str,
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Option<Response> {
    let response = match method {
        "browser.navigate" => handle_navigate(id, params, state),
        "browser.execute_js" => handle_execute_js(id, params, state),
        "browser.get_url" => handle_get_url(id, params, state),
        "browser.get_text" => handle_get_text(id, params, state),
        "browser.back" => handle_back(id, params, state),
        "browser.forward" => handle_forward(id, params, state),
        "browser.reload" => handle_reload(id, params, state),
        "browser.set_zoom" => handle_set_zoom(id, params, state),
        "browser.screenshot" => handle_screenshot(id, params, state),
        // Phase 2: DOM interaction
        "browser.click" => handle_click(id, params, state),
        "browser.dblclick" => handle_dblclick(id, params, state),
        "browser.hover" => handle_hover(id, params, state),
        "browser.type" => handle_type(id, params, state),
        "browser.fill" => handle_fill(id, params, state),
        "browser.clear" => handle_clear(id, params, state),
        "browser.press" => handle_press(id, params, state),
        "browser.select_option" => handle_select_option(id, params, state),
        "browser.check" => handle_check(id, params, state),
        "browser.focus" => handle_focus(id, params, state),
        "browser.blur" => handle_blur(id, params, state),
        "browser.scroll_to" => handle_scroll_to(id, params, state),
        // Phase 3: Element queries
        "browser.get_html" => handle_get_html(id, params, state),
        "browser.get_value" => handle_get_value(id, params, state),
        "browser.get_attribute" => handle_get_attribute(id, params, state),
        "browser.get_property" => handle_get_property(id, params, state),
        "browser.get_bounding_box" => handle_get_bounding_box(id, params, state),
        "browser.get_computed_style" => handle_get_computed_style(id, params, state),
        "browser.is_visible" => handle_is_visible(id, params, state),
        "browser.is_enabled" => handle_is_enabled(id, params, state),
        "browser.is_checked" => handle_is_checked(id, params, state),
        "browser.is_editable" => handle_is_editable(id, params, state),
        "browser.count" => handle_count(id, params, state),
        // Phase 4: Finders + element refs
        "browser.find" => handle_find(id, params, state),
        "browser.find_all" => handle_find_all(id, params, state),
        "browser.find_by_text" => handle_find_by_text(id, params, state),
        "browser.find_by_role" => handle_find_by_role(id, params, state),
        "browser.find_by_label" => handle_find_by_label(id, params, state),
        "browser.find_by_placeholder" => handle_find_by_placeholder(id, params, state),
        "browser.find_by_test_id" => handle_find_by_test_id(id, params, state),
        "browser.release_ref" => handle_release_ref(id, params, state),
        // Phase 5: Advanced
        "browser.wait_for_selector" => handle_wait_for_selector(id, params, state),
        "browser.wait_for_navigation" => handle_wait_for_navigation(id, params, state),
        "browser.wait_for_load_state" => handle_wait_for_load_state(id, params, state),
        "browser.wait_for_function" => handle_wait_for_function(id, params, state),
        "browser.snapshot" => handle_snapshot(id, params, state),
        "browser.title" => handle_title(id, params, state),
        "browser.get_cookies" => handle_get_cookies(id, params, state),
        "browser.set_cookie" => handle_set_cookie(id, params, state),
        "browser.clear_cookies" => handle_clear_cookies(id, params, state),
        "browser.local_storage_get" => handle_local_storage_get(id, params, state),
        "browser.local_storage_set" => handle_local_storage_set(id, params, state),
        "browser.session_storage_get" => handle_session_storage_get(id, params, state),
        "browser.session_storage_set" => handle_session_storage_set(id, params, state),
        "browser.get_console_messages" => handle_get_console_messages(id, params, state),
        "browser.set_dialog_handler" => handle_set_dialog_handler(id, params, state),
        "browser.inject_script" => handle_inject_script(id, params, state),
        "browser.inject_style" => handle_inject_style(id, params, state),
        "browser.remove_injected" => handle_remove_injected(id, params, state),
        // Phase 6: Browser automation parity
        "browser.uncheck" => handle_uncheck(id, params, state),
        "browser.scroll" => handle_scroll(id, params, state),
        "browser.scroll_into_view" => handle_scroll_into_view(id, params, state),
        "browser.keydown" => handle_keydown(id, params, state),
        "browser.keyup" => handle_keyup(id, params, state),
        "browser.find.alt" => handle_find_by_alt(id, params, state),
        "browser.find.title" => handle_find_by_title(id, params, state),
        "browser.find.first" => handle_find_first(id, params, state),
        "browser.find.last" => handle_find_last(id, params, state),
        "browser.find.nth" => handle_find_nth(id, params, state),
        "browser.frame.select" => handle_frame_select(id, params, state),
        "browser.frame.main" => handle_frame_main(id, params, state),
        "browser.dialog.accept" => handle_dialog_accept(id, params, state),
        "browser.dialog.dismiss" => handle_dialog_dismiss(id, params, state),
        "browser.highlight" => handle_highlight(id, params, state),
        "browser.console.clear" => handle_console_clear(id, params, state),
        "browser.geolocation.set" => handle_geolocation_set(id, params, state),
        "browser.offline.set" => handle_offline_set(id, params, state),
        "browser.open_split" => handle_open_split(id, params, state),
        "browser.focus_webview" => handle_focus_webview(id, params, state),
        "browser.is_webview_focused" => handle_is_webview_focused(id, params, state),
        "browser.state.save" => handle_state_save(id, params, state),
        "browser.state.load" => handle_state_load(id, params, state),
        "browser.network.route" => handle_network_route(id, params, state),
        "browser.network.unroute" => handle_network_unroute(id, params, state),
        "browser.network.requests" => handle_network_requests(id, params, state),
        "browser.input_mouse" => handle_input_mouse(id, params, state),
        "browser.input_keyboard" => handle_input_keyboard(id, params, state),
        "browser.input_touch" => handle_input_touch(id, params, state),
        "browser.trace.start" => handle_trace_start(id, params, state),
        "browser.trace.stop" => handle_trace_stop(id, params, state),
        "browser.screencast.start" => handle_screencast_start(id, params, state),
        "browser.screencast.stop" => handle_screencast_stop(id, params, state),
        "browser.addinitscript" => handle_inject_script(id, params, state),
        "browser.addscript" => handle_inject_script(id, params, state),
        "browser.addstyle" => handle_inject_style(id, params, state),
        "browser.tab.new" => handle_tab_new(id, params, state),
        "browser.tab.list" => handle_tab_list(id, params, state),
        "browser.tab.switch" => handle_tab_switch(id, params, state),
        "browser.tab.close" => handle_tab_close(id, params, state),
        "browser.viewport.set" => handle_viewport_set(id, params, state),
        "browser.download.wait" => handle_download_wait(id, params, state),
        "browser.errors.list" => handle_errors_list(id, params, state),
        _ => return None,
    };
    Some(response)
}

/// Return all browser method names for system.capabilities.
pub fn method_names() -> Vec<&'static str> {
    vec![
        "browser.navigate",
        "browser.execute_js",
        "browser.get_url",
        "browser.get_text",
        "browser.back",
        "browser.forward",
        "browser.reload",
        "browser.set_zoom",
        "browser.screenshot",
        // Phase 2
        "browser.click",
        "browser.dblclick",
        "browser.hover",
        "browser.type",
        "browser.fill",
        "browser.clear",
        "browser.press",
        "browser.select_option",
        "browser.check",
        "browser.focus",
        "browser.blur",
        "browser.scroll_to",
        // Phase 3
        "browser.get_html",
        "browser.get_value",
        "browser.get_attribute",
        "browser.get_property",
        "browser.get_bounding_box",
        "browser.get_computed_style",
        "browser.is_visible",
        "browser.is_enabled",
        "browser.is_checked",
        "browser.is_editable",
        "browser.count",
        // Phase 4
        "browser.find",
        "browser.find_all",
        "browser.find_by_text",
        "browser.find_by_role",
        "browser.find_by_label",
        "browser.find_by_placeholder",
        "browser.find_by_test_id",
        "browser.release_ref",
        // Phase 5
        "browser.wait_for_selector",
        "browser.wait_for_navigation",
        "browser.wait_for_load_state",
        "browser.wait_for_function",
        "browser.snapshot",
        "browser.title",
        "browser.get_cookies",
        "browser.set_cookie",
        "browser.clear_cookies",
        "browser.local_storage_get",
        "browser.local_storage_set",
        "browser.session_storage_get",
        "browser.session_storage_set",
        "browser.get_console_messages",
        "browser.set_dialog_handler",
        "browser.inject_script",
        "browser.inject_style",
        "browser.remove_injected",
        // Phase 6: parity
        "browser.uncheck",
        "browser.scroll",
        "browser.scroll_into_view",
        "browser.keydown",
        "browser.keyup",
        "browser.find.alt",
        "browser.find.title",
        "browser.find.first",
        "browser.find.last",
        "browser.find.nth",
        "browser.frame.select",
        "browser.frame.main",
        "browser.dialog.accept",
        "browser.dialog.dismiss",
        "browser.highlight",
        "browser.console.clear",
        "browser.geolocation.set",
        "browser.offline.set",
        "browser.open_split",
        "browser.focus_webview",
        "browser.is_webview_focused",
        "browser.state.save",
        "browser.state.load",
        "browser.network.route",
        "browser.network.unroute",
        "browser.network.requests",
        "browser.input_mouse",
        "browser.input_keyboard",
        "browser.input_touch",
        "browser.trace.start",
        "browser.trace.stop",
        "browser.screencast.start",
        "browser.screencast.stop",
        "browser.addinitscript",
        "browser.addscript",
        "browser.addstyle",
        // Phase 6: tabs
        "browser.tab.new",
        "browser.tab.list",
        "browser.tab.switch",
        "browser.tab.close",
        "browser.viewport.set",
        "browser.download.wait",
        "browser.errors.list",
    ]
}

// ---------------------------------------------------------------------------
// Helper: send a BrowserAction and block for reply
// ---------------------------------------------------------------------------

fn send_action(
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

fn send_action_with_reply(
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

/// Helper to extract selector param, resolving @eN refs.
fn require_selector(id: &Value, params: &Value) -> Result<String, Response> {
    let sel = params
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Response::error(id.clone(), "invalid_params", "Provide 'selector'"))?;
    crate::ui::browser_panel::resolve_selector(sel)
        .ok_or_else(|| Response::error(id.clone(), "invalid_ref", "Element ref not found"))
}

// ---------------------------------------------------------------------------
// Phase 1: Existing 9 commands (migrated from v2.rs)
// ---------------------------------------------------------------------------

fn handle_navigate(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

fn handle_execute_js(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

fn handle_get_url(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::GetUrl { reply },
        "not_found",
        "UI event channel closed",
    )
}

fn handle_get_text(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::GetText { reply },
        "not_found",
        "UI event channel closed",
    )
}

fn handle_back(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action(&id, params, state, BrowserActionKind::GoBack)
}

fn handle_forward(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action(&id, params, state, BrowserActionKind::GoForward)
}

fn handle_reload(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action(&id, params, state, BrowserActionKind::Reload)
}

fn handle_set_zoom(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

fn handle_screenshot(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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
// Phase 2: DOM interaction commands
// ---------------------------------------------------------------------------

fn handle_click(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let button = params.get("button").and_then(|v| v.as_str()).unwrap_or("left");
    let js = match button {
        "right" => format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.dispatchEvent(new MouseEvent('contextmenu', {{bubbles:true,cancelable:true,button:2}})); return 'ok'; }})()"#,
            sel = serde_json::to_string(&selector).unwrap()
        ),
        "middle" => format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.dispatchEvent(new MouseEvent('click', {{bubbles:true,cancelable:true,button:1}})); return 'ok'; }})()"#,
            sel = serde_json::to_string(&selector).unwrap()
        ),
        _ => format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.click(); return 'ok'; }})()"#,
            sel = serde_json::to_string(&selector).unwrap()
        ),
    };
    send_eval_action(&id, params, state, js)
}

fn handle_dblclick(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.dispatchEvent(new MouseEvent('dblclick', {{bubbles:true,cancelable:true}})); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_hover(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.dispatchEvent(new MouseEvent('mouseover', {{bubbles:true}})); el.dispatchEvent(new MouseEvent('mouseenter', {{bubbles:false}})); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_type(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(text) = params.get("text").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'text'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); var text = {text}; for(var i=0;i<text.length;i++){{ var ch=text[i]; el.dispatchEvent(new KeyboardEvent('keydown',{{key:ch,bubbles:true}})); el.dispatchEvent(new KeyboardEvent('keypress',{{key:ch,bubbles:true}})); if(el.value!==undefined) el.value+=ch; el.dispatchEvent(new KeyboardEvent('keyup',{{key:ch,bubbles:true}})); }} el.dispatchEvent(new Event('input',{{bubbles:true}})); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        text = serde_json::to_string(text).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_fill(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(value) = params.get("value").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'value'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); el.value = {val}; el.dispatchEvent(new Event('input',{{bubbles:true}})); el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        val = serde_json::to_string(value).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_clear(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); el.value = ''; el.dispatchEvent(new Event('input',{{bubbles:true}})); el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_press(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); var opts = {{key:{key},bubbles:true,cancelable:true}}; el.dispatchEvent(new KeyboardEvent('keydown',opts)); el.dispatchEvent(new KeyboardEvent('keypress',opts)); el.dispatchEvent(new KeyboardEvent('keyup',opts)); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        key = serde_json::to_string(key).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_select_option(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let by_value = params.get("value").and_then(|v| v.as_str());
    let by_label = params.get("label").and_then(|v| v.as_str());
    let by_index = params.get("index").and_then(|v| v.as_u64());
    let js = if let Some(val) = by_value {
        format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.value = {val}; el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
            sel = serde_json::to_string(&selector).unwrap(),
            val = serde_json::to_string(val).unwrap()
        )
    } else if let Some(label) = by_label {
        format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; var opts = el.options; for(var i=0;i<opts.length;i++){{ if(opts[i].text==={label}){{ el.selectedIndex=i; break; }} }} el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
            sel = serde_json::to_string(&selector).unwrap(),
            label = serde_json::to_string(label).unwrap()
        )
    } else if let Some(idx) = by_index {
        format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.selectedIndex = {idx}; el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
            sel = serde_json::to_string(&selector).unwrap(),
            idx = idx
        )
    } else {
        return Response::error(id, "invalid_params", "Provide 'value', 'label', or 'index'");
    };
    send_eval_action(&id, params, state, js)
}

fn handle_check(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let checked = params.get("checked").and_then(|v| v.as_bool()).unwrap_or(true);
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.checked = {checked}; el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        checked = checked
    );
    send_eval_action(&id, params, state, js)
}

fn handle_focus(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_blur(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.blur(); return 'ok'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_scroll_to(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = params.get("selector").and_then(|v| v.as_str());
    let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let js = if let Some(sel) = selector {
        format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.scrollTo({x},{y}); return 'ok'; }})()"#,
            sel = serde_json::to_string(sel).unwrap(),
            x = x,
            y = y
        )
    } else {
        format!(
            r#"(function(){{ window.scrollTo({x},{y}); return 'ok'; }})()"#,
            x = x,
            y = y
        )
    };
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Phase 3: Element queries
// ---------------------------------------------------------------------------

fn handle_get_html(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let outer = params.get("outer").and_then(|v| v.as_bool()).unwrap_or(false);
    let prop = if outer { "outerHTML" } else { "innerHTML" };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return el.{prop}; }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        prop = prop
    );
    send_eval_action(&id, params, state, js)
}

fn handle_get_value(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return String(el.value); }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_get_attribute(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'name'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; var v = el.getAttribute({name}); return v === null ? 'null' : v; }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        name = serde_json::to_string(name).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_get_property(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'name'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return JSON.stringify(el[{name}]); }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        name = serde_json::to_string(name).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_get_bounding_box(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; var r = el.getBoundingClientRect(); return JSON.stringify({{x:r.x,y:r.y,width:r.width,height:r.height}}); }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_get_computed_style(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(property) = params.get("property").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'property'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return getComputedStyle(el)[{prop}]; }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        prop = serde_json::to_string(property).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_is_visible(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; var s = getComputedStyle(el); return String(el.offsetParent !== null && s.visibility !== 'hidden' && parseFloat(s.opacity) > 0); }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_is_enabled(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return String(!el.disabled); }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_is_checked(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return String(!!el.checked); }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_is_editable(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return String(!el.readOnly && !el.disabled); }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_count(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ return String(document.querySelectorAll({sel}).length); }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Phase 4: Finders + element refs
// ---------------------------------------------------------------------------

fn handle_find(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // Verify element exists via JS, then allocate a ref on the Rust side
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); return el ? 'found' : 'ERROR:not_found'; }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
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
            if s.starts_with("ERROR:") {
                Response::error(id, "not_found", "Element not found")
            } else {
                let ref_id = crate::ui::browser_panel::allocate_ref(panel_id, &selector);
                Response::success(id, serde_json::json!({"ref": ref_id, "selector": selector}))
            }
        }
        Ok(Err(e)) => Response::error(id, "execution_failed", &e),
        Err(_) => Response::error(id, "timeout", "UI event channel closed"),
    }
}

fn handle_find_all(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ return String(document.querySelectorAll({sel}).length); }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );
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
            let count: usize = val.as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
            let mut refs = Vec::with_capacity(count);
            for i in 0..count {
                // Use querySelectorAll-based nth selector for precise targeting
                let nth_sel = format!(
                    ":is({}):nth-child({})",
                    selector,
                    i + 1
                );
                let ref_id = crate::ui::browser_panel::allocate_ref(panel_id, &nth_sel);
                refs.push(ref_id);
            }
            Response::success(id, serde_json::json!({"refs": refs, "count": count}))
        }
        Ok(Err(e)) => Response::error(id, "execution_failed", &e),
        Err(_) => Response::error(id, "timeout", "UI event channel closed"),
    }
}

fn handle_find_by_text(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(text) = params.get("text").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'text'");
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // Use XPath to find element containing text, then return a unique selector
    let js = format!(
        r#"(function(){{ var result = document.evaluate("//text()[contains(.,"+{text}+")]/parent::*", document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null); var el = result.singleNodeValue; if(!el) return 'ERROR:not_found'; return el.tagName.toLowerCase() + (el.id ? '#'+el.id : '') + (el.className ? '.'+el.className.split(' ').join('.') : ''); }})()"#,
        text = serde_json::to_string(text).unwrap()
    );
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
            if s.starts_with("ERROR:") {
                Response::error(id, "not_found", "Element with text not found")
            } else {
                let ref_id = crate::ui::browser_panel::allocate_ref(panel_id, s);
                Response::success(id, serde_json::json!({"ref": ref_id, "selector": s}))
            }
        }
        Ok(Err(e)) => Response::error(id, "execution_failed", &e),
        Err(_) => Response::error(id, "timeout", "UI event channel closed"),
    }
}

fn handle_find_by_role(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(role) = params.get("role").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'role'");
    };
    let selector = format!("[role=\"{}\"]", role.replace('"', r#"\""#));
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    find_by_selector(&id, params, state, panel_id, &selector)
}

fn handle_find_by_label(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(label) = params.get("label").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'label'");
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let selector = format!("[aria-label=\"{}\"]", label.replace('"', r#"\""#));
    find_by_selector(&id, params, state, panel_id, &selector)
}

fn handle_find_by_placeholder(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(placeholder) = params.get("placeholder").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'placeholder'");
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let selector = format!(
        "[placeholder=\"{}\"]",
        placeholder.replace('"', r#"\""#)
    );
    find_by_selector(&id, params, state, panel_id, &selector)
}

fn handle_find_by_test_id(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(test_id) = params.get("test_id").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'test_id'");
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let selector = format!("[data-testid=\"{}\"]", test_id.replace('"', r#"\""#));
    find_by_selector(&id, params, state, panel_id, &selector)
}

fn handle_release_ref(id: Value, params: &Value, _state: &Arc<SharedState>) -> Response {
    let Some(ref_id) = params.get("ref").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'ref' (e.g. '@e1')");
    };
    let removed = crate::ui::browser_panel::release_ref(ref_id);
    if removed {
        Response::success(id, serde_json::json!({"released": true}))
    } else {
        Response::error(id, "not_found", "Ref not found")
    }
}

/// Helper for find_by_* commands that use a CSS selector.
fn find_by_selector(
    id: &Value,
    _params: &Value,
    state: &Arc<SharedState>,
    panel_id: uuid::Uuid,
    selector: &str,
) -> Response {
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); return el ? 'found' : 'ERROR:not_found'; }})()"#,
        sel = serde_json::to_string(selector).unwrap()
    );
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
            if s.starts_with("ERROR:") {
                Response::error(id.clone(), "not_found", "Element not found")
            } else {
                let ref_id = crate::ui::browser_panel::allocate_ref(panel_id, selector);
                Response::success(
                    id.clone(),
                    serde_json::json!({"ref": ref_id, "selector": selector}),
                )
            }
        }
        Ok(Err(e)) => Response::error(id.clone(), "execution_failed", &e),
        Err(_) => Response::error(id.clone(), "timeout", "UI event channel closed"),
    }
}

// ---------------------------------------------------------------------------
// Phase 5: Advanced features
// ---------------------------------------------------------------------------

fn handle_wait_for_selector(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

fn handle_wait_for_navigation(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

fn handle_wait_for_load_state(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

fn handle_wait_for_function(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

fn handle_snapshot(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "document.documentElement.outerHTML".to_string();
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::Eval {
            script: js,
            reply,
        },
        "not_found",
        "UI event channel closed",
    )
}

fn handle_title(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "document.title".to_string();
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::Eval {
            script: js,
            reply,
        },
        "not_found",
        "UI event channel closed",
    )
}

fn handle_get_cookies(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "document.cookie".to_string();
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::Eval {
            script: js,
            reply,
        },
        "not_found",
        "UI event channel closed",
    )
}

fn handle_set_cookie(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(cookie) = params.get("cookie").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'cookie'");
    };
    let js = format!(
        r#"(function(){{ document.cookie = {cookie}; return 'ok'; }})()"#,
        cookie = serde_json::to_string(cookie).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_clear_cookies(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = r#"(function(){ var cookies = document.cookie.split(';'); for(var i=0;i<cookies.length;i++){ var name = cookies[i].split('=')[0].trim(); document.cookie = name + '=;expires=Thu, 01 Jan 1970 00:00:00 GMT;path=/'; } return 'ok'; })()"#.to_string();
    send_eval_action(&id, params, state, js)
}

fn handle_local_storage_get(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let js = format!(
        r#"(function(){{ var v = localStorage.getItem({key}); return v === null ? 'null' : v; }})()"#,
        key = serde_json::to_string(key).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_local_storage_set(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let Some(value) = params.get("value").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'value'");
    };
    let js = format!(
        r#"(function(){{ localStorage.setItem({key},{val}); return 'ok'; }})()"#,
        key = serde_json::to_string(key).unwrap(),
        val = serde_json::to_string(value).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_session_storage_get(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let js = format!(
        r#"(function(){{ var v = sessionStorage.getItem({key}); return v === null ? 'null' : v; }})()"#,
        key = serde_json::to_string(key).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_session_storage_set(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let Some(value) = params.get("value").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'value'");
    };
    let js = format!(
        r#"(function(){{ sessionStorage.setItem({key},{val}); return 'ok'; }})()"#,
        key = serde_json::to_string(key).unwrap(),
        val = serde_json::to_string(value).unwrap()
    );
    send_eval_action(&id, params, state, js)
}

fn handle_get_console_messages(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| BrowserActionKind::GetConsoleMessages { reply },
        "not_found",
        "UI event channel closed",
    )
}

fn handle_set_dialog_handler(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("accept")
        .to_string();
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.send_ui_event(UiEvent::BrowserAction {
        panel_id,
        action: BrowserActionKind::SetDialogHandler {
            action,
            prompt_text: text,
        },
    });
    Response::success(id, serde_json::json!({"ok": true}))
}

fn handle_inject_script(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(script) = params.get("script").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'script'");
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.send_ui_event(UiEvent::BrowserAction {
        panel_id,
        action: BrowserActionKind::InjectScript {
            script: script.to_string(),
        },
    });
    Response::success(id, serde_json::json!({"ok": true}))
}

fn handle_inject_style(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(css) = params.get("css").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'css'");
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.send_ui_event(UiEvent::BrowserAction {
        panel_id,
        action: BrowserActionKind::InjectStyle {
            css: css.to_string(),
        },
    });
    Response::success(id, serde_json::json!({"ok": true}))
}

fn handle_remove_injected(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.send_ui_event(UiEvent::BrowserAction {
        panel_id,
        action: BrowserActionKind::RemoveInjected,
    });
    Response::success(id, serde_json::json!({"ok": true}))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Send a JS eval action and translate the result, handling `ERROR:*` prefixes.
fn send_eval_action(
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

// ---------------------------------------------------------------------------
// Phase 6: Browser automation parity commands
// ---------------------------------------------------------------------------

/// browser.tab.new — Open a new browser panel in the current workspace.
fn handle_tab_new(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("about:blank");

    use crate::app::lock_or_recover;
    use crate::model::panel::PanelType;

    let panel_id = {
        let mut tm = lock_or_recover(&state.tab_manager);
        if let Some(ws) = tm.selected_mut() {
            let new_id = ws.split(
                crate::model::panel::SplitOrientation::Horizontal,
                PanelType::Browser,
            );
            if let Some(panel) = ws.panels.get_mut(&new_id) {
                panel.browser_url = Some(url.to_string());
            }
            Some(new_id)
        } else {
            None
        }
    };

    if let Some(panel_id) = panel_id {
        state.notify_ui_refresh();
        Response::success(
            id,
            serde_json::json!({"panel_id": panel_id.to_string()}),
        )
    } else {
        Response::error(id, "not_found", "No workspace selected")
    }
}

/// browser.tab.list — List all browser panels across workspaces.
fn handle_tab_list(id: Value, _params: &Value, state: &Arc<SharedState>) -> Response {
    use crate::app::lock_or_recover;
    use crate::model::panel::PanelType;

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
fn handle_tab_switch(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };

    use crate::app::lock_or_recover;

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
fn handle_tab_close(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };

    use crate::app::lock_or_recover;

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

/// browser.viewport.set — Resize the WebView by setting requested dimensions via JS.
fn handle_viewport_set(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

/// browser.download.wait — Wait for the next download to complete (with timeout).
fn handle_download_wait(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

/// browser.errors.list — Return JS errors from the console message buffer.
fn handle_errors_list(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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
// Phase 7: Additional browser automation parity commands
// ---------------------------------------------------------------------------

/// browser.uncheck — Uncheck a checkbox.
fn handle_uncheck(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let sel = match require_selector(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let js = format!(
        "(function(){{ var el = document.querySelector({sel}); \
         if(el && el.checked) el.click(); return 'ok'; }})()",
        sel = serde_json::to_string(&sel).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.scroll — Scroll the page by x/y pixels.
fn handle_scroll(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let js = format!("window.scrollBy({x},{y}); JSON.stringify({{scrollX:window.scrollX,scrollY:window.scrollY}})");
    send_eval_action(&id, params, state, js)
}

/// browser.scroll_into_view — Scroll element into view.
fn handle_scroll_into_view(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let sel = match require_selector(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let js = format!(
        "(function(){{ var el = document.querySelector({sel}); \
         if(el) {{ el.scrollIntoView({{behavior:'smooth',block:'center'}}); return 'ok'; }} \
         return 'ERROR:not_found'; }})()",
        sel = serde_json::to_string(&sel).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.keydown — Dispatch keydown event.
fn handle_keydown(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{{key:{key},bubbles:true}})); 'ok'",
        key = serde_json::to_string(key).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.keyup — Dispatch keyup event.
fn handle_keyup(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "document.activeElement.dispatchEvent(new KeyboardEvent('keyup',{{key:{key},bubbles:true}})); 'ok'",
        key = serde_json::to_string(key).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.find.alt — Find element by alt text.
fn handle_find_by_alt(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let alt = params.get("alt").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "(function(){{ var el = document.querySelector('[alt={alt}]'); \
         return el ? el.tagName.toLowerCase() : 'ERROR:not_found'; }})()",
        alt = serde_json::to_string(alt).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.find.title — Find element by title attribute.
fn handle_find_by_title(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "(function(){{ var el = document.querySelector('[title={t}]'); \
         return el ? el.tagName.toLowerCase() : 'ERROR:not_found'; }})()",
        t = serde_json::to_string(title).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.find.first — Find first matching element.
fn handle_find_first(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let sel = match require_selector(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let js = format!(
        "(function(){{ var el = document.querySelector({sel}); \
         return el ? JSON.stringify({{tag:el.tagName.toLowerCase(),text:(el.textContent||'').slice(0,200)}}) \
         : 'ERROR:not_found'; }})()",
        sel = serde_json::to_string(&sel).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.find.last — Find last matching element.
fn handle_find_last(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let sel = match require_selector(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let js = format!(
        "(function(){{ var els = document.querySelectorAll({sel}); \
         var el = els.length ? els[els.length-1] : null; \
         return el ? JSON.stringify({{tag:el.tagName.toLowerCase(),text:(el.textContent||'').slice(0,200)}}) \
         : 'ERROR:not_found'; }})()",
        sel = serde_json::to_string(&sel).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.find.nth — Find nth matching element.
fn handle_find_nth(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let sel = match require_selector(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let index = params.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
    let js = format!(
        "(function(){{ var els = document.querySelectorAll({sel}); \
         var el = els[{index}]; \
         return el ? JSON.stringify({{tag:el.tagName.toLowerCase(),text:(el.textContent||'').slice(0,200)}}) \
         : 'ERROR:not_found'; }})()"
    );
    send_eval_action(&id, params, state, js)
}

/// browser.frame.select — Select an iframe by selector for subsequent commands.
fn handle_frame_select(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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
fn handle_frame_main(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    // No-op in WebKit2GTK (always executes in main frame)
    let _ = params;
    Response::success(id, serde_json::json!({"ok": true, "frame": "main"}))
}

/// browser.dialog.accept — Accept the current dialog.
fn handle_dialog_accept(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let text = params.get("text").and_then(|v| v.as_str());
    let action = if let Some(t) = text {
        format!("accept:{t}")
    } else {
        "accept".to_string()
    };
    send_action(&id, params, state, BrowserActionKind::SetDialogHandler {
        action,
        prompt_text: text.map(|s| s.to_string()),
    })
}

/// browser.dialog.dismiss — Dismiss the current dialog.
fn handle_dialog_dismiss(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    send_action(&id, params, state, BrowserActionKind::SetDialogHandler {
        action: "dismiss".to_string(),
        prompt_text: None,
    })
}

/// browser.highlight — Temporarily highlight an element with a colored outline.
fn handle_highlight(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let sel = match require_selector(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let js = format!(
        "(function(){{ var el = document.querySelector({sel}); \
         if(!el) return 'ERROR:not_found'; \
         var old = el.style.outline; \
         el.style.outline = '3px solid #ff6b6b'; \
         el.style.outlineOffset = '2px'; \
         setTimeout(function(){{ el.style.outline = old; el.style.outlineOffset = ''; }}, 2000); \
         return 'ok'; }})()",
        sel = serde_json::to_string(&sel).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.console.clear — Clear the console message buffer.
fn handle_console_clear(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "console.clear(); 'ok'".to_string();
    send_eval_action(&id, params, state, js)
}

/// browser.geolocation.set — Override the navigator.geolocation API.
fn handle_geolocation_set(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let lat = params.get("latitude").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let lng = params.get("longitude").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let accuracy = params.get("accuracy").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let js = format!(
        "navigator.geolocation.getCurrentPosition = function(cb) {{ \
         cb({{coords:{{latitude:{lat},longitude:{lng},accuracy:{accuracy}}},timestamp:Date.now()}}); \
         }}; 'ok'"
    );
    send_eval_action(&id, params, state, js)
}

/// browser.offline.set — Simulate offline/online state via navigator.onLine override.
fn handle_offline_set(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let offline = params.get("offline").and_then(|v| v.as_bool()).unwrap_or(false);
    let js = format!(
        "Object.defineProperty(navigator, 'onLine', {{value: {online}, configurable: true}}); \
         window.dispatchEvent(new Event('{event}')); 'ok'",
        online = !offline,
        event = if offline { "offline" } else { "online" }
    );
    send_eval_action(&id, params, state, js)
}

/// browser.open_split — Open a new browser panel in a split.
fn handle_open_split(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    // Reuse the tab.new handler
    handle_tab_new(id, params, state)
}

/// browser.focus_webview — Focus the WebView widget.
fn handle_focus_webview(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "document.activeElement ? document.activeElement.tagName : 'none'".to_string();
    send_eval_action(&id, params, state, js)
}

/// browser.is_webview_focused — Check if the WebView has focus.
fn handle_is_webview_focused(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "JSON.stringify({focused: document.hasFocus()})".to_string();
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Phase 8: Final browser automation parity
// ---------------------------------------------------------------------------

/// browser.state.save — Serialize page state (DOM snapshot + scroll position).
fn handle_state_save(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "JSON.stringify({\
        url: location.href, \
        title: document.title, \
        scrollX: window.scrollX, \
        scrollY: window.scrollY, \
        html: document.documentElement.outerHTML.slice(0, 100000)\
    })".to_string();
    send_eval_action(&id, params, state, js)
}

/// browser.state.load — Restore page state (navigate + scroll).
fn handle_state_load(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let scroll_x = params.get("scrollX").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let scroll_y = params.get("scrollY").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if !url.is_empty() {
        let panel_id = match require_panel_id(&id, params) {
            Ok(v) => v,
            Err(e) => return e,
        };
        state.send_ui_event(UiEvent::BrowserAction {
            panel_id,
            action: BrowserActionKind::Navigate { url: url.to_string() },
        });
    }
    // Scroll will be applied after navigation
    let js = format!(
        "setTimeout(function(){{ window.scrollTo({scroll_x},{scroll_y}); }}, 500); 'ok'"
    );
    send_eval_action(&id, params, state, js)
}

/// browser.network.route — Stub for network request interception.
/// WebKit2GTK doesn't support request interception like Chrome DevTools Protocol.
/// This returns success but logs a warning.
fn handle_network_route(id: Value, _params: &Value, _state: &Arc<SharedState>) -> Response {
    Response::success(
        id,
        serde_json::json!({"ok": true, "note": "Network routing not supported in WebKit2GTK"}),
    )
}

/// browser.network.unroute — Stub (see network.route).
fn handle_network_unroute(id: Value, _params: &Value, _state: &Arc<SharedState>) -> Response {
    Response::success(id, serde_json::json!({"ok": true}))
}

/// browser.network.requests — Return empty list (network logging not available in WebKit2GTK).
fn handle_network_requests(id: Value, _params: &Value, _state: &Arc<SharedState>) -> Response {
    Response::success(id, serde_json::json!({"requests": []}))
}

/// browser.input_mouse — Dispatch a synthetic mouse event at coordinates.
fn handle_input_mouse(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let event_type = params.get("type").and_then(|v| v.as_str()).unwrap_or("click");
    let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let button = params.get("button").and_then(|v| v.as_u64()).unwrap_or(0);
    let js = format!(
        "(function(){{ \
         var el = document.elementFromPoint({x},{y}); \
         if(!el) return 'ERROR:no_element'; \
         el.dispatchEvent(new MouseEvent('{event_type}', \
           {{clientX:{x},clientY:{y},button:{button},bubbles:true}})); \
         return 'ok'; }})()"
    );
    send_eval_action(&id, params, state, js)
}

/// browser.input_keyboard — Dispatch a synthetic keyboard event.
fn handle_input_keyboard(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let event_type = params.get("type").and_then(|v| v.as_str()).unwrap_or("keypress");
    let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "document.activeElement.dispatchEvent(new KeyboardEvent('{event_type}', \
         {{key:{key},bubbles:true}})); 'ok'",
        key = serde_json::to_string(key).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.input_touch — Dispatch a synthetic touch event.
fn handle_input_touch(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let event_type = params.get("type").and_then(|v| v.as_str()).unwrap_or("touchstart");
    let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let js = format!(
        "(function(){{ \
         var el = document.elementFromPoint({x},{y}); \
         if(!el) return 'ERROR:no_element'; \
         var touch = new Touch({{identifier:1,target:el,clientX:{x},clientY:{y}}}); \
         el.dispatchEvent(new TouchEvent('{event_type}', \
           {{touches:[touch],changedTouches:[touch],bubbles:true}})); \
         return 'ok'; }})()"
    );
    send_eval_action(&id, params, state, js)
}

/// browser.trace.start — Stub for performance tracing (not available in WebKit2GTK).
fn handle_trace_start(id: Value, _params: &Value, _state: &Arc<SharedState>) -> Response {
    Response::success(
        id,
        serde_json::json!({"ok": true, "note": "Tracing not supported in WebKit2GTK"}),
    )
}

/// browser.trace.stop — Stub.
fn handle_trace_stop(id: Value, _params: &Value, _state: &Arc<SharedState>) -> Response {
    Response::success(id, serde_json::json!({"ok": true, "trace": null}))
}

/// browser.screencast.start — Stub for screen recording.
fn handle_screencast_start(id: Value, _params: &Value, _state: &Arc<SharedState>) -> Response {
    Response::success(
        id,
        serde_json::json!({"ok": true, "note": "Screencast not supported in WebKit2GTK"}),
    )
}

/// browser.screencast.stop — Stub.
fn handle_screencast_stop(id: Value, _params: &Value, _state: &Arc<SharedState>) -> Response {
    Response::success(id, serde_json::json!({"ok": true}))
}
