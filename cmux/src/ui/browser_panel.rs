//! Browser panel — embedded WebKit browser (webkit6 / WebKitGTK 6.0).

use std::cell::Cell;
use std::rc::Rc;

use gdk4;
use gtk4::prelude::*;
use webkit6::prelude::*;

use crate::settings;

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

    // ── WebView ──
    let web_view = webkit6::WebView::new();
    web_view.set_hexpand(true);
    web_view.set_vexpand(true);

    // Enable developer extras for inspector
    if let Some(ws) = webkit6::prelude::WebViewExt::settings(&web_view) {
        ws.set_enable_developer_extras(true);
    }

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
