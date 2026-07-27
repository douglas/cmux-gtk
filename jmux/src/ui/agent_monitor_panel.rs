//! Read-only sub-agent monitor pane.
//!
//! Renders one Claude Code subagent transcript (`agent-<id>.jsonl` under the
//! session's `subagents/` directory) inside a real ghostty terminal surface,
//! exactly the way Claude Code's own task view shows it (press ↓ then Enter on
//! a running task): the task prompt, thinking, `⏺` assistant lines, tool
//! invocations, and dimmed `⎿` results — following new entries live.
//!
//! The surface runs `jmux-cli agent view <transcript>`, which puts the tty in
//! no-echo mode and swallows input, so the pane is effectively read-only and
//! has no prompt — all steering stays with the primary agent's terminal. The
//! pane is never given keyboard focus (no click-to-focus controller) and is
//! never persisted to session snapshots.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::model::Panel;

/// Build the monitor widget for an `AgentMonitor` panel: a ghostty surface
/// running the read-only transcript viewer.
pub fn create_agent_monitor_widget(
    panel: &Panel,
    is_focused: bool,
    state: &Rc<AppState>,
) -> gtk4::Widget {
    let overlay = gtk4::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    // Same tag as terminals so focus-visual updates can dim/undim in place.
    container.add_css_class("pane-container");
    container.add_css_class("agent-monitor-panel");
    if is_focused {
        container.add_css_class("focused-panel");
    }

    let Some(transcript) = panel.markdown_file.clone() else {
        let label = gtk4::Label::new(Some("(no transcript path)"));
        label.set_vexpand(true);
        container.append(&label);
        overlay.set_child(Some(&container));
        return overlay.upcast();
    };

    // `jmux-cli agent view <transcript>` — resolved next to the running binary,
    // falling back to PATH. Ghostty runs this via `/bin/sh -c`, so the path is
    // shell-quoted.
    let command = format!("{} agent view {}", cli_binary(), shell_quote(&transcript));

    let gl_surface = state.terminal_surface_for(panel.id, panel.directory.as_deref(), Some(&command));
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
    gl_surface.queue_resize();

    container.set_widget_name(&panel.id.to_string());
    overlay.set_child(Some(&container));

    // Dim overlay when not focused, matching terminal panes. Toggled in place
    // by window::update_focus_visuals so focus changes never rebuild the pane.
    let inactive_overlay = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    inactive_overlay.set_hexpand(true);
    inactive_overlay.set_vexpand(true);
    inactive_overlay.add_css_class("inactive-pane-overlay");
    inactive_overlay.set_can_target(false);
    inactive_overlay.set_visible(!is_focused);
    overlay.add_overlay(&inactive_overlay);

    // A small "read-only" badge in the top-right so the pane reads as a monitor
    // rather than an interactive terminal.
    let badge = gtk4::Label::new(Some("read-only"));
    badge.add_css_class("agent-monitor-badge");
    badge.set_halign(gtk4::Align::End);
    badge.set_valign(gtk4::Align::Start);
    badge.set_margin_top(6);
    badge.set_margin_end(6);
    badge.set_can_target(false);
    overlay.add_overlay(&badge);

    // The animated "working" Claude sprite in the top-left — the same octopus
    // the sidebar/overview use — marks the pane as a non-interactive sub-agent
    // that's busy working, not a terminal awaiting input.
    let sprite = crate::ui::state_sprite::sprite_image(
        crate::model::claude_state::ClaudeState::Working,
        &container,
    );
    sprite.set_tooltip_text(Some("Sub-agent working (read-only)"));
    sprite.set_halign(gtk4::Align::Start);
    sprite.set_valign(gtk4::Align::Start);
    sprite.set_margin_top(6);
    sprite.set_margin_start(6);
    sprite.set_can_target(false);
    overlay.add_overlay(&sprite);

    overlay.upcast()
}

/// Path to the `jmux-cli` binary: sibling of the running executable when
/// present (installed or dev layout), else the bare name resolved via PATH.
fn cli_binary() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("jmux-cli");
            if candidate.is_file() {
                return shell_quote(&candidate.to_string_lossy());
            }
        }
    }
    "jmux-cli".to_string()
}

/// Single-quote a string for `/bin/sh`, escaping embedded single quotes.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
