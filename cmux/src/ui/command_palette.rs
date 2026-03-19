//! Command palette — modal dialog with fuzzy-filtered action list.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;

use crate::app::{lock_or_recover, AppState};
use crate::model::panel::SplitOrientation;
use crate::model::{PanelType, Workspace};

/// A registered command palette action.
struct PaletteAction {
    name: String,
    label: String,
}

/// Show the command palette as a modal dialog.
pub fn show_command_palette(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    on_refresh: Rc<dyn Fn()>,
) {
    let dialog = gtk4::Window::builder()
        .transient_for(window)
        .modal(true)
        .decorated(false)
        .default_width(480)
        .default_height(400)
        .build();
    dialog.add_css_class("command-palette");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let entry = gtk4::SearchEntry::new();
    entry.set_placeholder_text(Some("Type a command..."));
    entry.set_hexpand(true);
    vbox.append(&entry);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_vexpand(true);

    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.add_css_class("navigation-sidebar");
    scrolled.set_child(Some(&list_box));
    vbox.append(&scrolled);

    dialog.set_child(Some(&vbox));

    // Build static actions
    let actions = build_actions(state);

    // Populate initially
    populate_list(&list_box, &actions, "");

    // Filter on search
    {
        let list_box = list_box.clone();
        let actions = actions.clone();
        entry.connect_search_changed(move |entry| {
            let query = entry.text().to_string();
            populate_list(&list_box, &actions, &query);
        });
    }

    // Activate on row selection (click)
    {
        let state = state.clone();
        let dialog = dialog.clone();
        let on_refresh = on_refresh.clone();
        let actions = actions.clone();
        list_box.connect_row_activated(move |_list, row| {
            let index = row.index() as usize;
            // The visible rows correspond to the filtered actions, but
            // we stored the action name in the row's widget-name.
            let name = row.widget_name().to_string();
            execute_action(&name, &state, &on_refresh);
            dialog.close();
            let _ = (index, &actions);
        });
    }

    // Enter key activates selected row
    {
        let list_box = list_box.clone();
        let state = state.clone();
        let dialog_weak = dialog.downgrade();
        let on_refresh = on_refresh.clone();
        entry.connect_activate(move |_| {
            if let Some(row) = list_box.selected_row() {
                let name = row.widget_name().to_string();
                execute_action(&name, &state, &on_refresh);
                if let Some(dialog) = dialog_weak.upgrade() {
                    dialog.close();
                }
            }
        });
    }

    // Escape closes
    let key_controller = gtk4::EventControllerKey::new();
    {
        let dialog = dialog.clone();
        key_controller.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk4::Key::Escape {
                dialog.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    dialog.add_controller(key_controller);

    // Arrow keys move selection from entry
    let key_controller2 = gtk4::EventControllerKey::new();
    {
        let list_box = list_box.clone();
        key_controller2.connect_key_pressed(move |_, keyval, _, _| {
            match keyval {
                gdk4::Key::Down => {
                    if let Some(row) = list_box.selected_row() {
                        let next_index = row.index() + 1;
                        if let Some(next) = list_box.row_at_index(next_index) {
                            list_box.select_row(Some(&next));
                        }
                    } else if let Some(first) = list_box.row_at_index(0) {
                        list_box.select_row(Some(&first));
                    }
                    glib::Propagation::Stop
                }
                gdk4::Key::Up => {
                    if let Some(row) = list_box.selected_row() {
                        let prev_index = row.index() - 1;
                        if prev_index >= 0 {
                            if let Some(prev) = list_box.row_at_index(prev_index) {
                                list_box.select_row(Some(&prev));
                            }
                        }
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }
    entry.add_controller(key_controller2);

    dialog.present();
    entry.grab_focus();
}

fn build_actions(state: &Rc<AppState>) -> Rc<Vec<PaletteAction>> {
    let mut actions = vec![
        PaletteAction {
            name: "workspace.new".into(),
            label: "New Workspace".into(),
        },
        PaletteAction {
            name: "pane.split_horizontal".into(),
            label: "Split Horizontal".into(),
        },
        PaletteAction {
            name: "pane.split_vertical".into(),
            label: "Split Vertical".into(),
        },
        PaletteAction {
            name: "pane.close".into(),
            label: "Close Pane".into(),
        },
        PaletteAction {
            name: "workspace.close".into(),
            label: "Close Workspace".into(),
        },
        PaletteAction {
            name: "pane.zoom_toggle".into(),
            label: "Toggle Pane Zoom".into(),
        },
        PaletteAction {
            name: "settings.open".into(),
            label: "Open Settings".into(),
        },
        PaletteAction {
            name: "pane.focus_next".into(),
            label: "Focus Next Pane".into(),
        },
        PaletteAction {
            name: "pane.focus_prev".into(),
            label: "Focus Previous Pane".into(),
        },
        PaletteAction {
            name: "pane.last".into(),
            label: "Focus Last Pane".into(),
        },
    ];

    // Add dynamic workspace entries
    {
        let tm = lock_or_recover(&state.shared.tab_manager);
        for (i, ws) in tm.iter().enumerate() {
            actions.push(PaletteAction {
                name: format!("workspace.select.{i}"),
                label: format!("Go to: {} ({})", ws.display_title(), i + 1),
            });
        }
    }

    Rc::new(actions)
}

fn populate_list(list_box: &gtk4::ListBox, actions: &[PaletteAction], query: &str) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let query_lower = query.to_lowercase();
    let mut first = true;

    for action in actions {
        if !query.is_empty() && !fuzzy_match(&action.label, &query_lower) {
            continue;
        }

        let row = gtk4::ListBoxRow::new();
        row.set_widget_name(&action.name);

        let label = gtk4::Label::new(Some(&action.label));
        label.set_halign(gtk4::Align::Start);
        label.set_margin_start(12);
        label.set_margin_end(12);
        label.set_margin_top(6);
        label.set_margin_bottom(6);
        row.set_child(Some(&label));

        list_box.append(&row);
        if first {
            list_box.select_row(Some(&row));
            first = false;
        }
    }
}

fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    let haystack_lower = haystack.to_lowercase();
    let mut hay_iter = haystack_lower.chars();
    for needle_char in needle.chars() {
        loop {
            match hay_iter.next() {
                Some(h) if h == needle_char => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

fn execute_action(name: &str, state: &Rc<AppState>, on_refresh: &Rc<dyn Fn()>) {
    match name {
        "workspace.new" => {
            lock_or_recover(&state.shared.tab_manager).add_workspace(Workspace::new());
        }
        "pane.split_horizontal" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.split(SplitOrientation::Horizontal, PanelType::Terminal);
            }
        }
        "pane.split_vertical" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.split(SplitOrientation::Vertical, PanelType::Terminal);
            }
        }
        "pane.close" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.selected_mut() {
                if let Some(panel_id) = ws.focused_panel_id {
                    ws.remove_panel(panel_id);
                    if ws.is_empty() {
                        let ws_id = ws.id;
                        tm.remove_by_id(ws_id);
                    }
                }
            }
        }
        "workspace.close" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(idx) = tm.selected_index() {
                tm.remove(idx);
            }
        }
        "pane.zoom_toggle" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                if ws.zoomed_panel_id.is_some() {
                    ws.zoomed_panel_id = None;
                } else {
                    ws.zoomed_panel_id = ws.focused_panel_id;
                }
            }
        }
        "settings.open" => {
            state.shared.send_ui_event(crate::app::UiEvent::OpenSettings);
            return; // Don't refresh — the settings dialog handles itself
        }
        "pane.focus_next" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                if let Some(current) = ws.focused_panel_id {
                    if let Some(next) = ws.layout.next_panel_id(current) {
                        ws.focus_panel(next);
                    }
                }
            }
        }
        "pane.focus_prev" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                if let Some(current) = ws.focused_panel_id {
                    if let Some(prev) = ws.layout.prev_panel_id(current) {
                        ws.focus_panel(prev);
                    }
                }
            }
        }
        "pane.last" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                if let Some(prev_id) = ws.previous_focused_panel_id {
                    ws.focus_panel(prev_id);
                }
            }
        }
        name if name.starts_with("workspace.select.") => {
            if let Ok(index) = name[17..].parse::<usize>() {
                lock_or_recover(&state.shared.tab_manager).select(index);
            }
        }
        _ => {}
    }
    on_refresh();
}
