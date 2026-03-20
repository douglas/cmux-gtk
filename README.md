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

## Architecture

- `ghostty-sys/` — Raw FFI bindings to libghostty C API (`ghostty.h`)
- `ghostty-gtk/` — Safe Rust wrapper: GhosttyApp, GhosttyGlSurface, key mapping
- `cmux/` — Main application (GTK4/libadwaita)
  - `model/` — TabManager, Workspace, Panel, LayoutNode
  - `ui/` — Window, Sidebar, SplitView, TerminalPanel, BrowserPanel, CommandPalette, Settings
  - `socket/` — Unix socket server, v2 JSON protocol, auth
  - `session/` — Session persistence (XDG, JSON compatible with macOS cmux)
  - `settings/` — AppSettings, ShortcutConfig (XDG config persistence)
  - `notifications.rs` — Notification store + desktop notifications (gio::Notification)
- `cmux-cli/` — CLI client (`cmux workspace list`, `cmux surface send-text`, etc.)

## Architecture Review

**Read `docs/architecture-review.md` and `docs/ubuntu-mvp-spec.md` before making structural changes.**
They document the current Ubuntu MVP tradeoffs, Ghostty integration constraints, and review scope.

## Ghostty Integration

The `link-ghostty` feature enables actual FFI linking to libghostty.
Without it (default), the crates compile in stub mode for development.

To build with ghostty:
1. Initialize the ghostty submodule
2. Build with `cargo build --features cmux/link-ghostty`

## Socket Protocol

Unix socket at `$XDG_RUNTIME_DIR/cmux.sock` (falls back to `/tmp/cmux-$UID.sock`).
Line-delimited JSON v2 protocol. Compatible with macOS cmux socket API.

## Reference

- ghostty C API: `ghostty.h` in the ghostty submodule
- Ghostty GTK runtime: `ghostty/src/apprt/gtk/` (reference for GL/input integration)
