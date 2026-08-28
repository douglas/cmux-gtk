//! Command palette — modal dialog with fuzzy-filtered action list.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::app::{lock_or_recover, AppState};
use crate::settings::custom_commands::CommandEntry;
use crate::model::panel::SplitOrientation;
use crate::model::{PanelType, Workspace};

/// A registered command palette action.
#[derive(Default)]
struct PaletteAction {
    name: String,
    label: String,
    /// Keyboard shortcut hint (e.g., "Ctrl+Shift+T").
    shortcut: Option<String>,
    /// Whether this is a workspace switcher entry (shown in default mode).
    is_workspace: bool,
    /// Whether this is a surface text search result (only shown when query is non-empty).
    is_search_result: bool,
    /// Extra terms (custom-command keywords) matched in search but not displayed.
    search_terms: String,
}

/// Show the command palette as a modal dialog.
pub fn show_command_palette(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    on_refresh: Rc<dyn Fn()>,
) {
    // In-surface dialog (adw::Dialog) rather than a separate top-level window,
    // so it renders above the layer-shell quick-terminal overlay too.
    let dialog = adw::Dialog::new();
    dialog.add_css_class("command-palette");
    dialog.set_content_width(480);
    dialog.set_content_height(400);

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

    dialog.present(Some(window));
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
        "notification.defer_unread" => "notification.defer_unread",
        "notification.toggle_unread" => "notification.toggle_unread",
        "font.increase" => "font.increase",
        "font.decrease" => "font.decrease",
        "font.reset" => "font.reset",
        "surface.clear_screen" | "surface.clear_history" => "surface.clear",
        "tab.close_others" => "tab.close_others",
        "pane.split_browser_h" => "browser.split_horizontal",
        "pane.split_browser_v" => "browser.split_vertical",
        "task_manager.open" => return Some("Ctrl+Shift+A".to_string()),
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
            search_terms: String::new(),
        }
    };

    let mut actions = vec![
        cmd("workspace.new", "New Workspace"),
        cmd("workspace.reopen_closed", "Reopen Closed Workspace"),
        cmd("workspace.new_browser", "New Browser Workspace"),
        cmd("workspace.new_diff", "New Diff Workspace"),
        cmd("workspace.new_project", "New Project Visualizer"),
        cmd("workspace.new_notes", "Open Notes Scratchpad"),
        cmd("workspace.open_history", "Open History"),
        cmd("workspace.open_vault", "Open Vault"),
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
        cmd("workspace.focus_back", "Back (Recently Focused)"),
        cmd("workspace.focus_forward", "Forward (Recently Focused)"),
        cmd("workspace.hibernate", "Hibernate Agent (toggle)"),
        cmd("workspace.latest_unread", "Jump to Latest Unread"),
        cmd("workspace.rename", "Rename Workspace"),
        cmd("workspace.pin", "Pin/Unpin Workspace"),
        cmd("sidebar.toggle", "Toggle Sidebar"),
        cmd("dock.toggle", "Toggle Dock"),
        cmd("overview.open", "Pane Overview"),
        cmd("workspace.mark_read", "Mark Workspace as Read"),
        cmd("workspace.mark_unread", "Mark Workspace as Unread"),
        cmd("open_folder", "Open Folder in File Manager"),
        cmd("pane.new_browser", "New Browser Panel"),
        cmd("surface.flash", "Flash Panel"),
        cmd("surface.clear_screen", "Clear Terminal"),
        cmd("surface.clear_history", "Clear Scrollback History"),
        cmd("pane.equalize", "Equalize Splits"),
        cmd("tab.close", "Close Tab"),
        cmd("tab.reopen", "Reopen Closed Tab"),
        cmd("tab.rename", "Rename Tab"),
        cmd("tab.next_in_pane", "Next Tab in Pane"),
        cmd("tab.prev_in_pane", "Previous Tab in Pane"),
        cmd("notifications.toggle", "Show Notifications"),
        cmd("notification.defer_unread", "Defer Unread Notifications"),
        cmd("notification.toggle_unread", "Toggle Unread for Workspace"),
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
        cmd("task_manager.open", "Open Task Manager"),
        cmd("settings.ghostty", "Ghostty Settings"),
        cmd("settings.jmux", "jmux Settings"),
        cmd("goal.run", "Run Goal..."),
    ];

    // Add SSH workspace command if enabled in settings
    if crate::settings::load().remote_ssh_enabled {
        actions.push(PaletteAction {
            name: "workspace.new_ssh".into(),
            label: "New SSH Workspace...".into(),
            shortcut: None,
            is_workspace: false,
            is_search_result: false,
            search_terms: String::new(),
        });
    }

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
                search_terms: String::new(),
            });
        }
    }

    // Add dynamic workspace switcher entries (shown in default mode).
    // When multiple windows are open, annotate each entry with "(Window N)".
    {
        let tm = lock_or_recover(&state.shared.tab_manager);
        let window_count = state.shared.window_ids().len();

        // Build a stable window-index map: sort window IDs so the numbering is
        // deterministic across rebuilds (window_ids() returns an unordered Vec).
        let mut sorted_windows = state.shared.window_ids();
        sorted_windows.sort();

        for (i, ws) in tm.iter().enumerate() {
            let shortcut = if i < 9 {
                Some(format!("Ctrl+{}", i + 1))
            } else {
                None
            };

            // Append "(Window N)" suffix when more than one window is open.
            let window_suffix = if window_count > 1 {
                let win_idx = ws
                    .window_id
                    .and_then(|wid| sorted_windows.iter().position(|w| *w == wid))
                    .map(|idx| idx + 1)
                    .unwrap_or(1);
                format!(" (Window {win_idx})")
            } else {
                String::new()
            };

            actions.push(PaletteAction {
                name: format!("workspace.select.{i}"),
                label: format!(
                    "{}{} — {}",
                    ws.display_title(),
                    window_suffix,
                    ws.current_directory
                ),
                shortcut,
                is_workspace: true,
                is_search_result: false,
                search_terms: String::new(),
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
                                search_terms: String::new(),
                            });
                        }
                    }
                }
            }
        }
    }

    // Load custom commands from jmux.json (project .jmux/, project root, global).
    let workspace_dir = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.selected()
            .map(|ws| ws.current_directory.clone())
            .unwrap_or_default()
    };
    for entry in crate::settings::custom_commands::load(&workspace_dir) {
        // Dispatch by name (re-resolved on execute, so confirm/workspace work).
        actions.push(PaletteAction {
            name: format!("custom:{}", entry.name),
            label: format!("\u{25b6} {}", entry.name),
            shortcut: None,
            is_workspace: false,
            is_search_result: false,
            search_terms: entry.keywords.join(" "),
        });
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

        if !filter_query.is_empty() {
            let label_match = fuzzy_match(&action.label, &filter_query);
            let terms_match = !action.search_terms.is_empty()
                && fuzzy_match(&action.search_terms, &filter_query);
            if !label_match && !terms_match {
                continue;
            }
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

/// Open a file path in the user's preferred editor.
///
/// Uses `$EDITOR` if set; falls back to `xdg-open` so the desktop chooses an
/// appropriate application.
fn open_in_editor(path: &std::path::Path) {
    let editor = std::env::var("EDITOR").ok().filter(|e| !e.is_empty());
    if let Some(editor) = editor {
        let _ = std::process::Command::new(&editor).arg(path).spawn();
    } else {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// The current application window (for parenting dialogs).
fn active_app_window() -> Option<adw::ApplicationWindow> {
    gtk4::gio::Application::default()
        .and_then(|a| a.downcast::<adw::Application>().ok())
        .and_then(|a| a.active_window())
        .and_then(|w| w.downcast::<adw::ApplicationWindow>().ok())
}

/// Run a custom command from jmux.json — optionally behind a confirmation
/// dialog, then either run the shell command or open the workspace layout.
fn run_custom_command(
    entry: &CommandEntry,
    base_dir: &str,
    state: &Rc<AppState>,
    on_refresh: &Rc<dyn Fn()>,
) {
    if entry.confirm {
        let body = entry
            .description
            .clone()
            .unwrap_or_else(|| format!("Run \u{201c}{}\u{201d}?", entry.name));
        let dialog = adw::AlertDialog::new(Some("Run command?"), Some(&body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("run", "Run");
        dialog.set_default_response(Some("run"));
        dialog.set_close_response("cancel");
        dialog.set_response_appearance("run", adw::ResponseAppearance::Suggested);
        let entry = entry.clone();
        let base_dir = base_dir.to_string();
        let state = state.clone();
        let on_refresh = on_refresh.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "run" {
                do_custom_command(&entry, &base_dir, &state);
                on_refresh();
            }
        });
        dialog.present(active_app_window().as_ref());
        return;
    }
    do_custom_command(entry, base_dir, state);
    on_refresh();
}

/// Run a custom command (from jmux.json) by name, without the confirm dialog.
/// Used by `jmux run <name>` / `system.run_command`.
pub fn run_custom_command_by_name(name: &str, state: &Rc<AppState>) {
    let dir = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.selected()
            .map(|ws| ws.current_directory.clone())
            .unwrap_or_default()
    };
    if let Some(entry) = crate::settings::custom_commands::load(&dir)
        .into_iter()
        .find(|e| e.name == name)
    {
        do_custom_command(&entry, &dir, state);
    }
}

fn do_custom_command(entry: &CommandEntry, base_dir: &str, state: &Rc<AppState>) {
    if let Some(spec) = &entry.workspace {
        let mut ws = crate::settings::custom_commands::build_workspace(spec, base_dir);
        let ws_id = ws.id;
        let mut tm = lock_or_recover(&state.shared.tab_manager);
        ws.window_id = tm.selected().and_then(|w| w.window_id);
        tm.add_workspace(ws);
        tm.select_by_id(ws_id);
    } else if let Some(command) = &entry.command {
        let panel_id = {
            let tm = lock_or_recover(&state.shared.tab_manager);
            tm.selected().and_then(|w| w.focused_panel_id)
        };
        if let Some(panel_id) = panel_id {
            if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                surface.send_text(&format!("{command}\n"));
            }
        }
    }
}

/// Toggle the scope-grouped Notes panel in the current workspace: if one is
/// already open, close it; otherwise open it as a split beside the current
/// pane. Shared by the command palette, the header-bar Notes button, and the
/// `notes.open` keyboard shortcut. Callers are responsible for refreshing the
/// UI.
pub fn insert_notes_panel(state: &Rc<AppState>) {
    let mut tm = lock_or_recover(&state.shared.tab_manager);
    if let Some(ws) = tm.selected_mut() {
        // Toggle off: close any existing Notes panel(s) rather than stacking
        // another. Collecting all also cleans up duplicates from before this
        // was a toggle.
        let existing: Vec<_> = ws
            .panels
            .iter()
            .filter(|(_, p)| p.panel_type == crate::model::PanelType::Notes)
            .map(|(id, _)| *id)
            .collect();
        if !existing.is_empty() {
            for id in existing {
                ws.remove_panel(id);
            }
            return;
        }
        let panel =
            crate::model::Panel::new_notes(&crate::ui::notes_panel::default_notes_path());
        ws.insert_panel(panel, SplitOrientation::Horizontal);
    }
}

fn execute_action(name: &str, state: &Rc<AppState>, on_refresh: &Rc<dyn Fn()>) {
    match name {
        "workspace.new" => {
            lock_or_recover(&state.shared.tab_manager).add_workspace(Workspace::new());
        }
        "workspace.reopen_closed" => {
            lock_or_recover(&state.shared.tab_manager).reopen_last_closed();
        }
        "workspace.new_browser" => {
            let mut ws = Workspace::new();
            let pid = ws
                .focused_panel_id
                .or_else(|| ws.panels.keys().next().copied());
            if let Some(panel) = pid.and_then(|pid| ws.panels.get_mut(&pid)) {
                panel.panel_type = PanelType::Browser;
                panel.command = None;
            }
            lock_or_recover(&state.shared.tab_manager).add_workspace(ws);
        }
        "workspace.new_diff" => {
            let mut ws = Workspace::new();
            let dir = ws.current_directory.clone();
            let pid = ws
                .focused_panel_id
                .or_else(|| ws.panels.keys().next().copied());
            if let Some(panel) = pid.and_then(|pid| ws.panels.get_mut(&pid)) {
                panel.panel_type = PanelType::Diff;
                panel.command = None;
                panel.directory = Some(dir);
                panel.title = Some("Diff".to_string());
            }
            lock_or_recover(&state.shared.tab_manager).add_workspace(ws);
        }
        "workspace.new_project" => {
            let mut ws = Workspace::new();
            let dir = ws.current_directory.clone();
            let pid = ws
                .focused_panel_id
                .or_else(|| ws.panels.keys().next().copied());
            if let Some(panel) = pid.and_then(|pid| ws.panels.get_mut(&pid)) {
                panel.panel_type = PanelType::Project;
                panel.command = None;
                panel.directory = Some(dir);
                panel.title = Some("Project".to_string());
            }
            lock_or_recover(&state.shared.tab_manager).add_workspace(ws);
        }
        "workspace.new_notes" => {
            // Open notes as a split beside the current pane (consistent with open).
            insert_notes_panel(state);
        }
        "workspace.open_history" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.selected_mut() {
                ws.insert_panel(crate::model::Panel::new_history(), SplitOrientation::Horizontal);
            }
        }
        "workspace.open_vault" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.selected_mut() {
                ws.insert_panel(crate::model::Panel::new_vault(), SplitOrientation::Horizontal);
            }
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
            if let Some(panel_id) = tm.selected().and_then(|ws| ws.focused_panel_id) {
                tm.close_panel(panel_id);
                crate::ui::window::request_terminal_focus();
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
        "workspace.focus_back" => {
            lock_or_recover(&state.shared.tab_manager).focus_back();
        }
        "workspace.focus_forward" => {
            lock_or_recover(&state.shared.tab_manager).focus_forward();
        }
        "workspace.hibernate" => {
            let pid = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.selected().and_then(|ws| ws.focused_panel_id)
            };
            if let Some(pid) = pid {
                if state.shared.is_hibernated(&pid) {
                    state.shared.wake_panel(pid);
                } else {
                    state.shared.hibernate_panel(pid);
                }
            }
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
                "code", "cursor", "zed", "ghostty", "nvim", "vim", "emacs", "subl", "idea",
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
            if let Some(panel_id) = tm.selected().and_then(|ws| ws.focused_panel_id) {
                tm.close_panel(panel_id);
                crate::ui::window::request_terminal_focus();
            }
        }
        "tab.reopen" => {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.selected_mut() {
                ws.reopen_last_closed_panel();
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
        "notification.defer_unread" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::DeferUnread);
            return; // UiEvent handled
        }
        "notification.toggle_unread" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::ToggleUnread);
            return; // UiEvent handled
        }
        "workspace.open_folder" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::OpenFolderAsWorkspace);
            return; // UiEvent handled
        }
        "goal.run" => {
            // Same route as the SSH dialog: the palette closes itself first,
            // then the window opens the dialog off the UI event.
            state
                .shared
                .send_ui_event(crate::app::UiEvent::OpenGoalDialog { prefill: None });
        }
        "workspace.new_ssh" => {
            // Handled by the command palette UI layer — dispatch via custom event.
            // The palette dialog will call show_ssh_dialog after closing itself.
            state
                .shared
                .send_ui_event(crate::app::UiEvent::OpenSshDialog);
            return;
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
        "task_manager.open" => {
            state
                .shared
                .send_ui_event(crate::app::UiEvent::OpenTaskManager);
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
        "settings.ghostty" => {
            let path = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("~"))
                .join(".config/ghostty/config");
            open_in_editor(&path);
            return; // Don't refresh — external command
        }
        "settings.jmux" => {
            let path = crate::settings::active_config_path();
            open_in_editor(&path);
            return; // Don't refresh — external command
        }
        name if name.starts_with("workspace.select.") => {
            if let Ok(index) = name[17..].parse::<usize>() {
                lock_or_recover(&state.shared.tab_manager).select(index);
            }
        }
        "overview.open" => {
            if let Some(win) = active_app_window() {
                crate::ui::pane_overview::show_pane_overview(&win, state);
            }
        }
        "dock.toggle" => {
            if let Some(win) = active_app_window() {
                if let Ok(wid) = uuid::Uuid::parse_str(&win.widget_name()) {
                    let dir = {
                        let tm = lock_or_recover(&state.shared.tab_manager);
                        tm.selected()
                            .map(|ws| ws.current_directory.clone())
                            .unwrap_or_default()
                    };
                    crate::ui::dock::toggle(wid, &dir, state);
                }
            }
        }
        name if name.starts_with("custom:") => {
            let cmd_name = &name["custom:".len()..];
            // Re-resolve from jmux.json so confirm/workspace are honored.
            let workspace_dir = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.selected()
                    .map(|ws| ws.current_directory.clone())
                    .unwrap_or_default()
            };
            let Some(entry) = crate::settings::custom_commands::load(&workspace_dir)
                .into_iter()
                .find(|e| e.name == cmd_name)
            else {
                return;
            };
            run_custom_command(&entry, &workspace_dir, state, on_refresh);
            return;
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
