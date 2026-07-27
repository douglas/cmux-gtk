//! Settings — AdwPreferencesDialog (in-surface) for application configuration.

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::settings::{
    self, AppSettings, BrowserSettings, NewWorkspacePlacement, NotificationSound, PlusButtonAction,
    SearchEngine, SidebarDisplaySettings, SidebarFocusStyle, SocketAccess, ThemeMode,
};

/// Friendly, categorised catalog of keyboard-shortcut actions for the Keyboard
/// page: `(category, [(action_id, label)])`. Actions present in the config but
/// missing here are still shown under an "Other" group, so nothing is lost.
fn shortcut_catalog() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            "Tabs & Panes",
            vec![
                ("tab.new", "New tab"),
                ("tab.reopen", "Reopen closed tab"),
                ("close.tab", "Close tab"),
                ("close.tab.others", "Close other tabs"),
                ("tab.close_others", "Close other tabs"),
                ("pane.split_horizontal", "Split pane horizontally"),
                ("pane.split_vertical", "Split pane vertically"),
                ("pane.close", "Close pane"),
                ("pane.rename", "Rename pane"),
                ("pane.focus_left", "Focus pane left"),
                ("pane.focus_right", "Focus pane right"),
                ("pane.focus_up", "Focus pane up"),
                ("pane.focus_down", "Focus pane down"),
                ("pane.focus_prev", "Focus previous pane"),
                ("pane.focus_next", "Focus next pane"),
                ("overview.open", "Open pane overview"),
            ],
        ),
        (
            "Workspaces",
            vec![
                ("workspace.new", "New workspace"),
                ("workspace.close", "Close workspace"),
                ("workspace.rename", "Rename workspace"),
                ("workspace.latest_unread", "Go to latest unread"),
                ("workspace.move_up", "Move workspace up"),
                ("workspace.move_down", "Move workspace down"),
            ],
        ),
        (
            "Panels",
            vec![
                ("notes.open", "Open Notes"),
                ("dock.toggle", "Toggle Dock"),
                ("textbox.focus", "Focus TextBox composer"),
                ("agent.resume", "Resume agent session"),
                ("browser.split_horizontal", "Open browser (split right)"),
                ("browser.split_vertical", "Open browser (split down)"),
                ("browser.console_toggle", "Toggle browser console"),
            ],
        ),
        (
            "Find",
            vec![
                ("find", "Find"),
                ("find.next", "Find next"),
                ("find.previous", "Find previous"),
                ("find.use_selection", "Use selection for find"),
                ("find.in_directory", "Find in directory"),
            ],
        ),
        (
            "Terminal & View",
            vec![
                ("surface.clear", "Clear terminal"),
                ("font.increase", "Increase font size"),
                ("font.decrease", "Decrease font size"),
                ("font.reset", "Reset font size"),
            ],
        ),
        (
            "Notifications",
            vec![
                ("notifications.toggle", "Toggle notifications panel"),
                ("notification.defer_unread", "Defer unread"),
                ("notification.toggle_unread", "Toggle unread"),
            ],
        ),
        (
            "Application",
            vec![
                ("settings", "Open Settings"),
                ("config.reload", "Reload config"),
            ],
        ),
    ]
}

/// Create and show the settings preferences window.
/// `on_close` is called after settings are saved so callers can refresh the UI.
/// `shared` lets interactive controls (e.g. the ghostty theme picker) dispatch
/// UI events such as a ghostty config reload.
pub fn show_settings(
    parent: &adw::ApplicationWindow,
    shared: std::sync::Arc<crate::app::SharedState>,
    on_close: impl Fn() + 'static,
) {
    let current_settings = settings::load();

    // In-surface adw::PreferencesDialog (not a top-level window) so it renders
    // above the content — including the layer-shell quake drop-down.
    let window = adw::PreferencesDialog::new();
    window.set_title("Settings");
    // Searchable: type in the dialog's search to filter rows by title/subtitle
    // across every page — the main fix for "hard to find a setting".
    window.set_search_enabled(true);

    // ── Appearance page ──
    let appearance_page = adw::PreferencesPage::new();
    appearance_page.set_title("Appearance");
    appearance_page.set_icon_name(Some("preferences-desktop-appearance-symbolic"));

    // Split the formerly-overloaded Appearance page into focused pages so
    // features are easier to find (alongside the dialog's new search).
    let terminal_page = adw::PreferencesPage::new();
    terminal_page.set_title("Terminal");
    terminal_page.set_icon_name(Some("utilities-terminal-symbolic"));

    let workspace_page = adw::PreferencesPage::new();
    workspace_page.set_title("Workspace");
    workspace_page.set_icon_name(Some("preferences-system-windows-symbolic"));

    let editor_page = adw::PreferencesPage::new();
    editor_page.set_title("Editor & Files");
    editor_page.set_icon_name(Some("accessories-text-editor-symbolic"));

    let theme_group = adw::PreferencesGroup::new();
    theme_group.set_title("Theme");

    let theme_row = adw::ComboRow::new();
    theme_row.set_title("Color Scheme");
    theme_row.set_subtitle("Choose the application color scheme");
    let on_omarchy = settings::is_omarchy();
    let theme_list = if on_omarchy {
        gtk4::StringList::new(&["System", "Light", "Dark", "Omarchy"])
    } else {
        gtk4::StringList::new(&["System", "Light", "Dark"])
    };
    theme_row.set_model(Some(&theme_list));
    theme_row.set_selected(match current_settings.theme {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
        ThemeMode::Omarchy => {
            if on_omarchy {
                3
            } else {
                0
            }
        }
    });
    theme_group.add(&theme_row);

    let tab_bar_font_size_row = adw::EntryRow::new();
    tab_bar_font_size_row.set_title("Tab Bar Font Size");
    tab_bar_font_size_row.set_tooltip_text(Some(
        "Font size in pixels for pane tab bar labels. Leave empty or 0 for system default.",
    ));
    let font_size_initial = if current_settings.tab_bar_font_size > 0.0 {
        current_settings.tab_bar_font_size.to_string()
    } else {
        String::new()
    };
    tab_bar_font_size_row.set_text(&font_size_initial);
    theme_group.add(&tab_bar_font_size_row);

    let sidebar_font_size_row = adw::EntryRow::new();
    sidebar_font_size_row.set_title("Sidebar Font Size");
    sidebar_font_size_row.set_tooltip_text(Some(
        "Font size in pixels for the sidebar workspace list. Leave empty or 0 for system default.",
    ));
    let sidebar_font_size_initial = if current_settings.sidebar_font_size > 0.0 {
        current_settings.sidebar_font_size.to_string()
    } else {
        String::new()
    };
    sidebar_font_size_row.set_text(&sidebar_font_size_initial);
    theme_group.add(&sidebar_font_size_row);

    appearance_page.add(&theme_group);

    // ── Ghostty Terminal Themes group ──
    // A live picker over the themes available in the user's ghostty themes
    // directory and the system themes directory. Choosing one writes
    // `theme = <name>` into ~/.config/ghostty/config and reloads ghostty so the
    // embedded terminal updates immediately.
    {
        let ghostty_theme_group = adw::PreferencesGroup::new();
        ghostty_theme_group.set_title("Ghostty Terminal Themes");
        ghostty_theme_group.set_description(Some(
            "Sets `theme = Name` in ~/.config/ghostty/config and reloads the terminal.",
        ));

        // Merge user + system themes into one sorted, de-duplicated list.
        let mut themes = discover_ghostty_user_themes();
        themes.extend(discover_ghostty_system_themes());
        themes.sort();
        themes.dedup();

        if themes.is_empty() {
            let empty_row = adw::ActionRow::new();
            empty_row.set_title("No themes found");
            empty_row.set_subtitle("Install ghostty themes or add them to ~/.config/ghostty/themes/");
            ghostty_theme_group.add(&empty_row);
        } else {
            let theme_row = adw::ComboRow::new();
            theme_row.set_title("Terminal Theme");

            // If the config's current theme isn't a plain name we recognize
            // (e.g. a `dark:X|light:Y` conditional, or a theme not on disk),
            // surface it as the first entry so opening settings never silently
            // rewrites it.
            let current = read_ghostty_config_theme();
            let mut labels: Vec<String> = themes.clone();
            let mut selected = 0u32;
            match current
                .as_ref()
                .and_then(|c| themes.iter().position(|t| t == c))
            {
                Some(idx) => selected = idx as u32,
                None => {
                    if let Some(cur) = current.clone() {
                        labels.insert(0, format!("{cur}  (current)"));
                        themes.insert(0, cur);
                        selected = 0;
                    }
                }
            }

            let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            theme_row.set_model(Some(&gtk4::StringList::new(&label_refs)));
            theme_row.set_selected(selected);

            // Connect AFTER setting the initial selection so opening the dialog
            // doesn't spuriously rewrite the config.
            let themes_for_cb = themes.clone();
            let shared_for_cb = shared.clone();
            theme_row.connect_selected_notify(move |row| {
                let idx = row.selected() as usize;
                let Some(name) = themes_for_cb.get(idx) else {
                    return;
                };
                if let Err(e) = set_ghostty_config_theme(name) {
                    tracing::warn!(error = %e, theme = name, "Failed to write ghostty theme");
                    return;
                }
                tracing::info!(theme = name, "Set ghostty theme; reloading config");
                shared_for_cb.send_ui_event(crate::app::UiEvent::ReloadConfig);
            });

            ghostty_theme_group.add(&theme_row);
        }

        terminal_page.add(&ghostty_theme_group);
    }

    // ── Behavior group ──
    let behavior_group = adw::PreferencesGroup::new();
    behavior_group.set_title("Behavior");

    let focus_hover_row = adw::SwitchRow::new();
    focus_hover_row.set_title("Focus Follows Mouse");
    focus_hover_row.set_subtitle("Automatically focus terminal panes on mouse hover");
    focus_hover_row.set_active(current_settings.focus_follows_mouse);
    behavior_group.add(&focus_hover_row);

    let first_click_row = adw::SwitchRow::new();
    first_click_row.set_title("First Click Focus Only");
    first_click_row.set_subtitle(
        "Clicking an unfocused pane only focuses it, without passing the click to the terminal",
    );
    first_click_row.set_active(current_settings.first_click_focus);
    behavior_group.add(&first_click_row);

    let confirm_close_row = adw::SwitchRow::new();
    confirm_close_row.set_title("Confirm Before Close");
    confirm_close_row.set_subtitle("Show confirmation when quitting with active terminals");
    confirm_close_row.set_active(current_settings.confirm_before_close);
    behavior_group.add(&confirm_close_row);

    let confirm_quit_row = adw::SwitchRow::new();
    confirm_quit_row.set_title("Confirm Quit");
    confirm_quit_row.set_subtitle("Show confirmation dialog before quitting the app");
    confirm_quit_row.set_active(current_settings.confirm_quit);
    behavior_group.add(&confirm_quit_row);

    let show_tab_close_row = adw::SwitchRow::new();
    show_tab_close_row.set_title("Show Tab Close Button");
    show_tab_close_row
        .set_subtitle("Show the X on tabs; when off, close via middle-click or right-click menu");
    show_tab_close_row.set_active(current_settings.show_tab_close_button);
    behavior_group.add(&show_tab_close_row);

    let placement_row = adw::ComboRow::new();
    placement_row.set_title("New Workspace Placement");
    placement_row.set_subtitle("Where to insert newly created workspaces");
    let placement_labels: Vec<&str> = NewWorkspacePlacement::ALL
        .iter()
        .map(|p| p.label())
        .collect();
    let placement_list = gtk4::StringList::new(&placement_labels);
    placement_row.set_model(Some(&placement_list));
    placement_row.set_selected(current_settings.new_workspace_placement.to_index());
    behavior_group.add(&placement_row);

    let attention_ring_row = adw::SwitchRow::new();
    attention_ring_row.set_title("Pane Attention Ring");
    attention_ring_row.set_subtitle("Highlight panes that receive output while unfocused");
    attention_ring_row.set_active(current_settings.pane_attention_ring);
    behavior_group.add(&attention_ring_row);

    let flash_row = adw::SwitchRow::new();
    flash_row.set_title("Pane Flash Effect");
    flash_row.set_subtitle("Flash animation when manually triggering pane highlight");
    flash_row.set_active(current_settings.pane_flash_enabled);
    behavior_group.add(&flash_row);

    let auto_subagent_row = adw::SwitchRow::new();
    auto_subagent_row.set_title("Auto Sub-agent Monitor");
    auto_subagent_row.set_subtitle(
        "Automatically open the read-only sub-agent panes while Claude runs sub-agents, and close them when finished",
    );
    auto_subagent_row.set_active(current_settings.auto_subagent_monitor);
    behavior_group.add(&auto_subagent_row);

    let remote_ssh_row = adw::SwitchRow::new();
    remote_ssh_row.set_title("Remote SSH Workspaces");
    remote_ssh_row
        .set_subtitle("Enable SSH workspace connections (bootstraps daemon on remote host)");
    remote_ssh_row.set_active(current_settings.remote_ssh_enabled);
    behavior_group.add(&remote_ssh_row);

    let relay_ports_start_row = adw::SpinRow::new(
        Some(&gtk4::Adjustment::new(
            current_settings.remote_relay_ports.start as f64,
            1024.0,
            65535.0,
            1.0,
            10.0,
            0.0,
        )),
        1.0,
        0,
    );
    relay_ports_start_row.set_title("Relay Port Range Start");
    relay_ports_start_row
        .set_subtitle("First port scanned on the remote host for the jmux CLI relay tunnel");
    behavior_group.add(&relay_ports_start_row);

    let relay_ports_end_row = adw::SpinRow::new(
        Some(&gtk4::Adjustment::new(
            current_settings.remote_relay_ports.end as f64,
            1024.0,
            65535.0,
            1.0,
            10.0,
            0.0,
        )),
        1.0,
        0,
    );
    relay_ports_end_row.set_title("Relay Port Range End");
    relay_ports_end_row
        .set_subtitle("Last port scanned on the remote host for the jmux CLI relay tunnel");
    behavior_group.add(&relay_ports_end_row);

    workspace_page.add(&behavior_group);

    // ── Sidebar display group ──
    let sidebar_group = adw::PreferencesGroup::new();
    sidebar_group.set_title("Sidebar Display");
    sidebar_group.set_description(Some("Choose which metadata to show in workspace rows"));

    let hide_all_row = adw::SwitchRow::new();
    hide_all_row.set_title("Hide All Details");
    hide_all_row.set_subtitle("Collapse all metadata into a minimal view");
    hide_all_row.set_active(current_settings.sidebar.hide_all_details);
    sidebar_group.add(&hide_all_row);

    let git_row = adw::SwitchRow::new();
    git_row.set_title("Git Branch");
    git_row.set_active(current_settings.sidebar.show_git_branch);
    sidebar_group.add(&git_row);

    let branch_layout_row = adw::SwitchRow::new();
    branch_layout_row.set_title("Vertical Branch Layout");
    branch_layout_row.set_subtitle("Show per-pane branches on separate lines");
    branch_layout_row.set_active(current_settings.sidebar.branch_vertical_layout);
    sidebar_group.add(&branch_layout_row);

    let dir_row = adw::SwitchRow::new();
    dir_row.set_title("Directory Path");
    dir_row.set_active(current_settings.sidebar.show_directory);
    sidebar_group.add(&dir_row);

    let notif_msg_row = adw::SwitchRow::new();
    notif_msg_row.set_title("Notification Message");
    notif_msg_row.set_subtitle("Show latest notification below workspace title");
    notif_msg_row.set_active(current_settings.sidebar.show_notification_message);
    sidebar_group.add(&notif_msg_row);

    let pr_row = adw::SwitchRow::new();
    pr_row.set_title("PR Status");
    pr_row.set_active(current_settings.sidebar.show_pr_status);
    sidebar_group.add(&pr_row);

    let ports_row = adw::SwitchRow::new();
    ports_row.set_title("Listening Ports");
    ports_row.set_active(current_settings.sidebar.show_ports);
    sidebar_group.add(&ports_row);

    let logs_row = adw::SwitchRow::new();
    logs_row.set_title("Log Entries");
    logs_row.set_active(current_settings.sidebar.show_logs);
    sidebar_group.add(&logs_row);

    let progress_row = adw::SwitchRow::new();
    progress_row.set_title("Progress Bars");
    progress_row.set_active(current_settings.sidebar.show_progress);
    sidebar_group.add(&progress_row);

    let pills_row = adw::SwitchRow::new();
    pills_row.set_title("Status Pills");
    pills_row.set_active(current_settings.sidebar.show_status_pills);
    sidebar_group.add(&pills_row);

    // When "Hide All Details" is active, gray out individual toggles
    let detail_rows: Vec<adw::SwitchRow> = vec![
        git_row.clone(),
        branch_layout_row.clone(),
        dir_row.clone(),
        notif_msg_row.clone(),
        pr_row.clone(),
        ports_row.clone(),
        logs_row.clone(),
        progress_row.clone(),
        pills_row.clone(),
    ];
    // Set initial sensitivity based on hide_all_details
    {
        let sensitive = !hide_all_row.is_active();
        for row in &detail_rows {
            row.set_sensitive(sensitive);
        }
    }
    {
        let detail_rows = detail_rows.clone();
        hide_all_row.connect_active_notify(move |row| {
            let sensitive = !row.is_active();
            for r in &detail_rows {
                r.set_sensitive(sensitive);
            }
        });
    }

    let focus_style_row = adw::ComboRow::new();
    focus_style_row.set_title("Selection Style");
    focus_style_row.set_subtitle("How the selected workspace is highlighted");
    let focus_labels: Vec<&str> = SidebarFocusStyle::ALL.iter().map(|s| s.label()).collect();
    let focus_list = gtk4::StringList::new(&focus_labels);
    focus_style_row.set_model(Some(&focus_list));
    focus_style_row.set_selected(current_settings.sidebar.focus_style.to_index());
    sidebar_group.add(&focus_style_row);

    let port_external_row = adw::SwitchRow::new();
    port_external_row.set_title("Open Ports Externally");
    port_external_row.set_subtitle("Click port badges to open in the system browser instead of a panel");
    port_external_row.set_active(current_settings.sidebar.port_link_external);
    sidebar_group.add(&port_external_row);

    let selection_color_row = adw::EntryRow::new();
    selection_color_row.set_title("Selection Highlight Color");
    selection_color_row.set_tooltip_text(Some("CSS color for the selected workspace (e.g. #3584e4). Leave empty to use the default accent color."));
    selection_color_row.set_text(&current_settings.sidebar.selection_color);
    sidebar_group.add(&selection_color_row);

    let tint_opacity_row = adw::SpinRow::new(
        Some(&gtk4::Adjustment::new(
            current_settings.sidebar.tint_opacity as f64,
            0.0,
            1.0,
            0.05,
            0.1,
            0.0,
        )),
        0.05,
        2,
    );
    tint_opacity_row.set_title("Tint Opacity");
    tint_opacity_row.set_subtitle("Opacity of the sidebar tint color (0.0–1.0)");
    sidebar_group.add(&tint_opacity_row);

    let width_value = if current_settings.sidebar.width > 0 {
        current_settings.sidebar.width as f64
    } else {
        280.0
    };
    let width_row = adw::SpinRow::new(
        Some(&gtk4::Adjustment::new(
            width_value,
            150.0,
            600.0,
            10.0,
            10.0,
            0.0,
        )),
        10.0,
        0,
    );
    width_row.set_title("Sidebar Width");
    width_row.set_subtitle("Width of the workspace sidebar in pixels (applies on next window open)");
    sidebar_group.add(&width_row);

    let match_terminal_bg_row = adw::SwitchRow::new();
    match_terminal_bg_row.set_title("Match Terminal Background");
    match_terminal_bg_row.set_subtitle("Use the terminal's background color for the sidebar");
    match_terminal_bg_row.set_active(current_settings.sidebar.match_terminal_background);
    sidebar_group.add(&match_terminal_bg_row);

    appearance_page.add(&sidebar_group);

    // ── Workspace group ──
    let workspace_group = adw::PreferencesGroup::new();
    workspace_group.set_title("Workspace");

    let warn_close_tab_row = adw::SwitchRow::new();
    warn_close_tab_row.set_title("Warn Before Closing Tab");
    warn_close_tab_row
        .set_subtitle("Show confirmation before closing a workspace with active terminals");
    warn_close_tab_row.set_active(current_settings.warn_before_closing_tab);
    workspace_group.add(&warn_close_tab_row);

    let cwd_inherit_row = adw::SwitchRow::new();
    cwd_inherit_row.set_title("Inherit Working Directory");
    cwd_inherit_row
        .set_subtitle("New workspaces start in the same directory as the active terminal");
    cwd_inherit_row.set_active(current_settings.workspace_cwd_inheritance);
    workspace_group.add(&cwd_inherit_row);

    let plus_btn_row = adw::ComboRow::new();
    plus_btn_row.set_title("Plus Button Action");
    plus_btn_row.set_subtitle("What the + button in the header creates");
    let plus_btn_labels: Vec<&str> = PlusButtonAction::ALL.iter().map(|a| a.label()).collect();
    let plus_btn_list = gtk4::StringList::new(&plus_btn_labels);
    plus_btn_row.set_model(Some(&plus_btn_list));
    plus_btn_row.set_selected(current_settings.plus_button_action.to_index());
    workspace_group.add(&plus_btn_row);

    let split_ratio_persist_row = adw::SwitchRow::new();
    split_ratio_persist_row.set_title("Persist Split Ratios");
    split_ratio_persist_row
        .set_subtitle("Save and restore pane split positions across sessions");
    split_ratio_persist_row.set_active(current_settings.split_ratio_persist);
    workspace_group.add(&split_ratio_persist_row);

    workspace_page.add(&workspace_group);

    // ── Terminal group ──
    let terminal_group = adw::PreferencesGroup::new();
    terminal_group.set_title("Terminal");

    let copy_on_select_row = adw::SwitchRow::new();
    copy_on_select_row.set_title("Copy on Select");
    copy_on_select_row
        .set_subtitle("Automatically copy terminal selection to the clipboard");
    copy_on_select_row.set_active(current_settings.copy_on_select);
    terminal_group.add(&copy_on_select_row);

    let notes_path_row = adw::EntryRow::new();
    notes_path_row.set_title("Notes File");
    notes_path_row.set_tooltip_text(Some(
        "Path for the notes scratchpad (jmux notes). Leave empty for the default (~/.local/share/jmux/notes.md). A leading ~ is expanded.",
    ));
    notes_path_row.set_text(&current_settings.notes_path);
    terminal_group.add(&notes_path_row);

    terminal_page.add(&terminal_group);

    // ── TextBox group ──
    let textbox_group = adw::PreferencesGroup::new();
    textbox_group.set_title("TextBox");
    textbox_group.set_description(Some(
        "A prompt composer below the terminal. Enter sends; Shift+Enter adds a newline.",
    ));

    let show_textbox_row = adw::SwitchRow::new();
    show_textbox_row.set_title("Show TextBox on New Terminals");
    show_textbox_row.set_active(current_settings.show_textbox_on_new_terminals);
    textbox_group.add(&show_textbox_row);

    let focus_textbox_row = adw::SwitchRow::new();
    focus_textbox_row.set_title("Focus TextBox on New Terminals");
    focus_textbox_row.set_subtitle("Place the cursor in the TextBox instead of the terminal");
    focus_textbox_row.set_active(current_settings.focus_textbox_on_new_terminals);
    textbox_group.add(&focus_textbox_row);

    let textbox_lines_row = adw::EntryRow::new();
    textbox_lines_row.set_title("TextBox Max Lines");
    textbox_lines_row.set_text(&current_settings.textbox_max_lines.to_string());
    textbox_group.add(&textbox_lines_row);

    workspace_page.add(&textbox_group);

    // ── Dock group ──
    let dock_group = adw::PreferencesGroup::new();
    dock_group.set_title("Dock");
    dock_group.set_description(Some(
        "Right-side terminal controls from .jmux/dock.json (toggle with the header Dock button or palette).",
    ));
    let show_dock_row = adw::SwitchRow::new();
    show_dock_row.set_title("Show Dock");
    show_dock_row.set_active(current_settings.show_dock);
    dock_group.add(&show_dock_row);

    let edit_dock_row = adw::ActionRow::new();
    edit_dock_row.set_title("Edit Dock Controls…");
    edit_dock_row.set_subtitle("Add, edit, or remove controls in ~/.config/jmux/dock.json");
    edit_dock_row.set_activatable(true);
    edit_dock_row.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
    {
        let parent = parent.clone();
        edit_dock_row.connect_activated(move |_| {
            crate::ui::dock_editor::show_dock_editor(&parent, || {});
        });
    }
    dock_group.add(&edit_dock_row);

    workspace_page.add(&dock_group);

    // ── Editor & Files group ──
    let editor_group = adw::PreferencesGroup::new();
    editor_group.set_title("Editor and Files");

    let word_wrap_row = adw::SwitchRow::new();
    word_wrap_row.set_title("Word Wrap");
    word_wrap_row.set_subtitle("Wrap long lines in the file preview, editor, and notes panels");
    word_wrap_row.set_active(current_settings.editor_word_wrap);
    editor_group.add(&word_wrap_row);

    let file_open_action_row = adw::ComboRow::new();
    file_open_action_row.set_title("File Double-Click Action");
    file_open_action_row.set_subtitle("What double-clicking a file in the explorer does");
    file_open_action_row.set_model(Some(&gtk4::StringList::new(&[
        "Preview",
        "Default app",
        "Preferred editor",
    ])));
    file_open_action_row.set_selected(current_settings.file_explorer_open_action.to_index());
    editor_group.add(&file_open_action_row);

    let preferred_editor_row = adw::EntryRow::new();
    preferred_editor_row.set_title("Preferred Editor Command");
    preferred_editor_row.set_text(&current_settings.preferred_editor);
    editor_group.add(&preferred_editor_row);

    let ai_auto_naming_row = adw::SwitchRow::new();
    ai_auto_naming_row.set_title("AI Workspace Auto-Naming");
    ai_auto_naming_row.set_subtitle(
        "Name a workspace from its agent transcript when the agent finishes (uses ANTHROPIC_API_KEY)",
    );
    ai_auto_naming_row.set_active(current_settings.ai_auto_naming);
    editor_group.add(&ai_auto_naming_row);

    editor_page.add(&editor_group);

    // ── Quick Terminal group ──
    let qt_group = adw::PreferencesGroup::new();
    qt_group.set_title("Quick Terminal");
    qt_group.set_description(Some(
        "A Quake-style drop-down terminal that slides in from the top edge, toggled by a global hotkey. Requires a quick-terminal build + a layer-shell compositor (KDE/wlroots).",
    ));

    let qt_enabled_row = adw::SwitchRow::new();
    qt_enabled_row.set_title("Enable Quick Terminal");
    qt_enabled_row.set_active(current_settings.quick_terminal.enabled);
    qt_group.add(&qt_enabled_row);

    let qt_hotkey_row = adw::EntryRow::new();
    qt_hotkey_row.set_title("Global Hotkey");
    qt_hotkey_row.set_tooltip_text(Some(
        "Suggested trigger registered via the GlobalShortcuts portal (e.g. CTRL+grave, F12). \
         Some desktops (KDE) list it in System Settings → Shortcuts for you to assign/confirm. \
         As a reliable fallback, bind `jmux quick-terminal toggle` to a key in your desktop.",
    ));
    qt_hotkey_row.set_text(&current_settings.quick_terminal.hotkey);
    qt_group.add(&qt_hotkey_row);

    let qt_height_adj = gtk4::Adjustment::new(
        (current_settings.quick_terminal.height_fraction * 100.0) as f64,
        10.0,
        100.0,
        5.0,
        10.0,
        0.0,
    );
    let qt_height_row = adw::SpinRow::new(Some(&qt_height_adj), 1.0, 0);
    qt_height_row.set_title("Height (% of screen)");
    qt_group.add(&qt_height_row);

    terminal_page.add(&qt_group);

    window.add(&appearance_page);
    window.add(&terminal_page);
    window.add(&workspace_page);
    window.add(&editor_page);

    // ── Notifications page ──
    let notif_page = adw::PreferencesPage::new();
    notif_page.set_title("Notifications");
    notif_page.set_icon_name(Some("preferences-system-notifications-symbolic"));

    let notif_group = adw::PreferencesGroup::new();
    notif_group.set_title("Desktop Notifications");

    let sound_row = adw::SwitchRow::new();
    sound_row.set_title("Notification Sound");
    sound_row.set_subtitle("Play a sound when a notification arrives");
    sound_row.set_active(current_settings.notifications.sound_enabled);
    notif_group.add(&sound_row);

    // Sound preset dropdown
    let sound_preset_labels = [
        "Desktop Bell",
        "Message",
        "Bell",
        "Dialog Information",
        "Dialog Warning",
        "Complete",
        "Trash Empty",
        "Phone Incoming",
        "None",
        "Custom File...",
    ];
    let sound_preset_row = adw::ComboRow::new();
    sound_preset_row.set_title("Sound Preset");
    sound_preset_row.set_subtitle("Which sound to play for notifications");
    let sound_preset_list = gtk4::StringList::new(&sound_preset_labels);
    sound_preset_row.set_model(Some(&sound_preset_list));
    sound_preset_row.set_selected(match &current_settings.notifications.sound_name {
        NotificationSound::Default => 0,
        NotificationSound::Theme(name) => match name.as_str() {
            "message-new-instant" => 1,
            "bell" => 2,
            "dialog-information" => 3,
            "dialog-warning" => 4,
            "complete" => 5,
            "trash-empty" => 6,
            "phone-incoming-call" => 7,
            _ => 1,
        },
        NotificationSound::None => 8,
        NotificationSound::File(_) => 9,
    });
    notif_group.add(&sound_preset_row);

    let reorder_notif_row = adw::SwitchRow::new();
    reorder_notif_row.set_title("Auto-Reorder on Notification");
    reorder_notif_row.set_subtitle("Move workspaces with new notifications toward the top");
    reorder_notif_row.set_active(current_settings.notifications.reorder_on_notification);
    notif_group.add(&reorder_notif_row);

    let command_row = adw::EntryRow::new();
    command_row.set_title("Custom Command");
    if let Some(ref cmd) = current_settings.notifications.custom_command {
        command_row.set_text(cmd);
    }
    notif_group.add(&command_row);

    notif_page.add(&notif_group);
    window.add(&notif_page);

    // ── Browser page ──
    let browser_page = adw::PreferencesPage::new();
    browser_page.set_title("Browser");
    browser_page.set_icon_name(Some("globe-symbolic"));

    let browser_group = adw::PreferencesGroup::new();
    browser_group.set_title("Browser Panel");

    let browser_enabled_row = adw::SwitchRow::new();
    browser_enabled_row.set_title("Enable Browser");
    browser_enabled_row.set_subtitle("Enable the built-in browser engine (takes effect on next open)");
    browser_enabled_row.set_active(current_settings.browser.enabled);
    browser_group.add(&browser_enabled_row);

    let engine_row = adw::ComboRow::new();
    engine_row.set_title("Search Engine");
    engine_row.set_subtitle("Default search engine for URL bar queries");
    let engine_labels: Vec<&str> = SearchEngine::ALL.iter().map(|e| e.label()).collect();
    let engine_list = gtk4::StringList::new(&engine_labels);
    engine_row.set_model(Some(&engine_list));
    engine_row.set_selected(current_settings.browser.search_engine.to_index());
    browser_group.add(&engine_row);

    let custom_search_row = adw::EntryRow::new();
    custom_search_row.set_title("Custom Search URL");
    custom_search_row.set_tooltip_text(Some(
        "Used when Search Engine is set to Custom. Use %s for the query (e.g. https://search.brave.com/search?q=%s).",
    ));
    custom_search_row.set_text(&current_settings.browser.custom_search_template);
    browser_group.add(&custom_search_row);

    let home_row = adw::EntryRow::new();
    home_row.set_title("Home Page URL");
    home_row.set_text(&current_settings.browser.home_url);
    browser_group.add(&home_row);

    let suggestions_row = adw::SwitchRow::new();
    suggestions_row.set_title("Search Suggestions");
    suggestions_row.set_subtitle("Show live search suggestions from the search engine");
    suggestions_row.set_active(current_settings.browser.search_suggestions);
    browser_group.add(&suggestions_row);

    let theme_labels: Vec<&str> = crate::settings::BrowserThemeMode::ALL
        .iter()
        .map(|m| m.label())
        .collect();
    let browser_theme_row = adw::ComboRow::new();
    browser_theme_row.set_title("Browser Theme");
    browser_theme_row.set_subtitle("Override color scheme for web pages");
    browser_theme_row.set_model(Some(&gtk4::StringList::new(&theme_labels)));
    browser_theme_row.set_selected(current_settings.browser.browser_theme.to_index());
    browser_group.add(&browser_theme_row);

    let memory_saver_row = adw::SwitchRow::new();
    memory_saver_row.set_title("Memory Saver");
    memory_saver_row
        .set_subtitle("Suspend hidden browser tabs after 60 seconds to free memory");
    memory_saver_row.set_active(current_settings.browser.memory_saver_enabled);
    browser_group.add(&memory_saver_row);

    browser_page.add(&browser_group);

    // ── Browser data import ──
    #[cfg(feature = "webkit")]
    {
        let import_group = adw::PreferencesGroup::new();
        import_group.set_title("Import Browser Data");
        import_group.set_description(Some("Import cookies from another browser. Requires sqlite3 to be installed."));

        for (label, source) in [
            ("Import from Firefox", crate::browser_import::ImportSource::Firefox),
            ("Import from Chrome", crate::browser_import::ImportSource::Chrome),
            ("Import from Chromium", crate::browser_import::ImportSource::Chromium),
        ] {
            let row = adw::ActionRow::new();
            row.set_title(label);
            let btn = gtk4::Button::new();
            btn.set_label("Import");
            btn.set_valign(gtk4::Align::Center);
            btn.add_css_class("pill");
            {
                let window_ref = window.downgrade();
                btn.connect_clicked(move |_| {
                    let (count, err) = crate::browser_import::import_from(source);
                    let Some(win) = window_ref.upgrade() else { return; };
                    let (title, body) = if let Some(e) = err {
                        ("Import Failed".to_string(), e)
                    } else {
                        ("Import Complete".to_string(), format!("Imported {count} cookies."))
                    };
                    let dialog = libadwaita::AlertDialog::new(Some(&title), Some(&body));
                    dialog.add_response("ok", "OK");
                    dialog.present(Some(&win));
                });
            }
            row.add_suffix(&btn);
            import_group.add(&row);
        }
        browser_page.add(&import_group);
    }

    window.add(&browser_page);

    // ── Socket page ──
    let socket_page = adw::PreferencesPage::new();
    socket_page.set_title("Socket");
    socket_page.set_icon_name(Some("network-server-symbolic"));

    let socket_group = adw::PreferencesGroup::new();
    socket_group.set_title("Socket API Access");

    let socket_row = adw::ComboRow::new();
    socket_row.set_title("Access Mode");
    socket_row.set_subtitle("Controls who can connect to the jmux socket");
    let socket_list = gtk4::StringList::new(&["Off", "jmux only", "Allow all"]);
    socket_row.set_model(Some(&socket_list));
    socket_row.set_selected(match current_settings.socket_access {
        SocketAccess::Off => 0,
        SocketAccess::JmuxOnly => 1,
        SocketAccess::AllowAll => 2,
    });
    socket_group.add(&socket_row);

    // Show current socket path
    let socket_path = crate::socket::server::socket_path();
    let path_row = adw::ActionRow::new();
    path_row.set_title("Socket Path");
    path_row.set_subtitle(&socket_path);
    socket_group.add(&path_row);

    socket_page.add(&socket_group);
    window.add(&socket_page);

    // ── Keyboard page ──
    let keyboard_page = adw::PreferencesPage::new();
    keyboard_page.set_title("Keyboard");
    keyboard_page.set_icon_name(Some("input-keyboard-symbolic"));

    let shortcuts_intro = adw::PreferencesGroup::new();
    shortcuts_intro.set_title("Keyboard Shortcuts");
    shortcuts_intro.set_description(Some(
        "Click a shortcut to record a new binding. Press Escape to cancel.",
    ));
    keyboard_page.add(&shortcuts_intro);

    let shortcuts_state =
        std::rc::Rc::new(std::cell::RefCell::new(current_settings.shortcuts.clone()));
    // Handles for the Reset button to update in place: (action, label, clear btn).
    let row_handles: std::rc::Rc<
        std::cell::RefCell<Vec<(String, gtk4::Label, gtk4::Button)>>,
    > = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    // Build one labelled row (title = friendly label, subtitle = action id) with
    // click-to-record + clear, and add it to `group`.
    let add_shortcut_row = {
        let shortcuts_state = shortcuts_state.clone();
        let row_handles = row_handles.clone();
        move |group: &adw::PreferencesGroup, action: &str, label: &str| {
        let opt_binding: Option<settings::shortcuts::Keybinding> = shortcuts_state
            .borrow()
            .bindings
            .get(action)
            .cloned()
            .flatten();
        let row = adw::ActionRow::new();
        row.set_title(label);
        row.set_subtitle(action);
        row.set_activatable(true);

        let binding_text = opt_binding
            .as_ref()
            .map(|b| b.display())
            .unwrap_or_else(|| "unbound".to_string());
        let shortcut_label = gtk4::Label::new(Some(&binding_text));
        shortcut_label.add_css_class("dim-label");
        row.add_suffix(&shortcut_label);

        // Conflict warning label (hidden by default)
        let conflict_label = gtk4::Label::new(None);
        conflict_label.add_css_class("warning");
        conflict_label.set_visible(false);
        row.add_suffix(&conflict_label);

        // Clear button — unbinds the shortcut when clicked
        let clear_btn = gtk4::Button::from_icon_name("edit-clear-symbolic");
        clear_btn.set_tooltip_text(Some("Clear shortcut"));
        clear_btn.add_css_class("flat");
        clear_btn.add_css_class("circular");
        clear_btn.set_valign(gtk4::Align::Center);
        // Only show/enable when there is a binding
        clear_btn.set_sensitive(opt_binding.is_some());
        {
            let action_name_clear = action.to_string();
            let label_clear = shortcut_label.clone();
            let conflict_clear = conflict_label.clone();
            let state_clear = shortcuts_state.clone();
            let btn_weak = clear_btn.downgrade();
            clear_btn.connect_clicked(move |_| {
                state_clear
                    .borrow_mut()
                    .bindings
                    .insert(action_name_clear.clone(), None);
                label_clear.set_text("unbound");
                conflict_clear.set_visible(false);
                if let Some(btn) = btn_weak.upgrade() {
                    btn.set_sensitive(false);
                }
            });
        }
        row.add_suffix(&clear_btn);

        // Click-to-record: when the row is activated, listen for a key press
        let action_name = action.to_string();
        let label_clone = shortcut_label.clone();
        let conflict_clone = conflict_label.clone();
        let state = shortcuts_state.clone();
        let clear_btn_weak = clear_btn.downgrade();
        row.connect_activated(move |row| {
            label_clone.set_text("Press shortcut...");
            label_clone.remove_css_class("dim-label");
            label_clone.add_css_class("accent");
            conflict_clone.set_visible(false);

            let key_controller = gtk4::EventControllerKey::new();
            let label_inner = label_clone.clone();
            let conflict_inner = conflict_clone.clone();
            let action_inner = action_name.clone();
            let state_inner = state.clone();
            let row_weak = row.downgrade();
            let clear_btn_inner = clear_btn_weak.clone();
            key_controller.connect_key_pressed(move |ctl, keyval, _keycode, modifiers| {
                // Escape cancels
                if keyval == gdk4::Key::Escape {
                    let current = state_inner.borrow();
                    let text = current
                        .bindings
                        .get(&action_inner)
                        .and_then(|opt| opt.as_ref())
                        .map(|b| b.display())
                        .unwrap_or_else(|| "unbound".to_string());
                    label_inner.set_text(&text);
                    label_inner.remove_css_class("accent");
                    label_inner.add_css_class("dim-label");
                    conflict_inner.set_visible(false);
                    if let Some(row) = row_weak.upgrade() {
                        row.remove_controller(ctl);
                    }
                    return glib::Propagation::Stop;
                }

                // Ignore bare modifier keys
                if matches!(
                    keyval,
                    gdk4::Key::Shift_L
                        | gdk4::Key::Shift_R
                        | gdk4::Key::Control_L
                        | gdk4::Key::Control_R
                        | gdk4::Key::Alt_L
                        | gdk4::Key::Alt_R
                        | gdk4::Key::Super_L
                        | gdk4::Key::Super_R
                ) {
                    return glib::Propagation::Proceed;
                }

                let ctrl = modifiers.contains(gdk4::ModifierType::CONTROL_MASK);
                let shift = modifiers.contains(gdk4::ModifierType::SHIFT_MASK);
                let alt = modifiers.contains(gdk4::ModifierType::ALT_MASK);
                // Normalize the key name: GTK may return " " or an empty string
                // for the space bar; canonicalize to "space".
                let raw_key_name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
                let key_name = if raw_key_name.trim().is_empty()
                    || keyval == gdk4::Key::space
                {
                    "space".to_string()
                } else {
                    raw_key_name
                };

                let new_binding = settings::shortcuts::Keybinding {
                    key: key_name,
                    ctrl,
                    shift,
                    alt,
                };

                // Conflict detection: check if this binding is already used by
                // a different action.
                let conflict_action = {
                    let current = state_inner.borrow();
                    current
                        .bindings
                        .iter()
                        .find(|(other_action, opt)| {
                            *other_action != &action_inner
                                && opt.as_ref() == Some(&new_binding)
                        })
                        .map(|(other_action, _)| other_action.clone())
                };

                if let Some(conflict) = conflict_action {
                    conflict_inner.set_text(&format!("Already used by: {conflict}"));
                    conflict_inner.set_visible(true);
                } else {
                    conflict_inner.set_visible(false);
                }

                label_inner.set_text(&new_binding.display());
                label_inner.remove_css_class("accent");
                label_inner.add_css_class("dim-label");

                state_inner
                    .borrow_mut()
                    .bindings
                    .insert(action_inner.clone(), Some(new_binding));

                // Re-enable the clear button now that a binding exists
                if let Some(btn) = clear_btn_inner.upgrade() {
                    btn.set_sensitive(true);
                }

                if let Some(row) = row_weak.upgrade() {
                    row.remove_controller(ctl);
                }
                glib::Propagation::Stop
            });
            row.add_controller(key_controller);

            // Cancel recording on focus loss
            let focus_controller = gtk4::EventControllerFocus::new();
            let label_focus = label_clone.clone();
            let action_focus = action_name.clone();
            let state_focus = state.clone();
            let row_focus_weak = row.downgrade();
            focus_controller.connect_leave(move |ctl| {
                let current = state_focus.borrow();
                let text = current
                    .bindings
                    .get(&action_focus)
                    .and_then(|opt| opt.as_ref())
                    .map(|b| b.display())
                    .unwrap_or_else(|| "unbound".to_string());
                label_focus.set_text(&text);
                label_focus.remove_css_class("accent");
                label_focus.add_css_class("dim-label");
                if let Some(row) = row_focus_weak.upgrade() {
                    row.remove_controller(ctl);
                }
            });
            row.add_controller(focus_controller);
        });

        row_handles
            .borrow_mut()
            .push((action.to_string(), shortcut_label.clone(), clear_btn.clone()));
        group.add(&row);
        }
    };

    // Render the catalog: one PreferencesGroup per category, friendly labels.
    let mut rendered: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (category, actions) in shortcut_catalog() {
        let group = adw::PreferencesGroup::new();
        group.set_title(category);
        let mut added = false;
        for (action, label) in actions {
            if current_settings.shortcuts.bindings.contains_key(action) {
                add_shortcut_row(&group, action, label);
                rendered.insert(action.to_string());
                added = true;
            }
        }
        if added {
            keyboard_page.add(&group);
        }
    }
    // Any configured action not covered by the catalog → "Other".
    let mut leftovers: Vec<&String> = current_settings
        .shortcuts
        .bindings
        .keys()
        .filter(|k| !rendered.contains(*k))
        .collect();
    leftovers.sort();
    if !leftovers.is_empty() {
        let group = adw::PreferencesGroup::new();
        group.set_title("Other");
        for action in leftovers {
            add_shortcut_row(&group, action, action);
        }
        keyboard_page.add(&group);
    }

    // Reset-to-defaults row in its own group; updates labels via tracked handles.
    let reset_group = adw::PreferencesGroup::new();
    let reset_row = adw::ActionRow::new();
    reset_row.set_title("Reset All to Defaults");
    reset_row.set_activatable(true);
    reset_row.add_css_class("error");
    {
        let state = shortcuts_state.clone();
        let row_handles = row_handles.clone();
        reset_row.connect_activated(move |_| {
            let defaults = settings::shortcuts::ShortcutConfig::default();
            *state.borrow_mut() = defaults.clone();
            for (action, label, clear_btn) in row_handles.borrow().iter() {
                let binding = defaults.bindings.get(action).and_then(|o| o.as_ref());
                label.set_text(
                    &binding
                        .map(|b| b.display())
                        .unwrap_or_else(|| "unbound".to_string()),
                );
                label.remove_css_class("accent");
                label.add_css_class("dim-label");
                clear_btn.set_sensitive(binding.is_some());
            }
        });
    }
    reset_group.add(&reset_row);
    keyboard_page.add(&reset_group);
    window.add(&keyboard_page);

    // ── Agent Integrations page ──
    let agents_page = adw::PreferencesPage::new();
    agents_page.set_title("Agents");
    agents_page.set_icon_name(Some("system-run-symbolic"));

    let agents_group = adw::PreferencesGroup::new();
    agents_group.set_title("Session Restore");
    agents_group.set_description(Some(
        "When jmux restarts, detected AI agent sessions are resumed automatically. \
        Disable a toggle to use a plain shell instead.",
    ));

    let agent_claude_row = adw::SwitchRow::new();
    agent_claude_row.set_title("Claude Code");
    agent_claude_row.set_subtitle("Resume with `claude --continue`");
    agent_claude_row.set_active(current_settings.agent_restore.claude_code);
    agents_group.add(&agent_claude_row);

    let agent_opencode_row = adw::SwitchRow::new();
    agent_opencode_row.set_title("OpenCode");
    agent_opencode_row.set_subtitle("Resume with `opencode --resume`");
    agent_opencode_row.set_active(current_settings.agent_restore.opencode);
    agents_group.add(&agent_opencode_row);

    let agent_codex_row = adw::SwitchRow::new();
    agent_codex_row.set_title("Codex CLI");
    agent_codex_row.set_subtitle("Resume with `codex`");
    agent_codex_row.set_active(current_settings.agent_restore.codex);
    agents_group.add(&agent_codex_row);

    let agent_gemini_row = adw::SwitchRow::new();
    agent_gemini_row.set_title("Gemini CLI");
    agent_gemini_row.set_subtitle("Resume with `gemini`");
    agent_gemini_row.set_active(current_settings.agent_restore.gemini);
    agents_group.add(&agent_gemini_row);

    let agent_rovo_row = adw::SwitchRow::new();
    agent_rovo_row.set_title("Rovo Dev");
    agent_rovo_row.set_subtitle("Resume with `rovo dev`");
    agent_rovo_row.set_active(current_settings.agent_restore.rovo_dev);
    agents_group.add(&agent_rovo_row);

    let agent_cursor_row = adw::SwitchRow::new();
    agent_cursor_row.set_title("Cursor");
    agent_cursor_row.set_subtitle("Resume with `cursor`");
    agent_cursor_row.set_active(current_settings.agent_restore.cursor);
    agents_group.add(&agent_cursor_row);

    let agent_grok_row = adw::SwitchRow::new();
    agent_grok_row.set_title("Grok Build CLI");
    agent_grok_row.set_subtitle("Resume with `grok`");
    agent_grok_row.set_active(current_settings.agent_restore.grok);
    agents_group.add(&agent_grok_row);

    let agent_amp_row = adw::SwitchRow::new();
    agent_amp_row.set_title("Amp");
    agent_amp_row.set_subtitle("Resume with `amp`");
    agent_amp_row.set_active(current_settings.agent_restore.amp);
    agents_group.add(&agent_amp_row);

    let agent_pi_row = adw::SwitchRow::new();
    agent_pi_row.set_title("Pi Vault");
    agent_pi_row.set_subtitle("Resume with `pi`");
    agent_pi_row.set_active(current_settings.agent_restore.pi);
    agents_group.add(&agent_pi_row);

    let agent_hermes_row = adw::SwitchRow::new();
    agent_hermes_row.set_title("Hermes");
    agent_hermes_row.set_subtitle("Resume with `hermes`");
    agent_hermes_row.set_active(current_settings.agent_restore.hermes);
    agents_group.add(&agent_hermes_row);

    let agent_antigravity_row = adw::SwitchRow::new();
    agent_antigravity_row.set_title("Antigravity");
    agent_antigravity_row.set_subtitle("Resume with `antigravity`");
    agent_antigravity_row.set_active(current_settings.agent_restore.antigravity);
    agents_group.add(&agent_antigravity_row);

    agents_page.add(&agents_group);
    window.add(&agents_page);

    // ── About page ──
    let about_page = adw::PreferencesPage::new();
    about_page.set_title("About");
    about_page.set_icon_name(Some("help-about-symbolic"));

    let about_group = adw::PreferencesGroup::new();
    about_group.set_title("jmux");
    about_group.set_description(Some(
        "GTK4/libadwaita terminal multiplexer for AI coding agents",
    ));

    let version_row = adw::ActionRow::new();
    version_row.set_title("Version");
    version_row.set_subtitle(env!("CARGO_PKG_VERSION"));
    about_group.add(&version_row);

    let ghostty_row = adw::ActionRow::new();
    ghostty_row.set_title("Terminal Engine");
    ghostty_row.set_subtitle("Ghostty (libghostty embedded)");
    about_group.add(&ghostty_row);

    let toolkit_row = adw::ActionRow::new();
    toolkit_row.set_title("UI Toolkit");
    toolkit_row.set_subtitle("GTK4 + libadwaita");
    about_group.add(&toolkit_row);

    about_page.add(&about_group);
    window.add(&about_page);

    // ── Save on close ──
    {
        let theme_row = theme_row.clone();
        let focus_hover_row = focus_hover_row.clone();
        let first_click_row = first_click_row.clone();
        let confirm_close_row = confirm_close_row.clone();
        let placement_row = placement_row.clone();
        let sound_row = sound_row.clone();
        let reorder_notif_row = reorder_notif_row.clone();
        let command_row = command_row.clone();
        let socket_row = socket_row.clone();
        let git_row = git_row.clone();
        let dir_row = dir_row.clone();
        let pr_row = pr_row.clone();
        let ports_row = ports_row.clone();
        let logs_row = logs_row.clone();
        let progress_row = progress_row.clone();
        let pills_row = pills_row.clone();
        let focus_style_row = focus_style_row.clone();
        let engine_row = engine_row.clone();
        let custom_search_row = custom_search_row.clone();
        let home_row = home_row.clone();
        let suggestions_row = suggestions_row.clone();
        let browser_theme_row = browser_theme_row.clone();
        let memory_saver_row = memory_saver_row.clone();
        let shortcuts_state = shortcuts_state.clone();
        let sound_preset_row = sound_preset_row.clone();
        let confirm_quit_row = confirm_quit_row.clone();
        let tab_bar_font_size_row = tab_bar_font_size_row.clone();
        let sidebar_font_size_row = sidebar_font_size_row.clone();
        let warn_close_tab_row = warn_close_tab_row.clone();
        let cwd_inherit_row = cwd_inherit_row.clone();
        let plus_btn_row = plus_btn_row.clone();
        let split_ratio_persist_row = split_ratio_persist_row.clone();
        let copy_on_select_row = copy_on_select_row.clone();
        let agent_claude_row = agent_claude_row.clone();
        let agent_opencode_row = agent_opencode_row.clone();
        let agent_codex_row = agent_codex_row.clone();
        let agent_gemini_row = agent_gemini_row.clone();
        let agent_rovo_row = agent_rovo_row.clone();
        let agent_cursor_row = agent_cursor_row.clone();
        let agent_grok_row = agent_grok_row.clone();
        let agent_amp_row = agent_amp_row.clone();
        let agent_pi_row = agent_pi_row.clone();
        let agent_hermes_row = agent_hermes_row.clone();
        let agent_antigravity_row = agent_antigravity_row.clone();
        window.connect_closed(move |_| {
            let theme = match theme_row.selected() {
                1 => ThemeMode::Light,
                2 => ThemeMode::Dark,
                3 => ThemeMode::Omarchy,
                _ => ThemeMode::System,
            };
            let socket_access = match socket_row.selected() {
                0 => SocketAccess::Off,
                2 => SocketAccess::AllowAll,
                _ => SocketAccess::JmuxOnly,
            };
            let custom_command = {
                let text = command_row.text().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            };
            let home_url = {
                let text = home_row.text().to_string();
                if text.is_empty() {
                    BrowserSettings::default().home_url
                } else {
                    text
                }
            };

            let new_settings = AppSettings {
                theme,
                focus_follows_mouse: focus_hover_row.is_active(),
                first_click_focus: first_click_row.is_active(),
                confirm_before_close: confirm_close_row.is_active(),
                new_workspace_placement: NewWorkspacePlacement::from_index(
                    placement_row.selected(),
                ),
                notifications: settings::NotificationSettings {
                    sound_enabled: sound_row.is_active(),
                    sound_name: match sound_preset_row.selected() {
                        0 => NotificationSound::Default,
                        1 => NotificationSound::Theme("message-new-instant".into()),
                        2 => NotificationSound::Theme("bell".into()),
                        3 => NotificationSound::Theme("dialog-information".into()),
                        4 => NotificationSound::Theme("dialog-warning".into()),
                        5 => NotificationSound::Theme("complete".into()),
                        6 => NotificationSound::Theme("trash-empty".into()),
                        7 => NotificationSound::Theme("phone-incoming-call".into()),
                        8 => NotificationSound::None,
                        // Custom file — preserve existing path
                        _ => current_settings.notifications.sound_name.clone(),
                    },
                    custom_command,
                    reorder_on_notification: reorder_notif_row.is_active(),
                },
                socket_access,
                sidebar: SidebarDisplaySettings {
                    show_git_branch: git_row.is_active(),
                    show_directory: dir_row.is_active(),
                    show_pr_status: pr_row.is_active(),
                    show_ports: ports_row.is_active(),
                    show_logs: logs_row.is_active(),
                    show_progress: progress_row.is_active(),
                    show_status_pills: pills_row.is_active(),
                    hide_all_details: hide_all_row.is_active(),
                    branch_vertical_layout: branch_layout_row.is_active(),
                    show_notification_message: notif_msg_row.is_active(),
                    focus_style: SidebarFocusStyle::from_index(focus_style_row.selected()),
                    width: width_row.value() as u32,
                    tint_color: current_settings.sidebar.tint_color.clone(),
                    tint_color_light: current_settings.sidebar.tint_color_light.clone(),
                    tint_color_dark: current_settings.sidebar.tint_color_dark.clone(),
                    tint_opacity: tint_opacity_row.value() as f32,
                    port_link_external: port_external_row.is_active(),
                    selection_color: selection_color_row.text().to_string(),
                    match_terminal_background: match_terminal_bg_row.is_active(),
                },
                browser: BrowserSettings {
                    enabled: browser_enabled_row.is_active(),
                    search_engine: SearchEngine::from_index(engine_row.selected()),
                    custom_search_template: custom_search_row.text().to_string(),
                    search_keywords: current_settings.browser.search_keywords.clone(),
                    home_url,
                    search_suggestions: suggestions_row.is_active(),
                    http_allowlist: current_settings.browser.http_allowlist.clone(),
                    browser_theme: crate::settings::BrowserThemeMode::from_index(
                        browser_theme_row.selected(),
                    ),
                    memory_saver_enabled: memory_saver_row.is_active(),
                },
                pane_attention_ring: attention_ring_row.is_active(),
                pane_flash_enabled: flash_row.is_active(),
                link_routing: settings::load().link_routing,
                remote_ssh_enabled: remote_ssh_row.is_active(),
                remote_relay_ports: settings::RemotePortRange {
                    start: relay_ports_start_row.value() as u16,
                    end: relay_ports_end_row.value() as u16,
                },
                persist_scrollback: current_settings.persist_scrollback,
                warn_before_closing_tab: warn_close_tab_row.is_active(),
                copy_on_select: copy_on_select_row.is_active(),
                confirm_quit: confirm_quit_row.is_active(),
                tab_bar_font_size: tab_bar_font_size_row
                    .text()
                    .parse::<f32>()
                    .unwrap_or(0.0)
                    .max(0.0),
                sidebar_font_size: sidebar_font_size_row
                    .text()
                    .parse::<f32>()
                    .unwrap_or(0.0)
                    .max(0.0),
                workspace_cwd_inheritance: cwd_inherit_row.is_active(),
                plus_button_action: PlusButtonAction::from_index(plus_btn_row.selected()),
                split_ratio_persist: split_ratio_persist_row.is_active(),
                agent_restore: settings::AgentRestoreSettings {
                    claude_code: agent_claude_row.is_active(),
                    opencode: agent_opencode_row.is_active(),
                    codex: agent_codex_row.is_active(),
                    gemini: agent_gemini_row.is_active(),
                    rovo_dev: agent_rovo_row.is_active(),
                    cursor: agent_cursor_row.is_active(),
                    grok: agent_grok_row.is_active(),
                    amp: agent_amp_row.is_active(),
                    pi: agent_pi_row.is_active(),
                    hermes: agent_hermes_row.is_active(),
                    antigravity: agent_antigravity_row.is_active(),
                },
                resume_command_approvals: current_settings.resume_command_approvals.clone(),
                imessage_mode: current_settings.imessage_mode,
                notes_path: notes_path_row.text().trim().to_string(),
                show_textbox_on_new_terminals: show_textbox_row.is_active(),
                focus_textbox_on_new_terminals: focus_textbox_row.is_active(),
                textbox_max_lines: textbox_lines_row
                    .text()
                    .parse::<u32>()
                    .unwrap_or(6)
                    .clamp(1, 40),
                show_dock: show_dock_row.is_active(),
                editor_word_wrap: word_wrap_row.is_active(),
                file_explorer_open_action: settings::FileOpenAction::from_index(
                    file_open_action_row.selected(),
                ),
                preferred_editor: preferred_editor_row.text().trim().to_string(),
                ai_auto_naming: ai_auto_naming_row.is_active(),
                auto_subagent_monitor: auto_subagent_row.is_active(),
                // Runners are configured via the settings file / CLI for now;
                // the dialog preserves them untouched.
                goal: current_settings.goal.clone(),
                quick_terminal: settings::QuickTerminalSettings {
                    enabled: qt_enabled_row.is_active(),
                    hotkey: qt_hotkey_row.text().trim().to_string(),
                    height_fraction: (qt_height_row.value() as f32 / 100.0).clamp(0.1, 1.0),
                    animation_ms: current_settings.quick_terminal.animation_ms,
                },
                shortcuts: shortcuts_state.borrow().clone(),
                minimal_mode: current_settings.minimal_mode,
                show_tab_close_button: show_tab_close_row.is_active(),
            };

            if let Err(e) = settings::save(&new_settings) {
                tracing::warn!("Failed to save settings: {}", e);
            }

            // Apply theme immediately
            crate::app::apply_theme_from_settings();

            // Refresh the caller's UI now that settings are saved.
            on_close();
        });
    }

    window.present(Some(parent));
}

// ── Theme discovery helpers ──────────────────────────────────────────────────

/// Collect file names (without extension) from a ghostty themes directory.
fn collect_theme_names(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let s = name.to_str()?;
            if s.starts_with('.') {
                return None;
            }
            // Strip any file extension (ghostty themes have no extension, but
            // be lenient if someone adds .conf or similar)
            let stem = std::path::Path::new(s)
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or(s);
            Some(stem.to_string())
        })
        .collect();
    names.sort();
    names
}

/// Discover user-defined ghostty themes from `~/.config/ghostty/themes/`.
pub fn discover_ghostty_user_themes() -> Vec<String> {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("ghostty/themes");
    collect_theme_names(&dir)
}

/// Discover built-in ghostty themes from the system themes directories.
/// Searches **both** `$GHOSTTY_RESOURCES_DIR/themes/` (if set) and
/// `/usr/share/ghostty/themes/`, merging the results — themes may live in
/// either location depending on how ghostty was installed.
pub fn discover_ghostty_system_themes() -> Vec<String> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(res) = std::env::var("GHOSTTY_RESOURCES_DIR") {
        dirs.push(std::path::PathBuf::from(res).join("themes"));
    }
    dirs.push(std::path::PathBuf::from("/usr/share/ghostty/themes"));

    let mut names: Vec<String> = dirs.iter().flat_map(|d| collect_theme_names(d)).collect();
    names.sort();
    names.dedup();
    names
}

/// Path to the user's ghostty config file (`~/.config/ghostty/config`).
fn ghostty_config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("ghostty/config")
}

/// Return true if `line` is an uncommented `theme = ...` assignment.
fn is_theme_assignment(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    match trimmed.strip_prefix("theme") {
        Some(rest) => rest.trim_start().starts_with('='),
        None => false,
    }
}

/// Read the currently configured ghostty theme value from the config file.
/// Ghostty is last-wins, so return the value of the last uncommented
/// `theme = ...` line. Returns the raw value (which may be a `dark:X|light:Y`
/// conditional). `None` if there is no theme set or the file is missing.
pub fn read_ghostty_config_theme() -> Option<String> {
    let contents = std::fs::read_to_string(ghostty_config_path()).ok()?;
    contents
        .lines()
        .filter(|l| is_theme_assignment(l))
        .last()
        .and_then(|l| l.split_once('='))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Set `theme = <name>` in the ghostty config, preserving all other lines and
/// comments. Replaces the last uncommented `theme = ...` line (last-wins), or
/// appends one if none exists. Creates the file/dir if missing.
pub fn set_ghostty_config_theme(name: &str) -> std::io::Result<()> {
    let path = ghostty_config_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut lines: Vec<String> = existing.lines().map(String::from).collect();
    let new_line = format!("theme = {name}");

    match lines.iter().rposition(|l| is_theme_assignment(l)) {
        Some(idx) => lines[idx] = new_line,
        None => lines.push(new_line),
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = lines.join("\n");
    out.push('\n');
    std::fs::write(&path, out)
}
