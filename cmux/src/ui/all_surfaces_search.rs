//! All-surfaces search — searches text across all open terminal surfaces.
//!
//! Opens a dialog with a search entry that queries every terminal's scrollback
//! and shows matching lines with workspace/panel context.

use std::rc::Rc;

use gdk4;
use glib;
use gtk4::prelude::*;
use libadwaita as adw;

use crate::app::{lock_or_recover, AppState};

/// Result from searching a single surface.
struct SurfaceMatch {
    workspace_title: String,
    panel_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    /// Matching lines (up to 5 per surface).
    lines: Vec<String>,
}

/// Open the all-surfaces search dialog.
pub fn show_all_surfaces_search(
    parent: &adw::ApplicationWindow,
    state: &Rc<AppState>,
) {
    let dialog = gtk4::Window::builder()
        .title("Search All Terminals")
        .transient_for(parent)
        .modal(true)
        .default_width(600)
        .default_height(400)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    // Search entry
    let entry = gtk4::SearchEntry::new();
    entry.set_placeholder_text(Some("Search across all terminals..."));
    entry.set_hexpand(true);
    entry.set_margin_start(12);
    entry.set_margin_end(12);
    entry.set_margin_top(12);
    entry.set_margin_bottom(6);
    vbox.append(&entry);

    // Results list
    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.add_css_class("boxed-list");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&list_box));
    scroll.set_margin_start(12);
    scroll.set_margin_end(12);
    scroll.set_margin_bottom(12);
    vbox.append(&scroll);

    // Status label
    let status = gtk4::Label::new(Some("Type to search..."));
    status.add_css_class("dim-label");
    status.add_css_class("caption");
    status.set_margin_start(12);
    status.set_margin_bottom(8);
    status.set_halign(gtk4::Align::Start);
    vbox.append(&status);

    dialog.set_child(Some(&vbox));

    // Debounce search
    let debounce_gen = Rc::new(std::cell::Cell::new(0u64));

    {
        let list_box = list_box.clone();
        let status = status.clone();
        let state = state.clone();
        let debounce_gen = debounce_gen.clone();
        let dialog_weak = dialog.downgrade();

        entry.connect_search_changed(move |entry| {
            let gen = debounce_gen.get().wrapping_add(1);
            debounce_gen.set(gen);

            let query = entry.text().to_string();
            let list_box = list_box.clone();
            let status = status.clone();
            let state = state.clone();
            let debounce_gen = debounce_gen.clone();
            let dialog_weak = dialog_weak.clone();

            glib::timeout_add_local_once(
                std::time::Duration::from_millis(150),
                move || {
                    if debounce_gen.get() != gen {
                        return;
                    }
                    if dialog_weak.upgrade().is_none() {
                        return;
                    }
                    run_search(&query, &list_box, &status, &state);
                },
            );
        });
    }

    // Row activation → jump to that workspace/panel
    {
        let state = state.clone();
        let dialog_ref = dialog.clone();
        list_box.connect_row_activated(move |_, row| {
            let panel_id_str = row.widget_name().to_string();
            if let Ok(panel_id) = uuid::Uuid::parse_str(&panel_id_str) {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                    let ws_id = ws.id;
                    ws.focus_panel(panel_id);
                    tm.select_by_id(ws_id);
                }
                drop(tm);
                state.shared.notify_ui_refresh();
            }
            dialog_ref.close();
        });
    }

    // Escape to close
    let key_controller = gtk4::EventControllerKey::new();
    {
        let dialog_ref = dialog.clone();
        key_controller.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk4::Key::Escape {
                dialog_ref.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    dialog.add_controller(key_controller);

    dialog.present();
    entry.grab_focus();
}

fn run_search(
    query: &str,
    list_box: &gtk4::ListBox,
    status: &gtk4::Label,
    state: &Rc<AppState>,
) {
    // Clear previous results
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let trimmed = query.trim();
    if trimmed.is_empty() {
        status.set_text("Type to search...");
        return;
    }

    let query_lower = trimmed.to_lowercase();

    // Read text from all terminal surfaces and search
    let mut matches: Vec<SurfaceMatch> = Vec::new();
    let mut total_matches = 0;

    let tm = lock_or_recover(&state.shared.tab_manager);
    let cache = state.terminal_cache.borrow();

    for ws in tm.iter() {
        for (&panel_id, _panel) in &ws.panels {
            if let Some(surface) = cache.get(&panel_id) {
                if let Some(text) = surface.read_screen_text() {
                    let matching_lines: Vec<String> = text
                        .lines()
                        .filter(|line| line.to_lowercase().contains(&query_lower))
                        .take(5)
                        .map(|line| {
                            // Truncate long lines
                            if line.len() > 120 {
                                format!("{}...", &line[..120])
                            } else {
                                line.to_string()
                            }
                        })
                        .collect();

                    if !matching_lines.is_empty() {
                        total_matches += matching_lines.len();
                        matches.push(SurfaceMatch {
                            workspace_title: ws.display_title().to_string(),
                            panel_id,
                            workspace_id: ws.id,
                            lines: matching_lines,
                        });
                    }
                }
            }
        }
    }
    drop(cache);
    drop(tm);

    // Populate results
    for m in &matches {
        let row_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);

        let header = gtk4::Label::new(Some(&m.workspace_title));
        header.set_xalign(0.0);
        header.add_css_class("caption");
        header.add_css_class("accent");
        row_box.append(&header);

        for line in &m.lines {
            let line_label = gtk4::Label::new(Some(line));
            line_label.set_xalign(0.0);
            line_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            line_label.set_wrap(false);
            line_label.add_css_class("monospace");
            row_box.append(&line_label);
        }

        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&row_box));
        row.set_widget_name(&m.panel_id.to_string());
        list_box.append(&row);
    }

    let surfaces_count = matches.len();
    status.set_text(&format!(
        "{total_matches} match{} across {surfaces_count} terminal{}",
        if total_matches == 1 { "" } else { "es" },
        if surfaces_count == 1 { "" } else { "s" },
    ));
}
