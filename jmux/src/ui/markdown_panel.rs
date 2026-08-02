//! Markdown panel — renders `.md` files via pulldown-cmark → WebView.
//!
//! Uses a WebKit6 WebView to display rendered HTML from markdown content.
//! Watches the source file for changes and auto-reloads.

use gtk4::prelude::*;
use webkit6::prelude::*;

/// Create a markdown panel widget that renders the given file.
///
/// Layout:
/// ```text
/// VBox:
///   ├─ toolbar (HBox): [file_label] [spacer] [reload_btn] [open_btn]
///   └─ web_view (WebView): rendered markdown
/// ```
pub fn create_markdown_widget(
    panel_id: uuid::Uuid,
    file_path: Option<&str>,
    is_attention_source: bool,
) -> gtk4::Widget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.add_css_class("panel-shell");
    if is_attention_source {
        container.add_css_class("attention-panel");
    }
    container.set_widget_name(&panel_id.to_string());

    // ── Toolbar ──
    let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    toolbar.add_css_class("browser-nav-bar");
    toolbar.set_margin_start(6);
    toolbar.set_margin_end(6);
    toolbar.set_margin_top(2);
    toolbar.set_margin_bottom(2);

    let icon = gtk4::Image::from_icon_name("document-open-symbolic");
    icon.set_pixel_size(16);
    toolbar.append(&icon);

    let file_label = gtk4::Label::new(
        file_path
            .and_then(|p| std::path::Path::new(p).file_name().and_then(|n| n.to_str()))
            .or(Some("Markdown")),
    );
    file_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    file_label.set_max_width_chars(50);
    file_label.add_css_class("dim-label");
    toolbar.append(&file_label);

    let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    toolbar.append(&spacer);

    // Zoom controls
    let zoom_out_btn = gtk4::Button::from_icon_name("zoom-out-symbolic");
    zoom_out_btn.add_css_class("flat");
    zoom_out_btn.set_tooltip_text(Some("Zoom Out (Ctrl+-)"));
    toolbar.append(&zoom_out_btn);

    let zoom_reset_btn = gtk4::Button::from_icon_name("zoom-original-symbolic");
    zoom_reset_btn.add_css_class("flat");
    zoom_reset_btn.set_tooltip_text(Some("Reset Zoom (Ctrl+0)"));
    toolbar.append(&zoom_reset_btn);

    let zoom_in_btn = gtk4::Button::from_icon_name("zoom-in-symbolic");
    zoom_in_btn.add_css_class("flat");
    zoom_in_btn.set_tooltip_text(Some("Zoom In (Ctrl++)"));
    toolbar.append(&zoom_in_btn);

    // Reload button
    let reload_btn = gtk4::Button::from_icon_name("view-refresh-symbolic");
    reload_btn.add_css_class("flat");
    reload_btn.set_tooltip_text(Some("Reload"));
    toolbar.append(&reload_btn);

    container.append(&toolbar);

    // ── WebView for rendered content ──
    let web_view = webkit6::WebView::new();
    web_view.set_hexpand(true);
    web_view.set_vexpand(true);

    // Enable JavaScript for rendered content
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&web_view) {
        settings.set_enable_javascript(true);
        settings.set_enable_developer_extras(false);
    }

    // Deny all permission requests (camera, microphone, geolocation, etc.)
    web_view.connect_permission_request(|_, request| {
        request.deny();
        true
    });

    // Navigation policy: block external navigations — open http/https links
    // externally with xdg-open, ignore everything else (javascript:, etc.).
    // Programmatic loads via load_html() bypass this handler entirely.
    web_view.connect_decide_policy(|_, decision, decision_type| {
        use webkit6::PolicyDecisionType;
        if decision_type == PolicyDecisionType::NavigationAction {
            if let Ok(nav) = decision
                .clone()
                .downcast::<webkit6::NavigationPolicyDecision>()
            {
                if let Some(action) = nav.navigation_action() {
                    if action.is_user_gesture() {
                        if let Some(req) = action.request() {
                            if let Some(uri) = req.uri() {
                                let url = uri.to_string();
                                if url.starts_with("http://") || url.starts_with("https://") {
                                    let safe: String = url
                                        .chars()
                                        .filter(|c| !c.is_control())
                                        .take(4096)
                                        .collect();
                                    let _ =
                                        std::process::Command::new("xdg-open").arg(&safe).spawn();
                                }
                            }
                        }
                        nav.ignore();
                        return true;
                    }
                }
            }
        }
        false
    });

    // Load rendered content
    if let Some(path) = file_path {
        load_markdown_file(&web_view, path);
    } else {
        let html = render_markdown("# No file specified\n\nOpen a markdown file to view it.");
        web_view.load_html(&html, None);
    }

    // Reload button action
    {
        let wv = web_view.clone();
        let path = file_path.map(String::from);
        reload_btn.connect_clicked(move |_| {
            if let Some(ref p) = path {
                load_markdown_file(&wv, p);
            }
        });
    }

    // Zoom controls — WebKit WebView supports continuous zoom levels.
    const ZOOM_STEP: f64 = 0.1;
    const ZOOM_MIN: f64 = 0.3;
    const ZOOM_MAX: f64 = 5.0;
    {
        let wv = web_view.clone();
        zoom_in_btn.connect_clicked(move |_| {
            let z = (wv.zoom_level() + ZOOM_STEP).min(ZOOM_MAX);
            wv.set_zoom_level(z);
        });
    }
    {
        let wv = web_view.clone();
        zoom_out_btn.connect_clicked(move |_| {
            let z = (wv.zoom_level() - ZOOM_STEP).max(ZOOM_MIN);
            wv.set_zoom_level(z);
        });
    }
    {
        let wv = web_view.clone();
        zoom_reset_btn.connect_clicked(move |_| {
            wv.set_zoom_level(1.0);
        });
    }
    // Ctrl +/-/0 keyboard zoom on the panel.
    {
        // WEAK: the controller is attached to the web view itself — a strong
        // capture is a webview → controller → closure → webview cycle that
        // leaked a whole WebKit WebView per rebuild.
        let wv = web_view.downgrade();
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, keyval, _, modifier| {
            let Some(wv) = wv.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if !modifier.contains(gdk4::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            match keyval {
                gdk4::Key::plus | gdk4::Key::equal | gdk4::Key::KP_Add => {
                    let z = (wv.zoom_level() + ZOOM_STEP).min(ZOOM_MAX);
                    wv.set_zoom_level(z);
                    glib::Propagation::Stop
                }
                gdk4::Key::minus | gdk4::Key::KP_Subtract => {
                    let z = (wv.zoom_level() - ZOOM_STEP).max(ZOOM_MIN);
                    wv.set_zoom_level(z);
                    glib::Propagation::Stop
                }
                gdk4::Key::_0 | gdk4::Key::KP_0 => {
                    wv.set_zoom_level(1.0);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        web_view.add_controller(key_controller);
    }

    // File watcher — native inotify via gio::FileMonitor
    if let Some(path) = file_path {
        let file = gio::File::for_path(path);
        if let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        {
            let path = path.to_string();
            let wv = web_view.clone();
            monitor.connect_changed(move |_monitor, _file, _other, event| {
                if matches!(
                    event,
                    gio::FileMonitorEvent::Changed
                        | gio::FileMonitorEvent::Created
                        | gio::FileMonitorEvent::ChangesDoneHint
                ) {
                    load_markdown_file(&wv, &path);
                }
            });
            // Keep the monitor alive by attaching it to the container widget.
            // SAFETY: The monitor is a GObject that outlives the closure references.
            unsafe { container.set_data("file-monitor", monitor) };
        }
    }

    container.append(&web_view);
    container.upcast()
}

/// Load a markdown file and render it into the WebView.
fn load_markdown_file(web_view: &webkit6::WebView, path: &str) {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let html = render_markdown(&content);
            web_view.load_html(&html, None);
        }
        Err(e) => {
            let html = render_markdown(&format!(
                "# Error\n\nFailed to read `{}`:\n\n```\n{}\n```",
                path, e
            ));
            web_view.load_html(&html, None);
        }
    }
}

/// Convert markdown text to a complete HTML document with styling.
fn render_markdown(markdown: &str) -> String {
    use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    // Rewrite ```mermaid fenced blocks into `<pre class="mermaid">` with the raw
    // (un-escaped) diagram source so mermaid.js can render them client-side.
    let mut in_mermaid = false;
    let events: Vec<Event> = Parser::new_ext(markdown, options)
        .flat_map(|ev| match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref lang)))
                if lang.as_ref() == "mermaid" =>
            {
                in_mermaid = true;
                vec![Event::Html(r#"<pre class="mermaid">"#.into())]
            }
            Event::End(TagEnd::CodeBlock) if in_mermaid => {
                in_mermaid = false;
                vec![Event::Html("</pre>".into())]
            }
            Event::Text(t) if in_mermaid => vec![Event::Html(t)],
            other => vec![other],
        })
        .collect();

    let mut html_output = String::new();
    html::push_html(&mut html_output, events.into_iter());

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
:root {{
    color-scheme: light dark;
}}
body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
    line-height: 1.6;
    max-width: 800px;
    margin: 0 auto;
    padding: 20px;
    color: light-dark(#1a1a1a, #e0e0e0);
    background: light-dark(#ffffff, #1e1e1e);
}}
h1, h2, h3, h4, h5, h6 {{
    margin-top: 1.5em;
    margin-bottom: 0.5em;
    line-height: 1.25;
}}
h1 {{ font-size: 2em; border-bottom: 1px solid light-dark(#eee, #333); padding-bottom: 0.3em; }}
h2 {{ font-size: 1.5em; border-bottom: 1px solid light-dark(#eee, #333); padding-bottom: 0.3em; }}
pre {{
    background: light-dark(#f6f8fa, #2d2d2d);
    border-radius: 6px;
    padding: 16px;
    overflow-x: auto;
    font-size: 0.875em;
}}
code {{
    background: light-dark(#f0f0f0, #2d2d2d);
    padding: 0.2em 0.4em;
    border-radius: 3px;
    font-size: 0.875em;
}}
pre code {{ background: transparent; padding: 0; }}
pre.mermaid {{ background: transparent; text-align: center; }}
blockquote {{
    border-left: 4px solid light-dark(#ddd, #444);
    margin: 0;
    padding: 0.5em 1em;
    color: light-dark(#666, #aaa);
}}
table {{
    border-collapse: collapse;
    width: 100%;
    margin: 1em 0;
}}
th, td {{
    border: 1px solid light-dark(#ddd, #444);
    padding: 6px 12px;
    text-align: left;
}}
th {{ background: light-dark(#f6f8fa, #2d2d2d); }}
a {{ color: light-dark(#0969da, #58a6ff); }}
img {{ max-width: 100%; }}
hr {{ border: none; border-top: 1px solid light-dark(#eee, #333); margin: 2em 0; }}
input[type="checkbox"] {{ margin-right: 0.5em; }}
</style>
</head>
<body>{html_output}
<script type="module">
import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';
mermaid.initialize({{ startOnLoad: true, securityLevel: 'strict',
    theme: window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'default' }});
</script>
</body>
</html>"#
    )
}
