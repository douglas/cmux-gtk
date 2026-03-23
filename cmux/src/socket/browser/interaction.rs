//! DOM interaction + keyboard/input handlers for browser automation.

use std::sync::Arc;

use serde_json::Value;

use super::helpers::*;
use super::Response;
use crate::app::SharedState;

pub(super) fn handle_click(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let button = params
        .get("button")
        .and_then(|v| v.as_str())
        .unwrap_or("left");
    let js = match button {
        "right" => format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.dispatchEvent(new MouseEvent('contextmenu', {{bubbles:true,cancelable:true,button:2}})); return 'ok'; }})()"#,
            sel = js(&selector)
        ),
        "middle" => format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.dispatchEvent(new MouseEvent('click', {{bubbles:true,cancelable:true,button:1}})); return 'ok'; }})()"#,
            sel = js(&selector)
        ),
        _ => format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.click(); return 'ok'; }})()"#,
            sel = js(&selector)
        ),
    };
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_dblclick(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.dispatchEvent(new MouseEvent('dblclick', {{bubbles:true,cancelable:true}})); return 'ok'; }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_hover(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.dispatchEvent(new MouseEvent('mouseover', {{bubbles:true}})); el.dispatchEvent(new MouseEvent('mouseenter', {{bubbles:false}})); return 'ok'; }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_type(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(text) = params.get("text").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'text'");
    };
    let text = truncate_browser_input(text);
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); var text = {text}; for(var i=0;i<text.length;i++){{ var ch=text[i]; el.dispatchEvent(new KeyboardEvent('keydown',{{key:ch,bubbles:true}})); el.dispatchEvent(new KeyboardEvent('keypress',{{key:ch,bubbles:true}})); if(el.value!==undefined) el.value+=ch; el.dispatchEvent(new KeyboardEvent('keyup',{{key:ch,bubbles:true}})); }} el.dispatchEvent(new Event('input',{{bubbles:true}})); return 'ok'; }})()"#,
        sel = js(&selector),
        text = js(text)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_fill(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(value) = params.get("value").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'value'");
    };
    let value = truncate_browser_input(value);
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); el.value = {val}; el.dispatchEvent(new Event('input',{{bubbles:true}})); el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
        sel = js(&selector),
        val = js(value)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_clear(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); el.value = ''; el.dispatchEvent(new Event('input',{{bubbles:true}})); el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_press(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'key'");
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); var opts = {{key:{key},bubbles:true,cancelable:true}}; el.dispatchEvent(new KeyboardEvent('keydown',opts)); el.dispatchEvent(new KeyboardEvent('keypress',opts)); el.dispatchEvent(new KeyboardEvent('keyup',opts)); return 'ok'; }})()"#,
        sel = js(&selector),
        key = js(key)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_select_option(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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
            sel = js(&selector),
            val = js(val)
        )
    } else if let Some(label) = by_label {
        format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; var opts = el.options; for(var i=0;i<opts.length;i++){{ if(opts[i].text==={label}){{ el.selectedIndex=i; break; }} }} el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
            sel = js(&selector),
            label = js(label)
        )
    } else if let Some(idx) = by_index {
        format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.selectedIndex = {idx}; el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
            sel = js(&selector),
            idx = idx
        )
    } else {
        return Response::error(id, "invalid_params", "Provide 'value', 'label', or 'index'");
    };
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_check(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let checked = params
        .get("checked")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.checked = {checked}; el.dispatchEvent(new Event('change',{{bubbles:true}})); return 'ok'; }})()"#,
        sel = js(&selector),
        checked = checked
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_uncheck(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

pub(super) fn handle_focus(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.focus(); return 'ok'; }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_blur(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = match require_selector(&id, params) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let js = format!(
        r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.blur(); return 'ok'; }})()"#,
        sel = js(&selector)
    );
    send_eval_action(&id, params, state, js)
}

pub(super) fn handle_scroll_to(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let selector = params.get("selector").and_then(|v| v.as_str());
    let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let js = if let Some(sel) = selector {
        format!(
            r#"(function(){{ var el = document.querySelector({sel}); if(!el) return 'ERROR:not_found'; el.scrollTo({x},{y}); return 'ok'; }})()"#,
            sel = js(sel),
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

/// browser.scroll -- Scroll the page by x/y pixels.
pub(super) fn handle_scroll(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let js = format!("window.scrollBy({x},{y}); JSON.stringify({{scrollX:window.scrollX,scrollY:window.scrollY}})");
    send_eval_action(&id, params, state, js)
}

/// browser.scroll_into_view -- Scroll element into view.
pub(super) fn handle_scroll_into_view(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
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

/// browser.keydown -- Dispatch keydown event.
pub(super) fn handle_keydown(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{{key:{key},bubbles:true}})); 'ok'",
        key = serde_json::to_string(key).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.keyup -- Dispatch keyup event.
pub(super) fn handle_keyup(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "document.activeElement.dispatchEvent(new KeyboardEvent('keyup',{{key:{key},bubbles:true}})); 'ok'",
        key = serde_json::to_string(key).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.highlight -- Temporarily outline an element.
pub(super) fn handle_highlight(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
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

/// browser.input_mouse -- Dispatch a synthetic mouse event at coordinates.
pub(super) fn handle_input_mouse(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let event_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("click");
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

/// browser.input_keyboard -- Dispatch a synthetic keyboard event.
pub(super) fn handle_input_keyboard(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let event_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("keypress");
    let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let js = format!(
        "document.activeElement.dispatchEvent(new KeyboardEvent('{event_type}', \
         {{key:{key},bubbles:true}})); 'ok'",
        key = serde_json::to_string(key).unwrap_or_default()
    );
    send_eval_action(&id, params, state, js)
}

/// browser.input_touch -- Dispatch a synthetic touch event.
pub(super) fn handle_input_touch(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let event_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("touchstart");
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
