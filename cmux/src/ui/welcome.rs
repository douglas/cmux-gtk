//! Welcome screen — shown on first launch when no session exists.

use gtk4::prelude::*;

/// Check whether we should show the welcome screen.
/// Returns true if the session file doesn't exist (first launch).
pub fn should_show_welcome() -> bool {
    let path = crate::session::store::session_file_exists();
    !path
}

/// Build the welcome screen widget. Fills the content area until the user
/// creates their first workspace or starts using the terminal.
pub fn build_welcome() -> gtk4::Widget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    container.set_margin_start(48);
    container.set_margin_end(48);
    container.set_margin_top(48);
    container.set_margin_bottom(48);

    // App icon / title
    let title = gtk4::Label::new(Some("Welcome to cmux"));
    title.add_css_class("title-1");
    container.append(&title);

    let subtitle = gtk4::Label::new(Some(
        "A terminal multiplexer with integrated browser for Linux",
    ));
    subtitle.add_css_class("dim-label");
    container.append(&subtitle);

    // Quick-start tips
    let tips_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    tips_box.set_margin_top(24);

    let tips = [
        ("Ctrl+Shift+T", "New workspace"),
        ("Ctrl+Shift+D", "Split pane horizontally"),
        ("Ctrl+Shift+E", "Split pane vertically"),
        ("Ctrl+Shift+L", "Open browser panel"),
        ("Ctrl+Shift+P", "Command palette"),
        ("Ctrl+P", "Search all terminals"),
        ("Ctrl+F", "Find in terminal"),
        ("Ctrl+,", "Settings"),
    ];

    for (key, desc) in &tips {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        row.set_halign(gtk4::Align::Center);

        let key_label = gtk4::Label::new(Some(key));
        key_label.add_css_class("caption");
        key_label.add_css_class("monospace");
        key_label.set_width_chars(16);
        key_label.set_xalign(1.0);
        row.append(&key_label);

        let desc_label = gtk4::Label::new(Some(desc));
        desc_label.set_width_chars(26);
        desc_label.set_xalign(0.0);
        row.append(&desc_label);

        tips_box.append(&row);
    }

    container.append(&tips_box);

    // Version
    let version = gtk4::Label::new(Some("cmux-gtk"));
    version.add_css_class("caption");
    version.add_css_class("dim-label");
    version.set_margin_top(24);
    container.append(&version);

    container.upcast()
}
