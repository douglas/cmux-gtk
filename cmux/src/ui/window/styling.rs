//! CSS installation and git branch detection helpers.

use crate::model::panel::GitBranch;

pub(super) fn install_css() {
    // Ensure Adwaita legacy icons (terminal, etc.) resolve on all systems,
    // and add bundled cmux icons (globe, etc.).
    if let Some(display) = gtk4::gdk::Display::default() {
        let icon_theme = gtk4::IconTheme::for_display(&display);
        icon_theme.add_search_path("/usr/share/icons/Adwaita");

        // Bundled icons ship next to the binary at ../icons (dev) or alongside the crate source.
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        if let Some(dir) = exe_dir {
            let bundled = dir.join("../../cmux/icons");
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
        /* ── Workspace rows ── */
        .workspace-row {
            border-radius: 8px;
            margin: 1px 4px;
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
        ",
    );

    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
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
