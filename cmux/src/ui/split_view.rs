//! Split view — recursive GtkPaned tree from LayoutNode.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use uuid::Uuid;

use crate::app::{lock_or_recover, AppState};
use crate::model::panel::{LayoutNode, Panel, PanelType, SplitOrientation};
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

    // Tab bar — always shown (action buttons are always accessible)
    let tab_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    tab_bar.add_css_class("pane-tab-bar");

    for (tab_index, &panel_id) in panel_ids.iter().enumerate() {
        let title = panels
            .get(&panel_id)
            .map(|p| p.display_title().to_string())
            .unwrap_or_else(|| "?".to_string());
        let is_selected = selected_id == Some(panel_id);

        let panel_type = panels
            .get(&panel_id)
            .map(|p| p.panel_type)
            .unwrap_or(PanelType::Terminal);
        let tab_btn = build_tab_button(
            panel_id,
            tab_index,
            &title,
            is_selected,
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
    new_term_btn.add_css_class("circular");
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
    let new_browser_btn = gtk4::Button::from_icon_name("web-browser-symbolic");
    new_browser_btn.add_css_class("flat");
    new_browser_btn.add_css_class("circular");
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
    split_h_btn.add_css_class("circular");
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
    split_v_btn.add_css_class("circular");
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
    vbox.upcast()
}

/// Build a single tab button with label, close button, and drag-drop reorder.
fn build_tab_button(
    panel_id: Uuid,
    tab_index: usize,
    title: &str,
    is_selected: bool,
    panel_type: PanelType,
    stack: &gtk4::Stack,
    state: &Rc<AppState>,
) -> gtk4::Box {
    let tab = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    tab.add_css_class("pane-tab");
    if is_selected {
        tab.add_css_class("pane-tab-selected");
    }
    tab.set_margin_start(2);
    tab.set_margin_end(2);
    tab.set_margin_top(2);
    tab.set_margin_bottom(2);

    // Panel type icon
    let icon_name = match panel_type {
        PanelType::Terminal => "utilities-terminal-symbolic",
        PanelType::Browser => "web-browser-symbolic",
    };
    let icon = gtk4::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    tab.append(&icon);

    let label = gtk4::Label::new(Some(title));
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(20);
    tab.append(&label);

    // Close button
    let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("flat");
    close_btn.add_css_class("circular");
    close_btn.add_css_class("pane-tab-close");
    close_btn.set_tooltip_text(Some("Close tab"));
    {
        let state = Rc::clone(state);
        close_btn.connect_clicked(move |_| {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                ws.remove_panel(panel_id);
                if ws.is_empty() {
                    let ws_id = ws.id;
                    tm.remove_by_id(ws_id);
                }
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    tab.append(&close_btn);

    // Click to select tab — skip if the click lands on the close button
    let click = gtk4::GestureClick::new();
    click.set_button(1);
    {
        let stack = stack.clone();
        let state = Rc::clone(state);
        let close_btn_ref = close_btn.clone();
        click.connect_pressed(move |gesture, _n, x, _y| {
            // Don't steal clicks from the close button
            let Some(tab_widget) = gesture.widget() else {
                return;
            };
            let close_start = close_btn_ref.allocation().x() as f64
                - tab_widget.allocation().x() as f64;
            if x >= close_start {
                return;
            }
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            stack.set_visible_child_name(&panel_id.to_string());
            // Update model
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                ws.focus_panel(panel_id);
            }
            drop(tm);
            state.shared.notify_ui_refresh();
        });
    }
    tab.add_controller(click);

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

    // Drop target for reordering
    let drop_target = gtk4::DropTarget::new(glib::Type::STRING, gdk4::DragAction::MOVE);
    {
        let state = Rc::clone(state);
        let target_index = tab_index;
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

            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(source_panel_id) {
                ws.layout
                    .reorder_panel_in_pane(source_panel_id, target_index);
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
    let first_widget =
        build_layout(workspace_id, first, panels, attention_panel_id, focused_panel_id, state);
    let second_widget =
        build_layout(workspace_id, second, panels, attention_panel_id, focused_panel_id, state);

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
