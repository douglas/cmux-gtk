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
            if s.starts_with("ERROR:") {
                let code = &s[6..];
                Response::error(id.clone(), code, code)
            } else {
                Response::success(id.clone(), val)
            }
        }
        Ok(Err(e)) => Response::error(id.clone(), "execution_failed", &e),
        Err(_) => Response::error(id.clone(), "timeout", "UI event channel closed"),
    }
}
