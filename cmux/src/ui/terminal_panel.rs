//! Terminal panel — wraps a GhosttyGlSurface in a panel container.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::model::panel::{Panel, PanelType};

/// Create a GTK widget for a panel.
pub fn create_panel_widget(
    panel: &Panel,
    is_attention_source: bool,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    match panel.panel_type {
        PanelType::Terminal => create_terminal_widget(panel, is_attention_source, state),
        PanelType::Browser => create_browser_widget(panel, is_attention_source),
    }
}

/// Create a terminal panel widget backed by GhosttyGlSurface.
fn create_terminal_widget(
    panel: &Panel,
    is_attention_source: bool,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.add_css_class("panel-shell");
    if is_attention_source {
        container.add_css_class("attention-panel");
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

    container.upcast()
}

/// Create a browser panel with WebKitWebView.
fn create_browser_widget(panel: &Panel, is_attention_source: bool) -> gtk4::Widget {
    super::browser_panel::create_browser_widget(
        panel.id,
        panel.directory.as_deref(), // Reuse directory field as initial URL for browser panels
        is_attention_source,
    )
}
