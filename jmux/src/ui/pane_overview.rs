//! Pane overview — a full-window view of status tiles for every pane in every
//! workspace, grouped into clearly labelled per-workspace sections. Each tile
//! shows the pane's type, title, directory, a status dot (busy / idle /
//! attention / browser), and a one-line activity snippet. Clicking a tile (or
//! arrow-keys + Enter) selects that workspace and jumps to that pane.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use uuid::Uuid;

use crate::app::{lock_or_recover, AppState};
use crate::model::claude_state::{classify, is_shell_title, ClaudeState};
use crate::model::PanelType;

struct TileInfo {
    panel_id: Uuid,
    title: String,
    directory: String,
    activity: String,
    icon: &'static str,
    dot: &'static str,
    dot_class: &'static str,
    focused: bool,
    /// Claude working/needs-input/waiting state, when this pane is a live
    /// agent — drives the octopus sprite on the tile.
    claude_state: Option<ClaudeState>,
}

struct WorkspaceSection {
    workspace_id: Uuid,
    title: String,
    is_current: bool,
    unread_count: u32,
    tiles: Vec<TileInfo>,
}

/// Show the pane overview for all workspaces.
pub fn show_pane_overview(parent: &adw::ApplicationWindow, state: &Rc<AppState>) {
    let sections = collect_sections(state);

    // An in-surface adw::Dialog (not a separate top-level window) so it renders
    // above the content — including the layer-shell quake drop-down, which sits
    // on the overlay layer above any normal window.
    let dialog = adw::Dialog::new();
    dialog.set_title("Overview");
    dialog.set_content_width(900);
    dialog.set_content_height(620);

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let pane_count: usize = sections.iter().map(|s| s.tiles.len()).sum();
    let attention = sections
        .iter()
        .flat_map(|s| s.tiles.iter())
        .filter(|t| t.dot_class == "overview-dot-attention")
        .count();
    let mut subtitle = format!(
        "{pane_count} panes across {} workspaces",
        sections.len()
    );
    if attention > 0 {
        subtitle.push_str(&format!(" · {attention} need attention"));
    }
    header.set_title_widget(Some(&adw::WindowTitle::new("Overview", &subtitle)));
    toolbar.add_top_bar(&header);

    if pane_count == 0 {
        let empty = gtk4::Label::new(Some("No panes open."));
        empty.add_css_class("dim-label");
        empty.set_vexpand(true);
        toolbar.set_content(Some(&empty));
        dialog.set_child(Some(&toolbar));
        dialog.present(Some(parent));
        return;
    }

    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 20);
    column.set_margin_start(16);
    column.set_margin_end(16);
    column.set_margin_top(16);
    column.set_margin_bottom(16);

    let mut current_header: Option<gtk4::Widget> = None;
    for section in &sections {
        let widget = build_section(section, state, &dialog);
        if section.is_current {
            current_header = Some(widget.clone());
        }
        column.append(&widget);
    }

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&column));
    toolbar.set_content(Some(&scrolled));
    dialog.set_child(Some(&toolbar));

    // Scroll the current workspace's section into view once sizes are known.
    if let Some(widget) = current_header {
        let scrolled = scrolled.clone();
        gtk4::glib::idle_add_local_once(move || {
            if let Some((_, y)) = widget.translate_coordinates(&scrolled, 0.0, 0.0) {
                let adj = scrolled.vadjustment();
                adj.set_value((adj.value() + y).clamp(adj.lower(), adj.upper()));
            }
        });
    }

    // adw::Dialog closes on Escape natively — no key controller needed.
    dialog.present(Some(parent));
}

fn build_section(
    section: &WorkspaceSection,
    state: &Rc<AppState>,
    dialog: &adw::Dialog,
) -> gtk4::Widget {
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 8);

    // Section header: workspace name + current badge + pane/unread counts.
    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let name = gtk4::Label::new(Some(&section.title));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name.add_css_class("heading");
    head.append(&name);
    if section.is_current {
        let badge = gtk4::Label::new(Some("current"));
        badge.add_css_class("overview-ws-current");
        head.append(&badge);
    }
    let mut meta = format!("{} panes", section.tiles.len());
    if section.tiles.len() == 1 {
        meta = "1 pane".to_string();
    }
    if section.unread_count > 0 {
        meta.push_str(&format!(" · {} unread", section.unread_count));
    }
    let counts = gtk4::Label::new(Some(&meta));
    counts.add_css_class("dim-label");
    counts.add_css_class("caption");
    head.append(&counts);
    let rule = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    rule.set_hexpand(true);
    rule.set_valign(gtk4::Align::Center);
    head.append(&rule);
    vbox.append(&head);

    if section.tiles.is_empty() {
        let empty = gtk4::Label::new(Some("No panes in this workspace."));
        empty.set_xalign(0.0);
        empty.add_css_class("dim-label");
        empty.add_css_class("caption");
        vbox.append(&empty);
        return vbox.upcast();
    }

    let flow = gtk4::FlowBox::new();
    flow.set_selection_mode(gtk4::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(4);
    flow.set_row_spacing(12);
    flow.set_column_spacing(12);

    for tile in &section.tiles {
        flow.append(&build_tile(tile, section.workspace_id, state, dialog));
    }
    vbox.append(&flow);

    vbox.upcast()
}

fn build_tile(
    tile: &TileInfo,
    workspace_id: Uuid,
    state: &Rc<AppState>,
    dialog: &adw::Dialog,
) -> gtk4::Widget {
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.set_size_request(200, 120);
    vbox.add_css_class("overview-tile");

    // Header line: status dot + type icon + title + (working octopus).
    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let dot = gtk4::Label::new(Some(tile.dot));
    dot.add_css_class(tile.dot_class);
    head.append(&dot);
    let icon = gtk4::Image::from_icon_name(tile.icon);
    icon.set_pixel_size(14);
    head.append(&icon);
    let title = gtk4::Label::new(Some(&tile.title));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.add_css_class("heading");
    head.append(&title);
    // Animated deck octopus when an agent is live in this pane, so the overview
    // shows at a glance which panes are working / need input / waiting.
    if let Some(cs) = tile.claude_state {
        let sprite = crate::ui::state_sprite::sprite_image(cs, &title);
        sprite.set_valign(gtk4::Align::Center);
        head.append(&sprite);
    }
    vbox.append(&head);

    if !tile.directory.is_empty() {
        let dir = gtk4::Label::new(Some(&tile.directory));
        dir.set_xalign(0.0);
        dir.add_css_class("dim-label");
        dir.add_css_class("caption");
        dir.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        vbox.append(&dir);
    }

    let activity = gtk4::Label::new(Some(if tile.activity.is_empty() {
        "—"
    } else {
        &tile.activity
    }));
    activity.set_xalign(0.0);
    activity.set_yalign(0.0);
    activity.set_vexpand(true);
    activity.set_wrap(true);
    activity.set_lines(2);
    activity.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    activity.add_css_class("dim-label");
    activity.add_css_class("monospace");
    activity.add_css_class("caption");
    vbox.append(&activity);

    let button = gtk4::Button::new();
    button.set_child(Some(&vbox));
    button.add_css_class("overview-tile-button");
    if tile.focused {
        button.add_css_class("overview-tile-focused");
    }
    button.set_tooltip_text(Some(&tile.title));

    let panel_id = tile.panel_id;
    let state = Rc::clone(state);
    // WEAK: the button is a descendant of the dialog — a strong capture
    // leaked the whole overview (and kept its sprite timers alive) per open.
    let dialog = dialog.downgrade();
    button.connect_clicked(move |_| {
        let Some(dialog) = dialog.upgrade() else {
            return;
        };
        {
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.select_by_id(workspace_id);
            if let Some(ws) = tm.selected_mut() {
                ws.focus_panel(panel_id);
            }
        }
        state.shared.notify_ui_refresh();
        dialog.close();
    });

    button.upcast()
}

fn collect_sections(state: &Rc<AppState>) -> Vec<WorkspaceSection> {
    let tm = lock_or_recover(&state.shared.tab_manager);
    let selected_id = tm.selected_id();

    tm.iter()
        .map(|ws| {
            let is_current = selected_id == Some(ws.id);
            let focused = ws.focused_panel_id;
            let attention = ws.attention_panel_id;

            let tiles = ws
                .layout
                .all_panel_ids()
                .into_iter()
                .filter_map(|pid| {
                    let panel = ws.panels.get(&pid)?;
                    let is_browser = panel.panel_type == PanelType::Browser;
                    // Monitor panes render in a ghostty surface too, so their
                    // live output is readable the same way as a terminal's.
                    let has_screen = matches!(
                        panel.panel_type,
                        PanelType::Terminal | PanelType::AgentMonitor
                    );

                    // Last non-blank on-screen line, for the activity snippet
                    // and (for terminals) Claude-state classification.
                    let screen_text = if has_screen {
                        state
                            .terminal_cache
                            .borrow()
                            .get(&pid)
                            .and_then(|s| s.read_screen_text())
                    } else {
                        None
                    };
                    let last_line = screen_text.as_deref().map(|t| {
                        t.lines()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("")
                            .chars()
                            .take(120)
                            .collect::<String>()
                    });

                    // Activity snippet + busy status.
                    let (activity, busy) = if panel.panel_type == PanelType::Terminal {
                        (last_line.clone().unwrap_or_default(), crate::app::pane_is_busy(pid))
                    } else if panel.panel_type == PanelType::AgentMonitor {
                        (last_line.clone().unwrap_or_default(), None)
                    } else if is_browser {
                        (panel.browser_url.clone().unwrap_or_default(), None)
                    } else {
                        (panel.directory.clone().unwrap_or_default(), None)
                    };

                    // Octopus state: only for terminal agent panes (non-shell,
                    // not hibernated), mirroring the sidebar sprite logic.
                    let claude_state = if panel.panel_type == PanelType::Terminal {
                        let raw_title = panel.title.as_deref().unwrap_or("");
                        if is_shell_title(raw_title) || state.shared.is_hibernated(&pid) {
                            None
                        } else {
                            screen_text.as_deref().and_then(|t| classify(t, raw_title))
                        }
                    } else {
                        None
                    };

                    let (dot, dot_class) = if attention == Some(pid) {
                        ("●", "overview-dot-attention")
                    } else if is_browser {
                        ("⬤", "overview-dot-browser")
                    } else {
                        match busy {
                            Some(true) => ("●", "overview-dot-busy"),
                            Some(false) => ("○", "overview-dot-idle"),
                            None => ("•", "overview-dot-idle"),
                        }
                    };

                    Some(TileInfo {
                        panel_id: pid,
                        title: panel.display_title().to_string(),
                        directory: panel.directory.clone().unwrap_or_default(),
                        activity,
                        icon: icon_for(panel.panel_type),
                        dot,
                        dot_class,
                        focused: is_current && focused == Some(pid),
                        claude_state,
                    })
                })
                .collect();

            WorkspaceSection {
                workspace_id: ws.id,
                title: ws.display_title().to_string(),
                is_current,
                unread_count: ws.unread_count,
                tiles,
            }
        })
        .collect()
}

fn icon_for(kind: PanelType) -> &'static str {
    match kind {
        PanelType::Terminal => "utilities-terminal-symbolic",
        PanelType::Browser => "globe-symbolic",
        PanelType::Markdown => "document-open-symbolic",
        PanelType::Diff => "media-flash-symbolic",
        PanelType::Project => "view-list-symbolic",
        PanelType::FilePreview => "text-x-generic-symbolic",
        PanelType::Notes => "accessories-text-editor-symbolic",
        PanelType::History => "document-open-recent-symbolic",
        PanelType::Vault => "drive-multidisk-symbolic",
        PanelType::AgentMonitor => "utilities-system-monitor-symbolic",
        PanelType::Graph => "view-grid-symbolic",
    }
}
