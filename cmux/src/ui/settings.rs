//! Settings window — AdwPreferencesWindow for application configuration.

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::settings::{self, AppSettings, SocketAccess, ThemeMode};

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
    let theme_list = gtk4::StringList::new(&["System", "Light", "Dark"]);
    theme_row.set_model(Some(&theme_list));
    theme_row.set_selected(match current_settings.theme {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    });
    theme_group.add(&theme_row);
    appearance_page.add(&theme_group);

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
        "Edit ~/.config/cmux/shortcuts.json to customize keybindings",
    ));

    // Show current shortcuts (read-only for now)
    let mut sorted_bindings: Vec<_> = current_settings.shortcuts.bindings.iter().collect();
    sorted_bindings.sort_by_key(|(action, _)| (*action).clone());
    for (action, binding) in sorted_bindings {
        let row = adw::ActionRow::new();
        row.set_title(action);
        let label = gtk4::Label::new(Some(&binding.display()));
        label.add_css_class("dim-label");
        row.add_suffix(&label);
        shortcuts_group.add(&row);
    }

    keyboard_page.add(&shortcuts_group);
    window.add(&keyboard_page);

    // ── Save on close ──
    {
        let theme_row = theme_row.clone();
        let sound_row = sound_row.clone();
        let command_row = command_row.clone();
        let socket_row = socket_row.clone();
        window.connect_close_request(move |_| {
            let theme = match theme_row.selected() {
                1 => ThemeMode::Light,
                2 => ThemeMode::Dark,
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

            let new_settings = AppSettings {
                theme,
                notifications: settings::NotificationSettings {
                    sound_enabled: sound_row.is_active(),
                    custom_command,
                },
                socket_access,
                shortcuts: settings::shortcuts::ShortcutConfig::default(),
            };

            if let Err(e) = settings::save(&new_settings) {
                tracing::warn!("Failed to save settings: {}", e);
            }

            // Apply theme immediately
            if let Some(display) = gdk4::Display::default() {
                let style_manager = adw::StyleManager::for_display(&display);
                style_manager.set_color_scheme(match theme {
                    ThemeMode::System => adw::ColorScheme::Default,
                    ThemeMode::Light => adw::ColorScheme::ForceLight,
                    ThemeMode::Dark => adw::ColorScheme::ForceDark,
                });
            }

            glib::Propagation::Proceed
        });
    }

    window.present();
}
