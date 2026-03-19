//! Application settings — persistent configuration loaded from XDG config dir.

pub mod shortcuts;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Appearance settings.
    pub theme: ThemeMode,
    /// Notification settings.
    pub notifications: NotificationSettings,
    /// Socket access mode.
    pub socket_access: SocketAccess,
    /// Keyboard shortcuts.
    #[serde(skip)]
    pub shortcuts: shortcuts::ShortcutConfig,
}

/// Theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

/// Notification preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    /// Play a sound on notification.
    pub sound_enabled: bool,
    /// Custom command to run on notification (optional).
    pub custom_command: Option<String>,
}

/// Socket access level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocketAccess {
    Off,
    CmuxOnly,
    AllowAll,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            notifications: NotificationSettings::default(),
            socket_access: SocketAccess::CmuxOnly,
            shortcuts: shortcuts::ShortcutConfig::default(),
        }
    }
}


/// Get the settings directory path (~/.config/cmux/).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("cmux")
}

/// Load settings from disk. Returns defaults if file doesn't exist.
pub fn load() -> AppSettings {
    let mut settings = load_main_settings();
    settings.shortcuts = shortcuts::load();
    settings
}

/// Save settings to disk.
pub fn save(settings: &AppSettings) -> Result<(), std::io::Error> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;

    let path = dir.join("settings.json");
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::other(e))?;
    std::fs::write(path, json)?;

    shortcuts::save(&settings.shortcuts)?;
    Ok(())
}

fn load_main_settings() -> AppSettings {
    let path = config_dir().join("settings.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}
