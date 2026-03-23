# cmux-gtk

GTK4/libadwaita terminal multiplexer for AI coding agents. Rust + Ghostty.

## Setup

```bash
git submodule update --init
cargo build --features cmux/link-ghostty
```

## Build

```bash
cargo check          # Type check
cargo test           # Run tests
cargo build          # Debug build
cargo build --release # Release build
```

## Features

- **Terminal multiplexer** — workspaces, split panes, tab management, directional focus
- **Integrated browser** — WebKit6 panels with 120+ automation commands (Playwright-style API)
- **Shell integration** — auto-injected via ZDOTDIR/BASH_ENV; CWD, git branch, PR polling, semantic prompts
- **Remote SSH workspaces** — `cmux ssh user@host` with auto-bootstrap daemon, SOCKS5 proxy tunnel for browser traffic, CLI relay for remote cmux commands, sidebar connection indicators
- **Session persistence** — scrollback, geometry, zoom, URLs, browser back/forward history restored on restart
- **Socket API** — V1 text (60 commands) + V2 JSON-RPC protocol (210+ methods) for automation
- **CLI wrapper** — `cmux/bin/cmux` shell script for quick socket interaction
- **Claude Code wrapper** — `cmux/bin/claude` injects hooks for status/notifications in sidebar
- **URL routing** — `cmux/bin/xdg-open` intercepts HTTP(S) URLs to cmux in-app browser
- **Command palette** — 50+ commands, fuzzy search, workspace switcher
- **All-surfaces search** — Ctrl+P to search text across all terminals
- **Omnibar** — inline ghost text completion, switch-to-tab suggestions, search engine fallback (Google, DuckDuckGo, Bing, Kagi, Startpage)
- **Sidebar metadata** — status pills, rich metadata entries, markdown blocks, progress bars, log entries, PR check icons, hide-all-details toggle, vertical branch layout, help menu
- **Notification sounds** — freedesktop theme sound presets (7 presets + custom file), desktop notifications
- **OSC notifications** — OSC 9/777 triggers desktop notifications with pane attention ring
- **Browser profiles** — per-profile isolated NetworkSession with persistent cookies
- **Browser history** — frecency-scored history with omnibar autocomplete
- **Browser navigation** — window.open/target=_blank → new tab, Ctrl+click/middle-click → new tab, deep link handling (custom URI schemes → xdg-open), insecure HTTP interstitial with allowlist
- **Browser theme mode** — separate System/Light/Dark override with toolbar toggle and settings
- **Browser security** — user agent override (Safari-compatible), camera/mic permission auto-allow, context menu customization
- **Link routing** — configurable URL patterns for system vs cmux browser, HTTP allowlist
- **Keyboard copy mode** — Ghostty vi-style navigation with vim badge indicator
- **Ghostty config** — reads `~/.config/ghostty/config` for themes, fonts, colors, background opacity, unfocused split opacity, and split divider color; live reload via Ctrl+Shift+,
- **File drag-and-drop** — drop files from file manager onto terminal to paste shell-escaped paths
- **Omarchy themes** — colors.toml parsing with SIGUSR2 live reload
- **tmux compatibility** — CLI shim maps tmux commands (split-window, send-keys, capture-pane, etc.) to cmux socket API for tool compatibility
- **Theme browser** — `cmux themes [filter]` lists bundled ghostty themes from system and user directories
- **Multi-window** — workspaces assignable across windows
- **Workspace management** — pinning, custom colors, reorder, close-others/above/below
- **Welcome screen** — first-launch getting-started tips

## Architecture

- `ghostty-sys/` — Raw FFI bindings to libghostty C API (`ghostty.h`)
- `ghostty-gtk/` — Safe Rust wrapper: GhosttyApp, GhosttyGlSurface, key mapping
- `cmux/` — Main application (GTK4/libadwaita)
  - `app.rs` — AppState, SharedState, terminal surface lifecycle, window management
  - `model/` — TabManager, Workspace, Panel, LayoutNode
  - `ui/` — Window, Sidebar, SplitView, TerminalPanel, BrowserPanel, MarkdownPanel, CommandPalette, Omnibar, SearchOverlay, AllSurfacesSearch, NotificationsPanel, Welcome, Settings
  - `socket/` — Unix socket server, V1 text protocol, V2 JSON protocol, browser automation, auth
  - `session/` — Session persistence (XDG, JSON compatible with macOS cmux)
  - `settings/` — AppSettings, ShortcutConfig, SidebarDisplay, Notifications, LinkRouting
  - `remote/` — Remote SSH workspaces (bootstrap, proxy tunnel, RPC, CLI relay)
  - `notifications.rs` — Notification store, desktop notifications, sound playback
  - `browser_history.rs` — Frecency-scored browser history with search
  - `browser_profiles.rs` — Per-profile WebKit NetworkSession isolation
  - `ghostty_config.rs` — Reads ghostty config for themes, colors, opacity
  - `port_scanner.rs` — Port detection for sidebar display
- `cmux/bin/cmux` — CLI wrapper script (socket auto-discovery, ncat/socat/nc transport, claude-hook subcommand)
- `cmux/bin/claude` — Claude Code wrapper (session hooks, status reporting)
- `cmux/bin/xdg-open` — URL routing wrapper (HTTP(S) → cmux browser, fallback to system)
- `cmux/shell-integration/` — Auto-injected zsh/bash integration scripts

## Architecture Review

**Read `docs/architecture-review.md` and `docs/ubuntu-mvp-spec.md` before making structural changes.**
They document the current Ubuntu MVP tradeoffs, Ghostty integration constraints, and review scope.

## Shell Integration

cmux auto-injects shell integration via:
- **Zsh**: ZDOTDIR override → `.zshenv` bootstrap → sources integration, restores user ZDOTDIR
- **Bash**: BASH_ENV → sources integration script (PS0 preexec on Bash 4.4+)

Features: CWD reporting, fast git HEAD resolution (no fork), async git branch detection (3s throttle, background subshell), async git HEAD watcher during commands, smart PR polling with `gh` CLI (45s interval, 20s timeout, transient failure resilience), port scanning, semantic prompt markers (OSC 133 with `redraw=last;cl=line`), scrollback restoration, prompt wrap guard (zsh), WINCH guard (zsh), PATH prepend for cmux CLI, recursive process tree cleanup on exit.

## Socket Protocol

Unix socket at `$XDG_RUNTIME_DIR/cmux.sock` (falls back to `/tmp/cmux-$UID.sock`).

**V1 text protocol** — 60 line-delimited text commands for shell integration and CLI use.
**V2 JSON-RPC protocol** — 210+ methods for programmatic automation.
**Browser automation** — 120+ `browser.*` commands (Playwright-style API).

Use the CLI wrapper: `cmux/bin/cmux <command> [args...]`

## Ghostty Integration

The `link-ghostty` feature enables actual FFI linking to libghostty.
Without it (default), the crates compile in stub mode for development.

To build with ghostty:
1. Initialize the ghostty submodule
2. Build with `cargo build --features cmux/link-ghostty`

## Keyboard Shortcuts

All shortcuts are configurable via `~/.config/cmux/shortcuts.json`.

| Shortcut | Action |
|----------|--------|
| Ctrl+Shift+T | New workspace |
| Ctrl+Shift+N | New window |
| Ctrl+Shift+W | Close workspace |
| Ctrl+Shift+Q | Close focused pane |
| Ctrl+Shift+D | Split horizontally |
| Ctrl+Shift+E | Split vertically |
| Ctrl+Shift+P | Command palette |
| Ctrl+P | Search all terminals |
| Ctrl+F | Find in terminal |
| Ctrl+G | Find next match |
| Ctrl+Shift+G | Find previous match |
| Ctrl+E | Use selection for find |
| Ctrl+Shift+I | Toggle notifications |
| Ctrl+Shift+B | Toggle sidebar |
| Ctrl+Shift+H | Flash focused pane |
| Ctrl+Shift+R | Rename workspace |
| Ctrl+Shift+Z | Toggle pane zoom |
| Ctrl+Shift+M | Enter copy mode |
| Ctrl+Shift+Y | Reopen closed browser tab |
| Ctrl+Shift+U | Jump to latest unread |
| Ctrl+O | Open folder as new workspace |
| Ctrl+Shift+O | Open workspace directory in file manager |
| Ctrl+, | Settings |
| Ctrl+Shift+, | Reload ghostty config |
| Ctrl+K | Clear terminal scrollback |
| Ctrl+=/- | Increase/decrease font size |
| Ctrl+0 | Reset font size |
| Ctrl+1-9 | Jump to workspace |
| Ctrl+Tab | Next workspace |
| Ctrl+Shift+Tab | Previous workspace |
| Ctrl+Shift+Page Up/Down | Move workspace up/down |
| Alt+Arrow | Focus pane in direction |
| Ctrl+Shift+[/] | Focus previous/next pane |
| Ctrl+Alt+D | Split browser horizontal |
| Ctrl+Alt+E | Split browser vertical |
| Ctrl+Alt+C | Toggle browser console |
| Ctrl+Shift+Alt+W | Close other tabs in pane |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CMUX_SOCKET` | Override socket path |
| `CMUX_DISABLE_SESSION_RESTORE` | Set to `1` to skip session restore |

## Reference

- ghostty C API: `ghostty.h` in the ghostty submodule
- Ghostty GTK runtime: `ghostty/src/apprt/gtk/` (reference for GL/input integration)
