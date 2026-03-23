//! Thread-local registries and helpers for the browser panel subsystem.
//!
//! All thread-local state lives here so the rest of `browser_panel` can
//! access it through thin public helpers instead of touching the maps
//! directly.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;

use webkit6::prelude::*;

// ---------------------------------------------------------------------------
// Thread-local registries
// ---------------------------------------------------------------------------

thread_local! {
    /// Registry of panel_id -> WebView for browser automation socket commands.
    pub(super) static WEBVIEW_REGISTRY: RefCell<HashMap<uuid::Uuid, webkit6::WebView>> = RefCell::new(HashMap::new());

    /// Element reference registry: "@e1" -> ElementRef
    pub(super) static ELEMENT_REFS: RefCell<HashMap<String, ElementRef>> = RefCell::new(HashMap::new());

    /// Next element ref ID counter.
    pub(super) static NEXT_REF_ID: Cell<u64> = const { Cell::new(1) };

    /// Per-panel console message ring buffer (last 100 messages).
    pub(super) static CONSOLE_BUFFERS: RefCell<HashMap<uuid::Uuid, Vec<String>>> = RefCell::new(HashMap::new());

    /// Per-panel dialog handler config.
    pub(super) static DIALOG_HANDLERS: RefCell<HashMap<uuid::Uuid, DialogHandler>> = RefCell::new(HashMap::new());

    /// Per-panel favicon textures (updated on WebView favicon-notify signal).
    pub(super) static FAVICON_CACHE: RefCell<HashMap<uuid::Uuid, gdk4::Texture>> = RefCell::new(HashMap::new());

    /// Per-panel console pane widgets (for toggle via UiEvent).
    pub(super) static CONSOLE_PANELS: RefCell<HashMap<uuid::Uuid, gtk4::Box>> = RefCell::new(HashMap::new());

    /// Per-panel console TextViews (for appending messages).
    pub(super) static CONSOLE_TEXT_VIEWS: RefCell<HashMap<uuid::Uuid, gtk4::TextView>> = RefCell::new(HashMap::new());

    /// Per-panel download bar widgets.
    pub(super) static DOWNLOAD_BARS: RefCell<HashMap<uuid::Uuid, DownloadBarWidgets>> = RefCell::new(HashMap::new());

    /// Per-panel last downloaded file path (for "Open" button).
    pub(super) static DOWNLOAD_PATHS: RefCell<HashMap<uuid::Uuid, String>> = RefCell::new(HashMap::new());
}

// ---------------------------------------------------------------------------
// Helper structs
// ---------------------------------------------------------------------------

pub(super) struct DownloadBarWidgets {
    pub(super) container: gtk4::Box,
    pub(super) label: gtk4::Label,
    pub(super) progress: gtk4::ProgressBar,
    pub(super) open_btn: gtk4::Button,
}

pub(super) struct ElementRef {
    #[allow(dead_code)]
    pub(super) panel_id: uuid::Uuid,
    pub(super) selector: String,
}

#[allow(dead_code)] // fields populated by dialog signal handlers
pub(super) struct DialogHandler {
    pub(super) action: String, // "accept" or "dismiss"
    pub(super) prompt_text: Option<String>,
}

// ---------------------------------------------------------------------------
// Shared network session
// ---------------------------------------------------------------------------

/// Shared persistent NetworkSession — cookies and storage persist across panels and restarts.
/// Data stored at `~/.local/share/cmux/webkit/`.
#[allow(dead_code)] // available for WebView creation
pub(super) fn shared_network_session() -> webkit6::NetworkSession {
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
        super::wire_download_handling(&session);
        *slot = Some(session.clone());
        session
    })
}

// ---------------------------------------------------------------------------
// WebView registry accessors
// ---------------------------------------------------------------------------

/// Look up the WebView for a panel_id (GTK main thread only).
pub(crate) fn get_webview(panel_id: uuid::Uuid) -> Option<webkit6::WebView> {
    WEBVIEW_REGISTRY.with(|r| r.borrow().get(&panel_id).cloned())
}

/// Remove a panel from the WebView registry.
#[allow(dead_code)]
pub(crate) fn unregister_webview(panel_id: uuid::Uuid) {
    WEBVIEW_REGISTRY.with(|r| r.borrow_mut().remove(&panel_id));
}

/// Stop loading on all registered WebViews — call before shutdown to
/// prevent WebProcess segfaults when active content is torn down.
pub(crate) fn stop_all_webviews() {
    WEBVIEW_REGISTRY.with(|r| {
        for wv in r.borrow().values() {
            wv.stop_loading();
        }
    });
}

/// Collect current zoom levels for all browser panels (for session snapshots).
pub(crate) fn collect_webview_zoom_levels() -> HashMap<uuid::Uuid, f64> {
    WEBVIEW_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .map(|(&id, wv)| (id, wv.zoom_level()))
            .collect()
    })
}

/// Toggle the JS console panel for a browser panel.
pub(crate) fn toggle_console(panel_id: uuid::Uuid) {
    CONSOLE_PANELS.with(|c| {
        if let Some(pane) = c.borrow().get(&panel_id) {
            pane.set_visible(!pane.is_visible());
        }
    });
}

/// Get the cached favicon texture for a browser panel (if available).
pub(crate) fn get_favicon(panel_id: uuid::Uuid) -> Option<gdk4::Texture> {
    FAVICON_CACHE.with(|c| c.borrow().get(&panel_id).cloned())
}

/// Collect back/forward history URLs for all browser panels (for session snapshots).
pub(crate) fn collect_webview_histories() -> HashMap<uuid::Uuid, (Vec<String>, Vec<String>)> {
    WEBVIEW_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .filter_map(|(&id, wv)| {
                let bfl = wv.back_forward_list()?;
                let back: Vec<String> = bfl
                    .back_list()
                    .iter()
                    .filter_map(|item| item.uri().map(|u| u.to_string()))
                    .collect();
                let forward: Vec<String> = bfl
                    .forward_list()
                    .iter()
                    .filter_map(|item| item.uri().map(|u| u.to_string()))
                    .collect();
                Some((id, (back, forward)))
            })
            .collect()
    })
}

/// Collect current URLs for all browser panels (for session snapshots).
pub(crate) fn collect_webview_urls() -> HashMap<uuid::Uuid, String> {
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
pub(crate) fn allocate_ref(panel_id: uuid::Uuid, selector: &str) -> String {
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
pub(crate) fn release_ref(ref_id: &str) -> bool {
    ELEMENT_REFS.with(|refs| refs.borrow_mut().remove(ref_id).is_some())
}

/// Resolve a selector: if it starts with "@e", look up the stored CSS selector.
/// Otherwise return it as-is.
pub(crate) fn resolve_selector(selector: &str) -> Option<String> {
    if selector.starts_with("@e") {
        ELEMENT_REFS.with(|refs| refs.borrow().get(selector).map(|r| r.selector.clone()))
    } else {
        Some(selector.to_string())
    }
}

/// Clear all element refs for a given panel (called on navigation).
pub(crate) fn clear_refs_for_panel(panel_id: uuid::Uuid) {
    ELEMENT_REFS.with(|refs| {
        refs.borrow_mut().retain(|_, v| v.panel_id != panel_id);
    });
}

// ---------------------------------------------------------------------------
// Download helpers
// ---------------------------------------------------------------------------

/// Reverse-lookup the panel_id for a WebView in the registry.
pub(super) fn panel_id_for_webview(wv: &webkit6::WebView) -> Option<uuid::Uuid> {
    WEBVIEW_REGISTRY.with(|r| r.borrow().iter().find(|(_, v)| *v == wv).map(|(&id, _)| id))
}

/// Pick a unique download path in `dir`, appending " (1)", " (2)", etc. if needed.
pub(super) fn unique_download_path(dir: &Path, filename: &str) -> std::path::PathBuf {
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
pub(super) fn update_download_bar(
    panel_id: uuid::Uuid,
    text: &str,
    fraction: f64,
    show_open: bool,
) {
    DOWNLOAD_BARS.with(|bars| {
        if let Some(bar) = bars.borrow().get(&panel_id) {
            bar.container.set_visible(true);
            bar.label.set_text(text);
            bar.progress.set_fraction(fraction);
            bar.open_btn.set_visible(show_open);
        }
    });
}
