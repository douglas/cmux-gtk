//! Sidebar — workspace list using GtkListBox.

use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;

use glib::object::Cast;

use crate::app::{lock_or_recover, AppState};
use crate::model::Workspace;
use crate::settings::SidebarDisplaySettings;

pub struct SidebarWidgets {
    pub root: gtk4::Box,
    pub list_box: gtk4::ListBox,
    pub search_entry: gtk4::SearchEntry,
}

/// Create the sidebar widget containing the workspace list.
pub fn create_sidebar(state: &Rc<AppState>) -> SidebarWidgets {
    let sidebar_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    sidebar_box.add_css_class("sidebar");

    // Search/filter entry at top of sidebar
    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Filter workspaces..."));
    search_entry.set_margin_start(8);
    search_entry.set_margin_end(8);
    search_entry.set_margin_top(4);
    search_entry.set_margin_bottom(4);
    sidebar_box.append(&search_entry);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_vexpand(true);

    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.add_css_class("navigation-sidebar");

    // Apply sidebar focus style from settings
    if crate::settings::load().sidebar.focus_style
        == crate::settings::SidebarFocusStyle::LeftRail
    {
        list_box.add_css_class("sidebar-left-rail");
    }

    // Wire search entry to filter list rows
    {
        let search_entry_weak = search_entry.downgrade();
        list_box.set_filter_func(move |row| {
            let Some(search_entry) = search_entry_weak.upgrade() else {
                return true;
            };
            let query = search_entry.text().to_string().to_lowercase();
            if query.is_empty() {
                return true;
            }
            // Walk the row's widget tree to find the workspace-title label
            let Some(outer) = row.child() else {
                return true;
            };
            let Some(outer_box) = outer.downcast_ref::<gtk4::Box>() else {
                return true;
            };
            let Some(header) = outer_box.first_child() else {
                return true;
            };
            let Some(header_box) = header.downcast_ref::<gtk4::Box>() else {
                return true;
            };
            // Find the title label (has workspace-title class)
            let mut child = header_box.first_child();
            while let Some(c) = child {
                if c.has_css_class("workspace-title") {
                    if let Some(label) = c.downcast_ref::<gtk4::Label>() {
                        return label.text().to_lowercase().contains(&query);
                    }
                }
                child = c.next_sibling();
            }
            true
        });

        let list_box_clone = list_box.clone();
        search_entry.connect_search_changed(move |_| {
            list_box_clone.invalidate_filter();
        });
    }

    refresh_sidebar(&list_box, state);

    scrolled.set_child(Some(&list_box));
    sidebar_box.append(&scrolled);

    // Footer with version label
    let footer = gtk4::Label::new(Some(&format!("cmux v{}", env!("CARGO_PKG_VERSION"))));
    footer.add_css_class("dim-label");
    footer.add_css_class("caption");
    footer.set_margin_top(4);
    footer.set_margin_bottom(4);
    footer.set_halign(gtk4::Align::Center);
    footer.set_opacity(0.5);
    sidebar_box.append(&footer);

    SidebarWidgets {
        root: sidebar_box,
        list_box,
        search_entry,
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
    let sidebar_settings = crate::settings::load().sidebar;
    let (rows, selected_index): (Vec<gtk4::ListBoxRow>, Option<usize>) = {
        let tab_manager = lock_or_recover(&state.shared.tab_manager);
        let selected_index = tab_manager.selected_index();
        let rows = tab_manager
            .iter()
            .enumerate()
            .map(|(index, workspace)| {
                let row = create_workspace_row(workspace, index, &sidebar_settings);
                setup_row_context_menu(&row, index, workspace.is_pinned, state);
                setup_row_close_button(&row, index, state);
                row
            })
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

    // Reapply search filter after rebuild
    list_box.invalidate_filter();
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

fn create_workspace_row(
    workspace: &Workspace,
    index: usize,
    sidebar: &SidebarDisplaySettings,
) -> gtk4::ListBoxRow {
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

    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    outer.set_margin_start(10);
    outer.set_margin_end(10);
    outer.set_margin_top(5);
    outer.set_margin_bottom(5);

    // ── Header: index + pin icon + title + unread badge + close button ──
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);

    let index_label = gtk4::Label::new(Some(&format!("{}", index + 1)));
    index_label.add_css_class("dim-label");
    index_label.add_css_class("caption");
    index_label.add_css_class("workspace-index");
    header.append(&index_label);

    // Workspace type icon — pick based on dominant panel type
    let has_browser = workspace
        .panels
        .values()
        .any(|p| p.panel_type == crate::model::PanelType::Browser);
    let icon_name = if has_browser {
        "globe-symbolic"
    } else {
        "utilities-terminal-symbolic"
    };
    let type_icon = gtk4::Image::from_icon_name(icon_name);
    type_icon.set_pixel_size(14);
    type_icon.add_css_class("workspace-type-icon");
    header.append(&type_icon);

    // Pin indicator
    if workspace.is_pinned {
        let pin_icon = gtk4::Image::from_icon_name("view-pin-symbolic");
        pin_icon.set_pixel_size(12);
        pin_icon.add_css_class("dim-label");
        header.append(&pin_icon);
    }

    let title_label = gtk4::Label::new(Some(workspace.display_title()));
    title_label.set_hexpand(true);
    title_label.set_halign(gtk4::Align::Start);
    title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title_label.add_css_class("workspace-title");
    header.append(&title_label);

    if workspace.unread_count > 0 {
        let badge = gtk4::Label::new(Some(&workspace.unread_count.to_string()));
        badge.add_css_class("badge");
        badge.add_css_class("accent");
        header.append(&badge);
    }

    // Hover close button (hidden by default)
    let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("flat");
    close_btn.add_css_class("circular");
    close_btn.add_css_class("sidebar-close-btn");
    close_btn.set_visible(false);
    close_btn.set_tooltip_text(Some("Close workspace"));
    header.append(&close_btn);

    outer.append(&header);

    // ── Meta line: agent status | git branch | directory ──
    let meta_label = gtk4::Label::new(Some(&workspace_meta_text(workspace, sidebar)));
    meta_label.set_halign(gtk4::Align::Start);
    meta_label.set_wrap(false);
    meta_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    meta_label.add_css_class("caption");
    meta_label.add_css_class("dim-label");
    outer.append(&meta_label);

    // ── Status pills ──
    if sidebar.show_status_pills && !workspace.status_entries.is_empty() {
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
    if sidebar.show_progress {
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
                bar.pulse();
            } else {
                bar.set_fraction(progress.value.clamp(0.0, 1.0));
            }
            progress_box.append(&bar);
            outer.append(&progress_box);
        }
    }

    // ── Listening ports ──
    if sidebar.show_ports {
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
    }

    // ── Latest log entry ──
    if sidebar.show_logs {
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
    }

    // ── PR status pill ──
    if sidebar.show_pr_status {
    if let Some(ref pr_status) = workspace.pr_status {
        let pr_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        pr_box.set_halign(gtk4::Align::Start);
        let pr_label = gtk4::Label::new(Some(&format!("PR: {pr_status}")));
        pr_label.add_css_class("status-pill");
        pr_label.add_css_class("caption");
        match pr_status.as_str() {
            "merged" => pr_label.add_css_class("status-pill-green"),
            "open" | "draft" => pr_label.add_css_class("status-pill-yellow"),
            "closed" => pr_label.add_css_class("status-pill-red"),
            _ => {}
        }
        pr_box.append(&pr_label);
        outer.append(&pr_box);
    }
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

    // ── Hover show/hide close button ──
    let motion = gtk4::EventControllerMotion::new();
    {
        let close_btn = close_btn.clone();
        motion.connect_enter(move |_, _, _| {
            close_btn.set_visible(true);
        });
    }
    {
        let close_btn = close_btn.clone();
        motion.connect_leave(move |_| {
            close_btn.set_visible(false);
        });
    }
    row.add_controller(motion);

    row.set_child(Some(&outer));
    row
}

/// Set up right-click context menu on a sidebar row.
fn setup_row_context_menu(
    row: &gtk4::ListBoxRow,
    index: usize,
    is_pinned: bool,
    state: &Rc<AppState>,
) {
    let menu = gtk4::gio::Menu::new();
    menu.append(
        Some(if is_pinned { "Unpin" } else { "Pin" }),
        Some(&format!("sidebar.toggle-pin.{index}")),
    );
    menu.append(Some("Rename"), Some(&format!("sidebar.rename.{index}")));

    // Color submenu — 16-color palette matching macOS
    let color_menu = gtk4::gio::Menu::new();
    for (label, color) in &[
        ("Red", "red"),
        ("Crimson", "crimson"),
        ("Orange", "orange"),
        ("Amber", "amber"),
        ("Yellow", "yellow"),
        ("Lime", "lime"),
        ("Green", "green"),
        ("Teal", "teal"),
        ("Cyan", "cyan"),
        ("Sky", "sky"),
        ("Blue", "blue"),
        ("Indigo", "indigo"),
        ("Purple", "purple"),
        ("Violet", "violet"),
        ("Pink", "pink"),
        ("Rose", "rose"),
        ("None", ""),
    ] {
        color_menu.append(Some(label), Some(&format!("sidebar.color.{index}.{color}")));
    }
    menu.append_submenu(Some("Set Color"), &color_menu);

    menu.append(
        Some("Mark as Read"),
        Some(&format!("sidebar.mark-read.{index}")),
    );
    menu.append(
        Some("Mark as Unread"),
        Some(&format!("sidebar.mark-unread.{index}")),
    );
    menu.append(Some("Close"), Some(&format!("sidebar.close.{index}")));

    let popover = gtk4::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(row);
    popover.set_has_arrow(false);

    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3); // Right click
    {
        let popover = popover.clone();
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk4::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    row.add_controller(gesture);

    // Actions
    let action_group = gtk4::gio::SimpleActionGroup::new();

    // Toggle pin
    let pin_action = gtk4::gio::SimpleAction::new(&format!("toggle-pin.{index}"), None);
    {
        let state = state.clone();
        pin_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.get_mut(index) {
                ws.is_pinned = !ws.is_pinned;
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&pin_action);

    // Rename
    let rename_action = gtk4::gio::SimpleAction::new(&format!("rename.{index}"), None);
    {
        let state = state.clone();
        let row_weak = row.downgrade();
        rename_action.connect_activate(move |_, _| {
            let current_title = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.get(index).map(|ws| ws.display_title().to_string())
            };
            if let Some(title) = current_title {
                if let Some(row) = row_weak.upgrade() {
                    if let Some(root) = row.root() {
                        if let Some(window) =
                            root.downcast_ref::<libadwaita::ApplicationWindow>()
                        {
                            show_rename_for_index(window, &state, index, &title);
                        }
                    }
                }
            }
        });
    }
    action_group.add_action(&rename_action);

    // Color actions — 16-color palette matching macOS + "" for clear
    for color in &[
        "red", "crimson", "orange", "amber", "yellow", "lime", "green", "teal",
        "cyan", "sky", "blue", "indigo", "purple", "violet", "pink", "rose", "",
    ] {
        let action_name = format!("color.{index}.{color}");
        let color_action = gtk4::gio::SimpleAction::new(&action_name, None);
        let color_value = if color.is_empty() {
            None
        } else {
            Some(color_css_value(color).to_string())
        };
        {
            let state = state.clone();
            color_action.connect_activate(move |_, _| {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(ws) = tm.get_mut(index) {
                    ws.custom_color = color_value.clone();
                }
                drop(tm);
                state.shared.notify_ui_refresh();
            });
        }
        action_group.add_action(&color_action);
    }

    // Mark read
    let mark_read_action = gtk4::gio::SimpleAction::new(&format!("mark-read.{index}"), None);
    {
        let state = state.clone();
        mark_read_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.get_mut(index) {
                ws.mark_notifications_read();
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&mark_read_action);

    // Mark unread
    let mark_unread_action = gtk4::gio::SimpleAction::new(&format!("mark-unread.{index}"), None);
    {
        let state = state.clone();
        mark_unread_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.get_mut(index) {
                ws.unread_count = ws.unread_count.max(1);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&mark_unread_action);

    // Close
    let close_action = gtk4::gio::SimpleAction::new(&format!("close.{index}"), None);
    {
        let state = state.clone();
        close_action.connect_activate(move |_, _| {
            lock_or_recover(&state.shared.tab_manager).remove(index);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&close_action);

    row.insert_action_group("sidebar", Some(&action_group));
}

fn show_rename_for_index(
    window: &libadwaita::ApplicationWindow,
    state: &Rc<AppState>,
    index: usize,
    current_title: &str,
) {
    use libadwaita::prelude::*;

    let dialog = libadwaita::MessageDialog::new(
        Some(window),
        Some("Rename Workspace"),
        None::<&str>,
    );
    dialog.set_body("Enter a new name for this workspace:");

    let entry = gtk4::Entry::new();
    entry.set_text(current_title);
    entry.set_activates_default(true);
    dialog.set_extra_child(Some(&entry));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("rename", "Rename");
    dialog.set_default_response(Some("rename"));
    dialog.set_response_appearance("rename", libadwaita::ResponseAppearance::Suggested);

    let state = state.clone();
    dialog.connect_response(None::<&str>, move |dialog, response| {
        if response == "rename" {
            let entry = dialog
                .extra_child()
                .and_then(|w| w.downcast::<gtk4::Entry>().ok());
            if let Some(entry) = entry {
                let new_name = entry.text().to_string();
                if !new_name.is_empty() {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.get_mut(index) {
                        ws.custom_title = Some(new_name);
                    }
                    drop(tm);
                    state.shared.notify_ui_refresh();
                }
            }
        }
    });

    dialog.present();
}

fn color_css_value(name: &str) -> &str {
    match name {
        "red" => "#e01b24",
        "crimson" => "#dc143c",
        "orange" => "#ff7800",
        "amber" => "#ffbf00",
        "yellow" => "#f6d32d",
        "lime" => "#a3be8c",
        "green" => "#33d17a",
        "teal" => "#2aa198",
        "cyan" => "#00bcd4",
        "sky" => "#87ceeb",
        "blue" => "#3584e4",
        "indigo" => "#4b0082",
        "purple" => "#9141ac",
        "violet" => "#7c3aed",
        "pink" => "#e91e8c",
        "rose" => "#f43f5e",
        _ => "",
    }
}

/// Wire up the hover close button on a row.
fn setup_row_close_button(row: &gtk4::ListBoxRow, index: usize, state: &Rc<AppState>) {
    // Find the close button (it's the last child in the header box)
    let Some(outer) = row.child() else { return };
    let outer = outer.downcast_ref::<gtk4::Box>().cloned();
    let Some(outer) = outer else { return };
    let Some(header) = outer.first_child() else { return };
    let header = header.downcast_ref::<gtk4::Box>().cloned();
    let Some(header) = header else { return };

    // Walk to find the button
    let mut child = header.first_child();
    while let Some(c) = child {
        if c.has_css_class("sidebar-close-btn") {
            if let Some(btn) = c.downcast_ref::<gtk4::Button>() {
                let state = state.clone();
                btn.connect_clicked(move |_| {
                    lock_or_recover(&state.shared.tab_manager).remove(index);
                    state.shared.notify_ui_refresh();
                });
            }
            break;
        }
        child = c.next_sibling();
    }
}

fn workspace_meta_text(workspace: &Workspace, sidebar: &SidebarDisplaySettings) -> String {
    let mut parts = Vec::new();

    if let Some(status) = workspace.sidebar_status_label() {
        parts.push(status.to_string());
    }

    if sidebar.show_git_branch {
        if let Some(git_branch) = &workspace.git_branch {
            parts.push(if git_branch.is_dirty {
                format!("git {} *", git_branch.branch)
            } else {
                format!("git {}", git_branch.branch)
            });
        }
    }

    if sidebar.show_directory {
        parts.push(compact_path(&workspace.current_directory));
    }

    parts.join(" | ")
}

pub fn compact_path(path: &str) -> String {
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
