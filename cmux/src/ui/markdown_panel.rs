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
            .and_then(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
            })
            .or(Some("Markdown")),
    );
    file_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    file_label.set_max_width_chars(50);
    file_label.add_css_class("dim-label");
    toolbar.append(&file_label);

    let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    toolbar.append(&spacer);

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

    // File watcher — poll-based via glib timeout (simpler than notify for a single file)
    if let Some(path) = file_path {
        let path = path.to_string();
        let wv = web_view.clone();
        let last_modified = std::cell::Cell::new(file_mtime(&path));
        glib::timeout_add_seconds_local(2, move || {
            let current = file_mtime(&path);
            if current != last_modified.get() {
                last_modified.set(current);
                load_markdown_file(&wv, &path);
            }
            glib::ControlFlow::Continue
        });
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
    use pulldown_cmark::{html, Options, Parser};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

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
<body>{html_output}</body>
</html>"#
    )
}

/// Get file modification time as seconds since epoch (0 if unavailable).
fn file_mtime(path: &str) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
