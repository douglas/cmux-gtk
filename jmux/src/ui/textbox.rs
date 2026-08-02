//! TextBox — a prompt-composition surface below a terminal.
//!
//! A multi-line input where you compose a prompt and send it to the terminal in
//! one shot (handy for multi-line agent prompts). Enter sends; Shift+Enter
//! inserts a newline; Escape returns focus to the terminal. Toggled per the
//! `show_textbox_on_new_terminals` / `focus_textbox_on_new_terminals` settings.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use uuid::Uuid;

use crate::app::AppState;

thread_local! {
    /// panel_id → TextBox TextView, so a shortcut can focus it.
    static TEXTBOXES: RefCell<HashMap<Uuid, gtk4::TextView>> = RefCell::new(HashMap::new());
}

/// Focus the TextBox for `panel_id` if one exists. Returns true if focused.
pub fn focus_textbox(panel_id: Uuid) -> bool {
    TEXTBOXES.with(|m| {
        m.borrow()
            .get(&panel_id)
            .map(|tv| {
                tv.grab_focus();
                true
            })
            .unwrap_or(false)
    })
}

/// Build the TextBox composer for `panel_id`.
pub fn create_textbox(panel_id: Uuid, state: &Rc<AppState>) -> gtk4::Widget {
    let settings = crate::settings::load();

    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    container.add_css_class("textbox-composer");
    container.set_margin_start(4);
    container.set_margin_end(4);
    container.set_margin_top(2);
    container.set_margin_bottom(4);

    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
    text_view.set_hexpand(true);
    text_view.set_left_margin(6);
    text_view.set_right_margin(6);
    text_view.set_top_margin(4);
    text_view.set_bottom_margin(4);
    text_view.add_css_class("textbox-input");

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_hexpand(true);
    // Cap the visible height to `textbox_max_lines` lines (~20px each).
    let max_lines = settings.textbox_max_lines.clamp(1, 40) as i32;
    scrolled.set_max_content_height(max_lines * 20);
    scrolled.set_propagate_natural_height(true);
    scrolled.set_child(Some(&text_view));
    container.append(&scrolled);

    let send_btn = gtk4::Button::from_icon_name("document-send-symbolic");
    send_btn.add_css_class("flat");
    send_btn.set_valign(gtk4::Align::End);
    send_btn.set_tooltip_text(Some("Send to terminal (Enter)"));
    container.append(&send_btn);

    // Send helper: paste the composed text + Enter into the terminal, then clear.
    // WEAK text_view: a copy of this closure is held by the key controller
    // ON the text view — a strong capture is a cycle.
    let send = {
        let state = Rc::clone(state);
        let text_view = text_view.downgrade();
        move || {
            let Some(text_view) = text_view.upgrade() else {
                return;
            };
            let buffer = text_view.buffer();
            let (start, end) = buffer.bounds();
            let text = buffer.text(&start, &end, false).to_string();
            if text.trim().is_empty() {
                return;
            }
            if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                surface.send_text(&format!("{text}\n"));
            }
            buffer.set_text("");
            // Return focus to the terminal after sending.
            if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                surface.grab_focus();
            }
        }
    };

    {
        let send = send.clone();
        send_btn.connect_clicked(move |_| send());
    }

    // Enter sends; Shift+Enter inserts a newline; Escape focuses the terminal.
    let key = gtk4::EventControllerKey::new();
    {
        let send = send.clone();
        let state = Rc::clone(state);
        key.connect_key_pressed(move |_c, keyval, _code, modifier| {
            let shift = modifier.contains(gdk4::ModifierType::SHIFT_MASK);
            match keyval {
                gdk4::Key::Return | gdk4::Key::KP_Enter if !shift => {
                    send();
                    glib::Propagation::Stop
                }
                gdk4::Key::Escape => {
                    if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                        surface.grab_focus();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }
    text_view.add_controller(key);

    TEXTBOXES.with(|m| {
        m.borrow_mut().insert(panel_id, text_view.clone());
    });

    if settings.focus_textbox_on_new_terminals {
        glib::idle_add_local_once(move || {
            text_view.grab_focus();
        });
    }

    container.upcast()
}

/// Drop a closed panel's composer entry (called from `close_panel`).
pub fn unregister(panel_id: &Uuid) {
    TEXTBOXES.with(|m| {
        m.borrow_mut().remove(panel_id);
    });
}
