//! Terminal panel — wraps a GhosttyGlSurface in a panel container.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use uuid::Uuid;

use crate::app::{lock_or_recover, AppState};
use crate::model::panel::{Panel, PanelType};

// ── Vim badge registry ───────────────────────────────────────────────
// Thread-local map: panel_id → vim badge label widget.
// Used by window.rs CopyMode handler to show/hide the badge.
thread_local! {
    static VIM_BADGES: RefCell<HashMap<Uuid, gtk4::Label>> = RefCell::new(HashMap::new());
}

fn register_vim_badge(panel_id: Uuid, badge: gtk4::Label) {
    VIM_BADGES.with(|map| {
        map.borrow_mut().insert(panel_id, badge);
    });
}

/// Show the vim copy-mode badge for a panel.
pub fn show_vim_badge(panel_id: Uuid) {
    VIM_BADGES.with(|map| {
        if let Some(badge) = map.borrow().get(&panel_id) {
            badge.set_visible(true);
        }
    });
}

/// Hide the vim copy-mode badge for a panel.
pub fn hide_vim_badge(panel_id: Uuid) {
    VIM_BADGES.with(|map| {
        if let Some(badge) = map.borrow().get(&panel_id) {
            badge.set_visible(false);
        }
    });
}

/// Remove a vim badge (cleanup when panel is destroyed).
#[allow(dead_code)]
pub fn unregister_vim_badge(panel_id: &Uuid) {
    VIM_BADGES.with(|map| {
        map.borrow_mut().remove(panel_id);
    });
}

/// Create a GTK widget for a panel.
pub fn create_panel_widget(
    panel: &Panel,
    is_attention_source: bool,
    is_focused: bool,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    match panel.panel_type {
        PanelType::Terminal => {
            create_terminal_widget(panel, is_attention_source, is_focused, state)
        }
        PanelType::Browser => create_browser_widget(panel, is_attention_source, state),
        PanelType::Markdown => create_markdown_widget(panel, is_attention_source),
    }
}

/// Create a terminal panel widget backed by GhosttyGlSurface.
fn create_terminal_widget(
    panel: &Panel,
    is_attention_source: bool,
    is_focused: bool,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    // Overlay allows stacking the inactive dim on top of the terminal
    let overlay = gtk4::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    if is_attention_source {
        container.add_css_class("attention-panel");
    }
    if is_focused {
        container.add_css_class("focused-panel");
    }

    let gl_surface = state.terminal_surface_for(
        panel.id,
        panel.directory.as_deref(),
        panel.command.as_deref(),
    );
    {
        let state = Rc::clone(state);
        let panel_id = panel.id;
        gl_surface.set_close_handler(move |process_alive| {
            let _ = state.close_panel(panel_id, process_alive);
        });
    }
    if let Some(parent) = gl_surface.parent() {
        if let Ok(parent_box) = parent.downcast::<gtk4::Box>() {
            parent_box.remove(&gl_surface);
        }
    }

    container.append(&gl_surface);

    // Force a resize after reparenting a cached surface so the GL area
    // picks up its new (possibly smaller) allocation from the GtkPaned.
    gl_surface.queue_resize();

    // Store the panel ID for later lookup
    container.set_widget_name(&panel.id.to_string());

    overlay.set_child(Some(&container));

    // Inactive pane overlay — semi-transparent darken when not focused.
    // Only add for multi-pane layouts (single-pane is always focused).
    if !is_focused {
        let inactive_overlay = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        inactive_overlay.set_hexpand(true);
        inactive_overlay.set_vexpand(true);
        inactive_overlay.add_css_class("inactive-pane-overlay");
        // The overlay must not intercept clicks — pass them through.
        inactive_overlay.set_can_target(false);
        overlay.add_overlay(&inactive_overlay);
    }

    // File drop: drop files onto the terminal to paste their paths
    let file_drop = gtk4::DropTarget::new(gdk4::FileList::static_type(), gdk4::DragAction::COPY);
    {
        let state = Rc::clone(state);
        let panel_id = panel.id;
        file_drop.connect_drop(move |_target, value, _x, _y| {
            let Ok(file_list) = value.get::<gdk4::FileList>() else {
                return false;
            };
            let paths: Vec<String> = file_list
                .files()
                .iter()
                .filter_map(|f| f.path())
                .map(|p| {
                    let s = p.to_string_lossy().to_string();
                    // Shell-quote paths with spaces
                    if s.contains(' ') {
                        format!("'{s}'")
                    } else {
                        s
                    }
                })
                .collect();
            if paths.is_empty() {
                return false;
            }
            let text = paths.join(" ");
            state.send_input_to_panel(panel_id, &text);
            true
        });
    }
    overlay.add_controller(file_drop);

    // "vim" copy-mode badge — top-right corner, initially hidden.
    // Made visible when copy mode is activated via UiEvent::CopyMode.
    let vim_badge = gtk4::Label::new(Some("vim"));
    vim_badge.add_css_class("vim-badge");
    vim_badge.set_halign(gtk4::Align::End);
    vim_badge.set_valign(gtk4::Align::Start);
    vim_badge.set_margin_top(6);
    vim_badge.set_margin_end(6);
    vim_badge.set_can_target(false);
    vim_badge.set_visible(false);
    overlay.add_overlay(&vim_badge);

    // Register badge for this panel so window.rs can show/hide it
    {
        let badge = vim_badge.clone();
        register_vim_badge(panel.id, badge);
    }

    // Hover-to-focus: when focus_follows_mouse is enabled, focus on mouse enter.
    let motion = gtk4::EventControllerMotion::new();
    {
        let state = Rc::clone(state);
        let panel_id = panel.id;
        motion.connect_enter(move |_controller, _x, _y| {
            if !crate::settings::load().focus_follows_mouse {
                return;
            }
            let needs_refresh = {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                    if ws.focused_panel_id != Some(panel_id) {
                        ws.focus_panel(panel_id);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if needs_refresh {
                state.shared.notify_ui_refresh();
            }
        });
    }
    overlay.add_controller(motion);

    // Click-to-focus: when user clicks this pane, focus it in the model
    // and trigger a UI refresh so the active indicator moves.
    let click = gtk4::GestureClick::new();
    click.set_button(1); // Left click
    click.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let state = Rc::clone(state);
        let panel_id = panel.id;
        click.connect_pressed(move |gesture, _n, _x, _y| {
            let was_unfocused = {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                    if ws.focused_panel_id != Some(panel_id) {
                        ws.focus_panel(panel_id);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            // When first_click_focus is enabled and this pane was
            // unfocused, claim the event so the click only focuses
            // without passing through to the terminal.
            if was_unfocused && crate::settings::load().first_click_focus {
                gesture.set_state(gtk4::EventSequenceState::Claimed);
            } else {
                gesture.set_state(gtk4::EventSequenceState::None);
            }

            if was_unfocused {
                state.shared.notify_ui_refresh();
            }
        });
    }
    overlay.add_controller(click);

    overlay.upcast()
}

/// Create a browser panel with WebKitWebView.
fn create_browser_widget(
    panel: &Panel,
    is_attention_source: bool,
    state: &std::rc::Rc<crate::app::AppState>,
) -> gtk4::Widget {
    super::browser_panel::create_browser_widget(
        panel.id,
        panel.directory.as_deref(), // Reuse directory field as initial URL for browser panels
        is_attention_source,
        panel.pending_zoom,
        Some(state.shared.clone()),
    )
}

/// Create a markdown panel with WebView rendering.
fn create_markdown_widget(panel: &Panel, is_attention_source: bool) -> gtk4::Widget {
    super::markdown_panel::create_markdown_widget(
        panel.id,
        panel.markdown_file.as_deref(),
        is_attention_source,
    )
}
