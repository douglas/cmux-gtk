//! Main application window using AdwNavigationSplitView.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::sync::mpsc::UnboundedReceiver;

use std::cell::Cell;

use crate::app::{lock_or_recover, AppState, UiEvent};
use crate::model::panel::SplitOrientation;
use crate::model::{PanelType, Workspace};
use crate::ui::{notifications_panel, search_overlay, sidebar, split_view};

/// Create the main application window.
pub fn create_window(
    app: &adw::Application,
    state: &Rc<AppState>,
    ui_events: UnboundedReceiver<UiEvent>,
) -> adw::ApplicationWindow {
    install_css();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("cmux")
        .default_width(1280)
        .default_height(860)
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
    bind_shared_state_updates(&list_box, &content_box, &window, state, ui_events);

    let header = adw::HeaderBar::new();

    let new_ws_btn = gtk4::Button::from_icon_name("tab-new-symbolic");
    new_ws_btn.set_tooltip_text(Some("New Workspace"));
    {
        let state = state.clone();
        let list_box = list_box.clone();
        let content_box = content_box.clone();
        new_ws_btn.connect_clicked(move |_| {
            let workspace = Workspace::new();
            lock_or_recover(&state.shared.tab_manager).add_workspace(workspace);
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
            let widget = if let Some(zoomed_id) = zoomed_panel_id {
                split_view::build_zoomed(zoomed_id, &panels, &state)
            } else {
                split_view::build_layout(
                    id,
                    &layout,
                    &panels,
                    attention_panel_id,
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
            let title = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.selected().map(|ws| {
                    let title = ws.display_title();
                    let dir = crate::ui::sidebar::compact_path(&ws.current_directory);
                    format!("{title} — {dir} — cmux")
                })
            };
            if let Some(title) = title {
                window.set_title(Some(&title));
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

fn bind_shared_state_updates(
    list_box: &gtk4::ListBox,
    content_box: &gtk4::Box,
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    mut ui_events: UnboundedReceiver<UiEvent>,
) {
    let state = state.clone();
    let list_box = list_box.clone();
    let content_box = content_box.clone();
    let window_weak = window.downgrade();

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
                    // Search events are handled but we don't have the search
                    // overlay widget refs here. The search overlay reads state
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

        match (keyval, ctrl, shift) {
            // Ctrl+Comma: Settings
            (gdk4::Key::comma, true, false) => {
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
                lock_or_recover(&state.shared.tab_manager).add_workspace(workspace);
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
                            let removed = ws.remove_panel(panel_id);
                            if removed && ws.is_empty() {
                                let ws_id = ws.id;
                                tm.remove_by_id(ws_id);
                            }
                            removed
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
            _ => glib::Propagation::Proceed,
        }
    });

    window.add_controller(controller);
}

fn install_css() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(
        "
        /* ── Workspace rows ── */
        .workspace-row {
            border-radius: 10px;
        }

        .workspace-row-colored {
            border-radius: 10px;
            border-left: 4px solid transparent;
            padding-left: 0px;
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

        /* ── Progress bar ── */
        .sidebar-progress {
            min-height: 4px;
            border-radius: 2px;
        }

        .sidebar-progress trough {
            min-height: 4px;
            border-radius: 2px;
        }

        .sidebar-progress progress {
            min-height: 4px;
            border-radius: 2px;
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
            color: #3584e4;
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
            border: 1px solid rgba(127, 127, 127, 0.18);
            border-radius: 10px;
            padding: 3px;
        }

        .attention-panel {
            border: 2px solid #3584e4;
            background-color: rgba(53, 132, 228, 0.08);
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
