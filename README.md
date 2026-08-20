# jmux

**A Linux-first Claude Agent manager** — similar to cmux, cmux-gtk, and orca — supporting **remote and local claude/agent sessions**. A GTK4/libadwaita terminal multiplexer built for running many AI coding agents at once, in Rust + [Ghostty](https://ghostty.org).

![jmux](docs/screenshots/hero.png)

> **Note:** the screenshots and GIFs below are real captures, generated privacy-safely (synthetic demo data, no real paths) by [`docs/autocapture.sh`](docs/autocapture.sh) and [`docs/capture-tools/make-gifs.sh`](docs/capture-tools/make-gifs.sh) running jmux in a headless compositor. The GIFs drive real GTK interactions through a tiny `wlr-virtual-pointer`/`virtual-keyboard` client — see [`docs/SCREENSHOTS.md`](docs/SCREENSHOTS.md).

---

## Install

```bash
git submodule update --init
cargo build --release --features jmux/link-ghostty
sudo bash scripts/install.sh          # installs jmux-app + jmux CLI to /usr/local
```

Browser support (WebKit6) is on by default; build without it via `--no-default-features --features jmux/link-ghostty`.

The **quick terminal** (Quake-style drop-down) is an opt-in build that needs `gtk4-layer-shell` and a layer-shell compositor (KDE/wlroots):

```bash
# install libgtk4-layer-shell (e.g. `sudo pacman -S gtk4-layer-shell`), then:
cargo build --release --features jmux/link-ghostty,jmux/quick-terminal
```

### Browser location services

The integrated browser prompts for location/camera/mic permissions, but the
actual position comes from the system **GeoClue2** service — install it for
geolocation to resolve (`sudo pacman -S geoclue` on Arch). jmux warns in the
prompt when it's missing.

GeoClue 2.7+ removed the Mozilla Location Service, so WiFi-based lookups now
need a backend URL. Uncomment a provider such as [BeaconDB](https://beacondb.net)
in `/etc/geoclue/geoclue.conf` under `[wifi]`:

```ini
[wifi]
enable=true
url=https://api.beacondb.net/v1/geolocate
```

then restart it so the new config is read: `sudo pkill -x geoclue` (it
re-activates on the next request). WiFi geolocation requires a WiFi adapter with
access points in range — without GeoClue or a backend, jmux still shows the
prompt but the lookup times out.

---

### Crash resilience

jmux autosaves session structure every 8 s (off the main loop) and a memory
watchdog force-saves the *full* session — scrollback included — if RSS ever
climbs past 4 GB, so even an OOM kill lands after a fresh save. On restart,
layouts and scrollback restore and agent panes relaunch via `claude
--resume`. To have systemd bring the UI back automatically after a crash,
add a drop-in for the autostart unit:

```ini
# ~/.config/systemd/user/app-com.jacobbriggs.jmux@autostart.service.d/50-restart.conf
[Service]
Restart=on-failure
RestartSec=3
```

## Highlights

### Goal & Graph — autonomous agent loops with human gates

Point jmux at a goal file and it runs an agent to completion
(`jmux goal`); point it at a big goal and an agent first writes a **plan**
— the goal split into nodes that run in parallel (`jmux graph`) — each node
its own agent workspace in its own git worktree, scheduled by jmux,
reviewed by you at editor-based gates, with every iteration written to an
`iteration-N.md` audit trail (`goal.output_dir`, default `docs/roadmap`). A live **graph panel**
shows the DAG as clickable status chips: click a node for its status,
its per-iteration reports (openable/editable in the notes editor — the
feedback section steers the next loop), and an inline goal editor.

**[Full walkthrough with an end-to-end demo → docs/GOAL-GRAPH.md](docs/GOAL-GRAPH.md)**

```sh
jmux goal  ~/src/myrepo/goal.md --wait          # one goal, one agent
jmux graph "build a trail map site" --max-concurrency 3   # plan it, then run it
```

### Run a fleet of agents, see them all at a glance

**Pane Overview** — a status grid of every pane in the workspace: busy / idle / attention dots (detected from each pane's foreground process), a one-line activity snippet, and click-to-jump. Open it from the header grid button, the palette, or a configurable shortcut.

![Pane Overview](docs/demos/pane-overview.gif)

### The sidebar octopus — Claude state per workspace

Every sidebar workspace row grows a tiny animated octopus when a Claude agent in it is active — the state is classified live from the pane's title and terminal text (spinner, `esc to interrupt` footer, selection menus, trailing questions), and the workspace shows its **most urgent** pane:

| ![working](jmux/assets/working.gif) | ![needs input](jmux/assets/needs_input.gif) | ![waiting](jmux/assets/waiting.gif) |
|:---:|:---:|:---:|
| **working** — hammering an anvil: the main turn is running | **needs input** — holding a sparkler: a question or menu is waiting on you | **waiting** — typing at a laptop: a background shell/agent is still running |

One shared wall-clock frame iterator per state keeps every octopus in phase across sidebar rebuilds, and the sprite scales with your UI font. In context, each workspace row carries its most-urgent pane's octopus, so the whole fleet's state reads down the sidebar at a glance.

<!-- Run `bash docs/autocapture.sh` to (re)generate the in-context sidebar capture, then add:
![Sidebar octopus states](docs/screenshots/sidebar-octopus.png) -->


### Stream Deck Plus companion — your fleet on physical keys

**deck** (coming soon to a public mirror) mirrors the same octopus state model onto a **Stream Deck Plus**: each key is a jmux workspace (amber = needs input, blue = working, teal = waiting), press a key to focus it, and use the dials/touchscreen to switch Claude tabs and **answer questions without touching the keyboard**.

![Stream Deck Plus companion](docs/demos/streamdeck.gif)

*(Rendered by deck's own key renderer with synthetic session data — [`docs/capture-tools/make-streamdeck-mock.py`](docs/capture-tools/make-streamdeck-mock.py) — so it's pixel-identical to the real device.)*

### Agent integrations — teammates as native panes

`jmux claude-teams` (Claude Code agent-teams) and `jmux omo` (OpenCode / oh-my-openagent) launch agents whose teammates and subagents open as **native jmux panes**. A private tmux shim translates `split-window` / `send-keys` / `capture-pane` → jmux's `surface.split` / `send_text` / `read_text`, and `terminal-notifier` → `jmux notify` — no real tmux required.

![Agent integrations](docs/screenshots/agent-integrations.png)

### Drag tabs & move panes

Drag a tab to **reorder** it, drop it on a pane edge to **split**, or drop it on a sidebar workspace to **move it across workspaces**. Panes rearrange live.

![Dragging tabs](docs/demos/drag-tabs.gif)
![Moving panes](docs/demos/move-panes.gif)
![Splitting panes](docs/demos/drag-split.gif)

### History & Vault

| | |
|---|---|
| **History** — `jmux history` opens a searchable, day-grouped list of recently closed & focused workspaces; click to reopen. Closed history persists across restarts. | **Vault** — `jmux vault` indexes past Claude Code / Codex sessions with title/dir/preview search; click a session to resume it in a terminal. |
| ![History pane](docs/demos/history.gif) | ![Vault pane](docs/screenshots/vault-pane.png) |

### Dock — always-on terminal controls

A right-side column of small terminal "controls" (lazygit, log tails, build watchers) defined in `.jmux/dock.json` or `~/.config/jmux/dock.json`. Edit them from a GUI (**Settings → Dock → Edit Dock Controls**). Hidden by default; toggle from the header.

![Dock](docs/demos/dock.gif)

| Dock controls | Dock editor |
|---|---|
| ![Dock](docs/screenshots/dock.png) | ![Dock editor](docs/screenshots/dock-editor.png) |

### TextBox — compose prompts before sending

An opt-in multi-line input below a terminal for composing multi-line agent prompts. Enter sends, Shift+Enter adds a newline, Esc returns to the terminal.

![TextBox](docs/screenshots/textbox.png)

### Finder previews & Task Manager

| Finder previews | Task Manager |
|---|---|
| Inline **image, video, and PDF** previews (PDF rendered page-by-page via poppler) — ideal for reviewing screenshots and demo recordings agents produce. | `jmux top` opens a CPU/RAM monitor for the agent & terminal processes in your workspaces. |
| ![Finder previews](docs/screenshots/pdf-preview.png) | ![Task Manager](docs/screenshots/task-manager.png) |

### Panels — more than terminals

Any pane can hold a non-terminal view: a **rendered Markdown** file, a **git diff** (with a Staged toggle and branch compare), a **project tree** with a file-type summary, **scope-grouped Notes** beside your terminal, or a full **WebKit browser** with a Playwright-style automation API.

| Markdown viewer | Diff viewer |
|---|---|
| ![Markdown viewer](docs/screenshots/markdown.png) | ![Diff viewer](docs/screenshots/diff.png) |
| **Project visualizer** | **Integrated browser** |
| ![Project visualizer](docs/screenshots/project.png) | ![Integrated browser](docs/screenshots/browser.png) |

### Notes — scope-grouped scratchpads

`jmux notes` opens an editable, auto-saved notes panel that knows **where you are**. Notes are organised into colour-coded **scope groups**, each holding any number of tabs (one per file, labelled by filename):

- 🔵 **Global** — notes shared everywhere, not tied to a folder.
- 🟢 **Folder** — notes for the current **git repo root** (or the working directory when it isn't a repo).
- 🟣 **Host** — for `jmux ssh` sessions, notes scoped to the whole remote host (plus a Folder group for the remote project).

Hit **+** to add a note (auto-named, double-click its tab to rename), and a note is **deleted from disk the moment it's empty** — they only persist once they have content. Everything is stored client-side under `~/.local/share/jmux/notes/`, so remote notes never touch the SSH bridge.

![Scope-grouped notes](docs/screenshots/notes.png)

### Command palette & custom commands

Fuzzy command palette and workspace switcher. Define your own entries in `jmux.json` (project `.jmux/jmux.json`, project root, or global) that run a shell command **or** open a whole multi-pane `workspace` layout.

![Command palette](docs/demos/command-palette.gif)

---

## All features

### Workspaces, panes & windows
- **Terminal multiplexer** — workspaces, split panes, tab management, directional focus
- **Move tabs & panes** — drag a tab to reorder, split a pane (drop on an edge), or move it to another workspace; break/join panes across workspaces
- **New tab** — `Ctrl+Shift+T` opens a new terminal in the current pane, inheriting the focused terminal's working directory
- **Per-workspace environment** — `workspace.env` in a `jmux.json` layout injects variables into every shell spawned in that workspace
- **Pane overview** — status grid of every pane (busy/idle/attention) with click-to-jump (header button / palette / `overview.open`)
- **Multi-window** — single-instance app; launching again opens a new window, workspaces assignable across windows
- **Quick terminal** — Quake-style drop-down that slides in from the top edge (top-anchored layer-shell overlay with the full jmux UI minus the OS maximize/close), toggled by a configurable global hotkey or `jmux quick-terminal toggle`. Opt-in build (`--features jmux/quick-terminal`); enable + set the hotkey/height in **Settings → Quick Terminal**. The hotkey registers via the GlobalShortcuts portal — on KDE it appears in **System Settings → Shortcuts** to assign; for a guaranteed system-wide key, bind `jmux quick-terminal toggle` to a custom shortcut there
- **Workspace groups** — collapsible sidebar sections with per-group color, unread badges, drag-anchored membership, persistence (`jmux group`)
- **Workspace management** — pinning, custom colors, reorder, close-others/above/below
- **Workspace descriptions** — free-text per-workspace notes (`jmux describe`), shown in the sidebar tooltip
- **Focus history** — back/forward through recently-focused workspaces (`jmux back`/`forward`)
- **Reopen closed workspace** — `jmux reopen` restores the most recently closed workspace (layout + cwd, fresh shells)
- **Reopen closed tab** — `jmux reopen-tab` restores the most recently closed panel in the current workspace (type/dir/command preserved)
- **Agent hibernation** — pause (SIGSTOP) an idle agent to free CPU and resume (SIGCONT) on demand (`jmux hibernate`/`wake`)
- **Display placement** — `jmux window displays` / `jmux window display <name|index>` moves the window to a monitor

### Panels
- **Terminal** — Ghostty-backed GL surfaces with vi-style copy mode and a vim badge indicator
- **Integrated browser** — WebKit6 panels with 120+ automation commands (Playwright-style API), profiles, history, focus mode
- **Markdown viewer** — `jmux open file.md` / drag from the sidebar; renders **Mermaid diagrams**
- **Diff viewer** — `jmux diff [path]` git diff CodeView (`--staged` / `--branch <ref>`); plain-GTK, no WebKit; **syntax-highlighted** lines and right-click **review comments** (persisted to `.jmux/diff-comments.json`)
- **Project visualizer** — `jmux project [path]` directory tree + file-type/size summary
- **Finder previews** — inline image / video / **PDF** (poppler) previews in the file-preview panel
- **File explorer** — sidebar tree with configurable double-click action (preview / default app / preferred editor) and **Insert Path / Insert Relative Path** into the focused terminal
- **Notes** — `jmux notes` scope-grouped scratchpads (colour-coded **Global / Host / Folder** tab groups, keyed to the git repo root), multiple notes per scope with a **+** to add and double-click-to-rename tabs; empty notes auto-delete, stored client-side under `~/.local/share/jmux/notes/`. `jmux notes <file>` still opens a single file
- **Word wrap** — toggle line wrapping for the file preview/editor and notes panels (Settings → Editor & Files)
- **History pane** — `jmux history` searchable day-grouped closed/focused workspaces
- **Vault pane** — `jmux vault` searchable index of past Claude Code / Codex sessions, click to resume

### AI agent workflow
- **Sidebar Claude-state octopus** — animated sprite on each workspace row (hammering = working, sparkler = needs input, laptop = background task); classified from pane title + terminal text, most-urgent pane wins, hibernated agents excluded
- **Stream Deck Plus companion** — **deck** (coming soon to a public mirror) shows workspaces as keys with the live octopus state; press to focus, dials + touchscreen to pick Claude tabs and answer questions
- **Agent integrations** — `jmux claude-teams` & `jmux omo` open teammates/subagents as native panes via a tmux shim; `terminal-notifier` → `jmux notify`. Turnkey status/notification hooks: `jmux claude-hook` / `codex-hook` / `kiro-hook` / `cursor-hook` / `gemini-hook` (or generic `jmux agent hook … --cli <name>`)
- **AI workspace auto-naming** — opt-in: name an untitled workspace from its agent transcript when the agent finishes (`jmux ai-name`, or auto via Settings; uses `ANTHROPIC_API_KEY`)
- **Task Manager** — `jmux top` CPU/RAM monitor (also `jmux ps` for JSON)
- **TextBox** — multi-line prompt composer below terminals (opt-in)
- **Dock** — right-side terminal controls from `dock.json`, with a GUI editor
- **Custom commands** — `jmux.json` palette entries: shell commands or multi-pane `workspace` layouts (recursive `split`/`pane`/`surfaces` with per-surface `command`/`cwd`/`url`/`focus`)
- **Sidebar metadata** — status pills, metadata blocks, progress bars, log entries, PR check icons, hide-all-details toggle
- **Notifications** — OSC 9/777 desktop notifications with pane attention ring; freedesktop sound presets

### Browser
- 120+ `browser.*` automation commands (Playwright-style)
- Per-profile isolated NetworkSession with persistent cookies; cookie import
- Frecency-scored history with omnibar autocomplete
- Search engines (DuckDuckGo/Google/Bing/Kagi/Startpage/Custom) + **keyword search providers** (`gh rust` → GitHub search)
- `window.open`/`target=_blank` → new tab, Ctrl/middle-click → new tab, mouse 8/9 back/forward, deep-link handling, HTTP interstitial with allowlist
- Per-tab audio mute, distraction-free focus mode, React component grab
- System/Light/Dark theme override; UA override, camera/mic/geo denied by default

### CLI & automation
- **Socket API** — V1 text (60 commands) + V2 JSON-RPC (210+ methods)
- **CLI wrapper** — `jmux/bin/jmux <command>` (socket auto-discovery)
- **Diagnostics** — `jmux config doctor` checks install, daemon reachability, and config-file health
- **Open in IDE** — `jmux ide [editor] [dir]` opens a directory in VS Code / IntelliJ / Cursor / Zed / Sublime / …
- **tmux compatibility** — CLI shim maps tmux commands to the socket API
- **Claude Code wrapper** — `jmux/bin/claude` injects status/notification hooks
- **URL routing** — `jmux/bin/xdg-open` routes HTTP(S) to the in-app browser

### Integration & platform
- **Shell integration** — auto-injected (zsh/bash/fish): CWD, git branch, PR polling, semantic prompts
- **Remote SSH workspaces** — `jmux ssh user@host` with auto-bootstrap daemon, SOCKS5 proxy tunnel for browser traffic, CLI relay
- **Session persistence** — scrollback, geometry, zoom, URLs, browser history, closed-tab history restored on restart
- **Ghostty config** — reads `~/.config/ghostty/config` (themes, fonts, colors, opacity); live reload via `Ctrl+Shift+,`
- **Themes** — `jmux themes [filter]`; Omarchy `colors.toml` with SIGUSR2 live reload
- **App icon** — installed into the hicolor theme (SVG + 32–256px PNG) with a pixmaps fallback

---

## Keyboard shortcuts

All shortcuts are configurable via `~/.config/jmux/shortcuts.json` (or **Settings → Keyboard**). A per-action `when` clause (VS Code-style: `terminalFocused`, `browserFocused`, `editorFocused`, `panelFocused`, `paneZoomed`, combined with `&&`/`||`/`!`) can gate a binding by context.

| Shortcut | Action |
|----------|--------|
| Ctrl+Shift+T | New tab in the current pane |
| Ctrl+Shift+N | New window |
| Ctrl+Shift+W | Close workspace |
| Ctrl+Shift+Q | Close focused pane |
| Ctrl+Shift+D / E | Split horizontally / vertically |
| Ctrl+Shift+P | Command palette |
| Ctrl+P | Search all terminals |
| Ctrl+F / G / Shift+G | Find / next / previous |
| Ctrl+Shift+B | Toggle sidebar |
| Ctrl+Shift+Z | Toggle pane zoom |
| Ctrl+Shift+M | Enter copy mode |
| Ctrl+Shift+S | Toggle sub-agent monitor panes |
| Ctrl+Shift+U | Jump to latest unread |
| Ctrl+1–9 | Jump to workspace |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous workspace |
| Alt+Arrow | Focus pane in direction |
| Ctrl+, | Settings |

**Configurable, unbound by default** (bind in Settings → Keyboard): `tab.reopen` (reopen closed tab), `textbox.focus`, `dock.toggle`, `overview.open`. These are unbound because, with a terminal focused, ghostty's Kitty keyboard protocol encodes `Ctrl+Shift+<key>` to the shell before jmux sees it — so the **header buttons and command palette are the reliable triggers** for the Dock and Overview.

---

## Configuration files

| File | Purpose |
|------|---------|
| `~/.config/jmux/settings.json` (or `jmux.json`) | App settings |
| `~/.config/jmux/shortcuts.json` | Keybindings |
| `.jmux/jmux.json` / `jmux.json` / `~/.config/jmux/jmux.json` | Custom commands (`commands[]`, optional `workspace` layouts) |
| `.jmux/dock.json` / `~/.config/jmux/dock.json` | Dock controls (`id`/`title`/`command`/`cwd`/`height`) |

---

## Architecture

- `ghostty-sys/` — Raw FFI bindings to libghostty (`ghostty.h`)
- `ghostty-gtk/` — Safe Rust wrapper: GhosttyApp, GhosttyGlSurface, key mapping
- `jmux/` — Main application (GTK4/libadwaita)
  - `app.rs` — AppState, SharedState, terminal surface lifecycle, window management
  - `model/` — TabManager, Workspace, Panel, LayoutNode
  - `ui/` — window, sidebar, split view, terminal/browser/markdown/diff/project/file-preview/notes/history/vault panels, dock, textbox, pane overview, command palette, settings
  - `socket/` — Unix socket server, V1 text + V2 JSON protocols, browser automation, auth
  - `session/` — session persistence (XDG, upstream-compatible JSON format)
  - `settings/` — AppSettings, ShortcutConfig, custom commands
  - `remote/` — remote SSH workspaces (bootstrap, proxy tunnel, RPC, CLI relay)
- `jmux/bin/` — `jmux` (CLI), `claude` (hook wrapper), `xdg-open` (URL routing)
- `jmux/shell-integration/` — auto-injected zsh/bash/fish integration scripts

**Read `docs/architecture-review.md` before making structural changes** — it documents the Ghostty integration constraints.

---

## Socket protocol

Unix socket at `$XDG_RUNTIME_DIR/jmux.sock` (falls back to `/tmp/jmux-$UID.sock`). V1 text (60 commands) + V2 JSON-RPC (210+ methods) + 120+ `browser.*` automation commands. Use `jmux/bin/jmux <command> [args...]`.

## Environment variables

| Variable | Description |
|----------|-------------|
| `JMUX_SOCKET` | Override socket path |
| `JMUX_DISABLE_SESSION_RESTORE` | Set to `1` to skip session restore |
| `JMUXD_PROXY_ALLOW_PRIVATE` | Set to `1` on the **remote host** to allow the SOCKS5 proxy to reach private/loopback IPs |

## Security

See [docs/security.md](docs/security.md). Socket auth via `SO_PEERCRED`, HMAC-SHA256 relay auth, 0o600/0o700 file perms, input validation, documented `unsafe` with panic guards, SSRF denylist on the proxy tunnel, `cargo audit` in CI.

## Reference

- ghostty C API: `ghostty.h` in the ghostty submodule
- Ghostty GTK runtime: `ghostty/src/apprt/gtk/`

---

## Attribution

jmux is a port based on [cmux](https://cmux.com) and cmux-gtk.
