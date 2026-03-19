//! Browser panel — embedded WebKit browser (webkit6 / WebKitGTK 6.0).

use gtk4::prelude::*;
use webkit6::prelude::*;

/// Create an embedded browser panel widget.
///
/// Layout:
/// ```text
/// VBox:
///   ├─ nav_bar (HBox): [back] [fwd] [reload/stop] [url_entry]
///   └─ web_view (WebView): fills remaining space
/// ```
pub fn create_browser_widget(
    panel_id: uuid::Uuid,
    initial_url: Option<&str>,
    is_attention_source: bool,
) -> gtk4::Widget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.add_css_class("panel-shell");
    if is_attention_source {
        container.add_css_class("attention-panel");
    }

    // ── Navigation bar ──
    let nav_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    nav_bar.set_margin_start(4);
    nav_bar.set_margin_end(4);
    nav_bar.set_margin_top(4);
    nav_bar.set_margin_bottom(2);

    let back_btn = gtk4::Button::from_icon_name("go-previous-symbolic");
    back_btn.set_tooltip_text(Some("Back"));
    back_btn.set_sensitive(false);
    nav_bar.append(&back_btn);

    let fwd_btn = gtk4::Button::from_icon_name("go-next-symbolic");
    fwd_btn.set_tooltip_text(Some("Forward"));
    fwd_btn.set_sensitive(false);
    nav_bar.append(&fwd_btn);

    let reload_btn = gtk4::Button::from_icon_name("view-refresh-symbolic");
    reload_btn.set_tooltip_text(Some("Reload"));
    nav_bar.append(&reload_btn);

    let url_entry = gtk4::Entry::new();
    url_entry.set_hexpand(true);
    url_entry.set_placeholder_text(Some("Enter URL..."));
    if let Some(url) = initial_url {
        url_entry.set_text(url);
    }
    nav_bar.append(&url_entry);

    container.append(&nav_bar);

    // ── WebView ──
    let web_view = webkit6::WebView::new();
    web_view.set_hexpand(true);
    web_view.set_vexpand(true);

    // Apply dark mode stylesheet if system is dark
    apply_dark_mode(&web_view);

    container.append(&web_view);

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
        url_entry.connect_activate(move |entry| {
            let url = normalize_url(&entry.text());
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

    // ── Title notify: update URL entry on title change (optional feedback) ──
    {
        let _entry = url_entry.clone();
        web_view.connect_title_notify(move |wv| {
            // Title is available for model updates — the panel title sync
            // is handled by the caller via periodic model refresh.
            let _title = wv.title();
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

    // ── Load initial URL ──
    let url = initial_url.map(normalize_url);
    if let Some(ref url) = url {
        if url != "about:blank" {
            web_view.load_uri(url);
        }
    }

    container.set_widget_name(&panel_id.to_string());
    container.upcast()
}

/// Apply a dark-mode user stylesheet if the system prefers dark.
fn apply_dark_mode(web_view: &webkit6::WebView) {
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

fn normalize_url(input: &str) -> String {
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
    format!("https://duckduckgo.com/?q={}", trimmed.replace(' ', "+"))
}
