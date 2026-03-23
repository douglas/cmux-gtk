//! Browser panel — embedded WebKit browser (webkit6 / WebKitGTK 6.0).

mod actions;
mod registry;
mod theme;
mod util;

pub(crate) use actions::execute_action;
pub use actions::BrowserActionKind;
pub(crate) use registry::*;

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gdk4;
use glib::object::Cast;
use gtk4::prelude::*;
use webkit6::prelude::*;

use crate::browser_history;
use crate::browser_profiles;
use crate::settings;

// ---------------------------------------------------------------------------
// Download session wiring
// ---------------------------------------------------------------------------

/// Wire download-started handling for a NetworkSession.
/// Public so browser_profiles can reuse it for per-profile sessions.
pub fn wire_download_handling_for_session(session: &webkit6::NetworkSession) {
    wire_download_handling(session);
}

fn wire_download_handling(session: &webkit6::NetworkSession) {
    session.connect_download_started(|_session, download| {
        let panel_id = download
            .web_view()
            .and_then(|wv| registry::panel_id_for_webview(&wv));

        if let Some(pid) = panel_id {
            registry::update_download_bar(pid, "Starting download\u{2026}", 0.0, false);
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

            let path = registry::unique_download_path(&downloads_dir, suggested_filename);
            let dest = format!("file://{}", path.to_string_lossy());
            dl.set_allow_overwrite(false);
            dl.set_destination(&dest);

            if let Some(pid) = pid_dest {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                registry::update_download_bar(pid, &format!("Downloading: {filename}"), 0.0, false);
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
                registry::update_download_bar(
                    pid,
                    &format!("Downloading: {filename} \u{2014} {pct}%"),
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
                    s.strip_prefix("file://").unwrap_or(&s).to_string()
                });
                let filename = dest_path
                    .as_deref()
                    .and_then(|p| Path::new(p).file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".to_string());

                registry::update_download_bar(pid, &format!("Downloaded: {filename}"), 1.0, true);

                // Store the path for the Open/Show buttons
                if let Some(path) = dest_path {
                    registry::DOWNLOAD_PATHS.with(|paths| {
                        paths.borrow_mut().insert(pid, path);
                    });
                }

                // Auto-hide after 8 seconds
                glib::timeout_add_local_once(std::time::Duration::from_secs(8), move || {
                    registry::DOWNLOAD_BARS.with(|bars| {
                        if let Some(bar) = bars.borrow().get(&pid) {
                            bar.container.set_visible(false);
                        }
                    });
                });
            });
        }

        // Failed
        if let Some(pid) = panel_id {
            download.connect_failed(move |_dl, error| {
                let msg = error.message();
                registry::update_download_bar(pid, &format!("Download failed: {msg}"), 0.0, false);
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Browser widget creation
// ---------------------------------------------------------------------------

/// Create an embedded browser panel widget.
///
/// Layout:
/// ```text
/// VBox:
///   +-- nav_bar (HBox): [back] [fwd] [reload/stop] [home] [url_entry] [find] [devtools]
///   +-- progress_bar (ProgressBar): thin load indicator
///   +-- find_bar (HBox): [find_entry] [prev] [next] [match_count] [close]  (hidden by default)
///   +-- web_view (WebView): fills remaining space
/// ```
pub fn create_browser_widget(
    panel_id: uuid::Uuid,
    initial_url: Option<&str>,
    is_attention_source: bool,
    initial_zoom: Option<f64>,
    shared: Option<std::sync::Arc<crate::app::SharedState>>,
) -> gtk4::Widget {
    let profile_name = browser_profiles::default_profile_name();
    create_browser_widget_with_profile(
        panel_id,
        initial_url,
        is_attention_source,
        initial_zoom,
        &profile_name,
        shared,
    )
}

/// Create a browser widget using a specific profile for session isolation.
pub fn create_browser_widget_with_profile(
    panel_id: uuid::Uuid,
    initial_url: Option<&str>,
    is_attention_source: bool,
    initial_zoom: Option<f64>,
    profile_name: &str,
    shared: Option<std::sync::Arc<crate::app::SharedState>>,
) -> gtk4::Widget {
    let browser_settings = settings::load().browser;

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.add_css_class("panel-shell");
    if is_attention_source {
        container.add_css_class("attention-panel");
    }

    // -- Navigation bar --
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

    // -- Profile selector --
    let profiles = browser_profiles::list();
    let _profile_dropdown = if profiles.len() > 1 {
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        let model = gtk4::StringList::new(&names);
        let dropdown = gtk4::DropDown::new(Some(model), gtk4::Expression::NONE);
        dropdown.set_tooltip_text(Some("Browser Profile"));
        dropdown.add_css_class("flat");
        // Select current profile
        if let Some(idx) = profiles.iter().position(|p| p.name == profile_name) {
            dropdown.set_selected(idx as u32);
        }
        nav_bar.append(&dropdown);
        Some(dropdown)
    } else {
        None
    };

    let (omnibar_box, url_entry) =
        super::omnibar::build_omnibar(initial_url, browser_settings.search_engine);
    nav_bar.append(&omnibar_box);

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

    // -- Browser theme mode toggle --
    let theme_btn = gtk4::Button::new();
    let initial_theme = browser_settings.browser_theme;
    let theme_icon = match initial_theme {
        settings::BrowserThemeMode::System => "weather-clear-symbolic",
        settings::BrowserThemeMode::Light => "display-brightness-symbolic",
        settings::BrowserThemeMode::Dark => "weather-clear-night-symbolic",
    };
    theme_btn.set_icon_name(theme_icon);
    theme_btn.set_tooltip_text(Some(&format!("Browser Theme: {}", initial_theme.label())));
    theme_btn.add_css_class("flat");
    nav_bar.append(&theme_btn);

    let devtools_btn = gtk4::ToggleButton::new();
    devtools_btn.set_icon_name("utilities-terminal-symbolic");
    devtools_btn.set_tooltip_text(Some("Developer Tools"));
    devtools_btn.add_css_class("flat");
    nav_bar.append(&devtools_btn);

    container.append(&nav_bar);

    // -- Progress bar --
    let progress_bar = gtk4::ProgressBar::new();
    progress_bar.add_css_class("osd");
    progress_bar.set_visible(false);
    container.append(&progress_bar);

    // -- Find bar (hidden by default) --
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

    // -- WebView (profile-based session for cookie/storage isolation) --
    let web_view = webkit6::WebView::builder()
        .network_session(&browser_profiles::network_session_for(profile_name))
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
    registry::WEBVIEW_REGISTRY.with(|r| r.borrow_mut().insert(panel_id, web_view.clone()));

    // Enable developer extras for inspector + set user agent
    if let Some(ws) = webkit6::prelude::WebViewExt::settings(&web_view) {
        ws.set_enable_developer_extras(true);
        // Force a Safari-compatible UA to avoid bot detection / degraded content
        ws.set_user_agent(Some(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 \
             (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
        ));
    }

    // Apply dark mode stylesheet if system is dark
    theme::apply_dark_mode(&web_view);

    // -- Console capture: inject script to intercept console.* and post to Rust --
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
            registry::CONSOLE_BUFFERS.with(|bufs| {
                let mut map = bufs.borrow_mut();
                let buf = map.entry(panel_id).or_insert_with(Vec::new);
                buf.push(message.clone());
                if buf.len() > 100 {
                    buf.remove(0);
                }
            });
            // Append to the in-app console text view
            registry::CONSOLE_TEXT_VIEWS.with(|tvs| {
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

    // -- JS Console panel (collapsible, below WebView) --
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
            registry::CONSOLE_BUFFERS.with(|bufs| {
                bufs.borrow_mut().remove(&panel_id);
            });
        });
    }

    // Store console pane and text view references for toggle and message appending
    registry::CONSOLE_PANELS.with(|c| c.borrow_mut().insert(panel_id, console_pane.clone()));
    registry::CONSOLE_TEXT_VIEWS
        .with(|c| c.borrow_mut().insert(panel_id, console_text_view.clone()));

    // -- Download bar (hidden by default, shown when a download starts) --
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
            let path =
                registry::DOWNLOAD_PATHS.with(|paths| paths.borrow().get(&panel_id).cloned());
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
            let path =
                registry::DOWNLOAD_PATHS.with(|paths| paths.borrow().get(&panel_id).cloned());
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

    registry::DOWNLOAD_BARS.with(|bars| {
        bars.borrow_mut().insert(
            panel_id,
            registry::DownloadBarWidgets {
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

    // -- Navigation + download policy --
    {
        let wv_policy = web_view.clone();
        let shared_for_policy = shared.clone();
        let settings_for_policy = browser_settings.clone();
        web_view.connect_decide_policy(move |_wv, decision, decision_type| {
            tracing::trace!(?decision_type, "decide_policy fired");

            // Response policy: convert non-displayable responses to downloads
            if decision_type == webkit6::PolicyDecisionType::Response {
                if let Some(response_decision) =
                    decision.downcast_ref::<webkit6::ResponsePolicyDecision>()
                {
                    if !response_decision.is_mime_type_supported() {
                        decision.download();
                        return true;
                    }
                }
                return false;
            }

            // New-window policy: intercept requests that would open a new
            // browser window (e.g. target="_blank" redirects) and load them
            // in the current WebView instead of the system browser.
            if decision_type == webkit6::PolicyDecisionType::NewWindowAction {
                if let Some(nav_decision) =
                    decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
                {
                    if let Some(nav_action) = nav_decision.navigation_action() {
                        if let Some(request) = nav_action.request() {
                            if let Some(uri) = request.uri() {
                                let url = uri.to_string();
                                tracing::debug!(%url, "decide_policy: NewWindowAction \u{2192} loading in current view");
                                decision.ignore();
                                wv_policy.load_uri(&url);
                                return true;
                            }
                        }
                    }
                }
                decision.ignore();
                return true;
            }

            // Navigation action policy
            if decision_type == webkit6::PolicyDecisionType::NavigationAction {
                if let Some(nav_decision) =
                    decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
                {
                    if let Some(nav_action) = nav_decision.navigation_action() {
                        let nav_type = nav_action.navigation_type();

                        // Only apply custom policy to link clicks and "other"
                        // navigations.  Reloads, back/forward, and form
                        // submissions should always proceed normally.
                        let needs_policy = matches!(
                            nav_type,
                            webkit6::NavigationType::LinkClicked
                                | webkit6::NavigationType::Other
                        );

                        if let Some(request) = nav_action.request() {
                            if let Some(uri) = request.uri() {
                                tracing::debug!(
                                    url = %uri,
                                    nav_type = ?nav_type,
                                    needs_policy,
                                    mouse_button = nav_action.mouse_button(),
                                    "decide_policy: NavigationAction"
                                );
                            }
                        }

                        if needs_policy {
                            if let Some(request) = nav_action.request() {
                                if let Some(uri) = request.uri() {
                                    let url = uri.to_string();

                                    // Deep link / custom scheme -> xdg-open
                                    // Extract scheme: split on ":" (not "://")
                                    // to handle both "https://..." and "about:blank".
                                    if let Some(scheme) = url.split(':').next().map(|s| s.to_lowercase()) {
                                        if !matches!(
                                            scheme.as_str(),
                                            "http" | "https" | "file"
                                                | "about" | "data" | "blob"
                                                | "javascript"
                                        ) {
                                            tracing::warn!(%url, %scheme, "decide_policy: deep link \u{2192} xdg-open");
                                            decision.ignore();
                                            let _ = std::process::Command::new("xdg-open")
                                                .arg(&url)
                                                .spawn();
                                            return true;
                                        }
                                    }

                                    // Insecure HTTP interstitial
                                    if url.starts_with("http://") {
                                        let host = util::extract_host(&url);
                                        if !host.is_empty() {
                                            let is_allowed = matches!(
                                                host.as_str(),
                                                "localhost"
                                                    | "127.0.0.1"
                                                    | "::1"
                                                    | "0.0.0.0"
                                            ) || settings_for_policy
                                                .http_allowlist
                                                .iter()
                                                .any(|pat| {
                                                    if let Some(suffix) =
                                                        pat.strip_prefix("*.")
                                                    {
                                                        // *.example.com must match
                                                        // sub.example.com and
                                                        // example.com, but NOT
                                                        // notexample.com.
                                                        host == suffix
                                                            || host.ends_with(
                                                                &format!(".{suffix}"),
                                                            )
                                                    } else {
                                                        host == *pat
                                                    }
                                                });
                                            if !is_allowed {
                                                decision.ignore();
                                                let html =
                                                    util::insecure_http_interstitial(
                                                        &url,
                                                    );
                                                wv_policy
                                                    .load_html(&html, Some(&url));
                                                return true;
                                            }
                                        }
                                    }

                                    // Ctrl+click or middle-click -> new tab
                                    let mouse_button = nav_action.mouse_button();
                                    let modifiers = nav_action.modifiers();
                                    let ctrl_mask =
                                        gdk4::ModifierType::CONTROL_MASK.bits();
                                    let is_ctrl_click =
                                        (modifiers & ctrl_mask) != 0
                                            && mouse_button == 1;
                                    let is_middle_click = mouse_button == 2;

                                    if is_ctrl_click || is_middle_click {
                                        if let Some(ref shared) =
                                            shared_for_policy
                                        {
                                            decision.ignore();
                                            shared.send_ui_event(
                                                crate::app::UiEvent::BrowserOpenInNewTab {
                                                    source_panel_id: panel_id,
                                                    url,
                                                },
                                            );
                                            return true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Explicitly accept all navigations we didn't block above.
            // Returning false would let WebKit GTK fall through to the
            // system default handler (which opens Chrome/xdg-open).
            decision.use_();
            true
        });
    }

    // -- Context menu: augment default WebKit menu --
    {
        let wv = web_view.clone();
        let shared_for_ctx = shared.clone();
        web_view.connect_context_menu(move |_wv, menu, hit_test| {
            // Remove "Open * in New Window" items -- we're an embedded browser,
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

            // Link context: add "Open Link in New Tab" + "Open in Default Browser"
            if hit_test.context_is_link() {
                if let Some(link_uri) = hit_test.link_uri() {
                    let link_url = link_uri.to_string();
                    let action_group = gio::SimpleActionGroup::new();

                    // "Open Link in New Tab"
                    let new_tab_action = gio::SimpleAction::new("open-link-new-tab", None);
                    let url_for_tab = link_url.clone();
                    let shared_for_tab = shared_for_ctx.clone();
                    new_tab_action.connect_activate(move |_, _| {
                        if let Some(ref shared) = shared_for_tab {
                            shared.send_ui_event(crate::app::UiEvent::BrowserOpenInNewTab {
                                source_panel_id: panel_id,
                                url: url_for_tab.clone(),
                            });
                        }
                    });
                    action_group.add_action(&new_tab_action);
                    menu.prepend(&webkit6::ContextMenuItem::from_gaction(
                        &new_tab_action,
                        "Open Link in New Tab",
                        None,
                    ));

                    // "Open in Default Browser"
                    let ext_action = gio::SimpleAction::new("open-link-default-browser", None);
                    let url_for_ext = link_url;
                    ext_action.connect_activate(move |_, _| {
                        let _ = gio::AppInfo::launch_default_for_uri(
                            &url_for_ext,
                            gio::AppLaunchContext::NONE,
                        );
                    });
                    action_group.add_action(&ext_action);
                    menu.append(&webkit6::ContextMenuItem::from_gaction(
                        &ext_action,
                        "Open in Default Browser",
                        None,
                    ));

                    wv.insert_action_group("browser", Some(&action_group));
                }
            } else {
                // Non-link context: add "Copy Page URL"
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

    // -- Popup handling: window.open() / target="_blank" -> open in new tab --
    //
    // WebKit GTK opens the system browser when `create` returns None.
    // To prevent this, we create a temporary off-screen WebView that
    // absorbs the navigation, extract the URL, send it to our new-tab
    // handler, and then destroy the temporary WebView.
    {
        let shared_for_create = shared.clone();
        let network_session = web_view
            .network_session()
            .or_else(webkit6::NetworkSession::default);
        web_view.connect_create(move |_wv, nav_action| {
            // Extract URL before creating the related view -- we'll handle
            // it ourselves regardless.
            let url = nav_action
                .request()
                .and_then(|r| r.uri())
                .map(|u| u.to_string());

            if let Some(url) = url.as_deref() {
                tracing::debug!(%url, "connect_create: intercepted popup \u{2192} new tab");
                if let Some(ref shared) = shared_for_create {
                    shared.send_ui_event(crate::app::UiEvent::BrowserOpenInNewTab {
                        source_panel_id: panel_id,
                        url: url.to_string(),
                    });
                }
            }

            // Return a temporary hidden WebView so WebKit doesn't fall
            // through to the system browser.  It is never displayed;
            // once WebKit finishes with it, GLib will drop the ref.
            let mut builder = webkit6::WebView::builder();
            if let Some(ref ns) = network_session {
                builder = builder.network_session(ns);
            }
            let tmp = builder.build();
            // Block ALL navigations in the temp view so it can't open
            // the system browser via default decide_policy behavior.
            tmp.connect_decide_policy(|_wv, decision, decision_type| {
                tracing::debug!(
                    ?decision_type,
                    "TEMP WebView decide_policy \u{2192} ignoring"
                );
                decision.ignore();
                true
            });
            tmp.connect_create(|_wv, _nav| {
                tracing::debug!("TEMP WebView connect_create \u{2192} blocked");
                None::<gtk4::Widget>
            });
            tmp.load_uri("about:blank");
            Some(tmp.upcast::<gtk4::Widget>())
        });
    }

    // -- Permission requests (camera, microphone, geolocation) --
    {
        use webkit6::prelude::PermissionRequestExt;
        web_view.connect_permission_request(|_wv, request| {
            // Deny all permission requests by default. Granting camera, microphone,
            // or geolocation to arbitrary web pages is a security/privacy risk.
            request.deny();
            true
        });
    }

    // -- Favicon tracking --
    {
        web_view.connect_favicon_notify(move |wv| {
            if let Some(texture) = wv.favicon() {
                registry::FAVICON_CACHE.with(|c| c.borrow_mut().insert(panel_id, texture));
            } else {
                registry::FAVICON_CACHE.with(|c| c.borrow_mut().remove(&panel_id));
            }
        });
    }

    // -- Wire navigation buttons --
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

    // -- URL entry navigation --
    {
        let wv = web_view.clone();
        let engine = browser_settings.search_engine;
        url_entry.connect_activate(move |entry| {
            let url = util::normalize_url(&entry.text(), engine);
            tracing::debug!(%url, "Browser URL bar: loading URI");
            wv.load_uri(&url);
        });
    }

    // -- Load-changed signal: update URL bar + button sensitivity --
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
                    registry::clear_refs_for_panel(panel_id);
                    reload.set_icon_name("process-stop-symbolic");
                    reload.set_tooltip_text(Some("Stop"));
                }
                webkit6::LoadEvent::Finished => {
                    reload.set_icon_name("view-refresh-symbolic");
                    reload.set_tooltip_text(Some("Reload"));
                    // Record visit in browser history
                    let url = wv.uri().map(|u| u.to_string()).unwrap_or_default();
                    let title = wv.title().map(|t| t.to_string()).unwrap_or_default();
                    browser_history::record_visit(&url, &title);
                    // Inject browser theme mode override
                    let theme = settings::load().browser.browser_theme;
                    let js = theme.theme_injection_js();
                    wv.evaluate_javascript(js, None, None, gio::Cancellable::NONE, |_| {});
                }
                _ => {}
            }

            if let Some(uri) = wv.uri() {
                super::omnibar::set_url_quiet(&entry, &uri);
            }
        });
    }

    // -- Progress bar: track estimated load progress --
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

    // -- URI notify: keep URL bar in sync --
    {
        let entry = url_entry;
        web_view.connect_uri_notify(move |wv| {
            if let Some(uri) = wv.uri() {
                super::omnibar::set_url_quiet(&entry, &uri);
            }
        });
    }

    // -- Find-in-page wiring --
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
                    let opts =
                        webkit6::FindOptions::CASE_INSENSITIVE | webkit6::FindOptions::WRAP_AROUND;
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

    // -- Zoom controls --
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

    // -- Dev tools toggle --
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

    // -- Browser theme toggle handler --
    {
        let wv = web_view.clone();
        let btn = theme_btn.clone();
        theme_btn.connect_clicked(move |_| {
            // Cycle: System -> Light -> Dark -> System
            let current = settings::load().browser.browser_theme;
            let next = match current {
                settings::BrowserThemeMode::System => settings::BrowserThemeMode::Light,
                settings::BrowserThemeMode::Light => settings::BrowserThemeMode::Dark,
                settings::BrowserThemeMode::Dark => settings::BrowserThemeMode::System,
            };
            // Save
            let mut s = settings::load();
            s.browser.browser_theme = next;
            let _ = settings::save(&s);
            // Update button
            let icon = match next {
                settings::BrowserThemeMode::System => "weather-clear-symbolic",
                settings::BrowserThemeMode::Light => "display-brightness-symbolic",
                settings::BrowserThemeMode::Dark => "weather-clear-night-symbolic",
            };
            btn.set_icon_name(icon);
            btn.set_tooltip_text(Some(&format!("Browser Theme: {}", next.label())));
            // Apply to current page
            let js = next.theme_injection_js();
            wv.evaluate_javascript(js, None, None, gio::Cancellable::NONE, |_| {});
        });
    }

    // -- Load initial URL --
    let url = initial_url.map(|u| util::normalize_url(u, browser_settings.search_engine));
    if let Some(ref url) = url {
        if url != "about:blank" {
            web_view.load_uri(url);
        }
    }

    container.set_widget_name(&panel_id.to_string());
    container.upcast()
}
