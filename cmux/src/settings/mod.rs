//! Application settings — persistent configuration loaded from XDG config dir.

pub mod shortcuts;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSettings {
    /// Appearance settings.
    pub theme: ThemeMode,
    /// Focus pane on mouse hover (focus-follows-mouse).
    pub focus_follows_mouse: bool,
    /// Show confirmation dialog when quitting with active terminals.
    pub confirm_before_close: bool,
    /// Where to place newly created workspaces.
    pub new_workspace_placement: NewWorkspacePlacement,
    /// Notification settings.
    pub notifications: NotificationSettings,
    /// Socket access mode.
    pub socket_access: SocketAccess,
    /// Sidebar display toggles.
    pub sidebar: SidebarDisplaySettings,
    /// Browser settings.
    pub browser: BrowserSettings,
    /// First click on an unfocused pane only focuses it (doesn't pass through).
    pub first_click_focus: bool,
    /// Show attention ring on panes that receive output while unfocused.
    pub pane_attention_ring: bool,
    /// Enable flash effect on focused pane (Ctrl+Shift+H).
    pub pane_flash_enabled: bool,
    /// Link routing — which links open in cmux browser vs system browser.
    pub link_routing: LinkRoutingSettings,
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

/// Where new workspaces are placed in the sidebar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewWorkspacePlacement {
    #[default]
    End,
    AfterCurrent,
    Top,
}

impl NewWorkspacePlacement {
    pub const ALL: &[Self] = &[Self::End, Self::AfterCurrent, Self::Top];

    pub fn label(self) -> &'static str {
        match self {
            Self::End => "End",
            Self::AfterCurrent => "After Current",
            Self::Top => "Top",
        }
    }

    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::AfterCurrent,
            2 => Self::Top,
            _ => Self::End,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Self::End => 0,
            Self::AfterCurrent => 1,
            Self::Top => 2,
        }
    }
}

/// Notification preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    /// Play a sound on notification.
    pub sound_enabled: bool,
    /// Sound name from the freedesktop sound theme (e.g. "bell", "message-new-instant").
    /// "default" uses the desktop bell. "none" disables sound. Custom file paths
    /// (ending in .wav, .ogg, .oga) are played directly.
    pub sound_name: NotificationSound,
    /// Custom command to run on notification (optional).
    pub custom_command: Option<String>,
    /// Auto-reorder workspaces with new notifications toward the top.
    pub reorder_on_notification: bool,
}

/// Notification sound selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSound {
    /// Desktop bell (gdk4::Display::beep).
    #[default]
    Default,
    /// No sound at all.
    None,
    /// Freedesktop sound theme name (e.g. "message-new-instant", "bell", "dialog-information").
    #[serde(rename = "theme")]
    Theme(String),
    /// Custom file path (.wav, .ogg, .oga).
    #[serde(rename = "file")]
    File(String),
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
    /// Show remote search suggestions in the omnibar.
    pub search_suggestions: bool,
    /// Hosts allowed to load over insecure HTTP without warning.
    #[serde(default)]
    pub http_allowlist: Vec<String>,
    /// Browser color scheme override (separate from app theme).
    pub browser_theme: BrowserThemeMode,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            search_engine: SearchEngine::DuckDuckGo,
            home_url: "https://duckduckgo.com".to_string(),
            search_suggestions: true,
            http_allowlist: Vec::new(),
            browser_theme: BrowserThemeMode::System,
        }
    }
}

/// Browser-specific color scheme override.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl BrowserThemeMode {
    pub const ALL: &[Self] = &[Self::System, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::Light,
            2 => Self::Dark,
            _ => Self::System,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Self::System => 0,
            Self::Light => 1,
            Self::Dark => 2,
        }
    }

    /// Returns the JavaScript to inject for forcing browser color scheme.
    pub fn theme_injection_js(self) -> &'static str {
        match self {
            Self::System => {
                r#"
                (function() {
                    var meta = document.getElementById('cmux-browser-theme-meta');
                    if (meta) meta.remove();
                    document.documentElement.removeAttribute('data-cmux-browser-theme');
                })();
            "#
            }
            Self::Light => {
                r#"
                (function() {
                    var meta = document.getElementById('cmux-browser-theme-meta');
                    if (!meta) {
                        meta = document.createElement('meta');
                        meta.id = 'cmux-browser-theme-meta';
                        meta.name = 'color-scheme';
                        document.head.appendChild(meta);
                    }
                    meta.content = 'light';
                    document.documentElement.setAttribute('data-cmux-browser-theme', 'light');
                })();
            "#
            }
            Self::Dark => {
                r#"
                (function() {
                    var meta = document.getElementById('cmux-browser-theme-meta');
                    if (!meta) {
                        meta = document.createElement('meta');
                        meta.id = 'cmux-browser-theme-meta';
                        meta.name = 'color-scheme';
                        document.head.appendChild(meta);
                    }
                    meta.content = 'dark';
                    document.documentElement.setAttribute('data-cmux-browser-theme', 'dark');
                })();
            "#
            }
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
    Startpage,
}

impl SearchEngine {
    /// Return the search URL template (query appended after).
    pub fn search_url(self, query: &str) -> String {
        let encoded = urlencoded(query);
        match self {
            Self::Google => format!("https://www.google.com/search?q={encoded}"),
            Self::DuckDuckGo => format!("https://duckduckgo.com/?q={encoded}"),
            Self::Bing => format!("https://www.bing.com/search?q={encoded}"),
            Self::Kagi => format!("https://kagi.com/search?q={encoded}"),
            Self::Startpage => format!("https://www.startpage.com/do/search?q={encoded}"),
        }
    }

    pub const ALL: &[Self] = &[
        Self::Google,
        Self::DuckDuckGo,
        Self::Bing,
        Self::Kagi,
        Self::Startpage,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Bing => "Bing",
            Self::Kagi => "Kagi",
            Self::Startpage => "Startpage",
        }
    }

    pub fn from_index(i: u32) -> Self {
        match i {
            0 => Self::Google,
            1 => Self::DuckDuckGo,
            2 => Self::Bing,
            3 => Self::Kagi,
            4 => Self::Startpage,
            _ => Self::DuckDuckGo,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Self::Google => 0,
            Self::DuckDuckGo => 1,
            Self::Bing => 2,
            Self::Kagi => 3,
            Self::Startpage => 4,
        }
    }
}

/// Sidebar focus style for selected workspace row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarFocusStyle {
    /// Solid accent background fill (default).
    #[default]
    SolidFill,
    /// Left accent rail indicator.
    LeftRail,
}

impl SidebarFocusStyle {
    pub const ALL: &[Self] = &[Self::SolidFill, Self::LeftRail];

    pub fn label(self) -> &'static str {
        match self {
            Self::SolidFill => "Solid Fill",
            Self::LeftRail => "Left Rail",
        }
    }

    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::LeftRail,
            _ => Self::SolidFill,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Self::SolidFill => 0,
            Self::LeftRail => 1,
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
    /// When true, all metadata (git, PR, ports, logs, progress, pills, notifications)
    /// is hidden. Individual toggles are ignored.
    pub hide_all_details: bool,
    /// When true, show each panel's git branch on its own line instead of the
    /// single workspace-level branch.
    pub branch_vertical_layout: bool,
    /// When true, show latest notification message below workspace title.
    /// When false, always show directory path.
    pub show_notification_message: bool,
    pub focus_style: SidebarFocusStyle,
    /// Sidebar width in pixels (0 = use default from libadwaita).
    pub width: u32,
    /// Sidebar tint color (CSS color string, e.g. "#1e1e2e" or "rgba(30,30,46,0.9)").
    /// Empty string means no custom tint. Used as fallback when light/dark variants
    /// are not set.
    pub tint_color: String,
    /// Sidebar tint color for light mode. Overrides `tint_color` when in light mode.
    pub tint_color_light: String,
    /// Sidebar tint color for dark mode. Overrides `tint_color` when in dark mode.
    pub tint_color_dark: String,
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
            hide_all_details: false,
            branch_vertical_layout: false,
            show_notification_message: true,
            focus_style: SidebarFocusStyle::default(),
            width: 0,
            tint_color: String::new(),
            tint_color_light: String::new(),
            tint_color_dark: String::new(),
        }
    }
}

impl SidebarDisplaySettings {}

/// Link routing — determines which URLs open in cmux browser vs system browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkRoutingSettings {
    /// Default target for terminal hyperlinks.
    pub default_target: LinkTarget,
    /// Regex patterns for URLs that should open in the system browser.
    /// Matched against the full URL string.
    pub external_patterns: Vec<String>,
}

/// Where to open links from terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkTarget {
    CmuxBrowser,
    SystemBrowser,
}

impl Default for LinkRoutingSettings {
    fn default() -> Self {
        Self {
            default_target: LinkTarget::CmuxBrowser,
            external_patterns: Vec::new(),
        }
    }
}

impl LinkRoutingSettings {
    /// Check if a URL should be opened externally based on patterns.
    /// Patterns are simple substring or glob-style matches:
    /// - Plain string: substring match (e.g. "zoom.us" matches any zoom URL)
    /// - Starts with `^`: prefix match after scheme (e.g. "^mail.google.com")
    pub fn should_open_externally(&self, url: &str) -> bool {
        if self.default_target == LinkTarget::SystemBrowser {
            return true;
        }
        let url_lower = url.to_lowercase();
        self.external_patterns.iter().any(|pattern| {
            let pat = pattern.to_lowercase();
            if let Some(prefix) = pat.strip_prefix('^') {
                // Match against host+path portion (after "://")
                url_lower
                    .find("://")
                    .map(|idx| url_lower[idx + 3..].starts_with(prefix.trim()))
                    .unwrap_or(false)
            } else {
                url_lower.contains(&pat)
            }
        })
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            focus_follows_mouse: false,
            confirm_before_close: true,
            new_workspace_placement: NewWorkspacePlacement::default(),
            notifications: NotificationSettings::default(),
            socket_access: SocketAccess::CmuxOnly,
            sidebar: SidebarDisplaySettings::default(),
            browser: BrowserSettings::default(),
            first_click_focus: false,
            pane_attention_ring: true,
            pane_flash_enabled: true,
            link_routing: LinkRoutingSettings::default(),
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
    let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    std::fs::write(path, json)?;

    shortcuts::save(&settings.shortcuts)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings_roundtrip() {
        let settings = AppSettings::default();
        let json = serde_json::to_string_pretty(&settings).unwrap();
        let parsed: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.theme, settings.theme);
    }

    #[test]
    fn test_malformed_json_returns_default() {
        let result: AppSettings = serde_json::from_str("not valid json").unwrap_or_default();
        assert_eq!(result.theme, ThemeMode::System);
    }

    #[test]
    fn test_unknown_fields_rejected() {
        let json = r#"{"bogus_field": true}"#;
        let result: Result<AppSettings, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown fields should be rejected");
    }

    #[test]
    fn test_partial_settings_merged_with_defaults() {
        let json = r#"{"theme": "dark"}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.theme, ThemeMode::Dark);
        assert!(settings.confirm_before_close); // default value
    }
}

fn load_main_settings() -> AppSettings {
    let path = config_dir().join("settings.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|err| {
            tracing::warn!("Failed to parse {}: {err}", path.display());
            AppSettings::default()
        }),
        Err(_) => AppSettings::default(),
    }
}

/// Percent-encode a string for safe embedding in URL query parameters.
/// Unreserved characters (RFC 3986) pass through; spaces become `+`;
/// everything else is percent-encoded.
pub(crate) fn urlencoded(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                format!("{}", b as char)
            }
            b' ' => "+".to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}
