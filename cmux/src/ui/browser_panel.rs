//! Browser panel — embedded WebKit browser (webkit6 / WebKitGTK 6.0).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use gdk4;
use glib::object::Cast;
use gtk4::prelude::*;
use serde_json::Value;
use webkit6::prelude::*;

use crate::settings;

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
            Self::Eval { script, .. } => {
                f.debug_struct("Eval").field("script", script).finish()
            }
            Self::GetUrl { .. } => write!(f, "GetUrl"),
            Self::GetText { .. } => write!(f, "GetText"),
            Self::GoBack => write!(f, "GoBack"),
            Self::GoForward => write!(f, "GoForward"),
            Self::Reload => write!(f, "Reload"),
            Self::SetZoom { zoom } => f.debug_struct("SetZoom").field("zoom", zoom).finish(),
            Self::ZoomIn => write!(f, "ZoomIn"),
            Self::ZoomOut => write!(f, "ZoomOut"),
            Self::WaitForSelector { selector, .. } => {
                f.debug_struct("WaitForSelector").field("selector", selector).finish()
            }
            Self::WaitForNavigation { .. } => write!(f, "WaitForNavigation"),
            Self::WaitForLoadState { .. } => write!(f, "WaitForLoadState"),
            Self::WaitForFunction { expression, .. } => {
                f.debug_struct("WaitForFunction").field("expression", expression).finish()
            }
            Self::GetConsoleMessages { .. } => write!(f, "GetConsoleMessages"),
            Self::SetDialogHandler { action, .. } => {
                f.debug_struct("SetDialogHandler").field("action", action).finish()
            }
            Self::InjectScript { .. } => write!(f, "InjectScript"),
            Self::InjectStyle { .. } => write!(f, "InjectStyle"),
            Self::RemoveInjected => write!(f, "RemoveInjected"),
        }
    }
}

pub enum BrowserActionKind {
    // Phase 1: existing commands
    Navigate { url: String },
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
    SetZoom { zoom: f64 },
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
    InjectScript { script: String },
    InjectStyle { css: String },
    RemoveInjected,
}

// ---------------------------------------------------------------------------
// WebView registry
// ---------------------------------------------------------------------------

thread_local! {
    /// Registry of panel_id → WebView for browser automation socket commands.
    static WEBVIEW_REGISTRY: RefCell<HashMap<uuid::Uuid, webkit6::WebView>> = RefCell::new(HashMap::new());

    /// Element reference registry: "@e1" → ElementRef
    static ELEMENT_REFS: RefCell<HashMap<String, ElementRef>> = RefCell::new(HashMap::new());

    /// Next element ref ID counter.
    static NEXT_REF_ID: Cell<u64> = const { Cell::new(1) };

    /// Per-panel console message ring buffer (last 100 messages).
    static CONSOLE_BUFFERS: RefCell<HashMap<uuid::Uuid, Vec<String>>> = RefCell::new(HashMap::new());

    /// Per-panel dialog handler config.
    static DIALOG_HANDLERS: RefCell<HashMap<uuid::Uuid, DialogHandler>> = RefCell::new(HashMap::new());

    /// Per-panel favicon textures (updated on WebView favicon-notify signal).
    static FAVICON_CACHE: RefCell<HashMap<uuid::Uuid, gdk4::Texture>> = RefCell::new(HashMap::new());

    /// Per-panel console pane widgets (for toggle via UiEvent).
    static CONSOLE_PANELS: RefCell<HashMap<uuid::Uuid, gtk4::Box>> = RefCell::new(HashMap::new());

    /// Per-panel console TextViews (for appending messages).
    static CONSOLE_TEXT_VIEWS: RefCell<HashMap<uuid::Uuid, gtk4::TextView>> = RefCell::new(HashMap::new());

    /// Per-panel download bar widgets.
    static DOWNLOAD_BARS: RefCell<HashMap<uuid::Uuid, DownloadBarWidgets>> = RefCell::new(HashMap::new());

    /// Per-panel last downloaded file path (for "Open" button).
    static DOWNLOAD_PATHS: RefCell<HashMap<uuid::Uuid, String>> = RefCell::new(HashMap::new());
}

struct DownloadBarWidgets {
    container: gtk4::Box,
    label: gtk4::Label,
    progress: gtk4::ProgressBar,
    open_btn: gtk4::Button,
}

struct ElementRef {
    #[allow(dead_code)]
    panel_id: uuid::Uuid,
    selector: String,
}

struct DialogHandler {
    action: String,         // "accept" or "dismiss"
    prompt_text: Option<String>,
}

/// Shared persistent NetworkSession — cookies and storage persist across panels and restarts.
/// Data stored at `~/.local/share/cmux/webkit/`.
fn shared_network_session() -> webkit6::NetworkSession {
    thread_local! {
        static SESSION: RefCell<Option<webkit6::NetworkSession>> = const { RefCell::new(None) };
    }
    SESSION.with(|s| {
        let mut slot = s.borrow_mut();
        if let Some(ref session) = *slot {
            return session.clone();
        }
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"))
            .join("cmux/webkit");
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("~/.cache"))
            .join("cmux/webkit");
        let session = webkit6::NetworkSession::new(
            Some(data_dir.to_str().unwrap_or("~/.local/share/cmux/webkit")),
            Some(cache_dir.to_str().unwrap_or("~/.cache/cmux/webkit")),
        );
        wire_download_handling(&session);
        *slot = Some(session.clone());
        session
    })
}

/// Look up the WebView for a panel_id (GTK main thread only).
pub fn get_webview(panel_id: uuid::Uuid) -> Option<webkit6::WebView> {
    WEBVIEW_REGISTRY.with(|r| r.borrow().get(&panel_id).cloned())
}

/// Remove a panel from the WebView registry.
#[allow(dead_code)]
pub fn unregister_webview(panel_id: uuid::Uuid) {
    WEBVIEW_REGISTRY.with(|r| r.borrow_mut().remove(&panel_id));
}

/// Collect current zoom levels for all browser panels (for session snapshots).
pub fn collect_webview_zoom_levels() -> HashMap<uuid::Uuid, f64> {
    WEBVIEW_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .map(|(&id, wv)| (id, wv.zoom_level()))
            .collect()
    })
}

/// Toggle the JS console panel for a browser panel.
pub fn toggle_console(panel_id: uuid::Uuid) {
    CONSOLE_PANELS.with(|c| {
        if let Some(pane) = c.borrow().get(&panel_id) {
            pane.set_visible(!pane.is_visible());
        }
    });
}

/// Get the cached favicon texture for a browser panel (if available).
pub fn get_favicon(panel_id: uuid::Uuid) -> Option<gdk4::Texture> {
    FAVICON_CACHE.with(|c| c.borrow().get(&panel_id).cloned())
}

/// Collect current URLs for all browser panels (for session snapshots).
pub fn collect_webview_urls() -> HashMap<uuid::Uuid, String> {
    WEBVIEW_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .filter_map(|(&id, wv)| wv.uri().map(|u| (id, u.to_string())))
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Element ref management (called from socket thread via send_ui_event results)
// ---------------------------------------------------------------------------

/// Allocate a new element ref and return its ID (e.g. "@e1").
pub fn allocate_ref(panel_id: uuid::Uuid, selector: &str) -> String {
    ELEMENT_REFS.with(|refs| {
        NEXT_REF_ID.with(|id_cell| {
            let id = id_cell.get();
            id_cell.set(id + 1);
            let ref_id = format!("@e{}", id);
            refs.borrow_mut().insert(
                ref_id.clone(),
                ElementRef {
                    panel_id,
                    selector: selector.to_string(),
                },
            );
            ref_id
        })
    })
}

/// Release (remove) an element ref. Returns true if it existed.
pub fn release_ref(ref_id: &str) -> bool {
    ELEMENT_REFS.with(|refs| refs.borrow_mut().remove(ref_id).is_some())
}

/// Resolve a selector: if it starts with "@e", look up the stored CSS selector.
/// Otherwise return it as-is.
pub fn resolve_selector(selector: &str) -> Option<String> {
    if selector.starts_with("@e") {
        ELEMENT_REFS.with(|refs| {
            refs.borrow()
                .get(selector)
                .map(|r| r.selector.clone())
        })
    } else {
        Some(selector.to_string())
    }
}

/// Clear all element refs for a given panel (called on navigation).
pub fn clear_refs_for_panel(panel_id: uuid::Uuid) {
    ELEMENT_REFS.with(|refs| {
        refs.borrow_mut().retain(|_, v| v.panel_id != panel_id);
    });
}

// ---------------------------------------------------------------------------
// Download handling
// ---------------------------------------------------------------------------

/// Reverse-lookup the panel_id for a WebView in the registry.
fn panel_id_for_webview(wv: &webkit6::WebView) -> Option<uuid::Uuid> {
    WEBVIEW_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .find(|(_, v)| *v == wv)
            .map(|(&id, _)| id)
    })
}

/// Pick a unique download path in `dir`, appending " (1)", " (2)", etc. if needed.
fn unique_download_path(dir: &Path, filename: &str) -> std::path::PathBuf {
    let path = dir.join(filename);
    if !path.exists() {
        return path;
    }
    let stem = Path::new(filename)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let ext = Path::new(filename)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..1000 {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem} (dup){ext}"))
}

/// Update download bar UI for a panel.
fn update_download_bar(panel_id: uuid::Uuid, text: &str, fraction: f64, show_open: bool) {
    DOWNLOAD_BARS.with(|bars| {
        if let Some(bar) = bars.borrow().get(&panel_id) {
            bar.container.set_visible(true);
            bar.label.set_text(text);
            bar.progress.set_fraction(fraction);
            bar.open_btn.set_visible(show_open);
        }
    });
}

/// Wire download-started on the shared NetworkSession (called once on creation).
fn wire_download_handling(session: &webkit6::NetworkSession) {
    session.connect_download_started(|_session, download| {
        let panel_id = download
            .web_view()
            .and_then(|wv| panel_id_for_webview(&wv));

        if let Some(pid) = panel_id {
            update_download_bar(pid, "Starting download…", 0.0, false);
        }

        // decide-destination: auto-save to ~/Downloads with dedup
        let pid_dest = panel_id;
        download.connect_decide_destination(move |dl, suggested_filename| {
            let downloads_dir = dirs::download_dir().unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                    .join("Downloads")
            });
            std::fs::create_dir_all(&downloads_dir).ok();

            let path = unique_download_path(&downloads_dir, suggested_filename);
            let dest = format!("file://{}", path.to_string_lossy());
            dl.set_allow_overwrite(false);
            dl.set_destination(&dest);

            if let Some(pid) = pid_dest {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                update_download_bar(pid, &format!("Downloading: {filename}"), 0.0, false);
            }
            true
        });

        // Progress tracking
        if let Some(pid) = panel_id {
            download.connect_estimated_progress_notify(move |dl| {
                let progress = dl.estimated_progress();
                let filename = dl
                    .destination()
                    .map(|d| {
                        let s = d.to_string();
                        let p = s.strip_prefix("file://").unwrap_or(&s);
                        Path::new(p)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                let pct = (progress * 100.0).round() as u32;
                update_download_bar(
                    pid,
                    &format!("Downloading: {filename} — {pct}%"),
                    progress,
                    false,
                );
            });
        }

        // Finished
        if let Some(pid) = panel_id {
            download.connect_finished(move |dl| {
                let dest_path = dl.destination().map(|d| {
                    let s = d.to_string();
                    s.strip_prefix("file://")
                        .unwrap_or(&s)
                        .to_string()
                });
                let filename = dest_path
                    .as_deref()
                    .and_then(|p| Path::new(p).file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".to_string());

                update_download_bar(pid, &format!("Downloaded: {filename}"), 1.0, true);

                // Store the path for the Open/Show buttons
                if let Some(path) = dest_path {
                    DOWNLOAD_PATHS.with(|paths| {
                        paths.borrow_mut().insert(pid, path);
                    });
                }

                // Auto-hide after 8 seconds
                glib::timeout_add_local_once(
                    std::time::Duration::from_secs(8),
                    move || {
                        DOWNLOAD_BARS.with(|bars| {
                            if let Some(bar) = bars.borrow().get(&pid) {
                                bar.container.set_visible(false);
                            }
                        });
                    },
                );
            });
        }

        // Failed
        if let Some(pid) = panel_id {
            download.connect_failed(move |_dl, error| {
                let msg = error.message();
                update_download_bar(
                    pid,
                    &format!("Download failed: {msg}"),
                    0.0,
                    false,
                );
            });
        }
    });
}

// ---------------------------------------------------------------------------
// execute_action — dispatches BrowserActionKind on the GTK main thread
// ---------------------------------------------------------------------------

/// Execute a browser automation action. Called from window.rs on the GTK main thread.
pub fn execute_action(panel_id: uuid::Uuid, action: BrowserActionKind) {
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
            let result = get_webview(panel_id)
                .and_then(|wv| wv.uri().map(|u| u.to_string()));
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
            if let Some(wv) = get_webview(panel_id) {
                let start = std::time::Instant::now();
                let deadline = std::time::Duration::from_millis(timeout_ms);
                let reply = Rc::new(Cell::new(Some(reply)));
                let sel_js = serde_json::to_string(&selector).unwrap();
                let poll_js = format!(
                    r#"(function(){{ return document.querySelector({}) ? 'found' : ''; }})()"#,
                    sel_js
                );
                let reply_clone = reply.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    if start.elapsed() > deadline {
                        if let Some(tx) = reply_clone.take() {
                            let _ = tx.send(Err("Timeout waiting for selector".to_string()));
                        }
                        return glib::ControlFlow::Break;
                    }
                    let reply_inner = reply_clone.clone();
                    let poll_js_clone = poll_js.clone();
                    wv.evaluate_javascript(
                        &poll_js_clone,
                        None,
                        None,
                        None::<&gio::Cancellable>,
                        move |result| {
                            if let Ok(val) = result {
                                let s = val.to_str();
                                if s.as_str() == "found" {
                                    if let Some(tx) = reply_inner.take() {
                                        let _ = tx.send(Ok(
                                            serde_json::json!({"found": true}),
                                        ));
                                    }
                                }
                            }
                        },
                    );
                    if reply_clone.take().is_none() {
                        // Already replied in the callback
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
            } else {
                let _ = reply.send(Err("Browser panel not found".to_string()));
            }
        }
        BrowserActionKind::WaitForNavigation {
            timeout_ms,
            reply,
        } => {
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
        BrowserActionKind::WaitForLoadState {
            timeout_ms,
            reply,
        } => {
            if let Some(wv) = get_webview(panel_id) {
                let start = std::time::Instant::now();
                let deadline = std::time::Duration::from_millis(timeout_ms);
                let reply = Rc::new(Cell::new(Some(reply)));
                let reply_clone = reply.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    if start.elapsed() > deadline {
                        if let Some(tx) = reply_clone.take() {
                            let _ = tx.send(Err("Timeout waiting for load state".to_string()));
                        }
                        return glib::ControlFlow::Break;
                    }
                    let reply_inner = reply_clone.clone();
                    wv.evaluate_javascript(
                        "document.readyState",
                        None,
                        None,
                        None::<&gio::Cancellable>,
                        move |result| {
                            if let Ok(val) = result {
                                if val.to_str().as_str() == "complete" {
                                    if let Some(tx) = reply_inner.take() {
                                        let _ = tx.send(Ok(
                                            serde_json::json!({"state": "complete"}),
                                        ));
                                    }
                                }
                            }
                        },
                    );
                    if reply_clone.take().is_none() {
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
            } else {
                let _ = reply.send(Err("Browser panel not found".to_string()));
            }
        }
        BrowserActionKind::WaitForFunction {
            expression,
            timeout_ms,
            reply,
        } => {
            if let Some(wv) = get_webview(panel_id) {
                let start = std::time::Instant::now();
                let deadline = std::time::Duration::from_millis(timeout_ms);
                let reply = Rc::new(Cell::new(Some(reply)));
                let poll_js = format!(
                    r#"(function(){{ return ({}) ? 'truthy' : ''; }})()"#,
                    expression
                );
                let reply_clone = reply.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    if start.elapsed() > deadline {
                        if let Some(tx) = reply_clone.take() {
                            let _ = tx.send(Err("Timeout waiting for function".to_string()));
                        }
                        return glib::ControlFlow::Break;
                    }
                    let reply_inner = reply_clone.clone();
                    let poll_js_clone = poll_js.clone();
                    wv.evaluate_javascript(
                        &poll_js_clone,
                        None,
                        None,
                        None::<&gio::Cancellable>,
                        move |result| {
                            if let Ok(val) = result {
                                if val.to_str().as_str() == "truthy" {
                                    if let Some(tx) = reply_inner.take() {
                                        let _ = tx.send(Ok(
                                            serde_json::json!({"result": true}),
                                        ));
                                    }
                                }
                            }
                        },
                    );
                    if reply_clone.take().is_none() {
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
            } else {
                let _ = reply.send(Err("Browser panel not found".to_string()));
            }
        }
        BrowserActionKind::GetConsoleMessages { reply } => {
            let messages = CONSOLE_BUFFERS.with(|bufs| {
                bufs.borrow()
                    .get(&panel_id)
                    .cloned()
                    .unwrap_or_default()
            });
            let _ = reply.send(Ok(serde_json::json!({"messages": messages})));
        }
        BrowserActionKind::SetDialogHandler { action, prompt_text } => {
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

/// Create an embedded browser panel widget.
///
/// Layout:
/// ```text
/// VBox:
///   ├─ nav_bar (HBox): [back] [fwd] [reload/stop] [home] [url_entry] [find] [devtools]
///   ├─ progress_bar (ProgressBar): thin load indicator
///   ├─ find_bar (HBox): [find_entry] [prev] [next] [match_count] [close]  (hidden by default)
///   └─ web_view (WebView): fills remaining space
/// ```
pub fn create_browser_widget(
    panel_id: uuid::Uuid,
    initial_url: Option<&str>,
    is_attention_source: bool,
    initial_zoom: Option<f64>,
) -> gtk4::Widget {
    let browser_settings = settings::load().browser;

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.add_css_class("panel-shell");
    if is_attention_source {
        container.add_css_class("attention-panel");
    }

    // ── Navigation bar ──
    let nav_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    nav_bar.add_css_class("browser-nav-bar");
    nav_bar.set_margin_start(4);
    nav_bar.set_margin_end(4);
    nav_bar.set_margin_top(2);
    nav_bar.set_margin_bottom(2);

    let back_btn = gtk4::Button::from_icon_name("go-previous-symbolic");
    back_btn.set_tooltip_text(Some("Back"));
    back_btn.set_sensitive(false);
    back_btn.add_css_class("flat");
    nav_bar.append(&back_btn);

    let fwd_btn = gtk4::Button::from_icon_name("go-next-symbolic");
    fwd_btn.set_tooltip_text(Some("Forward"));
    fwd_btn.set_sensitive(false);
    fwd_btn.add_css_class("flat");
    nav_bar.append(&fwd_btn);

    let reload_btn = gtk4::Button::from_icon_name("view-refresh-symbolic");
    reload_btn.set_tooltip_text(Some("Reload"));
    reload_btn.add_css_class("flat");
    nav_bar.append(&reload_btn);

    let url_entry = gtk4::Entry::new();
    url_entry.set_hexpand(true);
    url_entry.set_placeholder_text(Some("Enter URL or search..."));
    url_entry.add_css_class("browser-url-entry");
    if let Some(url) = initial_url {
        url_entry.set_text(url);
    }
    nav_bar.append(&url_entry);

    let find_toggle_btn = gtk4::ToggleButton::new();
    find_toggle_btn.set_icon_name("edit-find-symbolic");
    find_toggle_btn.set_tooltip_text(Some("Find in Page (Ctrl+F)"));
    find_toggle_btn.add_css_class("flat");
    nav_bar.append(&find_toggle_btn);

    let zoom_out_btn = gtk4::Button::from_icon_name("zoom-out-symbolic");
    zoom_out_btn.set_tooltip_text(Some("Zoom Out (Ctrl+-)"));
    zoom_out_btn.add_css_class("flat");
    nav_bar.append(&zoom_out_btn);

    let zoom_label = gtk4::Label::new(Some("100%"));
    zoom_label.set_tooltip_text(Some("Reset Zoom (Ctrl+0)"));
    zoom_label.add_css_class("dim-label");
    zoom_label.set_width_chars(5);
    nav_bar.append(&zoom_label);

    let zoom_in_btn = gtk4::Button::from_icon_name("zoom-in-symbolic");
    zoom_in_btn.set_tooltip_text(Some("Zoom In (Ctrl+=)"));
    zoom_in_btn.add_css_class("flat");
    nav_bar.append(&zoom_in_btn);

    let devtools_btn = gtk4::ToggleButton::new();
    devtools_btn.set_icon_name("utilities-terminal-symbolic");
    devtools_btn.set_tooltip_text(Some("Developer Tools"));
    devtools_btn.add_css_class("flat");
    nav_bar.append(&devtools_btn);

    container.append(&nav_bar);

    // ── Progress bar ──
    let progress_bar = gtk4::ProgressBar::new();
    progress_bar.add_css_class("osd");
    progress_bar.set_visible(false);
    container.append(&progress_bar);

    // ── Find bar (hidden by default) ──
    let find_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    find_bar.set_margin_start(4);
    find_bar.set_margin_end(4);
    find_bar.set_margin_top(2);
    find_bar.set_margin_bottom(2);
    find_bar.set_visible(false);

    let find_entry = gtk4::SearchEntry::new();
    find_entry.set_hexpand(true);
    find_entry.set_placeholder_text(Some("Find in page..."));
    find_bar.append(&find_entry);

    let find_prev_btn = gtk4::Button::from_icon_name("go-up-symbolic");
    find_prev_btn.set_tooltip_text(Some("Previous Match"));
    find_bar.append(&find_prev_btn);

    let find_next_btn = gtk4::Button::from_icon_name("go-down-symbolic");
    find_next_btn.set_tooltip_text(Some("Next Match"));
    find_bar.append(&find_next_btn);

    let match_label = gtk4::Label::new(None);
    match_label.add_css_class("dim-label");
    find_bar.append(&match_label);

    let find_close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
    find_close_btn.set_tooltip_text(Some("Close Find"));
    find_bar.append(&find_close_btn);

    container.append(&find_bar);

    // ── WebView (shared persistent session for cookie/storage persistence) ──
    let web_view = webkit6::WebView::builder()
        .network_session(&shared_network_session())
        .build();
    web_view.set_hexpand(true);
    web_view.set_vexpand(true);

    // Restore zoom level from session if available
    if let Some(zoom) = initial_zoom {
        if zoom > 0.0 && zoom != 1.0 {
            web_view.set_zoom_level(zoom);
        }
    }

    // Register in the thread-local WebView registry for socket command access
    WEBVIEW_REGISTRY.with(|r| r.borrow_mut().insert(panel_id, web_view.clone()));

    // Enable developer extras for inspector
    if let Some(ws) = webkit6::prelude::WebViewExt::settings(&web_view) {
        ws.set_enable_developer_extras(true);
    }

    // Apply dark mode stylesheet if system is dark
    apply_dark_mode(&web_view);

    // ── Console capture: inject script to intercept console.* and post to Rust ──
    if let Some(ucm) = web_view.user_content_manager() {
        ucm.register_script_message_handler("cmux_console", None);
        let console_script = webkit6::UserScript::new(
            r#"(function(){
                var orig = {log: console.log, warn: console.warn, error: console.error, info: console.info};
                function hook(level) {
                    return function() {
                        orig[level].apply(console, arguments);
                        try {
                            var msg = Array.prototype.map.call(arguments, function(a){
                                return typeof a === 'string' ? a : JSON.stringify(a);
                            }).join(' ');
                            window.webkit.messageHandlers.cmux_console.postMessage(level + ': ' + msg);
                        } catch(e) {}
                    };
                }
                console.log = hook('log');
                console.warn = hook('warn');
                console.error = hook('error');
                console.info = hook('info');
            })();"#,
            webkit6::UserContentInjectedFrames::AllFrames,
            webkit6::UserScriptInjectionTime::Start,
            &[],
            &[],
        );
        ucm.add_script(&console_script);

        ucm.connect_script_message_received(Some("cmux_console"), move |_ucm, value| {
            let message = value.to_str().to_string();
            CONSOLE_BUFFERS.with(|bufs| {
                let mut map = bufs.borrow_mut();
                let buf = map.entry(panel_id).or_insert_with(Vec::new);
                buf.push(message.clone());
                if buf.len() > 100 {
                    buf.remove(0);
                }
            });
            // Append to the in-app console text view
            CONSOLE_TEXT_VIEWS.with(|tvs| {
                if let Some(tv) = tvs.borrow().get(&panel_id) {
                    let buf = tv.buffer();
                    let mut end = buf.end_iter();
                    buf.insert(&mut end, &message);
                    buf.insert(&mut end, "\n");
                    // Auto-scroll to bottom
                    if let Some(mark) = buf.mark("insert") {
                        tv.scroll_to_mark(&mark, 0.0, false, 0.0, 0.0);
                    }
                }
            });
        });
    }

    // ── JS Console panel (collapsible, below WebView) ──
    let console_pane = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    console_pane.add_css_class("browser-console-pane");
    console_pane.set_visible(false);
    console_pane.set_size_request(-1, 150);

    let console_header = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    console_header.set_margin_start(6);
    console_header.set_margin_end(6);
    console_header.set_margin_top(2);
    console_header.set_margin_bottom(2);
    let console_label = gtk4::Label::new(Some("Console"));
    console_label.add_css_class("heading");
    console_header.append(&console_label);
    let console_clear_btn = gtk4::Button::from_icon_name("edit-clear-symbolic");
    console_clear_btn.set_tooltip_text(Some("Clear Console"));
    console_clear_btn.add_css_class("flat");
    console_header.append(&console_clear_btn);
    console_pane.append(&console_header);

    let console_scroll = gtk4::ScrolledWindow::new();
    console_scroll.set_vexpand(true);
    console_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    let console_text_view = gtk4::TextView::new();
    console_text_view.set_editable(false);
    console_text_view.set_monospace(true);
    console_text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
    console_text_view.set_margin_start(6);
    console_text_view.set_margin_end(6);
    console_scroll.set_child(Some(&console_text_view));
    console_pane.append(&console_scroll);

    // Clear button clears the text view and buffer
    {
        let tv = console_text_view.clone();
        console_clear_btn.connect_clicked(move |_| {
            tv.buffer().set_text("");
            CONSOLE_BUFFERS.with(|bufs| {
                bufs.borrow_mut().remove(&panel_id);
            });
        });
    }

    // Store console pane and text view references for toggle and message appending
    CONSOLE_PANELS.with(|c| c.borrow_mut().insert(panel_id, console_pane.clone()));
    CONSOLE_TEXT_VIEWS.with(|c| c.borrow_mut().insert(panel_id, console_text_view.clone()));

    // ── Download bar (hidden by default, shown when a download starts) ──
    let download_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    download_bar.add_css_class("browser-download-bar");
    download_bar.set_margin_start(6);
    download_bar.set_margin_end(6);
    download_bar.set_margin_top(2);
    download_bar.set_margin_bottom(2);
    download_bar.set_visible(false);

    let dl_icon = gtk4::Image::from_icon_name("folder-download-symbolic");
    download_bar.append(&dl_icon);

    let dl_label = gtk4::Label::new(None);
    dl_label.set_hexpand(true);
    dl_label.set_xalign(0.0);
    dl_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    download_bar.append(&dl_label);

    let dl_progress = gtk4::ProgressBar::new();
    dl_progress.set_width_request(120);
    dl_progress.set_valign(gtk4::Align::Center);
    download_bar.append(&dl_progress);

    let dl_open_btn = gtk4::Button::with_label("Open");
    dl_open_btn.add_css_class("flat");
    dl_open_btn.set_visible(false);
    dl_open_btn.set_tooltip_text(Some("Open downloaded file"));
    {
        dl_open_btn.connect_clicked(move |_| {
            let path = DOWNLOAD_PATHS.with(|paths| {
                paths.borrow().get(&panel_id).cloned()
            });
            if let Some(path) = path {
                let _ = gio::AppInfo::launch_default_for_uri(
                    &format!("file://{path}"),
                    gio::AppLaunchContext::NONE,
                );
            }
        });
    }
    download_bar.append(&dl_open_btn);

    let dl_show_folder_btn = gtk4::Button::from_icon_name("folder-open-symbolic");
    dl_show_folder_btn.add_css_class("flat");
    dl_show_folder_btn.set_tooltip_text(Some("Show in file manager"));
    {
        dl_show_folder_btn.connect_clicked(move |_| {
            let path = DOWNLOAD_PATHS.with(|paths| {
                paths.borrow().get(&panel_id).cloned()
            });
            if let Some(path) = path {
                if let Some(parent) = Path::new(&path).parent() {
                    let _ = gio::AppInfo::launch_default_for_uri(
                        &format!("file://{}", parent.to_string_lossy()),
                        gio::AppLaunchContext::NONE,
                    );
                }
            }
        });
    }
    download_bar.append(&dl_show_folder_btn);

    let dl_dismiss_btn = gtk4::Button::from_icon_name("window-close-symbolic");
    dl_dismiss_btn.add_css_class("flat");
    {
        let bar = download_bar.clone();
        dl_dismiss_btn.connect_clicked(move |_| {
            bar.set_visible(false);
        });
    }
    download_bar.append(&dl_dismiss_btn);

    DOWNLOAD_BARS.with(|bars| {
        bars.borrow_mut().insert(
            panel_id,
            DownloadBarWidgets {
                container: download_bar.clone(),
                label: dl_label,
                progress: dl_progress,
                open_btn: dl_open_btn,
            },
        );
    });

    container.append(&web_view);
    container.append(&download_bar);
    container.append(&console_pane);

    // ── Download policy: convert non-displayable responses to downloads ──
    {
        web_view.connect_decide_policy(|_wv, decision, decision_type| {
            if decision_type == webkit6::PolicyDecisionType::Response {
                if let Some(response_decision) =
                    decision.downcast_ref::<webkit6::ResponsePolicyDecision>()
                {
                    if !response_decision.is_mime_type_supported() {
                        decision.download();
                        return true;
                    }
                }
            }
            false
        });
    }

    // ── Context menu: augment default WebKit menu ──
    {
        let wv = web_view.clone();
        web_view.connect_context_menu(move |_wv, menu, hit_test| {
            // Remove "Open * in New Window" items — we're an embedded browser,
            // not a standalone window-based browser.
            let items_to_remove: Vec<_> = menu
                .items()
                .into_iter()
                .filter(|item| {
                    matches!(
                        item.stock_action(),
                        webkit6::ContextMenuAction::OpenLinkInNewWindow
                            | webkit6::ContextMenuAction::OpenImageInNewWindow
                            | webkit6::ContextMenuAction::OpenFrameInNewWindow
                            | webkit6::ContextMenuAction::OpenVideoInNewWindow
                            | webkit6::ContextMenuAction::OpenAudioInNewWindow
                    )
                })
                .collect();
            for item in &items_to_remove {
                menu.remove(item);
            }

            // Add "Copy Page URL" at the end if not on a link
            if !hit_test.context_is_link() {
                let page_url = wv.uri().map(|u| u.to_string()).unwrap_or_default();
                if !page_url.is_empty() && page_url != "about:blank" {
                    let action_group = gio::SimpleActionGroup::new();
                    let copy_url_action = gio::SimpleAction::new("copy-page-url", None);
                    let url = page_url.clone();
                    copy_url_action.connect_activate(move |_, _| {
                        if let Some(display) = gdk4::Display::default() {
                            display.clipboard().set_text(&url);
                        }
                    });
                    action_group.add_action(&copy_url_action);
                    wv.insert_action_group("browser", Some(&action_group));

                    menu.append(&webkit6::ContextMenuItem::from_gaction(
                        &copy_url_action,
                        "Copy Page URL",
                        None,
                    ));
                }
            }

            false // show the (modified) context menu
        });
    }

    // ── Favicon tracking ──
    {
        web_view.connect_favicon_notify(move |wv| {
            if let Some(texture) = wv.favicon() {
                FAVICON_CACHE.with(|c| c.borrow_mut().insert(panel_id, texture));
            } else {
                FAVICON_CACHE.with(|c| c.borrow_mut().remove(&panel_id));
            }
        });
    }

    // ── Wire navigation buttons ──
    {
        let wv = web_view.clone();
        back_btn.connect_clicked(move |_| {
            wv.go_back();
        });
    }
    {
        let wv = web_view.clone();
        fwd_btn.connect_clicked(move |_| {
            wv.go_forward();
        });
    }
    {
        let wv = web_view.clone();
        reload_btn.connect_clicked(move |btn| {
            if wv.is_loading() {
                wv.stop_loading();
                btn.set_icon_name("view-refresh-symbolic");
                btn.set_tooltip_text(Some("Reload"));
            } else {
                wv.reload();
            }
        });
    }

    // ── URL entry navigation ──
    {
        let wv = web_view.clone();
        let engine = browser_settings.search_engine;
        url_entry.connect_activate(move |entry| {
            let url = normalize_url(&entry.text(), engine);
            wv.load_uri(&url);
        });
    }

    // ── Load-changed signal: update URL bar + button sensitivity ──
    {
        let entry = url_entry.clone();
        let back = back_btn.clone();
        let fwd = fwd_btn.clone();
        let reload = reload_btn.clone();
        web_view.connect_load_changed(move |wv, event| {
            back.set_sensitive(wv.can_go_back());
            fwd.set_sensitive(wv.can_go_forward());

            match event {
                webkit6::LoadEvent::Started => {
                    clear_refs_for_panel(panel_id);
                    reload.set_icon_name("process-stop-symbolic");
                    reload.set_tooltip_text(Some("Stop"));
                }
                webkit6::LoadEvent::Finished => {
                    reload.set_icon_name("view-refresh-symbolic");
                    reload.set_tooltip_text(Some("Reload"));
                }
                _ => {}
            }

            if let Some(uri) = wv.uri() {
                entry.set_text(&uri);
            }
        });
    }

    // ── Progress bar: track estimated load progress ──
    {
        let pbar = progress_bar.clone();
        web_view.connect_estimated_load_progress_notify(move |wv| {
            let progress = wv.estimated_load_progress();
            if progress < 1.0 {
                pbar.set_visible(true);
                pbar.set_fraction(progress);
            } else {
                pbar.set_visible(false);
                pbar.set_fraction(0.0);
            }
        });
    }

    // ── URI notify: keep URL bar in sync ──
    {
        let entry = url_entry;
        web_view.connect_uri_notify(move |wv| {
            if let Some(uri) = wv.uri() {
                entry.set_text(&uri);
            }
        });
    }

    // ── Find-in-page wiring ──
    let devtools_open = Rc::new(Cell::new(false));
    {
        let find_bar = find_bar.clone();
        let find_entry = find_entry.clone();
        find_toggle_btn.connect_toggled(move |btn| {
            let active = btn.is_active();
            find_bar.set_visible(active);
            if active {
                find_entry.grab_focus();
            }
        });
    }
    {
        let wv = web_view.clone();
        let match_label = match_label.clone();
        find_entry.connect_search_changed(move |entry| {
            let text = entry.text().to_string();
            if let Some(fc) = wv.find_controller() {
                if text.is_empty() {
                    fc.search_finish();
                    match_label.set_text("");
                } else {
                    let opts = webkit6::FindOptions::CASE_INSENSITIVE
                        | webkit6::FindOptions::WRAP_AROUND;
                    fc.search(&text, opts.bits(), 0);
                }
            }
        });
    }
    {
        let wv = web_view.clone();
        find_next_btn.connect_clicked(move |_| {
            if let Some(fc) = wv.find_controller() {
                fc.search_next();
            }
        });
    }
    {
        let wv = web_view.clone();
        find_prev_btn.connect_clicked(move |_| {
            if let Some(fc) = wv.find_controller() {
                fc.search_previous();
            }
        });
    }
    // Enter in find entry = next match
    {
        let wv = web_view.clone();
        find_entry.connect_activate(move |_| {
            if let Some(fc) = wv.find_controller() {
                fc.search_next();
            }
        });
    }
    // Close find bar
    {
        let find_toggle = find_toggle_btn.clone();
        let wv = web_view.clone();
        find_close_btn.connect_clicked(move |_| {
            find_toggle.set_active(false);
            if let Some(fc) = wv.find_controller() {
                fc.search_finish();
            }
        });
    }
    // Match count signal
    {
        let match_label = match_label;
        if let Some(fc) = web_view.find_controller() {
            fc.connect_counted_matches(move |_fc, count| {
                if count == 0 {
                    match_label.set_text("No matches");
                } else {
                    match_label.set_text(&format!("{count} matches"));
                }
            });
        }
    }

    // ── Zoom controls ──
    fn update_zoom_label(wv: &webkit6::WebView, label: &gtk4::Label) {
        let pct = (wv.zoom_level() * 100.0).round() as i32;
        label.set_text(&format!("{pct}%"));
    }
    {
        let wv = web_view.clone();
        let label = zoom_label.clone();
        zoom_in_btn.connect_clicked(move |_| {
            let new_zoom = (wv.zoom_level() + 0.1).min(5.0);
            wv.set_zoom_level(new_zoom);
            update_zoom_label(&wv, &label);
        });
    }
    {
        let wv = web_view.clone();
        let label = zoom_label.clone();
        zoom_out_btn.connect_clicked(move |_| {
            let new_zoom = (wv.zoom_level() - 0.1).max(0.25);
            wv.set_zoom_level(new_zoom);
            update_zoom_label(&wv, &label);
        });
    }
    {
        let wv = web_view.clone();
        let label = zoom_label.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(1);
        zoom_label.add_controller(gesture.clone());
        gesture.connect_released(move |_, _, _, _| {
            wv.set_zoom_level(1.0);
            update_zoom_label(&wv, &label);
        });
    }

    // Keyboard shortcuts: Ctrl+=/Ctrl+-/Ctrl+0 for zoom
    {
        let wv = web_view.clone();
        let label = zoom_label.clone();
        let zoom_controller = gtk4::EventControllerKey::new();
        zoom_controller.connect_key_pressed(move |_, keyval, _, modifier| {
            let ctrl = modifier.contains(gdk4::ModifierType::CONTROL_MASK);
            if !ctrl {
                return glib::Propagation::Proceed;
            }
            match keyval {
                gdk4::Key::equal | gdk4::Key::plus => {
                    let new_zoom = (wv.zoom_level() + 0.1).min(5.0);
                    wv.set_zoom_level(new_zoom);
                    update_zoom_label(&wv, &label);
                    glib::Propagation::Stop
                }
                gdk4::Key::minus => {
                    let new_zoom = (wv.zoom_level() - 0.1).max(0.25);
                    wv.set_zoom_level(new_zoom);
                    update_zoom_label(&wv, &label);
                    glib::Propagation::Stop
                }
                gdk4::Key::_0 => {
                    wv.set_zoom_level(1.0);
                    update_zoom_label(&wv, &label);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        container.add_controller(zoom_controller);
    }

    // ── Dev tools toggle ──
    {
        let wv = web_view.clone();
        let open = devtools_open.clone();
        devtools_btn.connect_toggled(move |btn| {
            if let Some(inspector) = wv.inspector() {
                if btn.is_active() {
                    inspector.show();
                    open.set(true);
                } else {
                    inspector.close();
                    open.set(false);
                }
            }
        });
    }

    // ── Load initial URL ──
    let url = initial_url.map(|u| normalize_url(u, browser_settings.search_engine));
    if let Some(ref url) = url {
        if url != "about:blank" {
            web_view.load_uri(url);
        }
    }

    container.set_widget_name(&panel_id.to_string());
    container.upcast()
}

/// Apply a dark-mode user stylesheet if the system prefers dark.
pub(crate) fn apply_dark_mode(web_view: &webkit6::WebView) {
    let style_manager = libadwaita::StyleManager::default();
    let is_dark = style_manager.is_dark();

    if is_dark {
        inject_dark_stylesheet(web_view);
    }

    // React to theme changes at runtime
    let wv = web_view.clone();
    style_manager.connect_dark_notify(move |sm: &libadwaita::StyleManager| {
        let ucm = wv.user_content_manager().unwrap();
        ucm.remove_all_style_sheets();
        if sm.is_dark() {
            inject_dark_stylesheet(&wv);
        }
    });
}

fn inject_dark_stylesheet(web_view: &webkit6::WebView) {
    let dark_css = r#"
        @media (prefers-color-scheme: light) {
            :root {
                color-scheme: dark;
            }
            html {
                filter: invert(0.88) hue-rotate(180deg);
            }
            img, video, canvas, svg, [style*="background-image"] {
                filter: invert(1) hue-rotate(180deg);
            }
        }
    "#;

    let stylesheet = webkit6::UserStyleSheet::new(
        dark_css,
        webkit6::UserContentInjectedFrames::AllFrames,
        webkit6::UserStyleLevel::User,
        &[],
        &[],
    );

    if let Some(ucm) = web_view.user_content_manager() {
        ucm.add_style_sheet(&stylesheet);
    }
}

fn normalize_url(input: &str, engine: settings::SearchEngine) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "about:blank".to_string();
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
    {
        return trimmed.to_string();
    }
    if trimmed.contains('.') && !trimmed.contains(' ') {
        return format!("https://{trimmed}");
    }
    engine.search_url(trimmed)
}
