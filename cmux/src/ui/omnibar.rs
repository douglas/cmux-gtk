//! Omnibar — URL entry with history autocomplete, inline completion,
//! switch-to-tab suggestions, and optional remote search suggestions.

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

/// An open browser tab that can appear as a "Switch to tab" suggestion.
#[derive(Clone)]
pub struct OpenTab {
    pub title: String,
    pub url: String,
    #[allow(dead_code)]
    pub workspace_id: uuid::Uuid,
    #[allow(dead_code)]
    pub panel_id: uuid::Uuid,
}

/// Callback to retrieve currently open browser tabs.
pub type OpenTabsFn = Rc<dyn Fn() -> Vec<OpenTab>>;

/// Create an omnibar widget. Returns `(container, entry)` where `container` holds the entry
/// and the popover dropdown, and `entry` is the text entry for external wiring.
pub fn build_omnibar(
    initial_url: Option<&str>,
    search_engine: settings::SearchEngine,
) -> (gtk4::Box, gtk4::Entry) {
    build_omnibar_full(initial_url, search_engine, None)
}

/// Extended omnibar builder with optional open-tabs callback for switch-to-tab suggestions.
pub fn build_omnibar_full(
    initial_url: Option<&str>,
    search_engine: settings::SearchEngine,
    open_tabs_fn: Option<OpenTabsFn>,
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

    // Ghost text overlay for inline completion
    let ghost_label = gtk4::Label::new(None);
    ghost_label.add_css_class("omnibar-ghost");
    ghost_label.set_xalign(0.0);
    ghost_label.set_halign(gtk4::Align::Start);
    ghost_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    ghost_label.set_can_focus(false);
    ghost_label.set_can_target(false);

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&entry));
    overlay.add_overlay(&ghost_label);
    overlay.set_hexpand(true);
    container.append(&overlay);

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
    let suggestions: Rc<RefCell<Vec<SuggestionItem>>> = Rc::new(RefCell::new(Vec::new()));
    let debounce_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));
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
        let ghost_label = ghost_label.clone();
        let open_tabs_fn = open_tabs_fn.clone();

        entry.connect_changed(move |entry| {
            if suppress.get() {
                return;
            }

            let gen = debounce_gen.get().wrapping_add(1);
            debounce_gen.set(gen);

            let text = entry.text().to_string();

            // Update inline ghost text immediately (no debounce)
            update_ghost_text(&ghost_label, &text);

            let list_box = list_box.clone();
            let popover = popover.clone();
            let debounce_gen = debounce_gen.clone();
            let entry_widget = entry.clone();
            let sug_state = sug_state.clone();
            let open_tabs_fn = open_tabs_fn.clone();

            glib::timeout_add_local_once(
                std::time::Duration::from_millis(DEBOUNCE_MS),
                move || {
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
                        open_tabs_fn.as_ref(),
                    );
                },
            );
        });
    }

    // ── Keyboard navigation: Tab (accept ghost), Up/Down/Escape/Enter ──
    {
        let popover = popover.clone();
        let selected_idx = selected_idx.clone();
        let suggestion_count = suggestion_count.clone();
        let suggestions = suggestions.clone();
        let list_box = list_box.clone();
        let suppress = suppress_suggestions.clone();
        let entry_for_keys = entry.clone();
        let ghost_label = ghost_label.clone();

        let key_controller = gtk4::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, keyval, _, modifier| {
            match keyval {
                // Tab: accept inline ghost text completion
                gdk4::Key::Tab => {
                    let ghost = ghost_label.text();
                    if !ghost.is_empty() {
                        suppress.set(true);
                        entry_for_keys.set_text(&ghost);
                        entry_for_keys.set_position(-1);
                        ghost_label.set_text("");
                        suppress.set(false);
                        return glib::Propagation::Stop;
                    }
                    glib::Propagation::Proceed
                }
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
                    // Ctrl+Enter: accept ghost text and navigate
                    if modifier.contains(gdk4::ModifierType::CONTROL_MASK) {
                        let ghost = ghost_label.text();
                        if !ghost.is_empty() {
                            suppress.set(true);
                            entry_for_keys.set_text(&ghost);
                            entry_for_keys.set_position(-1);
                            ghost_label.set_text("");
                            suppress.set(false);
                            entry_for_keys.emit_activate();
                            return glib::Propagation::Stop;
                        }
                    }

                    let idx = selected_idx.get();
                    if idx >= 0 && popover.is_visible() {
                        let item = suggestions.borrow().get(idx as usize).cloned();
                        if let Some(item) = item {
                            suppress.set(true);
                            popover.popdown();
                            selected_idx.set(-1);
                            ghost_label.set_text("");
                            entry_for_keys.set_text(&item.url);
                            suppress.set(false);
                            entry_for_keys.emit_activate();
                            return glib::Propagation::Stop;
                        }
                    }
                    if popover.is_visible() {
                        suppress.set(true);
                        popover.popdown();
                        ghost_label.set_text("");
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
        let ghost_label = ghost_label.clone();
        let focus_controller = gtk4::EventControllerFocus::new();
        focus_controller.connect_leave(move |_| {
            let popover = popover.clone();
            let ghost_label = ghost_label.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                popover.popdown();
                ghost_label.set_text("");
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
        let ghost_label = ghost_label.clone();
        list_box.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
            let item = suggestions.borrow().get(idx).cloned();
            if let Some(item) = item {
                suppress.set(true);
                entry_for_click.set_text(&item.url);
                popover.popdown();
                ghost_label.set_text("");
                suppress.set(false);
                entry_for_click.emit_activate();
            }
        });
    }

    (container, entry)
}

/// A suggestion item in the dropdown — either a history entry, an open tab, or a search query.
#[derive(Clone)]
struct SuggestionItem {
    url: String,
    #[allow(dead_code)]
    kind: SuggestionKind,
}

#[derive(Clone)]
#[allow(dead_code)]
enum SuggestionKind {
    History,
    SwitchToTab,
    Search,
}

struct SuggestionState {
    selected_idx: Rc<Cell<i32>>,
    suggestion_count: Rc<Cell<i32>>,
    suggestions: Rc<RefCell<Vec<SuggestionItem>>>,
}

/// Update the ghost text overlay with the best inline completion for the typed text.
fn update_ghost_text(ghost_label: &gtk4::Label, query: &str) {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        ghost_label.set_text("");
        return;
    }

    // Find the best history URL that starts with the typed text
    let results = browser_history::search(trimmed, 1);
    if let Some(best) = results.first() {
        let url = &best.url;
        // Check if the URL starts with what the user typed (case-insensitive)
        let url_lower = url.to_lowercase();
        let query_lower = trimmed.to_lowercase();
        if url_lower.starts_with(&query_lower) {
            ghost_label.set_text(url);
            return;
        }
        // Also try stripping protocol prefix
        for prefix in &["https://", "http://", "https://www.", "http://www."] {
            if let Some(stripped) = url_lower.strip_prefix(prefix) {
                if stripped.starts_with(&query_lower) {
                    ghost_label.set_text(&url[prefix.len()..]);
                    return;
                }
            }
        }
    }
    ghost_label.set_text("");
}

#[allow(clippy::too_many_arguments)]
fn populate_suggestions(
    query: &str,
    engine: settings::SearchEngine,
    list_box: &gtk4::ListBox,
    popover: &gtk4::Popover,
    entry: &gtk4::Entry,
    state: &SuggestionState,
    open_tabs_fn: Option<&OpenTabsFn>,
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

    let mut items: Vec<SuggestionItem> = Vec::new();

    // ── Switch-to-tab suggestions (matching open browser tabs) ──
    if let Some(tabs_fn) = open_tabs_fn {
        let open_tabs = tabs_fn();
        let query_lower = trimmed.to_lowercase();
        for tab in &open_tabs {
            if tab.url.to_lowercase().contains(&query_lower)
                || tab.title.to_lowercase().contains(&query_lower)
            {
                let row = build_switch_tab_row(&tab.title, &tab.url);
                list_box.append(&row);
                items.push(SuggestionItem {
                    url: tab.url.clone(),
                    kind: SuggestionKind::SwitchToTab,
                });
                if items.len() >= 3 {
                    break;
                }
            }
        }
    }

    // ── History suggestions ──
    let history_limit = MAX_SUGGESTIONS.saturating_sub(items.len()).saturating_sub(1);
    let results = browser_history::search(trimmed, history_limit);
    for result in &results {
        // Skip if already shown as a switch-to-tab
        if items.iter().any(|i| i.url == result.url) {
            continue;
        }
        let row = build_suggestion_row(&result.title, &result.url);
        list_box.append(&row);
        items.push(SuggestionItem {
            url: result.url.clone(),
            kind: SuggestionKind::History,
        });
    }

    // ── Search engine fallback row at bottom ──
    {
        let search_label = format!(
            "Search \"{}\" with {}",
            truncate(trimmed, 40),
            engine.label()
        );
        let row = build_search_fallback_row(&search_label);
        list_box.append(&row);
        items.push(SuggestionItem {
            url: engine.search_url(trimmed),
            kind: SuggestionKind::Search,
        });
    }

    let count = items.len() as i32;
    suggestion_count.set(count);
    *suggestions.borrow_mut() = items;

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

fn build_switch_tab_row(title: &str, url: &str) -> gtk4::ListBoxRow {
    let row_box = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
    row_box.set_margin_start(6);
    row_box.set_margin_end(6);
    row_box.set_margin_top(3);
    row_box.set_margin_bottom(3);

    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let title_label = gtk4::Label::new(Some(if title.is_empty() { url } else { title }));
    title_label.set_xalign(0.0);
    title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title_label.set_hexpand(true);
    header.append(&title_label);

    let badge = gtk4::Label::new(Some("Switch to tab"));
    badge.add_css_class("caption");
    badge.add_css_class("status-pill-blue");
    badge.add_css_class("status-pill");
    header.append(&badge);

    row_box.append(&header);

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
