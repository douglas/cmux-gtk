//! Element queries, finders, storage, cookies, console, injection, and dialog handlers.

use std::sync::Arc;

use serde_json::Value;

use super::helpers::*;
use super::Response;
use crate::app::{SharedState, UiEvent};
use crate::ui::browser_panel::BrowserActionKind;

fn find_by_selector(
    id: &Value,
    _params: &Value,
    state: &Arc<SharedState>,
    panel_id: uuid::Uuid,
    selector: &str,
) -> Response {
    let js_code = format!(
        r#"(function(){{ var el = document.querySelector({sel}); return el ? 'found' : 'ERROR:not_found'; }})()"#,
        sel = js(selector)
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.send_ui_event(UiEvent::BrowserAction {
        panel_id,
        action: BrowserActionKind::Eval {
            script: js_code,
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
// Element queries (Phase 3)
// ---------------------------------------------------------------------------

pub(super) fn handle_get_html(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let outer = params
        .get("outer")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let prop = if outer { "outerHTML" } else { "innerHTML" };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return el.{prop}; }})()"#,
        sel = js(&selector),
        prop = prop
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_get_value(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return String(el.value); }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_get_attribute(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'name'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; var v = el.getAttribute({name}); return v === null ? 'null' : v; }})()"#,
        sel = js(&selector),
        name = js(name)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_get_property(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'name'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return JSON.stringify(el[{name}]); }})()"#,
        sel = js(&selector),
        name = js(name)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_get_bounding_box(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; var r = el.getBoundingClientRect(); return JSON.stringify({{x:r.x,y:r.y,width:r.width,height:r.height}}); }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_get_computed_style(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(property) = params.get("property").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'property'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return getComputedStyle(el)[{prop}]; }})()"#,
        sel = js(&selector),
        prop = js(property)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_is_visible(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; var s = getComputedStyle(el); return String(el.offsetParent !== null && s.visibility !== 'hidden' && parseFloat(s.opacity) > 0); }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_is_enabled(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return String(!el.disabled); }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_is_checked(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return String(!!el.checked); }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_is_editable(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; return String(!el.readOnly && !el.disabled); }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_count(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ return String(document.querySelectorAll({sel}).length); }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Finders + element refs (Phase 4)
// ---------------------------------------------------------------------------

pub(super) fn handle_find(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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
        sel = js(&selector)
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.send_ui_event(crate::app::UiEvent::BrowserAction {
        panel_id,
        action: crate::ui::browser_panel::BrowserActionKind::Eval {
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

pub(super) fn handle_find_all(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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
        sel = js(&selector)
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.send_ui_event(crate::app::UiEvent::BrowserAction {
        panel_id,
        action: crate::ui::browser_panel::BrowserActionKind::Eval {
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
                let nth_sel = format!(":is({}):nth-child({})", selector, i + 1);
                let ref_id = crate::ui::browser_panel::allocate_ref(panel_id, &nth_sel);
                refs.push(ref_id);
            }
            Response::success(id, serde_json::json!({"refs": refs, "count": count}))
        }
        Ok(Err(e)) => Response::error(id, "execution_failed", &e),
        Err(_) => Response::error(id, "timeout", "UI event channel closed"),
    }
}

pub(super) fn handle_find_by_text(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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
        text = js(text)
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.send_ui_event(crate::app::UiEvent::BrowserAction {
        panel_id,
        action: crate::ui::browser_panel::BrowserActionKind::Eval {
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

pub(super) fn handle_find_by_role(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

pub(super) fn handle_find_by_label(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

pub(super) fn handle_find_by_placeholder(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(placeholder) = params.get("placeholder").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'placeholder'");
    };
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let selector = format!("[placeholder=\"{}\"]", placeholder.replace('"', r#"\""#));
    find_by_selector(&id, params, state, panel_id, &selector)
}

pub(super) fn handle_find_by_test_id(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

pub(super) fn handle_find_by_alt(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let alt = params.get("alt").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "(function(){{ var el = document.querySelector('[alt={alt}]'); \
         return el ? el.tagName.toLowerCase() : 'ERROR:not_found'; }})()",
        alt = serde_json::to_string(alt).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_find_by_title(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "(function(){{ var el = document.querySelector('[title={t}]'); \
         return el ? el.tagName.toLowerCase() : 'ERROR:not_found'; }})()",
        t = serde_json::to_string(title).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_find_first(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

pub(super) fn handle_find_last(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

pub(super) fn handle_find_nth(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

// ---------------------------------------------------------------------------
// Element ref release
// ---------------------------------------------------------------------------

pub(super) fn handle_release_ref(id: Value, params: &Value, _state: &Arc<SharedState>) -> Response {
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

// ---------------------------------------------------------------------------
// Cookies
// ---------------------------------------------------------------------------

pub(super) fn handle_get_cookies(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let js = "document.cookie".to_string();
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| crate::ui::browser_panel::BrowserActionKind::Eval { script: js, reply },
        "not_found",
        "UI event channel closed",
    )
}

pub(super) fn handle_set_cookie(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(cookie) = params.get("cookie").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'cookie'");
    };
    let js = format!(
        r#"(function(){{ document.cookie = {cookie}; return 'ok'; }})()"#,
        cookie = js(cookie)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_clear_cookies(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let js = r#"(function(){ var cookies = document.cookie.split(';'); for(var i=0;i<cookies.length;i++){ var name = cookies[i].split('=')[0].trim(); document.cookie = name + '=;expires=Thu, 01 Jan 1970 00:00:00 GMT;path=/'; } return 'ok'; })()"#.to_string();
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Local storage
// ---------------------------------------------------------------------------

pub(super) fn handle_local_storage_get(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let js = format!(
        r#"(function(){{ var v = localStorage.getItem({key}); return v === null ? 'null' : v; }})()"#,
        key = js(key)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_local_storage_set(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let Some(value) = params.get("value").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'value'");
    };
    let js = format!(
        r#"(function(){{ localStorage.setItem({key},{val}); return 'ok'; }})()"#,
        key = js(key),
        val = js(value)
    );
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Session storage
// ---------------------------------------------------------------------------

pub(super) fn handle_session_storage_get(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let js = format!(
        r#"(function(){{ var v = sessionStorage.getItem({key}); return v === null ? 'null' : v; }})()"#,
        key = js(key)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_session_storage_set(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let Some(value) = params.get("value").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'value'");
    };
    let js = format!(
        r#"(function(){{ sessionStorage.setItem({key},{val}); return 'ok'; }})()"#,
        key = js(key),
        val = js(value)
    );
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Console messages
// ---------------------------------------------------------------------------

pub(super) fn handle_get_console_messages(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    send_action_with_reply(
        &id,
        params,
        state,
        |reply| crate::ui::browser_panel::BrowserActionKind::GetConsoleMessages { reply },
        "not_found",
        "UI event channel closed",
    )
}

pub(super) fn handle_console_clear(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let js = "console.clear(); 'ok'".to_string();
    send_eval_action(&id, params, state, js)
}

// ---------------------------------------------------------------------------
// Dialog handler
// ---------------------------------------------------------------------------

pub(super) fn handle_set_dialog_handler(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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
    state.send_ui_event(crate::app::UiEvent::BrowserAction {
        panel_id,
        action: crate::ui::browser_panel::BrowserActionKind::SetDialogHandler {
            action,
            prompt_text: text,
        },
    });
    Response::success(id, serde_json::json!({"ok": true}))
}

pub(super) fn handle_dialog_accept(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let text = params.get("text").and_then(|v| v.as_str());
    let action = if let Some(t) = text {
        format!("accept:{t}")
    } else {
        "accept".to_string()
    };
    send_action(
        &id,
        params,
        state,
        crate::ui::browser_panel::BrowserActionKind::SetDialogHandler {
            action,
            prompt_text: text.map(|s| s.to_string()),
        },
    )
}

pub(super) fn handle_dialog_dismiss(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    send_action(
        &id,
        params,
        state,
        crate::ui::browser_panel::BrowserActionKind::SetDialogHandler {
            action: "dismiss".to_string(),
            prompt_text: None,
        },
    )
}

// ---------------------------------------------------------------------------
// Script / style injection
// ---------------------------------------------------------------------------

pub(super) fn handle_inject_script(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(script) = params.get("script").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'script'");
    };
    let script = truncate_browser_input(script);
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.send_ui_event(crate::app::UiEvent::BrowserAction {
        panel_id,
        action: crate::ui::browser_panel::BrowserActionKind::InjectScript {
            script: script.to_string(),
        },
    });
    Response::success(id, serde_json::json!({"ok": true}))
}

pub(super) fn handle_inject_style(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Some(css) = params.get("css").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'css'");
    };
    let css = truncate_browser_input(css);
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.send_ui_event(crate::app::UiEvent::BrowserAction {
        panel_id,
        action: crate::ui::browser_panel::BrowserActionKind::InjectStyle {
            css: css.to_string(),
        },
    });
    Response::success(id, serde_json::json!({"ok": true}))
}

pub(super) fn handle_remove_injected(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = match require_panel_id(&id, params) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.send_ui_event(crate::app::UiEvent::BrowserAction {
        panel_id,
        action: crate::ui::browser_panel::BrowserActionKind::RemoveInjected,
    });
    Response::success(id, serde_json::json!({"ok": true}))
}

// ---------------------------------------------------------------------------
// Geolocation / offline
// ---------------------------------------------------------------------------

pub(super) fn handle_geolocation_set(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let lat = params
        .get("latitude")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let lng = params
        .get("longitude")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let accuracy = params
        .get("accuracy")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let js = format!(
        "navigator.geolocation.getCurrentPosition = function(cb) {{ \
         cb({{coords:{{latitude:{lat},longitude:{lng},accuracy:{accuracy}}},timestamp:Date.now()}}); \
         }}; 'ok'"
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_offline_set(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let offline = params
        .get("offline")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let js = format!(
        "Object.defineProperty(navigator, 'onLine', {{value: {online}, configurable: true}}); \
         window.dispatchEvent(new Event('{event}')); 'ok'",
        online = !offline,
        event = if offline { "offline" } else { "online" }
    );
    send_eval_action(&id, params, state, js)
}
