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

- **Terminal multiplexer** — workspaces, split panes, tab management
- **Integrated browser** — WebKit6 panels with 78+ automation commands
- **Shell integration** — auto-injected via ZDOTDIR/BASH_ENV; CWD, git branch, PR polling, semantic prompts
- **Session persistence** — scrollback, geometry, zoom, URLs, browser back/forward history restored on restart
- **Socket API** — V1 text (90+ commands) + V2 JSON protocol for automation
- **CLI wrapper** — `cmux/bin/cmux` shell script for quick socket interaction
- **Claude Code wrapper** — `cmux/bin/claude` injects hooks for status/notifications in sidebar
- **URL routing** — `cmux/bin/xdg-open` intercepts HTTP(S) URLs to cmux in-app browser
- **Command palette** — 50+ commands, fuzzy search
- **All-surfaces search** — Ctrl+P to search text across all terminals
- **Omnibar** — inline ghost text completion, switch-to-tab suggestions, search engine fallback
- **Sidebar metadata** — status pills, rich metadata entries, markdown blocks, progress bars, log entries, PR check icons, hide-all-details toggle, vertical branch layout
- **Notification sounds** — freedesktop theme sound presets (8 presets + custom), desktop notifications
- **OSC notifications** — OSC 9/777 triggers desktop notifications with pane attention ring
- **Browser profiles** — per-profile isolated NetworkSession with persistent cookies
- **Browser history** — frecency-scored history with omnibar autocomplete
- **Link routing** — configurable URL patterns for system vs cmux browser
- **Keyboard copy mode** — Ghostty vi-style navigation with vim badge indicator
- **Ghostty config** — reads `~/.config/ghostty/config` for themes, fonts, colors, background opacity, unfocused split opacity, and split divider color
- **Omarchy themes** — colors.toml parsing with SIGUSR2 live reload
- **Multi-window** — workspaces assignable across windows
- **Welcome screen** — first-launch getting-started tips

## Architecture

- `ghostty-sys/` — Raw FFI bindings to libghostty C API (`ghostty.h`)
- `ghostty-gtk/` — Safe Rust wrapper: GhosttyApp, GhosttyGlSurface, key mapping
- `cmux/` — Main application (GTK4/libadwaita)
  - `model/` — TabManager, Workspace, Panel, LayoutNode
  - `ui/` — Window, Sidebar, SplitView, TerminalPanel, BrowserPanel, CommandPalette, Omnibar, AllSurfacesSearch, Welcome, Settings
  - `socket/` — Unix socket server, V1 text protocol, V2 JSON protocol, browser automation, auth
  - `session/` — Session persistence (XDG, JSON compatible with macOS cmux)
  - `settings/` — AppSettings, ShortcutConfig, SidebarDisplay (hide-all-details, vertical branch layout, notification message toggle, light/dark tint), Notifications, LinkRouting
  - `notifications.rs` — Notification store, desktop notifications, sound playback
  - `browser_history.rs` — Frecency-scored browser history with search
  - `browser_profiles.rs` — Per-profile WebKit NetworkSession isolation
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

**V1 text protocol** — 90+ line-delimited text commands for shell integration and CLI use.
**V2 JSON protocol** — 120+ JSON-RPC methods for programmatic automation.
**Browser automation** — 78+ `browser.*` commands (Playwright-style API).

Use the CLI wrapper: `cmux/bin/cmux <command> [args...]`

## Ghostty Integration

The `link-ghostty` feature enables actual FFI linking to libghostty.
Without it (default), the crates compile in stub mode for development.

To build with ghostty:
1. Initialize the ghostty submodule
2. Build with `cargo build --features cmux/link-ghostty`

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+Shift+T | New workspace |
| Ctrl+Shift+N | New window |
| Ctrl+Shift+W | Close workspace |
| Ctrl+Shift+D | Split horizontally |
| Ctrl+Shift+E | Split vertically |
| Ctrl+Shift+L | Open browser panel |
| Ctrl+Shift+P | Command palette |
| Ctrl+P | Search all terminals |
| Ctrl+F | Find in terminal |
| Ctrl+Shift+I | Toggle notifications |
| Ctrl+Shift+B | Toggle sidebar |
| Ctrl+Shift+H | Flash focused pane |
| Ctrl+Shift+R | Rename workspace |
| Ctrl+Shift+V | Enter copy mode |
| Ctrl+O | Open folder as workspace |
| Ctrl+, | Settings |
| Ctrl+1-9 | Jump to workspace |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CMUX_SOCKET` | Override socket path |
| `CMUX_DISABLE_SESSION_RESTORE` | Set to `1` to skip session restore |

## Reference

- ghostty C API: `ghostty.h` in the ghostty submodule
- Ghostty GTK runtime: `ghostty/src/apprt/gtk/` (reference for GL/input integration)
