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
    /// Keyboard shortcut hint (e.g., "Ctrl+Shift+T").
    shortcut: Option<String>,
    /// Whether this is a workspace switcher entry (shown in default mode).
    is_workspace: bool,
    /// Whether this is a surface text search result (only shown when query is non-empty).
    is_search_result: bool,
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
    entry.set_placeholder_text(Some("Switch workspace, or type > for commands..."));
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
        key_controller2.connect_key_pressed(move |_, keyval, _, _| match keyval {
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
        });
    }
    entry.add_controller(key_controller2);

    dialog.present();
    entry.grab_focus();
}

/// Shortcut config key mapping for palette action names.
/// Maps palette action names to ShortcutConfig keys where they differ.
fn shortcut_for_action(
    shortcuts: &crate::settings::shortcuts::ShortcutConfig,
    action_name: &str,
) -> Option<String> {
    // Map palette action names to shortcut config keys
    let key = match action_name {
        "workspace.new" => "workspace.new",
        "workspace.close" => "workspace.close",
        "workspace.rename" => "workspace.rename",
        "workspace.latest_unread" => "workspace.latest_unread",
        "pane.close" => "pane.close",
        "pane.split_horizontal" => "pane.split_horizontal",
        "pane.split_vertical" => "pane.split_vertical",
        "pane.focus_next" => "pane.focus_next",
        "pane.focus_prev" => "pane.focus_prev",
        "pane.focus_left" => "pane.focus_left",
        "pane.focus_right" => "pane.focus_right",
        "pane.focus_up" => "pane.focus_up",
        "pane.focus_down" => "pane.focus_down",
        "settings.open" => "settings",
        "config.reload" => "config.reload",
        "notifications.toggle" => "notifications.toggle",
        "font.increase" => "font.increase",
        "font.decrease" => "font.decrease",
        "font.reset" => "font.reset",
        "surface.clear_screen" | "surface.clear_history" => "surface.clear",
        "tab.close_others" => "tab.close_others",
        "pane.split_browser_h" => "browser.split_horizontal",
        "pane.split_browser_v" => "browser.split_vertical",
        _ => return None,
    };
    shortcuts.get(key).map(|kb| kb.display())
}

fn build_actions(state: &Rc<AppState>) -> Rc<Vec<PaletteAction>> {
    let shortcuts = crate::settings::shortcuts::load();

    let cmd = |name: &str, label: &str| -> PaletteAction {
        let shortcut = shortcut_for_action(&shortcuts, name);
        PaletteAction {
            name: name.into(),
            label: label.into(),
            shortcut,
            is_workspace: false,
            is_search_result: false,
        }
    };

    let mut actions = vec![
        cmd("workspace.new", "New Workspace"),
        cmd("pane.split_horizontal", "Split Horizontal"),
        cmd("pane.split_vertical", "Split Vertical"),
        cmd("pane.close", "Close Pane"),
        cmd("workspace.close", "Close Workspace"),
        cmd("pane.zoom_toggle", "Toggle Pane Zoom"),
        cmd("settings.open", "Open Settings"),
        cmd("config.reload", "Reload Ghostty Config"),
        cmd("pane.focus_next", "Focus Next Pane"),
        cmd("pane.focus_prev", "Focus Previous Pane"),
        cmd("pane.focus_left", "Focus Pane Left"),
        cmd("pane.focus_right", "Focus Pane Right"),
        cmd("pane.focus_up", "Focus Pane Up"),
        cmd("pane.focus_down", "Focus Pane Down"),
        cmd("pane.last", "Focus Last Pane"),
        cmd("pane.break", "Break Pane to New Workspace"),
        cmd("pane.join", "Join Pane from Other Workspace"),
        cmd("workspace.next", "Next Workspace"),
        cmd("workspace.previous", "Previous Workspace"),
        cmd("workspace.last", "Last Workspace"),
        cmd("workspace.latest_unread", "Jump to Latest Unread"),
        cmd("workspace.rename", "Rename Workspace"),
        cmd("workspace.pin", "Pin/Unpin Workspace"),
        cmd("sidebar.toggle", "Toggle Sidebar"),
        cmd("workspace.mark_read", "Mark Workspace as Read"),
        cmd("workspace.mark_unread", "Mark Workspace as Unread"),
        cmd("open_folder", "Open Folder in File Manager"),
        cmd("pane.new_browser", "New Browser Panel"),
        cmd("surface.flash", "Flash Panel"),
        cmd("surface.clear_screen", "Clear Terminal"),
        cmd("surface.clear_history", "Clear Scrollback History"),
        cmd("pane.equalize", "Equalize Splits"),
        cmd("tab.close", "Close Tab"),
        cmd("tab.rename", "Rename Tab"),
        cmd("tab.next_in_pane", "Next Tab in Pane"),
        cmd("tab.prev_in_pane", "Previous Tab in Pane"),
        cmd("notifications.toggle", "Show Notifications"),
        cmd("workspace.open_folder", "Open Folder as Workspace..."),
        cmd("terminal.copy_mode", "Enter Copy Mode"),
        cmd("browser.reopen_closed", "Reopen Closed Browser Tab"),
        cmd("markdown.open", "Open Markdown File..."),
        cmd("tab.close_others", "Close Other Tabs in Pane"),
        cmd("pane.split_browser_h", "Split Browser (Horizontal)"),
        cmd("pane.split_browser_v", "Split Browser (Vertical)"),
        cmd("font.increase", "Increase Font Size"),
        cmd("font.decrease", "Decrease Font Size"),
        cmd("font.reset", "Reset Font Size"),
    ];

    // Add "Open in..." commands for installed editors
    for (binary, label) in [
        ("code", "Open in VS Code"),
        ("cursor", "Open in Cursor"),
        ("zed", "Open in Zed"),
        ("ghostty", "Open in Ghostty"),
        ("nvim", "Open in Neovim (terminal)"),
        ("vim", "Open in Vim (terminal)"),
        ("emacs", "Open in Emacs"),
        ("subl", "Open in Sublime Text"),
        ("idea", "Open in IntelliJ IDEA"),
    ] {
        if which_exists(binary) {
            actions.push(PaletteAction {
                name: format!("open_in.{binary}"),
                label: label.into(),
                shortcut: None,
                is_workspace: false,
                is_search_result: false,
            });
        }
    }

    // Add dynamic workspace switcher entries (shown in default mode)
    {
        let tm = lock_or_recover(&state.shared.tab_manager);
        for (i, ws) in tm.iter().enumerate() {
            let shortcut = if i < 9 {
                Some(format!("Ctrl+{}", i + 1))
            } else {
                None
            };
            actions.push(PaletteAction {
                name: format!("workspace.select.{i}"),
                label: format!("{} — {}", ws.display_title(), ws.current_directory),
                shortcut,
                is_workspace: true,
                is_search_result: false,
            });
        }
    }

    // Add surface text search entries (shown only when user types a search query)
    {
        let tm = lock_or_recover(&state.shared.tab_manager);
        for (ws_idx, ws) in tm.iter().enumerate() {
            for (panel_id, panel) in &ws.panels {
                if panel.panel_type != PanelType::Terminal {
                    continue;
                }
                if let Some(surface) = state.terminal_cache.borrow().get(panel_id) {
                    if let Some(text) = surface.read_screen_text() {
                        if !text.trim().is_empty() {
                            let preview = text
                                .lines()
                                .rev()
                                .find(|l| !l.trim().is_empty())
                                .unwrap_or("")
                                .chars()
                                .take(80)
                                .collect::<String>();
                            actions.push(PaletteAction {
                                name: format!("surface.search.{ws_idx}.{panel_id}"),
                                label: format!("{} — {}", ws.display_title(), preview,),
                                shortcut: None,
                                is_workspace: false,
                                is_search_result: true,
                            });
                        }
                    }
                }
            }
        }
    }

    Rc::new(actions)
}

fn populate_list(list_box: &gtk4::ListBox, actions: &[PaletteAction], query: &str) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    // ">" prefix = command mode (show commands), otherwise = workspace switcher mode
    let (command_mode, filter_query) = if let Some(stripped) = query.strip_prefix('>') {
        (true, stripped.trim_start().to_lowercase())
    } else {
        (false, query.to_lowercase())
    };

    let mut first = true;

    for action in actions {
        if command_mode {
            // Command mode: show only commands (not workspace/search entries)
            if action.is_workspace || action.is_search_result {
                continue;
            }
        } else if query.is_empty() {
            // Default mode, no query: show only workspace switcher entries
            if !action.is_workspace {
                continue;
            }
        } else {
            // Default mode with query: show workspaces + search results, skip commands
            if !action.is_workspace && !action.is_search_result {
                continue;
            }
        }

        if !filter_query.is_empty() && !fuzzy_match(&action.label, &filter_query) {
            continue;
        }

        let row = gtk4::ListBoxRow::new();
        row.set_widget_name(&action.name);

        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);

        let label = gtk4::Label::new(Some(&action.label));
        label.set_halign(gtk4::Align::Start);
        label.set_hexpand(true);
        label.set_margin_start(12);
        label.set_margin_end(6);
        label.set_margin_top(6);
        label.set_margin_bottom(6);
        hbox.append(&label);

        if let Some(ref shortcut) = action.shortcut {
            let hint = gtk4::Label::new(Some(shortcut));
            hint.add_css_class("dim-label");
            hint.add_css_class("caption");
            hint.set_halign(gtk4::Align::End);
            hint.set_margin_end(12);
            hint.set_margin_top(6);
            hint.set_margin_bottom(6);
            hbox.append(&hint);
        }

        row.set_child(Some(&hbox));

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

/// Check if a binary is on PATH.
fn which_exists(binary: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
        .unwrap_or(false)
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
            state
                .shared
                .send_ui_event(crate::app::UiEvent::OpenSettings);
            return; // Don't refresh — the settings dialog handles itself
        }
        "config.reload" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::ReloadConfig);
            return;
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
        name @ ("pane.focus_left" | "pane.focus_right" | "pane.focus_up" | "pane.focus_down") => {
            use crate::model::panel::Direction;
            let dir = match name {
                "pane.focus_left" => Direction::Left,
                "pane.focus_right" => Direction::Right,
                "pane.focus_up" => Direction::Up,
                _ => Direction::Down,
            };
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                if let Some(current) = ws.focused_panel_id {
                    if let Some(neighbor) = ws.layout.neighbor(current, dir) {
                        ws.focus_panel(neighbor);
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
        "pane.break" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.selected_mut() {
                if let Some(panel_id) = ws.focused_panel_id {
                    if ws.panels.len() > 1 {
                        let source_dir = ws.current_directory.clone();
                        if let Some(panel) = ws.detach_panel(panel_id) {
                            let source_ws_id = ws.id;
                            if ws.is_empty() {
                                tm.remove_by_id(source_ws_id);
                            }
                            let mut new_ws = Workspace::new();
                            let default_pid = new_ws.focused_panel_id;
                            if let Some(dpid) = default_pid {
                                new_ws.panels.remove(&dpid);
                            }
                            new_ws.current_directory = source_dir;
                            new_ws.panels.insert(panel_id, panel);
                            new_ws.layout = crate::model::panel::LayoutNode::single_pane(panel_id);
                            new_ws.focused_panel_id = Some(panel_id);
                            tm.add_workspace(new_ws);
                        }
                    }
                }
            }
        }
        "pane.join" => {
            // Join is interactive — not practical in palette without a picker.
            // No-op for now; the socket command / CLI covers this.
        }
        "workspace.next" => {
            lock_or_recover(&state.shared.tab_manager).select_next(true);
        }
        "workspace.previous" => {
            lock_or_recover(&state.shared.tab_manager).select_previous(true);
        }
        "workspace.last" => {
            lock_or_recover(&state.shared.tab_manager).select_last();
        }
        "workspace.latest_unread" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.select_latest_unread();
        }
        "workspace.rename" => {
            // Can't show dialog from here easily — use the keyboard shortcut instead.
            // Trigger via UiEvent would be needed; skip for palette.
        }
        "workspace.pin" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.is_pinned = !ws.is_pinned;
            }
        }
        "sidebar.toggle" => {
            // We can't access the NavigationSplitView from here.
            // The keyboard shortcut (Ctrl+Shift+B) handles this.
        }
        "workspace.mark_read" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.mark_notifications_read();
            }
        }
        "workspace.mark_unread" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.unread_count = ws.unread_count.max(1);
            }
        }
        "open_folder" => {
            let dir = lock_or_recover(&state.shared.tab_manager)
                .selected()
                .map(|ws| ws.current_directory.clone());
            if let Some(dir) = dir {
                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
            }
            return; // Don't refresh — external command
        }
        #[cfg(feature = "webkit")]
        "pane.new_browser" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.split(SplitOrientation::Vertical, PanelType::Browser);
            }
        }
        name if name.starts_with("open_in.") => {
            const ALLOWED_EDITORS: &[&str] = &[
                "code", "cursor", "zed", "ghostty", "nvim", "vim", "emacs",
                "subl", "idea",
            ];
            let binary = &name[8..];
            if !ALLOWED_EDITORS.contains(&binary) {
                tracing::warn!(binary, "open_in: blocked unknown binary");
                return;
            }
            let dir = lock_or_recover(&state.shared.tab_manager)
                .selected()
                .map(|ws| ws.current_directory.clone());
            if let Some(dir) = dir {
                let _ = std::process::Command::new(binary).arg(&dir).spawn();
            }
            return; // Don't refresh — external command
        }
        "surface.flash" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected() {
                if let Some(panel_id) = ws.focused_panel_id {
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::TriggerFlash { panel_id });
                }
            }
            return; // UiEvent handled
        }
        "surface.clear_screen" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected() {
                if let Some(panel_id) = ws.focused_panel_id {
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::ClearHistory { panel_id });
                }
            }
            return;
        }
        "surface.clear_history" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected() {
                if let Some(panel_id) = ws.focused_panel_id {
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::ClearHistory { panel_id });
                }
            }
            return;
        }
        "pane.equalize" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.layout.equalize();
            }
        }
        "tab.close" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.selected_mut() {
                if let Some(panel_id) = ws.focused_panel_id {
                    ws.remove_panel(panel_id);
                }
            }
        }
        "tab.rename" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected() {
                if let Some(panel_id) = ws.focused_panel_id {
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::RenameTab { panel_id });
                }
            }
            return; // UiEvent handled
        }
        "tab.next_in_pane" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                if let Some(current) = ws.focused_panel_id {
                    if let Some(next) = ws.layout.next_panel_in_pane(current) {
                        ws.focus_panel(next);
                    }
                }
            }
        }
        "tab.prev_in_pane" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                if let Some(current) = ws.focused_panel_id {
                    if let Some(prev) = ws.layout.prev_panel_in_pane(current) {
                        ws.focus_panel(prev);
                    }
                }
            }
        }
        "notifications.toggle" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::ToggleNotifications);
            return; // UiEvent handled
        }
        "workspace.open_folder" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::OpenFolderAsWorkspace);
            return; // UiEvent handled
        }
        "terminal.copy_mode" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected() {
                if let Some(panel_id) = ws.focused_panel_id {
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::CopyMode { panel_id });
                }
            }
            return; // UiEvent handled
        }
        "browser.reopen_closed" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::ReopenClosedBrowser);
            return; // UiEvent handled
        }
        "markdown.open" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::OpenMarkdownFile);
            return; // UiEvent handled
        }
        "tab.close_others" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.selected_mut() {
                if let Some(panel_id) = ws.focused_panel_id {
                    if let Some(pane_ids) = ws.layout.find_pane_with_panel_readonly(panel_id) {
                        let to_close: Vec<uuid::Uuid> = pane_ids
                            .iter()
                            .filter(|&&pid| pid != panel_id)
                            .copied()
                            .collect();
                        for pid in &to_close {
                            ws.panels.remove(pid);
                            ws.layout.remove_panel(*pid);
                        }
                    }
                }
            }
        }
        #[cfg(feature = "webkit")]
        "pane.split_browser_h" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.split(SplitOrientation::Horizontal, PanelType::Browser);
            }
        }
        #[cfg(feature = "webkit")]
        "pane.split_browser_v" => {
            if let Some(ws) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                ws.split(SplitOrientation::Vertical, PanelType::Browser);
            }
        }
        "font.increase" => {
            let info = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.selected().and_then(|ws| {
                    ws.focused_panel_id
                        .and_then(|pid| ws.panels.get(&pid).map(|p| (pid, p.panel_type)))
                })
            };
            if let Some((panel_id, panel_type)) = info {
                if panel_type == PanelType::Browser {
                    #[cfg(feature = "webkit")]
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::BrowserAction {
                            panel_id,
                            action: crate::ui::browser_panel::BrowserActionKind::ZoomIn,
                        });
                } else if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                    surface.binding_action("increase_font_size:1");
                }
            }
            return;
        }
        "font.decrease" => {
            let info = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.selected().and_then(|ws| {
                    ws.focused_panel_id
                        .and_then(|pid| ws.panels.get(&pid).map(|p| (pid, p.panel_type)))
                })
            };
            if let Some((panel_id, panel_type)) = info {
                if panel_type == PanelType::Browser {
                    #[cfg(feature = "webkit")]
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::BrowserAction {
                            panel_id,
                            action: crate::ui::browser_panel::BrowserActionKind::ZoomOut,
                        });
                } else if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                    surface.binding_action("decrease_font_size:1");
                }
            }
            return;
        }
        "font.reset" => {
            let info = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.selected().and_then(|ws| {
                    ws.focused_panel_id
                        .and_then(|pid| ws.panels.get(&pid).map(|p| (pid, p.panel_type)))
                })
            };
            if let Some((panel_id, panel_type)) = info {
                if panel_type == PanelType::Browser {
                    #[cfg(feature = "webkit")]
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::BrowserAction {
                            panel_id,
                            action: crate::ui::browser_panel::BrowserActionKind::SetZoom {
                                zoom: 1.0,
                            },
                        });
                } else if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                    surface.binding_action("reset_font_size");
                }
            }
            return;
        }
        name if name.starts_with("workspace.select.") => {
            if let Ok(index) = name[17..].parse::<usize>() {
                lock_or_recover(&state.shared.tab_manager).select(index);
            }
        }
        name if name.starts_with("surface.search.") => {
            // Format: surface.search.<ws_idx>.<panel_id>
            let parts: Vec<&str> = name.splitn(4, '.').collect();
            if parts.len() == 4 {
                if let Ok(ws_idx) = parts[2].parse::<usize>() {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    tm.select(ws_idx);
                    if let Ok(panel_id) = uuid::Uuid::parse_str(parts[3]) {
                        if let Some(ws) = tm.selected_mut() {
                            ws.focus_panel(panel_id);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    on_refresh();
}
