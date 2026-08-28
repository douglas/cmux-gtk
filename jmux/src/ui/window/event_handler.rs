use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::app::{lock_or_recover, AppState, UiEvent};
use crate::model::Workspace;
use crate::ui::notifications_panel;

#[allow(clippy::too_many_arguments)]
pub(super) fn bind_shared_state_updates(
    list_box: &gtk4::ListBox,
    content_box: &gtk4::Box,
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    mut ui_events: UnboundedReceiver<UiEvent>,
    nav_split_view: &adw::NavigationSplitView,
    sidebar_page: &adw::NavigationPage,
    notif_page: &adw::NavigationPage,
    showing_notifications: &Rc<Cell<bool>>,
    notif_panel: &notifications_panel::NotificationsPanel,
    toast_overlay: &adw::ToastOverlay,
    header: &adw::HeaderBar,
) {
    let state = state.clone();
    let list_box = list_box.clone();
    let content_box = content_box.clone();
    let window_weak = window.downgrade();
    let nav_split_view = nav_split_view.clone();
    let sidebar_page = sidebar_page.clone();
    let notif_page = notif_page.clone();
    let showing_notifications = showing_notifications.clone();
    let notif_panel = notif_panel.clone();
    let toast_overlay = toast_overlay.clone();
    let header = header.clone();

    // Debounce flag: prevents scheduling multiple idle callbacks for
    // metadata-only refreshes (SetTitle/SetPwd).  Cleared inside the callback.
    let metadata_idle_scheduled = Rc::new(Cell::new(false));
    // Throttle: track when the last metadata refresh actually ran.
    // If a refresh was done within METADATA_THROTTLE_MS, defer the next
    // one to avoid saturating the main loop with sidebar rebuilds when
    // shell integration fires SetTitle/SetPwd at high frequency (~22/s).
    let last_metadata_refresh = Rc::new(Cell::new(
        std::time::Instant::now() - std::time::Duration::from_secs(1),
    ));
    // Throttle for full rebuilds: panel switching fires notify_ui_refresh()
    // from multiple sites (tab click, click-to-focus) within ~500ms,
    // causing 3 rapid rebuild_content calls that starve input events.
    // After the first immediate rebuild, defer additional ones by 300ms.
    let full_refresh_scheduled = Rc::new(Cell::new(false));
    let last_full_refresh = Rc::new(Cell::new(
        std::time::Instant::now() - std::time::Duration::from_secs(1),
    ));

    glib::MainContext::default().spawn_local(async move {
        while let Some(event) = ui_events.recv().await {
            let mut pending = Some(event);
            let mut needs_refresh = false;
            let mut needs_metadata_refresh = false;
            loop {
                let event = match pending.take() {
                    Some(event) => event,
                    None => match ui_events.try_recv() {
                        Ok(event) => event,
                        Err(_) => break,
                    },
                };

                match event {
                    UiEvent::Refresh => needs_refresh = true,
                    UiEvent::MetadataRefresh => needs_metadata_refresh = true,
                    UiEvent::ShowToast(msg) => {
                        let toast = adw::Toast::new(&msg);
                        toast.set_timeout(4);
                        toast_overlay.add_toast(toast);
                    }
                    UiEvent::SendInput { panel_id, text } => {
                        let sent = state.send_input_to_panel(panel_id, &text);
                        if !sent {
                            tracing::warn!(
                                %panel_id,
                                "surface.send_input dropped because panel is not ready"
                            );
                        }
                    }
                    UiEvent::OpenSettings => {
                        if let Some(window) = window_weak.upgrade() {
                            let lb = list_box.clone();
                            let cb = content_box.clone();
                            let st = Rc::clone(&state);
                            let shared = st.shared.clone();
                            let app = window.application();
                            crate::ui::settings::show_settings(&window, shared, move || {
                                super::refresh_ui(&lb, &cb, &st);
                                // Register the quick-terminal hotkey if it was
                                // just enabled (no-op otherwise / if already up).
                                if let Some(app) = &app {
                                    crate::ui::quick_terminal::register_global_shortcut(
                                        app,
                                        st.shared.clone(),
                                    );
                                }
                            });
                        }
                    }
                    UiEvent::TriggerFlash { panel_id } => {
                        if !crate::settings::load().pane_flash_enabled {
                            continue;
                        }
                        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                            let widget = surface.clone().upcast::<gtk4::Widget>();
                            // Two-phase pulse: on → off → on → off (with weak ref guards)
                            widget.add_css_class("flash-panel");
                            let weak1 = widget.downgrade();
                            glib::timeout_add_local_once(
                                std::time::Duration::from_millis(200),
                                move || {
                                    let Some(w) = weak1.upgrade() else { return };
                                    w.remove_css_class("flash-panel");
                                    let weak2 = w.downgrade();
                                    glib::timeout_add_local_once(
                                        std::time::Duration::from_millis(150),
                                        move || {
                                            let Some(w) = weak2.upgrade() else { return };
                                            w.add_css_class("flash-panel");
                                            let weak3 = w.downgrade();
                                            glib::timeout_add_local_once(
                                                std::time::Duration::from_millis(200),
                                                move || {
                                                    if let Some(w) = weak3.upgrade() {
                                                        w.remove_css_class("flash-panel");
                                                    }
                                                },
                                            );
                                        },
                                    );
                                },
                            );
                        }
                    }
                    UiEvent::SendKey {
                        panel_id,
                        keyval,
                        keycode,
                        mods,
                    } => {
                        // If the ghostty surface is already in the cache, deliver immediately.
                        // Otherwise schedule a single retry on the next idle cycle so keystrokes
                        // sent immediately after a panel is focused (e.g. via the send-key socket
                        // command) are not silently dropped before the surface is initialized.
                        //
                        // Note: interactive keystrokes (keyboard → GTK → Ghostty) take a
                        // direct path and are never lost; only socket-driven SendKey events
                        // can arrive before surface initialisation.
                        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id).cloned() {
                            surface.send_key(keyval, keycode, mods);
                        } else {
                            let state_weak = Rc::downgrade(&state);
                            glib::idle_add_local_once(move || {
                                let Some(state) = state_weak.upgrade() else { return };
                                let surface = state.terminal_cache.borrow().get(&panel_id).cloned();
                                if let Some(surface) = surface {
                                    surface.send_key(keyval, keycode, mods);
                                } else {
                                    tracing::warn!(
                                        %panel_id,
                                        "SendKey dropped: surface not ready after idle retry"
                                    );
                                }
                            });
                        }
                    }
                    UiEvent::ReadText {
                        panel_id,
                        scrollback,
                        lines,
                        reply,
                    } => {
                        let text = state
                            .terminal_cache
                            .borrow()
                            .get(&panel_id)
                            .and_then(|s| {
                                if scrollback {
                                    s.read_scrollback_text()
                                } else {
                                    s.read_screen_text()
                                }
                            })
                            .map(|t| match lines {
                                Some(n) if n > 0 => {
                                    let all: Vec<&str> = t.lines().collect();
                                    let start = all.len().saturating_sub(n);
                                    all[start..].join("\n")
                                }
                                _ => t,
                            });
                        let _ = reply.send(text);
                    }
                    UiEvent::RefreshSurface { panel_id } => {
                        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                            surface.refresh();
                        }
                    }
                    UiEvent::ClearHistory { panel_id } => {
                        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                            surface.binding_action("clear_screen");
                            surface.refresh();
                        }
                    }
                    UiEvent::ToggleMinimalMode => {
                        let visible = !header.is_visible();
                        header.set_visible(visible);
                        let mut settings = crate::settings::load();
                        settings.minimal_mode = !visible;
                        let _ = crate::settings::save(&settings);
                    }
                    UiEvent::ToggleNotifications => {
                        if showing_notifications.get() {
                            nav_split_view.set_sidebar(Some(&sidebar_page));
                            showing_notifications.set(false);
                        } else {
                            notif_panel.refresh(&state);
                            nav_split_view.set_sidebar(Some(&notif_page));
                            showing_notifications.set(true);
                        }
                    }
                    UiEvent::ShowSidebar(show) => {
                        // Restore the workspace sidebar page first (in case notifications
                        // panel was active) then set the collapsed state.
                        if show && showing_notifications.get() {
                            nav_split_view.set_sidebar(Some(&sidebar_page));
                            showing_notifications.set(false);
                        }
                        nav_split_view.set_collapsed(!show);
                    }
                    UiEvent::ToggleSidebar => {
                        let currently_collapsed = nav_split_view.is_collapsed();
                        if !currently_collapsed && showing_notifications.get() {
                            // Collapsing: notifications panel stays as-is, just hide
                        }
                        nav_split_view.set_collapsed(!currently_collapsed);
                    }
                    UiEvent::DeferUnread => {
                        // Mark all unread notifications for the current workspace as read,
                        // clearing the unread badge (defer effect without a timer).
                        let workspace_id = {
                            let tm = lock_or_recover(&state.shared.tab_manager);
                            tm.selected().map(|ws| ws.id)
                        };
                        if let Some(workspace_id) = workspace_id {
                            mark_workspace_read(&state, workspace_id);
                            needs_metadata_refresh = true;
                        }
                    }
                    UiEvent::ToggleUnread => {
                        let workspace_id = {
                            let tm = lock_or_recover(&state.shared.tab_manager);
                            tm.selected().map(|ws| ws.id)
                        };
                        if let Some(workspace_id) = workspace_id {
                            let has_unread = {
                                let store = lock_or_recover(&state.shared.notifications);
                                store.unread_count_for_workspace(workspace_id) > 0
                            };
                            if has_unread {
                                // Mark all unread as read
                                mark_workspace_read(&state, workspace_id);
                            } else {
                                // Mark most recent notification as unread again
                                lock_or_recover(&state.shared.notifications)
                                    .mark_latest_unread_for_workspace(workspace_id);
                                // Bump the workspace unread count
                                if let Some(ws) = lock_or_recover(&state.shared.tab_manager)
                                    .workspace_mut(workspace_id)
                                {
                                    ws.unread_count = 1;
                                }
                            }
                            needs_metadata_refresh = true;
                        }
                    }
                    UiEvent::RenameTab { panel_id } => {
                        if let Some(window) = window_weak.upgrade() {
                            super::dialogs::show_rename_tab_dialog(&window, &state, panel_id);
                            needs_refresh = true;
                        }
                    }
                    // Search events are handled but we don't have the search
                    // overlay widget refs here. The search overlay reads state
                    UiEvent::SetTitle { surface, title } => {
                        // Sanitize terminal-sourced title: strip C0/C1 control chars
                        // to prevent escape sequence injection into GTK labels.
                        let title: String = title
                            .chars()
                            .filter(|c| !c.is_control())
                            .collect();
                        // Reverse-lookup panel_id from terminal_cache
                        let panel_id = state
                            .terminal_cache
                            .borrow()
                            .iter()
                            .find(|(_, s)| s.raw_surface() == surface.0)
                            .map(|(id, _)| *id);

                        if let Some(panel_id) = panel_id {
                            let mut tm = lock_or_recover(&state.shared.tab_manager);
                            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                                if let Some(panel) = ws.panels.get_mut(&panel_id) {
                                    panel.title = Some(title.clone());
                                }
                                if ws.focused_panel_id == Some(panel_id) {
                                    ws.process_title = title;
                                }
                            }
                            drop(tm);
                            needs_metadata_refresh = true;
                        }
                    }
                    UiEvent::SetPwd { surface, directory } => {
                        // Sanitize terminal-sourced directory path
                        let directory: String = directory
                            .chars()
                            .filter(|c| !c.is_control())
                            .collect();
                        let panel_id = state
                            .terminal_cache
                            .borrow()
                            .iter()
                            .find(|(_, s)| s.raw_surface() == surface.0)
                            .map(|(id, _)| *id);

                        if let Some(panel_id) = panel_id {
                            let mut tm = lock_or_recover(&state.shared.tab_manager);
                            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                                if let Some(panel) = ws.panels.get_mut(&panel_id) {
                                    panel.directory = Some(directory.clone());
                                }
                                if ws.focused_panel_id == Some(panel_id) {
                                    ws.current_directory = directory.clone();
                                    // Auto-detect git branch from directory
                                    ws.git_branch = super::styling::detect_git_branch(&directory);
                                }
                            }
                            drop(tm);
                            needs_metadata_refresh = true;
                        }
                    }
                    UiEvent::OpenFolderAsWorkspace => {
                        if let Some(window) = window_weak.upgrade() {
                            let state = state.clone();
                            let list_box = list_box.clone();
                            let content_box = content_box.clone();
                            #[allow(deprecated)]
                            let dialog = gtk4::FileChooserNative::builder()
                                .title("Open Folder as Workspace")
                                .transient_for(&window)
                                .modal(true)
                                .action(gtk4::FileChooserAction::SelectFolder)
                                .build();
                            #[allow(deprecated)]
                            dialog.connect_response(move |dlg, response| {
                                if response == gtk4::ResponseType::Accept {
                                    #[allow(deprecated)]
                                    if let Some(file) = dlg.file() {
                                        if let Some(path) = file.path() {
                                            let dir = path.to_string_lossy().to_string();
                                            let ws = Workspace::with_directory(&dir);
                                            let placement =
                                                crate::settings::load().new_workspace_placement;
                                            lock_or_recover(&state.shared.tab_manager)
                                                .add_workspace_with_placement(ws, placement);
                                            super::refresh_ui(&list_box, &content_box, &state);
                                        }
                                    }
                                }
                            });
                            dialog.show();
                        }
                    }
                    UiEvent::CopyMode { panel_id } => {
                        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                            surface.binding_action("copy_mode");
                            // Show vim badge overlay
                            crate::ui::terminal_panel::show_vim_badge(panel_id);
                            // Auto-hide after 30 seconds (copy mode may end earlier,
                            // but we can't detect Ghostty's internal state change)
                            glib::timeout_add_local_once(
                                std::time::Duration::from_secs(30),
                                move || {
                                    crate::ui::terminal_panel::hide_vim_badge(panel_id);
                                },
                            );
                        }
                    }
                    UiEvent::BrowserOpenInNewTab {
                        source_panel_id,
                        url,
                    } => {
                        // Open URL in a new browser tab in the same pane as
                        // source_panel_id (window.open / Ctrl+click / middle-click).
                        let mut tm = lock_or_recover(&state.shared.tab_manager);
                        if let Some(ws) = tm.selected_mut() {
                            let mut panel = crate::model::panel::Panel::new_browser();
                            panel.browser_url = Some(url.clone());
                            panel.directory = Some(url);
                            let new_panel_id = panel.id;
                            ws.panels.insert(new_panel_id, panel);
                            ws.layout.add_panel_to_pane(source_panel_id, new_panel_id);
                            ws.focused_panel_id = Some(new_panel_id);
                        }
                        drop(tm);
                        needs_refresh = true;
                    }
                    UiEvent::ReopenClosedBrowser => {
                        if let Some(url) = state.shared.pop_closed_browser_url() {
                            let mut tm = lock_or_recover(&state.shared.tab_manager);
                            if let Some(ws) = tm.selected_mut() {
                                let mut panel = crate::model::panel::Panel::new_browser();
                                panel.browser_url = Some(url.clone());
                                panel.directory = Some(url);
                                let panel_id = panel.id;
                                ws.panels.insert(panel_id, panel);
                                ws.layout.add_panel_to_pane(
                                    ws.focused_panel_id.unwrap_or(panel_id),
                                    panel_id,
                                );
                                ws.focused_panel_id = Some(panel_id);
                            }
                            drop(tm);
                            needs_refresh = true;
                        }
                    }
                    UiEvent::OpenUrlInBrowser { url } => {
                        // Check link routing — external patterns open in system browser
                        let settings = crate::settings::load();
                        if settings.link_routing.should_open_externally(&url) {
                            tracing::debug!(%url, "OpenUrlInBrowser → launching in system browser");
                            let _ = gio::AppInfo::launch_default_for_uri(
                                &url,
                                gio::AppLaunchContext::NONE,
                            );
                        } else {
                            // Route to a jmux browser panel
                            let mut tm = lock_or_recover(&state.shared.tab_manager);
                            let mut panel = crate::model::panel::Panel::new_browser();
                            panel.browser_url = Some(url.clone());
                            panel.directory = Some(url);
                            let panel_id = panel.id;
                            if let Some(ws) = tm.selected_mut() {
                                ws.panels.insert(panel_id, panel);
                                ws.layout.add_panel_to_pane(
                                    ws.focused_panel_id.unwrap_or(panel_id),
                                    panel_id,
                                );
                                ws.focused_panel_id = Some(panel_id);
                            }
                            drop(tm);
                            needs_refresh = true;
                        }
                    }
                    UiEvent::OpenMarkdownFile => {
                        let Some(window) = window_weak.upgrade() else {
                            continue;
                        };
                        let list_box = list_box.clone();
                        let content_box = content_box.clone();
                        let state = Rc::clone(&state);
                        #[allow(deprecated)]
                        let dialog = gtk4::FileChooserNative::new(
                            Some("Open Markdown File"),
                            Some(&window),
                            gtk4::FileChooserAction::Open,
                            Some("Open"),
                            Some("Cancel"),
                        );
                        let filter = gtk4::FileFilter::new();
                        filter.set_name(Some("Markdown files"));
                        filter.add_pattern("*.md");
                        filter.add_pattern("*.markdown");
                        filter.add_pattern("*.mdx");
                        dialog.add_filter(&filter);
                        dialog.connect_response(move |dialog, response| {
                            if response == gtk4::ResponseType::Accept {
                                if let Some(file) = dialog.file() {
                                    if let Some(path) = file.path() {
                                        let path_str = path.to_string_lossy().to_string();
                                        let panel =
                                            crate::model::panel::Panel::new_markdown(&path_str);
                                        let panel_id = panel.id;
                                        let mut tm = lock_or_recover(&state.shared.tab_manager);
                                        if let Some(ws) = tm.selected_mut() {
                                            ws.panels.insert(panel_id, panel);
                                            if let Some(focused) = ws.focused_panel_id {
                                                ws.layout.add_panel_to_pane(focused, panel_id);
                                            }
                                            ws.previous_focused_panel_id = ws.focused_panel_id;
                                            ws.focused_panel_id = Some(panel_id);
                                        }
                                        drop(tm);
                                        super::refresh_ui(&list_box, &content_box, &state);
                                    }
                                }
                            }
                        });
                        dialog.show();
                    }
                    #[cfg(feature = "webkit")]
                    UiEvent::BrowserAction { panel_id, action } => {
                        crate::ui::browser_panel::execute_action(panel_id, action);
                    }
                    #[cfg(feature = "webkit")]
                    UiEvent::ImportBrowserCookies { source, reply } => {
                        let result = crate::browser_import::import_from(source);
                        let _ = reply.send(result);
                    }
                    UiEvent::CreateWindow => {
                        if let Some(win) = window_weak.upgrade() {
                            if let Some(app) = win.application() {
                                if let Some(adw_app) = app.downcast_ref::<adw::Application>() {
                                    let new_window_id = uuid::Uuid::new_v4();
                                    crate::app::open_window(adw_app, &state, new_window_id);
                                }
                            }
                        }
                    }
                    UiEvent::ListDisplays { reply } => {
                        let names = list_monitor_names();
                        let _ = reply.send(names);
                    }
                    UiEvent::WindowToDisplay { monitor, reply } => {
                        let result = window_weak
                            .upgrade()
                            .ok_or_else(|| "No window".to_string())
                            .and_then(|win| place_window_on_monitor(&win, &monitor));
                        let _ = reply.send(result);
                    }
                    UiEvent::ReloadConfig => {
                        if let Some(app) = state.ghostty_app.borrow_mut().as_mut() {
                            app.reload_config();
                            let ui_config = crate::ghostty_config::GhosttyUiConfig::from_app(app);
                            tracing::info!(?ui_config, "Reloaded ghostty config");
                            crate::app::apply_ghostty_css(&ui_config);
                            *state.ghostty_ui_config.borrow_mut() = ui_config;
                        }
                        super::refresh_ui(&list_box, &content_box, &state);
                    }
                    UiEvent::ReloadTheme => {
                        tracing::info!("ReloadTheme: re-applying theme from settings");
                        crate::app::apply_theme_from_settings();
                    }
                    UiEvent::DesktopNotification {
                        surface,
                        title,
                        body,
                    } => {
                        // Reverse-lookup panel_id from terminal_cache
                        let panel_id = state
                            .terminal_cache
                            .borrow()
                            .iter()
                            .find(|(_, s)| s.raw_surface() == surface.0)
                            .map(|(id, _)| *id);

                        let ws_id = panel_id.and_then(|pid| {
                            let tm = lock_or_recover(&state.shared.tab_manager);
                            tm.find_workspace_with_panel(pid).map(|ws| ws.id)
                        });

                        // Record in notification store with desktop alert
                        {
                            let mut store = lock_or_recover(&state.shared.notifications);
                            store.add(&title, &body, ws_id, panel_id, true);
                        }

                        // Record workspace-level notification for sidebar badge
                        if let Some(ws_id) = ws_id {
                            let mut tm = lock_or_recover(&state.shared.tab_manager);
                            if let Some(ws) = tm.workspace_mut(ws_id) {
                                ws.record_notification(&title, &body, panel_id);
                            }
                        }

                        // Sidebar badge update only — no layout change needed.
                        needs_metadata_refresh = true;
                    }
                    UiEvent::OpenGoalDialog { prefill } => {
                        if let Some(window) = window_weak.upgrade() {
                            crate::ui::goal_dialog::show_goal_dialog(&window, &state, prefill);
                        }
                    }
                    UiEvent::OpenSshDialog => {
                        if let Some(window) = window_weak.upgrade() {
                            super::dialogs::show_ssh_dialog(&window, &state);
                        }
                    }
                    UiEvent::OpenSshDeepLink { destination, port } => {
                        let mut params = serde_json::json!({
                            "destination": destination,
                        });
                        if let Some(p) = port {
                            params["port"] = serde_json::json!(p);
                        }
                        let resp = crate::socket::v2::workspace::handle_workspace_create_ssh(
                            serde_json::json!(0),
                            &params,
                            &state.shared,
                        );
                        if resp.ok {
                            tracing::info!("SSH deep link opened workspace for {}", destination);
                        } else {
                            tracing::warn!(
                                "SSH deep link failed for {}: {:?}",
                                destination,
                                resp.error
                            );
                        }
                    }
                    UiEvent::RemoteConnect { workspace_id } => {
                        if !crate::settings::load().remote_ssh_enabled {
                            tracing::warn!("Remote SSH disabled in settings — ignoring connect request");
                        } else {
                            let config = {
                                let tm = lock_or_recover(&state.shared.tab_manager);
                                tm.workspace(workspace_id)
                                    .and_then(|ws| ws.remote_config.clone())
                            };
                            if let Some(config) = config {
                                let shared = state.shared.clone();
                                let ws_id = workspace_id;
                                // Update state to Connecting immediately
                                {
                                    let mut tm = lock_or_recover(&shared.tab_manager);
                                    if let Some(ws) = tm.workspace_mut(ws_id) {
                                        ws.remote_state =
                                            Some(crate::remote::session::RemoteState::Connecting);
                                    }
                                }
                                needs_refresh = true;
                                // Spawn connection in background
                                std::thread::spawn(move || {
                                    let controller =
                                        crate::remote::session::RemoteSessionController::new(
                                            config,
                                        );
                                    let session: crate::remote::session::SharedRemoteSession =
                                        std::sync::Arc::new(std::sync::Mutex::new(controller));
                                    let result = {
                                        let mut ctrl = session.lock().unwrap_or_else(|p| p.into_inner());
                                        ctrl.start()
                                    };
                                    let new_state = {
                                        let ctrl = session.lock().unwrap_or_else(|p| p.into_inner());
                                        ctrl.state.clone()
                                    };
                                    // Store session if connected, then start health monitor
                                    if result.is_ok() {
                                        lock_or_recover(&shared.remote_sessions)
                                            .insert(ws_id, session.clone());
                                        // Health monitor: detect SSH dropout and surface error
                                        let session_mon = session.clone();
                                        let shared_mon = shared.clone();
                                        std::thread::spawn(move || {
                                            loop {
                                                std::thread::sleep(
                                                    std::time::Duration::from_secs(10),
                                                );
                                                // Stop if session was removed (user disconnected)
                                                if !lock_or_recover(&shared_mon.remote_sessions)
                                                    .contains_key(&ws_id)
                                                {
                                                    break;
                                                }
                                                let alive = lock_or_recover(&*session_mon)
                                                    .is_alive();
                                                if !alive {
                                                    tracing::warn!(
                                                        %ws_id,
                                                        "Remote connection lost"
                                                    );
                                                    // Remove from sessions to prevent double-stop
                                                    lock_or_recover(&shared_mon.remote_sessions)
                                                        .remove(&ws_id);
                                                    shared_mon.send_ui_event(
                                                        UiEvent::RemoteStateChanged {
                                                            workspace_id: ws_id,
                                                            state: crate::remote::session::RemoteState::Error(
                                                                "Connection lost — use Reconnect to retry".to_string(),
                                                            ),
                                                        },
                                                    );
                                                    break;
                                                }
                                            }
                                        });
                                    }
                                    shared.send_ui_event(UiEvent::RemoteStateChanged {
                                        workspace_id: ws_id,
                                        state: new_state,
                                    });
                                });
                            }
                        }
                    }
                    UiEvent::RemoteDisconnect { workspace_id } => {
                        let session = lock_or_recover(&state.shared.remote_sessions)
                            .remove(&workspace_id);
                        if let Some(session) = session {
                            let mut ctrl = session.lock().unwrap_or_else(|p| p.into_inner());
                            ctrl.stop();
                        }
                        {
                            let mut tm = lock_or_recover(&state.shared.tab_manager);
                            if let Some(ws) = tm.workspace_mut(workspace_id) {
                                ws.remote_state =
                                    Some(crate::remote::session::RemoteState::Disconnected);
                            }
                        }
                        needs_refresh = true;
                    }
                    UiEvent::RemoteStateChanged {
                        workspace_id,
                        state: remote_state,
                    } => {
                        // Show toast for connection errors
                        if let crate::remote::session::RemoteState::Error(ref msg) = remote_state {
                            let toast = adw::Toast::new(msg);
                            toast.set_timeout(6);
                            toast_overlay.add_toast(toast);
                        }
                        let mut tm = lock_or_recover(&state.shared.tab_manager);
                        if let Some(ws) = tm.workspace_mut(workspace_id) {
                            ws.remote_state = Some(remote_state);
                        }
                        drop(tm);
                        needs_refresh = true;
                    }
                    UiEvent::OpenTaskManager => {
                        if let Some(window) = window_weak.upgrade() {
                            crate::ui::task_manager::show_task_manager(&window, &state);
                        }
                    }
                    UiEvent::OpenOverview => {
                        if let Some(window) = window_weak.upgrade() {
                            crate::ui::pane_overview::show_pane_overview(&window, &state);
                        }
                    }
                    UiEvent::OpenCommandPalette => {
                        if let Some(window) = window_weak.upgrade() {
                            let on_refresh: Rc<dyn Fn()> = {
                                let lb = list_box.clone();
                                let cb = content_box.clone();
                                let st = state.clone();
                                Rc::new(move || super::refresh_ui(&lb, &cb, &st))
                            };
                            crate::ui::command_palette::show_command_palette(
                                &window, &state, on_refresh,
                            );
                        }
                    }
                    UiEvent::ShowDock => {
                        if let Some(window) = window_weak.upgrade() {
                            if let Ok(wid) = uuid::Uuid::parse_str(&window.widget_name()) {
                                let dir = {
                                    let tm = lock_or_recover(&state.shared.tab_manager);
                                    tm.selected()
                                        .map(|w| w.current_directory.clone())
                                        .unwrap_or_default()
                                };
                                crate::ui::dock::set_visible(wid, &dir, &state, true);
                            }
                        }
                    }
                    UiEvent::RunCustomCommand(name) => {
                        crate::ui::command_palette::run_custom_command_by_name(&name, &state);
                    }
                    UiEvent::QuickTerminal(action) => {
                        if let Some(window) = window_weak.upgrade() {
                            if let Some(app) = window.application() {
                                crate::ui::quick_terminal::handle(action, &app, &state);
                            }
                        }
                    }
                    UiEvent::AgentResume => {
                        // Detect which agent (if any) is running in the focused
                        // terminal and send its resume command.
                        let panel_info = {
                            let tm = lock_or_recover(&state.shared.tab_manager);
                            tm.selected().and_then(|ws| {
                                ws.focused_panel_id.and_then(|pid| {
                                    ws.panels.get(&pid).map(|p| {
                                        (pid, p.title.clone(), p.command.clone())
                                    })
                                })
                            })
                        };

                        if let Some((panel_id, title, command)) = panel_info {
                            let resume_cmd =
                                crate::session::snapshot::detect_agent_resume_command(
                                    title.as_deref(),
                                    command.as_deref(),
                                );
                            if let Some(cmd) = resume_cmd {
                                let agent_restore = crate::settings::load().agent_restore;
                                if agent_restore.is_enabled_for(&cmd) {
                                    let text = format!("{}\n", cmd);
                                    let sent = state.send_input_to_panel(panel_id, &text);
                                    if !sent {
                                        tracing::warn!(
                                            %panel_id,
                                            "AgentResume: panel not ready"
                                        );
                                    }
                                }
                            }
                        }
                    }
                    // directly via its own callbacks.
                    UiEvent::StartSearch
                    | UiEvent::EndSearch
                    | UiEvent::SearchTotal
                    | UiEvent::SearchSelected => {}
                }
            }

            if needs_refresh && !full_refresh_scheduled.get() {
                const FULL_REFRESH_THROTTLE: std::time::Duration =
                    std::time::Duration::from_millis(300);
                let elapsed = std::time::Instant::now()
                    .duration_since(last_full_refresh.get());
                if elapsed >= FULL_REFRESH_THROTTLE {
                    // Enough time since last full rebuild — run immediately.
                    tracing::trace!("refresh_ui (full layout rebuild, immediate)");
                    last_full_refresh.set(std::time::Instant::now());
                    metadata_idle_scheduled.set(false);
                    super::refresh_ui(&list_box, &content_box, &state);
                } else {
                    // Rapid-fire rebuild detected (panel switching fires
                    // notify_ui_refresh from tab-click + click-to-focus).
                    // Defer to collapse into a single rebuild.
                    tracing::trace!("refresh_ui deferred (throttle, {}ms elapsed)", elapsed.as_millis());
                    full_refresh_scheduled.set(true);
                    let delay = FULL_REFRESH_THROTTLE - elapsed;
                    let lb = list_box.clone();
                    let cb = content_box.clone();
                    let st = state.clone();
                    let flag = full_refresh_scheduled.clone();
                    let ts = last_full_refresh.clone();
                    let mflag = metadata_idle_scheduled.clone();
                    glib::timeout_add_local_once(delay, move || {
                        flag.set(false);
                        ts.set(std::time::Instant::now());
                        mflag.set(false);
                        tracing::trace!("refresh_ui (full layout rebuild, deferred)");
                        super::refresh_ui(&lb, &cb, &st);
                    });
                }
            } else if needs_metadata_refresh && !metadata_idle_scheduled.get() {
                // Throttle metadata refreshes to at most once per 200ms.
                // Shell integration can fire SetTitle/SetPwd at ~22/s;
                // without throttling the sidebar rebuilds saturate the
                // main loop and starve input events (causing missed
                // Enter presses, etc.).
                const METADATA_THROTTLE: std::time::Duration =
                    std::time::Duration::from_millis(200);
                let elapsed = std::time::Instant::now()
                    .duration_since(last_metadata_refresh.get());
                metadata_idle_scheduled.set(true);
                if elapsed >= METADATA_THROTTLE {
                    // Enough time since last refresh — run on next idle.
                    let lb = list_box.clone();
                    let cb = content_box.clone();
                    let st = state.clone();
                    let flag = metadata_idle_scheduled.clone();
                    let ts = last_metadata_refresh.clone();
                    glib::idle_add_local_once(move || {
                        flag.set(false);
                        ts.set(std::time::Instant::now());
                        super::refresh_metadata(&lb, &cb, &st);
                    });
                } else {
                    // Throttled — defer until the throttle window expires.
                    let delay = METADATA_THROTTLE - elapsed;
                    let lb = list_box.clone();
                    let cb = content_box.clone();
                    let st = state.clone();
                    let flag = metadata_idle_scheduled.clone();
                    let ts = last_metadata_refresh.clone();
                    glib::timeout_add_local_once(delay, move || {
                        flag.set(false);
                        ts.set(std::time::Instant::now());
                        super::refresh_metadata(&lb, &cb, &st);
                    });
                }
            }
        }
    });
}

pub(super) fn select_workspace_by_index(state: &Rc<AppState>, index: usize) -> bool {
    let (selected, already_selected, workspace_id) = {
        let mut tab_manager = lock_or_recover(&state.shared.tab_manager);
        let already_selected = tab_manager.selected_index() == Some(index);
        let selected = tab_manager.select(index);
        let workspace_id = tab_manager.get(index).map(|workspace| workspace.id);
        (selected, already_selected, workspace_id)
    };

    if !selected || already_selected {
        return false;
    }

    if let Some(workspace_id) = workspace_id {
        mark_workspace_read(state, workspace_id);
    }

    true
}

pub(super) fn select_latest_unread(state: &Rc<AppState>) -> bool {
    let workspace_id = {
        let mut tab_manager = lock_or_recover(&state.shared.tab_manager);
        tab_manager.select_latest_unread()
    };

    let Some(workspace_id) = workspace_id else {
        return false;
    };

    mark_workspace_read(state, workspace_id);
    true
}

pub(super) fn mark_workspace_read(state: &Rc<AppState>, workspace_id: uuid::Uuid) {
    lock_or_recover(&state.shared.notifications).mark_workspace_read(workspace_id);

    if let Some(workspace) = lock_or_recover(&state.shared.tab_manager).workspace_mut(workspace_id)
    {
        workspace.mark_notifications_read();
        workspace.clear_attention();
    }
}

/// Names (connector ids) of the connected monitors, e.g. ["DP-1", "HDMI-1"].
fn list_monitor_names() -> Vec<String> {
    let Some(display) = gtk4::gdk::Display::default() else {
        return Vec::new();
    };
    let monitors = display.monitors();
    let mut names = Vec::new();
    for i in 0..monitors.n_items() {
        if let Some(m) = monitors
            .item(i)
            .and_then(|o| o.downcast::<gtk4::gdk::Monitor>().ok())
        {
            let name = m
                .connector()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| m.model().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("monitor-{i}"));
            names.push(name);
        }
    }
    names
}

/// Place a window on a monitor identified by connector name or 0-based index.
///
/// Wayland forbids positioning a normal top-level window, so we fullscreen on
/// the target monitor — the closest portable equivalent. Returns the matched
/// monitor's label.
fn place_window_on_monitor(
    window: &adw::ApplicationWindow,
    target: &str,
) -> Result<String, String> {
    let Some(display) = gtk4::gdk::Display::default() else {
        return Err("No default display".to_string());
    };
    let monitors = display.monitors();
    let idx_target = target.parse::<u32>().ok();
    for i in 0..monitors.n_items() {
        let Some(m) = monitors
            .item(i)
            .and_then(|o| o.downcast::<gtk4::gdk::Monitor>().ok())
        else {
            continue;
        };
        let name = m.connector().map(|s| s.to_string()).unwrap_or_default();
        if name.eq_ignore_ascii_case(target) || idx_target == Some(i) {
            window.fullscreen_on_monitor(&m);
            let label = if name.is_empty() {
                format!("monitor-{i}")
            } else {
                name
            };
            return Ok(label);
        }
    }
    Err(format!(
        "No monitor matching '{target}' (available: {})",
        list_monitor_names().join(", ")
    ))
}

/// The maximum level of UI refresh a `UiEvent` can require.
///
/// This is used to ensure metadata-only events (title, PWD) never trigger
/// a full layout rebuild, which would unparent browser panels.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
enum RefreshKind {
    /// No UI refresh needed (e.g. search events handled elsewhere).
    None,
    /// Sidebar + window title only — layout is unchanged.
    MetadataOnly,
    /// Full rebuild: sidebar + content layout + window title.
    Full,
}

/// Classify a `UiEvent` by the refresh it may require.
///
/// This is a pure function with no GTK dependencies, so it can be unit-tested.
/// The dispatch loop in `bind_shared_state_updates` must respect this
/// classification: `SetTitle` and `SetPwd` are `MetadataOnly` and must never
/// set `needs_refresh = true`.
#[cfg(test)]
fn event_refresh_kind(event: &UiEvent) -> RefreshKind {
    match event {
        // Metadata changes: only sidebar labels + window title need updating.
        // Must NOT trigger rebuild_content — browser panels would unparent.
        UiEvent::SetTitle { .. }
        | UiEvent::SetPwd { .. }
        | UiEvent::DesktopNotification { .. } => RefreshKind::MetadataOnly,

        // Notification shortcut events: only sidebar badge needs updating.
        UiEvent::DeferUnread | UiEvent::ToggleUnread => RefreshKind::MetadataOnly,

        // No UI refresh — handled via dedicated callbacks or state only.
        UiEvent::StartSearch
        | UiEvent::EndSearch
        | UiEvent::SearchTotal
        | UiEvent::SearchSelected
        | UiEvent::SendInput { .. }
        | UiEvent::SendKey { .. }
        | UiEvent::ReadText { .. }
        | UiEvent::RefreshSurface { .. }
        | UiEvent::ClearHistory { .. }
        | UiEvent::TriggerFlash { .. }
        | UiEvent::QuickTerminal(_)
        | UiEvent::ToggleMinimalMode => RefreshKind::None,

        // Task Manager opens a secondary window — no layout rebuild needed.
        UiEvent::OpenTaskManager
        | UiEvent::OpenOverview
        | UiEvent::OpenCommandPalette
        | UiEvent::ShowDock => RefreshKind::None,

        // ShowSidebar/ToggleSidebar collapse/expand the NavigationSplitView only.
        UiEvent::ShowSidebar(_) | UiEvent::ToggleSidebar => RefreshKind::None,

        // Everything else may require a full layout rebuild.
        _ => RefreshKind::Full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    fn null_surface() -> crate::app::SendSurfacePtr {
        crate::app::SendSurfacePtr(ptr::null_mut())
    }

    // ── The regression tests ───────────────────────────────────────────────
    // These guard the fix for GitHub issue #1: SetTitle and SetPwd must never
    // trigger a full layout rebuild (which unparents browser panels).

    #[test]
    fn set_title_is_metadata_only() {
        let event = UiEvent::SetTitle {
            surface: null_surface(),
            title: "vim".to_string(),
        };
        assert_eq!(
            event_refresh_kind(&event),
            RefreshKind::MetadataOnly,
            "SetTitle must not trigger rebuild_content — browser panels would reload"
        );
    }

    #[test]
    fn set_pwd_is_metadata_only() {
        let event = UiEvent::SetPwd {
            surface: null_surface(),
            directory: "/home/user/project".to_string(),
        };
        assert_eq!(
            event_refresh_kind(&event),
            RefreshKind::MetadataOnly,
            "SetPwd must not trigger rebuild_content — browser panels would reload"
        );
    }

    #[test]
    fn set_title_is_not_full_refresh() {
        let event = UiEvent::SetTitle {
            surface: null_surface(),
            title: "bash".to_string(),
        };
        assert_ne!(event_refresh_kind(&event), RefreshKind::Full);
    }

    #[test]
    fn set_pwd_is_not_full_refresh() {
        let event = UiEvent::SetPwd {
            surface: null_surface(),
            directory: "/tmp".to_string(),
        };
        assert_ne!(event_refresh_kind(&event), RefreshKind::Full);
    }

    // ── Sanity checks for other event classes ─────────────────────────────

    #[test]
    fn search_events_are_noop() {
        for event in [
            UiEvent::StartSearch,
            UiEvent::EndSearch,
            UiEvent::SearchTotal,
            UiEvent::SearchSelected,
        ] {
            assert_eq!(
                event_refresh_kind(&event),
                RefreshKind::None,
                "{event:?} should not trigger any refresh"
            );
        }
    }

    #[test]
    fn structural_events_are_full_refresh() {
        // Events that change layout must still trigger a full rebuild.
        assert_eq!(event_refresh_kind(&UiEvent::Refresh), RefreshKind::Full);
        assert_eq!(event_refresh_kind(&UiEvent::ToggleNotifications), RefreshKind::Full);
        assert_eq!(event_refresh_kind(&UiEvent::OpenFolderAsWorkspace), RefreshKind::Full);
        assert_eq!(event_refresh_kind(&UiEvent::CreateWindow), RefreshKind::Full);
    }

    #[test]
    fn desktop_notification_is_metadata_only() {
        // Notifications update sidebar badge only — must not unparent browser panels.
        let event = UiEvent::DesktopNotification {
            surface: null_surface(),
            title: "Done".to_string(),
            body: "cargo build finished".to_string(),
        };
        assert_eq!(
            event_refresh_kind(&event),
            RefreshKind::MetadataOnly,
            "DesktopNotification must not trigger rebuild_content — browser panels would reload"
        );
    }
}
