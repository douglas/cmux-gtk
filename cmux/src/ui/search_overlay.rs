//! Terminal find overlay — search bar on top of the terminal surface.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;

/// Shared search state updated by ghostty callbacks and read by the overlay.
#[derive(Debug, Default)]
pub struct SearchState {
    pub total: Cell<isize>,
    pub selected: Cell<isize>,
    pub visible: Cell<bool>,
}

/// Widgets for the search overlay.
pub struct SearchOverlay {
    pub overlay: gtk4::Overlay,
    pub search_bar: gtk4::Box,
    pub entry: gtk4::SearchEntry,
    pub count_label: gtk4::Label,
    pub state: Rc<SearchState>,
}

/// Create a search overlay that wraps the given child widget.
pub fn create_search_overlay(
    child: &gtk4::Widget,
    app_state: &Rc<AppState>,
) -> SearchOverlay {
    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(child));
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    let search_state = Rc::new(SearchState::default());

    // Search bar container — anchored to top-right
    let search_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    search_bar.add_css_class("search-overlay");
    search_bar.set_halign(gtk4::Align::End);
    search_bar.set_valign(gtk4::Align::Start);
    search_bar.set_margin_top(8);
    search_bar.set_margin_end(16);
    search_bar.set_visible(false);

    let entry = gtk4::SearchEntry::new();
    entry.set_placeholder_text(Some("Find..."));
    entry.set_width_chars(24);
    search_bar.append(&entry);

    let count_label = gtk4::Label::new(Some(""));
    count_label.add_css_class("caption");
    count_label.add_css_class("dim-label");
    count_label.set_margin_start(4);
    count_label.set_margin_end(4);
    search_bar.append(&count_label);

    let prev_btn = gtk4::Button::from_icon_name("go-up-symbolic");
    prev_btn.set_tooltip_text(Some("Previous (Shift+Enter)"));
    prev_btn.add_css_class("flat");
    search_bar.append(&prev_btn);

    let next_btn = gtk4::Button::from_icon_name("go-down-symbolic");
    next_btn.set_tooltip_text(Some("Next (Enter)"));
    next_btn.add_css_class("flat");
    search_bar.append(&next_btn);

    let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
    close_btn.set_tooltip_text(Some("Close (Escape)"));
    close_btn.add_css_class("flat");
    search_bar.append(&close_btn);

    overlay.add_overlay(&search_bar);

    // Wire up search-as-you-type
    let needle: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    {
        let app_state = app_state.clone();
        let needle = needle.clone();
        entry.connect_search_changed(move |entry| {
            let text = entry.text().to_string();
            *needle.borrow_mut() = text.clone();
            if text.is_empty() {
                do_search_action(&app_state, "close_search");
            } else {
                // Ghostty binding action: "search:fwd:NEEDLE"
                // We use the binding_action on the focused surface
                do_search_needle(&app_state, &text, true);
            }
        });
    }

    // Enter = next match, Shift+Enter = previous
    {
        let app_state = app_state.clone();
        let needle = needle.clone();
        let search_bar_ref = search_bar.clone();
        let entry_for_keypress = entry.clone();
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_ctrl, keyval, _keycode, modifier| {
            let shift = modifier.contains(gdk4::ModifierType::SHIFT_MASK);
            match keyval {
                gdk4::Key::Escape => {
                    do_search_action(&app_state, "close_search");
                    search_bar_ref.set_visible(false);
                    // Return focus to terminal
                    if let Some(parent) = search_bar_ref.parent() {
                        if let Ok(overlay) = parent.downcast::<gtk4::Overlay>() {
                            if let Some(child) = overlay.child() {
                                child.grab_focus();
                            }
                        }
                    }
                    glib::Propagation::Stop
                }
                gdk4::Key::Return | gdk4::Key::KP_Enter => {
                    let text = needle.borrow().clone();
                    if !text.is_empty() {
                        do_search_needle(&app_state, &text, !shift);
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        entry_for_keypress.add_controller(key_controller);
    }

    // Button handlers
    {
        let app_state = app_state.clone();
        let needle = needle.clone();
        next_btn.connect_clicked(move |_| {
            let text = needle.borrow().clone();
            if !text.is_empty() {
                do_search_needle(&app_state, &text, true);
            }
        });
    }
    {
        let app_state = app_state.clone();
        let needle = needle.clone();
        prev_btn.connect_clicked(move |_| {
            let text = needle.borrow().clone();
            if !text.is_empty() {
                do_search_needle(&app_state, &text, false);
            }
        });
    }
    {
        let app_state = app_state.clone();
        let bar = search_bar.clone();
        close_btn.connect_clicked(move |_| {
            do_search_action(&app_state, "close_search");
            bar.set_visible(false);
            if let Some(parent) = bar.parent() {
                if let Ok(overlay) = parent.downcast::<gtk4::Overlay>() {
                    if let Some(child) = overlay.child() {
                        child.grab_focus();
                    }
                }
            }
        });
    }

    SearchOverlay {
        overlay,
        search_bar,
        entry,
        count_label,
        state: search_state,
    }
}

/// Execute a binding action on the currently focused terminal surface.
fn do_search_action(state: &Rc<AppState>, action: &str) {
    let panel_id = {
        let tm = crate::app::lock_or_recover(&state.shared.tab_manager);
        tm.selected().and_then(|ws| ws.focused_panel_id)
    };
    if let Some(panel_id) = panel_id {
        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
            surface.binding_action(action);
        }
    }
}

/// Send a search needle to the focused surface.
fn do_search_needle(state: &Rc<AppState>, needle: &str, forward: bool) {
    let direction = if forward { "fwd" } else { "bwd" };
    let action = format!("search:{direction}:{needle}");
    let panel_id = {
        let tm = crate::app::lock_or_recover(&state.shared.tab_manager);
        tm.selected().and_then(|ws| ws.focused_panel_id)
    };
    if let Some(panel_id) = panel_id {
        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
            surface.binding_action(&action);
        }
    }
}

/// Update the search overlay match count from ghostty callbacks.
pub fn update_search_count(overlay: &SearchOverlay) {
    let total = overlay.state.total.get();
    let selected = overlay.state.selected.get();

    if total < 0 {
        overlay.count_label.set_text("");
    } else if total == 0 {
        overlay.count_label.set_text("No matches");
    } else {
        overlay
            .count_label
            .set_text(&format!("{}/{}", selected + 1, total));
    }
}
