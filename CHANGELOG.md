# Changelog

## [0.62.0-alpha.4] - 2026-03-23

### Changed

- Switch ghostty submodule from manaflow-ai fork to upstream ghostty 1.3.1 stable (douglas/ghostty) — no fork-specific code remains

### Fixed

- Fix `?` and other shifted keys not working in Claude Code / Codex TUI (caused by manaflow ghostty fork key handling)
- Fix shell integration debug output (`cur_head=...`) leaking to terminal when Claude Code enables xtrace
- Fix GTK warnings about finalized ListBoxRow with leftover PopoverMenu children on sidebar refresh

## [0.62.0-alpha.3] - 2026-03-23

### Added

- GitHub Actions release workflow with automated AUR publishing on tag push
- `cargo-audit` security check in CI pipeline

## [0.62.0-alpha.2] - 2026-03-23

### Changed

- Browser permissions (camera, microphone, geolocation) are now denied by default instead of auto-allowed

### Fixed

- Fix self-deadlock when opening sidebar context menu with multiple windows (caused socket commands to hang permanently)
- Fix stale socket preventing app restart after crash — PID lockfile detects dead processes automatically
- CLI retries transient connection failures (EAGAIN, ECONNREFUSED) with backoff instead of failing immediately
- Fix HTTP allowlist wildcard matching — `*.example.com` no longer incorrectly matches `notexample.com`
- Fix search URL parameter injection — special characters (`&`, `#`) now properly percent-encoded
- Prevent shell injection in SSH workspace creation and notification custom commands
- Prevent XSS in HTTP insecurity interstitial via single-quote escaping
- Prevent download filename path traversal via absolute paths
- Prevent browser profile name path traversal in create/rename/delete
- Browser eval and action-with-reply commands now time out after 30 seconds instead of hanging indefinitely
- Scrollback temp files moved to `~/.cache/cmux/scrollback/` with restrictive permissions (0o600) and symlink protection
- Browser history file written with 0o600 permissions (no longer world-readable)
- WebKit profile data/cache directories set to 0o700 permissions
- XDG_RUNTIME_DIR validation no longer follows symlinks
- Socket handler inputs hardened: ports array capped, TTY/directory strings truncated, priority cast uses safe conversion
- Console messages truncated to 64KB per entry, browser history capped to 50,000 entries on load
- URLs passed to xdg-open sanitized (control characters stripped, length capped)
- Weak HMAC in relay auth replaced with openssl HMAC-SHA256 (fail-closed)
- Remote module shell injection prevented at 5 sites via shell-escape

## [0.62.0-alpha.1] - 2026-03-22

First public release of cmux-gtk — the Linux port of cmux, a terminal multiplexer for AI coding agents. Built with Rust, GTK4/libadwaita, and Ghostty.

### Added

- **Terminal multiplexer** — workspaces, split panes (horizontal/vertical), tabbed surfaces, directional focus (Alt+Arrow), pane zoom, drag-and-drop reordering
- **Integrated browser** — WebKit6 panels with 120+ automation commands (Playwright-style API: click, fill, type, find, wait, snapshot, eval, cookies, storage, network interception)
- **Shell integration** — auto-injected for zsh and bash; reports CWD, git branch, PR status, listening ports, and semantic prompt markers
- **Remote SSH workspaces** — `cmux ssh user@host` with auto-bootstrap daemon, SOCKS5 proxy tunnel for browser traffic, CLI relay, sidebar connection indicators
- **Session persistence** — terminal scrollback, window geometry, pane layout, browser URLs and back/forward history all restored on restart
- **Socket API** — V1 text protocol (60 commands) and V2 JSON-RPC protocol (210+ methods) for full automation
- **Browser automation** — element finding (by text, role, label, placeholder, test ID), waiting, screenshots, dialog handling, frame selection, console capture, network interception, geolocation/offline spoofing
- **Command palette** — 50+ commands with fuzzy search, workspace switcher, shortcut hints, editor integration (VS Code, Cursor, Zed, Neovim, etc.)
- **Omnibar** — frecency-scored browser history autocomplete, inline ghost text completion, switch-to-tab suggestions, remote search suggestions
- **Notification system** — terminal OSC 9/777 triggers, desktop notifications, sound presets (freedesktop theme sounds + custom files), pane attention ring, unread badges
- **Settings UI** — theme (System/Light/Dark/Omarchy), sidebar display toggles, browser config (search engine, home URL, HTTP allowlist), keyboard shortcuts, link routing, notification sounds
- **Sidebar metadata** — git branch, PR status with CI checks, working directory, listening ports, log entries, progress bars, custom status pills, freeform markdown blocks
- **Multi-window** — Ctrl+Shift+N for new windows, per-window workspaces, cross-window workspace movement, geometry persistence
- **Ghostty integration** — reads ~/.config/ghostty/config for themes, fonts, background opacity, split colors; live reload via Ctrl+Shift+Comma; SIGUSR2 Omarchy theme reload
- **Browser features** — window.open/target=_blank → new tab, Ctrl+click/middle-click → new tab, deep links → xdg-open, HTTP interstitial for insecure origins, find in page, developer tools, download bar, context menu (Open in New Tab, Open in Default Browser, Copy Page URL)
- **File drag-and-drop** — drop files onto terminal to paste shell-escaped paths
- **All-surfaces search** — Ctrl+P to search text across all terminals in all workspaces
- **Copy mode** — vi-style terminal text selection with vim badge indicator
- **tmux compatibility** — CLI shim that maps tmux commands to cmux socket API
- **Themes browser** — `cmux themes` command lists bundled Ghostty themes
- **Claude Code wrapper** — `cmux/bin/claude` injects hooks for sidebar status and notifications
- **macOS command aliases** — browser commands accept both underscore (`browser.find_by_text`) and dot notation (`browser.find.text`) for cross-platform script parity
- **Configurable shortcuts** — all keyboard shortcuts customizable via ~/.config/cmux/shortcuts.json

### Fixed

- Browser WebViews now cached across layout rebuilds (no more grey panels or crashes when closing adjacent terminals)
- Navigation policy correctly handles `about:blank` scheme (no longer routes to system browser)
- Omnibar suggestion popover dismisses after page load (no more sticky dropdown)
- WebView shutdown cleanup prevents WebProcess segfault on app close
- Stale session restore filters empty windows to prevent invisible app launch
- Browser `open_split` socket command now properly sets initial URL
