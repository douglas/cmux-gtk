//! Split view — recursive GtkPaned tree from LayoutNode.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;
use uuid::Uuid;

use crate::app::{lock_or_recover, AppState};
use crate::model::panel::{Direction, LayoutNode, Panel, PanelType, SplitOrientation};
use crate::ui::terminal_panel;

/// Build a zoomed view — renders only a single panel full-size.
pub fn build_zoomed(
    panel_id: Uuid,
    panels: &HashMap<Uuid, Panel>,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    if let Some(panel) = panels.get(&panel_id) {
        terminal_panel::create_panel_widget(panel, false, true, state)
    } else {
        gtk4::Label::new(Some("Panel not found")).upcast()
    }
}

/// Build a GTK widget tree from a LayoutNode.
///
/// - `LayoutNode::Pane` → GtkStack (with tabs if multiple panels) wrapping terminal widgets
/// - `LayoutNode::Split` → GtkPaned with recursive children
pub fn build_layout(
    workspace_id: Uuid,
    node: &LayoutNode,
    panels: &HashMap<Uuid, Panel>,
    attention_panel_id: Option<Uuid>,
    focused_panel_id: Option<Uuid>,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    match node {
        LayoutNode::Pane {
            panel_ids,
            selected_panel_id,
        } => build_pane(
            workspace_id,
            panel_ids,
            *selected_panel_id,
            panels,
            attention_panel_id,
            focused_panel_id,
            state,
        ),

        LayoutNode::Split {
            orientation,
            divider_position,
            first,
            second,
        } => build_split(
            workspace_id,
            *orientation,
            *divider_position,
            first,
            second,
            panels,
            attention_panel_id,
            focused_panel_id,
            state,
        ),
    }
}

/// Build a pane widget (single or tabbed panels).
///
/// Always shows a tab bar with action buttons, even for single-panel panes.
#[allow(clippy::too_many_arguments)]
fn build_pane(
    workspace_id: Uuid,
    panel_ids: &[Uuid],
    selected_id: Option<Uuid>,
    panels: &HashMap<Uuid, Panel>,
    attention_panel_id: Option<Uuid>,
    focused_panel_id: Option<Uuid>,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    if panel_ids.is_empty() {
        // Empty pane — show placeholder
        let label = gtk4::Label::new(Some("Empty pane"));
        label.set_hexpand(true);
        label.set_vexpand(true);
        return label.upcast();
    }

    // Always use GtkStack (even for single panel) so tab bar logic is uniform
    let stack = gtk4::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    for &panel_id in panel_ids {
        if let Some(panel) = panels.get(&panel_id) {
            let is_focused = focused_panel_id == Some(panel_id);
            let widget = terminal_panel::create_panel_widget(
                panel,
                attention_panel_id == Some(panel_id),
                is_focused,
                state,
            );
            let page = stack.add_child(&widget);
            page.set_title(panel.display_title());
            page.set_name(&panel_id.to_string());
        }
    }

    // Select the active panel
    if let Some(sel_id) = selected_id {
        stack.set_visible_child_name(&sel_id.to_string());
    }

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    // Marks a split-region container so focus changes can re-dim it without a
    // full rebuild (see window::update_focus_visuals).
    vbox.add_css_class("pane-region");

    // Tab bar — always shown (action buttons are always accessible)
    let tab_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    tab_bar.add_css_class("pane-tab-bar");

    for (tab_index, &panel_id) in panel_ids.iter().enumerate() {
        let title = panels
            .get(&panel_id)
            .map(|p| p.display_title().to_string())
            .unwrap_or_else(|| "?".to_string());
        let is_selected = selected_id == Some(panel_id);
        let is_attention = attention_panel_id == Some(panel_id)
            || panels
                .get(&panel_id)
                .map(|p| p.is_manually_unread)
                .unwrap_or(false);

        let panel_type = panels
            .get(&panel_id)
            .map(|p| p.panel_type)
            .unwrap_or(PanelType::Terminal);
        let tab_btn = build_tab_button(
            panel_id,
            tab_index,
            &title,
            is_selected,
            is_attention,
            panel_type,
            &stack,
            state,
        );
        tab_bar.append(&tab_btn);
    }

    // Spacer to push action buttons to the right
    let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    tab_bar.append(&spacer);

    // Thin separator between tabs and action buttons
    let sep = gtk4::Separator::new(gtk4::Orientation::Vertical);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    sep.set_margin_start(2);
    sep.set_margin_end(4);
    tab_bar.append(&sep);

    // Target panel for "add tab" actions (selected or first)
    let target_panel_id = selected_id.or_else(|| panel_ids.first().copied());

    // New Terminal button
    let new_term_btn = gtk4::Button::from_icon_name("utilities-terminal-symbolic");
    new_term_btn.add_css_class("flat");
    new_term_btn.add_css_class("pane-tab-action");
    new_term_btn.set_tooltip_text(Some("New Terminal"));
    {
        let state = Rc::clone(state);
        new_term_btn.connect_clicked(move |_| {
            let new_panel = Panel::new_terminal();
            let new_id = new_panel.id;
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.workspace_mut(workspace_id) {
                ws.panels.insert(new_id, new_panel);
                if let Some(tid) = target_panel_id {
                    ws.layout.add_panel_to_pane(tid, new_id);
                }
                ws.previous_focused_panel_id = ws.focused_panel_id;
                ws.focused_panel_id = Some(new_id);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    tab_bar.append(&new_term_btn);

    // New Browser button
    let new_browser_btn = gtk4::Button::from_icon_name("globe-symbolic");
    new_browser_btn.add_css_class("flat");
    new_browser_btn.add_css_class("pane-tab-action");
    new_browser_btn.set_tooltip_text(Some("New Browser"));
    {
        let state = Rc::clone(state);
        new_browser_btn.connect_clicked(move |_| {
            let new_panel = Panel::new_browser();
            let new_id = new_panel.id;
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.workspace_mut(workspace_id) {
                ws.panels.insert(new_id, new_panel);
                if let Some(tid) = target_panel_id {
                    ws.layout.add_panel_to_pane(tid, new_id);
                }
                ws.previous_focused_panel_id = ws.focused_panel_id;
                ws.focused_panel_id = Some(new_id);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    tab_bar.append(&new_browser_btn);

    // Split Right button
    let split_h_btn = gtk4::Button::from_icon_name("view-dual-symbolic");
    split_h_btn.add_css_class("flat");
    split_h_btn.add_css_class("pane-tab-action");
    split_h_btn.set_tooltip_text(Some("Split Right"));
    {
        let state = Rc::clone(state);
        split_h_btn.connect_clicked(move |_| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.workspace_mut(workspace_id) {
                ws.split(SplitOrientation::Horizontal, PanelType::Terminal);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    tab_bar.append(&split_h_btn);

    // Split Down button
    let split_v_btn = gtk4::Button::from_icon_name("view-paged-symbolic");
    split_v_btn.add_css_class("flat");
    split_v_btn.add_css_class("pane-tab-action");
    split_v_btn.set_tooltip_text(Some("Split Down"));
    {
        let state = Rc::clone(state);
        split_v_btn.connect_clicked(move |_| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.workspace_mut(workspace_id) {
                ws.split(SplitOrientation::Vertical, PanelType::Terminal);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    tab_bar.append(&split_v_btn);

    vbox.append(&tab_bar);
    vbox.append(&stack);
    vbox.set_hexpand(true);
    vbox.set_vexpand(true);

    // Apply unfocused split opacity from ghostty config
    let pane_has_focus = panel_ids.iter().any(|id| focused_panel_id == Some(*id));
    if !pane_has_focus {
        let ui_config = state.ghostty_ui_config.borrow();
        if let Some(opacity) = ui_config.unfocused_split_opacity {
            if opacity < 1.0 {
                vbox.set_opacity(opacity.clamp(0.15, 1.0));
            }
        }
    }

    // Drop target over the whole pane body: dropping a tab near an edge splits
    // the pane (right/left → horizontal, bottom/top → vertical); dropping in the
    // center moves the tab into this pane. Tab-bar tabs keep their own drop
    // targets (reorder / move), so this only fires over the terminal area.
    {
        let state = Rc::clone(state);
        let pane_panels: Vec<Uuid> = panel_ids.to_vec();
        let vbox_weak = vbox.downgrade();
        let drop_target = gtk4::DropTarget::new(glib::Type::STRING, gdk4::DragAction::MOVE);
        drop_target.connect_drop(move |_t, value, x, y| {
            let Ok(data) = value.get::<String>() else {
                return false;
            };
            // Only tab payloads ("<index>/<uuid>") apply here.
            let Some(source) = data
                .split_once('/')
                .and_then(|(_, id)| Uuid::parse_str(id).ok())
            else {
                return false;
            };
            let Some(vbox) = vbox_weak.upgrade() else {
                return false;
            };
            let (w, h) = (vbox.width() as f64, vbox.height() as f64);
            if w <= 0.0 || h <= 0.0 {
                return false;
            }
            // A representative panel of this pane that isn't the dragged one.
            let Some(target) = pane_panels.iter().copied().find(|&p| p != source) else {
                return false;
            };
            let (fx, fy) = (x / w, y / h);
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            let Some(ws) = tm.find_workspace_with_panel_mut(target) else {
                return false;
            };
            let did = if fx > 0.66 {
                ws.split_panel_into_pane(source, target, SplitOrientation::Horizontal, Direction::Right)
            } else if fx < 0.34 {
                ws.split_panel_into_pane(source, target, SplitOrientation::Horizontal, Direction::Left)
            } else if fy > 0.66 {
                ws.split_panel_into_pane(source, target, SplitOrientation::Vertical, Direction::Down)
            } else if fy < 0.34 {
                ws.split_panel_into_pane(source, target, SplitOrientation::Vertical, Direction::Up)
            } else {
                ws.move_panel_to_pane(source, target)
            };
            drop(tm);
            if did {
                state.shared.notify_ui_refresh();
            }
            did
        });
        vbox.add_controller(drop_target);
    }

    vbox.upcast()
}

/// Build a single tab button with label, close button, and drag-drop reorder.
#[allow(clippy::too_many_arguments)]
fn build_tab_button(
    panel_id: Uuid,
    tab_index: usize,
    title: &str,
    is_selected: bool,
    is_attention: bool,
    panel_type: PanelType,
    stack: &gtk4::Stack,
    state: &Rc<AppState>,
) -> gtk4::Box {
    let tab = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    tab.add_css_class("pane-tab");
    if is_selected {
        tab.add_css_class("pane-tab-selected");
    }
    if is_attention {
        tab.add_css_class("pane-tab-attention");
    }
    tab.set_margin_start(2);
    tab.set_margin_end(2);
    tab.set_margin_top(2);
    tab.set_margin_bottom(2);

    // Panel type icon (use favicon for browser panels if available)
    #[cfg(feature = "webkit")]
    let browser_favicon = if panel_type == PanelType::Browser {
        super::browser_panel::get_favicon(panel_id)
    } else {
        None
    };
    #[cfg(not(feature = "webkit"))]
    let browser_favicon: Option<gdk4::Texture> = None;
    let icon = if let Some(texture) = browser_favicon {
        let img = gtk4::Image::from_paintable(Some(&texture));
        img.set_pixel_size(14);
        img
    } else {
        let icon_name = match panel_type {
            PanelType::Terminal => "utilities-terminal-symbolic",
            PanelType::Markdown => "document-open-symbolic",
            PanelType::Diff => "media-flash-symbolic",
            PanelType::Project => "view-list-symbolic",
            PanelType::FilePreview => "text-x-generic-symbolic",
            PanelType::Notes => "accessories-text-editor-symbolic",
            PanelType::History => "document-open-recent-symbolic",
            PanelType::Vault => "drive-multidisk-symbolic",
            _ => "globe-symbolic",
        };
        let img = gtk4::Image::from_icon_name(icon_name);
        img.set_pixel_size(14);
        img
    };
    tab.append(&icon);

    let label = gtk4::Label::new(Some(title));
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(20);
    tab.append(&label);

    // Close button — optional, controlled by the show_tab_close_button setting.
    // When hidden, tabs can still be closed via middle-click or context menu.
    let show_close_button = crate::settings::load().show_tab_close_button;
    let close_btn = if show_close_button {
        let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        close_btn.add_css_class("flat");
        close_btn.add_css_class("circular");
        close_btn.add_css_class("pane-tab-close");
        close_btn.set_tooltip_text(Some("Close tab"));
        // Never let keyboard focus land on the close button: after a tab closes
        // and the bar rebuilds, focus would otherwise settle on an adjacent
        // tab's close button, where a stray Enter/Space closes another tab.
        close_btn.set_can_focus(false);
        close_btn.set_focus_on_click(false);
        {
            let state = Rc::clone(state);
            close_btn.connect_clicked(move |_| {
                // Closing the last tab closes the workspace (TabManager::close_panel).
                lock_or_recover(&state.shared.tab_manager).close_panel(panel_id);
                crate::ui::window::request_terminal_focus();
                state.shared.notify_ui_refresh();
            });
        }
        tab.append(&close_btn);
        Some(close_btn)
    } else {
        None
    };

    // Context menu — created once, kept alive with the tab
    let ctx_menu = gio::Menu::new();
    ctx_menu.append(Some("Rename"), Some("tab.rename"));
    ctx_menu.append(Some("Close"), Some("tab.close"));
    ctx_menu.append(Some("Show Close Buttons"), Some("tab.toggle_close_btn"));

    let action_group = gio::SimpleActionGroup::new();

    // Stateful toggle mirrors the global show_tab_close_button setting; renders
    // as a checkmark item and rebuilds the tabs so the change is visible.
    let toggle_close_action =
        gio::SimpleAction::new_stateful("toggle_close_btn", None, &show_close_button.to_variant());
    {
        let state = Rc::clone(state);
        toggle_close_action.connect_activate(move |action, _| {
            let mut settings = crate::settings::load();
            settings.show_tab_close_button = !settings.show_tab_close_button;
            let _ = crate::settings::save(&settings);
            action.set_state(&settings.show_tab_close_button.to_variant());
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&toggle_close_action);

    let rename_action = gio::SimpleAction::new("rename", None);
    {
        let state = Rc::clone(state);
        // WEAK: this action lives in the group inserted on `tab` itself — a
        // strong capture is a tab → group → action → closure → tab cycle
        // that leaked the tab (and, via its click closure, the whole pane
        // stack with every panel subtree) on every layout rebuild.
        let tab_ref = tab.downgrade();
        rename_action.connect_activate(move |_, _| {
            let Some(tab_ref) = tab_ref.upgrade() else {
                return;
            };
            let root = tab_ref.root();
            let window = root
                .as_ref()
                .and_then(|r| r.downcast_ref::<libadwaita::ApplicationWindow>());
            if let Some(window) = window {
                crate::ui::window::show_rename_tab_dialog(window, &state, panel_id);
            }
        });
    }
    action_group.add_action(&rename_action);

    let close_action = gio::SimpleAction::new("close", None);
    {
        let state = Rc::clone(state);
        close_action.connect_activate(move |_, _| {
            lock_or_recover(&state.shared.tab_manager).close_panel(panel_id);
            crate::ui::window::request_terminal_focus();
            state.shared.notify_ui_refresh();
        });
    }
    action_group.add_action(&close_action);

    tab.insert_action_group("tab", Some(&action_group));

    let ctx_popover = gtk4::PopoverMenu::from_model(Some(&ctx_menu));
    ctx_popover.set_parent(&tab);
    ctx_popover.set_has_arrow(false);

    // Right-click gesture just positions and shows the popover
    let right_click = gtk4::GestureClick::new();
    right_click.set_button(3);
    {
        let popover = ctx_popover.clone();
        right_click.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk4::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    tab.add_controller(right_click);

    // Middle-click to close tab
    let middle_click = gtk4::GestureClick::new();
    middle_click.set_button(2);
    {
        let state = Rc::clone(state);
        middle_click.connect_pressed(move |gesture, _n, _x, _y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            lock_or_recover(&state.shared.tab_manager).close_panel(panel_id);
            crate::ui::window::request_terminal_focus();
            state.shared.notify_ui_refresh();
        });
    }
    tab.add_controller(middle_click);

    // Click to select tab — skip if the click lands on the close button.
    // Selection runs on `pressed` but the sequence is left unclaimed so the
    // DragSource below can still recognize a drag-to-reorder. (Claiming the
    // press here would cancel the drag gesture before it could start.)
    let click = gtk4::GestureClick::new();
    click.set_button(1);
    {
        // WEAK: defensive — if the tab ever outlives a rebuild, a strong
        // stack capture would pin every page (all panel subtrees) with it.
        let stack = stack.downgrade();
        let state = Rc::clone(state);
        let close_btn_ref = close_btn.clone();
        click.connect_pressed(move |gesture, _n, x, _y| {
            let Some(stack) = stack.upgrade() else {
                return;
            };
            // Don't steal clicks from the close button — compare x against
            // the tab's total width minus the close button's width.
            let Some(tab_widget) = gesture.widget() else {
                return;
            };
            let tab_width = tab_widget.width() as f64;
            let close_width = close_btn_ref
                .as_ref()
                .map(|b| b.width() as f64)
                .unwrap_or(0.0);
            if close_width > 0.0 && x >= tab_width - close_width {
                return;
            }
            stack.set_visible_child_name(&panel_id.to_string());
            // Move the selection highlight without a full rebuild: the
            // metadata refresh below does not touch the tab bar, so clear
            // `pane-tab-selected` from sibling tabs and apply it to this one.
            if let Some(parent) = tab_widget.parent() {
                let mut sibling = parent.first_child();
                while let Some(child) = sibling {
                    if child.has_css_class("pane-tab") {
                        child.remove_css_class("pane-tab-selected");
                    }
                    sibling = child.next_sibling();
                }
            }
            tab_widget.add_css_class("pane-tab-selected");
            // Update model
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                ws.focus_panel(panel_id);
            }
            drop(tm);
            // Focus change only — stack.set_visible_child_name() already
            // switched the visible panel. A full rebuild would cause
            // GLArea unrealize/realize and swallow input events (Enter key).
            // Metadata refresh updates sidebar + title without layout teardown.
            state.shared.notify_metadata_refresh();
        });
    }
    tab.add_controller(click);

    // Double-click on tab header → toggle zoom for this panel
    let dbl_click = gtk4::GestureClick::new();
    dbl_click.set_button(1);
    {
        let state = Rc::clone(state);
        dbl_click.connect_pressed(move |gesture, n_press, x, _y| {
            if n_press != 2 {
                return;
            }
            // Don't steal clicks meant for the close button
            let Some(tab_widget) = gesture.widget() else {
                return;
            };
            // Avoid triggering zoom when clicking the close button area
            // (close button is the last child; use rough rightmost 28px guard).
            // Skip the guard entirely when the close button is hidden.
            let tab_width = tab_widget.width() as f64;
            if show_close_button && tab_width > 0.0 && x >= tab_width - 28.0 {
                return;
            }
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                if ws.zoomed_panel_id == Some(panel_id) {
                    ws.zoomed_panel_id = None;
                } else {
                    ws.zoomed_panel_id = Some(panel_id);
                }
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    tab.add_controller(dbl_click);

    // Drag source for reordering
    let drag_source = gtk4::DragSource::new();
    drag_source.set_actions(gdk4::DragAction::MOVE);
    {
        let index_str = tab_index.to_string();
        let id_str = panel_id.to_string();
        drag_source.connect_prepare(move |_source, _x, _y| {
            let data = format!("{}/{}", index_str, id_str);
            let content = gdk4::ContentProvider::for_value(&data.to_value());
            Some(content)
        });
    }
    tab.add_controller(drag_source);

    // Drop target — reorder within the same pane, or move the tab into this
    // pane when it comes from a different pane.
    let drop_target = gtk4::DropTarget::new(glib::Type::STRING, gdk4::DragAction::MOVE);
    {
        let state = Rc::clone(state);
        let target_index = tab_index;
        let target_panel_id = panel_id;
        drop_target.connect_drop(move |_target, value, _x, _y| {
            let Ok(data) = value.get::<String>() else {
                return false;
            };
            let parts: Vec<&str> = data.splitn(2, '/').collect();
            if parts.len() != 2 {
                return false;
            }
            let Ok(source_panel_id) = uuid::Uuid::parse_str(parts[1]) else {
                return false;
            };
            if source_panel_id == target_panel_id {
                return false;
            }

            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(source_panel_id) {
                let same_pane = ws
                    .layout
                    .find_pane_with_panel_readonly(target_panel_id)
                    .map(|ids| ids.contains(&source_panel_id))
                    .unwrap_or(false);
                if same_pane {
                    ws.layout
                        .reorder_panel_in_pane(source_panel_id, target_index);
                } else {
                    ws.move_panel_to_pane(source_panel_id, target_panel_id);
                }
            }
            drop(tm);
            state.shared.notify_ui_refresh();
            true
        });
    }
    tab.add_controller(drop_target);

    tab
}

/// Build a split widget (GtkPaned with two children).
#[allow(clippy::too_many_arguments)]
fn build_split(
    workspace_id: Uuid,
    orientation: SplitOrientation,
    divider_position: f64,
    first: &LayoutNode,
    second: &LayoutNode,
    panels: &HashMap<Uuid, Panel>,
    attention_panel_id: Option<Uuid>,
    focused_panel_id: Option<Uuid>,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    let gtk_orientation = match orientation {
        SplitOrientation::Horizontal => gtk4::Orientation::Horizontal,
        SplitOrientation::Vertical => gtk4::Orientation::Vertical,
    };

    let paned = gtk4::Paned::new(gtk_orientation);
    paned.set_wide_handle(true);
    paned.set_hexpand(true);
    paned.set_vexpand(true);

    let first_panel_ids = first.all_panel_ids();
    let second_panel_ids = second.all_panel_ids();
    let first_widget = build_layout(
        workspace_id,
        first,
        panels,
        attention_panel_id,
        focused_panel_id,
        state,
    );
    let second_widget = build_layout(
        workspace_id,
        second,
        panels,
        attention_panel_id,
        focused_panel_id,
        state,
    );

    paned.set_start_child(Some(&first_widget));
    paned.set_end_child(Some(&second_widget));

    let pos = divider_position;
    let initial_position_applied = Rc::new(Cell::new(false));
    let state = Rc::clone(state);
    let initial_position_applied_for_notify = Rc::clone(&initial_position_applied);
    paned.connect_position_notify(move |paned| {
        let size = match paned.orientation() {
            gtk4::Orientation::Horizontal => paned.width(),
            _ => paned.height(),
        };
        if size <= 0 {
            return;
        }

        if !initial_position_applied_for_notify.replace(true) {
            let desired_position = (size as f64 * pos) as i32;
            if paned.position() != desired_position {
                paned.set_position(desired_position);
            }
            return;
        }

        let divider_position = (paned.position() as f64 / size as f64).clamp(0.0, 1.0);
        {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(workspace) = tm.workspace_mut(workspace_id) {
                let _ = workspace.layout.set_divider_position_for_split(
                    &first_panel_ids,
                    &second_panel_ids,
                    divider_position,
                );
            }
        }
    });

    paned.upcast()
}
