//! Omnibar — URL entry with history autocomplete dropdown.
//!
//! Replaces the plain `gtk4::Entry` in the browser nav bar with a rich
//! autocomplete widget backed by `browser_history::search`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gdk4;
use glib;
use gtk4::prelude::*;

use crate::browser_history;
use crate::settings;

/// Number of suggestion rows shown in the dropdown.
const MAX_SUGGESTIONS: usize = 8;

/// Debounce delay for history queries (ms).
const DEBOUNCE_MS: u64 = 120;

/// Create an omnibar widget. Returns `(container, entry)` where `container` holds the entry
/// and the popover dropdown, and `entry` is the text entry for external wiring.
pub fn build_omnibar(
    initial_url: Option<&str>,
    search_engine: settings::SearchEngine,
) -> (gtk4::Box, gtk4::Entry) {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);

    let entry = gtk4::Entry::new();
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some("Search or enter URL..."));
    entry.add_css_class("browser-url-entry");
    if let Some(url) = initial_url {
        entry.set_text(url);
    }
    container.append(&entry);

    // Suggestion list
    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.add_css_class("omnibar-suggestions");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_max_content_height(400);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&list_box));

    let popover = gtk4::Popover::new();
    popover.set_child(Some(&scroll));
    popover.set_autohide(false);
    popover.set_has_arrow(false);
    popover.add_css_class("omnibar-popover");
    popover.set_parent(&entry);
    popover.set_position(gtk4::PositionType::Bottom);

    // State for keyboard navigation
    let selected_idx: Rc<Cell<i32>> = Rc::new(Cell::new(-1));
    let suggestion_count: Rc<Cell<i32>> = Rc::new(Cell::new(0));
    let suggestions: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    // Debounce via generation counter — each keystroke increments; the timer
    // only fires if the generation hasn't changed since it was scheduled.
    let debounce_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    // Suppress suggestion popup when we're programmatically updating the entry text
    let suppress_suggestions: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    let engine = search_engine;

    let sug_state = Rc::new(SuggestionState {
        selected_idx: selected_idx.clone(),
        suggestion_count: suggestion_count.clone(),
        suggestions: suggestions.clone(),
    });

    // ── Populate suggestions on text change (debounced) ──
    {
        let list_box = list_box.clone();
        let popover = popover.clone();
        let debounce_gen = debounce_gen.clone();
        let suppress = suppress_suggestions.clone();
        let sug_state = sug_state.clone();

        entry.connect_changed(move |entry| {
            if suppress.get() {
                return;
            }

            // Bump generation to invalidate any pending debounce timer
            let gen = debounce_gen.get().wrapping_add(1);
            debounce_gen.set(gen);

            let text = entry.text().to_string();
            let list_box = list_box.clone();
            let popover = popover.clone();
            let debounce_gen = debounce_gen.clone();
            let entry_widget = entry.clone();
            let sug_state = sug_state.clone();

            glib::timeout_add_local_once(
                std::time::Duration::from_millis(DEBOUNCE_MS),
                move || {
                    // Only fire if no newer keystroke has occurred
                    if debounce_gen.get() != gen {
                        return;
                    }
                    populate_suggestions(
                        &text,
                        engine,
                        &list_box,
                        &popover,
                        &entry_widget,
                        &sug_state,
                    );
                },
            );
        });
    }

    // ── Keyboard navigation: Up/Down/Escape/Enter ──
    {
        let popover = popover.clone();
        let selected_idx = selected_idx.clone();
        let suggestion_count = suggestion_count.clone();
        let suggestions = suggestions.clone();
        let list_box = list_box.clone();
        let suppress = suppress_suggestions.clone();
        let entry_for_keys = entry.clone();

        let key_controller = gtk4::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, keyval, _, _modifier| {
            match keyval {
                gdk4::Key::Down => {
                    let count = suggestion_count.get();
                    if count > 0 && popover.is_visible() {
                        let new_idx = (selected_idx.get() + 1).min(count - 1);
                        selected_idx.set(new_idx);
                        if let Some(row) = list_box.row_at_index(new_idx) {
                            list_box.select_row(Some(&row));
                        }
                        return glib::Propagation::Stop;
                    }
                    glib::Propagation::Proceed
                }
                gdk4::Key::Up => {
                    if suggestion_count.get() > 0 && popover.is_visible() {
                        let new_idx = (selected_idx.get() - 1).max(-1);
                        selected_idx.set(new_idx);
                        if new_idx >= 0 {
                            if let Some(row) = list_box.row_at_index(new_idx) {
                                list_box.select_row(Some(&row));
                            }
                        } else {
                            list_box.unselect_all();
                        }
                        return glib::Propagation::Stop;
                    }
                    glib::Propagation::Proceed
                }
                gdk4::Key::Escape => {
                    if popover.is_visible() {
                        popover.popdown();
                        selected_idx.set(-1);
                        list_box.unselect_all();
                        return glib::Propagation::Stop;
                    }
                    glib::Propagation::Proceed
                }
                gdk4::Key::Return | gdk4::Key::KP_Enter => {
                    let idx = selected_idx.get();
                    if idx >= 0 && popover.is_visible() {
                        let url = suggestions.borrow().get(idx as usize).cloned();
                        if let Some(url) = url {
                            suppress.set(true);
                            popover.popdown();
                            selected_idx.set(-1);
                            entry_for_keys.set_text(&url);
                            suppress.set(false);
                            entry_for_keys.emit_activate();
                            return glib::Propagation::Stop;
                        }
                    }
                    if popover.is_visible() {
                        suppress.set(true);
                        popover.popdown();
                        suppress.set(false);
                    }
                    glib::Propagation::Proceed
                }
                _ => glib::Propagation::Proceed,
            }
        });
        entry.add_controller(key_controller);
    }

    // ── Close popover on focus loss ──
    {
        let popover = popover.clone();
        let focus_controller = gtk4::EventControllerFocus::new();
        focus_controller.connect_leave(move |_| {
            let popover = popover.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                popover.popdown();
            });
        });
        entry.add_controller(focus_controller);
    }

    // ── Row activation (click on suggestion) ──
    {
        let suggestions = suggestions.clone();
        let popover = popover.clone();
        let entry_for_click = entry.clone();
        let suppress = suppress_suggestions.clone();
        list_box.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
            let url = suggestions.borrow().get(idx).cloned();
            if let Some(url) = url {
                suppress.set(true);
                entry_for_click.set_text(&url);
                popover.popdown();
                suppress.set(false);
                entry_for_click.emit_activate();
            }
        });
    }

    (container, entry)
}

struct SuggestionState {
    selected_idx: Rc<Cell<i32>>,
    suggestion_count: Rc<Cell<i32>>,
    suggestions: Rc<RefCell<Vec<String>>>,
}

#[allow(clippy::too_many_arguments)]
fn populate_suggestions(
    query: &str,
    engine: settings::SearchEngine,
    list_box: &gtk4::ListBox,
    popover: &gtk4::Popover,
    entry: &gtk4::Entry,
    state: &SuggestionState,
) {
    let selected_idx = &state.selected_idx;
    let suggestion_count = &state.suggestion_count;
    let suggestions = &state.suggestions;
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }
    selected_idx.set(-1);

    let trimmed = query.trim();
    if trimmed.is_empty() {
        popover.popdown();
        suggestion_count.set(0);
        suggestions.borrow_mut().clear();
        return;
    }

    let results = browser_history::search(trimmed, MAX_SUGGESTIONS);
    let mut urls: Vec<String> = Vec::new();

    for result in &results {
        let row = build_suggestion_row(&result.title, &result.url);
        list_box.append(&row);
        urls.push(result.url.clone());
    }

    // Search engine fallback row at bottom
    {
        let search_label = format!(
            "Search \"{}\" with {}",
            truncate(trimmed, 40),
            engine.label()
        );
        let row = build_search_fallback_row(&search_label);
        list_box.append(&row);
        urls.push(engine.search_url(trimmed));
    }

    let count = urls.len() as i32;
    suggestion_count.set(count);
    *suggestions.borrow_mut() = urls;

    if count > 0 {
        let width = entry.allocated_width();
        if width > 0 {
            popover.set_size_request(width, -1);
        }
        popover.popup();
    } else {
        popover.popdown();
    }
}

fn build_suggestion_row(title: &str, url: &str) -> gtk4::ListBoxRow {
    let row_box = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
    row_box.set_margin_start(6);
    row_box.set_margin_end(6);
    row_box.set_margin_top(3);
    row_box.set_margin_bottom(3);

    let title_label = gtk4::Label::new(Some(if title.is_empty() { url } else { title }));
    title_label.set_xalign(0.0);
    title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    row_box.append(&title_label);

    let url_label = gtk4::Label::new(Some(url));
    url_label.set_xalign(0.0);
    url_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    url_label.add_css_class("dim-label");
    url_label.add_css_class("caption");
    row_box.append(&url_label);

    let row = gtk4::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row
}

fn build_search_fallback_row(label: &str) -> gtk4::ListBoxRow {
    let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row_box.set_margin_start(6);
    row_box.set_margin_end(6);
    row_box.set_margin_top(4);
    row_box.set_margin_bottom(4);

    let icon = gtk4::Image::from_icon_name("system-search-symbolic");
    row_box.append(&icon);

    let text = gtk4::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text.add_css_class("dim-label");
    row_box.append(&text);

    let row = gtk4::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
