//! Settings window — AdwPreferencesWindow for application configuration.

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::settings::{
    self, AppSettings, BrowserSettings, SearchEngine, SidebarDisplaySettings, SocketAccess,
    ThemeMode,
};

/// Create and show the settings preferences window.
pub fn show_settings(parent: &adw::ApplicationWindow) {
    let current_settings = settings::load();

    let window = adw::PreferencesWindow::new();
    window.set_title(Some("Settings"));
    window.set_transient_for(Some(parent));
    window.set_modal(true);
    window.set_default_width(600);
    window.set_default_height(500);

    // ── Appearance page ──
    let appearance_page = adw::PreferencesPage::new();
    appearance_page.set_title("Appearance");
    appearance_page.set_icon_name(Some("preferences-desktop-appearance-symbolic"));

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
        ThemeMode::Omarchy => if on_omarchy { 3 } else { 0 },
    });
    theme_group.add(&theme_row);
    appearance_page.add(&theme_group);

    // ── Behavior group ──
    let behavior_group = adw::PreferencesGroup::new();
    behavior_group.set_title("Behavior");

    let focus_hover_row = adw::SwitchRow::new();
    focus_hover_row.set_title("Focus Follows Mouse");
    focus_hover_row.set_subtitle("Automatically focus terminal panes on mouse hover");
    focus_hover_row.set_active(current_settings.focus_follows_mouse);
    behavior_group.add(&focus_hover_row);

    appearance_page.add(&behavior_group);

    // ── Sidebar display group ──
    let sidebar_group = adw::PreferencesGroup::new();
    sidebar_group.set_title("Sidebar Display");
    sidebar_group.set_description(Some("Choose which metadata to show in workspace rows"));

    let git_row = adw::SwitchRow::new();
    git_row.set_title("Git Branch");
    git_row.set_active(current_settings.sidebar.show_git_branch);
    sidebar_group.add(&git_row);

    let dir_row = adw::SwitchRow::new();
    dir_row.set_title("Directory Path");
    dir_row.set_active(current_settings.sidebar.show_directory);
    sidebar_group.add(&dir_row);

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

    appearance_page.add(&sidebar_group);

    window.add(&appearance_page);

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
    browser_page.set_icon_name(Some("web-browser-symbolic"));

    let browser_group = adw::PreferencesGroup::new();
    browser_group.set_title("Browser Panel");

    let engine_row = adw::ComboRow::new();
    engine_row.set_title("Search Engine");
    engine_row.set_subtitle("Default search engine for URL bar queries");
    let engine_labels: Vec<&str> = SearchEngine::ALL.iter().map(|e| e.label()).collect();
    let engine_list = gtk4::StringList::new(&engine_labels);
    engine_row.set_model(Some(&engine_list));
    engine_row.set_selected(current_settings.browser.search_engine.to_index());
    browser_group.add(&engine_row);

    let home_row = adw::EntryRow::new();
    home_row.set_title("Home Page URL");
    home_row.set_text(&current_settings.browser.home_url);
    browser_group.add(&home_row);

    browser_page.add(&browser_group);
    window.add(&browser_page);

    // ── Socket page ──
    let socket_page = adw::PreferencesPage::new();
    socket_page.set_title("Socket");
    socket_page.set_icon_name(Some("network-server-symbolic"));

    let socket_group = adw::PreferencesGroup::new();
    socket_group.set_title("Socket API Access");

    let socket_row = adw::ComboRow::new();
    socket_row.set_title("Access Mode");
    socket_row.set_subtitle("Controls who can connect to the cmux socket");
    let socket_list = gtk4::StringList::new(&["Off", "cmux only", "Allow all"]);
    socket_row.set_model(Some(&socket_list));
    socket_row.set_selected(match current_settings.socket_access {
        SocketAccess::Off => 0,
        SocketAccess::CmuxOnly => 1,
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

    let shortcuts_group = adw::PreferencesGroup::new();
    shortcuts_group.set_title("Keyboard Shortcuts");
    shortcuts_group.set_description(Some(
        "Click a shortcut to record a new binding. Press Escape to cancel.",
    ));

    let shortcuts_state = std::rc::Rc::new(std::cell::RefCell::new(
        current_settings.shortcuts.clone(),
    ));

    let mut sorted_bindings: Vec<_> = current_settings.shortcuts.bindings.iter().collect();
    sorted_bindings.sort_by_key(|(action, _)| (*action).clone());
    for (action, binding) in &sorted_bindings {
        let row = adw::ActionRow::new();
        row.set_title(action.as_str());
        row.set_activatable(true);

        let shortcut_label = gtk4::Label::new(Some(&binding.display()));
        shortcut_label.add_css_class("dim-label");
        row.add_suffix(&shortcut_label);

        // Click-to-record: when the row is activated, listen for a key press
        let action_name = (*action).clone();
        let label_clone = shortcut_label.clone();
        let state = shortcuts_state.clone();
        row.connect_activated(move |row| {
            label_clone.set_text("Press a key...");
            label_clone.remove_css_class("dim-label");
            label_clone.add_css_class("accent");

            let controller = gtk4::EventControllerKey::new();
            let label_inner = label_clone.clone();
            let action_inner = action_name.clone();
            let state_inner = state.clone();
            let row_weak = row.downgrade();
            controller.connect_key_pressed(move |ctl, keyval, _keycode, modifiers| {
                // Escape cancels
                if keyval == gdk4::Key::Escape {
                    let current = state_inner.borrow();
                    if let Some(b) = current.bindings.get(&action_inner) {
                        label_inner.set_text(&b.display());
                    }
                    label_inner.remove_css_class("accent");
                    label_inner.add_css_class("dim-label");
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
                let key_name = keyval.name().map(|n| n.to_string()).unwrap_or_default();

                let new_binding = settings::shortcuts::Keybinding {
                    key: key_name,
                    ctrl,
                    shift,
                    alt,
                };

                label_inner.set_text(&new_binding.display());
                label_inner.remove_css_class("accent");
                label_inner.add_css_class("dim-label");

                state_inner
                    .borrow_mut()
                    .bindings
                    .insert(action_inner.clone(), new_binding);

                if let Some(row) = row_weak.upgrade() {
                    row.remove_controller(ctl);
                }
                glib::Propagation::Stop
            });
            row.add_controller(controller);
        });

        shortcuts_group.add(&row);
    }

    // Reset to defaults button
    let reset_row = adw::ActionRow::new();
    reset_row.set_title("Reset All to Defaults");
    reset_row.set_activatable(true);
    reset_row.add_css_class("error");
    {
        let state = shortcuts_state.clone();
        let shortcuts_group_weak = shortcuts_group.downgrade();
        reset_row.connect_activated(move |_| {
            *state.borrow_mut() = settings::shortcuts::ShortcutConfig::default();
            // Update all labels in the group
            if let Some(group) = shortcuts_group_weak.upgrade() {
                let defaults = settings::shortcuts::ShortcutConfig::default();
                // Walk children and update suffix labels
                let mut child = group.first_child();
                while let Some(widget) = child {
                    if let Ok(row) = widget.clone().downcast::<adw::ActionRow>() {
                        let action_name = row.title().to_string();
                        if let Some(binding) = defaults.bindings.get(&action_name) {
                            // Find the suffix label
                            let mut suffix = row.first_child();
                            while let Some(s) = suffix {
                                if let Ok(label) = s.clone().downcast::<gtk4::Label>() {
                                    label.set_text(&binding.display());
                                    break;
                                }
                                // Check inside Box containers (Adw wraps suffixes)
                                if let Ok(bx) = s.clone().downcast::<gtk4::Box>() {
                                    let mut inner = bx.first_child();
                                    while let Some(ic) = inner {
                                        if let Ok(label) = ic.clone().downcast::<gtk4::Label>() {
                                            label.set_text(&binding.display());
                                            break;
                                        }
                                        inner = ic.next_sibling();
                                    }
                                }
                                suffix = s.next_sibling();
                            }
                        }
                    }
                    child = widget.next_sibling();
                }
            }
        });
    }
    shortcuts_group.add(&reset_row);

    keyboard_page.add(&shortcuts_group);
    window.add(&keyboard_page);

    // ── Save on close ──
    {
        let theme_row = theme_row.clone();
        let focus_hover_row = focus_hover_row.clone();
        let sound_row = sound_row.clone();
        let command_row = command_row.clone();
        let socket_row = socket_row.clone();
        let git_row = git_row.clone();
        let dir_row = dir_row.clone();
        let pr_row = pr_row.clone();
        let ports_row = ports_row.clone();
        let logs_row = logs_row.clone();
        let progress_row = progress_row.clone();
        let pills_row = pills_row.clone();
        let engine_row = engine_row.clone();
        let home_row = home_row.clone();
        let shortcuts_state = shortcuts_state.clone();
        window.connect_close_request(move |_| {
            let theme = match theme_row.selected() {
                1 => ThemeMode::Light,
                2 => ThemeMode::Dark,
                3 => ThemeMode::Omarchy,
                _ => ThemeMode::System,
            };
            let socket_access = match socket_row.selected() {
                0 => SocketAccess::Off,
                2 => SocketAccess::AllowAll,
                _ => SocketAccess::CmuxOnly,
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
                notifications: settings::NotificationSettings {
                    sound_enabled: sound_row.is_active(),
                    custom_command,
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
                },
                browser: BrowserSettings {
                    search_engine: SearchEngine::from_index(engine_row.selected()),
                    home_url,
                },
                shortcuts: shortcuts_state.borrow().clone(),
            };

            if let Err(e) = settings::save(&new_settings) {
                tracing::warn!("Failed to save settings: {}", e);
            }

            // Apply theme immediately
            crate::app::apply_theme_from_settings();

            glib::Propagation::Proceed
        });
    }

    window.present();
}
