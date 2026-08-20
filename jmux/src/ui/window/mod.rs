//! Main application window using AdwNavigationSplitView.

pub(crate) mod dialogs;
mod event_handler;
mod shortcuts;
mod styling;

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::app::{lock_or_recover, AppState, UiEvent};
use crate::model::panel::{Panel, SplitOrientation};
use crate::model::{PanelType, Workspace};
use crate::ui::{notifications_panel, search_overlay, sidebar, split_view};

// Re-export the public dialog function.
pub use dialogs::show_rename_tab_dialog;

/// Create an application window with per-window ID for multi-window support.
pub fn create_window(
    app: &adw::Application,
    state: &Rc<AppState>,
    window_id: uuid::Uuid,
    ui_events: UnboundedReceiver<UiEvent>,
    chromeless: bool,
) -> adw::ApplicationWindow {
    styling::install_css();

    // Use saved window geometry if available, otherwise defaults
    let (width, height) = lock_or_recover(&state.shared.window_sizes)
        .get(&window_id)
        .copied()
        .unwrap_or((1280, 860));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("jmux")
        .default_width(width)
        .default_height(height)
        .build();
    window.set_widget_name(&window_id.to_string());

    let split_view = adw::NavigationSplitView::new();
    let sidebar_settings = &crate::settings::load().sidebar;
    let sidebar_width = if sidebar_settings.width > 0 {
        sidebar_settings.width as f64
    } else {
        280.0
    };
    // Honor the configured width as the actual maximum so it doesn't get forced
    // wider on low-resolution screens. Let it shrink below that on narrow windows.
    split_view.set_min_sidebar_width(sidebar_width.min(180.0));
    split_view.set_max_sidebar_width(sidebar_width);
    split_view.set_vexpand(true);
    split_view.set_hexpand(true);

    let sidebar_widgets = sidebar::create_sidebar(state);
    let list_box = sidebar_widgets.list_box.clone();
    let sidebar_page = adw::NavigationPage::new(&sidebar_widgets.root, "Workspaces");
    split_view.set_sidebar(Some(&sidebar_page));

    let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content_box.set_hexpand(true);
    content_box.set_vexpand(true);
    rebuild_content(&content_box, state, Some(window_id));

    // Search overlay wraps the content area
    let search = search_overlay::create_search_overlay(&content_box.clone().upcast(), state);
    let search_bar = search.search_bar.clone();
    let search_entry = search.entry.clone();
    let search_count_label = search.count_label.clone();
    let search_state = search.state.clone();

    // Dock — right-side terminal controls from dock.json, placed beside the
    // workspace content.
    let dock_dir = {
        let tm = crate::app::lock_or_recover(&state.shared.tab_manager);
        tm.selected()
            .map(|ws| ws.current_directory.clone())
            .unwrap_or_default()
    };
    let dock_box = crate::ui::dock::create_dock(window_id, &dock_dir, state);
    let content_with_dock = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    content_with_dock.set_hexpand(true);
    content_with_dock.set_vexpand(true);
    search.overlay.set_hexpand(true);
    content_with_dock.append(&search.overlay);
    content_with_dock.append(&dock_box);

    let content_page = adw::NavigationPage::new(&content_with_dock, "Terminal");
    split_view.set_content(Some(&content_page));

    // Notification panel — replaces sidebar when toggled
    let notif_panel = notifications_panel::create_notifications_panel(state);
    let notif_root = notif_panel.root.clone();
    let notif_page = adw::NavigationPage::new(&notif_root, "Notifications");
    let showing_notifications: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // Toast overlay must be created before binding events so it can be passed in
    let toast_overlay = adw::ToastOverlay::new();

    let header = adw::HeaderBar::new();
    // Minimal mode hides the header bar entirely for a distraction-free terminal.
    if crate::settings::load().minimal_mode {
        header.set_visible(false);
    }
    // A chromeless (quick-terminal) window keeps the full UI — sidebar, header,
    // tabs and global buttons — but drops the OS-style window controls
    // (close/maximize), since a layer-shell drop-down isn't a normal window.
    if chromeless {
        header.set_show_start_title_buttons(false);
        header.set_show_end_title_buttons(false);
    }

    bind_sidebar_selection(&list_box, &content_box, state);
    event_handler::bind_shared_state_updates(
        &list_box,
        &content_box,
        &window,
        state,
        ui_events,
        &split_view,
        &sidebar_page,
        &notif_page,
        &showing_notifications,
        &notif_panel,
        &toast_overlay,
        &header,
    );
    let initial_title = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.selected()
            .map(|ws| ws.display_title().to_string())
            .unwrap_or_else(|| "jmux".to_string())
    };
    let header_title = gtk4::Label::new(Some(&initial_title));
    header_title.add_css_class("heading");
    header_title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    header.set_title_widget(Some(&header_title));

    let new_ws_btn = gtk4::Button::from_icon_name("tab-new-symbolic");
    new_ws_btn.set_tooltip_text(Some("New Workspace"));
    // Never let keyboard focus land here: after a content rebuild drops focus,
    // this button is the first focusable widget, and a stray Space/Enter while
    // typing would open a new tab (same precedent as the pane-tab close button).
    new_ws_btn.set_can_focus(false);
    new_ws_btn.set_focus_on_click(false);
    {
        let state = state.clone();
        let list_box = list_box.clone();
        let content_box = content_box.clone();
        new_ws_btn.connect_clicked(move |_| {
            use crate::settings::PlusButtonAction;
            let settings = crate::settings::load();
            match settings.plus_button_action {
                PlusButtonAction::NewTab => {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        let new_panel = Panel::new_terminal();
                        let new_id = new_panel.id;
                        if let Some(focused_id) = ws.focused_panel_id {
                            ws.layout.add_panel_to_pane(focused_id, new_id);
                        }
                        ws.panels.insert(new_id, new_panel);
                        ws.previous_focused_panel_id = ws.focused_panel_id;
                        ws.focused_panel_id = Some(new_id);
                    }
                    drop(tm);
                    state.shared.notify_ui_refresh();
                }
                PlusButtonAction::NewWorkspace => {
                    let placement = settings.new_workspace_placement;
                    let workspace = if settings.workspace_cwd_inheritance {
                        let cwd = lock_or_recover(&state.shared.tab_manager)
                            .selected()
                            .map(|ws| ws.current_directory.clone())
                            .unwrap_or_default();
                        if cwd.is_empty() {
                            Workspace::new()
                        } else {
                            Workspace::with_directory(&cwd)
                        }
                    } else {
                        Workspace::new()
                    };
                    lock_or_recover(&state.shared.tab_manager)
                        .add_workspace_with_placement(workspace, placement);
                    refresh_ui(&list_box, &content_box, &state);
                }
            }
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

    // Settings button (right side of header bar)
    let settings_btn = gtk4::Button::from_icon_name("preferences-system-symbolic");
    settings_btn.set_tooltip_text(Some("Settings"));
    settings_btn.add_css_class("flat");
    {
        let window_ref = window.clone();
        let list_box = list_box.clone();
        let content_box = content_box.clone();
        let state = Rc::clone(state);
        settings_btn.connect_clicked(move |_| {
            let lb = list_box.clone();
            let cb = content_box.clone();
            let st = Rc::clone(&state);
            let shared = st.shared.clone();
            super::settings::show_settings(&window_ref, shared, move || {
                refresh_ui(&lb, &cb, &st);
            });
        });
    }
    header.pack_end(&settings_btn);

    // Dock toggle button — reliable even when a terminal grabs the keyboard
    // shortcut (Kitty keyboard protocol).
    let dock_btn = gtk4::Button::from_icon_name("sidebar-show-right-symbolic");
    dock_btn.set_tooltip_text(Some("Toggle Dock"));
    dock_btn.add_css_class("flat");
    {
        let state = Rc::clone(state);
        dock_btn.connect_clicked(move |_| {
            let dir = {
                let tm = crate::app::lock_or_recover(&state.shared.tab_manager);
                tm.selected()
                    .map(|ws| ws.current_directory.clone())
                    .unwrap_or_default()
            };
            crate::ui::dock::toggle(window_id, &dir, &state);
        });
    }
    header.pack_end(&dock_btn);

    // Sub-agent monitor toggle — read-only panes tailing Claude's sub-agents.
    let monitor_btn = gtk4::Button::from_icon_name("utilities-system-monitor-symbolic");
    monitor_btn.set_tooltip_text(Some(
        "Toggle sub-agent monitor panes (Ctrl+Shift+S)",
    ));
    monitor_btn.add_css_class("flat");
    // Keep it out of the focus chain — a focused header button turns stray
    // Space/Enter presses into surprise activations while typing (see the
    // new-workspace button fix).
    monitor_btn.set_can_focus(false);
    monitor_btn.set_focus_on_click(false);
    {
        let state = Rc::clone(state);
        monitor_btn.connect_clicked(move |_| {
            crate::agent_monitor::toggle_selected(&state.shared);
        });
    }
    header.pack_end(&monitor_btn);

    // Pane overview button.
    let overview_btn = gtk4::Button::from_icon_name("view-grid-symbolic");
    overview_btn.set_tooltip_text(Some("Pane overview"));
    overview_btn.add_css_class("flat");
    {
        let state = Rc::clone(state);
        let window_ref = window.clone();
        overview_btn.connect_clicked(move |_| {
            crate::ui::pane_overview::show_pane_overview(&window_ref, &state);
        });
    }
    header.pack_end(&overview_btn);

    // Notes button — opens the scope-grouped notes panel beside the current pane.
    let notes_btn = gtk4::Button::from_icon_name("accessories-text-editor-symbolic");
    notes_btn.set_tooltip_text(Some("Notes"));
    notes_btn.add_css_class("flat");
    {
        let state = Rc::clone(state);
        let list_box = list_box.clone();
        let content_box = content_box.clone();
        notes_btn.connect_clicked(move |_| {
            crate::ui::command_palette::insert_notes_panel(&state);
            refresh_ui(&list_box, &content_box, &state);
        });
    }
    header.pack_end(&notes_btn);

    let outer_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    outer_box.append(&header);
    outer_box.append(&split_view);

    toast_overlay.set_child(Some(&outer_box));
    window.set_content(Some(&toast_overlay));
    shortcuts::setup_shortcuts(
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
        &header,
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

    // Close/quit handling
    {
        let state = state.clone();
        window.connect_close_request(move |window| {
            let wid = uuid::Uuid::parse_str(&window.widget_name()).ok();

            // Stop all browser WebViews to prevent WebProcess segfault
            // during shutdown (active content like YouTube embeds can
            // crash if torn down abruptly).
            #[cfg(feature = "webkit")]
            super::browser_panel::stop_all_webviews();

            // Clean up per-window state
            if let Some(ref wid) = wid {
                state.shared.remove_ui_event_sender(wid);
                lock_or_recover(&state.shared.window_sizes).remove(wid);
            }

            // If other windows exist, just close this one without confirmation
            let other_windows = window
                .application()
                .map(|app| app.windows().len() > 1)
                .unwrap_or(false);
            if other_windows {
                return glib::Propagation::Proceed;
            }

            // Last window — check for quit confirmation
            let settings = crate::settings::load();
            if !settings.confirm_before_close || !settings.confirm_quit {
                return glib::Propagation::Proceed;
            }

            let terminal_count = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                tm.iter()
                    .flat_map(|ws| ws.panels.values())
                    .filter(|p| p.panel_type == PanelType::Terminal)
                    .count()
            };

            if terminal_count == 0 {
                return glib::Propagation::Proceed;
            }

            let dialog = adw::AlertDialog::new(Some("Quit jmux?"), None);
            dialog.add_css_class("jmux-confirm-dialog");
            dialog.set_body(&format!(
                "There {} still active. Are you sure you want to quit?",
                if terminal_count == 1 {
                    "is 1 terminal".to_string()
                } else {
                    format!("are {terminal_count} terminals")
                }
            ));
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("quit", "Quit");
            dialog.set_default_response(Some("cancel"));
            dialog.set_response_appearance("quit", adw::ResponseAppearance::Destructive);

            let window_cb = window.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "quit" {
                    window_cb.destroy();
                }
            });

            dialog.present(Some(window));
            glib::Propagation::Stop
        });
    }

    window
}

/// Rebuild the content area from the current workspace layout.
/// The jmux window id (stored as the window's widget name) a widget lives in.
fn window_id_of(widget: &gtk4::Widget) -> Option<uuid::Uuid> {
    widget
        .root()
        .and_then(|r| r.downcast::<adw::ApplicationWindow>().ok())
        .and_then(|w| uuid::Uuid::parse_str(&w.widget_name()).ok())
}

/// The workspace a `content_box` should render: its window's per-window
/// selection, falling back to the global selection. Centralises the
/// `window_id_of` → `selected_for_window` resolution so every per-window render
/// path (focus visuals, title, …) stays consistent — a secondary window like
/// the quake drop-down must not show the main window's workspace.
fn workspace_for<'a>(
    content_box: &gtk4::Box,
    tm: &'a crate::model::TabManager,
) -> Option<&'a Workspace> {
    match window_id_of(content_box.upcast_ref::<gtk4::Widget>()) {
        Some(wid) => tm.selected_for_window(wid),
        None => tm.selected(),
    }
}

thread_local! {
    /// Set by an interactive tab-close path; the next content rebuild grabs
    /// keyboard focus on the active pane's terminal so typing goes straight to
    /// the now-focused (left) tab instead of staying on a button or nowhere.
    static FOCUS_ACTIVE_TERMINAL: Cell<bool> = const { Cell::new(false) };

    /// Every window's content `GtkStack`, weakly — used to find a realized
    /// window to park headless-spawn surfaces in. Weak on purpose: a strong ref
    /// here would keep a closed window's whole widget tree (and its GL
    /// surfaces) alive forever.
    static CONTENT_STACKS: std::cell::RefCell<Vec<glib::WeakRef<gtk4::Stack>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Request that the next `rebuild_content` focus the active pane's terminal.
/// Call from tab-close handlers after closing; the focus grab is deferred to the
/// rebuild so it runs once the (reused) terminal surfaces are re-parented.
pub(crate) fn request_terminal_focus() {
    FOCUS_ACTIVE_TERMINAL.with(|f| f.set(true));
}

/// Grab keyboard focus on the selected workspace's active pane terminal so
/// typing goes straight there. Call only after the content has been (re)built —
/// the terminal surface must already be parented for the grab to take. Used both
/// by the deferred close-path flag below and directly when opening a workspace.
pub(crate) fn focus_active_terminal(state: &Rc<AppState>) {
    let focused = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.selected().and_then(|ws| ws.focused_panel_id)
    };
    if let Some(pid) = focused {
        if let Some(surface) = state.terminal_cache.borrow().get(&pid) {
            surface.grab_focus();
        }
    }
}

/// Name used for the non-workspace stack page (welcome screen / empty state);
/// never a valid workspace UUID, so it's skipped when pruning stale pages.
const NO_WORKSPACE_PAGE: &str = "::no-workspace::";

/// Get the persistent content `GtkStack` that holds one page per workspace,
/// creating it as the sole child of `content_box` on first use.
///
/// Workspaces are switched by making a page visible, never by tearing the tree
/// down — hidden pages stay *realized* (GtkStack only unmaps them), so their GL
/// surfaces are never unrealized and GTK never downloads+orphans the GLArea
/// compositing texture, which used to leak ~4.7 MB on every switch.
fn content_stack(content_box: &gtk4::Box) -> gtk4::Stack {
    if let Some(child) = content_box.first_child() {
        if let Ok(stack) = child.downcast::<gtk4::Stack>() {
            return stack;
        }
    }
    let stack = gtk4::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    // No animation: a workspace switch should be instant, and crossfade would
    // keep the outgoing page mapped (rendering) mid-transition for no benefit.
    stack.set_transition_type(gtk4::StackTransitionType::None);
    content_box.append(&stack);
    CONTENT_STACKS.with(|stacks| {
        let mut stacks = stacks.borrow_mut();
        stacks.retain(|w| w.upgrade().is_some());
        stacks.push(stack.downgrade());
    });
    stack
}

/// Page name of the spawn nursery — see `spawn_nursery`. Not a UUID, so
/// `prune_workspace_pages` leaves it alone.
const HEADLESS_PAGE: &str = "::headless-spawn::";

/// A never-visible `GtkBox` inside a realized window, used to park terminal
/// surfaces that must spawn their command before (or without ever) being shown.
///
/// It is a page of the content `GtkStack` that is never made the visible child,
/// so GTK realizes nothing and allocates nothing on its own — a surface in here
/// gets no allocation, which is exactly why `spawn_headless` has to realize the
/// widget itself. Being a `gtk4::Box` matters: the reparenting paths
/// (`unparent_workspace_widgets`, `create_terminal_widget`) only detach a
/// surface whose parent is a Box, and that is how a parked surface moves into
/// its real page when the workspace is finally opened.
///
/// Returns `None` until some window can host it (no window yet, or none with a
/// workspace page); callers retry on their next tick.
pub(crate) fn spawn_nursery() -> Option<gtk4::Box> {
    let live: Vec<gtk4::Stack> = CONTENT_STACKS.with(|stacks| {
        let mut stacks = stacks.borrow_mut();
        stacks.retain(|w| w.upgrade().is_some());
        stacks.iter().filter_map(|w| w.upgrade()).collect()
    });
    // Prefer a window that is already on screen; a hidden one is only realized
    // as a fallback (see `nursery_in`).
    let shown = live
        .iter()
        .filter(|s| s.root().is_some_and(|r| r.is_realized()));
    for stack in shown.chain(live.iter()) {
        if let Some(nursery) = nursery_in(stack) {
            return Some(nursery);
        }
    }
    None
}

/// Get (or create) `stack`'s nursery page, making sure its window is realized so
/// surfaces parented there can obtain a GL context.
fn nursery_in(stack: &gtk4::Stack) -> Option<gtk4::Box> {
    let root: gtk4::Widget = stack.root()?.upcast();
    if !root.is_realized() {
        // A window that has never been shown — the quake drop-down at rest —
        // has no GdkSurface, so no GL context can exist under it. Realizing it
        // creates the surface *without mapping anything*: nothing appears on
        // screen (verified), but background agents can start before the user
        // ever drops the terminal down.
        tracing::debug!("realizing a hidden window so background terminals can spawn");
        gtk4::prelude::WidgetExt::realize(&root);
        if !root.is_realized() {
            return None;
        }
    }
    if let Some(existing) = stack.child_by_name(HEADLESS_PAGE) {
        return existing.downcast::<gtk4::Box>().ok();
    }
    // A GtkStack adopts the first page added as its visible child, so never add
    // the nursery to an empty stack — it would become the window's content —
    // and restore the visible child afterwards.
    let previous = stack.visible_child()?;
    let nursery = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    nursery.set_widget_name(HEADLESS_PAGE);
    stack.add_named(&nursery, Some(HEADLESS_PAGE));
    stack.set_visible_child(&previous);
    Some(nursery)
}

/// Fold everything `build_layout`/`build_zoomed` consume into one hash. Two
/// workspace states with the same signature produce identical widget trees, so
/// the page can be reused as-is (just re-shown) instead of rebuilt. Excludes
/// divider positions (GtkPaned owns those live), all panel *metadata* (title,
/// dir, git branch — handled by `refresh_metadata` without a rebuild), and the
/// focused panel — focus changes only run `update_focus_visuals` in place, so
/// hashing focus here would leave a stale signature behind every pane-focus
/// change and force a full GL-churning rebuild on the next workspace switch.
fn workspace_signature(
    layout: &crate::model::panel::LayoutNode,
    panels: &std::collections::HashMap<uuid::Uuid, Panel>,
    effective_attention: Option<uuid::Uuid>,
    zoomed_panel_id: Option<uuid::Uuid>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    fn hash_node(
        node: &crate::model::panel::LayoutNode,
        panels: &std::collections::HashMap<uuid::Uuid, Panel>,
        h: &mut std::collections::hash_map::DefaultHasher,
    ) {
        use crate::model::panel::LayoutNode;
        match node {
            LayoutNode::Pane {
                panel_ids,
                selected_panel_id,
            } => {
                0u8.hash(h);
                for id in panel_ids {
                    id.hash(h);
                    // Panel type affects which widget is built (terminal vs browser
                    // vs markdown …), so a type change must force a rebuild.
                    if let Some(p) = panels.get(id) {
                        std::mem::discriminant(&p.panel_type).hash(h);
                    }
                }
                selected_panel_id.hash(h);
            }
            LayoutNode::Split {
                orientation,
                first,
                second,
                ..
            } => {
                1u8.hash(h);
                std::mem::discriminant(orientation).hash(h);
                hash_node(first, panels, h);
                hash_node(second, panels, h);
            }
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash_node(layout, panels, &mut h);
    zoomed_panel_id.hash(&mut h);
    effective_attention.hash(&mut h);
    h.finish()
}

/// The page signature actually cached: `workspace_signature` plus the *build
/// environment* — whether ghostty is initialized and whether each terminal
/// panel has an initialized surface. A page built before ghostty came up (or
/// whose surfaces were pruned) produces widgets that can never spawn a shell;
/// without these bits its signature would match forever and the page would
/// never be rebuilt — the "all terminals blank" regression that forced the
/// first landing of the GtkStack to be reverted (8bb1ab3). Recompute this
/// *after* a build when recording it, so the surfaces created by the build
/// don't immediately invalidate their own page.
fn page_signature(
    state: &Rc<AppState>,
    layout: &crate::model::panel::LayoutNode,
    panels: &std::collections::HashMap<uuid::Uuid, Panel>,
    effective_attention: Option<uuid::Uuid>,
    zoomed_panel_id: Option<uuid::Uuid>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    workspace_signature(layout, panels, effective_attention, zoomed_panel_id).hash(&mut h);
    state.ghostty_app.borrow().is_some().hash(&mut h);
    {
        let cache = state.terminal_cache.borrow();
        for id in layout.all_panel_ids() {
            let terminal_ready = cache.get(&id).map(|s| s.is_initialized());
            terminal_ready.hash(&mut h);
        }
    }
    h.finish()
}

pub fn rebuild_content(content_box: &gtk4::Box, state: &Rc<AppState>, window_id: Option<uuid::Uuid>) {
    tracing::debug!("rebuild_content triggered");

    // Which window are we rendering for? Use the explicit id, else derive it
    // from the content box's root window. Lets each window render its own
    // workspace and avoid stealing GL surfaces that live in other windows.
    let win_id = window_id.or_else(|| window_id_of(content_box.upcast_ref::<gtk4::Widget>()));
    let stack = content_stack(content_box);

    // Clone workspace data out of the lock so we don't hold it during widget construction.
    let workspace_data = {
        let tab_manager = lock_or_recover(&state.shared.tab_manager);
        let selected = match win_id {
            Some(wid) => tab_manager.selected_for_window(wid),
            None => tab_manager.selected(),
        };
        selected.map(|ws| {
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
        let effective_attention = if crate::settings::load().pane_attention_ring {
            attention_panel_id
        } else {
            None
        };
        let signature = page_signature(state, &layout, &panels, effective_attention, zoomed_panel_id);
        let page_name = id.to_string();

        // Rebuild this workspace's page only when its structure actually changed
        // (split/close/new-tab/zoom/type). A pure selection switch keeps the same
        // signature → we skip straight to `set_visible_child` and never unrealize.
        let needs_build = state
            .workspace_page_signatures
            .borrow()
            .get(&id)
            .map(|prev| *prev != signature)
            .unwrap_or(true);
        tracing::debug!(workspace = %id, needs_build, "rebuild_content: page decision");

        if needs_build {
            // Free this workspace's cached surfaces/browsers from whatever page
            // currently holds them, so build_layout can reparent them into the
            // fresh page. Scoped to this workspace's panels — other workspaces'
            // pages (and their realized surfaces) are left untouched.
            unparent_workspace_widgets(&panels, state);

            let widget = if let Some(zoomed_id) = zoomed_panel_id {
                split_view::build_zoomed(zoomed_id, &panels, state)
            } else {
                split_view::build_layout(
                    id,
                    &layout,
                    &panels,
                    effective_attention,
                    focused_panel_id,
                    state,
                )
            };
            if let Some(old) = stack.child_by_name(&page_name) {
                stack.remove(&old);
            }
            stack.add_named(&widget, Some(&page_name));
            // Record the *post-build* signature: the build itself creates and
            // initializes surfaces, which are part of the signature.
            let built_signature =
                page_signature(state, &layout, &panels, effective_attention, zoomed_panel_id);
            state
                .workspace_page_signatures
                .borrow_mut()
                .insert(id, built_signature);
        }

        if let Some(page) = stack.child_by_name(&page_name) {
            stack.set_visible_child(&page);
            // Deferred map probe: shells only spawn once the page allocates,
            // so an unmapped page here is the blank-terminal failure. Debug
            // logging only — but loud enough to catch in the field.
            let page_weak = page.downgrade();
            let stack_weak = stack.downgrade();
            glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                let (page, stack) = match (page_weak.upgrade(), stack_weak.upgrade()) {
                    (Some(p), Some(s)) => (p, s),
                    (p, s) => {
                        tracing::warn!(
                            page_alive = p.is_some(),
                            stack_alive = s.is_some(),
                            "stack page probe: widgets destroyed after set_visible_child"
                        );
                        return;
                    }
                };
                tracing::debug!(
                    page_mapped = page.is_mapped(),
                    page_w = page.width(),
                    page_h = page.height(),
                    stack_mapped = stack.is_mapped(),
                    stack_w = stack.width(),
                    stack_h = stack.height(),
                    "stack page state probe"
                );
                if !page.is_mapped() {
                    tracing::warn!(
                        "visible stack page still unmapped 2s after switch — \
                         terminals in it cannot spawn"
                    );
                }
            });
        }
    } else {
        // No workspace to show — welcome screen or an empty-state label. Rebuilt
        // each time (cheap, no GL surfaces) under a fixed page name.
        let widget: gtk4::Widget = if super::welcome::should_show_welcome() {
            super::welcome::build_welcome()
        } else {
            let label = gtk4::Label::new(Some("No workspace selected"));
            label.add_css_class("dim-label");
            label.upcast()
        };
        if let Some(old) = stack.child_by_name(NO_WORKSPACE_PAGE) {
            stack.remove(&old);
        }
        stack.add_named(&widget, Some(NO_WORKSPACE_PAGE));
        stack.set_visible_child(&widget);
    }

    // Drop pages for workspaces that no longer exist in this window (closed).
    // Removing a page unrealizes its surfaces — correct here, the workspace is
    // gone — and is what finally releases their GL resources.
    prune_workspace_pages(&stack, state, win_id);

    // Re-apply focus-dependent visuals to the now-visible page. A reused page
    // carries whatever dim/border state it had when last shown (or was given by
    // a metadata refresh while hidden), so without this a workspace switch
    // shows its focused pane dimmed until the next periodic refresh.
    update_focus_visuals(content_box, state);

    // A tab was just closed — move keyboard focus onto the now-active pane's
    // terminal (surfaces are re-parented above, so this runs at the right time).
    if FOCUS_ACTIVE_TERMINAL.with(|f| f.replace(false)) {
        focus_active_terminal(state);
    }
}

/// Unparent the cached GL surfaces and browser widgets for `panels` from
/// whatever stack page currently holds them, so they can be reparented into a
/// freshly-built page. A GL surface must be unparented before it can be added
/// elsewhere; scoping to one workspace's panels avoids disturbing other
/// workspaces' still-realized pages.
fn unparent_workspace_widgets(
    panels: &std::collections::HashMap<uuid::Uuid, Panel>,
    state: &Rc<AppState>,
) {
    let terminal_cache = state.terminal_cache.borrow();
    for panel_id in panels.keys() {
        if let Some(surface) = terminal_cache.get(panel_id) {
            if let Some(parent) = surface.parent() {
                if let Ok(parent_box) = parent.downcast::<gtk4::Box>() {
                    parent_box.remove(surface);
                }
            }
        }
    }
    drop(terminal_cache);
    let browser_cache = state.browser_cache.borrow();
    for panel_id in panels.keys() {
        if let Some(browser_widget) = browser_cache.get(panel_id) {
            if browser_widget.parent().is_some() {
                browser_widget.unparent();
            }
        }
    }
}

/// Remove stack pages (and forget the cache entries) for workspaces that no
/// longer exist anywhere.
///
/// Existence is deliberately the ONLY staleness test. Pages only enter this
/// stack because THIS window's rebuild built them, and the render path
/// (`selected_for_window`) treats `window_id: None` workspaces as belonging
/// to any window — the original landing (74bb9b2) instead pruned on
/// `ws.window_id == win_id`, which is false for every `window_id: None`
/// workspace, so each freshly-built page was destroyed at the end of the same
/// rebuild that created it. That unrealized the just-parented GL surfaces
/// before their first allocation, the shell never spawned (spawn rides the
/// first resize), and every terminal rendered permanently blank — the
/// regression behind the 8bb1ab3 revert. A workspace moved to another window
/// may briefly leave an empty page here; it is pruned when the workspace
/// closes, which is harmless — wrongly pruning a live page is not.
fn prune_workspace_pages(stack: &gtk4::Stack, state: &Rc<AppState>, _win_id: Option<uuid::Uuid>) {
    let live_ids: std::collections::HashSet<uuid::Uuid> = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.iter().map(|ws| ws.id).collect()
    };
    // Collect names first (can't mutate the stack while iterating its children).
    let mut stale: Vec<(uuid::Uuid, gtk4::Widget)> = Vec::new();
    let mut child = stack.first_child();
    while let Some(w) = child {
        child = w.next_sibling();
        let Some(name) = stack.page(&w).name() else {
            continue;
        };
        if name == NO_WORKSPACE_PAGE {
            continue;
        }
        if let Ok(id) = uuid::Uuid::parse_str(&name) {
            if !live_ids.contains(&id) {
                stale.push((id, w));
            }
        }
    }
    for (id, w) in stale {
        stack.remove(&w);
        state.workspace_page_signatures.borrow_mut().remove(&id);
    }
}

fn refresh_ui(list_box: &gtk4::ListBox, content_box: &gtk4::Box, state: &Rc<AppState>) {
    // Closing the last tab closes its workspace; if that leaves no workspaces at
    // all, enforce the "always something to show" invariant before rebuilding.
    // Returns false when it quit the app — nothing left to render.
    if !enforce_workspace_invariant(content_box, state) {
        return;
    }
    state.prune_terminal_cache();
    state.shared.cleanup_stale_remote_sessions();
    sidebar::refresh_sidebar(list_box, state);
    rebuild_content(content_box, state, None);
    update_window_title(content_box, state);
}

/// When the last workspace is closed, keep jmux in a sane state:
/// - quake mode: spawn a fresh workspace + tab so the drop-down console is never
///   empty (it must always have a workspace, or it would fall back to rendering
///   the main window's selection);
/// - otherwise: quit jmux — there are no workspaces or tabs left to show. A
///   relaunch starts fresh with one workspace and one tab, like a first launch.
///
/// Returns true if the caller should continue rendering, false if it quit.
fn enforce_workspace_invariant(content_box: &gtk4::Box, state: &Rc<AppState>) -> bool {
    if lock_or_recover(&state.shared.tab_manager).iter().next().is_some() {
        return true;
    }
    // The welcome screen legitimately shows no workspaces — don't act on it.
    if crate::ui::welcome::should_show_welcome() {
        return true;
    }

    if crate::app::quake_mode() {
        // Spawn a fresh workspace + tab so the drop-down console is never empty.
        // It becomes the global selection (this is the only window), so the
        // console renders it — a plain workspace, not a dedicated one.
        lock_or_recover(&state.shared.tab_manager).add_workspace(Workspace::new());
        true
    } else {
        if let Some(app) = content_box
            .root()
            .and_then(|r| r.downcast::<adw::ApplicationWindow>().ok())
            .and_then(|w| w.application())
        {
            app.quit();
        }
        false
    }
}

/// Lightweight refresh for metadata-only changes (title, PWD, git branch).
/// Updates the sidebar and window title without touching the content layout,
/// so browser panels are not unparented/reparented.
pub fn refresh_metadata(list_box: &gtk4::ListBox, content_box: &gtk4::Box, state: &Rc<AppState>) {
    tracing::debug!("refresh_metadata: start");
    sidebar::refresh_sidebar(list_box, state);
    update_focus_visuals(content_box, state);
    update_window_title(content_box, state);
    tracing::debug!("refresh_metadata: done");
}

/// Re-apply focus-dependent visuals (split-region dim + per-pane inactive
/// overlay) in place, without rebuilding the content layout.
///
/// The split-region opacity and the inactive-pane overlays are decided at build
/// time from `focused_panel_id`; on a focus change we only run a metadata
/// refresh (a full rebuild would churn the GLArea and swallow input), so this
/// walks the existing widget tree and updates those visuals directly. Without
/// it, the pane that was focused when the layout was built stays bright while
/// the others stay dimmed regardless of which pane is actually active.
///
/// Walks only the stack's *visible* page: hidden pages belong to other
/// workspaces whose panes are never the focused panel, so touching them would
/// dim every hidden page and make each workspace switch surface a pre-dimmed
/// layout (and walking every realized workspace tree on each periodic
/// metadata refresh is wasted work). `rebuild_content` re-applies visuals on
/// every switch, so a page shown later is never stale.
fn update_focus_visuals(content_box: &gtk4::Box, state: &Rc<AppState>) {
    let focused_str = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        // Resolve the focused panel from *this window's* workspace, not the
        // global selection — otherwise a secondary window (e.g. the quake
        // drop-down) highlights nothing and every pane renders unfocused/grey.
        workspace_for(content_box, &tm)
            .and_then(|ws| ws.focused_panel_id)
            .map(|id| id.to_string())
    };
    let unfocused_opacity = state
        .ghostty_ui_config
        .borrow()
        .unfocused_split_opacity
        .map(|o| o.clamp(0.15, 1.0))
        .filter(|&o| o < 1.0);

    let root: gtk4::Widget = content_stack(content_box)
        .visible_child()
        .unwrap_or_else(|| content_box.clone().upcast());
    for_each_descendant(&root, &mut |w| {
        // Split-region dim: bright when this region holds the focused pane.
        if w.has_css_class("pane-region") {
            let region_focused = focused_str
                .as_deref()
                .map(|fid| descendant_named(w, fid))
                .unwrap_or(false);
            let opacity = if region_focused {
                1.0
            } else {
                unfocused_opacity.unwrap_or(1.0)
            };
            w.set_opacity(opacity);
        }
        // Per-pane inactive overlay: shown only when the pane is not focused.
        if w.has_css_class("inactive-pane-overlay") {
            if let Some(pid) = overlay_panel_id(w) {
                let is_focused = focused_str.as_deref() == Some(pid.as_str());
                w.set_visible(!is_focused);
            }
        }
        // Per-pane focused border highlight.
        if w.has_css_class("pane-container") {
            let is_focused = focused_str.as_deref() == Some(w.widget_name().as_str());
            if is_focused {
                w.add_css_class("focused-panel");
            } else {
                w.remove_css_class("focused-panel");
            }
        }
    });
}

/// Walk `widget` and all of its descendants, invoking `f` on each.
fn for_each_descendant(widget: &gtk4::Widget, f: &mut dyn FnMut(&gtk4::Widget)) {
    f(widget);
    let mut child = widget.first_child();
    while let Some(c) = child {
        for_each_descendant(&c, f);
        child = c.next_sibling();
    }
}

/// True if `widget` or any descendant has the GTK widget-name `name`.
fn descendant_named(widget: &gtk4::Widget, name: &str) -> bool {
    if widget.widget_name() == name {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if descendant_named(&c, name) {
            return true;
        }
        child = c.next_sibling();
    }
    false
}

/// For an `inactive-pane-overlay` widget, return the panel id it belongs to —
/// the widget-name of the parent `GtkOverlay`'s main child (the pane container).
fn overlay_panel_id(overlay_child: &gtk4::Widget) -> Option<String> {
    let overlay = overlay_child.parent()?.downcast::<gtk4::Overlay>().ok()?;
    let name = overlay.child()?.widget_name();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn update_window_title(content_box: &gtk4::Box, state: &Rc<AppState>) {
    if let Some(root) = content_box.root() {
        if let Some(window) = root.downcast_ref::<adw::ApplicationWindow>() {
            let titles = {
                let tm = lock_or_recover(&state.shared.tab_manager);
                // This window's own workspace, not the global selection, so the
                // drop-down's title/dir match what it actually shows.
                workspace_for(content_box, &tm).map(|ws| {
                    let title = ws.display_title();
                    let dir = crate::ui::sidebar::compact_path(&ws.current_directory);
                    (format!("{title} — {dir} — jmux"), title.to_string())
                })
            };
            if let Some((full_title, ws_title)) = titles {
                window.set_title(Some(&full_title));
                if let Some(root) = window.content() {
                    // Unwrap ToastOverlay wrapper if present to reach outer_box
                    let outer = root
                        .clone()
                        .downcast::<adw::ToastOverlay>()
                        .ok()
                        .and_then(|ov| ov.child())
                        .unwrap_or(root);
                    if let Some(hb) = outer.first_child() {
                        if let Some(header) = hb.downcast_ref::<adw::HeaderBar>() {
                            if let Some(tw) = header.title_widget() {
                                if let Some(lbl) = tw.downcast_ref::<gtk4::Label>() {
                                    lbl.set_text(&ws_title);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn bind_sidebar_selection(list_box: &gtk4::ListBox, content_box: &gtk4::Box, state: &Rc<AppState>) {
    let state = state.clone();
    let state_for_activate = state.clone();
    let lb = list_box.clone();
    let content_box = content_box.clone();

    // Coalesce rapid workspace switches. A deliberate switch runs the refresh
    // immediately (instant page swap); switches arriving faster than the window
    // collapse into a single trailing refresh, so a burst — a held index key or
    // fast clicking down the sidebar — can't fire N synchronous rebuilds
    // back-to-back and starve pointer/key input. `select_workspace_by_index`
    // updates the model on every event, so the trailing refresh always lands on
    // the final selection.
    let last_switch = Rc::new(std::cell::Cell::new(
        std::time::Instant::now() - std::time::Duration::from_secs(1),
    ));
    let switch_scheduled = Rc::new(std::cell::Cell::new(false));

    list_box.connect_row_selected(move |_list_box, row| {
        let Some(row) = row else {
            return;
        };

        // The workspace index is carried in the row's widget name ("ws-N").
        // row.index() is the row's VISUAL position, which drifts from the
        // workspace index as soon as a group header row sits above it —
        // using it selected the wrong workspace and cascaded through
        // refreshes until the last one stuck.
        let name = row.widget_name();
        let Some(index) = name
            .as_str()
            .strip_prefix("ws-")
            .and_then(|s| s.parse::<usize>().ok())
        else {
            return;
        };
        if !event_handler::select_workspace_by_index(&state, index) {
            return;
        }

        const SWITCH_THROTTLE: std::time::Duration = std::time::Duration::from_millis(120);
        let elapsed = std::time::Instant::now().duration_since(last_switch.get());
        if elapsed >= SWITCH_THROTTLE {
            last_switch.set(std::time::Instant::now());
            refresh_ui(&lb, &content_box, &state);
        } else if !switch_scheduled.get() {
            switch_scheduled.set(true);
            let delay = SWITCH_THROTTLE - elapsed;
            let lb2 = lb.clone();
            let cb2 = content_box.clone();
            let st2 = state.clone();
            let flag = switch_scheduled.clone();
            let ts = last_switch.clone();
            glib::timeout_add_local_once(delay, move || {
                flag.set(false);
                ts.set(std::time::Instant::now());
                refresh_ui(&lb2, &cb2, &st2);
            });
        }
    });

    // Explicitly opening a workspace (click or Enter) should hand keyboard focus
    // to its active terminal so typing goes straight there. row-activated fires
    // only on click/Enter — never on plain arrow-key browsing — so this doesn't
    // hijack keyboard navigation of the sidebar. The preceding row-selected has
    // already rebuilt the content, so the terminal surface is parented by now.
    {
        let state = state_for_activate;
        list_box.connect_row_activated(move |_list_box, _row| {
            focus_active_terminal(&state);
        });
    }
}
