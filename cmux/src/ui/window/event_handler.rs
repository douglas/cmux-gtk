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

    glib::MainContext::default().spawn_local(async move {
        while let Some(event) = ui_events.recv().await {
            let mut pending = Some(event);
            let mut needs_refresh = false;
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
                            crate::ui::settings::show_settings(&window, move || {
                                super::refresh_ui(&lb, &cb, &st);
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
                        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                            surface.send_key(keyval, keycode, mods);
                        }
                    }
                    UiEvent::ReadText { panel_id, reply } => {
                        let text = state
                            .terminal_cache
                            .borrow()
                            .get(&panel_id)
                            .and_then(|s| s.read_screen_text());
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
                            needs_refresh = true;
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
                            needs_refresh = true;
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
                            // Route to a cmux browser panel
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

                        needs_refresh = true;
                    }
                    UiEvent::OpenSshDialog => {
                        if let Some(window) = window_weak.upgrade() {
                            super::dialogs::show_ssh_dialog(&window, &state);
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
                                    // Store session if connected
                                    if result.is_ok() {
                                        lock_or_recover(&shared.remote_sessions)
                                            .insert(ws_id, session);
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
                        let mut tm = lock_or_recover(&state.shared.tab_manager);
                        if let Some(ws) = tm.workspace_mut(workspace_id) {
                            ws.remote_state = Some(remote_state);
                        }
                        drop(tm);
                        needs_refresh = true;
                    }
                    // directly via its own callbacks.
                    UiEvent::StartSearch
                    | UiEvent::EndSearch
                    | UiEvent::SearchTotal
                    | UiEvent::SearchSelected => {}
                }
            }

            if needs_refresh {
                super::refresh_ui(&list_box, &content_box, &state);
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
