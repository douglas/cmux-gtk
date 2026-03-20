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
    /// Focus pane on mouse hover (focus-follows-mouse).
    pub focus_follows_mouse: bool,
    /// Notification settings.
    pub notifications: NotificationSettings,
    /// Socket access mode.
    pub socket_access: SocketAccess,
    /// Sidebar display toggles.
    pub sidebar: SidebarDisplaySettings,
    /// Browser settings.
    pub browser: BrowserSettings,
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
    Omarchy,
}

/// Check whether this system is running Omarchy (theme dir exists).
pub fn is_omarchy() -> bool {
    dirs::config_dir()
        .map(|d| d.join("omarchy/current/theme/colors.toml").exists())
        .unwrap_or(false)
}

/// Read the Omarchy theme and determine if it's light or dark.
/// Returns `true` if the current Omarchy theme is a light theme.
pub fn omarchy_is_light() -> bool {
    dirs::config_dir()
        .map(|d| d.join("omarchy/current/theme/light.mode").exists())
        .unwrap_or(false)
}

/// Parsed Omarchy theme colors from colors.toml.
#[derive(Debug, Default)]
pub struct OmarchyColors {
    pub accent: Option<String>,
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub cursor: Option<String>,
    pub selection_foreground: Option<String>,
    pub selection_background: Option<String>,
    pub color0: Option<String>,
    pub color8: Option<String>,
}

/// Read all colors from the current Omarchy theme's colors.toml.
pub fn omarchy_colors() -> OmarchyColors {
    let path = match dirs::config_dir() {
        Some(d) => d.join("omarchy/current/theme/colors.toml"),
        None => return OmarchyColors::default(),
    };
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return OmarchyColors::default(),
    };

    let mut colors = OmarchyColors::default();
    for line in content.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            if value.is_empty() {
                continue;
            }
            let v = Some(value.to_string());
            match key {
                "accent" => colors.accent = v,
                "background" => colors.background = v,
                "foreground" => colors.foreground = v,
                "cursor" => colors.cursor = v,
                "selection_foreground" => colors.selection_foreground = v,
                "selection_background" => colors.selection_background = v,
                "color0" => colors.color0 = v,
                "color8" => colors.color8 = v,
                _ => {}
            }
        }
    }
    colors
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

/// Browser panel settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserSettings {
    /// Default search engine for non-URL queries.
    pub search_engine: SearchEngine,
    /// Home page URL (shown when clicking home button).
    pub home_url: String,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            search_engine: SearchEngine::DuckDuckGo,
            home_url: "https://duckduckgo.com".to_string(),
        }
    }
}

/// Search engine for browser URL bar queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    Google,
    DuckDuckGo,
    Bing,
    Kagi,
}

impl SearchEngine {
    /// Return the search URL template (query appended after).
    pub fn search_url(self, query: &str) -> String {
        let encoded = query.replace(' ', "+");
        match self {
            Self::Google => format!("https://www.google.com/search?q={encoded}"),
            Self::DuckDuckGo => format!("https://duckduckgo.com/?q={encoded}"),
            Self::Bing => format!("https://www.bing.com/search?q={encoded}"),
            Self::Kagi => format!("https://kagi.com/search?q={encoded}"),
        }
    }

    pub const ALL: &[Self] = &[
        Self::Google,
        Self::DuckDuckGo,
        Self::Bing,
        Self::Kagi,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Bing => "Bing",
            Self::Kagi => "Kagi",
        }
    }

    pub fn from_index(i: u32) -> Self {
        match i {
            0 => Self::Google,
            1 => Self::DuckDuckGo,
            2 => Self::Bing,
            3 => Self::Kagi,
            _ => Self::DuckDuckGo,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Self::Google => 0,
            Self::DuckDuckGo => 1,
            Self::Bing => 2,
            Self::Kagi => 3,
        }
    }
}

/// Sidebar display toggles — which metadata to show in workspace rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarDisplaySettings {
    pub show_git_branch: bool,
    pub show_directory: bool,
    pub show_pr_status: bool,
    pub show_ports: bool,
    pub show_logs: bool,
    pub show_progress: bool,
    pub show_status_pills: bool,
}

impl Default for SidebarDisplaySettings {
    fn default() -> Self {
        Self {
            show_git_branch: true,
            show_directory: true,
            show_pr_status: true,
            show_ports: true,
            show_logs: true,
            show_progress: true,
            show_status_pills: true,
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            focus_follows_mouse: false,
            notifications: NotificationSettings::default(),
            socket_access: SocketAccess::CmuxOnly,
            sidebar: SidebarDisplaySettings::default(),
            browser: BrowserSettings::default(),
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
        .map_err(std::io::Error::other)?;
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
