//! Application entry point — creates the AdwApplication and main window.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ghostty_sys::*;
use gtk4::gio;
use gtk4::prelude::*;
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;

/// Lock a mutex, recovering from poisoning rather than panicking.
/// Prevents cascading panics when one thread panics while holding a lock.
pub fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        tracing::error!("Mutex was poisoned, recovering");
        poisoned.into_inner()
    })
}

use crate::model::TabManager;
use crate::notifications::NotificationStore;
use crate::session;
use crate::socket;
use crate::ui;
use uuid::Uuid;

/// Shared application state accessible from UI callbacks (single-threaded, GTK main thread).
pub struct AppState {
    pub shared: Arc<SharedState>,
    pub ghostty_app: RefCell<Option<ghostty_gtk::app::GhosttyApp>>,
    pub terminal_cache: RefCell<HashMap<Uuid, ghostty_gtk::surface::GhosttyGlSurface>>,
    /// Cached browser panel widgets — survive layout rebuilds (like terminal_cache).
    pub browser_cache: RefCell<HashMap<Uuid, gtk4::Widget>>,
    /// Signature of the layout each workspace's content page was last built
    /// from, keyed by workspace id. The pages themselves live in each window's
    /// content GtkStack and stay realized (just hidden) while another workspace
    /// is shown, so switching workspaces is a `set_visible_child` and never
    /// unrealizes the GL surfaces — which previously leaked GTK's GLArea
    /// compositing texture (~4.7 MB) on every switch. The page is only rebuilt
    /// when this signature changes (split/close/new-tab/zoom), not on selection.
    pub workspace_page_signatures: RefCell<HashMap<Uuid, u64>>,
    /// Cached ghostty config values for UI decisions (background, opacity, etc.).
    pub ghostty_ui_config: RefCell<crate::ghostty_config::GhosttyUiConfig>,
    /// Stored to keep the callbacks alive for the lifetime of the app.
    _callbacks: RefCell<Option<ghostty_gtk::callbacks::RuntimeCallbacks>>,
}

/// Release a dead panel's terminal surface: unparent it from its `GtkBox` (a
/// layout pane, or the spawn nursery, which holds background surfaces that no
/// page has claimed yet) so the last reference goes away and `dispose` frees the
/// ghostty surface.
///
/// Deferred to an idle callback on purpose. The caller may be running inside
/// ghostty's own close callback (the child exited), and dropping the final
/// reference there would call `ghostty_surface_free` while libghostty is still
/// executing inside that surface.
fn release_surface(surface: ghostty_gtk::surface::GhosttyGlSurface) {
    glib::idle_add_local_once(move || {
        if let Some(parent) = surface.parent() {
            if let Ok(parent_box) = parent.downcast::<gtk4::Box>() {
                parent_box.remove(&surface);
            }
        }
    });
}

/// Synthetic pty geometry for a headless spawn, in physical pixels. The child
/// has to start with a usable grid — a TUI agent reads `$COLUMNS`/`$LINES` from
/// the pty at startup — and no allocation will ever tell us the real one. At a
/// typical 9×19 cell this is ~106×31 cells (the design doc asks for at least
/// 80×24); the first real allocation resizes it like any window resize.
const HEADLESS_SPAWN_WIDTH_PX: u32 = 960;
const HEADLESS_SPAWN_HEIGHT_PX: u32 = 600;

impl AppState {
    pub fn new(shared: Arc<SharedState>) -> Self {
        Self {
            shared,
            ghostty_app: RefCell::new(None),
            terminal_cache: RefCell::new(HashMap::new()),
            browser_cache: RefCell::new(HashMap::new()),
            workspace_page_signatures: RefCell::new(HashMap::new()),
            ghostty_ui_config: RefCell::new(Default::default()),
            _callbacks: RefCell::new(None),
        }
    }

    pub fn terminal_surface_for(
        &self,
        panel_id: Uuid,
        working_directory: Option<&str>,
        command: Option<&str>,
    ) -> ghostty_gtk::surface::GhosttyGlSurface {
        // Reuse a cached surface — but only return it as-is when it was
        // actually initialized. A surface built before ghostty_app existed
        // never wired its spawn path (initialize_with_env was skipped below),
        // and handing it back uninitialized forever is the "all terminals
        // blank" failure that forced the GtkStack revert (8bb1ab3). Fall
        // through instead and initialize it now.
        let cached = self.terminal_cache.borrow().get(&panel_id).cloned();
        if let Some(surface) = &cached {
            if surface.is_initialized() {
                return surface.clone();
            }
            tracing::warn!(
                %panel_id,
                "cached terminal surface was never initialized (built before \
                 ghostty init) — initializing it now"
            );
        }

        // Guard against malformed launch commands: a command that is blank or
        // contains control characters (e.g. a stray "\u{1}") would make
        // ghostty's `/bin/sh -c <cmd>` fail with "command not found" and show a
        // broken pane. Fall back to the default shell and warn so the source is
        // visible in logs if it recurs.
        let command = match command {
            Some(c) if c.trim().is_empty() || c.chars().any(|ch| ch.is_control()) => {
                tracing::warn!(
                    %panel_id,
                    raw = ?c,
                    "Ignoring malformed terminal command (blank/control chars); using default shell"
                );
                None
            }
            other => other,
        };

        let gl_surface = cached.unwrap_or_else(|| {
            let s = ghostty_gtk::surface::GhosttyGlSurface::new();
            s.set_hexpand(true);
            s.set_vexpand(true);
            s
        });

        // Match the grace-period background to the terminal theme color.
        // Priority: Omarchy theme > ghostty config background > default black.
        if let Some(ref bg) = crate::settings::omarchy_colors().background {
            gl_surface.set_initial_bg(bg);
        } else if let Some(ref hex) = self.ghostty_ui_config.borrow().background_hex() {
            gl_surface.set_initial_bg(hex);
        }

        // Check if this panel has pending scrollback to restore
        let scrollback_file = {
            let mut tm = lock_or_recover(&self.shared.tab_manager);
            tm.find_workspace_with_panel_mut(panel_id)
                .and_then(|ws| ws.panels.get_mut(&panel_id))
                .and_then(|panel| panel.pending_scrollback.take())
                .and_then(|text| write_scrollback_temp_file(panel_id, &text))
        };

        // Build environment variables for the terminal process
        let socket_path = crate::socket::server::socket_path();
        let panel_id_str = panel_id.to_string();
        let (workspace_id_str, workspace_env) = {
            let tm = lock_or_recover(&self.shared.tab_manager);
            tm.find_workspace_with_panel(panel_id)
                .map(|ws| (ws.id.to_string(), ws.env.clone()))
                .unwrap_or_default()
        };

        // Resolve shell integration directory for auto-injection
        let shell_integration_dir = shell_integration_dir();
        let (zdotdir_var, bash_env_var, original_zdotdir) =
            shell_injection_env_vars(&shell_integration_dir);

        let mut env_vars: Vec<(&str, &str)> = vec![
            ("JMUX_SOCKET", &socket_path),
            ("JMUX_PANEL_ID", &panel_id_str),
            // LC_-prefixed mirrors so an `ssh` child can forward the panel id to a
            // remote host via `SendEnv=LC_JMUX_*`: stock sshd ships `AcceptEnv
            // LANG LC_*`, and a non-locale `LC_*` name is ignored by libc. The
            // remote jmux-env.sh maps these back to JMUX_PANEL_ID/WORKSPACE_ID so
            // the remote `claude` wrapper can attribute its session-id report.
            ("LC_JMUX_PANEL_ID", &panel_id_str),
        ];
        if !workspace_id_str.is_empty() {
            env_vars.push(("JMUX_WORKSPACE_ID", &workspace_id_str));
            env_vars.push(("LC_JMUX_WORKSPACE_ID", &workspace_id_str));
        }
        // Per-workspace environment variables (from jmux.json `workspace.env`).
        for (k, v) in &workspace_env {
            env_vars.push((k.as_str(), v.as_str()));
        }
        if let Some(ref path) = scrollback_file {
            env_vars.push(("JMUX_RESTORE_SCROLLBACK_FILE", path));
        }

        // For fish, prepend the shell-integration dir to XDG_DATA_DIRS so fish
        // auto-sources `<dir>/fish/vendor_conf.d/jmux.fish` on startup. Computed
        // here so the owned String outlives the `env_vars` borrows below.
        let fish_data_dirs = fish_xdg_data_dirs(&shell_integration_dir);

        // Auto-inject shell integration via ZDOTDIR (zsh), BASH_ENV (bash),
        // or XDG_DATA_DIRS vendor_conf.d (fish).
        let shell = std::env::var("SHELL").unwrap_or_default();
        if shell.ends_with("/zsh") || shell.ends_with("/zsh5") {
            if let Some(ref dir) = zdotdir_var {
                env_vars.push(("ZDOTDIR", dir));
            }
            if let Some(ref val) = original_zdotdir {
                env_vars.push(("JMUX_ZSH_ORIGINAL_ZDOTDIR", val));
            }
        } else if shell.ends_with("/bash") {
            if let Some(ref path) = bash_env_var {
                env_vars.push(("BASH_ENV", path));
            }
        } else if shell.ends_with("/fish") {
            if let Some(ref dirs) = fish_data_dirs {
                env_vars.push(("XDG_DATA_DIRS", dirs));
            }
        }

        // When restoring a Claude Code session, forward Anthropic env vars so the
        // child process inherits the caller's API key and config dir.
        let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let anthropic_base_url = std::env::var("ANTHROPIC_BASE_URL").ok();
        let claude_config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
        let is_claude_command = command
            .map(|c| c.starts_with("claude"))
            .unwrap_or(false);
        if is_claude_command {
            if let Some(ref v) = anthropic_api_key {
                env_vars.push(("ANTHROPIC_API_KEY", v));
            }
            if let Some(ref v) = anthropic_base_url {
                env_vars.push(("ANTHROPIC_BASE_URL", v));
            }
            if let Some(ref v) = claude_config_dir {
                env_vars.push(("CLAUDE_CONFIG_DIR", v));
            }
        }

        if let Some(app) = self.ghostty_app.borrow().as_ref() {
            gl_surface.initialize_with_env(app.raw(), working_directory, command, &env_vars);
        } else {
            // The uninitialized surface is still cached (widget identity must
            // be stable), but is_initialized() stays false, so the next call
            // and the page-rebuild signature both know to finish the job.
            tracing::warn!(
                %panel_id,
                "terminal surface built before ghostty init — shell spawn deferred \
                 to the next rebuild"
            );
        }

        self.terminal_cache
            .borrow_mut()
            .insert(panel_id, gl_surface.clone());
        gl_surface
    }

    pub fn send_input_to_panel(&self, panel_id: Uuid, text: &str) -> bool {
        let surface = if let Some(surface) = self.terminal_cache.borrow().get(&panel_id).cloned() {
            surface
        } else {
            let (working_directory, command) = {
                let tab_manager = lock_or_recover(&self.shared.tab_manager);
                let Some(workspace) = tab_manager.find_workspace_with_panel(panel_id) else {
                    return false;
                };
                let Some(panel) = workspace.panel(panel_id) else {
                    return false;
                };
                if panel.panel_type != crate::model::PanelType::Terminal {
                    return false;
                }
                (panel.directory.clone(), panel.command.clone())
            };
            self.terminal_surface_for(panel_id, working_directory.as_deref(), command.as_deref())
        };

        tracing::debug!(%panel_id, text_len = text.len(), "send_input_to_panel");
        surface.send_text(text)
    }

    pub fn close_panel(&self, panel_id: Uuid, process_alive: bool) -> bool {
        {
            let mut tab_manager = lock_or_recover(&self.shared.tab_manager);
            // Closing the last tab closes its workspace; the UI refresh then
            // enforces the no-empty-app invariant (quake recreates, else quits).
            if !tab_manager.close_panel(panel_id) {
                return false;
            }
        }

        if let Some(surface) = self.terminal_cache.borrow_mut().remove(&panel_id) {
            release_surface(surface);
        }
        crate::ui::terminal_panel::unregister_vim_badge(&panel_id);
        crate::ui::textbox::unregister(&panel_id);
        lock_or_recover(&self.shared.hibernated_panels).remove(&panel_id);
        crate::ui::window::request_terminal_focus();
        self.shared.notify_ui_refresh();
        tracing::debug!(%panel_id, process_alive, "closed terminal panel");
        true
    }

    pub fn prune_terminal_cache(&self) {
        let live_panels: HashSet<Uuid> = {
            let tab_manager = lock_or_recover(&self.shared.tab_manager);
            tab_manager
                .iter()
                .flat_map(|workspace| workspace.panels.values())
                .map(|panel| panel.id)
                .collect()
        };

        // Dropping the cache entry is not enough for a surface parked in the
        // spawn nursery: the nursery still parents it, so the widget (and its
        // ghostty surface + pty) would never be disposed. Unparent it so the
        // last reference goes away with the cache entry.
        {
            let mut cache = self.terminal_cache.borrow_mut();
            let dead: Vec<Uuid> = cache
                .keys()
                .filter(|id| !live_panels.contains(id))
                .copied()
                .collect();
            for id in dead {
                if let Some(surface) = cache.remove(&id) {
                    release_surface(surface);
                }
            }
        }
        self.browser_cache
            .borrow_mut()
            .retain(|panel_id, _| live_panels.contains(panel_id));

        // Sweep the browser-panel registries too — dropping browser_cache alone
        // leaves the WebView (and its WebProcess) pinned by WEBVIEW_REGISTRY and
        // a dozen sibling maps that never removed closed panels.
        #[cfg(feature = "webkit")]
        crate::ui::browser_panel::prune_browser_panels(&live_panels);
    }

    /// Start a terminal panel's pty + child process without waiting for its
    /// workspace to be shown.
    ///
    /// Terminal surfaces normally spawn on their first visible allocation, so a
    /// workspace created in the background (a goal/graph node launched by the
    /// scheduler) would run nothing until someone selected it. This creates the
    /// surface up front — parking it in the window's never-mapped spawn nursery
    /// when no page holds it yet — and has ghostty open the pty at a synthetic
    /// size. The workspace's page is built normally later and the surface is
    /// reparented into it; its first real allocation only *resizes* the running
    /// terminal.
    ///
    /// Also covers a *selected* workspace inside a window that is not mapped
    /// (the quake drop-down at rest): selected is not the same as allocated.
    ///
    /// Returns true when the pty is running. False means "not yet" — the surface
    /// is about to be allocated anyway, no window is realized, or GL/ghostty
    /// refused — and it is always safe to call again.
    pub fn spawn_panel_headless(&self, panel_id: Uuid) -> bool {
        if let Some(surface) = self.terminal_cache.borrow().get(&panel_id) {
            if surface.has_spawned() {
                return true;
            }
            // Mapped: GTK will allocate it on the next frame and the normal path
            // spawns with the real size. Don't race that with a synthetic one.
            if surface.is_mapped() {
                return false;
            }
        }

        let (directory, command) = {
            let tab_manager = lock_or_recover(&self.shared.tab_manager);
            let Some(workspace) = tab_manager.find_workspace_with_panel(panel_id) else {
                return false;
            };
            let Some(panel) = workspace.panel(panel_id) else {
                return false;
            };
            if panel.panel_type != crate::model::PanelType::Terminal {
                return false;
            }
            (panel.directory.clone(), panel.command.clone())
        };

        let surface =
            self.terminal_surface_for(panel_id, directory.as_deref(), command.as_deref());
        if surface.has_spawned() {
            return true;
        }
        if surface.is_mapped() {
            return false;
        }
        // Unparented surfaces need a home inside a realized window to get a GL
        // context; one already sitting in an unmapped page can spawn in place.
        if surface.parent().is_none() {
            let Some(nursery) = crate::ui::window::spawn_nursery() else {
                tracing::debug!(
                    %panel_id,
                    "headless spawn deferred — no realized window to park the surface in"
                );
                return false;
            };
            nursery.append(&surface);
        }
        let spawned = surface.spawn_headless(HEADLESS_SPAWN_WIDTH_PX, HEADLESS_SPAWN_HEIGHT_PX);
        if !spawned {
            tracing::warn!(%panel_id, "headless spawn failed — terminal will start when shown");
        }
        spawned
    }

    /// Look up a cached browser widget by panel ID.
    pub fn get_cached_browser(&self, id: Uuid) -> Option<gtk4::Widget> {
        self.browser_cache.borrow().get(&id).cloned()
    }

    /// Store a browser widget in the cache.
    pub fn cache_browser(&self, id: Uuid, widget: gtk4::Widget) {
        self.browser_cache.borrow_mut().insert(id, widget);
    }
}

/// Quick-terminal (drop-down) action requested by a hotkey or the socket.
#[derive(Debug, Clone, Copy)]
pub enum QuickTermAction {
    Toggle,
    Show,
    Hide,
}

/// Messages from background tasks that require a UI refresh.
#[derive(Debug)]
pub enum UiEvent {
    Refresh,
    /// Metadata-only refresh — sidebar + window title, no layout rebuild.
    /// Used by socket handlers that update directory, git branch, ports, etc.
    MetadataRefresh,
    /// Show a transient toast message in the window.
    ShowToast(String),
    SendInput {
        panel_id: Uuid,
        text: String,
    },
    SearchTotal,
    SearchSelected,
    StartSearch,
    EndSearch,
    OpenSettings,
    /// Open the "Run goal" dialog, optionally with the goal box filled in
    /// (`jmux goal --configure "..."`).
    OpenGoalDialog {
        prefill: Option<String>,
    },
    TriggerFlash {
        panel_id: Uuid,
    },
    SendKey {
        panel_id: Uuid,
        keyval: u32,
        keycode: u32,
        mods: u32,
    },
    ReadText {
        panel_id: Uuid,
        /// Read the full scrollback buffer instead of just the visible screen.
        scrollback: bool,
        /// Keep only the last N lines of the result (None = all).
        lines: Option<usize>,
        reply: tokio::sync::oneshot::Sender<Option<String>>,
    },
    RefreshSurface {
        panel_id: Uuid,
    },
    ClearHistory {
        panel_id: Uuid,
    },
    ToggleNotifications,
    #[allow(dead_code)]
    ToggleMinimalMode,
    RenameTab {
        panel_id: Uuid,
    },
    SetTitle {
        surface: SendSurfacePtr,
        title: String,
    },
    SetPwd {
        surface: SendSurfacePtr,
        directory: String,
    },
    OpenFolderAsWorkspace,
    CopyMode {
        panel_id: Uuid,
    },
    ReopenClosedBrowser,
    /// Open a URL in a new browser tab within the same pane as the source panel.
    BrowserOpenInNewTab {
        source_panel_id: Uuid,
        url: String,
    },
    OpenMarkdownFile,
    #[cfg(feature = "webkit")]
    BrowserAction {
        panel_id: Uuid,
        action: crate::ui::browser_panel::BrowserActionKind,
    },
    /// Open a URL in a new browser panel (routed from terminal hyperlinks).
    OpenUrlInBrowser {
        url: String,
    },
    /// Desktop notification triggered from terminal (OSC 9/777).
    DesktopNotification {
        surface: SendSurfacePtr,
        title: String,
        body: String,
    },
    /// Create a new application window.
    CreateWindow,
    /// List connected monitors (names), replying with their connector names.
    ListDisplays {
        reply: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    /// Place the focused window on a named (or 0-based index) monitor. Wayland
    /// disallows positioning a normal window, so this fullscreens on the target
    /// monitor — the closest portable behavior. Replies with the matched
    /// monitor name, or an error.
    WindowToDisplay {
        monitor: String,
        reply: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
    /// Reload ghostty configuration from disk.
    ReloadConfig,
    /// Re-apply the app theme from settings (adw color scheme + ghostty color scheme).
    /// Used by the `settings.reload` socket command and the Omarchy SIGUSR2 path.
    ReloadTheme,
    /// Show the SSH workspace creation dialog.
    OpenSshDialog,
    /// Open an SSH workspace from a deep link (jmux://ssh/user@host[:port][/path]).
    OpenSshDeepLink {
        /// SSH destination string (e.g. "user@host").
        destination: String,
        /// Optional port number.
        port: Option<u16>,
    },
    /// Import browser cookies from a local profile (Firefox/Chrome/Chromium).
    /// Must be handled on the GTK main thread.
    #[cfg(feature = "webkit")]
    ImportBrowserCookies {
        source: crate::browser_import::ImportSource,
        reply: tokio::sync::oneshot::Sender<(usize, Option<String>)>,
    },
    /// Connect a remote SSH workspace.
    RemoteConnect {
        workspace_id: Uuid,
    },
    /// Disconnect a remote SSH workspace.
    RemoteDisconnect {
        workspace_id: Uuid,
    },
    /// Remote connection state changed (from background thread).
    RemoteStateChanged {
        workspace_id: Uuid,
        state: crate::remote::session::RemoteState,
    },
    /// Defer unread: mark all unread notifications for the current workspace as read
    /// (effectively postponing the unread badge).
    DeferUnread,
    /// Toggle unread: if there are unread notifications for the current workspace,
    /// mark them all read; otherwise mark the most recent one unread.
    ToggleUnread,
    /// Detect the agent running in the focused panel and send its resume command.
    AgentResume,
    /// Open the Task Manager window.
    OpenTaskManager,
    /// Open the pane overview grid.
    OpenOverview,
    /// Open the command palette.
    OpenCommandPalette,
    /// Show the Dock panel.
    ShowDock,
    /// Run a custom command (from jmux.json) by name.
    RunCustomCommand(String),
    /// Toggle / show / hide the Quake-style drop-down quick terminal.
    QuickTerminal(QuickTermAction),
    /// Show or hide the left sidebar.
    /// `true` = show (expand), `false` = hide (collapse).
    ShowSidebar(bool),
    /// Toggle the left sidebar visibility (flip current state).
    ToggleSidebar,
}

/// Wrapper to send a raw ghostty_surface_t across threads.
#[derive(Clone, Copy)]
pub struct SendSurfacePtr(pub ghostty_surface_t);
// SAFETY: SendSurfacePtr wraps a ghostty_surface_t (opaque C pointer) that is
// sent via channels from background threads to the GTK main thread. The pointer
// is only dereferenced on the main thread. Sync is intentionally not implemented
// — the pointer must not be shared concurrently across threads.
unsafe impl Send for SendSurfacePtr {}
impl std::fmt::Debug for SendSurfacePtr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SendSurfacePtr")
            .field(&(self.0 as *const ()))
            .finish()
    }
}

/// Thread-safe state shared between GTK main thread and socket server.
/// The socket server reads/writes through this, then signals the GTK main thread
/// via glib channels for UI updates.
pub struct SharedState {
    pub tab_manager: Mutex<TabManager>,
    pub notifications: Mutex<NotificationStore>,
    /// Stack of recently closed browser panel URLs (for reopen).
    pub closed_browser_urls: Mutex<Vec<String>>,
    /// Per-window dimensions (width, height) for session persistence.
    pub window_sizes: Mutex<HashMap<Uuid, (i32, i32)>>,
    /// Per-window UI event senders.
    ui_event_txs: Mutex<HashMap<Uuid, UnboundedSender<UiEvent>>>,
    /// Active remote SSH sessions keyed by workspace ID.
    pub remote_sessions: Mutex<HashMap<Uuid, crate::remote::session::SharedRemoteSession>>,
    /// Panels whose agent process group is currently hibernated (SIGSTOP'd).
    pub hibernated_panels: Mutex<HashSet<Uuid>>,
    /// Active goal runs (`jmux goal`), keyed by workspace id.
    pub goals: Mutex<crate::goal::GoalRegistry>,
    /// Known goal graphs (`jmux graph`), keyed by graph name.
    pub graphs: Mutex<crate::goal::graph::GraphRegistry>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            tab_manager: Mutex::new(TabManager::new()),
            notifications: Mutex::new(NotificationStore::new()),
            closed_browser_urls: Mutex::new(Vec::new()),
            window_sizes: Mutex::new(HashMap::new()),
            ui_event_txs: Mutex::new(HashMap::new()),
            remote_sessions: Mutex::new(HashMap::new()),
            hibernated_panels: Mutex::new(HashSet::new()),
            goals: Mutex::new(crate::goal::GoalRegistry::new()),
            graphs: Mutex::new(crate::goal::graph::GraphRegistry::new()),
        }
    }

    /// Whether a panel's agent is currently hibernated.
    pub fn is_hibernated(&self, panel_id: &Uuid) -> bool {
        lock_or_recover(&self.hibernated_panels).contains(panel_id)
    }

    /// Hibernate the agent running in `panel_id` (by its reported TTY).
    /// Returns true if a process group was signalled.
    pub fn hibernate_panel(&self, panel_id: Uuid) -> bool {
        let tty = {
            let tm = lock_or_recover(&self.tab_manager);
            tm.find_workspace_with_panel(panel_id)
                .and_then(|ws| ws.panel(panel_id))
                .and_then(|p| p.tty_name.clone())
        };
        let Some(tty) = tty else { return false };
        if crate::hibernate::hibernate(&tty) {
            lock_or_recover(&self.hibernated_panels).insert(panel_id);
            true
        } else {
            false
        }
    }

    /// Resume the agent running in `panel_id`.
    pub fn wake_panel(&self, panel_id: Uuid) -> bool {
        let tty = {
            let tm = lock_or_recover(&self.tab_manager);
            tm.find_workspace_with_panel(panel_id)
                .and_then(|ws| ws.panel(panel_id))
                .and_then(|p| p.tty_name.clone())
        };
        // Always clear the flag; attempt SIGCONT if we have a tty.
        lock_or_recover(&self.hibernated_panels).remove(&panel_id);
        match tty {
            Some(tty) => crate::hibernate::wake(&tty),
            None => false,
        }
    }

    pub fn install_ui_event_sender(&self, window_id: Uuid, sender: UnboundedSender<UiEvent>) {
        lock_or_recover(&self.ui_event_txs).insert(window_id, sender);
    }

    pub fn remove_ui_event_sender(&self, window_id: &Uuid) {
        lock_or_recover(&self.ui_event_txs).remove(window_id);
    }

    /// List all registered window IDs.
    pub fn window_ids(&self) -> Vec<Uuid> {
        lock_or_recover(&self.ui_event_txs)
            .keys()
            .copied()
            .collect()
    }

    /// Send a UI event to the first registered window (primary).
    /// Most events (socket commands, notifications) target the active window.
    pub fn send_ui_event(&self, event: UiEvent) -> bool {
        let txs = lock_or_recover(&self.ui_event_txs);
        txs.values().next().is_some_and(|tx| tx.send(event).is_ok())
    }

    /// Send a UI event to a specific window.
    pub fn send_ui_event_to(&self, window_id: &Uuid, event: UiEvent) -> bool {
        lock_or_recover(&self.ui_event_txs)
            .get(window_id)
            .is_some_and(|tx| tx.send(event).is_ok())
    }

    pub fn notify_ui_refresh(&self) {
        let _ = self.send_ui_event(UiEvent::Refresh);
    }

    /// Request a metadata-only UI refresh (sidebar + window title).
    /// Does NOT trigger `rebuild_content`, so browser/terminal panels
    /// are not unparented.  Use for directory, git branch, ports, and
    /// other sidebar-displayed metadata updates from socket handlers.
    pub fn notify_metadata_refresh(&self) {
        let _ = self.send_ui_event(UiEvent::MetadataRefresh);
    }

    /// Push a closed browser URL onto the reopen stack (max 20).
    pub fn push_closed_browser_url(&self, url: String) {
        let mut stack = lock_or_recover(&self.closed_browser_urls);
        if stack.len() >= 20 {
            stack.remove(0);
        }
        stack.push(url);
    }

    /// Pop the most recently closed browser URL.
    pub fn pop_closed_browser_url(&self) -> Option<String> {
        lock_or_recover(&self.closed_browser_urls).pop()
    }

    /// Stop and remove remote sessions whose workspace no longer exists.
    ///
    /// Called during UI refresh so all workspace removal paths (sidebar close,
    /// socket close, close-others/above/below, keyboard shortcuts) are covered
    /// by a single check-point rather than requiring explicit dispatch at each
    /// call site.
    pub fn cleanup_stale_remote_sessions(&self) {
        // Collect live workspace IDs, then release the lock before touching
        // remote_sessions to avoid holding two locks simultaneously.
        let live_ids: HashSet<Uuid> = {
            let tm = lock_or_recover(&self.tab_manager);
            tm.iter().map(|ws| ws.id).collect()
        };

        let stale: Vec<(Uuid, crate::remote::session::SharedRemoteSession)> = {
            let mut sessions = lock_or_recover(&self.remote_sessions);
            if sessions.is_empty() {
                return;
            }
            let stale_keys: Vec<Uuid> = sessions
                .keys()
                .filter(|id| !live_ids.contains(id))
                .copied()
                .collect();
            if stale_keys.is_empty() {
                return;
            }
            stale_keys
                .into_iter()
                .filter_map(|id| sessions.remove(&id).map(|s| (id, s)))
                .collect()
        };

        for (ws_id, session) in stale {
            lock_or_recover(&session).stop();
            tracing::info!(%ws_id, "Cleaned up orphaned remote session");
        }
    }
}

/// Run the GTK application. Returns the exit code.
pub fn run() -> i32 {
    // Single-instance: the first process registers on the session bus and owns
    // the CLI socket; a second `jmux-app` launch is forwarded here (activate →
    // new window, open → route the jmux:// URI into the running instance)
    // instead of starting a rival process that can't bind the socket.
    // `JMUX_APP_ID` gives a build its own identity, so a test instance runs
    // beside the one you use all day instead of just activating its window.
    // Pair it with `XDG_RUNTIME_DIR` (its own CLI socket) and
    // `JMUX_DISABLE_SESSION_RESTORE=1` (its own empty session).
    let app_id = std::env::var("JMUX_APP_ID")
        .ok()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| "com.jacobbriggs.jmux".to_string());
    let app = adw::Application::builder()
        .application_id(&app_id)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    let shared = Arc::new(SharedState::new());
    let state = Rc::new(AppState::new(shared.clone()));

    {
        let shared_for_socket = shared.clone();
        let shared_for_ports = shared.clone();
        let state_for_goal = state.clone();
        app.connect_startup(move |startup_app| {
            // Use the bundled app icon for windows / alt-tab / taskbar.
            gtk4::Window::set_default_icon_name("com.jacobbriggs.jmux");
            let shared = shared_for_socket.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                rt.block_on(async {
                    if let Err(e) = socket::server::run_socket_server(shared).await {
                        tracing::error!("Socket server error: {}", e);
                    }
                });
            });

            crate::port_scanner::spawn(shared_for_ports.clone());

            // Sub-agent monitor sync (no-op per tick unless a workspace has
            // the monitor toggled on).
            crate::agent_monitor::start_ticker(shared_for_ports.clone());

            // Goal auto-driver (no-op per tick unless a goal is registered).
            crate::goal::start_driver(state_for_goal.clone());

            // Register the quick-terminal global hotkey (GlobalShortcuts portal)
            // if the feature build + setting are enabled.
            crate::ui::quick_terminal::register_global_shortcut(
                startup_app,
                shared_for_ports.clone(),
            );
        });
    }

    // One-time heavy init (ghostty, session restore, first windows) runs once
    // per process. `activate` fires on launch and on every subsequent launch
    // forwarded from a second `jmux-app`; the first does init, the rest open a
    // new window.
    let initialized = Rc::new(std::cell::Cell::new(false));

    let state_clone = state.clone();
    let init_flag = initialized.clone();
    app.connect_activate(move |app| {
        let first = !init_flag.replace(true);
        let quake = quake_mode();
        if first {
            first_launch_init(app, &state_clone, quake);
            if quake {
                // Resident quake daemon: keep the process alive while the
                // console is hidden so the global hotkey always has something to
                // toggle. hold() returns a guard that releases on drop; forget
                // it so the hold lasts the whole process (jmux is quit
                // explicitly, e.g. `pkill jmux-app`), not just this closure.
                std::mem::forget(app.hold());
            }
        }
        if quake {
            // Quake-console mode: every launch (first run, `jmux-app` re-launch,
            // launcher/icon) drops the console down. Single instance, never a
            // second main window. `handle` creates the drop-down on first use.
            // Exception: an autostart launch (JMUX_QUAKE_HIDDEN) comes up hidden
            // — resident and ready, but not in the user's face at login.
            let action = if first && std::env::var_os("JMUX_QUAKE_HIDDEN").is_some() {
                QuickTermAction::Hide
            } else {
                QuickTermAction::Show
            };
            crate::ui::quick_terminal::handle(
                action,
                app.upcast_ref::<gtk4::Application>(),
                &state_clone,
            );
        } else if !first {
            // Normal mode: a re-launch raises the existing window.
            present_main_window(app, &state_clone);
        }
    });

    // Handle jmux:// deep links opened via xdg-open / D-Bus.
    // The .desktop file declares MimeType=x-scheme-handler/jmux so xdg-open
    // routes jmux:// URIs here. With a single instance, `open` is forwarded to
    // the running process and routed into a current window. If the very first
    // launch is via a URI (no prior activate), initialize first.
    {
        let state_clone = state.clone();
        let init_flag = initialized.clone();
        app.connect_open(move |app, files, _hint| {
            if !init_flag.replace(true) {
                first_launch_init(app, &state_clone, quake_mode());
            }
            for file in files {
                let uri = file.uri();
                let uri_str = uri.as_str();
                tracing::info!("Deep link received: {}", uri_str);
                if let Some(event) = parse_deep_link(uri_str) {
                    state_clone.shared.send_ui_event(event);
                } else {
                    tracing::warn!("Unrecognised jmux:// deep link: {}", uri_str);
                }
            }
        });
    }

    {
        let state = state.clone();
        app.connect_shutdown(move |_app| {
            // Save session before shutdown — capture full scrollback here (the
            // one place we pay that cost), so a clean quit/SIGTERM preserves it.
            // Blocking on the /proc walks is fine here: we're quitting anyway.
            let mut snapshot = session::store::create_snapshot(&state, true);
            session::store::enrich_snapshot_with_live_proc(&mut snapshot);
            if let Err(e) = session::store::save_session(&snapshot) {
                tracing::error!("Failed to save session on shutdown: {}", e);
            }

            // Drain all remote sessions so SSH processes are killed before exit.
            let sessions: Vec<_> = lock_or_recover(&state.shared.remote_sessions)
                .drain()
                .collect();
            for (_ws_id, session) in sessions {
                lock_or_recover(&session).stop();
            }

            if let Ok(mut ptr) = GHOSTTY_APP_PTR.lock() {
                *ptr = SendAppPtr(std::ptr::null_mut());
            }
            GHOSTTY_TICK_PENDING.store(false, Ordering::Release);
            socket::server::cleanup();
            tracing::info!("Application shutdown");
        });
    }

    // Quit cleanly on SIGTERM (e.g. `pkill jmux-app`) so `connect_shutdown` runs
    // and the shutdown snapshot — the only one that captures scrollback now — is
    // written before exit. The handler restores the default disposition, so a
    // second SIGTERM still terminates hard if the loop is wedged.
    {
        unsafe {
            libc::signal(
                libc::SIGTERM,
                sigterm_handler as *const () as libc::sighandler_t,
            );
        }
        let app_weak = app.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            if !SIGTERM_RECEIVED.load(Ordering::Relaxed) {
                return glib::ControlFlow::Continue;
            }
            if let Some(app) = app_weak.upgrade() {
                tracing::info!("SIGTERM received — quitting to save session");
                app.quit();
            }
            glib::ControlFlow::Break
        });
    }

    app.run().into()
}

static SIGTERM_RECEIVED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigterm_handler(sig: libc::c_int) {
    SIGTERM_RECEIVED.store(true, Ordering::Relaxed);
    // Restore default so a second SIGTERM terminates immediately.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
    }
}

/// One-time per-process initialization: ghostty, theme, session restore, and
/// the first window(s). Runs on the first `activate`/`open` only.
fn first_launch_init(app: &adw::Application, state: &Rc<AppState>, quake: bool) {
    // Remove socket password from environment so child terminal processes
    // cannot read it. The password is already cached by socket_password().
    std::env::remove_var("JMUX_SOCKET_PASSWORD");

    // Apply saved theme preference
    apply_theme_from_settings();

    // Initialize browser history and profiles (loads from disk)
    crate::browser_history::init();
    #[cfg(feature = "webkit")]
    crate::browser_profiles::init();

    // Register SIGUSR2 handler for Omarchy live theme switching.
    // Signal handler sets an AtomicBool; a glib timer polls it.
    install_sigusr2_theme_reload();

    init_ghostty(state);

    // Restore session after ghostty is initialized so terminals can be created
    let restored_window_ids = restore_session(state);

    // Open windows — either restored or a fresh default. In quake-console mode
    // we open no main window at all; `activate` drops the quick terminal down
    // instead (the only window jmux shows).
    if quake {
        // no-op: the drop-down console is the entire UI
    } else if restored_window_ids.is_empty() {
        open_window(app, state, Uuid::new_v4());
    } else {
        for window_id in restored_window_ids {
            open_window(app, state, window_id);
        }
    }

    // Auto-reconnect remote workspaces from restored session
    if crate::settings::load().remote_ssh_enabled {
        let remote_ws_ids: Vec<Uuid> = {
            let tm = lock_or_recover(&state.shared.tab_manager);
            tm.iter()
                .filter(|ws| ws.remote_config.is_some())
                .map(|ws| ws.id)
                .collect()
        };
        for ws_id in remote_ws_ids {
            state.shared.send_ui_event(UiEvent::RemoteConnect {
                workspace_id: ws_id,
            });
        }
    }

    // Start periodic autosave (every 8 seconds, matching macOS jmux)
    {
        let state = state.clone();
        let app_weak = app.downgrade();
        glib::timeout_add_local(std::time::Duration::from_secs(8), move || {
            // Capture current window sizes from all windows
            if let Some(app) = app_weak.upgrade() {
                for win in app.windows() {
                    if let Some(win) = win.downcast_ref::<adw::ApplicationWindow>() {
                        if let Ok(wid) = Uuid::parse_str(&win.widget_name()) {
                            let w = win.width();
                            let h = win.height();
                            if w > 0 && h > 0 {
                                lock_or_recover(&state.shared.window_sizes).insert(wid, (w, h));
                            }
                        }
                    }
                }
            }
            // Periodic autosave: structure only (no per-terminal scrollback
            // read), so it stays cheap no matter how long the daemon runs. Write
            // it off the main loop — save_session fsyncs, which under I/O load
            // would otherwise freeze the GLib main loop (and so the Ctrl+`
            // hotkey) every 8s. Skip a tick if a prior write is still running.
            let snapshot = session::store::create_snapshot(&state, false);
            static AUTOSAVE_WRITING: AtomicBool = AtomicBool::new(false);
            if !AUTOSAVE_WRITING.swap(true, Ordering::AcqRel) {
                std::thread::spawn(move || {
                    // The /proc walks (live cwds, claude session resolution)
                    // also live on this thread — they read hundreds of files
                    // and would hitch the main loop just like the fsync.
                    let mut snapshot = snapshot;
                    session::store::enrich_snapshot_with_live_proc(&mut snapshot);
                    if let Err(e) = session::store::save_session(&snapshot) {
                        tracing::warn!("Autosave failed: {}", e);
                    }
                    AUTOSAVE_WRITING.store(false, Ordering::Release);
                });
            }
            // Flush browser history to disk
            crate::browser_history::flush();
            glib::ControlFlow::Continue
        });
    }

    // Memory watchdog. jmux has been OOM-killed (uncatchable SIGKILL) after
    // leaking to tens of GB, which skips the shutdown save — and the periodic
    // autosave above never captures scrollback. Poll our own RSS and, once it
    // crosses a threshold, force a *full* synchronous save (scrollback and all)
    // before the kernel gets to us. Re-fires each additional GB so the last
    // save on disk is as fresh as possible when the kill lands.
    {
        let state = state.clone();
        let threshold_mb: u64 = std::env::var("JMUX_MEMORY_SAVE_THRESHOLD_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4096);
        let threshold = threshold_mb.saturating_mul(1024 * 1024);
        let mut last_saved_at: u64 = 0;
        glib::timeout_add_local(std::time::Duration::from_secs(5), move || {
            if let Some(rss) = current_rss_bytes() {
                // Once over the threshold, save again for every extra GB grown.
                let step: u64 = 1024 * 1024 * 1024;
                if rss >= threshold && rss >= last_saved_at.saturating_add(step) {
                    tracing::warn!(
                        "Memory watchdog: RSS {} MB ≥ {} MB threshold — forcing full \
                         session save before a possible OOM kill",
                        rss / (1024 * 1024),
                        threshold_mb
                    );
                    let mut snapshot = session::store::create_snapshot(&state, true);
                    session::store::enrich_snapshot_with_live_proc(&mut snapshot);
                    if let Err(e) = session::store::save_session(&snapshot) {
                        tracing::error!("Watchdog save failed: {}", e);
                    }
                    last_saved_at = rss;
                }
            }
            glib::ControlFlow::Continue
        });
    }
}

/// Current resident set size (RSS) of this process in bytes, from
/// `/proc/self/statm`. Returns `None` if it can't be read/parsed.
fn current_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    // Fields: size resident shared text lib data dt (in pages).
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf(_SC_PAGESIZE) is always safe to call.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return None;
    }
    Some(resident_pages.saturating_mul(page_size as u64))
}

/// Open a new window with its own event channel and workspace set.
pub fn open_window(app: &adw::Application, state: &Rc<AppState>, window_id: Uuid) {
    let (ui_event_tx, ui_event_rx) = tokio::sync::mpsc::unbounded_channel();
    state.shared.install_ui_event_sender(window_id, ui_event_tx);

    // If no workspaces are assigned to this window, create a default one
    // (unless this is first launch — the welcome screen handles that)
    {
        let mut tm = lock_or_recover(&state.shared.tab_manager);
        let has_workspaces = tm.iter().any(|ws| ws.window_id == Some(window_id));
        if !has_workspaces && !crate::ui::welcome::should_show_welcome() {
            let mut ws = crate::model::Workspace::new();
            ws.window_id = Some(window_id);
            tm.add_workspace(ws);
        }
    }

    let window = ui::window::create_window(app, state, window_id, ui_event_rx, false);
    window.present();
}

/// True when this build supports the drop-down quick terminal *and* the user
/// has enabled it. In that "quake console" mode jmux is a single-instance
/// drop-down: every launch drops the console down and no main window is opened.
pub(crate) fn quake_mode() -> bool {
    cfg!(feature = "quick-terminal") && crate::settings::load().quick_terminal.enabled
}

/// Raise the existing main window (single-window app). Skips the quick-terminal
/// drop-down. If somehow no main window exists, opens a fresh one.
pub fn present_main_window(app: &adw::Application, state: &Rc<AppState>) {
    #[cfg(feature = "quick-terminal")]
    let quick_id = crate::ui::quick_terminal::quick_window_id().to_string();
    for win in app.windows() {
        #[cfg(feature = "quick-terminal")]
        if win.widget_name() == quick_id {
            continue; // never surface the drop-down for a launcher re-activate
        }
        win.present();
        return;
    }
    // No main window present (all closed) — open one.
    open_window(app, state, Uuid::new_v4());
}

/// Remove all scrollback temp files left from the previous session.
///
/// Files in `~/.cache/jmux/scrollback/` are written at session-restore time
/// and read by the shell integration script.  They should be cleaned up after
/// the session starts, but since there is no reliable "terminal has read it"
/// signal, we clean up at the start of the *next* session instead.
fn cleanup_scrollback_temp_files() {
    let dir = dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("jmux/scrollback");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().extension().is_some_and(|e| e == "txt") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Restore workspaces from a saved session. Returns window IDs for each restored window.
///
/// Session restore can be disabled by setting `JMUX_DISABLE_SESSION_RESTORE=1`.
/// Best working directory for a new tab opened from `ws`: the focused
/// terminal's *live* cwd, read straight from `/proc`.
///
/// We locate the shell process by the `JMUX_PANEL_ID` env var jmux sets on
/// every terminal — so this works even when shell-integration pwd reporting is
/// inactive. Falls back to the last-reported directory, then the workspace dir.
pub fn new_tab_directory(ws: &crate::model::Workspace) -> Option<String> {
    if let Some(panel_id) = ws.focused_panel_id {
        if let Some(cwd) = live_cwd_for_panel(panel_id).filter(|d| !d.is_empty()) {
            return Some(cwd);
        }
    }
    ws.inherited_terminal_directory()
}

/// Read the live cwd of the shell backing `panel_id` from `/proc`. Finds the
/// process whose `JMUX_PANEL_ID` env var matches (preferring the shell — a
/// direct child of this process) and returns `/proc/<pid>/cwd`.
fn live_cwd_for_panel(panel_id: Uuid) -> Option<String> {
    let needle = format!("JMUX_PANEL_ID={panel_id}");
    let me = std::process::id();
    let mut fallback: Option<(u32, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let proc_dir = entry.path();
        // environ is NUL-separated KEY=VALUE; only readable for our own children.
        let Ok(environ) = std::fs::read(proc_dir.join("environ")) else {
            continue;
        };
        if !environ.split(|&b| b == 0).any(|kv| kv == needle.as_bytes()) {
            continue;
        }
        // The shell is a direct child of the jmux process; its cwd tracks `cd`.
        let ppid = std::fs::read_to_string(proc_dir.join("stat"))
            .ok()
            .and_then(|s| parse_ppid(&s));
        if ppid == Some(me) {
            return read_cwd(&proc_dir);
        }
        if fallback.as_ref().map(|(p, _)| pid < *p).unwrap_or(true) {
            fallback = Some((pid, proc_dir));
        }
    }
    fallback.and_then(|(_, dir)| read_cwd(&dir))
}

fn read_cwd(proc_dir: &std::path::Path) -> Option<String> {
    std::fs::read_link(proc_dir.join("cwd"))
        .ok()
        .and_then(|p| p.to_str().map(String::from))
}

/// Parse the ppid (field 4) from a `/proc/<pid>/stat` line, skipping the
/// `(comm)` field which may itself contain spaces or parens.
fn parse_ppid(stat: &str) -> Option<u32> {
    stat_field_after_comm(stat, 1).map(|v| v as u32) // state, ppid
}

/// Parse a 0-indexed field after the `(comm)` field of `/proc/<pid>/stat`.
/// Fields: 0=state 1=ppid 2=pgrp 3=session 4=tty_nr 5=tpgid …
fn stat_field_after_comm(stat: &str, idx: usize) -> Option<i64> {
    let after_comm = stat.rfind(')')?;
    stat[after_comm + 1..]
        .trim()
        .split_ascii_whitespace()
        .nth(idx)
        .and_then(|s| s.parse().ok())
}

/// Find the shell process backing `panel_id` (matched by the `JMUX_PANEL_ID`
/// env var jmux sets), preferring the direct child of this process.
fn shell_pid_for_panel(panel_id: Uuid) -> Option<u32> {
    let needle = format!("JMUX_PANEL_ID={panel_id}");
    let me = std::process::id();
    let mut fallback: Option<u32> = None;
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let proc_dir = entry.path();
        let Ok(environ) = std::fs::read(proc_dir.join("environ")) else {
            continue;
        };
        if !environ.split(|&b| b == 0).any(|kv| kv == needle.as_bytes()) {
            continue;
        }
        let ppid = std::fs::read_to_string(proc_dir.join("stat"))
            .ok()
            .and_then(|s| parse_ppid(&s));
        if ppid == Some(me) {
            return Some(pid);
        }
        fallback.get_or_insert(pid);
    }
    fallback
}

/// Whether a terminal pane is "busy" — a foreground command is running rather
/// than the shell sitting at its prompt. `None` if it can't be determined.
pub fn pane_is_busy(panel_id: Uuid) -> Option<bool> {
    let shell_pid = shell_pid_for_panel(panel_id)?;
    let stat = std::fs::read_to_string(format!("/proc/{shell_pid}/stat")).ok()?;
    // tpgid = foreground process group of the controlling terminal. When it
    // equals the shell's own pid, the shell is in the foreground (idle).
    let tpgid = stat_field_after_comm(&stat, 5)?;
    Some(tpgid > 0 && tpgid as u32 != shell_pid)
}

/// Build the local command that reconnects a remote tab over SSH and resumes a
/// captured Claude session directly on the far host: `ssh -t <opts> <dest> 'cd
/// <dir>; exec claude --resume <id>'`. Mirrors the SSH flag construction in
/// `handle_workspace_create_ssh` and adds `-t` so Claude gets a real tty. The
/// session id is a uuid (safe), but is escaped alongside the rest for defence.
fn build_remote_claude_resume(
    remote_config: &crate::remote::session::RemoteConfig,
    directory: Option<&str>,
    session_id: &str,
) -> String {
    let mut cmd = "ssh -t".to_string();
    if remote_config.agent_forward {
        cmd += " -A";
    }
    if let Some(port) = remote_config.port {
        cmd += &format!(" -p {port}");
    }
    if let Some(ref identity) = remote_config.identity {
        cmd += &format!(" -i {}", shell_escape::escape(identity.into()));
    }
    cmd += &format!(" {}", shell_escape::escape(remote_config.destination.clone().into()));

    // Remote-side command: cd into the tab's directory (best-effort — a missing
    // dir shouldn't block the resume) then exec Claude resuming the session.
    let escaped_id = shell_escape::escape(session_id.into());
    let remote_cmd = match directory {
        Some(dir) => format!(
            "cd {} 2>/dev/null; exec claude --resume {}",
            shell_escape::escape(dir.into()),
            escaped_id
        ),
        None => format!("exec claude --resume {}", escaped_id),
    };
    cmd += &format!(" {}", shell_escape::escape(remote_cmd.into()));
    cmd
}

/// Reconstruct a `Workspace` from its session snapshot (used for both live
/// workspaces and the persisted closed-history entries).
fn build_workspace_from_snapshot(
    ws_snapshot: &session::snapshot::SessionWorkspaceSnapshot,
    window_id: Option<Uuid>,
    agent_restore_settings: &crate::settings::AgentRestoreSettings,
) -> crate::model::Workspace {
    let mut workspace = crate::model::Workspace::with_directory(&ws_snapshot.current_directory);
    workspace.window_id = window_id;
    workspace.custom_title = ws_snapshot.custom_title.clone();
    workspace.custom_color = ws_snapshot.custom_color.clone();
    workspace.is_pinned = ws_snapshot.is_pinned;
    workspace.process_title = ws_snapshot.process_title.clone();
    workspace.status_entries = ws_snapshot.status_entries.clone();
    workspace.log_entries = ws_snapshot.log_entries.clone();
    workspace.progress = ws_snapshot.progress.clone();
    workspace.git_branch = ws_snapshot.git_branch.clone();
    workspace.remote_config = ws_snapshot.remote_config.clone();
    workspace.group_id = ws_snapshot.group_id;

    let layout = ws_snapshot.layout.to_layout();
    let mut panels = std::collections::HashMap::new();
    for panel_snapshot in &ws_snapshot.panels {
        let panel_type = match panel_snapshot.panel_type.as_str() {
            #[cfg(feature = "webkit")]
            "browser" => crate::model::PanelType::Browser,
            #[cfg(feature = "webkit")]
            "markdown" => crate::model::PanelType::Markdown,
            #[cfg(not(feature = "webkit"))]
            "browser" | "markdown" => continue, // skip browser panels when webkit disabled
            "diff" => crate::model::PanelType::Diff,
            "project" => crate::model::PanelType::Project,
            "file_preview" => crate::model::PanelType::FilePreview,
            "notes" => crate::model::PanelType::Notes,
            "history" => crate::model::PanelType::History,
            "vault" => crate::model::PanelType::Vault,
            // Must restore as Graph: its `command` field holds the graph
            // name, which a Terminal fallback would execute as a shell command.
            "graph" => crate::model::PanelType::Graph,
            _ => crate::model::PanelType::Terminal,
        };
        let panel = crate::model::panel::Panel {
            id: panel_snapshot.id,
            panel_type,
            title: panel_snapshot.title.clone(),
            custom_title: panel_snapshot.custom_title.clone(),
            directory: panel_snapshot.directory.clone(),
            is_pinned: panel_snapshot.is_pinned,
            is_manually_unread: panel_snapshot.is_manually_unread,
            git_branch: panel_snapshot.git_branch.clone(),
            listening_ports: panel_snapshot.listening_ports.clone(),
            tty_name: panel_snapshot.tty_name.clone(),
            browser_url: panel_snapshot
                .browser
                .as_ref()
                .and_then(|b| b.url_string.clone()),
            markdown_file: panel_snapshot.markdown.as_ref().map(|m| m.file_path.clone()),
            command: {
                let claude_enabled = agent_restore_settings.is_enabled_for("claude");
                let claude_id = panel_snapshot
                    .agent_session_id
                    .as_deref()
                    .filter(|_| claude_enabled);
                if let Some(remote_config) = ws_snapshot.remote_config.as_ref() {
                    // Remote tab: NEVER run the agent command locally (it would
                    // launch Claude on this machine). With a captured session id,
                    // relaunch Claude on the remote in its directory; otherwise
                    // just reconnect the remote shell as before.
                    match claude_id {
                        Some(id) => {
                            tracing::info!(
                                panel_id = %panel_snapshot.id,
                                "Restoring remote Claude session over ssh"
                            );
                            Some(build_remote_claude_resume(
                                remote_config,
                                panel_snapshot.directory.as_deref(),
                                id,
                            ))
                        }
                        None => panel_snapshot.command.clone(),
                    }
                } else {
                    // Local tab: prefer the agent resume command detected at save
                    // time (already `claude --resume <id>` when we captured one)
                    // if its per-agent toggle is enabled.
                    let agent_cmd = panel_snapshot
                        .agent_resume_command
                        .as_deref()
                        .filter(|cmd| agent_restore_settings.is_enabled_for(cmd));
                    if let Some(cmd) = agent_cmd {
                        tracing::info!(
                            panel_id = %panel_snapshot.id,
                            cmd,
                            "Restoring agent session with resume command"
                        );
                        Some(cmd.to_string())
                    } else {
                        panel_snapshot.command.clone()
                    }
                }
            },
            pending_scrollback: panel_snapshot
                .terminal
                .as_ref()
                .and_then(|t| t.scrollback.clone()),
            pending_zoom: panel_snapshot
                .browser
                .as_ref()
                .map(|b| b.page_zoom)
                .filter(|&z| z != 1.0),
            parent_panel_id: None,
            agent_session_id: panel_snapshot.agent_session_id.clone(),
        };
        panels.insert(panel.id, panel);
    }

    workspace.layout = layout;
    workspace.panels = panels;
    workspace.focused_panel_id = ws_snapshot.focused_panel_id;
    workspace
}

fn restore_session(state: &Rc<AppState>) -> Vec<Uuid> {
    // Clean up temp scrollback files from the previous session before creating new ones.
    cleanup_scrollback_temp_files();

    if std::env::var("JMUX_DISABLE_SESSION_RESTORE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        tracing::info!("Session restore disabled via JMUX_DISABLE_SESSION_RESTORE");
        return vec![];
    }

    let snapshot = match session::store::load_session() {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return vec![],
        Err(e) => {
            tracing::warn!("Failed to load session: {}", e);
            return vec![];
        }
    };

    if snapshot.windows.is_empty() {
        return vec![];
    }

    // Filter out empty windows (stale sessions from crashes can leave windows with 0
    // workspaces). Consolidate all workspaces into a single window if every window
    // is empty — this prevents launching invisible windows with no content.
    let total_workspaces: usize = snapshot
        .windows
        .iter()
        .map(|w| w.tab_manager.workspaces.len())
        .sum();
    if total_workspaces == 0 {
        tracing::warn!(
            "Session has {} windows but 0 workspaces — discarding stale session",
            snapshot.windows.len()
        );
        return vec![];
    }

    // The quick terminal is recreated on demand, never restored — older session
    // files may still contain its workspace, so filter it out everywhere.
    let is_quick = |ws: &crate::session::snapshot::SessionWorkspaceSnapshot| {
        ws.custom_title.as_deref() == Some("Quick Terminal")
    };

    // Drop windows that have no workspaces or only empty/quick-terminal
    // workspaces (stale sessions can leave windows with 0 real panels).
    let live_windows: Vec<_> = snapshot
        .windows
        .iter()
        .filter(|w| {
            w.tab_manager
                .workspaces
                .iter()
                .any(|ws| !ws.panels.is_empty() && !is_quick(ws))
        })
        .collect();
    if live_windows.len() < snapshot.windows.len() {
        tracing::info!(
            "Dropped {} empty windows from session restore",
            snapshot.windows.len() - live_windows.len()
        );
    }

    // Load per-agent restore toggle settings once before iterating panels.
    let agent_restore_settings = crate::settings::load().agent_restore;

    // Build restored TabManager outside the lock to avoid blocking socket
    // handlers that need to read workspace state during startup.
    let mut restored_tm = TabManager::empty();
    let mut window_ids: Vec<Uuid> = Vec::new();
    let mut restored_groups: Vec<crate::model::WorkspaceGroup> = Vec::new();

    for window_snapshot in &live_windows {
        let window_id = Uuid::new_v4();

        // Restore window geometry (separate lock, brief)
        if let Some(frame) = &window_snapshot.frame {
            let w = frame.width as i32;
            let h = frame.height as i32;
            if w > 0 && h > 0 {
                lock_or_recover(&state.shared.window_sizes).insert(window_id, (w, h));
            }
        }

        window_ids.push(window_id);

        let tm_snapshot = &window_snapshot.tab_manager;

        // Restore this window's workspace groups, preserving their IDs so
        // workspace.group_id references stay valid.
        for group_snapshot in &tm_snapshot.groups {
            restored_groups.push(crate::model::WorkspaceGroup {
                id: group_snapshot.id,
                name: group_snapshot.name.clone(),
                color: group_snapshot.color.clone(),
                collapsed: group_snapshot.collapsed,
                window_id: Some(window_id),
            });
        }

        for ws_snapshot in &tm_snapshot.workspaces {
            if ws_snapshot.panels.is_empty() || is_quick(ws_snapshot) {
                continue; // Skip empty + quick-terminal workspaces
            }
            let workspace = build_workspace_from_snapshot(
                ws_snapshot,
                Some(window_id),
                &agent_restore_settings,
            );
            restored_tm.add_workspace(workspace);
        }
    }

    // Restore the recently-closed history (for the History pane + reopen).
    let closed_entries: Vec<crate::model::tab_manager::ClosedEntry> = snapshot
        .closed_workspaces
        .iter()
        .filter(|e| !e.workspace.panels.is_empty())
        .map(|e| crate::model::tab_manager::ClosedEntry {
            workspace: build_workspace_from_snapshot(&e.workspace, None, &agent_restore_settings),
            closed_at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(e.closed_at_unix),
            title: e.title.clone(),
        })
        .collect();
    if !closed_entries.is_empty() {
        restored_tm.set_closed_entries(closed_entries);
    }

    restored_tm.set_groups(restored_groups);

    tracing::info!(
        "Restored {} workspaces across {} windows from session",
        restored_tm.len(),
        window_ids.len(),
    );

    // Swap into shared state — lock held only for the assignment
    let mut tab_manager = lock_or_recover(&state.shared.tab_manager);
    *tab_manager = restored_tm;
    drop(tab_manager);

    window_ids
}

/// Locate the shell-integration directory bundled with the jmux binary.
///
/// Search order:
///   1. Sibling of the executable: `<exe_dir>/../shell-integration/`
///   2. Cargo workspace source tree (development): `<exe_dir>/../../jmux/shell-integration/`
///   3. XDG data: `~/.local/share/jmux/shell-integration/`
fn shell_integration_dir() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;

    // 1. Installed layout: <prefix>/bin/jmux-app → <prefix>/share/jmux/shell-integration/
    let installed = exe_dir
        .parent()
        .map(|prefix| prefix.join("share/jmux/shell-integration"));
    if let Some(ref p) = installed {
        if p.join(".zshenv").exists() {
            return p.to_str().map(|s| s.to_string());
        }
    }

    // 2. Development layout: target/debug/jmux-app → jmux/shell-integration/
    // Walk up from exe_dir looking for the jmux/shell-integration directory
    let mut ancestor = exe_dir.to_path_buf();
    for _ in 0..5 {
        let candidate = ancestor.join("jmux/shell-integration");
        if candidate.join(".zshenv").exists() {
            return candidate.to_str().map(|s| s.to_string());
        }
        if !ancestor.pop() {
            break;
        }
    }

    // 3. XDG data directory
    let xdg = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"))
        .join("jmux/shell-integration");
    if xdg.join(".zshenv").exists() {
        return xdg.to_str().map(|s| s.to_string());
    }

    None
}

/// Compute the environment variables needed for shell integration auto-injection.
///
/// Returns (zdotdir, bash_env, original_zdotdir) where:
///   - `zdotdir`: path to set as ZDOTDIR for zsh (our shell-integration dir)
///   - `bash_env`: path to set as BASH_ENV for bash
///   - `original_zdotdir`: value for JMUX_ZSH_ORIGINAL_ZDOTDIR (preserves user's ZDOTDIR)
fn shell_injection_env_vars(
    integration_dir: &Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(dir) = integration_dir else {
        return (None, None, None);
    };

    let zdotdir = Some(dir.clone());
    let bash_env = {
        let path = std::path::Path::new(dir).join("jmux-bash-integration.bash");
        if path.exists() {
            path.to_str().map(|s| s.to_string())
        } else {
            None
        }
    };

    // Preserve the user's original ZDOTDIR so our .zshenv can restore it
    let original_zdotdir = match std::env::var("ZDOTDIR") {
        Ok(val) => Some(val),
        Err(_) => Some("__jmux_unset__".to_string()),
    };

    (zdotdir, bash_env, original_zdotdir)
}

/// Compute the `XDG_DATA_DIRS` value for fish shell integration.
///
/// Fish auto-sources `fish/vendor_conf.d/*.fish` from every entry in
/// `XDG_DATA_DIRS`. We prepend the shell-integration directory (which contains
/// `fish/vendor_conf.d/jmux.fish`), preserving any existing value so the user's
/// other vendored data is untouched. Returns `None` when the fish script is
/// absent so we never hijack XDG_DATA_DIRS without a real integration file.
fn fish_xdg_data_dirs(integration_dir: &Option<String>) -> Option<String> {
    let dir = integration_dir.as_ref()?;
    let fish_script = std::path::Path::new(dir).join("fish/vendor_conf.d/jmux.fish");
    if !fish_script.exists() {
        return None;
    }
    let existing = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    Some(format!("{dir}:{existing}"))
}

/// Write scrollback text to a temp file for session restore.
/// Returns the file path on success, or `None` if writing fails.
///
/// Uses `~/.cache/jmux/scrollback/` (user-private) instead of `/tmp/` to
/// avoid symlink attacks and information disclosure.  Files are created with
/// `O_CREAT|O_EXCL` (mode 0600) so a pre-existing symlink causes a clean
/// failure rather than writing through it.
fn write_scrollback_temp_file(panel_id: Uuid, text: &str) -> Option<String> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let dir = dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("jmux/scrollback");

    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    // Ensure the directory is user-private (0700).
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));

    let path = dir.join(format!("{panel_id}.txt"));

    // O_CREAT | O_EXCL: fail if path already exists (prevents symlink attacks).
    // If file exists from a previous session, remove it first.
    let _ = std::fs::remove_file(&path);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .ok()?;

    use std::io::Write;
    file.write_all(text.as_bytes()).ok()?;
    path.to_str().map(|s| s.to_string())
}

/// Atomic flag set by the SIGUSR2 signal handler.
static SIGUSR2_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Install a SIGUSR2 signal handler that triggers Omarchy theme reload.
fn install_sigusr2_theme_reload() {
    // SAFETY: libc::signal is always safe to call. The handler function only
    // writes to an AtomicBool, which is async-signal-safe.
    unsafe {
        libc::signal(
            libc::SIGUSR2,
            sigusr2_handler as *const () as libc::sighandler_t,
        );
    }

    // Poll the flag from the GTK main loop
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        if SIGUSR2_RECEIVED.swap(false, Ordering::Relaxed) {
            let settings = crate::settings::load();
            if settings.theme == crate::settings::ThemeMode::Omarchy {
                tracing::info!("SIGUSR2 received — reloading Omarchy theme");
                apply_theme_from_settings();
            }
        }
        glib::ControlFlow::Continue
    });
}

extern "C" fn sigusr2_handler(_sig: libc::c_int) {
    SIGUSR2_RECEIVED.store(true, Ordering::Relaxed);
}

/// Apply the current theme from settings. Handles System/Light/Dark/Omarchy modes.
/// Also notifies ghostty of the resolved dark/light state so that conditional
/// theme values like `dark:Dracula|light:GitHub` in `~/.config/ghostty/config`
/// are resolved correctly.
pub fn apply_theme_from_settings() {
    let settings = crate::settings::load();
    let Some(display) = gdk4::Display::default() else {
        return;
    };
    let style_manager = adw::StyleManager::for_display(&display);

    match settings.theme {
        crate::settings::ThemeMode::System => {
            style_manager.set_color_scheme(adw::ColorScheme::Default);
        }
        crate::settings::ThemeMode::Light => {
            style_manager.set_color_scheme(adw::ColorScheme::ForceLight);
        }
        crate::settings::ThemeMode::Dark => {
            style_manager.set_color_scheme(adw::ColorScheme::ForceDark);
        }
        crate::settings::ThemeMode::Omarchy => {
            let is_light = crate::settings::omarchy_is_light();
            style_manager.set_color_scheme(if is_light {
                adw::ColorScheme::ForceLight
            } else {
                adw::ColorScheme::ForceDark
            });

            // Apply full Omarchy color palette via CSS overrides
            let colors = crate::settings::omarchy_colors();
            let mut css = String::new();
            if let Some(ref bg) = colors.background {
                css += &format!(
                    "@define-color window_bg_color {bg};\n\
                     @define-color view_bg_color {bg};\n\
                     @define-color headerbar_bg_color {bg};\n\
                     @define-color headerbar_backdrop_color {bg};\n\
                     @define-color sidebar_bg_color {bg};\n\
                     @define-color sidebar_backdrop_color {bg};\n\
                     @define-color card_bg_color {bg};\n\
                     @define-color dialog_bg_color {bg};\n\
                     @define-color popover_bg_color {bg};\n"
                );
            }
            if let Some(ref fg) = colors.foreground {
                css += &format!(
                    "@define-color window_fg_color {fg};\n\
                     @define-color view_fg_color {fg};\n\
                     @define-color headerbar_fg_color {fg};\n\
                     @define-color sidebar_fg_color {fg};\n\
                     @define-color card_fg_color {fg};\n\
                     @define-color dialog_fg_color {fg};\n\
                     @define-color popover_fg_color {fg};\n"
                );
            }
            if let Some(ref accent) = colors.accent {
                css += &format!(
                    "@define-color accent_color {accent};\n\
                     @define-color accent_bg_color {accent};\n\
                     .navigation-sidebar row:selected {{\n\
                         background-color: alpha({accent}, 0.25);\n\
                     }}\n\
                     .pane-tab-selected {{\n\
                         background-color: alpha({accent}, 0.15);\n\
                         border-color: alpha({accent}, 0.25);\n\
                     }}\n\
                     .pane-tab-attention {{\n\
                         background-color: alpha({accent}, 0.18);\n\
                         color: {accent};\n\
                         border-color: alpha({accent}, 0.35);\n\
                     }}\n\
                     .attention-panel {{\n\
                         border-color: {accent};\n\
                         background-color: alpha({accent}, 0.08);\n\
                     }}\n\
                     .sidebar-progress progress {{\n\
                         background-color: {accent};\n\
                     }}\n"
                );
            }
            if !css.is_empty() {
                let provider = gtk4::CssProvider::new();
                provider.load_from_data(&css);
                gtk4::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
        }
    }

    // Notify ghostty of the resolved dark/light state so that conditional
    // theme values like `dark:Dracula|light:GitHub` in the ghostty config are
    // picked up on the next config reload.
    #[cfg(feature = "link-ghostty")]
    {
        use ghostty_sys::*;
        let is_dark = style_manager.is_dark();
        let scheme = if is_dark {
            ghostty_color_scheme_e::GHOSTTY_COLOR_SCHEME_DARK
        } else {
            ghostty_color_scheme_e::GHOSTTY_COLOR_SCHEME_LIGHT
        };
        if let Ok(app_ptr) = GHOSTTY_APP_PTR.lock() {
            if !app_ptr.is_null() {
                // SAFETY: app_ptr is non-null (checked above) and this is called on the
                // GTK main thread. ghostty_app_set_color_scheme is safe to call here.
                unsafe {
                    ghostty_app_set_color_scheme(app_ptr.0, scheme);
                }
            }
        }
    }

    // Apply tab bar font size override if non-zero
    if settings.tab_bar_font_size > 0.0 {
        let size = settings.tab_bar_font_size;
        let css = format!(".pane-tab {{ font-size: {size}px; }}\n");
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&css);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }

    // Apply sidebar font size override if non-zero
    if settings.sidebar_font_size > 0.0 {
        let size = settings.sidebar_font_size;
        let css = format!(".navigation-sidebar {{ font-size: {size}px; }}\n");
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&css);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }

    // Inject chat-bubble CSS for iMessage mode (always present; only visible when
    // the workspace has imessage_mode enabled and the labels have these classes).
    {
        let css = "\
.chat-bubble-assistant { \
    background: alpha(@accent_color, 0.15); \
    border-radius: 12px; \
    padding: 4px 10px; \
    margin: 1px 2px; \
}\n\
.chat-bubble-user { \
    background: alpha(@window_fg_color, 0.08); \
    border-radius: 12px; \
    padding: 4px 10px; \
    margin: 1px 2px; \
}\n";
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(css);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // Notes-panel scope colours — Global / Host / Folder tab groups. A small
    // coloured dot precedes each tab's filename so scopes read as groups.
    {
        let css = "\
.notes-dot { min-width: 8px; min-height: 8px; border-radius: 4px; margin-right: 4px; }\n\
.notes-dot-global { background-color: #3584e4; }\n\
.notes-dot-host { background-color: #c061cb; }\n\
.notes-dot-folder { background-color: #33d17a; }\n\
.notes-tab-global label { color: #3584e4; }\n\
.notes-tab-host label { color: #c061cb; }\n\
.notes-tab-folder label { color: #33d17a; }\n";
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(css);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // Apply sidebar tint color (with opacity) when set.
    // Prefer the light/dark variant over the fallback tint_color.
    {
        let style_manager = adw::StyleManager::for_display(&display);
        let effective_tint = if style_manager.is_dark() {
            if !settings.sidebar.tint_color_dark.is_empty() {
                Some(settings.sidebar.tint_color_dark.clone())
            } else if !settings.sidebar.tint_color.is_empty() {
                Some(settings.sidebar.tint_color.clone())
            } else {
                None
            }
        } else if !settings.sidebar.tint_color_light.is_empty() {
            Some(settings.sidebar.tint_color_light.clone())
        } else if !settings.sidebar.tint_color.is_empty() {
            Some(settings.sidebar.tint_color.clone())
        } else {
            None
        };
        if let Some(ref color) = effective_tint {
            let opacity = settings.sidebar.tint_opacity;
            let css = format!(
                ".navigation-sidebar {{ background-color: alpha({color}, {opacity:.2}); }}\n\
                 .navigation-sidebar.backdrop {{ background-color: alpha({color}, {opacity:.2}); }}\n"
            );
            let provider = gtk4::CssProvider::new();
            provider.load_from_data(&css);
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
    }

    // Apply custom sidebar selection color if set (overrides theme default)
    let sel = &settings.sidebar.selection_color;
    if !sel.is_empty() {
        let css = format!(
            ".navigation-sidebar row:selected {{ background-color: {sel}; }}\n\
             .navigation-sidebar row:selected .workspace-title {{ color: inherit; }}\n\
             .navigation-sidebar row:selected .dim-label,\n\
             .navigation-sidebar row:selected .caption {{ color: alpha(currentColor, 0.8); }}\n"
        );
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&css);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
    }
}

/// Initialize the ghostty embedded runtime and store it in AppState.
fn init_ghostty(state: &Rc<AppState>) {
    if state.ghostty_app.borrow().is_some() {
        return;
    }

    if let Err(e) = ghostty_gtk::app::GhosttyApp::init() {
        tracing::error!("Failed to init ghostty: {}", e);
        return;
    }

    let handler = JmuxCallbackHandler {
        shared: state.shared.clone(),
    };

    let callbacks = ghostty_gtk::callbacks::RuntimeCallbacks::new(Box::new(handler));

    match ghostty_gtk::app::GhosttyApp::new(&callbacks) {
        Ok(ghostty_app) => {
            tracing::info!("Ghostty app initialized successfully");
            if let Ok(mut ptr) = GHOSTTY_APP_PTR.lock() {
                *ptr = SendAppPtr(ghostty_app.raw());
            }

            // Cache ghostty config values for jmux UI decisions
            let ui_config = crate::ghostty_config::GhosttyUiConfig::from_app(&ghostty_app);
            tracing::info!(?ui_config, "Loaded ghostty UI config");
            apply_ghostty_css(&ui_config);
            *state.ghostty_ui_config.borrow_mut() = ui_config;

            *state.ghostty_app.borrow_mut() = Some(ghostty_app);
            *state._callbacks.borrow_mut() = Some(callbacks);
        }
        Err(e) => {
            tracing::error!("Failed to create GhosttyApp: {}", e);
        }
    }
}

/// Apply CSS overrides derived from ghostty config (split divider color, etc.).
pub fn apply_ghostty_css(config: &crate::ghostty_config::GhosttyUiConfig) {
    let Some(display) = gdk4::Display::default() else {
        return;
    };

    let mut css = String::new();

    // Match sidebar background to terminal background color when enabled
    let settings = crate::settings::load();
    if settings.sidebar.match_terminal_background {
        if let Some(ref hex) = config.background_hex() {
            css += &format!(
                "@define-color sidebar_bg_color {hex};\n\
                 @define-color sidebar_backdrop_color {hex};\n"
            );
        }
    }

    if let Some((r, g, b)) = config.split_divider_color {
        let hex = format!(
            "#{:02x}{:02x}{:02x}",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
        );
        css += &format!("paned > separator {{ background-color: {hex}; }}\n");
    }

    if !css.is_empty() {
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&css);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Callback handler that bridges ghostty events to the GTK main loop.
struct JmuxCallbackHandler {
    shared: Arc<SharedState>,
}

impl ghostty_gtk::callbacks::GhosttyCallbackHandler for JmuxCallbackHandler {
    fn on_wakeup(&self) {
        let is_null = GHOSTTY_APP_PTR.lock().ok().is_none_or(|p| p.is_null());
        if is_null {
            return;
        }

        if GHOSTTY_TICK_PENDING.swap(true, Ordering::AcqRel) {
            return;
        }

        glib::MainContext::default().invoke_with_priority(glib::Priority::DEFAULT, move || {
            GHOSTTY_TICK_PENDING.store(false, Ordering::Release);
            let app_ptr = match GHOSTTY_APP_PTR.lock() {
                Ok(p) => *p,
                Err(_) => return,
            };
            if app_ptr.is_null() {
                return;
            }

            #[cfg(feature = "link-ghostty")]
            // SAFETY: app_ptr is initialized once at startup and lives for the
            // process lifetime. ghostty_app_tick is called on the main thread.
            unsafe {
                ghostty_app_tick(app_ptr.get());
            }
            #[cfg(not(feature = "link-ghostty"))]
            let _ = ();
        });
    }

    fn on_action(&self, target: ghostty_target_s, action: ghostty_action_s) -> bool {
        match action.tag {
            ghostty_action_tag_e::GHOSTTY_ACTION_RENDER => {
                // The target surface wants a re-render.
                if target.tag == ghostty_target_tag_e::GHOSTTY_TARGET_SURFACE {
                    // SAFETY: tag == GHOSTTY_TARGET_SURFACE guarantees the union
                    // contains a valid surface pointer (ghostty FFI contract).
                    let surface_ptr = unsafe { target.target.surface };
                    if !surface_ptr.is_null() {
                        #[cfg(feature = "link-ghostty")]
                        unsafe {
                            let userdata = ghostty_surface_userdata(surface_ptr);
                            let _ = ghostty_gtk::callbacks::queue_render_from_userdata(userdata);
                        }
                    }
                }
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_SET_TITLE => {
                if target.tag == ghostty_target_tag_e::GHOSTTY_TARGET_SURFACE {
                    // SAFETY: tag == GHOSTTY_TARGET_SURFACE guarantees the union
                    // contains a valid surface pointer (ghostty FFI contract).
                    let surface_ptr = unsafe { target.target.surface };
                    if !surface_ptr.is_null() {
                        // SAFETY: action tag is SET_TITLE so the union contains
                        // set_title. The title pointer is null-checked before
                        // CStr::from_ptr. Ghostty guarantees NUL-terminated strings.
                        let title = unsafe {
                            let cstr = action.action.set_title.title;
                            if cstr.is_null() {
                                None
                            } else {
                                std::ffi::CStr::from_ptr(cstr)
                                    .to_str()
                                    .ok()
                                    .map(String::from)
                            }
                        };
                        if let Some(title) = title {
                            self.shared.send_ui_event(UiEvent::SetTitle {
                                surface: SendSurfacePtr(surface_ptr),
                                title,
                            });
                        }
                    }
                }
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_PWD => {
                if target.tag == ghostty_target_tag_e::GHOSTTY_TARGET_SURFACE {
                    // SAFETY: tag == GHOSTTY_TARGET_SURFACE guarantees the union
                    // contains a valid surface pointer (ghostty FFI contract).
                    let surface_ptr = unsafe { target.target.surface };
                    if !surface_ptr.is_null() {
                        // SAFETY: action tag is PWD so the union contains pwd.
                        // Pointer is null-checked. Ghostty guarantees NUL-terminated strings.
                        let pwd = unsafe {
                            let cstr = action.action.pwd.pwd;
                            if cstr.is_null() {
                                None
                            } else {
                                std::ffi::CStr::from_ptr(cstr)
                                    .to_str()
                                    .ok()
                                    .map(String::from)
                            }
                        };
                        if let Some(pwd) = pwd {
                            self.shared.send_ui_event(UiEvent::SetPwd {
                                surface: SendSurfacePtr(surface_ptr),
                                directory: pwd,
                            });
                        }
                    }
                }
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_START_SEARCH => {
                self.shared.send_ui_event(UiEvent::StartSearch);
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_END_SEARCH => {
                self.shared.send_ui_event(UiEvent::EndSearch);
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_SEARCH_TOTAL => {
                self.shared.send_ui_event(UiEvent::SearchTotal);
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_SEARCH_SELECTED => {
                self.shared.send_ui_event(UiEvent::SearchSelected);
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_RING_BELL => {
                // Play the system bell sound via GDK (must be on GTK main thread)
                glib::idle_add_local_once(|| {
                    if let Some(display) = gdk4::Display::default() {
                        display.beep();
                    }
                });
                true
            }
            // Ghostty notifies the host when the scrollbar state changes (e.g.
            // on alt-screen enter/exit, or when the scrollback buffer grows).
            // jmux does not render its own scrollbar — ghostty draws entirely
            // within the GLArea — so we acknowledge the action and do nothing.
            ghostty_action_tag_e::GHOSTTY_ACTION_SCROLLBAR => true,
            // Ghostty's default `ctrl+shift+t` keybind fires this action (and
            // consumes the key, so our GTK window handler never sees it). Honor
            // it by opening a new terminal tab in the focused pane of the active
            // workspace — mirroring `file.open`'s tab placement.
            ghostty_action_tag_e::GHOSTTY_ACTION_NEW_TAB => {
                {
                    let mut tm = lock_or_recover(&self.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        let mut panel = crate::model::panel::Panel::new_terminal();
                        // Inherit the focused terminal's working directory.
                        panel.directory = new_tab_directory(ws);
                        let new_id = panel.id;
                        ws.panels.insert(new_id, panel);
                        let target = ws
                            .focused_panel_id
                            .or_else(|| ws.layout.all_panel_ids().into_iter().next());
                        if let Some(target) = target {
                            ws.layout.add_panel_to_pane(target, new_id);
                        }
                        ws.previous_focused_panel_id = ws.focused_panel_id;
                        ws.focused_panel_id = Some(new_id);
                    }
                }
                self.shared.notify_ui_refresh();
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_OPEN_URL => {
                // SAFETY: action tag is OPEN_URL so the union contains open_url.
                // Pointer and length are null/zero-checked. from_raw_parts requires
                // the pointer to be valid for `len` bytes — guaranteed by ghostty.
                let url = unsafe {
                    let open_url = &action.action.open_url;
                    if open_url.url.is_null() || open_url.len == 0 {
                        None
                    } else {
                        let slice =
                            std::slice::from_raw_parts(open_url.url as *const u8, open_url.len);
                        std::str::from_utf8(slice).ok().map(String::from)
                    }
                };
                if let Some(url) = url {
                    self.shared.send_ui_event(UiEvent::OpenUrlInBrowser { url });
                }
                true
            }
            // Parity batch 25 — Kitty keyboard mode reset at prompt boundaries.
            //
            // When OSC 133 D (command finished) fires, any Kitty keyboard protocol
            // mode that a TUI app left active after abnormal exit should be cleared.
            // ghostty does not yet expose a dedicated `ghostty_surface_reset_keyboard_mode`
            // FFI call, and there is no `reset_terminal` binding action available.
            //
            // TODO: ghostty_surface_reset_keyboard_mode — no FFI yet.
            //       Once ghostty exposes such a function (or a binding action like
            //       "reset_keyboard_mode"), call it here on the target surface so
            //       that stale Kitty protocol modes are cleared at every prompt.
            ghostty_action_tag_e::GHOSTTY_ACTION_COMMAND_FINISHED => {
                // Acknowledge the action. Actual keyboard-mode reset is deferred
                // until the upstream ghostty FFI exposes the necessary call.
                tracing::trace!("COMMAND_FINISHED — Kitty keyboard mode reset pending FFI");
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_DESKTOP_NOTIFICATION => {
                // SAFETY: action tag is DESKTOP_NOTIFICATION so the union contains
                // desktop_notification. Pointers are null-checked before CStr::from_ptr.
                let (title, body) = unsafe {
                    let notif = &action.action.desktop_notification;
                    let title = if notif.title.is_null() {
                        String::new()
                    } else {
                        std::ffi::CStr::from_ptr(notif.title)
                            .to_string_lossy()
                            .to_string()
                    };
                    let body = if notif.body.is_null() {
                        String::new()
                    } else {
                        std::ffi::CStr::from_ptr(notif.body)
                            .to_string_lossy()
                            .to_string()
                    };
                    (title, body)
                };

                if target.tag == ghostty_target_tag_e::GHOSTTY_TARGET_SURFACE {
                    // SAFETY: tag == GHOSTTY_TARGET_SURFACE guarantees the union
                    // contains a valid surface pointer (ghostty FFI contract).
                    let surface_ptr = unsafe { target.target.surface };
                    if !surface_ptr.is_null() {
                        self.shared.send_ui_event(UiEvent::DesktopNotification {
                            surface: SendSurfacePtr(surface_ptr),
                            title,
                            body,
                        });
                    }
                }
                true
            }
            // copy_on_select: automatically copy terminal selection to the clipboard.
            //
            // Ghostty does not yet expose a selection-changed action or a
            // `ghostty_surface_get_selection_text` FFI function that fires when the
            // user finishes a mouse-drag selection. When such a signal is available,
            // the handler should:
            //
            //   1. Check `crate::settings::load().copy_on_select`.
            //   2. Call `ghostty_surface_get_selection_text(surface_ptr)` to read the text.
            //   3. Set the clipboard: `gdk4::Display::default().unwrap().clipboard().set_text(&text)`.
            //
            // TODO: wire when ghostty exposes a selection-changed FFI (e.g.
            //       GHOSTTY_ACTION_SELECTION_CHANGED or ghostty_surface_get_selection_text).
            _ => {
                tracing::trace!("Unhandled ghostty action: {:?}", action.tag as u32);
                false
            }
        }
    }
}

#[derive(Clone, Copy)]
struct SendAppPtr(ghostty_app_t);

// SAFETY: SendAppPtr wraps a ghostty_app_t passed via channel to the main thread.
unsafe impl Send for SendAppPtr {}

impl SendAppPtr {
    #[cfg(feature = "link-ghostty")]
    fn get(self) -> ghostty_app_t {
        self.0
    }

    fn is_null(self) -> bool {
        self.0.is_null()
    }
}

static GHOSTTY_APP_PTR: Mutex<SendAppPtr> = Mutex::new(SendAppPtr(std::ptr::null_mut()));
static GHOSTTY_TICK_PENDING: AtomicBool = AtomicBool::new(false);

/// Parse a `jmux://` deep link URI and return the corresponding `UiEvent`.
///
/// Supported schemes:
/// - `jmux://ssh/<user>@<host>[:<port>][/<path>]`
///   The host portion (`<user>@<host>`) maps directly to the SSH `destination`
///   parameter.  An optional port in the authority overrides the default SSH port.
///
/// Returns `None` for unrecognised or malformed URIs.
fn parse_deep_link(uri: &str) -> Option<UiEvent> {
    // Only handle our custom scheme.
    let rest = uri.strip_prefix("jmux://")?;

    // Extract the first path component as the sub-command.
    // e.g. "ssh/user@host:22/path" → command="ssh", tail="user@host:22/path"
    let (command, tail) = rest.split_once('/').unwrap_or((rest, ""));

    match command {
        "ssh" => {
            // Strip any trailing path component — we only need the authority.
            let authority = tail.split('/').next().unwrap_or(tail);
            if authority.is_empty() {
                return None;
            }

            // Parse optional port from "user@host:port".
            let (destination, port) = if let Some(at_pos) = authority.rfind('@') {
                let host_part = &authority[at_pos + 1..];
                if let Some(colon_pos) = host_part.rfind(':') {
                    let port_str = &host_part[colon_pos + 1..];
                    let host = &host_part[..colon_pos];
                    let user = &authority[..at_pos];
                    if let Ok(p) = port_str.parse::<u16>() {
                        (format!("{}@{}", user, host), Some(p))
                    } else {
                        (authority.to_string(), None)
                    }
                } else {
                    (authority.to_string(), None)
                }
            } else {
                // No "@" — bare host, possibly with port.
                if let Some(colon_pos) = authority.rfind(':') {
                    let port_str = &authority[colon_pos + 1..];
                    let host = &authority[..colon_pos];
                    if let Ok(p) = port_str.parse::<u16>() {
                        (host.to_string(), Some(p))
                    } else {
                        (authority.to_string(), None)
                    }
                } else {
                    (authority.to_string(), None)
                }
            };

            if destination.is_empty() {
                return None;
            }

            Some(UiEvent::OpenSshDeepLink { destination, port })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_claude_resume_command_shape() {
        use crate::remote::session::RemoteConfig;
        let rc = RemoteConfig {
            destination: "user@host".to_string(),
            port: Some(2222),
            identity: Some("/home/u/.ssh/id".to_string()),
            ssh_options: Vec::new(),
            agent_forward: true,
            remote_daemon_path: None,
        };
        let cmd = build_remote_claude_resume(&rc, Some("/srv/app"), "abc-123");
        // ssh gets a tty, forwards the agent, uses the port + identity, and runs
        // Claude resuming the exact session in the remote directory.
        assert!(cmd.starts_with("ssh -t -A -p 2222 -i "), "got: {cmd}");
        assert!(cmd.contains("user@host"), "got: {cmd}");
        assert!(cmd.contains("cd "), "got: {cmd}");
        assert!(cmd.contains("claude --resume abc-123"), "got: {cmd}");

        // No directory -> resume without a cd.
        let cmd2 = build_remote_claude_resume(&rc, None, "id2");
        assert!(cmd2.contains("exec claude --resume id2"), "got: {cmd2}");
        assert!(!cmd2.contains("cd "), "got: {cmd2}");
    }

    #[test]
    fn close_panel_closes_empty_workspace() {
        let shared = Arc::new(SharedState::new());
        let state = AppState::new(shared.clone());
        let panel_id = shared
            .tab_manager
            .lock()
            .unwrap()
            .selected()
            .and_then(|workspace| workspace.focused_panel_id)
            .expect("workspace should have a focused panel");

        assert!(state.close_panel(panel_id, false));
        // Closing the last tab closes the workspace too.
        let tm = shared.tab_manager.lock().unwrap();
        assert_eq!(tm.len(), 0);
    }

    #[test]
    fn close_panel_returns_false_for_unknown_panel() {
        let state = AppState::new(Arc::new(SharedState::new()));
        assert!(!state.close_panel(Uuid::new_v4(), true));
    }
}
