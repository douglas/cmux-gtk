//! Sidebar — workspace list using GtkListBox.

use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;

use glib::object::Cast;

use crate::app::{lock_or_recover, AppState};
use crate::model::Workspace;
use crate::settings::SidebarDisplaySettings;

pub struct SidebarWidgets {
    pub root: gtk4::Box,
    pub list_box: gtk4::ListBox,
    #[allow(dead_code)] // kept alive for GTK widget tree
    pub search_entry: gtk4::SearchEntry,
}

/// Create the sidebar widget containing the workspace list.
pub fn create_sidebar(state: &Rc<AppState>) -> SidebarWidgets {
    let sidebar_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    sidebar_box.add_css_class("sidebar");

    // Search/filter entry at top of sidebar
    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Filter workspaces..."));
    search_entry.set_margin_start(8);
    search_entry.set_margin_end(8);
    search_entry.set_margin_top(4);
    search_entry.set_margin_bottom(4);
    sidebar_box.append(&search_entry);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_vexpand(true);

    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.add_css_class("navigation-sidebar");

    // Apply sidebar focus style from settings
    if crate::settings::load().sidebar.focus_style == crate::settings::SidebarFocusStyle::LeftRail {
        list_box.add_css_class("sidebar-left-rail");
    }

    // Wire search entry to filter list rows
    {
        let search_entry_weak = search_entry.downgrade();
        list_box.set_filter_func(move |row| {
            let Some(search_entry) = search_entry_weak.upgrade() else {
                return true;
            };
            let query = search_entry.text().to_string().to_lowercase();
            if query.is_empty() {
                return true;
            }
            // Walk the row's widget tree to find the workspace-title label
            let Some(outer_box) = row_outer_box(row) else {
                return true;
            };
            let Some(header) = outer_box.first_child() else {
                return true;
            };
            let Some(header_box) = header.downcast_ref::<gtk4::Box>() else {
                return true;
            };
            // Find the title label (has workspace-title class)
            let mut child = header_box.first_child();
            while let Some(c) = child {
                if c.has_css_class("workspace-title") {
                    if let Some(label) = c.downcast_ref::<gtk4::Label>() {
                        return label.text().to_lowercase().contains(&query);
                    }
                }
                child = c.next_sibling();
            }
            true
        });

        let list_box_clone = list_box.clone();
        search_entry.connect_search_changed(move |_| {
            list_box_clone.invalidate_filter();
        });
    }

    refresh_sidebar(&list_box, state);

    // Double-click on empty sidebar space creates a new workspace
    let dbl_click = gtk4::GestureClick::new();
    dbl_click.set_button(1);
    {
        let state = state.clone();
        let list_box_weak = list_box.downgrade();
        dbl_click.connect_pressed(move |gesture, n_press, _x, y| {
            if n_press != 2 {
                return;
            }
            let Some(list_box) = list_box_weak.upgrade() else {
                return;
            };
            // Only fire if no row is under the cursor (clicked empty space)
            if list_box.row_at_y(y as i32).is_none() {
                gesture.set_state(gtk4::EventSequenceState::Claimed);
                let workspace = crate::model::Workspace::new();
                let placement = crate::settings::load().new_workspace_placement;
                lock_or_recover(&state.shared.tab_manager)
                    .add_workspace_with_placement(workspace, placement);
                state.shared.notify_ui_refresh();
            }
        });
    }
    list_box.add_controller(dbl_click);

    scrolled.set_child(Some(&list_box));
    sidebar_box.append(&scrolled);

    // Footer with help menu button
    let footer_btn = gtk4::MenuButton::new();
    footer_btn.set_label(&format!("jmux v{}", env!("CARGO_PKG_VERSION")));
    footer_btn.add_css_class("flat");
    footer_btn.add_css_class("dim-label");
    footer_btn.add_css_class("caption");
    footer_btn.set_margin_top(2);
    footer_btn.set_margin_bottom(2);
    footer_btn.set_halign(gtk4::Align::Center);
    footer_btn.set_direction(gtk4::ArrowType::Up);

    let help_popover = gtk4::Popover::new();
    let help_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    help_box.set_margin_start(8);
    help_box.set_margin_end(8);
    help_box.set_margin_top(8);
    help_box.set_margin_bottom(8);

    let welcome_btn = gtk4::Button::with_label("Welcome");
    welcome_btn.add_css_class("flat");
    {
        let state = state.clone();
        let help_popover = help_popover.clone();
        welcome_btn.connect_clicked(move |_| {
            help_popover.popdown();
            state.shared.notify_ui_refresh();
        });
    }
    help_box.append(&welcome_btn);

    let monitor_btn = gtk4::Button::with_label("Sub-agent Monitor");
    monitor_btn.add_css_class("flat");
    monitor_btn.set_tooltip_text(Some(
        "Show read-only panes tailing each Claude sub-agent in this workspace \
         (Ctrl+Shift+S toggles)",
    ));
    {
        let state = state.clone();
        let help_popover = help_popover.clone();
        monitor_btn.connect_clicked(move |_| {
            help_popover.popdown();
            crate::agent_monitor::toggle_selected(&state.shared);
        });
    }
    help_box.append(&monitor_btn);

    let shortcuts_btn = gtk4::Button::with_label("Keyboard Shortcuts");
    shortcuts_btn.add_css_class("flat");
    {
        let state = state.clone();
        let help_popover = help_popover.clone();
        shortcuts_btn.connect_clicked(move |_| {
            help_popover.popdown();
            state
                .shared
                .send_ui_event(crate::app::UiEvent::OpenSettings);
        });
    }
    help_box.append(&shortcuts_btn);

    let version_label = gtk4::Label::new(Some(&format!(
        "GTK4 + libadwaita\nGhostty terminal engine\nv{}",
        env!("CARGO_PKG_VERSION")
    )));
    version_label.add_css_class("dim-label");
    version_label.add_css_class("caption");
    version_label.set_margin_top(4);
    help_box.append(&version_label);

    help_popover.set_child(Some(&help_box));
    footer_btn.set_popover(Some(&help_popover));
    sidebar_box.append(&footer_btn);

    SidebarWidgets {
        root: sidebar_box,
        list_box,
        search_entry,
    }
}

thread_local! {
    /// Render signature of each sidebar ListBox's last build, keyed by widget
    /// pointer. Shell integration re-reports identical titles/pwds many times
    /// per second; without this, every report tears down and rebuilds every
    /// row (hundreds of widgets + closures). See the skip check below.
    static SIDEBAR_SIGNATURES: std::cell::RefCell<std::collections::HashMap<usize, u64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Refresh the workspace list from shared state.
pub fn refresh_sidebar(list_box: &gtk4::ListBox, state: &Rc<AppState>) {
    // Re-entrancy guard — checked BEFORE the signature bookkeeping so a
    // skipped inner call cannot record a signature for rows it never built.
    // The append loop below calls `select_row`, which fires `row-selected`
    // synchronously → the handler calls refresh_ui → back into this
    // function. If the model changed mid-rebuild (socket threads mutate
    // tab_manager — e.g. graph.create adding a workspace), the inner rebuild
    // would clear and refill the list while the outer loop then appends its
    // remaining rows on top: duplicated sidebar rows. The trailing throttled
    // refresh from the selection handler repaints any staleness ~120 ms
    // later.
    thread_local! {
        static REBUILDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if REBUILDING.with(|r| r.get()) {
        tracing::debug!("skipping re-entrant sidebar rebuild");
        return;
    }
    REBUILDING.with(|r| r.set(true));
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            REBUILDING.with(|r| r.set(false));
        }
    }
    let _guard = Guard;

    // Skip the rebuild when nothing the sidebar renders has changed. The
    // signature hashes the Debug form of every workspace and group plus the
    // selection and display settings — deliberately over-inclusive (a change
    // to a non-rendered field costs one extra rebuild, same as before this
    // check; a missed field would mean stale UI, so we hash everything).
    // Selection is deliberately excluded from the signature: a pure workspace
    // switch changes only which row is highlighted, and rebuilding every row to
    // move that highlight was the dominant cost that made switches sluggish (and
    // dropped clicks that landed while the rows were being torn down). On the
    // unchanged path we move the highlight in place instead (see below).
    let signature = {
        use std::hash::{Hash, Hasher};
        let tab_manager = lock_or_recover(&state.shared.tab_manager);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for ws in tab_manager.iter() {
            format!("{ws:?}").hash(&mut h);
        }
        for group in tab_manager.groups() {
            format!("{group:?}").hash(&mut h);
        }
        format!("{:?}", crate::settings::load().sidebar).hash(&mut h);
        h.finish()
    };
    let key = list_box.as_ptr() as usize;
    let unchanged = SIDEBAR_SIGNATURES.with(|s| s.borrow().get(&key) == Some(&signature));
    // An empty list means this ListBox was never built (or a new widget reused
    // a freed address) — never skip the first build.
    if unchanged && list_box.first_child().is_some() {
        // Structure is identical; only the selection may have moved. Move the
        // highlight without a full teardown/rebuild of every row.
        update_selected_row(list_box, state);
        return;
    }
    SIDEBAR_SIGNATURES.with(|s| {
        s.borrow_mut().insert(key, signature);
    });

    // Unparent popover menus before removing rows to avoid GTK warnings
    // about finalized widgets with leftover children.
    while let Some(child) = list_box.first_child() {
        if let Some(row) = child.downcast_ref::<gtk4::ListBoxRow>() {
            let mut maybe = row.first_child();
            while let Some(c) = maybe {
                maybe = c.next_sibling();
                if c.downcast_ref::<gtk4::PopoverMenu>().is_some() {
                    c.unparent();
                }
            }
        }
        list_box.remove(&child);
    }

    // Build rows and capture selection index while holding the lock, then
    // release the lock before calling list_box.select_row.  select_row emits
    // `row-selected` synchronously; the connected handler tries to acquire
    // the same tab_manager lock, which would deadlock on std::sync::Mutex.
    //
    // We also pre-collect all workspace identifiers needed by the context menu
    // so that setup_row_context_menu does not re-acquire tab_manager while the
    // lock is already held (std::sync::Mutex is not re-entrant).
    let sidebar_settings = crate::settings::load().sidebar;
    // Each entry pairs a row widget with its workspace index (None for group
    // header rows), so selection and drag-drop keep using workspace indices.
    let (rows, selected_index): (Vec<(gtk4::ListBoxRow, Option<usize>)>, Option<usize>) = {
        let mut tab_manager = lock_or_recover(&state.shared.tab_manager);
        // Record focus history on the single chokepoint that runs after any
        // selection change. No-ops when the selection is unchanged.
        tab_manager.record_focus_if_changed();
        let selected_index = tab_manager.selected_index();
        // Pre-collect (index, id, title) so setup_row_context_menu can build the
        // "Move Focused Pane → …" submenu without re-locking tab_manager.
        let all_workspaces: Vec<(usize, uuid::Uuid, String)> = tab_manager
            .iter()
            .enumerate()
            .map(|(i, ws)| (i, ws.id, ws.display_title().to_string()))
            .collect();
        // Pre-collect groups (id, name) for the workspace "Add to Group" menu.
        let all_groups: Vec<(uuid::Uuid, String)> = tab_manager
            .groups()
            .iter()
            .map(|g| (g.id, g.name.clone()))
            .collect();
        let mut rows: Vec<(gtk4::ListBoxRow, Option<usize>)> = Vec::new();
        let mut rendered_groups: std::collections::HashSet<uuid::Uuid> =
            std::collections::HashSet::new();
        for (index, workspace) in tab_manager.iter().enumerate() {
            // Insert a group header before the first member of each group, and
            // hide member rows when the group is collapsed.
            let mut collapsed = false;
            if let Some(gid) = workspace.group_id {
                if let Some(group) = tab_manager.group(gid) {
                    collapsed = group.collapsed;
                    if rendered_groups.insert(gid) {
                        let unread = tab_manager.group_unread_count(gid);
                        let header = create_group_header_row(group, unread, state);
                        rows.push((header, None));
                    }
                }
                if collapsed {
                    continue;
                }
            }
            let row = create_workspace_row(workspace, index, &sidebar_settings, state);
            if workspace.group_id.is_some() {
                row.add_css_class("workspace-row-grouped");
            }
            setup_row_context_menu(
                &row,
                index,
                workspace.is_pinned,
                workspace.window_id,
                workspace.id,
                workspace.remote_config.is_some(),
                workspace.group_id,
                &all_workspaces,
                &all_groups,
                state,
            );
            setup_row_close_button(&row, index, state);
            rows.push((row, Some(index)));
        }
        (rows, selected_index)
    };

    for (row, ws_index) in rows.iter() {
        // Drag-and-drop for workspace reordering (workspace rows only)
        if let Some(idx) = ws_index {
            setup_row_drag_drop(row, *idx, state);
        }
        list_box.append(row);
        if let Some(idx) = ws_index {
            if selected_index == Some(*idx) {
                list_box.select_row(Some(row));
            }
        }
    }

    // Reapply search filter after rebuild
    list_box.invalidate_filter();
}

/// Move the selection highlight to the currently selected workspace's row
/// without rebuilding the list. The fast path for a pure workspace switch: the
/// rows already exist (structure unchanged), so we just re-select the matching
/// one by its `ws-<index>` widget name.
///
/// Records focus history first — the chokepoint the full rebuild would
/// otherwise own (`record_focus_if_changed` no-ops when unchanged). The
/// `tab_manager` lock is released before `select_row`, which emits
/// `row-selected` synchronously and would re-lock (std::sync::Mutex is not
/// re-entrant).
fn update_selected_row(list_box: &gtk4::ListBox, state: &Rc<AppState>) {
    let selected_index = {
        let mut tab_manager = lock_or_recover(&state.shared.tab_manager);
        tab_manager.record_focus_if_changed();
        tab_manager.selected_index()
    };
    let Some(selected_index) = selected_index else {
        list_box.unselect_all();
        return;
    };
    let target_name = format!("ws-{selected_index}");
    let mut child = list_box.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if widget.widget_name().as_str() != target_name {
            continue;
        }
        if let Some(row) = widget.downcast_ref::<gtk4::ListBoxRow>() {
            if !row.is_selected() {
                list_box.select_row(Some(row));
            }
        }
        return;
    }
}

/// Set up drag-and-drop on a sidebar workspace row for reordering.
fn setup_row_drag_drop(row: &gtk4::ListBoxRow, index: usize, state: &Rc<AppState>) {
    // Drag source — provides the source index as a string
    let drag_source = gtk4::DragSource::new();
    drag_source.set_actions(gdk4::DragAction::MOVE);
    {
        let index_str = index.to_string();
        drag_source.connect_prepare(move |_source, _x, _y| {
            let content = gdk4::ContentProvider::for_value(&index_str.to_value());
            Some(content)
        });
    }
    row.add_controller(drag_source);

    // Drop target — accepts a string (the source index) and reorders
    let drop_target = gtk4::DropTarget::new(glib::Type::STRING, gdk4::DragAction::MOVE);
    {
        let state = state.clone();
        let target_index = index;
        drop_target.connect_drop(move |_target, value, _x, _y| {
            let Ok(source_str) = value.get::<String>() else {
                return false;
            };
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            // A group header drag carries "group:<uuid>" — move the whole group
            // to sit before the workspace under the drop.
            if let Some(gid) = source_str
                .strip_prefix("group:")
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
            {
                let target_ws_id = tm.get(target_index).map(|w| w.id);
                let moved = tm.move_group_before(gid, target_ws_id);
                drop(tm);
                if moved {
                    state.shared.notify_ui_refresh();
                }
                return moved;
            }
            // A tab drag carries "<index>/<panel_uuid>" — move that panel into
            // the workspace under the drop.
            if let Some(panel_id) = source_str
                .split_once('/')
                .and_then(|(_, id)| uuid::Uuid::parse_str(id).ok())
            {
                let Some(target_ws_id) = tm.get(target_index).map(|w| w.id) else {
                    return false;
                };
                let moved = tm
                    .move_panel_to_workspace(panel_id, target_ws_id)
                    .is_some();
                drop(tm);
                if moved {
                    state.shared.notify_ui_refresh();
                }
                return moved;
            }
            let Ok(source_index) = source_str.parse::<usize>() else {
                return false;
            };
            if source_index == target_index {
                return false;
            }
            tm.move_workspace(source_index, target_index);
            drop(tm);
            state.shared.notify_ui_refresh();
            true
        });
    }
    row.add_controller(drop_target);
}

/// The outer content box of a workspace row — unwrapping the Claude-sprite
/// Overlay when present (rows with an active agent wrap `outer` in one).
fn row_outer_box(row: &gtk4::ListBoxRow) -> Option<gtk4::Box> {
    let child = row.child()?;
    if let Some(overlay) = child.downcast_ref::<gtk4::Overlay>() {
        return overlay.child()?.downcast::<gtk4::Box>().ok();
    }
    child.downcast::<gtk4::Box>().ok()
}

fn create_workspace_row(
    workspace: &Workspace,
    index: usize,
    sidebar: &SidebarDisplaySettings,
    state: &Rc<AppState>,
) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    // Tag the row with its workspace index so `update_selected_row` can find and
    // re-highlight it on a selection-only switch without rebuilding the list.
    row.set_widget_name(&format!("ws-{index}"));

    // Workspace color indicator: colored left border when custom_color is set.
    if let Some(ref color) = workspace.custom_color {
        row.add_css_class("workspace-row-colored");
        let css = gtk4::CssProvider::new();
        css.load_from_data(&format!("row {{ border-left-color: {}; }}", color));
        row.style_context()
            .add_provider(&css, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    } else {
        row.add_css_class("workspace-row");
    }

    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    outer.set_margin_start(10);
    outer.set_margin_end(10);
    outer.set_margin_top(5);
    outer.set_margin_bottom(5);

    // ── Header: index + pin icon + title + unread badge + close button ──
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);

    let index_label = gtk4::Label::new(Some(&format!("{}", index + 1)));
    index_label.add_css_class("dim-label");
    index_label.add_css_class("caption");
    index_label.add_css_class("workspace-index");
    header.append(&index_label);

    // Workspace type icon — pick based on dominant panel type
    let has_browser = workspace
        .panels
        .values()
        .any(|p| p.panel_type == crate::model::PanelType::Browser);
    let icon_name = if has_browser {
        "globe-symbolic"
    } else {
        "utilities-terminal-symbolic"
    };
    let type_icon = gtk4::Image::from_icon_name(icon_name);
    type_icon.set_pixel_size(14);
    type_icon.add_css_class("workspace-type-icon");
    header.append(&type_icon);

    // Pin indicator
    if workspace.is_pinned {
        let pin_icon = gtk4::Image::from_icon_name("view-pin-symbolic");
        pin_icon.set_pixel_size(12);
        pin_icon.add_css_class("dim-label");
        header.append(&pin_icon);
    }

    // Remote connection state indicator
    if workspace.remote_config.is_some() {
        let (icon_name, css_class, tooltip) = match &workspace.remote_state {
            Some(crate::remote::session::RemoteState::Connected { .. }) => (
                "emblem-ok-symbolic",
                "remote-connected",
                "Remote: Connected",
            ),
            Some(crate::remote::session::RemoteState::Connecting) => (
                "content-loading-symbolic",
                "remote-connecting",
                "Remote: Connecting...",
            ),
            Some(crate::remote::session::RemoteState::Error(msg)) => {
                ("dialog-warning-symbolic", "remote-error", msg.as_str())
            }
            _ => (
                "network-offline-symbolic",
                "remote-disconnected",
                "Remote: Disconnected",
            ),
        };
        let state_icon = gtk4::Image::from_icon_name(icon_name);
        state_icon.set_pixel_size(12);
        state_icon.add_css_class(css_class);
        state_icon.set_tooltip_text(Some(tooltip));
        header.append(&state_icon);

        // Manual reconnect control — shown when the remote is not actively
        // connected/connecting (i.e. errored, disconnected, or never started).
        let show_reconnect = !matches!(
            &workspace.remote_state,
            Some(crate::remote::session::RemoteState::Connected { .. })
                | Some(crate::remote::session::RemoteState::Connecting)
        );
        if show_reconnect {
            let reconnect_btn = gtk4::Button::from_icon_name("view-refresh-symbolic");
            reconnect_btn.add_css_class("flat");
            reconnect_btn.add_css_class("circular");
            reconnect_btn.set_tooltip_text(Some("Reconnect"));
            let st = state.clone();
            let wsid = workspace.id;
            reconnect_btn.connect_clicked(move |_| {
                st.shared
                    .send_ui_event(crate::app::UiEvent::RemoteConnect { workspace_id: wsid });
            });
            header.append(&reconnect_btn);
        }
    }

    let title_label = gtk4::Label::new(Some(workspace.display_title()));
    title_label.set_hexpand(true);
    title_label.set_halign(gtk4::Align::Start);
    title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title_label.add_css_class("workspace-title");
    header.append(&title_label);

    // Hibernation indicator — shown when the focused pane's agent is paused.
    if workspace
        .focused_panel_id
        .map(|pid| state.shared.is_hibernated(&pid))
        .unwrap_or(false)
    {
        let hib_icon = gtk4::Image::from_icon_name("media-playback-pause-symbolic");
        hib_icon.set_pixel_size(12);
        hib_icon.add_css_class("dim-label");
        hib_icon.set_tooltip_text(Some("Agent hibernated (paused)"));
        header.append(&hib_icon);
    }

    if workspace.unread_count > 0 {
        let badge = gtk4::Label::new(Some(&workspace.unread_count.to_string()));
        badge.add_css_class("badge");
        badge.add_css_class("accent");
        header.append(&badge);
    }

    // Hover close button (hidden by default)
    let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("flat");
    close_btn.add_css_class("circular");
    close_btn.add_css_class("sidebar-close-btn");
    close_btn.set_visible(false);
    close_btn.set_tooltip_text(Some("Close workspace"));
    header.append(&close_btn);

    outer.append(&header);

    // ── Meta line: agent status | git branch | directory ──
    let meta_label = gtk4::Label::new(Some(&workspace_meta_text(workspace, sidebar)));
    meta_label.set_halign(gtk4::Align::Start);
    meta_label.set_wrap(false);
    meta_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    meta_label.add_css_class("caption");
    meta_label.add_css_class("dim-label");
    outer.append(&meta_label);

    // ── Per-panel branch vertical layout ──
    if !sidebar.hide_all_details && sidebar.branch_vertical_layout && sidebar.show_git_branch {
        let branches: Vec<_> = workspace
            .panels
            .values()
            .filter_map(|p| p.git_branch.as_ref())
            .collect();
        if branches.len() > 1 || (branches.len() == 1 && workspace.git_branch.is_some()) {
            let branch_box = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
            let mut sorted_panels: Vec<_> = workspace.panels.iter().collect();
            sorted_panels.sort_by_key(|(id, _)| *id);
            for (_, panel) in sorted_panels {
                if let Some(ref gb) = panel.git_branch {
                    let panel_title = panel
                        .custom_title
                        .as_deref()
                        .or(panel.title.as_deref())
                        .unwrap_or("pane");
                    let text = if gb.is_dirty {
                        format!("  {} git {} *", panel_title, gb.branch)
                    } else {
                        format!("  {} git {}", panel_title, gb.branch)
                    };
                    let label = gtk4::Label::new(Some(&text));
                    label.set_halign(gtk4::Align::Start);
                    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                    label.add_css_class("caption");
                    label.add_css_class("dim-label");
                    branch_box.append(&label);
                }
            }
            outer.append(&branch_box);
        }
    }

    // ── Status pills ──
    if !sidebar.hide_all_details
        && sidebar.show_status_pills
        && !workspace.status_entries.is_empty()
    {
        let pills_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        pills_box.set_halign(gtk4::Align::Start);
        // Show up to 4 most recent status entries
        let entries: Vec<_> = workspace.status_entries.iter().rev().take(4).collect();
        for entry in entries.into_iter().rev() {
            let text = if entry.key == "agent" {
                entry.value.clone()
            } else {
                format!("{}: {}", entry.key, entry.value)
            };
            let pill = gtk4::Label::new(Some(&text));
            pill.add_css_class("status-pill");
            pill.add_css_class("caption");
            pill.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            pill.set_max_width_chars(20);
            if let Some(ref color) = entry.color {
                match color.as_str() {
                    "blue" => pill.add_css_class("status-pill-blue"),
                    "green" => pill.add_css_class("status-pill-green"),
                    "red" => pill.add_css_class("status-pill-red"),
                    "orange" => pill.add_css_class("status-pill-orange"),
                    "purple" => pill.add_css_class("status-pill-purple"),
                    "yellow" => pill.add_css_class("status-pill-yellow"),
                    _ => {}
                }
            }
            pills_box.append(&pill);
        }
        outer.append(&pills_box);
    }

    // ── Metadata entries (sorted by priority desc) ──
    if !sidebar.hide_all_details && !workspace.metadata_entries.is_empty() {
        let mut sorted: Vec<_> = workspace.metadata_entries.iter().collect();
        sorted.sort_by(|a, b| {
            b.priority.cmp(&a.priority).then(
                a.timestamp
                    .partial_cmp(&b.timestamp)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        let meta_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        for entry in sorted.iter().take(6) {
            let text = format!("{}: {}", entry.key, entry.value);
            let label = gtk4::Label::new(Some(&text));
            label.set_halign(gtk4::Align::Start);
            label.set_wrap(false);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.add_css_class("caption");
            if entry.url.is_some() {
                label.add_css_class("meta-link");
            }
            if let Some(ref color) = entry.color {
                match color.as_str() {
                    "blue" => label.add_css_class("status-pill-blue"),
                    "green" => label.add_css_class("status-pill-green"),
                    "red" => label.add_css_class("status-pill-red"),
                    "orange" => label.add_css_class("status-pill-orange"),
                    "purple" => label.add_css_class("status-pill-purple"),
                    _ => label.add_css_class("dim-label"),
                }
            } else {
                label.add_css_class("dim-label");
            }
            meta_box.append(&label);
        }
        outer.append(&meta_box);
    }

    // ── Metadata blocks (freeform markdown, sorted by priority desc) ──
    if !sidebar.hide_all_details && !workspace.metadata_blocks.is_empty() {
        let mut sorted: Vec<_> = workspace.metadata_blocks.iter().collect();
        sorted.sort_by(|a, b| {
            b.priority.cmp(&a.priority).then(
                a.timestamp
                    .partial_cmp(&b.timestamp)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        for (i, block) in sorted.iter().take(3).enumerate() {
            let first_line = block.content.lines().next().unwrap_or(&block.content);
            let text = if block.key.is_empty() {
                first_line.to_string()
            } else {
                format!("[{}] {}", block.key, first_line)
            };
            let label = gtk4::Label::new(Some(&text));
            label.set_wrap(false);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.add_css_class("caption");
            if workspace.imessage_mode {
                // Alternate assistant/user alignment per block index
                let (bubble_class, align) = if i % 2 == 0 {
                    ("chat-bubble-assistant", gtk4::Align::End)
                } else {
                    ("chat-bubble-user", gtk4::Align::Start)
                };
                let bubble = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
                bubble.set_halign(align);
                label.add_css_class(bubble_class);
                label.set_halign(align);
                bubble.append(&label);
                outer.append(&bubble);
            } else {
                label.set_halign(gtk4::Align::Start);
                label.add_css_class("dim-label");
                outer.append(&label);
            }
        }
    }

    // ── Progress bar ──
    if !sidebar.hide_all_details && sidebar.show_progress {
        if let Some(ref progress) = workspace.progress {
            let progress_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            if let Some(ref label_text) = progress.label {
                let label = gtk4::Label::new(Some(label_text));
                label.set_halign(gtk4::Align::Start);
                label.add_css_class("caption");
                label.add_css_class("dim-label");
                label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                progress_box.append(&label);
            }
            let bar = gtk4::ProgressBar::new();
            bar.add_css_class("sidebar-progress");
            if progress.value > 1.0 {
                bar.pulse();
            } else {
                bar.set_fraction(progress.value.clamp(0.0, 1.0));
            }
            progress_box.append(&bar);
            outer.append(&progress_box);
        }
    }

    // ── Listening ports ──
    if !sidebar.hide_all_details && sidebar.show_ports {
        let all_ports: Vec<u16> = workspace
            .panels
            .values()
            .flat_map(|p| &p.listening_ports)
            .copied()
            .collect();
        if !all_ports.is_empty() {
            let ports_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            ports_box.set_halign(gtk4::Align::Start);
            let mut sorted_ports = all_ports;
            sorted_ports.sort_unstable();
            sorted_ports.dedup();
            for port in sorted_ports.iter().take(5) {
                let port_label = gtk4::Label::new(Some(&format!(":{port}")));
                port_label.add_css_class("port-badge");
                port_label.add_css_class("caption");
                // Click to open localhost:PORT (internal browser or xdg-open)
                let url = format!("http://localhost:{port}");
                let gesture = gtk4::GestureClick::new();
                gesture.set_button(1);
                {
                    let state = state.clone();
                    gesture.connect_pressed(move |gesture, _, _, _| {
                        gesture.set_state(gtk4::EventSequenceState::Claimed);
                        if crate::settings::load().sidebar.port_link_external {
                            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                        } else {
                            state.shared.send_ui_event(crate::app::UiEvent::OpenUrlInBrowser {
                                url: url.clone(),
                            });
                        }
                    });
                }
                port_label.set_can_focus(false);
                port_label.add_controller(gesture);
                ports_box.append(&port_label);
            }
            if sorted_ports.len() > 5 {
                let more = gtk4::Label::new(Some(&format!("+{}", sorted_ports.len() - 5)));
                more.add_css_class("port-badge");
                more.add_css_class("caption");
                ports_box.append(&more);
            }
            outer.append(&ports_box);
        }
    }

    // ── Log entries ──
    if !sidebar.hide_all_details && sidebar.show_logs {
        if workspace.imessage_mode {
            // iMessage mode: show up to 5 recent log entries as alternating bubbles
            let entries: Vec<_> = workspace.log_entries.iter().rev().take(5).collect();
            for (i, log_entry) in entries.into_iter().rev().enumerate() {
                let log_text = if let Some(ref source) = log_entry.source {
                    format!("[{}] {}", source, log_entry.message)
                } else {
                    log_entry.message.clone()
                };
                let log_label = gtk4::Label::new(Some(&log_text));
                log_label.set_wrap(false);
                log_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                log_label.add_css_class("caption");
                // "error"/"warning" levels from shell = user side; progress/info = assistant side
                let is_user_side = matches!(
                    log_entry.level.as_str(),
                    "warning" | "warn" | "error" | "info"
                );
                let (bubble_class, align) = if i % 2 == 0 || !is_user_side {
                    ("chat-bubble-assistant", gtk4::Align::End)
                } else {
                    ("chat-bubble-user", gtk4::Align::Start)
                };
                log_label.add_css_class(bubble_class);
                log_label.set_halign(align);
                let bubble = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
                bubble.set_halign(align);
                bubble.append(&log_label);
                outer.append(&bubble);
            }
        } else if let Some(log_entry) = workspace.log_entries.last() {
            let log_text = if let Some(ref source) = log_entry.source {
                format!("[{}] {}", source, log_entry.message)
            } else {
                log_entry.message.clone()
            };
            let log_label = gtk4::Label::new(Some(&log_text));
            log_label.set_halign(gtk4::Align::Start);
            log_label.set_wrap(false);
            log_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            log_label.add_css_class("caption");
            match log_entry.level.as_str() {
                "warning" | "warn" => log_label.add_css_class("log-warning"),
                "error" => log_label.add_css_class("log-error"),
                "success" => log_label.add_css_class("log-success"),
                "progress" => log_label.add_css_class("log-progress"),
                _ => log_label.add_css_class("log-info"),
            }
            outer.append(&log_label);
        }
    }

    // ── PR status pill ──
    if !sidebar.hide_all_details && sidebar.show_pr_status {
        if let Some(ref pr_status) = workspace.pr_status {
            let pr_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            pr_box.set_halign(gtk4::Align::Start);
            let pr_label = gtk4::Label::new(Some(&format!("PR: {pr_status}")));
            pr_label.add_css_class("status-pill");
            pr_label.add_css_class("caption");
            match pr_status.as_str() {
                "merged" => pr_label.add_css_class("status-pill-green"),
                "open" | "draft" => pr_label.add_css_class("status-pill-yellow"),
                "closed" => pr_label.add_css_class("status-pill-red"),
                _ => {}
            }
            // Show individual check icons if available
            if !workspace.pr_checks.is_empty() {
                for check in workspace.pr_checks.iter().take(8) {
                    let icon = match check.conclusion.as_str() {
                        "SUCCESS" => "\u{2713}",             // ✓
                        "FAILURE" => "\u{2717}",             // ✗
                        "NEUTRAL" | "SKIPPED" => "\u{2014}", // —
                        _ => "\u{25CB}",                     // ○ (pending)
                    };
                    let check_label = gtk4::Label::new(Some(&format!("{} {}", icon, check.name)));
                    check_label.set_halign(gtk4::Align::Start);
                    check_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                    check_label.set_max_width_chars(30);
                    check_label.add_css_class("caption");
                    match check.conclusion.as_str() {
                        "SUCCESS" => check_label.add_css_class("status-pill-green"),
                        "FAILURE" => check_label.add_css_class("status-pill-red"),
                        "PENDING" | "" => check_label.add_css_class("status-pill-yellow"),
                        _ => check_label.add_css_class("dim-label"),
                    }
                    pr_box.append(&check_label);
                }
            }
            outer.append(&pr_box);
        }
    }

    // ── Notification line ──
    if !sidebar.hide_all_details {
        let notification_text = if sidebar.show_notification_message {
            workspace.latest_notification.clone().or_else(|| {
                sidebar
                    .show_directory
                    .then(|| compact_path(&workspace.current_directory))
            })
        } else if sidebar.show_directory {
            Some(compact_path(&workspace.current_directory))
        } else {
            None
        };
        if let Some(text) = notification_text {
            let notification_label = gtk4::Label::new(Some(&text));
            notification_label.set_halign(gtk4::Align::Start);
            notification_label.set_wrap(false);
            notification_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            notification_label.add_css_class("caption");
            if sidebar.show_notification_message && workspace.unread_count > 0 {
                notification_label.add_css_class("sidebar-notification");
            } else {
                notification_label.add_css_class("dim-label");
            }
            outer.append(&notification_label);
        }
    }

    // ── File explorer (collapsible) ──
    if !sidebar.hide_all_details {
        let expander = gtk4::Expander::new(Some("Files"));
        expander.add_css_class("caption");
        expander.set_margin_top(2);

        if workspace.remote_config.is_some() {
            let placeholder = crate::ui::file_explorer::FileExplorer::new_ssh_placeholder(
                &workspace.current_directory,
            );
            expander.set_child(Some(placeholder.widget()));
        } else {
            let explorer = crate::ui::file_explorer::FileExplorer::new();
            explorer.set_root(&workspace.current_directory);
            {
                // Insert Path / Insert Relative Path → send into the focused terminal.
                let state = state.clone();
                let ws_dir = workspace.current_directory.clone();
                explorer.set_insert_callback(move |full_path, relative| {
                    let text = if relative {
                        let base = format!("{}/", ws_dir.trim_end_matches('/'));
                        full_path
                            .strip_prefix(&base)
                            .map(String::from)
                            .unwrap_or_else(|| full_path.clone())
                    } else {
                        full_path.clone()
                    };
                    let panel_id = {
                        let tm = lock_or_recover(&state.shared.tab_manager);
                        tm.selected().and_then(|w| w.focused_panel_id)
                    };
                    if let Some(panel_id) = panel_id {
                        if let Some(surface) = state.terminal_cache.borrow().get(&panel_id) {
                            surface.send_text(&format!("{text} "));
                        }
                    }
                });
            }
            expander.set_child(Some(explorer.widget()));
        }

        outer.append(&expander);
    }

    // ── Rich tooltip: workspace name + panel count + directory + status ──
    {
        let panel_count = workspace.panels.len();
        let active_label = workspace
            .sidebar_status_label()
            .map(|s| format!("\nStatus: {s}"))
            .unwrap_or_default();
        let git_label = workspace
            .git_branch
            .as_ref()
            .map(|gb| {
                if gb.is_dirty {
                    format!("\nBranch: {} *", gb.branch)
                } else {
                    format!("\nBranch: {}", gb.branch)
                }
            })
            .unwrap_or_default();
        let dir_label = if workspace.current_directory.is_empty() {
            String::new()
        } else {
            format!("\nDirectory: {}", compact_path(&workspace.current_directory))
        };
        let desc_label = workspace
            .description
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(|d| format!("\n{d}"))
            .unwrap_or_default();
        let ports_label = {
            let ports: Vec<u16> = workspace
                .panels
                .values()
                .flat_map(|p| &p.listening_ports)
                .copied()
                .collect();
            if ports.is_empty() {
                String::new()
            } else {
                let mut sorted = ports;
                sorted.sort_unstable();
                sorted.dedup();
                let port_strs: Vec<String> = sorted.iter().take(5).map(|p| p.to_string()).collect();
                format!("\nPorts: {}", port_strs.join(", "))
            }
        };
        let tooltip = format!(
            "{}{}\nPanels: {}{}{}{}{}",
            workspace.display_title(),
            desc_label,
            panel_count,
            dir_label,
            git_label,
            active_label,
            ports_label,
        );
        row.set_tooltip_text(Some(&tooltip));
    }

    // ── Hover show/hide close button ──
    let motion = gtk4::EventControllerMotion::new();
    {
        let close_btn = close_btn.clone();
        motion.connect_enter(move |_, _, _| {
            close_btn.set_visible(true);
        });
    }
    {
        let close_btn = close_btn.clone();
        motion.connect_leave(move |_| {
            close_btn.set_visible(false);
        });
    }
    row.add_controller(motion);

    // Claude state sprite — the deck octopus, bottom-right, when an agent in
    // this workspace is working / needs input / waiting on a background task.
    if let Some(cs) = crate::ui::state_sprite::workspace_claude_state(workspace, state) {
        let overlay = gtk4::Overlay::new();
        overlay.set_child(Some(&outer));
        let sprite = crate::ui::state_sprite::sprite_image(cs, &outer);
        sprite.set_halign(gtk4::Align::End);
        sprite.set_valign(gtk4::Align::End);
        sprite.set_margin_end(8);
        sprite.set_margin_bottom(3);
        sprite.set_can_target(false); // clicks fall through to the row
        overlay.add_overlay(&sprite);
        row.set_child(Some(&overlay));
    } else {
        row.set_child(Some(&outer));
    }
    row
}

/// Build a non-selectable group header row with collapse toggle, color,
/// name, unread badge, and a right-click context menu.
fn create_group_header_row(
    group: &crate::model::WorkspaceGroup,
    unread: u32,
    state: &Rc<AppState>,
) -> gtk4::ListBoxRow {
    let group_id = group.id;
    let row = gtk4::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.add_css_class("workspace-group-header");

    // Colored left border when a group color is set.
    if let Some(ref color) = group.color {
        let css = gtk4::CssProvider::new();
        css.load_from_data(&format!("row {{ border-left: 3px solid {color}; }}"));
        row.style_context()
            .add_provider(&css, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    hbox.set_margin_start(8);
    hbox.set_margin_end(10);
    hbox.set_margin_top(3);
    hbox.set_margin_bottom(3);

    let chevron = gtk4::Image::from_icon_name(if group.collapsed {
        "pan-end-symbolic"
    } else {
        "pan-down-symbolic"
    });
    chevron.set_pixel_size(12);
    chevron.add_css_class("dim-label");
    hbox.append(&chevron);

    let name = gtk4::Label::new(Some(&group.name));
    name.add_css_class("caption-heading");
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name.set_xalign(0.0);
    hbox.append(&name);

    let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    hbox.append(&spacer);

    if unread > 0 {
        let badge = gtk4::Label::new(Some(&unread.to_string()));
        badge.add_css_class("badge");
        badge.add_css_class("accent");
        hbox.append(&badge);
    }

    row.set_child(Some(&hbox));

    // Left-click toggles collapse.
    {
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        let state = state.clone();
        click.connect_released(move |_, _, _, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.set_group_collapsed(group_id, None);
            drop(tm);
            state.shared.notify_ui_refresh();
        });
        row.add_controller(click);
    }

    // Right-click context menu.
    let menu = gtk4::gio::Menu::new();
    menu.append(
        Some(if group.collapsed { "Expand" } else { "Collapse" }),
        Some("group.collapse"),
    );
    menu.append(Some("Rename"), Some("group.rename"));
    // Set Color — a live swatch grid embedded as a custom popover child (GTK4
    // popover menus ignore per-item icons, so a plain menu would show only the
    // colour names). The swatches activate the `group.color.*` actions below.
    let color_menu = gtk4::gio::Menu::new();
    let grid_item = gtk4::gio::MenuItem::new(None, None);
    grid_item.set_attribute_value("custom", Some(&"group-color-grid".to_variant()));
    color_menu.append_item(&grid_item);
    menu.append_submenu(Some("Set Color"), &color_menu);
    menu.append(Some("New Workspace in Group"), Some("group.new-ws"));
    let delete_menu = gtk4::gio::Menu::new();
    delete_menu.append(Some("Delete Group"), Some("group.delete"));
    menu.append_section(None, &delete_menu);

    let popover = gtk4::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(&row);
    popover.set_has_arrow(false);
    popover.add_child(
        &build_color_swatch_grid(GROUP_COLORS, &popover, |key| format!("group.color.{key}")),
        "group-color-grid",
    );
    {
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        let popover = popover.clone();
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk4::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
        row.add_controller(gesture);
    }

    // Actions
    let ag = gtk4::gio::SimpleActionGroup::new();
    {
        let collapse = gtk4::gio::SimpleAction::new("collapse", None);
        let state = state.clone();
        collapse.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.set_group_collapsed(group_id, None);
            drop(tm);
            state.shared.notify_ui_refresh();
        });
        ag.add_action(&collapse);
    }
    {
        let rename = gtk4::gio::SimpleAction::new("rename", None);
        let state = state.clone();
        let row_weak = row.downgrade();
        let current = group.name.clone();
        rename.connect_activate(move |_, _| {
            if let Some(row) = row_weak.upgrade() {
                if let Some(root) = row.root() {
                    if let Some(window) = root.downcast_ref::<libadwaita::ApplicationWindow>() {
                        show_group_rename(window, &state, group_id, &current);
                    }
                }
            }
        });
        ag.add_action(&rename);
    }
    for color in &[
        "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink", "",
    ] {
        let action = gtk4::gio::SimpleAction::new(&format!("color.{color}"), None);
        let state = state.clone();
        let color_value = if color.is_empty() {
            None
        } else {
            Some(color.to_string())
        };
        action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.set_group_color(group_id, color_value.clone());
            drop(tm);
            state.shared.notify_ui_refresh();
        });
        ag.add_action(&action);
    }
    {
        let new_ws = gtk4::gio::SimpleAction::new("new-ws", None);
        let state = state.clone();
        new_ws.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            let win = tm.group(group_id).and_then(|g| g.window_id);
            let mut ws = crate::model::Workspace::new();
            ws.window_id = win;
            let ws_id = tm.add_workspace(ws);
            tm.assign_to_group(ws_id, Some(group_id));
            drop(tm);
            state.shared.notify_ui_refresh();
        });
        ag.add_action(&new_ws);
    }
    {
        let delete = gtk4::gio::SimpleAction::new("delete", None);
        let state = state.clone();
        delete.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.remove_group(group_id);
            drop(tm);
            state.shared.notify_ui_refresh();
        });
        ag.add_action(&delete);
    }
    row.insert_action_group("group", Some(&ag));

    // Drag source — a group header carries "group:<uuid>" so it can be dropped
    // on a workspace row or another group header to reorder the whole group.
    {
        let drag_source = gtk4::DragSource::new();
        drag_source.set_actions(gdk4::DragAction::MOVE);
        let payload = format!("group:{group_id}");
        drag_source.connect_prepare(move |_s, _x, _y| {
            Some(gdk4::ContentProvider::for_value(&payload.to_value()))
        });
        row.add_controller(drag_source);
    }

    // Drop target — accept a group (reorder before this group) or a workspace
    // (add it to this group).
    {
        let drop_target = gtk4::DropTarget::new(glib::Type::STRING, gdk4::DragAction::MOVE);
        let state = state.clone();
        drop_target.connect_drop(move |_t, value, _x, _y| {
            let Ok(s) = value.get::<String>() else {
                return false;
            };
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            // The workspace this group is anchored at (its first member).
            let first_member = tm
                .iter()
                .find(|w| w.group_id == Some(group_id))
                .map(|w| w.id);
            if let Some(gid) = s
                .strip_prefix("group:")
                .and_then(|x| uuid::Uuid::parse_str(x).ok())
            {
                if gid == group_id {
                    return false;
                }
                let moved = tm.move_group_before(gid, first_member);
                drop(tm);
                if moved {
                    state.shared.notify_ui_refresh();
                }
                return moved;
            }
            // Workspace index payload → add that workspace to this group.
            if let Ok(idx) = s.parse::<usize>() {
                if let Some(ws_id) = tm.get(idx).map(|w| w.id) {
                    let ok = tm.assign_to_group(ws_id, Some(group_id));
                    drop(tm);
                    if ok {
                        state.shared.notify_ui_refresh();
                    }
                    return ok;
                }
            }
            false
        });
        row.add_controller(drop_target);
    }

    row
}

/// Show a small dialog to rename a workspace group.
fn show_group_rename(
    window: &libadwaita::ApplicationWindow,
    state: &Rc<AppState>,
    group_id: uuid::Uuid,
    current_name: &str,
) {
    let state = state.clone();
    crate::ui::window::dialogs::present_rename_dialog(
        window,
        "Rename Group",
        None,
        current_name,
        move |name| {
            if name.trim().is_empty() {
                return;
            }
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.rename_group(group_id, name);
            drop(tm);
            state.shared.notify_ui_refresh();
        },
    );
}

/// Set up right-click context menu on a sidebar row.
#[allow(clippy::too_many_arguments)]
fn setup_row_context_menu(
    row: &gtk4::ListBoxRow,
    index: usize,
    is_pinned: bool,
    window_id: Option<uuid::Uuid>,
    workspace_id: uuid::Uuid,
    has_remote: bool,
    current_group_id: Option<uuid::Uuid>,
    all_workspaces: &[(usize, uuid::Uuid, String)],
    all_groups: &[(uuid::Uuid, String)],
    state: &Rc<AppState>,
) {
    let menu = gtk4::gio::Menu::new();
    menu.append(
        Some(if is_pinned { "Unpin" } else { "Pin" }),
        Some(&format!("sidebar.toggle-pin.{index}")),
    );
    menu.append(Some("Rename"), Some(&format!("sidebar.rename.{index}")));

    // Color submenu — 16-color palette matching macOS + custom color picker.
    // The palette is a live swatch grid (custom popover child) so you see the
    // actual colour, not just its name — GTK4 popover menus ignore per-item
    // icons. The swatches activate the `sidebar.color.{index}.*` actions below.
    let color_menu = gtk4::gio::Menu::new();
    let grid_item = gtk4::gio::MenuItem::new(None, None);
    grid_item.set_attribute_value("custom", Some(&"ws-color-grid".to_variant()));
    color_menu.append_item(&grid_item);
    color_menu.append(
        Some("Custom Color…"),
        Some(&format!("sidebar.custom-color.{index}")),
    );
    menu.append_submenu(Some("Set Color"), &color_menu);

    menu.append(
        Some("Mark as Read"),
        Some(&format!("sidebar.mark-read.{index}")),
    );
    menu.append(
        Some("Mark as Unread"),
        Some(&format!("sidebar.mark-unread.{index}")),
    );

    // Reorder submenu
    let reorder_menu = gtk4::gio::Menu::new();
    reorder_menu.append(
        Some("Move to Top"),
        Some(&format!("sidebar.move-top.{index}")),
    );
    reorder_menu.append(Some("Move Up"), Some(&format!("sidebar.move-up.{index}")));
    reorder_menu.append(
        Some("Move Down"),
        Some(&format!("sidebar.move-down.{index}")),
    );
    menu.append_section(None, &reorder_menu);

    // Move Focused Pane submenu — "to New Workspace" + flat list of other workspaces
    {
        let move_pane_menu = gtk4::gio::Menu::new();
        move_pane_menu.append(
            Some("Move to New Workspace"),
            Some(&format!("sidebar.move-pane-new.{index}")),
        );

        // Collect other workspaces for the flat list (pre-collected by caller to avoid re-locking)
        let other_workspaces: Vec<(uuid::Uuid, String)> = all_workspaces
            .iter()
            .filter(|(i, _, _)| *i != index)
            .map(|(i, id, title)| (*id, format!("Move to: {} ({})", title, i + 1)))
            .collect();
        for (wid, label) in &other_workspaces {
            move_pane_menu.append(
                Some(label),
                Some(&format!("sidebar.move-pane-to.{index}.{wid}")),
            );
        }
        menu.append_section(None, &move_pane_menu);
    }

    // Move to Window submenu (only when multiple windows exist)
    let window_ids: Vec<uuid::Uuid> = lock_or_recover(&state.shared.window_sizes)
        .keys()
        .copied()
        .collect();
    if window_ids.len() > 1 {
        let window_menu = gtk4::gio::Menu::new();
        for (i, wid) in window_ids.iter().enumerate() {
            let is_current = window_id == Some(*wid);
            let label = if is_current {
                format!("Window {} (current)", i + 1)
            } else {
                format!("Window {}", i + 1)
            };
            window_menu.append(
                Some(&label),
                Some(&format!("sidebar.move-to-window.{index}.{wid}")),
            );
        }
        menu.append_submenu(Some("Move to Window"), &window_menu);
    }

    // Remote SSH section (only for remote workspaces)
    if has_remote {
        let remote_menu = gtk4::gio::Menu::new();
        remote_menu.append(
            Some("Reconnect"),
            Some(&format!("sidebar.remote-reconnect.{index}")),
        );
        remote_menu.append(
            Some("Disconnect"),
            Some(&format!("sidebar.remote-disconnect.{index}")),
        );
        menu.append_section(None, &remote_menu);
    }

    // Group section — assign to an existing group, create a new one, or remove.
    {
        let group_menu = gtk4::gio::Menu::new();
        group_menu.append(
            Some("New Group…"),
            Some(&format!("sidebar.new-group.{index}")),
        );
        for (gid, name) in all_groups {
            if current_group_id == Some(*gid) {
                continue;
            }
            group_menu.append(
                Some(&format!("Add to: {name}")),
                Some(&format!("sidebar.add-to-group.{index}.{gid}")),
            );
        }
        if current_group_id.is_some() {
            group_menu.append(
                Some("Remove from Group"),
                Some(&format!("sidebar.remove-from-group.{index}")),
            );
        }
        menu.append_submenu(Some("Group"), &group_menu);
    }

    // Agent hibernation toggle. The label is static (a toggle) because this
    // menu is built while `refresh_sidebar` already holds the tab_manager lock;
    // re-locking it here would deadlock the non-reentrant std::sync::Mutex.
    {
        let hib_menu = gtk4::gio::Menu::new();
        hib_menu.append(
            Some("Hibernate / Wake Agent"),
            Some(&format!("sidebar.hibernate.{index}")),
        );
        menu.append_section(None, &hib_menu);
    }

    // Close section
    let close_menu = gtk4::gio::Menu::new();
    close_menu.append(Some("Close"), Some(&format!("sidebar.close.{index}")));
    close_menu.append(
        Some("Close Others"),
        Some(&format!("sidebar.close-others.{index}")),
    );
    close_menu.append(
        Some("Close Above"),
        Some(&format!("sidebar.close-above.{index}")),
    );
    close_menu.append(
        Some("Close Below"),
        Some(&format!("sidebar.close-below.{index}")),
    );
    menu.append_section(None, &close_menu);

    let popover = gtk4::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(row);
    popover.set_has_arrow(false);
    popover.add_child(
        &build_color_swatch_grid(PALETTE_COLORS, &popover, move |key| {
            format!("sidebar.color.{index}.{key}")
        }),
        "ws-color-grid",
    );

    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3); // Right click
    {
        let popover = popover.clone();
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk4::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    row.add_controller(gesture);

    // Actions
    let action_group = gtk4::gio::SimpleActionGroup::new();

    // Toggle pin
    let pin_action = gtk4::gio::SimpleAction::new(&format!("toggle-pin.{index}"), None);
    {
        let state = state.clone();
        pin_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.get_mut(index) {
                ws.is_pinned = !ws.is_pinned;
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&pin_action);

    // Rename
    let rename_action = gtk4::gio::SimpleAction::new(&format!("rename.{index}"), None);
    {
        let state = state.clone();
        let row_weak = row.downgrade();
        rename_action.connect_activate(move |_, _| {
            let current_title = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.get(index).map(|ws| ws.display_title().to_string())
            };
            if let Some(title) = current_title {
                if let Some(row) = row_weak.upgrade() {
                    if let Some(root) = row.root() {
                        if let Some(window) = root.downcast_ref::<libadwaita::ApplicationWindow>() {
                            show_rename_for_index(window, &state, index, &title);
                        }
                    }
                }
            }
        });
    }
    action_group.add_action(&rename_action);

    // Color actions — 16-color palette matching macOS + "" for clear
    for color in &[
        "red", "crimson", "orange", "amber", "yellow", "lime", "green", "teal", "cyan", "sky",
        "blue", "indigo", "purple", "violet", "pink", "rose", "",
    ] {
        let action_name = format!("color.{index}.{color}");
        let color_action = gtk4::gio::SimpleAction::new(&action_name, None);
        let color_value = if color.is_empty() {
            None
        } else {
            Some(color_css_value(color).to_string())
        };
        {
            let state = state.clone();
            color_action.connect_activate(move |_, _| {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(ws) = tm.get_mut(index) {
                    ws.custom_color = color_value.clone();
                }
                drop(tm);
                state.shared.notify_ui_refresh();
            });
        }
        action_group.add_action(&color_action);
    }

    // Custom color picker
    let custom_color_action = gtk4::gio::SimpleAction::new(&format!("custom-color.{index}"), None);
    {
        let state = state.clone();
        let row_weak = row.downgrade();
        custom_color_action.connect_activate(move |_, _| {
            if let Some(row) = row_weak.upgrade() {
                if let Some(root) = row.root() {
                    if let Some(window) = root.downcast_ref::<libadwaita::ApplicationWindow>() {
                        show_custom_color_picker(window, &state, index);
                    }
                }
            }
        });
    }
    action_group.add_action(&custom_color_action);

    // Mark read
    let mark_read_action = gtk4::gio::SimpleAction::new(&format!("mark-read.{index}"), None);
    {
        let state = state.clone();
        mark_read_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.get_mut(index) {
                ws.mark_notifications_read();
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&mark_read_action);

    // Mark unread
    let mark_unread_action = gtk4::gio::SimpleAction::new(&format!("mark-unread.{index}"), None);
    {
        let state = state.clone();
        mark_unread_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.get_mut(index) {
                ws.unread_count = ws.unread_count.max(1);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&mark_unread_action);

    // Move to top
    let move_top_action = gtk4::gio::SimpleAction::new(&format!("move-top.{index}"), None);
    {
        let state = state.clone();
        move_top_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.move_workspace(index, 0);
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&move_top_action);

    // Move up
    let move_up_action = gtk4::gio::SimpleAction::new(&format!("move-up.{index}"), None);
    {
        let state = state.clone();
        move_up_action.connect_activate(move |_, _| {
            if index > 0 {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                tm.move_workspace(index, index - 1);
                drop(tm);
                state.shared.notify_ui_refresh();
            }
        });
    }
    action_group.add_action(&move_up_action);

    // Move down
    let move_down_action = gtk4::gio::SimpleAction::new(&format!("move-down.{index}"), None);
    {
        let state = state.clone();
        move_down_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            let len = tm.len();
            if index + 1 < len {
                tm.move_workspace(index, index + 1);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&move_down_action);

    // Move focused pane to new workspace
    let move_pane_new_action =
        gtk4::gio::SimpleAction::new(&format!("move-pane-new.{index}"), None);
    {
        let state = state.clone();
        move_pane_new_action.connect_activate(move |_, _| {
            use crate::model::panel::LayoutNode;
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            let source = tm.get_mut(index);
            let Some(source) = source else { return };
            let Some(panel_id) = source.focused_panel_id else {
                return;
            };
            if source.panels.len() <= 1 {
                return;
            }
            let source_dir = source.current_directory.clone();
            if let Some(panel) = source.detach_panel(panel_id) {
                let source_ws_id = source.id;
                if source.is_empty() {
                    tm.remove_by_id(source_ws_id);
                }
                let mut new_ws = crate::model::Workspace::new();
                // Remove the default panel that Workspace::new() creates
                let default_pid = new_ws.focused_panel_id;
                if let Some(dpid) = default_pid {
                    new_ws.panels.remove(&dpid);
                }
                new_ws.current_directory = source_dir;
                new_ws.panels.insert(panel_id, panel);
                new_ws.layout = LayoutNode::single_pane(panel_id);
                new_ws.focused_panel_id = Some(panel_id);
                let placement = crate::settings::load().new_workspace_placement;
                tm.add_workspace_with_placement(new_ws, placement);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&move_pane_new_action);

    // Move focused pane to an existing workspace
    {
        let other_workspace_ids: Vec<uuid::Uuid> = all_workspaces
            .iter()
            .filter(|(i, _, _)| *i != index)
            .map(|(_, id, _)| *id)
            .collect();
        for target_wid in other_workspace_ids {
            let action_name = format!("move-pane-to.{index}.{target_wid}");
            let move_pane_action = gtk4::gio::SimpleAction::new(&action_name, None);
            {
                let state = state.clone();
                move_pane_action.connect_activate(move |_, _| {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    let panel_id = tm.get(index).and_then(|ws| ws.focused_panel_id);
                    let Some(panel_id) = panel_id else { return };
                    tm.move_panel_to_workspace(panel_id, target_wid);
                    drop(tm);
                    state.shared.notify_ui_refresh();
                });
            }
            action_group.add_action(&move_pane_action);
        }
    }

    // Move to window actions
    for wid in &window_ids {
        let action_name = format!("move-to-window.{index}.{wid}");
        let move_window_action = gtk4::gio::SimpleAction::new(&action_name, None);
        let target_wid = *wid;
        {
            let state = state.clone();
            move_window_action.connect_activate(move |_, _| {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                if let Some(ws) = tm.get_mut(index) {
                    ws.window_id = Some(target_wid);
                }
                drop(tm);
                state.shared.notify_ui_refresh();
            });
        }
        action_group.add_action(&move_window_action);
    }

    // Close
    let close_action = gtk4::gio::SimpleAction::new(&format!("close.{index}"), None);
    {
        let state = state.clone();
        let row_weak = row.downgrade();
        close_action.connect_activate(move |_, _| {
            let (is_pinned, title, has_terminals) = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.get(index)
                    .map(|ws| {
                        use crate::model::panel::PanelType;
                        let has_terminals =
                            ws.panels.values().any(|p| p.panel_type == PanelType::Terminal);
                        (ws.is_pinned, ws.display_title().to_string(), has_terminals)
                    })
                    .unwrap_or((false, String::new(), false))
            };
            if let Some(row) = row_weak.upgrade() {
                if let Some(root) = row.root() {
                    if let Some(window) = root.downcast_ref::<libadwaita::ApplicationWindow>() {
                        if is_pinned {
                            show_close_pinned_dialog(window, &state, index, &title);
                            return;
                        }
                        let warn = crate::settings::load().warn_before_closing_tab;
                        if warn && has_terminals {
                            show_close_tab_dialog(window, &state, index, &title);
                            return;
                        }
                    }
                }
            }
            lock_or_recover(&state.shared.tab_manager).remove(index);
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&close_action);

    // Close others
    let close_others_action = gtk4::gio::SimpleAction::new(&format!("close-others.{index}"), None);
    {
        let state = state.clone();
        close_others_action.connect_activate(move |_, _| {
            let ws_id = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.get(index).map(|ws| ws.id)
            };
            if let Some(id) = ws_id {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                tm.close_others(id);
                drop(tm);
                state.shared.notify_ui_refresh();
            }
        });
    }
    action_group.add_action(&close_others_action);

    // Close above
    let close_above_action = gtk4::gio::SimpleAction::new(&format!("close-above.{index}"), None);
    {
        let state = state.clone();
        close_above_action.connect_activate(move |_, _| {
            let ws_id = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.get(index).map(|ws| ws.id)
            };
            if let Some(id) = ws_id {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                tm.close_above(id);
                drop(tm);
                state.shared.notify_ui_refresh();
            }
        });
    }
    action_group.add_action(&close_above_action);

    // Close below
    let close_below_action = gtk4::gio::SimpleAction::new(&format!("close-below.{index}"), None);
    {
        let state = state.clone();
        close_below_action.connect_activate(move |_, _| {
            let ws_id = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.get(index).map(|ws| ws.id)
            };
            if let Some(id) = ws_id {
                let mut tm = lock_or_recover(&state.shared.tab_manager);
                tm.close_below(id);
                drop(tm);
                state.shared.notify_ui_refresh();
            }
        });
    }
    action_group.add_action(&close_below_action);

    // Remote SSH actions (only registered when this is a remote workspace)
    if has_remote {
        let reconnect_action =
            gtk4::gio::SimpleAction::new(&format!("remote-reconnect.{index}"), None);
        {
            let state = state.clone();
            reconnect_action.connect_activate(move |_, _| {
                state
                    .shared
                    .send_ui_event(crate::app::UiEvent::RemoteDisconnect { workspace_id });
                state
                    .shared
                    .send_ui_event(crate::app::UiEvent::RemoteConnect { workspace_id });
            });
        }
        action_group.add_action(&reconnect_action);

        let disconnect_action =
            gtk4::gio::SimpleAction::new(&format!("remote-disconnect.{index}"), None);
        {
            let state = state.clone();
            disconnect_action.connect_activate(move |_, _| {
                state
                    .shared
                    .send_ui_event(crate::app::UiEvent::RemoteDisconnect { workspace_id });
            });
        }
        action_group.add_action(&disconnect_action);
    }

    // Group: new group (assign this workspace to a freshly created group)
    {
        let new_group_action = gtk4::gio::SimpleAction::new(&format!("new-group.{index}"), None);
        let state = state.clone();
        new_group_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            let win = tm.workspace(workspace_id).and_then(|ws| ws.window_id);
            let gid = tm.create_group("New Group", win);
            tm.assign_to_group(workspace_id, Some(gid));
            drop(tm);
            state.shared.notify_ui_refresh();
        });
        action_group.add_action(&new_group_action);
    }

    // Group: add to an existing group
    for (gid, _name) in all_groups {
        if current_group_id == Some(*gid) {
            continue;
        }
        let action_name = format!("add-to-group.{index}.{gid}");
        let add_action = gtk4::gio::SimpleAction::new(&action_name, None);
        let target_gid = *gid;
        let state = state.clone();
        add_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.assign_to_group(workspace_id, Some(target_gid));
            drop(tm);
            state.shared.notify_ui_refresh();
        });
        action_group.add_action(&add_action);
    }

    // Group: remove from current group
    if current_group_id.is_some() {
        let remove_action =
            gtk4::gio::SimpleAction::new(&format!("remove-from-group.{index}"), None);
        let state = state.clone();
        remove_action.connect_activate(move |_, _| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.assign_to_group(workspace_id, None);
            drop(tm);
            state.shared.notify_ui_refresh();
        });
        action_group.add_action(&remove_action);
    }

    // Hibernate / wake the workspace's focused agent.
    {
        let hibernate_action = gtk4::gio::SimpleAction::new(&format!("hibernate.{index}"), None);
        let state = state.clone();
        hibernate_action.connect_activate(move |_, _| {
            let pid = lock_or_recover(&state.shared.tab_manager)
                .workspace(workspace_id)
                .and_then(|ws| ws.focused_panel_id);
            if let Some(pid) = pid {
                if state.shared.is_hibernated(&pid) {
                    state.shared.wake_panel(pid);
                } else {
                    state.shared.hibernate_panel(pid);
                }
                state.shared.notify_ui_refresh();
            }
        });
        action_group.add_action(&hibernate_action);
    }

    row.insert_action_group("sidebar", Some(&action_group));
}

/// Show a color chooser dialog for custom workspace color.
fn show_custom_color_picker(
    window: &libadwaita::ApplicationWindow,
    state: &Rc<AppState>,
    index: usize,
) {
    use gtk4::prelude::*;
    use libadwaita::prelude::*;

    // In-surface dialog (renders above the layer-shell quick-terminal overlay)
    // hosting an embeddable color chooser widget.
    let dialog = libadwaita::AlertDialog::new(Some("Choose Workspace Color"), None);
    let chooser = gtk4::ColorChooserWidget::new();
    chooser.set_use_alpha(false);

    // Pre-select the current custom color if set
    let current_color = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.get(index).and_then(|ws| ws.custom_color.clone())
    };
    if let Some(ref css_color) = current_color {
        let rgba = gdk4::RGBA::parse(css_color).unwrap_or(gdk4::RGBA::BLACK);
        chooser.set_rgba(&rgba);
    }
    dialog.set_extra_child(Some(&chooser));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("select", "Select");
    dialog.set_default_response(Some("select"));
    dialog.set_response_appearance("select", libadwaita::ResponseAppearance::Suggested);

    let state = state.clone();
    let chooser_cb = chooser.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "select" {
            let rgba = chooser_cb.rgba();
            let css = format!(
                "#{:02x}{:02x}{:02x}",
                (rgba.red() * 255.0) as u8,
                (rgba.green() * 255.0) as u8,
                (rgba.blue() * 255.0) as u8,
            );
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.get_mut(index) {
                ws.custom_color = Some(css);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        }
    });

    dialog.present(Some(window));
}

fn show_rename_for_index(
    window: &libadwaita::ApplicationWindow,
    state: &Rc<AppState>,
    index: usize,
    current_title: &str,
) {
    let state = state.clone();
    crate::ui::window::dialogs::present_rename_dialog(
        window,
        "Rename Workspace",
        Some("Enter a new name for this workspace:"),
        current_title,
        move |new_name| {
            if new_name.is_empty() {
                return;
            }
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.get_mut(index) {
                ws.custom_title = Some(new_name);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        },
    );
}

/// Show a confirmation dialog before closing a non-pinned workspace that has active terminals.
fn show_close_tab_dialog(
    window: &libadwaita::ApplicationWindow,
    state: &Rc<AppState>,
    index: usize,
    title: &str,
) {
    use libadwaita::prelude::*;

    let heading = format!("Close '{title}'?");
    let dialog = libadwaita::AlertDialog::new(
        Some(&heading),
        Some("This workspace has active terminals. Are you sure you want to close it?"),
    );
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("close", "Close");
    dialog.set_default_response(Some("cancel"));
    dialog.set_response_appearance("close", libadwaita::ResponseAppearance::Destructive);

    let state = state.clone();
    dialog.connect_response(None::<&str>, move |_, response| {
        if response == "close" {
            lock_or_recover(&state.shared.tab_manager).remove(index);
            state.shared.notify_ui_refresh();
        }
    });
    dialog.present(Some(window));
}

/// Show a confirmation dialog before closing a pinned workspace.
fn show_close_pinned_dialog(
    window: &libadwaita::ApplicationWindow,
    state: &Rc<AppState>,
    index: usize,
    title: &str,
) {
    use libadwaita::prelude::*;

    let heading = format!("Close '{title}'?");
    let dialog = libadwaita::AlertDialog::new(
        Some(&heading),
        Some("This workspace is pinned. Are you sure you want to close it?"),
    );
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("close", "Close");
    dialog.set_default_response(Some("cancel"));
    dialog.set_response_appearance("close", libadwaita::ResponseAppearance::Destructive);

    let state = state.clone();
    dialog.connect_response(None::<&str>, move |_, response| {
        if response == "close" {
            lock_or_recover(&state.shared.tab_manager).remove(index);
            state.shared.notify_ui_refresh();
        }
    });
    dialog.present(Some(window));
}

/// The 16-color workspace palette (macOS-style) + a trailing "None" clear
/// entry. Each pair is (display label, colour key); the key maps to a hex via
/// [`color_css_value`] and to an action suffix.
const PALETTE_COLORS: &[(&str, &str)] = &[
    ("Red", "red"),
    ("Crimson", "crimson"),
    ("Orange", "orange"),
    ("Amber", "amber"),
    ("Yellow", "yellow"),
    ("Lime", "lime"),
    ("Green", "green"),
    ("Teal", "teal"),
    ("Cyan", "cyan"),
    ("Sky", "sky"),
    ("Blue", "blue"),
    ("Indigo", "indigo"),
    ("Purple", "purple"),
    ("Violet", "violet"),
    ("Pink", "pink"),
    ("Rose", "rose"),
    ("None", ""),
];

/// The smaller group-header palette (all CSS-keyword colours) + "None".
const GROUP_COLORS: &[(&str, &str)] = &[
    ("Red", "red"),
    ("Orange", "orange"),
    ("Yellow", "yellow"),
    ("Green", "green"),
    ("Teal", "teal"),
    ("Blue", "blue"),
    ("Purple", "purple"),
    ("Pink", "pink"),
    ("None", ""),
];

/// Build a grid of colour swatches to embed in a `PopoverMenu` as a custom
/// child (via a menu item's `custom` attribute). GTK4 popover menus ignore
/// per-item icons, so this is how "Set Color" shows an actual swatch per colour
/// — purple for Purple — instead of a bare list of names. Clicking a swatch
/// activates `action_for(key)` on the widget hierarchy (the same actions a
/// plain menu item would trigger) and dismisses the menu. The `""` key renders
/// as a "no colour" swatch.
fn build_color_swatch_grid(
    colors: &[(&str, &str)],
    popover: &gtk4::PopoverMenu,
    action_for: impl Fn(&str) -> String,
) -> gtk4::Widget {
    let flow = gtk4::FlowBox::new();
    flow.set_selection_mode(gtk4::SelectionMode::None);
    flow.set_min_children_per_line(8);
    flow.set_max_children_per_line(8);
    flow.set_column_spacing(4);
    flow.set_row_spacing(4);
    flow.set_margin_start(8);
    flow.set_margin_end(8);
    flow.set_margin_top(6);
    flow.set_margin_bottom(6);
    flow.set_can_focus(false);
    flow.add_css_class("color-swatch-grid");

    for (label, key) in colors {
        let btn = gtk4::Button::new();
        btn.add_css_class("color-swatch");
        btn.set_tooltip_text(Some(label));
        btn.set_size_request(22, 22);
        if key.is_empty() {
            btn.add_css_class("color-swatch-none");
        } else {
            // Static class defined in the global stylesheet — no per-widget
            // CssProvider, which would force a global style revalidation.
            btn.add_css_class(&format!("color-swatch-{key}"));
        }
        let action = action_for(key);
        let popover = popover.clone();
        btn.connect_clicked(move |b| {
            let _ = b.activate_action(&action, None);
            popover.popdown();
        });
        flow.append(&btn);
    }
    flow.upcast()
}

fn color_css_value(name: &str) -> &str {
    match name {
        "red" => "#e01b24",
        "crimson" => "#dc143c",
        "orange" => "#ff7800",
        "amber" => "#ffbf00",
        "yellow" => "#f6d32d",
        "lime" => "#a3be8c",
        "green" => "#33d17a",
        "teal" => "#2aa198",
        "cyan" => "#00bcd4",
        "sky" => "#87ceeb",
        "blue" => "#3584e4",
        "indigo" => "#4b0082",
        "purple" => "#9141ac",
        "violet" => "#7c3aed",
        "pink" => "#e91e8c",
        "rose" => "#f43f5e",
        _ => "",
    }
}

/// Wire up the hover close button on a row.
fn setup_row_close_button(row: &gtk4::ListBoxRow, index: usize, state: &Rc<AppState>) {
    // Find the close button (it's the last child in the header box)
    let Some(outer) = row_outer_box(row) else { return };
    let Some(header) = outer.first_child() else {
        return;
    };
    let header = header.downcast_ref::<gtk4::Box>().cloned();
    let Some(header) = header else { return };

    // Walk to find the button
    let mut child = header.first_child();
    while let Some(c) = child {
        if c.has_css_class("sidebar-close-btn") {
            if let Some(btn) = c.downcast_ref::<gtk4::Button>() {
                let state = state.clone();
                btn.connect_clicked(move |btn| {
                    let (is_pinned, title) = {
                        let tm = lock_or_recover(&state.shared.tab_manager);
                        tm.get(index)
                            .map(|ws| (ws.is_pinned, ws.display_title().to_string()))
                            .unwrap_or((false, String::new()))
                    };
                    if is_pinned {
                        if let Some(root) = btn.root() {
                            if let Some(window) =
                                root.downcast_ref::<libadwaita::ApplicationWindow>()
                            {
                                show_close_pinned_dialog(window, &state, index, &title);
                                return;
                            }
                        }
                    }
                    lock_or_recover(&state.shared.tab_manager).remove(index);
                    state.shared.notify_ui_refresh();
                });
            }
            break;
        }
        child = c.next_sibling();
    }
}

fn workspace_meta_text(workspace: &Workspace, sidebar: &SidebarDisplaySettings) -> String {
    let mut parts = Vec::new();

    if !sidebar.hide_all_details {
        if let Some(status) = workspace.sidebar_status_label() {
            parts.push(status.to_string());
        }

        if sidebar.show_git_branch {
            if let Some(git_branch) = &workspace.git_branch {
                parts.push(if git_branch.is_dirty {
                    format!("git {} *", git_branch.branch)
                } else {
                    format!("git {}", git_branch.branch)
                });
            }
        }

        if sidebar.show_directory {
            parts.push(compact_path(&workspace.current_directory));
        }
    }

    parts.join(" | ")
}

pub fn compact_path(path: &str) -> String {
    if path.is_empty() {
        return "~".to_string();
    }

    if let Ok(home) = std::env::var("HOME") {
        // Guard against HOME="/" where strip_prefix would match any absolute path
        if home != "/" {
            let p = Path::new(path);
            if let Ok(stripped) = p.strip_prefix(&home) {
                let s = stripped.display();
                return if stripped.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    format!("~/{s}")
                };
            }
        }
    }

    let path = Path::new(path);
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        return name.to_string();
    }

    path.to_string_lossy().into_owned()
}
