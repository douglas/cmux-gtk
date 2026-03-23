//! Keyboard shortcut handling for the main application window.

use std::rc::Rc;

use std::cell::Cell;

use gtk4::prelude::*;
use libadwaita as adw;

use crate::app::{lock_or_recover, AppState};
use crate::model::panel::SplitOrientation;
use crate::model::{PanelType, Workspace};
use crate::ui::{notifications_panel, search_overlay};

#[allow(clippy::too_many_arguments)]
pub(super) fn setup_shortcuts(
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
                    super::refresh_ui(&list_box, &content_box, &state);
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
                        workspace.split(SplitOrientation::Horizontal, PanelType::Browser);
                    }
                    super::refresh_ui(&list_box, &content_box, &state);
                    return glib::Propagation::Stop;
                }
                // Ctrl+Alt+E: Split browser vertical
                gdk4::Key::e => {
                    if let Some(workspace) =
                        lock_or_recover(&state.shared.tab_manager).selected_mut()
                    {
                        workspace.split(SplitOrientation::Vertical, PanelType::Browser);
                    }
                    super::refresh_ui(&list_box, &content_box, &state);
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
                        let pane_ids = ws.layout.find_pane_with_panel_readonly(panel_id);
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
                super::refresh_ui(&list_box, &content_box, &state);
            }
            return glib::Propagation::Stop;
        }

        match (keyval, ctrl, shift) {
            // Ctrl+Comma: Settings
            (gdk4::Key::comma, true, false) => {
                if let Some(window) = window_weak.upgrade() {
                    let lb = list_box.clone();
                    let cb = content_box.clone();
                    let st = Rc::clone(&state);
                    crate::ui::settings::show_settings(&window, move || {
                        super::refresh_ui(&lb, &cb, &st);
                    });
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+Comma: Reload ghostty config
            (gdk4::Key::comma, true, true) => {
                state
                    .shared
                    .send_ui_event(crate::app::UiEvent::ReloadConfig);
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
            // Ctrl+E: Use selection for Find — reads primary selection into find bar
            (gdk4::Key::e, true, false) => {
                // On Linux, the primary clipboard holds the current selection
                let search_bar_c = search_bar.clone();
                let search_entry_c = search_entry.clone();
                if let Some(display) = gdk4::Display::default() {
                    let primary = display.primary_clipboard();
                    primary.read_text_async(gio::Cancellable::NONE, move |result| {
                        if let Ok(Some(text)) = result {
                            let text = text.to_string();
                            if !text.is_empty() {
                                if !search_bar_c.is_visible() {
                                    search_bar_c.set_visible(true);
                                }
                                search_entry_c.set_text(&text);
                                search_entry_c.grab_focus();
                            }
                        }
                    });
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
            // Ctrl+P: All-surfaces search
            (gdk4::Key::p, true, false) => {
                if let Some(window) = window_weak.upgrade() {
                    crate::ui::all_surfaces_search::show_all_surfaces_search(&window, &state);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+P: Command palette
            (gdk4::Key::P, true, true) => {
                if let Some(window) = window_weak.upgrade() {
                    let lb = list_box.clone();
                    let cb = content_box.clone();
                    let st = state.clone();
                    let on_refresh = Rc::new(move || super::refresh_ui(&lb, &cb, &st));
                    crate::ui::command_palette::show_command_palette(&window, &state, on_refresh);
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
                    super::refresh_ui(&list_box, &content_box, &state);
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
                        super::dialogs::show_rename_dialog(
                            &window,
                            &state,
                            &list_box,
                            &content_box,
                            &title,
                        );
                    }
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+T: New workspace
            (gdk4::Key::T, true, true) => {
                let mut workspace = Workspace::new();
                workspace.window_id = uuid::Uuid::parse_str(
                    &window_weak
                        .upgrade()
                        .map(|w| w.widget_name().to_string())
                        .unwrap_or_default(),
                )
                .ok();
                let placement = crate::settings::load().new_workspace_placement;
                lock_or_recover(&state.shared.tab_manager)
                    .add_workspace_with_placement(workspace, placement);
                super::refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+N: New window
            (gdk4::Key::N, true, true) => {
                state
                    .shared
                    .send_ui_event(crate::app::UiEvent::CreateWindow);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+W: Close workspace
            (gdk4::Key::W, true, true) => {
                let mut tab_manager = lock_or_recover(&state.shared.tab_manager);
                if let Some(index) = tab_manager.selected_index() {
                    tab_manager.remove(index);
                }
                drop(tab_manager);
                super::refresh_ui(&list_box, &content_box, &state);
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
                    super::refresh_ui(&list_box, &content_box, &state);
                }
                glib::Propagation::Stop
            }
            // Ctrl+Shift+D: Split horizontal
            (gdk4::Key::D, true, true) => {
                if let Some(workspace) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                    workspace.split(SplitOrientation::Horizontal, PanelType::Terminal);
                }
                super::refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+E: Split vertical
            (gdk4::Key::E, true, true) => {
                if let Some(workspace) = lock_or_recover(&state.shared.tab_manager).selected_mut() {
                    workspace.split(SplitOrientation::Vertical, PanelType::Terminal);
                }
                super::refresh_ui(&list_box, &content_box, &state);
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
                    let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
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
                if super::event_handler::select_latest_unread(&state) {
                    super::refresh_ui(&list_box, &content_box, &state);
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
                    super::refresh_ui(&list_box, &content_box, &state);
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
                    super::refresh_ui(&list_box, &content_box, &state);
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
                super::refresh_ui(&list_box, &content_box, &state);
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
                super::refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Tab: Next workspace
            (gdk4::Key::Tab, true, false) => {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                tm.select_next(true);
                let ws_id = tm.selected_id();
                drop(tm);
                if let Some(workspace_id) = ws_id {
                    super::event_handler::mark_workspace_read(&state, workspace_id);
                }
                super::refresh_ui(&list_box, &content_box, &state);
                glib::Propagation::Stop
            }
            // Ctrl+Shift+Tab: Previous workspace
            (gdk4::Key::ISO_Left_Tab, true, true) => {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                tm.select_previous(true);
                let ws_id = tm.selected_id();
                drop(tm);
                if let Some(workspace_id) = ws_id {
                    super::event_handler::mark_workspace_read(&state, workspace_id);
                }
                super::refresh_ui(&list_box, &content_box, &state);
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
                if super::event_handler::select_workspace_by_index(&state, index) {
                    super::refresh_ui(&list_box, &content_box, &state);
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
                        ws.focused_panel_id
                            .and_then(|pid| ws.panels.get(&pid).map(|p| (pid, p.panel_type)))
                    })
                };
                if let Some((panel_id, panel_type)) = info {
                    if panel_type == PanelType::Browser {
                        state
                            .shared
                            .send_ui_event(crate::app::UiEvent::BrowserAction {
                                panel_id,
                                action: crate::ui::browser_panel::BrowserActionKind::ZoomIn,
                            });
                    } else if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
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
                        ws.focused_panel_id
                            .and_then(|pid| ws.panels.get(&pid).map(|p| (pid, p.panel_type)))
                    })
                };
                if let Some((panel_id, panel_type)) = info {
                    if panel_type == PanelType::Browser {
                        state
                            .shared
                            .send_ui_event(crate::app::UiEvent::BrowserAction {
                                panel_id,
                                action: crate::ui::browser_panel::BrowserActionKind::ZoomOut,
                            });
                    } else if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
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
                        ws.focused_panel_id
                            .and_then(|pid| ws.panels.get(&pid).map(|p| (pid, p.panel_type)))
                    })
                };
                if let Some((panel_id, panel_type)) = info {
                    if panel_type == PanelType::Browser {
                        state
                            .shared
                            .send_ui_event(crate::app::UiEvent::BrowserAction {
                                panel_id,
                                action: crate::ui::browser_panel::BrowserActionKind::SetZoom {
                                    zoom: 1.0,
                                },
                            });
                    } else if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
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
