//! Application entry point — creates the AdwApplication and main window.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ghostty_sys::*;
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
    /// Stored to keep the callbacks alive for the lifetime of the app.
    _callbacks: RefCell<Option<ghostty_gtk::callbacks::RuntimeCallbacks>>,
}

impl AppState {
    pub fn new(shared: Arc<SharedState>) -> Self {
        Self {
            shared,
            ghostty_app: RefCell::new(None),
            terminal_cache: RefCell::new(HashMap::new()),
            _callbacks: RefCell::new(None),
        }
    }

    pub fn terminal_surface_for(
        &self,
        panel_id: Uuid,
        working_directory: Option<&str>,
    ) -> ghostty_gtk::surface::GhosttyGlSurface {
        if let Some(surface) = self.terminal_cache.borrow().get(&panel_id) {
            return surface.clone();
        }

        let gl_surface = ghostty_gtk::surface::GhosttyGlSurface::new();
        gl_surface.set_hexpand(true);
        gl_surface.set_vexpand(true);

        // Check if this panel has pending scrollback to restore
        let scrollback_file = {
            let mut tm = lock_or_recover(&self.shared.tab_manager);
            tm.find_workspace_with_panel_mut(panel_id)
                .and_then(|ws| ws.panels.get_mut(&panel_id))
                .and_then(|panel| panel.pending_scrollback.take())
                .and_then(|text| write_scrollback_temp_file(panel_id, &text))
        };

        if let Some(app) = self.ghostty_app.borrow().as_ref() {
            if let Some(ref path) = scrollback_file {
                gl_surface.initialize_with_env(
                    app.raw(),
                    working_directory,
                    None,
                    &[("CMUX_RESTORE_SCROLLBACK_FILE", path)],
                );
            } else {
                gl_surface.initialize(app.raw(), working_directory, None);
            }
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
            let working_directory = {
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
                panel.directory.clone()
            };
            self.terminal_surface_for(panel_id, working_directory.as_deref())
        };

        surface.send_text(text)
    }

    pub fn close_panel(&self, panel_id: Uuid, process_alive: bool) -> bool {
        {
            let mut tab_manager = lock_or_recover(&self.shared.tab_manager);
            let Some(workspace) = tab_manager.find_workspace_with_panel_mut(panel_id) else {
                return false;
            };
            if !workspace.remove_panel(panel_id) {
                return false;
            }
            // Keep workspace alive even when empty (parity with macOS behavior)
        }

        self.terminal_cache.borrow_mut().remove(&panel_id);
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
                .filter(|panel| panel.panel_type == crate::model::PanelType::Terminal)
                .map(|panel| panel.id)
                .collect()
        };

        self.terminal_cache
            .borrow_mut()
            .retain(|panel_id, _| live_panels.contains(panel_id));
    }
}

/// Messages from background tasks that require a UI refresh.
#[derive(Debug)]
pub enum UiEvent {
    Refresh,
    SendInput { panel_id: Uuid, text: String },
    SearchTotal { total: isize },
    SearchSelected { selected: isize },
    StartSearch,
    EndSearch,
    OpenSettings,
    TriggerFlash { panel_id: Uuid },
    SendKey {
        panel_id: Uuid,
        keyval: u32,
        keycode: u32,
        mods: u32,
    },
    ReadText {
        panel_id: Uuid,
        reply: tokio::sync::oneshot::Sender<Option<String>>,
    },
    RefreshSurface {
        panel_id: Uuid,
    },
    ClearHistory {
        panel_id: Uuid,
    },
    ToggleNotifications,
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
    OpenMarkdownFile,
    BrowserAction {
        panel_id: Uuid,
        action: crate::ui::browser_panel::BrowserActionKind,
    },
}

/// Wrapper to send a raw ghostty_surface_t across threads.
#[derive(Clone, Copy)]
pub struct SendSurfacePtr(pub ghostty_surface_t);
unsafe impl Send for SendSurfacePtr {}
unsafe impl Sync for SendSurfacePtr {}
impl std::fmt::Debug for SendSurfacePtr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SendSurfacePtr").field(&(self.0 as *const ())).finish()
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
    /// Last-known window dimensions (width, height) for session persistence.
    pub window_size: Mutex<(i32, i32)>,
    ui_event_tx: Mutex<Option<UnboundedSender<UiEvent>>>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            tab_manager: Mutex::new(TabManager::new()),
            notifications: Mutex::new(NotificationStore::new()),
            closed_browser_urls: Mutex::new(Vec::new()),
            window_size: Mutex::new((1280, 860)),
            ui_event_tx: Mutex::new(None),
        }
    }

    pub fn install_ui_event_sender(&self, sender: UnboundedSender<UiEvent>) {
        *lock_or_recover(&self.ui_event_tx) = Some(sender);
    }

    pub fn send_ui_event(&self, event: UiEvent) -> bool {
        lock_or_recover(&self.ui_event_tx)
            .as_ref()
            .is_some_and(|sender| sender.send(event).is_ok())
    }

    pub fn notify_ui_refresh(&self) {
        let _ = self.send_ui_event(UiEvent::Refresh);
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
}

/// Run the GTK application. Returns the exit code.
pub fn run() -> i32 {
    let app = adw::Application::builder()
        .application_id("ai.manaflow.cmux")
        .build();

    let shared = Arc::new(SharedState::new());
    let state = Rc::new(AppState::new(shared.clone()));

    {
        let shared_for_socket = shared.clone();
        let shared_for_ports = shared.clone();
        app.connect_startup(move |_app| {
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
        });
    }

    let state_clone = state.clone();
    app.connect_activate(move |app| {
        activate(app, &state_clone);
    });

    {
        let state = state.clone();
        app.connect_shutdown(move |_app| {
            // Save session before shutdown
            let snapshot = session::store::create_snapshot(&state);
            if let Err(e) = session::store::save_session(&snapshot) {
                tracing::error!("Failed to save session on shutdown: {}", e);
            }

            *GHOSTTY_APP_PTR.lock().unwrap() = SendAppPtr(std::ptr::null_mut());
            GHOSTTY_TICK_PENDING.store(false, Ordering::Release);
            socket::server::cleanup();
            tracing::info!("Application shutdown");
        });
    }

    app.run().into()
}

fn activate(app: &adw::Application, state: &Rc<AppState>) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }

    let (ui_event_tx, ui_event_rx) = tokio::sync::mpsc::unbounded_channel();
    state.shared.install_ui_event_sender(ui_event_tx);

    // Apply saved theme preference
    apply_theme_from_settings();

    // Register SIGUSR2 handler for Omarchy live theme switching.
    // Signal handler sets an AtomicBool; a glib timer polls it.
    install_sigusr2_theme_reload();

    init_ghostty(state);

    // Restore session after ghostty is initialized so terminals can be created
    restore_session(state);

    // Create the main window
    let window = ui::window::create_window(app, state, ui_event_rx);
    window.present();

    // Start periodic autosave (every 8 seconds, matching macOS cmux)
    {
        let state = state.clone();
        let window_weak = window.downgrade();
        glib::timeout_add_local(std::time::Duration::from_secs(8), move || {
            // Capture current window size before snapshot
            if let Some(win) = window_weak.upgrade() {
                let w = win.width();
                let h = win.height();
                if w > 0 && h > 0 {
                    *lock_or_recover(&state.shared.window_size) = (w, h);
                }
            }
            let snapshot = session::store::create_snapshot(&state);
            if let Err(e) = session::store::save_session(&snapshot) {
                tracing::warn!("Autosave failed: {}", e);
            }
            glib::ControlFlow::Continue
        });
    }
}

/// Restore workspaces from a saved session, if one exists.
fn restore_session(state: &Rc<AppState>) {
    let snapshot = match session::store::load_session() {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("Failed to load session: {}", e);
            return;
        }
    };

    // Take the first window's tab manager snapshot (Linux typically has one window)
    let Some(window_snapshot) = snapshot.windows.into_iter().next() else {
        return;
    };

    // Restore window geometry if saved
    if let Some(frame) = &window_snapshot.frame {
        let w = frame.width as i32;
        let h = frame.height as i32;
        if w > 0 && h > 0 {
            *lock_or_recover(&state.shared.window_size) = (w, h);
        }
    }

    let tm_snapshot = window_snapshot.tab_manager;
    if tm_snapshot.workspaces.is_empty() {
        return;
    }

    let mut tab_manager = lock_or_recover(&state.shared.tab_manager);

    // Replace the default workspace with restored ones
    *tab_manager = TabManager::empty();

    for ws_snapshot in &tm_snapshot.workspaces {
        let mut workspace = crate::model::Workspace::with_directory(&ws_snapshot.current_directory);
        workspace.custom_title = ws_snapshot.custom_title.clone();
        workspace.custom_color = ws_snapshot.custom_color.clone();
        workspace.is_pinned = ws_snapshot.is_pinned;
        workspace.process_title = ws_snapshot.process_title.clone();
        workspace.status_entries = ws_snapshot.status_entries.clone();
        workspace.log_entries = ws_snapshot.log_entries.clone();
        workspace.progress = ws_snapshot.progress.clone();
        workspace.git_branch = ws_snapshot.git_branch.clone();

        // Restore layout from snapshot
        let layout = ws_snapshot.layout.to_layout();

        // Rebuild panels map from snapshot panels
        let mut panels = std::collections::HashMap::new();
        for panel_snapshot in &ws_snapshot.panels {
            let panel_type = match panel_snapshot.panel_type.as_str() {
                "browser" => crate::model::PanelType::Browser,
                "markdown" => crate::model::PanelType::Markdown,
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
                markdown_file: panel_snapshot
                    .markdown
                    .as_ref()
                    .map(|m| m.file_path.clone()),
                pending_scrollback: panel_snapshot
                    .terminal
                    .as_ref()
                    .and_then(|t| t.scrollback.clone()),
                pending_zoom: panel_snapshot
                    .browser
                    .as_ref()
                    .map(|b| b.page_zoom)
                    .filter(|&z| z != 1.0),
            };
            panels.insert(panel.id, panel);
        }

        workspace.layout = layout;
        workspace.panels = panels;
        workspace.focused_panel_id = ws_snapshot.focused_panel_id;

        tab_manager.add_workspace(workspace);
    }

    // Restore selection
    if let Some(index) = tm_snapshot.selected_workspace_index {
        tab_manager.select(index);
    }

    tracing::info!(
        "Restored {} workspaces from session",
        tab_manager.len()
    );
}

/// Write scrollback text to a temp file for session restore.
/// Returns the file path on success, or `None` if writing fails.
fn write_scrollback_temp_file(panel_id: Uuid, text: &str) -> Option<String> {
    let dir = std::env::temp_dir().join("cmux-scrollback");
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = dir.join(format!("{panel_id}.txt"));
    if std::fs::write(&path, text).is_err() {
        return None;
    }
    path.to_str().map(|s| s.to_string())
}

/// Atomic flag set by the SIGUSR2 signal handler.
static SIGUSR2_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Install a SIGUSR2 signal handler that triggers Omarchy theme reload.
fn install_sigusr2_theme_reload() {
    // Register the signal handler (async-signal-safe: only sets an atomic)
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

    let handler = CmuxCallbackHandler {
        shared: state.shared.clone(),
    };

    let callbacks = ghostty_gtk::callbacks::RuntimeCallbacks::new(Box::new(handler));

    match ghostty_gtk::app::GhosttyApp::new(&callbacks) {
        Ok(ghostty_app) => {
            tracing::info!("Ghostty app initialized successfully");
            *GHOSTTY_APP_PTR.lock().unwrap() = SendAppPtr(ghostty_app.raw());
            *state.ghostty_app.borrow_mut() = Some(ghostty_app);
            *state._callbacks.borrow_mut() = Some(callbacks);
        }
        Err(e) => {
            tracing::error!("Failed to create GhosttyApp: {}", e);
        }
    }
}

/// Callback handler that bridges ghostty events to the GTK main loop.
struct CmuxCallbackHandler {
    shared: Arc<SharedState>,
}

impl ghostty_gtk::callbacks::GhosttyCallbackHandler for CmuxCallbackHandler {
    fn on_wakeup(&self) {
        if (*GHOSTTY_APP_PTR.lock().unwrap()).is_null() {
            return;
        }

        if GHOSTTY_TICK_PENDING.swap(true, Ordering::AcqRel) {
            return;
        }

        glib::MainContext::default().invoke_with_priority(glib::Priority::DEFAULT, move || {
            GHOSTTY_TICK_PENDING.store(false, Ordering::Release);
            let app_ptr = *GHOSTTY_APP_PTR.lock().unwrap();
            if app_ptr.is_null() {
                return;
            }

            #[cfg(feature = "link-ghostty")]
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
                    let surface_ptr = unsafe { target.target.surface };
                    if !surface_ptr.is_null() {
                        let title = unsafe {
                            let cstr = action.action.set_title.title;
                            if cstr.is_null() {
                                None
                            } else {
                                std::ffi::CStr::from_ptr(cstr).to_str().ok().map(String::from)
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
                    let surface_ptr = unsafe { target.target.surface };
                    if !surface_ptr.is_null() {
                        let pwd = unsafe {
                            let cstr = action.action.pwd.pwd;
                            if cstr.is_null() {
                                None
                            } else {
                                std::ffi::CStr::from_ptr(cstr).to_str().ok().map(String::from)
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
                let total = unsafe { action.action.search_total.total };
                self.shared.send_ui_event(UiEvent::SearchTotal { total });
                true
            }
            ghostty_action_tag_e::GHOSTTY_ACTION_SEARCH_SELECTED => {
                let selected = unsafe { action.action.search_selected.selected };
                self.shared
                    .send_ui_event(UiEvent::SearchSelected { selected });
                true
            }
            _ => {
                tracing::trace!("Unhandled ghostty action: {:?}", action.tag as u32);
                false
            }
        }
    }
}

#[derive(Clone, Copy)]
struct SendAppPtr(ghostty_app_t);

unsafe impl Send for SendAppPtr {}
unsafe impl Sync for SendAppPtr {}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_panel_keeps_workspace_alive() {
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
        // Workspace stays alive even after closing its last panel
        let tm = shared.tab_manager.lock().unwrap();
        assert_eq!(tm.len(), 1);
        assert!(tm.selected().unwrap().is_empty());
    }

    #[test]
    fn close_panel_returns_false_for_unknown_panel() {
        let state = AppState::new(Arc::new(SharedState::new()));
        assert!(!state.close_panel(Uuid::new_v4(), true));
    }
}
