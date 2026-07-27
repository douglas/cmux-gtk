//! Application settings — persistent configuration loaded from XDG config dir.

pub mod custom_commands;
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
    /// Link routing — which links open in jmux browser vs system browser.
    pub link_routing: LinkRoutingSettings,
    /// Enable remote SSH workspaces (off by default for security).
    pub remote_ssh_enabled: bool,
    /// Port range (inclusive) scanned on the remote host for the CLI relay's
    /// reverse-tunnel listener. The first free port in the range is used, so a
    /// range avoids collisions when several remote sessions are open at once.
    #[serde(default)]
    pub remote_relay_ports: RemotePortRange,
    /// Hide the titlebar (header bar) for a distraction-free terminal.
    pub minimal_mode: bool,
    /// Show the close (X) button on each tab. When false, tabs can only be
    /// closed via middle-click or the right-click context menu, preventing
    /// accidental closes.
    pub show_tab_close_button: bool,
    /// Persist terminal scrollback in session.json (default: true).
    /// Disable if terminal output may contain sensitive data (passwords, tokens).
    pub persist_scrollback: bool,
    /// Show confirmation dialog before closing a non-pinned workspace/tab.
    pub warn_before_closing_tab: bool,
    /// Copy terminal selection to clipboard automatically.
    pub copy_on_select: bool,
    /// Show confirmation dialog before quitting the app (Cmd/Ctrl+Q).
    pub confirm_quit: bool,
    /// Font size for the workspace tab bar labels (0.0 = system default).
    pub tab_bar_font_size: f32,
    /// Font size for the sidebar workspace list (0.0 = system default).
    pub sidebar_font_size: f32,
    /// New workspaces inherit the cwd from the active terminal panel.
    pub workspace_cwd_inheritance: bool,
    /// What the + button in the tab bar does.
    pub plus_button_action: PlusButtonAction,
    /// Persist split ratios (divider positions) in session.
    pub split_ratio_persist: bool,
    /// Agent session restore settings.
    pub agent_restore: AgentRestoreSettings,
    /// Resume commands that are pre-approved and skip any confirmation dialog.
    /// Any command whose prefix matches an entry here is launched without prompting.
    #[serde(default = "default_resume_command_approvals")]
    pub resume_command_approvals: Vec<String>,
    /// When true, render workspace log entries and metadata blocks as iMessage-style
    /// chat bubbles in the sidebar (applies globally; per-workspace toggle overrides).
    #[serde(default)]
    pub imessage_mode: bool,
    /// Path to the notes scratchpad file. Empty = default
    /// (`$XDG_DATA_HOME/jmux/notes.md`). Supports a leading `~`.
    #[serde(default)]
    pub notes_path: String,
    /// Show the TextBox prompt-composer below new terminals.
    #[serde(default)]
    pub show_textbox_on_new_terminals: bool,
    /// Focus the TextBox (instead of the terminal) when a terminal is created.
    #[serde(default)]
    pub focus_textbox_on_new_terminals: bool,
    /// Maximum visible lines in the TextBox before it scrolls.
    #[serde(default = "default_textbox_max_lines")]
    pub textbox_max_lines: u32,
    /// Show the Dock (right-side terminal controls from dock.json) on startup.
    /// Off by default — reveal it via the header button / palette / shortcut.
    #[serde(default)]
    pub show_dock: bool,
    /// Word-wrap long lines in the file preview / editor and notes panels.
    #[serde(default = "default_true")]
    pub editor_word_wrap: bool,
    /// What double-clicking a file in the sidebar file explorer does.
    #[serde(default)]
    pub file_explorer_open_action: FileOpenAction,
    /// Command for the "preferred editor" open action (e.g. "code", "nvim").
    /// `{path}` is substituted with the file path; if absent the path is appended.
    #[serde(default)]
    pub preferred_editor: String,
    /// Opt-in: auto-name a workspace from its agent transcript (via the Anthropic
    /// API, using ANTHROPIC_API_KEY) when an agent finishes, if it has no title.
    #[serde(default)]
    pub ai_auto_naming: bool,
    /// Opt-in: automatically open the read-only sub-agent monitor in a workspace
    /// while its Claude session is running sub-agents, and close it again once
    /// they finish — no manual toggle needed. When enabled the monitor's
    /// visibility tracks live sub-agents in every workspace.
    #[serde(default)]
    pub auto_subagent_monitor: bool,
    /// Goal-driven agent workspaces (`jmux goal`): runners + driver tuning.
    #[serde(default)]
    pub goal: GoalSettings,
    /// Quake-style drop-down "quick terminal" (slides in from the top edge,
    /// toggled by a global hotkey). Requires a `quick-terminal` feature build.
    #[serde(default)]
    pub quick_terminal: QuickTerminalSettings,
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

/// What the + button in the workspace tab bar does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlusButtonAction {
    /// Create a new workspace (default).
    #[default]
    NewWorkspace,
    /// Add a new tab (panel) to the current workspace.
    NewTab,
}

impl PlusButtonAction {
    pub const ALL: &[Self] = &[Self::NewWorkspace, Self::NewTab];

    pub fn label(self) -> &'static str {
        match self {
            Self::NewWorkspace => "New Workspace",
            Self::NewTab => "New Tab",
        }
    }

    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::NewTab,
            _ => Self::NewWorkspace,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Self::NewWorkspace => 0,
            Self::NewTab => 1,
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
    JmuxOnly,
    AllowAll,
}

/// Browser panel settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserSettings {
    /// Enable the browser panel engine at runtime.
    /// When false, browser panels show a placeholder instead of WebKit.
    #[serde(default = "bool_true")]
    pub enabled: bool,
    /// Default search engine for non-URL queries.
    pub search_engine: SearchEngine,
    /// Custom search URL template, used when `search_engine` is `Custom`.
    /// The query is substituted for `%s` (or appended if no placeholder).
    /// Example: `https://search.brave.com/search?q=%s`.
    #[serde(default)]
    pub custom_search_template: String,
    /// Extra search providers selectable by a keyword prefix in the omnibar.
    /// Typing `<keyword> terms` routes the search to that provider's template
    /// (e.g. keyword `gh` → `https://github.com/search?q=%s`).
    #[serde(default)]
    pub search_keywords: Vec<SearchKeyword>,
    /// Home page URL (shown when clicking home button).
    pub home_url: String,
    /// Show remote search suggestions in the omnibar.
    pub search_suggestions: bool,
    /// Hosts allowed to load over insecure HTTP without warning.
    #[serde(default)]
    pub http_allowlist: Vec<String>,
    /// Browser color scheme override (separate from app theme).
    pub browser_theme: BrowserThemeMode,
    /// Suspend hidden browser tabs after 60 seconds to free memory.
    /// When the tab is re-shown it reloads automatically.
    pub memory_saver_enabled: bool,
}

fn bool_true() -> bool {
    true
}

/// A keyword-triggered search provider. When the omnibar query begins with
/// `keyword ` (a space), the rest is searched via `url_template` (`%s` = query,
/// or appended if absent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchKeyword {
    pub keyword: String,
    pub url_template: String,
}

impl SearchKeyword {
    /// If `query` begins with this provider's keyword, return the resolved URL.
    pub fn try_resolve(&self, query: &str) -> Option<String> {
        let kw = self.keyword.trim();
        if kw.is_empty() {
            return None;
        }
        let rest = query.strip_prefix(kw)?.strip_prefix(' ')?;
        let encoded = urlencoded(rest.trim());
        Some(if self.url_template.contains("%s") {
            self.url_template.replace("%s", &encoded)
        } else {
            format!("{}{}", self.url_template, encoded)
        })
    }
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            search_engine: SearchEngine::DuckDuckGo,
            custom_search_template: String::new(),
            search_keywords: Vec::new(),
            home_url: "https://duckduckgo.com".to_string(),
            search_suggestions: true,
            http_allowlist: Vec::new(),
            browser_theme: BrowserThemeMode::System,
            memory_saver_enabled: true,
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
                    var meta = document.getElementById('jmux-browser-theme-meta');
                    if (meta) meta.remove();
                    document.documentElement.removeAttribute('data-jmux-browser-theme');
                })();
            "#
            }
            Self::Light => {
                r#"
                (function() {
                    var meta = document.getElementById('jmux-browser-theme-meta');
                    if (!meta) {
                        meta = document.createElement('meta');
                        meta.id = 'jmux-browser-theme-meta';
                        meta.name = 'color-scheme';
                        document.head.appendChild(meta);
                    }
                    meta.content = 'light';
                    document.documentElement.setAttribute('data-jmux-browser-theme', 'light');
                })();
            "#
            }
            Self::Dark => {
                r#"
                (function() {
                    var meta = document.getElementById('jmux-browser-theme-meta');
                    if (!meta) {
                        meta = document.createElement('meta');
                        meta.id = 'jmux-browser-theme-meta';
                        meta.name = 'color-scheme';
                        document.head.appendChild(meta);
                    }
                    meta.content = 'dark';
                    document.documentElement.setAttribute('data-jmux-browser-theme', 'dark');
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
    /// User-defined provider; URL template stored in `BrowserSettings::custom_search_template`.
    Custom,
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
            Self::Custom => {
                // Read the configured template on demand so this method keeps its
                // simple `(self, query)` signature used across the codebase.
                let template = load().browser.custom_search_template;
                if template.trim().is_empty() {
                    // No template configured — fall back to DuckDuckGo.
                    format!("https://duckduckgo.com/?q={encoded}")
                } else if template.contains("%s") {
                    template.replace("%s", &encoded)
                } else {
                    format!("{template}{encoded}")
                }
            }
        }
    }

    pub const ALL: &[Self] = &[
        Self::Google,
        Self::DuckDuckGo,
        Self::Bing,
        Self::Kagi,
        Self::Startpage,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Bing => "Bing",
            Self::Kagi => "Kagi",
            Self::Startpage => "Startpage",
            Self::Custom => "Custom",
        }
    }

    pub fn from_index(i: u32) -> Self {
        match i {
            0 => Self::Google,
            1 => Self::DuckDuckGo,
            2 => Self::Bing,
            3 => Self::Kagi,
            4 => Self::Startpage,
            5 => Self::Custom,
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
            Self::Custom => 5,
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
    /// Opacity applied to the sidebar tint color (0.0–1.0). Default: 0.85.
    #[serde(default = "default_tint_opacity")]
    pub tint_opacity: f32,
    /// When true, port badge clicks open localhost URLs in the system browser
    /// instead of the jmux built-in browser panel.
    pub port_link_external: bool,
    /// Custom CSS color for the selected workspace row background.
    /// Empty string means use the default accent color.
    pub selection_color: String,
    /// When true, use the ghostty terminal background color as the sidebar background tint.
    pub match_terminal_background: bool,
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
            tint_opacity: default_tint_opacity(),
            port_link_external: false,
            selection_color: String::new(),
            match_terminal_background: false,
        }
    }
}

impl SidebarDisplaySettings {}

/// Link routing — determines which URLs open in jmux browser vs system browser.
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
    JmuxBrowser,
    SystemBrowser,
}

impl Default for LinkRoutingSettings {
    fn default() -> Self {
        Self {
            default_target: LinkTarget::JmuxBrowser,
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

/// Per-agent session restore toggles.
///
/// When a panel is restored and an agent resume command was detected at save time,
/// jmux will use that command to re-launch the agent only if the corresponding
/// toggle is enabled.  All agents default to enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentRestoreSettings {
    /// Restore Claude Code sessions on launch (`claude --continue`).
    pub claude_code: bool,
    /// Restore Codex CLI sessions on launch (`codex`).
    pub codex: bool,
    /// Restore OpenCode sessions on launch (`opencode --resume`).
    pub opencode: bool,
    /// Restore Gemini CLI sessions on launch (`gemini`).
    pub gemini: bool,
    /// Restore Rovo Dev sessions on launch (`rovo dev`).
    pub rovo_dev: bool,
    /// Restore Cursor sessions on launch (`cursor`).
    #[serde(default)]
    pub cursor: bool,
    /// Restore Grok Build CLI sessions on launch (`grok`).
    #[serde(default)]
    pub grok: bool,
    /// Restore Amp sessions on launch (`amp`).
    #[serde(default)]
    pub amp: bool,
    /// Restore Pi Vault sessions on launch (`pi`).
    #[serde(default)]
    pub pi: bool,
    /// Restore Hermes sessions on launch (`hermes`).
    #[serde(default)]
    pub hermes: bool,
    /// Restore Antigravity sessions on launch (`antigravity`).
    #[serde(default)]
    pub antigravity: bool,
}

impl Default for AgentRestoreSettings {
    fn default() -> Self {
        Self {
            claude_code: true,
            codex: true,
            opencode: true,
            gemini: true,
            rovo_dev: true,
            cursor: true,
            grok: true,
            amp: true,
            pi: true,
            hermes: true,
            antigravity: true,
        }
    }
}

impl AgentRestoreSettings {
    /// Return `true` if the given agent resume command should be used on restore.
    /// The `resume_cmd` is matched by prefix against the known agent commands.
    pub fn is_enabled_for(&self, resume_cmd: &str) -> bool {
        if resume_cmd.starts_with("claude") {
            self.claude_code
        } else if resume_cmd.starts_with("opencode") {
            self.opencode
        } else if resume_cmd.starts_with("codex") {
            self.codex
        } else if resume_cmd.starts_with("gemini") {
            self.gemini
        } else if resume_cmd.starts_with("rovo") {
            self.rovo_dev
        } else if resume_cmd.starts_with("cursor") {
            self.cursor
        } else if resume_cmd.starts_with("grok") {
            self.grok
        } else if resume_cmd.starts_with("amp") {
            self.amp
        } else if resume_cmd.starts_with("pi") {
            self.pi
        } else if resume_cmd.starts_with("hermes") {
            self.hermes
        } else if resume_cmd.starts_with("antigravity") {
            self.antigravity
        } else {
            false
        }
    }
}

fn default_tint_opacity() -> f32 {
    0.85
}

fn default_textbox_max_lines() -> u32 {
    6
}

fn default_true() -> bool {
    true
}

/// Quake-style drop-down quick terminal configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QuickTerminalSettings {
    /// Enable the quick terminal (only effective in a `quick-terminal` build).
    pub enabled: bool,
    /// Suggested global-shortcut trigger registered with the GlobalShortcuts
    /// portal (xdg shortcut syntax, e.g. "CTRL+grave", "F12"). The user can
    /// rebind it in their desktop's system settings.
    pub hotkey: String,
    /// Drop-down height as a fraction of the monitor height (0.1–1.0).
    pub height_fraction: f32,
    /// Slide animation duration in milliseconds.
    pub animation_ms: u32,
}

impl Default for QuickTerminalSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: "CTRL+grave".to_string(),
            height_fraction: 0.45,
            animation_ms: 150,
        }
    }
}

/// Action taken when a file is double-clicked in the sidebar file explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOpenAction {
    /// Open in jmux's inline file preview / markdown viewer.
    Preview,
    /// Open with the system default application (`xdg-open`). The default.
    #[default]
    DefaultApp,
    /// Open with the configured `preferred_editor` command.
    PreferredEditor,
}

impl FileOpenAction {
    pub fn from_index(i: u32) -> Self {
        match i {
            0 => Self::Preview,
            2 => Self::PreferredEditor,
            _ => Self::DefaultApp,
        }
    }
    pub fn to_index(self) -> u32 {
        match self {
            Self::Preview => 0,
            Self::DefaultApp => 1,
            Self::PreferredEditor => 2,
        }
    }
}

fn default_resume_command_approvals() -> Vec<String> {
    vec![
        "claude --continue".to_string(),
        "codex".to_string(),
        "opencode --resume".to_string(),
        "gemini".to_string(),
        "rovo dev".to_string(),
    ]
}

impl AppSettings {
    /// Return `true` if `cmd` is in the pre-approved resume commands list.
    ///
    /// Matching is prefix-based so that `"codex"` matches `"codex --dir /foo"`.
    #[allow(dead_code)]
    pub fn is_resume_command_approved(&self, cmd: &str) -> bool {
        self.resume_command_approvals
            .iter()
            .any(|approved| cmd == approved || cmd.starts_with(&format!("{approved} ")))
    }
}

/// Inclusive port range scanned on the remote host for the CLI relay tunnel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RemotePortRange {
    pub start: u16,
    pub end: u16,
}

impl Default for RemotePortRange {
    fn default() -> Self {
        Self {
            start: 10000,
            end: 10100,
        }
    }
}

impl RemotePortRange {
    /// Normalized (low, high) bounds, guarding against a misconfigured range
    /// where `end < start` by falling back to the default span.
    pub fn bounds(&self) -> (u16, u16) {
        if self.end >= self.start {
            (self.start, self.end)
        } else {
            let d = Self::default();
            (d.start, d.end)
        }
    }
}

/// A named goal runner: which agent CLI + model + effort executes a goal
/// (see docs/roadmap/DESIGN-goal-graph.md).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct GoalRunner {
    /// Adapter: "claude" (default — full state detection + sub-agent
    /// mirroring) or "custom" (uses `command_template`).
    pub agent: String,
    /// Model passed to the agent CLI (empty = the CLI's default).
    pub model: String,
    /// Effort level passed to the agent CLI (empty = default).
    pub effort: String,
    /// Extra args appended to the claude command line.
    pub extra_args: Vec<String>,
    /// For `agent = "custom"`: the full launch command. Placeholders:
    /// `{sid}`, `{model}`, `{effort}`, `{seed}` (shell-quoted seed text),
    /// `{seed_file}` (shell-quoted path to a file containing the seed).
    pub command_template: String,
    /// "claude" | "none" — whether ClaudeState screen classification (and
    /// nudging) applies. Empty = "claude" for the claude adapter, "none"
    /// for custom runners.
    pub state_detection: String,
}

/// Settings for goal-driven agent workspaces (`jmux goal`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct GoalSettings {
    /// Runner used when a goal doesn't name one. An empty/unknown name
    /// falls back to a stock claude runner.
    pub default_runner: String,
    /// Named runners selectable via `jmux goal --runner <name>` or per
    /// graph node.
    pub runners: std::collections::HashMap<String, GoalRunner>,
    /// Consecutive idle driver ticks (2 s each) before a nudge is sent.
    pub idle_ticks_before_nudge: u32,
    /// Nudges sent before the driver escalates instead.
    pub nudge_budget: u32,
    /// Per-goal wall-clock cap in minutes (0 = uncapped).
    pub wall_clock_minutes: u32,
}

impl Default for GoalSettings {
    fn default() -> Self {
        Self {
            default_runner: String::new(),
            runners: std::collections::HashMap::new(),
            idle_ticks_before_nudge: 5,
            nudge_budget: 3,
            wall_clock_minutes: 120,
        }
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
            socket_access: SocketAccess::JmuxOnly,
            sidebar: SidebarDisplaySettings::default(),
            browser: BrowserSettings::default(),
            first_click_focus: false,
            pane_attention_ring: true,
            pane_flash_enabled: true,
            link_routing: LinkRoutingSettings::default(),
            remote_ssh_enabled: false,
            remote_relay_ports: RemotePortRange::default(),
            minimal_mode: false,
            show_tab_close_button: true,
            persist_scrollback: true,
            warn_before_closing_tab: true,
            copy_on_select: false,
            confirm_quit: true,
            tab_bar_font_size: 0.0,
            sidebar_font_size: 0.0,
            workspace_cwd_inheritance: true,
            plus_button_action: PlusButtonAction::default(),
            split_ratio_persist: true,
            agent_restore: AgentRestoreSettings::default(),
            resume_command_approvals: default_resume_command_approvals(),
            imessage_mode: false,
            notes_path: String::new(),
            show_textbox_on_new_terminals: false,
            focus_textbox_on_new_terminals: false,
            textbox_max_lines: default_textbox_max_lines(),
            show_dock: false,
            editor_word_wrap: true,
            file_explorer_open_action: FileOpenAction::default(),
            preferred_editor: String::new(),
            ai_auto_naming: false,
            auto_subagent_monitor: false,
            goal: GoalSettings::default(),
            quick_terminal: QuickTerminalSettings::default(),
            shortcuts: shortcuts::ShortcutConfig::default(),
        }
    }
}

/// Get the settings directory path (~/.config/jmux/).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("jmux")
}

/// Return the active config file path.
///
/// Prefers `~/.config/jmux/jmux.json`; falls back to `~/.config/jmux/settings.json`.
pub fn active_config_path() -> PathBuf {
    let dir = config_dir();
    let jmux_json = dir.join("jmux.json");
    if jmux_json.exists() {
        jmux_json
    } else {
        dir.join("settings.json")
    }
}

/// Parsed-settings cache, validated by file mtimes. `load()` is called from
/// hot paths (sidebar refresh, content rebuild — several times per second
/// while an agent streams title updates), and re-reading + re-parsing two
/// JSON files each time dominated those paths. A stat per file per call is
/// enough to keep external edits, `save()`, and SIGUSR2 reloads all working:
/// any write bumps the mtime and the next `load()` re-parses.
struct SettingsCache {
    settings: AppSettings,
    main_path: PathBuf,
    main_mtime: Option<std::time::SystemTime>,
    shortcuts_mtime: Option<std::time::SystemTime>,
}

static SETTINGS_CACHE: std::sync::Mutex<Option<SettingsCache>> = std::sync::Mutex::new(None);

fn mtime_of(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Load settings from disk. Returns defaults if file doesn't exist.
pub fn load() -> AppSettings {
    let main_path = active_config_path();
    let main_mtime = mtime_of(&main_path);
    let shortcuts_mtime = mtime_of(&config_dir().join("shortcuts.json"));

    let mut cache = match SETTINGS_CACHE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(c) = cache.as_ref() {
        if c.main_path == main_path
            && c.main_mtime == main_mtime
            && c.shortcuts_mtime == shortcuts_mtime
            && main_mtime.is_some()
        {
            return c.settings.clone();
        }
    }

    let mut settings = load_main_settings();
    settings.shortcuts = shortcuts::load();
    *cache = Some(SettingsCache {
        settings: settings.clone(),
        main_path,
        main_mtime,
        shortcuts_mtime,
    });
    settings
}

/// Save settings to disk.
///
/// Writes to `jmux.json` if that file exists (or if neither file exists, creating
/// `jmux.json` as the canonical new format); writes to `settings.json` only when
/// the user has a `settings.json` and no `jmux.json` (backward compatibility).
pub fn save(settings: &AppSettings) -> Result<(), std::io::Error> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }

    let path = active_config_path();
    let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        f.write_all(json.as_bytes())?;
    }

    shortcuts::save(&settings.shortcuts)?;
    Ok(())
}

/// Strip JSONC comments from a string, returning clean JSON.
///
/// Handles:
/// - Line comments: `// ...` (not inside strings)
/// - Block comments: `/* ... */` (not inside strings, may span multiple lines)
/// - Strings with escaped quotes (`\"`) are tracked correctly.
pub fn strip_jsonc_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                // Escaped character — consume next char verbatim to avoid treating
                // `\"` as a string terminator.
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '"' => {
                    in_string = true;
                    out.push(c);
                }
                '/' => match chars.peek() {
                    Some('/') => {
                        // Line comment — consume until end of line.
                        chars.next();
                        for ch in chars.by_ref() {
                            if ch == '\n' {
                                out.push('\n');
                                break;
                            }
                        }
                    }
                    Some('*') => {
                        // Block comment — consume until `*/`.
                        chars.next();
                        let mut prev = '\0';
                        for ch in chars.by_ref() {
                            if prev == '*' && ch == '/' {
                                break;
                            }
                            // Preserve newlines so line numbers stay accurate for error messages.
                            if ch == '\n' {
                                out.push('\n');
                            }
                            prev = ch;
                        }
                    }
                    _ => {
                        out.push(c);
                    }
                },
                _ => {
                    out.push(c);
                }
            }
        }
    }

    out
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

    #[test]
    fn test_strip_jsonc_line_comment() {
        let input = r#"{"key": "value"} // line comment"#;
        let output = strip_jsonc_comments(input);
        assert!(output.contains(r#""key": "value""#));
        assert!(!output.contains("line comment"));
    }

    #[test]
    fn test_strip_jsonc_block_comment() {
        let input = r#"{"key": /* block */ "value"}"#;
        let output = strip_jsonc_comments(input);
        assert!(output.contains(r#""value""#));
        assert!(!output.contains("block"));
    }

    #[test]
    fn test_strip_jsonc_url_in_string_not_stripped() {
        // `//` inside a string must NOT be treated as a line comment.
        let input = r#"{"url": "http://example.com // not a comment"}"#;
        let output = strip_jsonc_comments(input);
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            v["url"].as_str().unwrap(),
            "http://example.com // not a comment"
        );
    }

    #[test]
    fn test_strip_jsonc_escaped_quote_in_string() {
        let input = r#"{"msg": "he said \"hello\" // not comment"}"#;
        let output = strip_jsonc_comments(input);
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["msg"].as_str().unwrap(), r#"he said "hello" // not comment"#);
    }

    #[test]
    fn test_strip_jsonc_multiline_block_comment() {
        let input = "{\n  /* multi\n     line */\n  \"k\": 1\n}";
        let output = strip_jsonc_comments(input);
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["k"].as_i64().unwrap(), 1);
    }
}

fn load_main_settings() -> AppSettings {
    let path = active_config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let clean = strip_jsonc_comments(&content);
            serde_json::from_str(&clean).unwrap_or_else(|err| {
                tracing::warn!("Failed to parse {}: {err}", path.display());
                AppSettings::default()
            })
        }
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
