//! Main application window using AdwNavigationSplitView.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::sync::mpsc::UnboundedReceiver;

use std::cell::Cell;


use crate::app::{lock_or_recover, AppState, UiEvent};
use crate::model::panel::{GitBranch, SplitOrientation};
use crate::model::{PanelType, Workspace};
use crate::ui::{notifications_panel, search_overlay, sidebar, split_view};

/// Create the main application window.
pub fn create_window(
    app: &adw::Application,
    state: &Rc<AppState>,
    ui_events: UnboundedReceiver<UiEvent>,
) -> adw::ApplicationWindow {
    install_css();

    // Use saved window geometry if available, otherwise defaults
    let (width, height) = *lock_or_recover(&state.shared.window_size);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("cmux")
        .default_width(width)
        .default_height(height)
        .build();

    let split_view = adw::NavigationSplitView::new();
    split_view.set_min_sidebar_width(220.0);
    split_view.set_max_sidebar_width(360.0);
    split_view.set_vexpand(true);
    split_view.set_hexpand(true);

    let sidebar_widgets = sidebar::create_sidebar(state);
    let list_box = sidebar_widgets.list_box.clone();
    let sidebar_page = adw::NavigationPage::new(&sidebar_widgets.root, "Workspaces");
    split_view.set_sidebar(Some(&sidebar_page));

    let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content_box.set_hexpand(true);
    content_box.set_vexpand(true);
    rebuild_content(&content_box, state);

    // Search overlay wraps the content area
    let search = search_overlay::create_search_overlay(&content_box.clone().upcast(), state);
    let search_bar = search.search_bar.clone();
    let search_entry = search.entry.clone();
    let search_count_label = search.count_label.clone();
    let search_state = search.state.clone();

    let content_page = adw::NavigationPage::new(&search.overlay, "Terminal");
    split_view.set_content(Some(&content_page));

    // Notification panel — replaces sidebar when toggled
    let notif_panel = notifications_panel::create_notifications_panel(state);
    let notif_root = notif_panel.root.clone();
    let notif_page = adw::NavigationPage::new(&notif_root, "Notifications");
    let showing_notifications: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    bind_sidebar_selection(&list_box, &content_box, state);
    bind_shared_state_updates(
        &list_box,
        &content_box,
        &window,
        state,
        ui_events,
        &split_view,
        &sidebar_page,
        &notif_page,
        &showing_notifications,
        &notif_panel,
    );

    let header = adw::HeaderBar::new();
    let initial_title = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.selected()
            .map(|ws| ws.display_title().to_string())
            .unwrap_or_else(|| "cmux".to_string())
    };
    let header_title = gtk4::Label::new(Some(&initial_title));
    header_title.add_css_class("heading");
    header_title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    header.set_title_widget(Some(&header_title));

    let new_ws_btn = gtk4::Button::from_icon_name("tab-new-symbolic");
    new_ws_btn.set_tooltip_text(Some("New Workspace"));
    {
        let state = state.clone();
        let list_box = list_box.clone();
        let content_box = content_box.clone();
        new_ws_btn.connect_clicked(move |_| {
            let workspace = Workspace::new();
            let placement = crate::settings::load().new_workspace_placement;
            lock_or_recover(&state.shared.tab_manager)
                .add_workspace_with_placement(workspace, placement);
            refresh_ui(&list_box, &content_box, &state);
        });
    }
    header.pack_start(&new_ws_btn);

    let split_h_btn = gtk4::Button::from_icon_name("view-dual-symbolic");
    split_h_btn.set_tooltip_text(Some("Split Horizontal"));
    {
        let state = state.clone();
        let list_box = list_box.clone();
        let content_box = content_box.clone();
        split_h_btn.connect_clicked(move |_| {
            if let Some(workspace) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                workspace.split(SplitOrientation::Horizontal, PanelType::Terminal);
            }
            refresh_ui(&list_box, &content_box, &state);
        });
    }
    header.pack_start(&split_h_btn);

    let split_v_btn = gtk4::Button::from_icon_name("view-paged-symbolic");
    split_v_btn.set_tooltip_text(Some("Split Vertical"));
    {
        let state = state.clone();
        let list_box = list_box.clone();
        let content_box = content_box.clone();
        split_v_btn.connect_clicked(move |_| {
            if let Some(workspace) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                workspace.split(SplitOrientation::Vertical, PanelType::Terminal);
            }
            refresh_ui(&list_box, &content_box, &state);
        });
    }
    header.pack_start(&split_v_btn);

    // Settings button (right side of header bar)
    let settings_btn = gtk4::Button::from_icon_name("preferences-system-symbolic");
    settings_btn.set_tooltip_text(Some("Settings"));
    settings_btn.add_css_class("flat");
    {
        let window_ref = window.clone();
        settings_btn.connect_clicked(move |_| {
            super::settings::show_settings(&window_ref);
        });
    }
    header.pack_end(&settings_btn);

    let outer_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    outer_box.append(&header);
    outer_box.append(&split_view);

    window.set_content(Some(&outer_box));
    setup_shortcuts(
        &window,
        state,
        &list_box,
        &content_box,
        &search_bar,
        &search_entry,
        &search_count_label,
        &search_state,
        &split_view,
        &sidebar_page,
        &notif_page,
        &showing_notifications,
        &notif_panel,
    );

    {
        let state = state.clone();
        window.connect_is_active_notify(move |window| {
            let active = window.is_active();
            if let Some(app) = state.ghostty_app.borrow().as_ref() {
                app.set_focus(active);
            }
        });
    }

    // Quit confirmation dialog
    {
        let state = state.clone();
        window.connect_close_request(move |window| {
            let settings = crate::settings::load();
            if !settings.confirm_before_close {
                return glib::Propagation::Proceed;
            }

            let terminal_count = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.iter()
                    .flat_map(|ws| ws.panels.values())
                    .filter(|p| p.panel_type == PanelType::Terminal)
                    .count()
            };

            if terminal_count == 0 {
                return glib::Propagation::Proceed;
            }

            let dialog = adw::MessageDialog::new(
                Some(window),
                Some("Quit cmux?"),
                None,
            );
            dialog.set_body(&format!(
                "There {} still active. Are you sure you want to quit?",
                if terminal_count == 1 {
                    "is 1 terminal".to_string()
                } else {
                    format!("are {terminal_count} terminals")
                }
            ));
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("quit", "Quit");
            dialog.set_default_response(Some("cancel"));
            dialog.set_response_appearance("quit", adw::ResponseAppearance::Destructive);

            let window = window.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "quit" {
                    window.destroy();
                }
            });

            dialog.present();
            glib::Propagation::Stop
        });
    }

    window
}

/// Rebuild the content area from the current workspace layout.
///
/// GtkGLArea breaks if you remove a widget and re-add it in the same GTK
/// main-loop tick (the GL context gets destroyed and recreated, invalidating
/// the renderer's state). Ghostty's own GTK app has the same problem and
/// solves it with a two-phase approach: orphan all surfaces first, then
/// rebuild in an idle callback once GTK has fully processed the unparent.
pub fn rebuild_content(content_box: &gtk4::Box, state: &Rc<AppState>) {
    // Phase 1: Remove all children so GTK can fully orphan the surfaces.
    while let Some(child) = content_box.first_child() {
        content_box.remove(&child);
    }

    // Explicitly unparent cached GL surfaces — they may have been nested
    // inside intermediate containers (Paned, Box) that were just removed.
    for surface in state.terminal_cache.borrow().values() {
        if let Some(parent) = surface.parent() {
            if let Ok(parent_box) = parent.downcast::<gtk4::Box>() {
                parent_box.remove(surface);
            }
        }
    }

    // Phase 2: Schedule the actual rebuild on the next idle tick, giving GTK
    // time to fully process the unparent cascade before re-adding surfaces.
    let content_box = content_box.clone();
    let state = state.clone();
    glib::idle_add_local_once(move || {
        // Guard: clear any children that may have been added by a racing
        // rebuild callback (multiple refreshes can queue before idle fires).
        while let Some(child) = content_box.first_child() {
            content_box.remove(&child);
        }

        // Clone workspace data out of the lock so we don't hold it during
        // GTK widget construction (build_layout callbacks may re-acquire it).
        let workspace_data = {
            let tab_manager = lock_or_recover(&state.shared.tab_manager);
            tab_manager.selected().map(|ws| {
                (
                    ws.id,
                    ws.layout.clone(),
                    ws.panels.clone(),
                    ws.attention_panel_id,
                    ws.zoomed_panel_id,
                    ws.focused_panel_id,
                )
            })
        };

        if let Some((id, layout, panels, attention_panel_id, zoomed_panel_id, focused_panel_id)) =
            workspace_data
        {
            let effective_attention = if crate::settings::load().pane_attention_ring {
                attention_panel_id
            } else {
                None
            };
            let widget = if let Some(zoomed_id) = zoomed_panel_id {
                split_view::build_zoomed(zoomed_id, &panels, &state)
            } else {
                split_view::build_layout(
                    id,
                    &layout,
                    &panels,
                    effective_attention,
                    focused_panel_id,
                    &state,
                )
            };
            content_box.append(&widget);
        } else {
            let label = gtk4::Label::new(Some("No workspace selected"));
            label.add_css_class("dim-label");
            content_box.append(&label);
        }
    });
}

fn refresh_ui(list_box: &gtk4::ListBox, content_box: &gtk4::Box, state: &Rc<AppState>) {
    state.prune_terminal_cache();
    sidebar::refresh_sidebar(list_box, state);
    rebuild_content(content_box, state);

    // Update window title to reflect selected workspace
    if let Some(root) = content_box.root() {
        if let Some(window) = root.downcast_ref::<adw::ApplicationWindow>() {
            let titles = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.selected().map(|ws| {
                    let title = ws.display_title();
                    let dir = crate::ui::sidebar::compact_path(&ws.current_directory);
                    (format!("{title} — {dir} — cmux"), title.to_string())
                })
            };
            if let Some((full_title, ws_title)) = titles {
                window.set_title(Some(&full_title));
                // Update the header bar's title label
                if let Some(outer) = window.content() {
                    if let Some(hb) = outer.first_child() {
                        if let Some(header) = hb.downcast_ref::<adw::HeaderBar>() {
                            if let Some(tw) = header.title_widget() {
                                if let Some(lbl) = tw.downcast_ref::<gtk4::Label>() {
                                    lbl.set_text(&ws_title);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn bind_sidebar_selection(list_box: &gtk4::ListBox, content_box: &gtk4::Box, state: &Rc<AppState>) {
    let state = state.clone();
    let lb = list_box.clone();
    let content_box = content_box.clone();

    list_box.connect_row_selected(move |_list_box, row| {
        let Some(row) = row else {
            return;
        };

        let index = row.index();
        if index < 0 {
            return;
        }
        if select_workspace_by_index(&state, index as usize) {
            refresh_ui(&lb, &content_box, &state);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn bind_shared_state_updates(
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
                            super::settings::show_settings(&window);
                        }
                    }
                    UiEvent::TriggerFlash { panel_id } => {
                        if !crate::settings::load().pane_flash_enabled {
                            continue;
                        }
                        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                            let widget = surface.clone().upcast::<gtk4::Widget>();
                            // Two-phase pulse: on → off → on → off
                            widget.add_css_class("flash-panel");
                            let w = widget.clone();
                            glib::timeout_add_local_once(
                                std::time::Duration::from_millis(200),
                                move || {
                                    w.remove_css_class("flash-panel");
                                    let w2 = w.clone();
                                    glib::timeout_add_local_once(
                                        std::time::Duration::from_millis(150),
                                        move || {
                                            w2.add_css_class("flash-panel");
                                            let w3 = w2.clone();
                                            glib::timeout_add_local_once(
                                                std::time::Duration::from_millis(200),
                                                move || {
                                                    w3.remove_css_class("flash-panel");
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
                            show_rename_tab_dialog(&window, &state, panel_id);
                            needs_refresh = true;
                        }
                    }
                    // Search events are handled but we don't have the search
                    // overlay widget refs here. The search overlay reads state
                    UiEvent::SetTitle { surface, title } => {
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
                                    ws.git_branch = detect_git_branch(&directory);
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
                                            refresh_ui(&list_box, &content_box, &state);
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
                        }
                    }
                    UiEvent::ReopenClosedBrowser => {
                        if let Some(url) = state.shared.pop_closed_browser_url() {
                            let mut tm = lock_or_recover(&state.shared.tab_manager);
                            if let Some(ws) = tm.selected_mut() {
                                let mut panel =
                                    crate::model::panel::Panel::new_browser();
                                panel.browser_url = Some(url);
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
                                        let mut tm =
                                            lock_or_recover(&state.shared.tab_manager);
                                        if let Some(ws) = tm.selected_mut() {
                                            ws.panels.insert(panel_id, panel);
                                            if let Some(focused) = ws.focused_panel_id {
                                                ws.layout
                                                    .add_panel_to_pane(focused, panel_id);
                                            }
                                            ws.previous_focused_panel_id =
                                                ws.focused_panel_id;
                                            ws.focused_panel_id = Some(panel_id);
                                        }
                                        drop(tm);
                                        refresh_ui(&list_box, &content_box, &state);
                                    }
                                }
                            }
                        });
                        dialog.show();
                    }
                    UiEvent::BrowserAction { panel_id, action } => {
                        crate::ui::browser_panel::execute_action(panel_id, action);
                    }
                    // directly via its own callbacks.
                    UiEvent::StartSearch
                    | UiEvent::EndSearch
                    | UiEvent::SearchTotal { .. }
                    | UiEvent::SearchSelected { .. } => {}
                }
            }

            if needs_refresh {
                refresh_ui(&list_box, &content_box, &state);
            }
        }
    });
}

fn select_workspace_by_index(state: &Rc<AppState>, index: usize) -> bool {
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

fn select_latest_unread(state: &Rc<AppState>) -> bool {
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

fn mark_workspace_read(state: &Rc<AppState>, workspace_id: uuid::Uuid) {
    lock_or_recover(&state.shared.notifications).mark_workspace_read(workspace_id);

    if let Some(workspace) =
        lock_or_recover(&state.shared.tab_manager).workspace_mut(workspace_id)
    {
        workspace.mark_notifications_read();
        workspace.clear_attention();
    }
}

fn show_rename_dialog(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    list_box: &gtk4::ListBox,
    content_box: &gtk4::Box,
    current_title: &str,
) {
    let dialog = adw::MessageDialog::new(Some(window), Some("Rename Workspace"), None);
    dialog.set_body("Enter a new name for this workspace:");

    let entry = gtk4::Entry::new();
    entry.set_text(current_title);
    entry.set_activates_default(true);
    dialog.set_extra_child(Some(&entry));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("rename", "Rename");
    dialog.set_default_response(Some("rename"));
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);

    let state = state.clone();
    let list_box = list_box.clone();
    let content_box = content_box.clone();
    dialog.connect_response(None, move |dialog, response| {
        if response == "rename" {
            let entry = dialog
                .extra_child()
                .and_then(|w| w.downcast::<gtk4::Entry>().ok());
            if let Some(entry) = entry {
                let new_name = entry.text().to_string();
                if !new_name.is_empty() {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        ws.custom_title = Some(new_name);
                    }
                    drop(tm);
                    refresh_ui(&list_box, &content_box, &state);
                }
            }
        }
    });

    dialog.present();
}

#[allow(clippy::too_many_arguments)]
fn setup_shortcuts(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    list_box: &gtk4::ListBox,
    content_box: &gtk4::Box,
    search_bar: &gtk4::Box,
    search_entry: &gtk4::SearchEntry,
    search_count_label: &gtk4::Label,
    search_state: &Rc<search_overlay::SearchState>,
    nav_split_view: &adw::NavigationSplitView,
    sidebar_page: &adw::NavigationPage,
    notif_page: &adw::NavigationPage,
    showing_notifications: &Rc<Cell<bool>>,
    notif_panel: &notifications_panel::NotificationsPanel,
) {
    let controller = gtk4::EventControllerKey::new();

    let state = state.clone();
    let list_box = list_box.clone();
    let content_box = content_box.clone();
    let search_bar = search_bar.clone();
    let search_entry = search_entry.clone();
    let _search_count_label = search_count_label.clone();
    let _search_state = search_state.clone();
    let nav_split_view = nav_split_view.clone();
    let sidebar_page = sidebar_page.clone();
    let notif_page = notif_page.clone();
    let showing_notifications = showing_notifications.clone();
    let notif_panel = notif_panel.clone();
    let window_weak = window.downgrade();

    controller.connect_key_pressed(move |_controller, keyval, _keycode, modifier| {
        let ctrl = modifier.contains(gdk4::ModifierType::CONTROL_MASK);
        let shift = modifier.contains(gdk4::ModifierType::SHIFT_MASK);
        let alt = modifier.contains(gdk4::ModifierType::ALT_MASK);

        // Alt+Arrow: Directional pane focus
        if alt && !ctrl && !shift {
            use crate::model::panel::Direction;
            let direction = match keyval {
                gdk4::Key::Left => Some(Direction::Left),
                gdk4::Key::Right => Some(Direction::Right),
                gdk4::Key::Up => Some(Direction::Up),
                gdk4::Key::Down => Some(Direction::Down),
                _ => None,
            };
            if let Some(dir) = direction {
                let changed = {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        if let Some(current) = ws.focused_panel_id {
                            if let Some(neighbor) = ws.layout.neighbor(current, dir) {
                                ws.focus_panel(neighbor)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if changed {
                    refresh_ui(&list_box, &content_box, &state);
                }
                return glib::Propagation::Stop;
            }
        }

        // Ctrl+Alt combinations (no shift)
        if ctrl && alt && !shift {
            match keyval {
                // Ctrl+Alt+D: Split browser horizontal
                gdk4::Key::d => {
                    if let Some(workspace) =
                        lock_or_recover(&state.shared.tab_manager).selected_mut()
                    {
                        workspace.split(
                            SplitOrientation::Horizontal,
                            PanelType::Browser,
                        );
                    }
                    refresh_ui(&list_box, &content_box, &state);
                    return glib::Propagation::Stop;
                }
                // Ctrl+Alt+E: Split browser vertical
                gdk4::Key::e => {
                    if let Some(workspace) =
                        lock_or_recover(&state.shared.tab_manager).selected_mut()
                    {
                        workspace.split(
                            SplitOrientation::Vertical,
                            PanelType::Browser,
                        );
                    }
                    refresh_ui(&list_box, &content_box, &state);
                    return glib::Propagation::Stop;
                }
                // Ctrl+Alt+C: Toggle browser JS console
                gdk4::Key::c => {
                    let panel_id = {
                        let tm = lock_or_recover(&state.shared.tab_manager);
                        tm.selected().and_then(|ws| {
                            ws.focused_panel_id.and_then(|pid| {
                                ws.panels.get(&pid).and_then(|p| {
                                    (p.panel_type == PanelType::Browser).then_some(pid)
                                })
                            })
                        })
                    };
                    if let Some(panel_id) = panel_id {
                        crate::ui::browser_panel::toggle_console(panel_id);
                    }
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
        }

        // Ctrl+Shift+Alt+W: Close other tabs in the current pane
        if ctrl && shift && alt && keyval == gdk4::Key::W {
            let closed = {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(ws) = tm.selected_mut() {
                    if let Some(panel_id) = ws.focused_panel_id {
                        let pane_ids =
                            ws.layout.find_pane_with_panel_readonly(panel_id);
                        if let Some(pane_ids) = pane_ids {
                            let to_close: Vec<uuid::Uuid> = pane_ids
                                .iter()
                                .filter(|&&pid| pid != panel_id)
                                .copied()
                                .collect();
                            for pid in &to_close {
                                ws.panels.remove(pid);
                                ws.layout.remove_panel(*pid);
                            }
                            !to_close.is_empty()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if closed {
                refresh_ui(&list_box, &content_box, &state);
            }
            return glib::Propagation::Stop;
        }

        match (keyval, ctrl, shift) {
            // Ctrl+Comma or Ctrl+Shift+Comma: Settings
            (gdk4::Key::comma, true, false) | (gdk4::Key::comma, true, true) => {
                if let Some(window) = window_weak.upgrade() {
                    super::settings::show_settings(&window);
                }
                glib::Propagation::Stop
            }
            // Ctrl+F: Toggle terminal find
            (gdk4::Key::f, true, false) => {
                if search_bar.is_visible() {
                    search_bar.set_visible(false);
                    // Return focus to terminal content
                    content_box.grab_focus();
                } else {
                    search_bar.set_visible(true);
                    search_entry.grab_focus();
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+I: Toggle notification panel
            (gdk4::Key::I, true, true) => {
                if showing_notifications.get() {
                    // Switch back to workspaces sidebar
                    nav_split_view.set_sidebar(Some(&sidebar_page));
                    showing_notifications.set(false);
                } else {
                    // Refresh and show notification panel
                    notif_panel.refresh(&state);
                    nav_split_view.set_sidebar(Some(&notif_page));
                    showing_notifications.set(true);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+P: Command palette
            (gdk4::Key::P, true, true) => {
                if let Some(window) = window_weak.upgrade() {
                    let lb = list_box.clone();
                    let cb = content_box.clone();
                    let st = state.clone();
                    let on_refresh = Rc::new(move || refresh_ui(&lb, &cb, &st));
                    super::command_palette::show_command_palette(&window, &state, on_refresh);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+Z: Toggle pane zoom
            (gdk4::Key::Z, true, true) => {
                let changed = {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        if ws.zoomed_panel_id.is_some() {
                            ws.zoomed_panel_id = None;
                        } else {
                            ws.zoomed_panel_id = ws.focused_panel_id;
                        }
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    refresh_ui(&list_box, &content_box, &state);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+H: Flash focused panel
            (gdk4::Key::H, true, true) => {
                let panel_id = {
                    let tm = lock_or_recover(&state.shared.tab_manager);
                    tm.selected().and_then(|ws| ws.focused_panel_id)
                };
                if let Some(panel_id) = panel_id {
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::TriggerFlash { panel_id });
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+B: Toggle sidebar
            (gdk4::Key::B, true, true) => {
                nav_split_view.set_collapsed(!nav_split_view.is_collapsed());
                glib::Propagation::Stop
            }
            // Ctrl+Shift+R: Rename workspace
            (gdk4::Key::R, true, true) => {
                let current_title = {
                    let tm = lock_or_recover(&state.shared.tab_manager);
                    tm.selected().map(|ws| ws.display_title().to_string())
                };
                if let Some(title) = current_title {
                    if let Some(window) = window_weak.upgrade() {
                        show_rename_dialog(&window, &state, &list_box, &content_box, &title);
                    }
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+T: New workspace
            (gdk4::Key::T, true, true) => {
                let workspace = Workspace::new();
                let placement = crate::settings::load().new_workspace_placement;
                lock_or_recover(&state.shared.tab_manager)
                    .add_workspace_with_placement(workspace, placement);
                refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+W: Close workspace
            (gdk4::Key::W, true, true) => {
                let mut tab_manager = lock_or_recover(&state.shared.tab_manager);
                if let Some(index) = tab_manager.selected_index() {
                    tab_manager.remove(index);
                }
                drop(tab_manager);
                refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+Q: Close focused pane
            (gdk4::Key::Q, true, true) => {
                let closed = {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        if let Some(panel_id) = ws.focused_panel_id {
                            // Capture browser URL before closing
                            if let Some(panel) = ws.panels.get(&panel_id) {
                                if panel.panel_type == PanelType::Browser {
                                    if let Some(ref url) = panel.browser_url {
                                        state.shared.push_closed_browser_url(url.clone());
                                    }
                                }
                            }
                            ws.remove_panel(panel_id)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if closed {
                    refresh_ui(&list_box, &content_box, &state);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+D: Split horizontal
            (gdk4::Key::D, true, true) => {
                if let Some(workspace) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                    workspace.split(SplitOrientation::Horizontal, PanelType::Terminal);
                }
                refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+E: Split vertical
            (gdk4::Key::E, true, true) => {
                if let Some(workspace) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                    workspace.split(SplitOrientation::Vertical, PanelType::Terminal);
                }
                refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+O: Open workspace directory in file manager
            (gdk4::Key::O, true, true) => {
                let dir = {
                    let tm = lock_or_recover(&state.shared.tab_manager);
                    tm.selected().map(|ws| ws.current_directory.clone())
                };
                if let Some(dir) = dir {
                    let path = if dir.is_empty() {
                        std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
                    } else {
                        dir
                    };
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&path)
                        .spawn();
                }
                glib::Propagation::Stop
            }
            // Ctrl+O: Open folder as new workspace (folder picker)
            (gdk4::Key::o, true, false) => {
                state
                    .shared
                    .send_ui_event(crate::app::UiEvent::OpenFolderAsWorkspace);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+Y: Reopen last closed browser panel
            (gdk4::Key::Y, true, true) => {
                state
                    .shared
                    .send_ui_event(crate::app::UiEvent::ReopenClosedBrowser);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+M: Enter terminal copy mode
            (gdk4::Key::M, true, true) => {
                let panel_id = {
                    let tm = lock_or_recover(&state.shared.tab_manager);
                    tm.selected().and_then(|ws| ws.focused_panel_id)
                };
                if let Some(panel_id) = panel_id {
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::CopyMode { panel_id });
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+U: Jump to latest unread
            (gdk4::Key::U, true, true) => {
                if select_latest_unread(&state) {
                    refresh_ui(&list_box, &content_box, &state);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+[: Focus previous pane
            (gdk4::Key::bracketleft, true, true) => {
                let changed = {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        if let Some(current) = ws.focused_panel_id {
                            if let Some(prev) = ws.layout.prev_panel_id(current) {
                                ws.focus_panel(prev)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if changed {
                    refresh_ui(&list_box, &content_box, &state);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+]: Focus next pane
            (gdk4::Key::bracketright, true, true) => {
                let changed = {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        if let Some(current) = ws.focused_panel_id {
                            if let Some(next) = ws.layout.next_panel_id(current) {
                                ws.focus_panel(next)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if changed {
                    refresh_ui(&list_box, &content_box, &state);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+PageUp: Move workspace up
            (gdk4::Key::Page_Up, true, true) => {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(idx) = tm.selected_index() {
                    if idx > 0 {
                        tm.move_workspace(idx, idx - 1);
                    }
                }
                drop(tm);
                refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+PageDown: Move workspace down
            (gdk4::Key::Page_Down, true, true) => {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(idx) = tm.selected_index() {
                    if idx + 1 < tm.len() {
                        tm.move_workspace(idx, idx + 1);
                    }
                }
                drop(tm);
                refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Tab: Next workspace
            (gdk4::Key::Tab, true, false) => {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                tm.select_next(true);
                let ws_id = tm.selected_id();
                drop(tm);
                if let Some(workspace_id) = ws_id {
                    mark_workspace_read(&state, workspace_id);
                }
                refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+Tab: Previous workspace
            (gdk4::Key::ISO_Left_Tab, true, true) => {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                tm.select_previous(true);
                let ws_id = tm.selected_id();
                drop(tm);
                if let Some(workspace_id) = ws_id {
                    mark_workspace_read(&state, workspace_id);
                }
                refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+1-9: Select workspace by number
            (keyval, true, false)
                if matches!(
                    keyval,
                    gdk4::Key::_1
                        | gdk4::Key::_2
                        | gdk4::Key::_3
                        | gdk4::Key::_4
                        | gdk4::Key::_5
                        | gdk4::Key::_6
                        | gdk4::Key::_7
                        | gdk4::Key::_8
                        | gdk4::Key::_9
                ) =>
            {
                let index = match keyval {
                    gdk4::Key::_1 => 0,
                    gdk4::Key::_2 => 1,
                    gdk4::Key::_3 => 2,
                    gdk4::Key::_4 => 3,
                    gdk4::Key::_5 => 4,
                    gdk4::Key::_6 => 5,
                    gdk4::Key::_7 => 6,
                    gdk4::Key::_8 => 7,
                    gdk4::Key::_9 => 8,
                    _ => unreachable!(),
                };
                if select_workspace_by_index(&state, index) {
                    refresh_ui(&list_box, &content_box, &state);
                }
                glib::Propagation::Stop
            }
            // Ctrl+K: Clear screen + scrollback
            (gdk4::Key::k, true, false) => {
                let panel_id = {
                    let tm = lock_or_recover(&state.shared.tab_manager);
                    tm.selected().and_then(|ws| ws.focused_panel_id)
                };
                if let Some(panel_id) = panel_id {
                    state
                        .shared
                        .send_ui_event(crate::app::UiEvent::ClearHistory { panel_id });
                }
                glib::Propagation::Stop
            }
            // Ctrl+G: Find next match
            (gdk4::Key::g, true, false) => {
                if search_bar.is_visible() {
                    search_overlay::trigger_find_next(&state, &search_entry);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+G: Find previous match
            (gdk4::Key::G, true, true) => {
                if search_bar.is_visible() {
                    search_overlay::trigger_find_prev(&state, &search_entry);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Equal/Plus: Increase font size (terminal) or zoom (browser)
            (gdk4::Key::equal, true, false) | (gdk4::Key::plus, true, _) => {
                let info = {
                    let tm = lock_or_recover(&state.shared.tab_manager);
                    tm.selected().and_then(|ws| {
                        ws.focused_panel_id.and_then(|pid| {
                            ws.panels.get(&pid).map(|p| (pid, p.panel_type))
                        })
                    })
                };
                if let Some((panel_id, panel_type)) = info {
                    if panel_type == PanelType::Browser {
                        state.shared.send_ui_event(
                            crate::app::UiEvent::BrowserAction {
                                panel_id,
                                action:
                                    crate::ui::browser_panel::BrowserActionKind::ZoomIn,
                            },
                        );
                    } else if let Some(surface) =
                        state.terminal_cache.borrow().get(&panel_id)
                    {
                        surface.binding_action("increase_font_size:1");
                    }
                }
                glib::Propagation::Stop
            }
            // Ctrl+Minus: Decrease font size (terminal) or zoom (browser)
            (gdk4::Key::minus, true, false) => {
                let info = {
                    let tm = lock_or_recover(&state.shared.tab_manager);
                    tm.selected().and_then(|ws| {
                        ws.focused_panel_id.and_then(|pid| {
                            ws.panels.get(&pid).map(|p| (pid, p.panel_type))
                        })
                    })
                };
                if let Some((panel_id, panel_type)) = info {
                    if panel_type == PanelType::Browser {
                        state.shared.send_ui_event(
                            crate::app::UiEvent::BrowserAction {
                                panel_id,
                                action:
                                    crate::ui::browser_panel::BrowserActionKind::ZoomOut,
                            },
                        );
                    } else if let Some(surface) =
                        state.terminal_cache.borrow().get(&panel_id)
                    {
                        surface.binding_action("decrease_font_size:1");
                    }
                }
                glib::Propagation::Stop
            }
            // Ctrl+0: Reset font size (terminal) or zoom (browser)
            (gdk4::Key::_0, true, false) => {
                let info = {
                    let tm = lock_or_recover(&state.shared.tab_manager);
                    tm.selected().and_then(|ws| {
                        ws.focused_panel_id.and_then(|pid| {
                            ws.panels.get(&pid).map(|p| (pid, p.panel_type))
                        })
                    })
                };
                if let Some((panel_id, panel_type)) = info {
                    if panel_type == PanelType::Browser {
                        state.shared.send_ui_event(
                            crate::app::UiEvent::BrowserAction {
                                panel_id,
                                action:
                                    crate::ui::browser_panel::BrowserActionKind::SetZoom {
                                        zoom: 1.0,
                                    },
                            },
                        );
                    } else if let Some(surface) =
                        state.terminal_cache.borrow().get(&panel_id)
                    {
                        surface.binding_action("reset_font_size");
                    }
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });

    window.add_controller(controller);
}

/// Show a dialog to rename a panel tab.
pub fn show_rename_tab_dialog(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    panel_id: uuid::Uuid,
) {
    let current_title = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.find_workspace_with_panel(panel_id)
            .and_then(|ws| ws.panels.get(&panel_id))
            .map(|p| p.display_title().to_string())
            .unwrap_or_default()
    };

    let dialog = gtk4::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Rename Tab")
        .default_width(320)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);

    let entry = gtk4::Entry::new();
    entry.set_text(&current_title);
    entry.set_activates_default(true);
    vbox.append(&entry);

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk4::Align::End);

    let cancel_btn = gtk4::Button::with_label("Cancel");
    let ok_btn = gtk4::Button::with_label("Rename");
    ok_btn.add_css_class("suggested-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&ok_btn);
    vbox.append(&btn_box);

    dialog.set_child(Some(&vbox));

    {
        let dialog = dialog.clone();
        cancel_btn.connect_clicked(move |_| dialog.close());
    }

    {
        let state = state.clone();
        let dialog = dialog.clone();
        let entry = entry.clone();
        ok_btn.connect_clicked(move |_| {
            let new_title = entry.text().to_string();
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                if let Some(panel) = ws.panels.get_mut(&panel_id) {
                    if new_title.is_empty() {
                        panel.custom_title = None;
                    } else {
                        panel.custom_title = Some(new_title);
                    }
                }
            }
            drop(tm);
            state.shared.notify_ui_refresh();
            dialog.close();
        });
    }

    // Enter activates OK
    {
        let ok_btn = ok_btn.clone();
        entry.connect_activate(move |_| {
            ok_btn.emit_clicked();
        });
    }

    // Escape closes
    let key_controller = gtk4::EventControllerKey::new();
    {
        let dialog = dialog.clone();
        key_controller.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk4::Key::Escape {
                dialog.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    dialog.add_controller(key_controller);

    dialog.present();
    entry.grab_focus();
    entry.select_region(0, -1);
}

fn install_css() {
    // Ensure Adwaita legacy icons (terminal, etc.) resolve on all systems,
    // and add bundled cmux icons (globe, etc.).
    if let Some(display) = gdk4::Display::default() {
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
        ",
    );

    if let Some(display) = gdk4::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Detect git branch and dirty state from a directory path.
fn detect_git_branch(directory: &str) -> Option<GitBranch> {
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
