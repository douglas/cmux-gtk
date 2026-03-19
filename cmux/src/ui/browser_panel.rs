//! Browser panel — embedded browser or external browser fallback.
//!
//! WebKit2GTK integration (webkit6 crate) requires gtk4 0.11+.
//! Until the gtk4 dependency is upgraded, this module provides a URL entry
//! that opens pages in the system default browser, with a preview area.

use gtk4::prelude::*;

/// Create a browser panel widget.
///
/// Currently a URL entry + "Open in Browser" button since webkit6 requires
/// gtk4 0.11 (we're on 0.9). The address bar and navigation structure is
/// ready for WebKitWebView integration.
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

    let url_entry = gtk4::Entry::new();
    url_entry.set_hexpand(true);
    url_entry.set_placeholder_text(Some("Enter URL..."));
    if let Some(url) = initial_url {
        url_entry.set_text(url);
    }
    nav_bar.append(&url_entry);

    let open_btn = gtk4::Button::with_label("Open");
    open_btn.add_css_class("suggested-action");
    nav_bar.append(&open_btn);

    container.append(&nav_bar);

    // ── Info area ──
    let info_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    info_box.set_hexpand(true);
    info_box.set_vexpand(true);
    info_box.set_valign(gtk4::Align::Center);
    info_box.set_halign(gtk4::Align::Center);

    let icon = gtk4::Image::from_icon_name("web-browser-symbolic");
    icon.set_pixel_size(48);
    icon.add_css_class("dim-label");
    info_box.append(&icon);

    let label = gtk4::Label::new(Some("Browser Panel"));
    label.add_css_class("title-3");
    info_box.append(&label);

    let subtitle = gtk4::Label::new(Some(
        "Enter a URL above and press Enter or click Open.\nPages open in your default browser.",
    ));
    subtitle.add_css_class("dim-label");
    subtitle.set_justify(gtk4::Justification::Center);
    info_box.append(&subtitle);

    container.append(&info_box);

    // ── Wire navigation ──
    {
        let entry = url_entry.clone();
        open_btn.connect_clicked(move |_| {
            open_url(&entry.text());
        });
    }
    {
        url_entry.connect_activate(move |entry| {
            open_url(&entry.text());
        });
    }

    container.set_widget_name(&panel_id.to_string());
    container.upcast()
}

fn open_url(input: &str) {
    let url = normalize_url(input);
    if url == "about:blank" {
        return;
    }
    if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
        tracing::warn!("Failed to open URL: {}", e);
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
