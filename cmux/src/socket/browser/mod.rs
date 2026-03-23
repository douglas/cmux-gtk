//! Browser automation socket command handlers.
//!
//! All `browser.*` methods are routed here from `v2::dispatch()`.

mod helpers;
mod interaction;
mod navigation;
mod queries;
mod tabs;

use std::sync::Arc;

use serde_json::Value;

use crate::app::SharedState;

use super::v2::Response;

// Re-export the js() helper for use by other modules.
pub(crate) use helpers::js;

/// Dispatch a `browser.*` method. Returns `Some(Response)` if handled, `None` if unrecognized.
pub fn dispatch(
    method: &str,
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Option<Response> {
    let response = match method {
        "browser.navigate" => navigation::handle_navigate(id, params, state),
        "browser.execute_js" => navigation::handle_execute_js(id, params, state),
        "browser.get_url" => navigation::handle_get_url(id, params, state),
        "browser.get_text" => navigation::handle_get_text(id, params, state),
        "browser.back" => navigation::handle_back(id, params, state),
        "browser.forward" => navigation::handle_forward(id, params, state),
        "browser.reload" => navigation::handle_reload(id, params, state),
        "browser.set_zoom" => navigation::handle_set_zoom(id, params, state),
        "browser.screenshot" => navigation::handle_screenshot(id, params, state),
        // DOM interaction
        "browser.click" => interaction::handle_click(id, params, state),
        "browser.dblclick" => interaction::handle_dblclick(id, params, state),
        "browser.hover" => interaction::handle_hover(id, params, state),
        "browser.type" => interaction::handle_type(id, params, state),
        "browser.fill" => interaction::handle_fill(id, params, state),
        "browser.clear" => interaction::handle_clear(id, params, state),
        "browser.press" => interaction::handle_press(id, params, state),
        "browser.select_option" => interaction::handle_select_option(id, params, state),
        "browser.check" => interaction::handle_check(id, params, state),
        "browser.focus" => interaction::handle_focus(id, params, state),
        "browser.blur" => interaction::handle_blur(id, params, state),
        "browser.scroll_to" => interaction::handle_scroll_to(id, params, state),
        // Element queries
        "browser.get_html" => queries::handle_get_html(id, params, state),
        "browser.get_value" => queries::handle_get_value(id, params, state),
        "browser.get_attribute" => queries::handle_get_attribute(id, params, state),
        "browser.get_property" => queries::handle_get_property(id, params, state),
        "browser.get_bounding_box" => queries::handle_get_bounding_box(id, params, state),
        "browser.get_computed_style" => queries::handle_get_computed_style(id, params, state),
        "browser.is_visible" => queries::handle_is_visible(id, params, state),
        "browser.is_enabled" => queries::handle_is_enabled(id, params, state),
        "browser.is_checked" => queries::handle_is_checked(id, params, state),
        "browser.is_editable" => queries::handle_is_editable(id, params, state),
        "browser.count" => queries::handle_count(id, params, state),
        // Finders + element refs
        "browser.find" => queries::handle_find(id, params, state),
        "browser.find_all" => queries::handle_find_all(id, params, state),
        "browser.find_by_text" => queries::handle_find_by_text(id, params, state),
        "browser.find_by_role" => queries::handle_find_by_role(id, params, state),
        "browser.find_by_label" => queries::handle_find_by_label(id, params, state),
        "browser.find_by_placeholder" => queries::handle_find_by_placeholder(id, params, state),
        "browser.find_by_test_id" => queries::handle_find_by_test_id(id, params, state),
        "browser.release_ref" => queries::handle_release_ref(id, params, state),
        // Wait commands
        "browser.wait_for_selector" => navigation::handle_wait_for_selector(id, params, state),
        "browser.wait_for_navigation" => navigation::handle_wait_for_navigation(id, params, state),
        "browser.wait_for_load_state" => navigation::handle_wait_for_load_state(id, params, state),
        "browser.wait_for_function" => navigation::handle_wait_for_function(id, params, state),
        "browser.snapshot" => navigation::handle_snapshot(id, params, state),
        "browser.title" => navigation::handle_title(id, params, state),
        // Cookies & storage
        "browser.get_cookies" => queries::handle_get_cookies(id, params, state),
        "browser.set_cookie" => queries::handle_set_cookie(id, params, state),
        "browser.clear_cookies" => queries::handle_clear_cookies(id, params, state),
        "browser.local_storage_get" => queries::handle_local_storage_get(id, params, state),
        "browser.local_storage_set" => queries::handle_local_storage_set(id, params, state),
        "browser.session_storage_get" => queries::handle_session_storage_get(id, params, state),
        "browser.session_storage_set" => queries::handle_session_storage_set(id, params, state),
        // Console & injection
        "browser.get_console_messages" => queries::handle_get_console_messages(id, params, state),
        "browser.set_dialog_handler" => queries::handle_set_dialog_handler(id, params, state),
        "browser.inject_script" => queries::handle_inject_script(id, params, state),
        "browser.inject_style" => queries::handle_inject_style(id, params, state),
        "browser.remove_injected" => queries::handle_remove_injected(id, params, state),
        // Parity commands
        "browser.uncheck" => interaction::handle_uncheck(id, params, state),
        "browser.scroll" => interaction::handle_scroll(id, params, state),
        "browser.scroll_into_view" => interaction::handle_scroll_into_view(id, params, state),
        "browser.keydown" => interaction::handle_keydown(id, params, state),
        "browser.keyup" => interaction::handle_keyup(id, params, state),
        "browser.find.alt" => queries::handle_find_by_alt(id, params, state),
        "browser.find.title" => queries::handle_find_by_title(id, params, state),
        "browser.find.first" => queries::handle_find_first(id, params, state),
        "browser.find.last" => queries::handle_find_last(id, params, state),
        "browser.find.nth" => queries::handle_find_nth(id, params, state),
        "browser.frame.select" => tabs::handle_frame_select(id, params, state),
        "browser.frame.main" => tabs::handle_frame_main(id, params, state),
        "browser.dialog.accept" => queries::handle_dialog_accept(id, params, state),
        "browser.dialog.dismiss" => queries::handle_dialog_dismiss(id, params, state),
        "browser.highlight" => interaction::handle_highlight(id, params, state),
        "browser.console.clear" => queries::handle_console_clear(id, params, state),
        "browser.geolocation.set" => queries::handle_geolocation_set(id, params, state),
        "browser.offline.set" => queries::handle_offline_set(id, params, state),
        "browser.open_split" => tabs::handle_open_split(id, params, state),
        "browser.focus_webview" => tabs::handle_focus_webview(id, params, state),
        "browser.is_webview_focused" => tabs::handle_is_webview_focused(id, params, state),
        "browser.state.save" => tabs::handle_state_save(id, params, state),
        "browser.state.load" => tabs::handle_state_load(id, params, state),
        "browser.network.route" => tabs::handle_network_route(id, params, state),
        "browser.network.unroute" => tabs::handle_network_unroute(id, params, state),
        "browser.network.requests" => tabs::handle_network_requests(id, params, state),
        "browser.input_mouse" => interaction::handle_input_mouse(id, params, state),
        "browser.input_keyboard" => interaction::handle_input_keyboard(id, params, state),
        "browser.input_touch" => interaction::handle_input_touch(id, params, state),
        "browser.trace.start" => tabs::handle_trace_start(id, params, state),
        "browser.trace.stop" => tabs::handle_trace_stop(id, params, state),
        "browser.screencast.start" => tabs::handle_screencast_start(id, params, state),
        "browser.screencast.stop" => tabs::handle_screencast_stop(id, params, state),
        "browser.addinitscript" => queries::handle_inject_script(id, params, state),
        "browser.addscript" => queries::handle_inject_script(id, params, state),
        "browser.addstyle" => queries::handle_inject_style(id, params, state),
        "browser.tab.new" => tabs::handle_tab_new(id, params, state),
        "browser.tab.list" => tabs::handle_tab_list(id, params, state),
        "browser.tab.switch" => tabs::handle_tab_switch(id, params, state),
        "browser.tab.close" => tabs::handle_tab_close(id, params, state),
        "browser.viewport.set" => tabs::handle_viewport_set(id, params, state),
        "browser.download.wait" => tabs::handle_download_wait(id, params, state),
        "browser.errors.list" => tabs::handle_errors_list(id, params, state),
        // macOS cmux-compatible aliases
        "browser.eval" => navigation::handle_execute_js(id, params, state),
        "browser.select" => interaction::handle_select_option(id, params, state),
        "browser.find.text" => queries::handle_find_by_text(id, params, state),
        "browser.find.role" => queries::handle_find_by_role(id, params, state),
        "browser.find.label" => queries::handle_find_by_label(id, params, state),
        "browser.find.placeholder" => queries::handle_find_by_placeholder(id, params, state),
        "browser.find.testid" => queries::handle_find_by_test_id(id, params, state),
        "browser.get.text" => navigation::handle_get_text(id, params, state),
        "browser.get.html" => queries::handle_get_html(id, params, state),
        "browser.get.value" => queries::handle_get_value(id, params, state),
        "browser.get.attr" => queries::handle_get_attribute(id, params, state),
        "browser.get.title" => navigation::handle_title(id, params, state),
        "browser.get.count" => queries::handle_count(id, params, state),
        "browser.get.box" => queries::handle_get_bounding_box(id, params, state),
        "browser.get.styles" => queries::handle_get_computed_style(id, params, state),
        "browser.is.visible" => queries::handle_is_visible(id, params, state),
        "browser.is.enabled" => queries::handle_is_enabled(id, params, state),
        "browser.is.checked" => queries::handle_is_checked(id, params, state),
        "browser.storage.get" => queries::handle_local_storage_get(id, params, state),
        "browser.storage.set" => queries::handle_local_storage_set(id, params, state),
        "browser.console.list" => queries::handle_get_console_messages(id, params, state),
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
        "browser.find",
        "browser.find_all",
        "browser.find_by_text",
        "browser.find_by_role",
        "browser.find_by_label",
        "browser.find_by_placeholder",
        "browser.find_by_test_id",
        "browser.release_ref",
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
        "browser.tab.new",
        "browser.tab.list",
        "browser.tab.switch",
        "browser.tab.close",
        "browser.viewport.set",
        "browser.download.wait",
        "browser.errors.list",
        "browser.eval",
        "browser.select",
        "browser.find.text",
        "browser.find.role",
        "browser.find.label",
        "browser.find.placeholder",
        "browser.find.testid",
        "browser.get.text",
        "browser.get.html",
        "browser.get.value",
        "browser.get.attr",
        "browser.get.title",
        "browser.get.count",
        "browser.get.box",
        "browser.get.styles",
        "browser.is.visible",
        "browser.is.enabled",
        "browser.is.checked",
        "browser.storage.get",
        "browser.storage.set",
        "browser.console.list",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_helper_strings() {
        assert_eq!(helpers::js("hello"), r#""hello""#);
        assert_eq!(helpers::js(&42), "42");
        assert_eq!(helpers::js(&true), "true");
    }

    #[test]
    fn test_js_helper_special_chars() {
        assert_eq!(helpers::js("a\"b"), r#""a\"b""#);
        assert_eq!(helpers::js("a\\b"), r#""a\\b""#);
    }

    #[test]
    fn test_method_names_includes_aliases() {
        let names = method_names();
        assert!(names.contains(&"browser.navigate"));
        assert!(names.contains(&"browser.eval"));
        assert!(names.contains(&"browser.get.text"));
        assert!(names.contains(&"browser.find.text"));
    }
}
