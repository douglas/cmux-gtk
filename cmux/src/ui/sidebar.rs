//! Sidebar — workspace list using GtkListBox.

use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{lock_or_recover, AppState};
use crate::model::Workspace;

pub struct SidebarWidgets {
    pub root: gtk4::Box,
    pub list_box: gtk4::ListBox,
}

/// Create the sidebar widget containing the workspace list.
pub fn create_sidebar(state: &Rc<AppState>) -> SidebarWidgets {
    let sidebar_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    sidebar_box.add_css_class("sidebar");

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_vexpand(true);

    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.add_css_class("navigation-sidebar");

    refresh_sidebar(&list_box, state);

    scrolled.set_child(Some(&list_box));
    sidebar_box.append(&scrolled);

    SidebarWidgets {
        root: sidebar_box,
        list_box,
    }
}

/// Refresh the workspace list from shared state.
pub fn refresh_sidebar(list_box: &gtk4::ListBox, state: &Rc<AppState>) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    // Build rows and capture selection index while holding the lock, then
    // release the lock before calling list_box.select_row.  select_row emits
    // `row-selected` synchronously; the connected handler tries to acquire
    // the same tab_manager lock, which would deadlock on std::sync::Mutex.
    let (rows, selected_index): (Vec<gtk4::ListBoxRow>, Option<usize>) = {
        let tab_manager = lock_or_recover(&state.shared.tab_manager);
        let selected_index = tab_manager.selected_index();
        let rows = tab_manager
            .iter()
            .enumerate()
            .map(|(index, workspace)| create_workspace_row(workspace, index))
            .collect();
        (rows, selected_index)
    };

    for (index, row) in rows.iter().enumerate() {
        // Drag-and-drop for workspace reordering
        setup_row_drag_drop(row, index, state);
        list_box.append(row);
        if selected_index == Some(index) {
            list_box.select_row(Some(row));
        }
    }
}

/// Set up drag-and-drop on a sidebar workspace row for reordering.
fn setup_row_drag_drop(row: &gtk4::ListBoxRow, index: usize, state: &Rc<AppState>) {
    // Drag source — provides the source index as a string
    let drag_source = gtk4::DragSource::new();
    drag_source.set_actions(gdk4::DragAction::MOVE);
    {
        let index_str = index.to_string();
        drag_source.connect_prepare(move |_source, _x, _y| {
            let content = gdk4::ContentProvider::for_value(&index_str.to_value());
            Some(content)
        });
    }
    row.add_controller(drag_source);

    // Drop target — accepts a string (the source index) and reorders
    let drop_target = gtk4::DropTarget::new(glib::Type::STRING, gdk4::DragAction::MOVE);
    {
        let state = state.clone();
        let target_index = index;
        drop_target.connect_drop(move |_target, value, _x, _y| {
            let Ok(source_str) = value.get::<String>() else {
                return false;
            };
            let Ok(source_index) = source_str.parse::<usize>() else {
                return false;
            };
            if source_index == target_index {
                return false;
            }
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.move_workspace(source_index, target_index);
            drop(tm);
            state.shared.notify_ui_refresh();
            true
        });
    }
    row.add_controller(drop_target);
}

fn create_workspace_row(workspace: &Workspace, index: usize) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();

    // Workspace color indicator: colored left border when custom_color is set.
    if let Some(ref color) = workspace.custom_color {
        row.add_css_class("workspace-row-colored");
        let css = gtk4::CssProvider::new();
        css.load_from_data(&format!(
            "row {{ border-left-color: {}; }}",
            color
        ));
        row.style_context().add_provider(&css, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    } else {
        row.add_css_class("workspace-row");
    }

    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    outer.set_margin_start(10);
    outer.set_margin_end(10);
    outer.set_margin_top(8);
    outer.set_margin_bottom(8);

    // ── Header: index + title + unread badge ──
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);

    let index_label = gtk4::Label::new(Some(&format!("{}", index + 1)));
    index_label.add_css_class("dim-label");
    index_label.add_css_class("caption");
    header.append(&index_label);

    let title_label = gtk4::Label::new(Some(workspace.display_title()));
    title_label.set_hexpand(true);
    title_label.set_halign(gtk4::Align::Start);
    title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    header.append(&title_label);

    if workspace.unread_count > 0 {
        let badge = gtk4::Label::new(Some(&workspace.unread_count.to_string()));
        badge.add_css_class("badge");
        badge.add_css_class("accent");
        header.append(&badge);
    }

    outer.append(&header);

    // ── Meta line: agent status | git branch | directory ──
    let meta_label = gtk4::Label::new(Some(&workspace_meta_text(workspace)));
    meta_label.set_halign(gtk4::Align::Start);
    meta_label.set_wrap(false);
    meta_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    meta_label.add_css_class("caption");
    meta_label.add_css_class("dim-label");
    outer.append(&meta_label);

    // ── Status pills ──
    if !workspace.status_entries.is_empty() {
        let pills_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        pills_box.set_halign(gtk4::Align::Start);
        // Show up to 4 most recent status entries
        let entries: Vec<_> = workspace.status_entries.iter().rev().take(4).collect();
        for entry in entries.into_iter().rev() {
            let text = if entry.key == "agent" {
                entry.value.clone()
            } else {
                format!("{}: {}", entry.key, entry.value)
            };
            let pill = gtk4::Label::new(Some(&text));
            pill.add_css_class("status-pill");
            pill.add_css_class("caption");
            pill.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            pill.set_max_width_chars(20);
            if let Some(ref color) = entry.color {
                match color.as_str() {
                    "blue" => pill.add_css_class("status-pill-blue"),
                    "green" => pill.add_css_class("status-pill-green"),
                    "red" => pill.add_css_class("status-pill-red"),
                    "orange" => pill.add_css_class("status-pill-orange"),
                    "purple" => pill.add_css_class("status-pill-purple"),
                    "yellow" => pill.add_css_class("status-pill-yellow"),
                    _ => {}
                }
            }
            pills_box.append(&pill);
        }
        outer.append(&pills_box);
    }

    // ── Progress bar ──
    if let Some(ref progress) = workspace.progress {
        let progress_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        if let Some(ref label_text) = progress.label {
            let label = gtk4::Label::new(Some(label_text));
            label.set_halign(gtk4::Align::Start);
            label.add_css_class("caption");
            label.add_css_class("dim-label");
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            progress_box.append(&label);
        }
        let bar = gtk4::ProgressBar::new();
        bar.add_css_class("sidebar-progress");
        if progress.value > 1.0 {
            // Indeterminate — pulse
            bar.pulse();
        } else {
            bar.set_fraction(progress.value.clamp(0.0, 1.0));
        }
        progress_box.append(&bar);
        outer.append(&progress_box);
    }

    // ── Listening ports ──
    let all_ports: Vec<u16> = workspace
        .panels
        .values()
        .flat_map(|p| &p.listening_ports)
        .copied()
        .collect();
    if !all_ports.is_empty() {
        let ports_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        ports_box.set_halign(gtk4::Align::Start);
        let mut sorted_ports = all_ports;
        sorted_ports.sort_unstable();
        sorted_ports.dedup();
        for port in sorted_ports.iter().take(5) {
            let port_label = gtk4::Label::new(Some(&format!(":{port}")));
            port_label.add_css_class("port-badge");
            port_label.add_css_class("caption");
            ports_box.append(&port_label);
        }
        if sorted_ports.len() > 5 {
            let more = gtk4::Label::new(Some(&format!("+{}", sorted_ports.len() - 5)));
            more.add_css_class("port-badge");
            more.add_css_class("caption");
            ports_box.append(&more);
        }
        outer.append(&ports_box);
    }

    // ── Latest log entry ──
    if let Some(log_entry) = workspace.log_entries.last() {
        let log_text = if let Some(ref source) = log_entry.source {
            format!("[{}] {}", source, log_entry.message)
        } else {
            log_entry.message.clone()
        };
        let log_label = gtk4::Label::new(Some(&log_text));
        log_label.set_halign(gtk4::Align::Start);
        log_label.set_wrap(false);
        log_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        log_label.add_css_class("caption");
        match log_entry.level.as_str() {
            "warning" | "warn" => log_label.add_css_class("log-warning"),
            "error" => log_label.add_css_class("log-error"),
            "success" => log_label.add_css_class("log-success"),
            "progress" => log_label.add_css_class("log-progress"),
            _ => log_label.add_css_class("log-info"),
        }
        outer.append(&log_label);
    }

    // ── Notification line ──
    let notification_text = workspace
        .latest_notification
        .clone()
        .unwrap_or_else(|| compact_path(&workspace.current_directory));
    let notification_label = gtk4::Label::new(Some(&notification_text));
    notification_label.set_halign(gtk4::Align::Start);
    notification_label.set_wrap(false);
    notification_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    notification_label.add_css_class("caption");
    if workspace.unread_count > 0 {
        notification_label.add_css_class("sidebar-notification");
    } else {
        notification_label.add_css_class("dim-label");
    }
    outer.append(&notification_label);

    row.set_child(Some(&outer));
    row
}

fn workspace_meta_text(workspace: &Workspace) -> String {
    let mut parts = Vec::new();

    if let Some(status) = workspace.sidebar_status_label() {
        parts.push(status.to_string());
    }

    if let Some(git_branch) = &workspace.git_branch {
        parts.push(if git_branch.is_dirty {
            format!("git {} *", git_branch.branch)
        } else {
            format!("git {}", git_branch.branch)
        });
    } else {
        parts.push(compact_path(&workspace.current_directory));
    }

    parts.join(" | ")
}

fn compact_path(path: &str) -> String {
    if path.is_empty() {
        return "~".to_string();
    }

    if let Ok(home) = std::env::var("HOME") {
        // Guard against HOME="/" where strip_prefix would match any absolute path
        if home != "/" {
            let p = Path::new(path);
            if let Ok(stripped) = p.strip_prefix(&home) {
                let s = stripped.display();
                return if stripped.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    format!("~/{s}")
                };
            }
        }
    }

    let path = Path::new(path);
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        return name.to_string();
    }

    path.to_string_lossy().into_owned()
}
