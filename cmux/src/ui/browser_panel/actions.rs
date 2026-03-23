//! Browser automation actions — enum definition and dispatch logic.

use std::cell::Cell;
use std::rc::Rc;

use serde_json::Value;
use webkit6::prelude::*;

use super::registry::{get_webview, DialogHandler, CONSOLE_BUFFERS, DIALOG_HANDLERS};
use super::theme::apply_dark_mode;

// ---------------------------------------------------------------------------
// BrowserActionKind — all browser automation actions dispatched via UiEvent
// ---------------------------------------------------------------------------

/// A browser automation action sent from socket handlers to the GTK main thread.
///
/// Cannot derive Debug because variants contain `oneshot::Sender` which is not Debug.
impl std::fmt::Debug for BrowserActionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Navigate { url } => f.debug_struct("Navigate").field("url", url).finish(),
            Self::Eval { script, .. } => f.debug_struct("Eval").field("script", script).finish(),
            Self::GetUrl { .. } => write!(f, "GetUrl"),
            Self::GetText { .. } => write!(f, "GetText"),
            Self::GoBack => write!(f, "GoBack"),
            Self::GoForward => write!(f, "GoForward"),
            Self::Reload => write!(f, "Reload"),
            Self::SetZoom { zoom } => f.debug_struct("SetZoom").field("zoom", zoom).finish(),
            Self::ZoomIn => write!(f, "ZoomIn"),
            Self::ZoomOut => write!(f, "ZoomOut"),
            Self::WaitForSelector { selector, .. } => f
                .debug_struct("WaitForSelector")
                .field("selector", selector)
                .finish(),
            Self::WaitForNavigation { .. } => write!(f, "WaitForNavigation"),
            Self::WaitForLoadState { .. } => write!(f, "WaitForLoadState"),
            Self::WaitForFunction { expression, .. } => f
                .debug_struct("WaitForFunction")
                .field("expression", expression)
                .finish(),
            Self::GetConsoleMessages { .. } => write!(f, "GetConsoleMessages"),
            Self::SetDialogHandler { action, .. } => f
                .debug_struct("SetDialogHandler")
                .field("action", action)
                .finish(),
            Self::InjectScript { .. } => write!(f, "InjectScript"),
            Self::InjectStyle { .. } => write!(f, "InjectStyle"),
            Self::RemoveInjected => write!(f, "RemoveInjected"),
        }
    }
}

pub enum BrowserActionKind {
    // Phase 1: existing commands
    Navigate {
        url: String,
    },
    Eval {
        script: String,
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    },
    GetUrl {
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    },
    GetText {
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    },
    GoBack,
    GoForward,
    Reload,
    SetZoom {
        zoom: f64,
    },
    ZoomIn,
    ZoomOut,

    // Phase 5: Wait commands
    WaitForSelector {
        selector: String,
        timeout_ms: u64,
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    },
    WaitForNavigation {
        timeout_ms: u64,
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    },
    WaitForLoadState {
        timeout_ms: u64,
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    },
    WaitForFunction {
        expression: String,
        timeout_ms: u64,
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    },

    // Phase 5: Console & dialog hooks
    GetConsoleMessages {
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    },
    SetDialogHandler {
        action: String,
        prompt_text: Option<String>,
    },

    // Phase 5: Script & style injection
    InjectScript {
        script: String,
    },
    InjectStyle {
        css: String,
    },
    RemoveInjected,
}

// ---------------------------------------------------------------------------
// poll_js_until_truthy — shared poll-loop helper
// ---------------------------------------------------------------------------

/// Poll a JavaScript expression every 100 ms until it returns a non-empty
/// string, or timeout.  Used by WaitForSelector, WaitForLoadState, and
/// WaitForFunction to avoid duplicating the same poll-loop boilerplate.
fn poll_js_until_truthy(
    panel_id: uuid::Uuid,
    poll_js: &str,
    timeout_ms: u64,
    label: &str,
    success_value: serde_json::Value,
    reply: tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>,
) {
    let Some(wv) = get_webview(panel_id) else {
        let _ = reply.send(Err("Browser panel not found".to_string()));
        return;
    };
    let start = std::time::Instant::now();
    let deadline = std::time::Duration::from_millis(timeout_ms);
    let reply = Rc::new(Cell::new(Some(reply)));
    let reply_poll = reply.clone();
    let poll_js = poll_js.to_string();
    let label = label.to_string();
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        if start.elapsed() > deadline {
            if let Some(tx) = reply_poll.take() {
                let _ = tx.send(Err(format!("Timeout waiting for {label}")));
            }
            return glib::ControlFlow::Break;
        }
        let reply_inner = reply_poll.clone();
        let success = success_value.clone();
        wv.evaluate_javascript(
            &poll_js,
            None,
            None,
            None::<&gio::Cancellable>,
            move |result| {
                if let Ok(val) = result {
                    if !val.to_str().is_empty() {
                        if let Some(tx) = reply_inner.take() {
                            let _ = tx.send(Ok(success));
                        }
                    }
                }
            },
        );
        if reply_poll.take().is_none() {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

// ---------------------------------------------------------------------------
// execute_action — dispatches BrowserActionKind on the GTK main thread
// ---------------------------------------------------------------------------

/// Execute a browser automation action. Called from window.rs on the GTK main thread.
pub(crate) fn execute_action(panel_id: uuid::Uuid, action: BrowserActionKind) {
    match action {
        BrowserActionKind::Navigate { url } => {
            if let Some(wv) = get_webview(panel_id) {
                wv.load_uri(&url);
            }
        }
        BrowserActionKind::Eval { script, reply } => {
            if let Some(wv) = get_webview(panel_id) {
                wv.evaluate_javascript(
                    &script,
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    move |result| {
                        let resp = match result {
                            Ok(val) => Ok(Value::String(val.to_str().to_string())),
                            Err(e) => Err(e.to_string()),
                        };
                        let _ = reply.send(resp);
                    },
                );
            } else {
                let _ = reply.send(Err("Browser panel not found".to_string()));
            }
        }
        BrowserActionKind::GetUrl { reply } => {
            let result = get_webview(panel_id).and_then(|wv| wv.uri().map(|u| u.to_string()));
            match result {
                Some(url) => {
                    let _ = reply.send(Ok(serde_json::json!({"url": url})));
                }
                None => {
                    let _ = reply.send(Err("Browser panel not found".to_string()));
                }
            }
        }
        BrowserActionKind::GetText { reply } => {
            if let Some(wv) = get_webview(panel_id) {
                wv.evaluate_javascript(
                    "document.body.innerText",
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    move |result| {
                        let resp = match result {
                            Ok(val) => Ok(serde_json::json!({"text": val.to_str().to_string()})),
                            Err(e) => Err(e.to_string()),
                        };
                        let _ = reply.send(resp);
                    },
                );
            } else {
                let _ = reply.send(Err("Browser panel not found".to_string()));
            }
        }
        BrowserActionKind::GoBack => {
            if let Some(wv) = get_webview(panel_id) {
                wv.go_back();
            }
        }
        BrowserActionKind::GoForward => {
            if let Some(wv) = get_webview(panel_id) {
                wv.go_forward();
            }
        }
        BrowserActionKind::Reload => {
            if let Some(wv) = get_webview(panel_id) {
                wv.reload();
            }
        }
        BrowserActionKind::SetZoom { zoom } => {
            if let Some(wv) = get_webview(panel_id) {
                wv.set_zoom_level(zoom);
            }
        }
        BrowserActionKind::ZoomIn => {
            if let Some(wv) = get_webview(panel_id) {
                let new_zoom = (wv.zoom_level() + 0.1).min(5.0);
                wv.set_zoom_level(new_zoom);
            }
        }
        BrowserActionKind::ZoomOut => {
            if let Some(wv) = get_webview(panel_id) {
                let new_zoom = (wv.zoom_level() - 0.1).max(0.25);
                wv.set_zoom_level(new_zoom);
            }
        }
        BrowserActionKind::WaitForSelector {
            selector,
            timeout_ms,
            reply,
        } => {
            let sel_js = crate::socket::browser::js(&selector);
            let poll_js = format!(
                r#"(function(){{ return document.querySelector({sel_js}) ? 'found' : ''; }})()"#,
            );
            poll_js_until_truthy(
                panel_id,
                &poll_js,
                timeout_ms,
                "selector",
                serde_json::json!({"found": true}),
                reply,
            );
        }
        BrowserActionKind::WaitForNavigation { timeout_ms, reply } => {
            if let Some(wv) = get_webview(panel_id) {
                let reply = Rc::new(Cell::new(Some(reply)));
                let reply_timeout = reply.clone();

                // Listen for load-changed FINISHED
                let handler_id: Rc<Cell<Option<glib::SignalHandlerId>>> = Rc::new(Cell::new(None));
                let handler_id_clone = handler_id.clone();
                let reply_signal = reply.clone();
                let wv_clone = wv.clone();
                let sig = wv.connect_load_changed(move |_wv, event| {
                    if matches!(event, webkit6::LoadEvent::Finished) {
                        if let Some(tx) = reply_signal.take() {
                            let _ = tx.send(Ok(serde_json::json!({"navigated": true})));
                        }
                        if let Some(hid) = handler_id_clone.take() {
                            wv_clone.disconnect(hid);
                        }
                    }
                });
                handler_id.set(Some(sig));

                // Timeout
                let wv_for_timeout = wv.clone();
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(timeout_ms),
                    move || {
                        if let Some(tx) = reply_timeout.take() {
                            let _ = tx.send(Err("Timeout waiting for navigation".to_string()));
                        }
                        if let Some(hid) = handler_id.take() {
                            wv_for_timeout.disconnect(hid);
                        }
                    },
                );
            } else {
                let _ = reply.send(Err("Browser panel not found".to_string()));
            }
        }
        BrowserActionKind::WaitForLoadState { timeout_ms, reply } => {
            let poll_js =
                r#"(function(){ return document.readyState === 'complete' ? 'complete' : ''; })()"#
                    .to_string();
            poll_js_until_truthy(
                panel_id,
                &poll_js,
                timeout_ms,
                "load state",
                serde_json::json!({"state": "complete"}),
                reply,
            );
        }
        BrowserActionKind::WaitForFunction {
            expression,
            timeout_ms,
            reply,
        } => {
            let poll_js = format!(r#"(function(){{ return ({expression}) ? 'truthy' : ''; }})()"#,);
            poll_js_until_truthy(
                panel_id,
                &poll_js,
                timeout_ms,
                "function",
                serde_json::json!({"result": true}),
                reply,
            );
        }
        BrowserActionKind::GetConsoleMessages { reply } => {
            let messages = CONSOLE_BUFFERS
                .with(|bufs| bufs.borrow().get(&panel_id).cloned().unwrap_or_default());
            let _ = reply.send(Ok(serde_json::json!({"messages": messages})));
        }
        BrowserActionKind::SetDialogHandler {
            action,
            prompt_text,
        } => {
            DIALOG_HANDLERS.with(|handlers| {
                handlers.borrow_mut().insert(
                    panel_id,
                    DialogHandler {
                        action,
                        prompt_text,
                    },
                );
            });
        }
        BrowserActionKind::InjectScript { script } => {
            if let Some(wv) = get_webview(panel_id) {
                let user_script = webkit6::UserScript::new(
                    &script,
                    webkit6::UserContentInjectedFrames::AllFrames,
                    webkit6::UserScriptInjectionTime::End,
                    &[],
                    &[],
                );
                if let Some(ucm) = wv.user_content_manager() {
                    ucm.add_script(&user_script);
                }
            }
        }
        BrowserActionKind::InjectStyle { css } => {
            if let Some(wv) = get_webview(panel_id) {
                let stylesheet = webkit6::UserStyleSheet::new(
                    &css,
                    webkit6::UserContentInjectedFrames::AllFrames,
                    webkit6::UserStyleLevel::User,
                    &[],
                    &[],
                );
                if let Some(ucm) = wv.user_content_manager() {
                    ucm.add_style_sheet(&stylesheet);
                }
            }
        }
        BrowserActionKind::RemoveInjected => {
            if let Some(wv) = get_webview(panel_id) {
                if let Some(ucm) = wv.user_content_manager() {
                    ucm.remove_all_scripts();
                    ucm.remove_all_style_sheets();
                    // Re-apply dark mode stylesheet if needed
                    apply_dark_mode(&wv);
                }
            }
        }
    }
}
