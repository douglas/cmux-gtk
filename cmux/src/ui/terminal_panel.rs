//! Terminal panel — wraps a GhosttyGlSurface in a panel container.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{lock_or_recover, AppState};
use crate::model::panel::{Panel, PanelType};

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
        PanelType::Browser => create_browser_widget(panel, is_attention_source),
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

    let gl_surface = state.terminal_surface_for(panel.id, panel.directory.as_deref());
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

    // Click-to-focus: when user clicks this pane, focus it in the model
    // and trigger a UI refresh so the active indicator moves.
    let click = gtk4::GestureClick::new();
    click.set_button(1); // Left click
    click.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let state = Rc::clone(state);
        let panel_id = panel.id;
        click.connect_pressed(move |gesture, _n, _x, _y| {
            // Don't claim the event — let it propagate to the terminal
            gesture.set_state(gtk4::EventSequenceState::None);

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
    overlay.add_controller(click);

    overlay.upcast()
}

/// Create a browser panel with WebKitWebView.
fn create_browser_widget(panel: &Panel, is_attention_source: bool) -> gtk4::Widget {
    super::browser_panel::create_browser_widget(
        panel.id,
        panel.directory.as_deref(), // Reuse directory field as initial URL for browser panels
        is_attention_source,
    )
}
