//! CSS installation and git branch detection helpers.

use crate::model::panel::GitBranch;

pub(super) fn install_css() {
    // Ensure Adwaita legacy icons (terminal, etc.) resolve on all systems,
    // and add bundled jmux icons (globe, etc.).
    if let Some(display) = gtk4::gdk::Display::default() {
        let icon_theme = gtk4::IconTheme::for_display(&display);
        icon_theme.add_search_path("/usr/share/icons/Adwaita");

        // Bundled icons ship next to the binary at ../icons (dev) or alongside the crate source.
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        if let Some(dir) = exe_dir {
            let bundled = dir.join("../../jmux/icons");
            if bundled.exists() {
                icon_theme.add_search_path(bundled.to_string_lossy().as_ref());
            }
        }
        // Also check the compile-time manifest dir (works in `cargo run`).
        let manifest_icons = concat!(env!("CARGO_MANIFEST_DIR"), "/icons");
        icon_theme.add_search_path(manifest_icons);
    }

    let provider = gtk4::CssProvider::new();
    provider.load_from_data(
        "
        /* ── Quick-terminal resize grip (bottom border drag handle) ── */
        .quick-terminal-resize-grip {
            background: alpha(@window_fg_color, 0.10);
            min-height: 6px;
        }
        .quick-terminal-resize-grip:hover {
            background: alpha(@accent_color, 0.45);
        }

        /* ── Workspace rows ── */
        .workspace-row {
            border-radius: 8px;
            margin: 1px 4px;
        }

        /* ── Workspace group header ── */
        .workspace-group-header {
            border-radius: 6px;
            margin: 2px 4px 0px 4px;
            opacity: 0.85;
        }
        .workspace-group-header:hover {
            opacity: 1.0;
            background: alpha(@window_fg_color, 0.06);
        }

        /* ── Workspaces nested inside a group are slightly indented ── */
        .workspace-row-grouped {
            margin-left: 12px;
        }

        .workspace-row-colored {
            border-radius: 8px;
            border-left: 4px solid transparent;
            padding-left: 0px;
            margin: 1px 4px;
        }

        /* ── Workspace title — bolder, slightly larger ── */
        .workspace-title {
            font-weight: 600;
            font-size: 1.05em;
        }

        /* ── Index label — tabular numerals ── */
        .workspace-index {
            font-variant-numeric: tabular-nums;
            min-width: 1em;
        }

        /* ── Workspace type icon ── */
        .workspace-type-icon {
            opacity: 0.7;
        }

        /* ── Hover highlight on rows ── */
        .workspace-row:hover,
        .workspace-row-colored:hover {
            background-color: alpha(@theme_fg_color, 0.04);
        }

        /* ── Selected row — solid accent highlight with white text (default) ── */
        .navigation-sidebar row:selected {
            background-color: @accent_bg_color;
            color: white;
        }

        /* ── Left-rail variant — accent left border, no background fill ── */
        .sidebar-left-rail row:selected {
            background-color: alpha(@accent_bg_color, 0.12);
            color: @theme_fg_color;
            border-left: 3px solid @accent_bg_color;
        }
        .sidebar-left-rail row:selected .workspace-title {
            color: @theme_fg_color;
        }
        .sidebar-left-rail row:selected .dim-label,
        .sidebar-left-rail row:selected .caption {
            color: alpha(@theme_fg_color, 0.6);
        }
        .navigation-sidebar row:selected .workspace-title {
            color: white;
        }
        .navigation-sidebar row:selected .workspace-type-icon {
            opacity: 0.9;
        }
        .navigation-sidebar row:selected .dim-label,
        .navigation-sidebar row:selected .caption {
            color: rgba(255, 255, 255, 0.8);
        }
        .navigation-sidebar row:selected .sidebar-notification {
            color: rgba(255, 255, 255, 0.95);
        }
        .navigation-sidebar row:selected .status-pill,
        .navigation-sidebar row:selected .status-pill-blue,
        .navigation-sidebar row:selected .status-pill-green,
        .navigation-sidebar row:selected .status-pill-red,
        .navigation-sidebar row:selected .status-pill-orange,
        .navigation-sidebar row:selected .status-pill-purple,
        .navigation-sidebar row:selected .status-pill-yellow {
            background-color: rgba(255, 255, 255, 0.18);
            color: rgba(255, 255, 255, 0.95);
        }
        .navigation-sidebar row:selected .port-badge {
            background-color: rgba(255, 255, 255, 0.15);
            color: rgba(255, 255, 255, 0.85);
        }
        .navigation-sidebar row:selected .log-info,
        .navigation-sidebar row:selected .log-warning,
        .navigation-sidebar row:selected .log-error,
        .navigation-sidebar row:selected .log-success,
        .navigation-sidebar row:selected .log-progress {
            color: rgba(255, 255, 255, 0.8);
        }
        .navigation-sidebar row:selected .sidebar-progress progress {
            background-color: rgba(255, 255, 255, 0.8);
        }
        .navigation-sidebar row:selected .sidebar-progress trough {
            background-color: rgba(255, 255, 255, 0.15);
        }

        /* ── Split handle — thin like macOS ── */
        paned > separator {
            min-width: 1px;
            min-height: 1px;
            background-color: alpha(@theme_fg_color, 0.12);
        }

        /* ── Pane tab bar ── */
        .pane-tab-bar {
            background-color: alpha(@headerbar_bg_color, 0.95);
            border-bottom: 1px solid alpha(@theme_fg_color, 0.1);
            padding: 1px 4px;
        }
        .pane-tab {
            border-radius: 8px;
            padding: 3px 10px;
            color: alpha(@theme_fg_color, 0.55);
            border: 1px solid transparent;
            margin: 3px 1px;
        }
        .pane-tab:hover {
            background-color: alpha(@theme_fg_color, 0.08);
            border-color: alpha(@theme_fg_color, 0.08);
        }
        .pane-tab-selected {
            background-color: alpha(@theme_fg_color, 0.10);
            color: @theme_fg_color;
            border-color: alpha(@theme_fg_color, 0.15);
        }
        .pane-tab-attention {
            background-color: alpha(@accent_bg_color, 0.15);
            color: @accent_color;
            border-color: alpha(@accent_bg_color, 0.30);
        }
        .pane-tab-close {
            min-width: 14px;
            min-height: 14px;
            padding: 0;
            opacity: 0.5;
        }
        .pane-tab-close:hover {
            opacity: 1;
        }
        .pane-tab-action {
            min-width: 18px;
            min-height: 18px;
            padding: 1px;
            opacity: 0.55;
            border-radius: 0;
        }
        .pane-tab-action:hover {
            opacity: 1;
        }

        /* ── Browser toolbar ── */
        .browser-nav-bar button {
            background: none;
            border: none;
            box-shadow: none;
            min-width: 24px;
            min-height: 24px;
            padding: 4px;
            opacity: 0.7;
        }
        .browser-nav-bar button:hover {
            opacity: 1;
            background-color: alpha(@theme_fg_color, 0.08);
            border-radius: 6px;
        }
        .browser-nav-bar button:disabled {
            opacity: 0.3;
        }
        .browser-url-entry {
            background-color: alpha(@theme_fg_color, 0.06);
            border: none;
            border-radius: 6px;
            padding: 4px 8px;
            min-height: 24px;
        }

        /* ── Omnibar ghost text (inline completion) ── */
        .omnibar-ghost {
            color: alpha(@theme_fg_color, 0.3);
            padding: 4px 8px;
            min-height: 24px;
        }

        /* ── Metadata link style ── */
        .meta-link {
            text-decoration: underline;
        }

        .sidebar-notification {
            color: @accent_color;
            font-weight: 600;
        }

        /* ── Status pills ── */
        .status-pill {
            border-radius: 8px;
            padding: 1px 6px;
            font-size: 0.8em;
            font-weight: 600;
            background-color: alpha(@accent_color, 0.15);
            color: @accent_color;
        }

        .status-pill-blue {
            background-color: alpha(#3584e4, 0.15);
            color: #3584e4;
        }

        .status-pill-green {
            background-color: alpha(#33d17a, 0.15);
            color: #26a269;
        }

        .status-pill-red {
            background-color: alpha(#e01b24, 0.15);
            color: #e01b24;
        }

        .status-pill-orange {
            background-color: alpha(#ff7800, 0.15);
            color: #e66100;
        }

        .status-pill-purple {
            background-color: alpha(#9141ac, 0.15);
            color: #9141ac;
        }

        .status-pill-yellow {
            background-color: alpha(#f6d32d, 0.2);
            color: #986a44;
        }

        /* ── Colour swatch grid (Set Color menu) ── */
        .color-swatch-grid flowboxchild {
            padding: 0;
            min-width: 0;
            min-height: 0;
        }

        .color-swatch {
            min-width: 20px;
            min-height: 20px;
            padding: 0;
            border-radius: 50%;
            border: 1px solid alpha(@theme_fg_color, 0.25);
            box-shadow: none;
            background-image: none;
        }

        .color-swatch:hover {
            border-color: @theme_fg_color;
        }

        /* Swatch fill colours as static classes so each swatch just adds a
           class instead of installing its own CssProvider (per-widget
           providers trigger a global style revalidation and made the Set
           Color popup sluggish). Keep in sync with color_css_value(). */
        .color-swatch-red    { background-color: #e01b24; }
        .color-swatch-crimson{ background-color: #dc143c; }
        .color-swatch-orange { background-color: #ff7800; }
        .color-swatch-amber  { background-color: #ffbf00; }
        .color-swatch-yellow { background-color: #f6d32d; }
        .color-swatch-lime   { background-color: #a3be8c; }
        .color-swatch-green  { background-color: #33d17a; }
        .color-swatch-teal   { background-color: #2aa198; }
        .color-swatch-cyan   { background-color: #00bcd4; }
        .color-swatch-sky    { background-color: #87ceeb; }
        .color-swatch-blue   { background-color: #3584e4; }
        .color-swatch-indigo { background-color: #4b0082; }
        .color-swatch-purple { background-color: #9141ac; }
        .color-swatch-violet { background-color: #7c3aed; }
        .color-swatch-pink   { background-color: #e91e8c; }
        .color-swatch-rose   { background-color: #f43f5e; }

        /* No-colour swatch — hollow with a diagonal strike. */
        .color-swatch-none {
            background-color: transparent;
            background-image: linear-gradient(
                to bottom right,
                transparent calc(50% - 1px),
                #e01b24 calc(50% - 1px),
                #e01b24 calc(50% + 1px),
                transparent calc(50% + 1px));
        }

        /* ── Graph panel (jmux graph control pane) ── */
        .graph-panel .graph-status {
            padding: 1px 10px;
            border-radius: 999px;
            font-size: 0.8em;
            font-weight: 700;
        }
        .graph-status-running  { background: alpha(#3584e4, 0.2); color: #7db6f5; }
        .graph-status-done     { background: alpha(#33d17a, 0.18); color: #6fe0a0; }
        .graph-status-review   { background: alpha(#e5a50a, 0.2); color: #f2c14e; }
        .graph-status-paused   { background: alpha(#e01b24, 0.16); color: #ff8888; }

        .graph-chip {
            padding: 2px 10px;
            border-radius: 8px;
            font-size: 0.9em;
            font-weight: 600;
            border: 1px solid alpha(@theme_fg_color, 0.2);
            background-image: none;
        }
        .graph-chip-selected {
            background-color: alpha(@theme_fg_color, 0.14);
        }
        .graph-detail {
            padding: 10px;
            border-radius: 10px;
            border: 1px solid alpha(@theme_fg_color, 0.18);
            background-color: alpha(@theme_fg_color, 0.04);
        }
        .graph-goal-editor {
            padding: 6px;
        }
        .graph-chip-pending     { opacity: 0.55; }
        .graph-chip-running     { border-color: alpha(#3584e4, 0.8); }
        .graph-chip-review      { border-color: alpha(#e5a50a, 0.8); }
        .graph-chip-done        { border-color: alpha(#33d17a, 0.7); }
        .graph-chip-blocked     { border-color: alpha(#e01b24, 0.7); }
        .graph-chip-interrupted { border-color: alpha(#e01b24, 0.45); opacity: 0.7; }

        /* ── Progress bar (capsule style) ── */
        .sidebar-progress {
            min-height: 3px;
            border-radius: 1.5px;
        }

        .sidebar-progress trough {
            min-height: 3px;
            border-radius: 1.5px;
            background-color: alpha(@theme_fg_color, 0.12);
        }

        .sidebar-progress progress {
            min-height: 3px;
            border-radius: 1.5px;
            background-color: @accent_bg_color;
        }

        /* ── Log entry levels ── */
        .log-info {
            color: alpha(@theme_fg_color, 0.55);
        }

        .log-warning {
            color: #e66100;
        }

        .log-error {
            color: #e01b24;
        }

        .log-success {
            color: #26a269;
        }

        .log-progress {
            color: @accent_color;
        }

        /* ── Port badges ── */
        .port-badge {
            border-radius: 6px;
            padding: 0px 4px;
            font-size: 0.75em;
            font-weight: 600;
            background-color: alpha(@theme_fg_color, 0.08);
            color: alpha(@theme_fg_color, 0.6);
        }

        /* ── Panel shell ── */
        .panel-shell {
            border: 1px solid alpha(@theme_fg_color, 0.12);
            border-radius: 0;
            padding: 2px;
        }

        .attention-panel {
            border: 2px solid @accent_bg_color;
            background-color: alpha(@accent_bg_color, 0.08);
        }

        /* ── Vim copy-mode badge ── */
        .vim-badge {
            background-color: alpha(#33d17a, 0.85);
            color: white;
            font-size: 0.75em;
            font-weight: 700;
            border-radius: 4px;
            padding: 2px 8px;
            opacity: 0.9;
        }

        /* ── Sub-agent monitor read-only badge ── */
        .agent-monitor-badge {
            background-color: alpha(@accent_bg_color, 0.85);
            color: white;
            font-size: 0.72em;
            font-weight: 700;
            border-radius: 4px;
            padding: 1px 7px;
            opacity: 0.85;
        }

        /* ── Search overlay ── */
        .search-overlay {
            background-color: @theme_bg_color;
            border: 1px solid alpha(@theme_fg_color, 0.15);
            border-radius: 8px;
            padding: 4px 8px;
            box-shadow: 0 2px 8px alpha(black, 0.15);
        }

        /* ── Notification panel ── */
        .notification-row {
            padding: 8px 12px;
        }

        .notification-row-unread {
            background-color: alpha(@accent_color, 0.06);
        }

        .notification-title {
            font-weight: 600;
        }

        .notification-timestamp {
            color: alpha(@theme_fg_color, 0.45);
            font-size: 0.85em;
        }

        /* ── Inactive pane overlay ── */
        .inactive-pane-overlay {
            background-color: alpha(black, 0.12);
        }

        /* ── Dock panel ── */
        .dock-panel {
            border-left: 1px solid alpha(@borders, 0.8);
        }
        .dock-control {
            border-top: 1px solid alpha(@borders, 0.5);
        }
        .dock-control-title {
            opacity: 0.7;
        }
        /* ── TextBox composer ── */
        .textbox-composer {
            border-top: 1px solid alpha(@borders, 0.6);
        }

        /* ── Pane overview ── */
        .overview-tile-button {
            padding: 10px;
        }
        .overview-tile-focused {
            border: 2px solid alpha(@accent_color, 0.8);
        }
        .overview-ws-current {
            background: alpha(@accent_color, 0.15);
            color: @accent_color;
            border-radius: 99px;
            padding: 1px 8px;
            font-size: 0.8em;
        }
        .overview-dot-busy { color: @success_color; }
        .overview-dot-idle { color: alpha(@window_fg_color, 0.4); }
        .overview-dot-attention { color: @accent_color; }
        .overview-dot-browser { color: alpha(@accent_color, 0.7); }

        /* ── Focused panel indicator ── */
        .focused-panel {
            border-color: alpha(@accent_color, 0.5);
        }

        /* ── Flash panel ── */
        .flash-panel {
            background-color: alpha(@accent_color, 0.25);
        }

        /* ── Command palette ── */
        .command-palette {
            background-color: @theme_bg_color;
            border: 1px solid alpha(@theme_fg_color, 0.15);
            border-radius: 12px;
            box-shadow: 0 8px 32px alpha(black, 0.3);
        }

        /* ── Sidebar close button ── */
        .sidebar-close-btn {
            min-width: 16px;
            min-height: 16px;
            padding: 0;
        }

        /* ── Remote workspace status icons ── */
        .remote-connected {
            color: #26a269;
        }

        .remote-connecting {
            color: @accent_color;
            opacity: 0.8;
        }

        .remote-error {
            color: #e01b24;
        }

        .remote-disconnected {
            color: alpha(@theme_fg_color, 0.4);
        }

        /* ── Confirmation dialogs — square corners ── */
        .jmux-confirm-dialog,
        .jmux-confirm-dialog > .background,
        .jmux-confirm-dialog .dialog-content,
        .jmux-confirm-dialog > decoration,
        .jmux-confirm-dialog > decoration-overlay {
            border-radius: 0;
        }
        ",
    );

    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // Channel-build accent override: JMUX_CHANNEL=insiders → amber tint,
    // JMUX_CHANNEL=nightly → purple tint, so testers can tell builds apart.
    let channel = std::env::var("JMUX_CHANNEL")
        .unwrap_or_default()
        .to_lowercase();
    let channel_css = match channel.as_str() {
        "insiders" => Some(
            "@define-color accent_bg_color #e6a817;
             @define-color accent_color #c48b00;",
        ),
        "nightly" => Some(
            "@define-color accent_bg_color #9141ac;
             @define-color accent_color #7b35a0;",
        ),
        _ => None,
    };
    if let Some(css) = channel_css {
        let channel_provider = gtk4::CssProvider::new();
        channel_provider.load_from_data(css);
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &channel_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
    }
}

/// Detect git branch and dirty state from a directory path.
pub(super) fn detect_git_branch(directory: &str) -> Option<GitBranch> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(directory)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        return None;
    }

    let is_dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "-uno"])
        .current_dir(directory)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    Some(GitBranch { branch, is_dirty })
}
