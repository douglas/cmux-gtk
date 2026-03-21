//! V1 text protocol parser.
//!
//! Shell integration scripts send simple text lines like:
//!   `report_pwd /home/user --tab=abc123 --panel=def456`
//!
//! This module parses them and translates to V2 JSON dispatch.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::app::SharedState;
use crate::socket::v2;

/// Check if a line looks like a V1 text command (not JSON).
pub fn is_v1(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with('{')
}

/// Parse and dispatch a V1 text line. Returns the JSON response string.
pub fn dispatch(line: &str, state: &Arc<SharedState>) -> String {
    let trimmed = line.trim();
    let (command, rest) = match trimmed.split_once(' ') {
        Some((cmd, rest)) => (cmd, rest.trim()),
        None => (trimmed, ""),
    };

    // Extract --key=value flags and positional args
    let (args, flags) = parse_args(rest);

    let workspace_id = flags.get("tab").or_else(|| flags.get("workspace"));
    let panel_id = flags.get("panel").or_else(|| flags.get("surface"));

    let (method, params) = match command {
        "report_pwd" => {
            let dir = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"directory": dir});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("workspace.report_pwd", p)
        }
        "report_git_branch" => {
            let branch = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"branch": branch});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_git_branch", p)
        }
        "clear_git_branch" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_git_branch", json!({"branch": "", "workspace": ws_or_null(workspace_id)}))
        }
        "report_pr" => {
            let status = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"status": status});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_pr", p)
        }
        "clear_pr" => {
            let mut p = json!({"status": ""});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_pr", p)
        }
        "report_tty" => {
            let tty = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"tty": tty});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("workspace.report_tty", p)
        }
        "ports_kick" => ("workspace.ports_kick", json!({})),
        "report_shell_state" => {
            let state_val = args.first().map(|s| s.as_str()).unwrap_or("prompt");
            let mut p = json!({"state": state_val});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.set_status", p)
        }
        "ping" => ("system.ping", json!({})),
        "report_ports" => {
            let ports: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let mut p = json!({"ports": ports});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_ports", p)
        }
        "report_meta" => {
            // report_meta <key> <value> [--icon=X] [--color=X] [--url=X]
            //   [--priority=N] [--format=plain|markdown] [--tab=X]
            let key = args.first().map(|s| s.as_str()).unwrap_or("");
            let value = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"key": key, "value": value});
            if let Some(icon) = flags.get("icon") {
                p["icon"] = json!(icon);
            }
            if let Some(color) = flags.get("color") {
                p["color"] = json!(color);
            }
            if let Some(url) = flags.get("url") {
                p["url"] = json!(url);
            }
            if let Some(priority) = flags.get("priority") {
                if let Ok(n) = priority.parse::<i64>() {
                    p["priority"] = json!(n);
                }
            }
            if let Some(format) = flags.get("format") {
                p["format"] = json!(format);
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_meta", p)
        }
        "report_meta_block" => {
            // report_meta_block <key> <content> [--priority=N] [--tab=X]
            let key = args.first().map(|s| s.as_str()).unwrap_or("");
            let content = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"key": key, "content": content});
            if let Some(priority) = flags.get("priority") {
                if let Ok(n) = priority.parse::<i64>() {
                    p["priority"] = json!(n);
                }
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_meta_block", p)
        }
        "clear_meta" => {
            let mut p = json!({});
            if let Some(key) = args.first() {
                p["key"] = json!(key);
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.clear_meta", p)
        }
        "clear_meta_block" => {
            let mut p = json!({});
            if let Some(key) = args.first() {
                p["key"] = json!(key);
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.clear_meta_block", p)
        }
        "list_meta" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.list_meta", p)
        }
        "list_meta_blocks" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.list_meta_blocks", p)
        }

        // ── System commands ──────────────────────────────────────────
        "capabilities" => ("system.capabilities", json!({})),
        "identify" => ("system.identify", json!({})),
        "tree" => ("system.tree", json!({})),
        "help" => {
            let help_text = V1_HELP_TEXT;
            return format!(
                "{{\"ok\":true,\"result\":{{\"help\":{}}}}}\n",
                serde_json::to_string(help_text).unwrap_or_default()
            );
        }

        // ── Window commands ──────────────────────────────────────────
        "list_windows" | "window_list" => ("window.list", json!({})),
        "current_window" | "window_current" => ("window.current", json!({})),
        "new_window" | "window_new" => ("window.new", json!({})),
        "focus_window" | "window_focus" => {
            let id_str = args.first().map(|s| s.as_str()).unwrap_or("");
            ("window.focus", json!({"window": id_str}))
        }
        "close_window" | "window_close" => {
            let id_str = args.first().map(|s| s.as_str()).unwrap_or("");
            ("window.close", json!({"window": id_str}))
        }

        // ── Workspace commands ───────────────────────────────────────
        "list_workspaces" | "workspace_list" | "list" => {
            ("workspace.list", json!({}))
        }
        "new_workspace" | "workspace_new" | "new" => {
            let mut p = json!({});
            if let Some(dir) = args.first() {
                p["directory"] = json!(dir);
            }
            ("workspace.new", p)
        }
        "select_workspace" | "workspace_select" | "select" => {
            let target = args.first().map(|s| s.as_str()).unwrap_or("");
            // Accept both UUID and numeric index
            if let Ok(idx) = target.parse::<usize>() {
                ("workspace.select", json!({"index": idx}))
            } else {
                ("workspace.select", json!({"workspace": target}))
            }
        }
        "current_workspace" | "workspace_current" | "current" => {
            ("workspace.current", json!({}))
        }
        "close_workspace" | "workspace_close" => {
            let mut p = json!({});
            if let Some(ws) = args.first().or(workspace_id) {
                p["workspace"] = json!(ws);
            }
            ("workspace.close", p)
        }
        "next_workspace" | "workspace_next" | "next" => {
            ("workspace.next", json!({}))
        }
        "previous_workspace" | "workspace_previous" | "prev" => {
            ("workspace.previous", json!({}))
        }
        "last_workspace" | "workspace_last" | "last" => {
            ("workspace.last", json!({}))
        }
        "latest_unread" => ("workspace.latest_unread", json!({})),
        "rename_workspace" | "workspace_rename" | "rename" => {
            let name = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"name": name});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.rename", p)
        }
        "reorder_workspace" | "workspace_reorder" => {
            let idx = args.first()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let mut p = json!({"index": idx});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.reorder", p)
        }
        "workspace_action" => {
            let action = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"action": action});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.action", p)
        }
        "move_workspace_to_window" => {
            let window = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"window": window});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.move_to_window", p)
        }

        // ── Status / Progress / Log ──────────────────────────────────
        "set_status" => {
            let key = args.first().map(|s| s.as_str()).unwrap_or("");
            let value = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"key": key, "value": value});
            if let Some(icon) = flags.get("icon") {
                p["icon"] = json!(icon);
            }
            if let Some(color) = flags.get("color") {
                p["color"] = json!(color);
            }
            if let Some(url) = flags.get("url") {
                p["url"] = json!(url);
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.set_status", p)
        }
        "clear_status" => {
            let mut p = json!({});
            if let Some(key) = args.first() {
                p["key"] = json!(key);
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.clear_status", p)
        }
        "list_status" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.list_status", p)
        }
        "set_progress" => {
            let value = args.first()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let mut p = json!({"value": value});
            if let Some(label) = args.get(1) {
                p["label"] = json!(label);
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.set_progress", p)
        }
        "clear_progress" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.clear_progress", p)
        }
        "log" | "append_log" => {
            let message = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"message": message});
            if let Some(level) = flags.get("level") {
                p["level"] = json!(level);
            }
            if let Some(source) = flags.get("source") {
                p["source"] = json!(source);
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.append_log", p)
        }
        "clear_log" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.clear_log", p)
        }
        "list_log" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.list_log", p)
        }

        // ── Surface / Pane commands ──────────────────────────────────
        "list_surfaces" | "surface_list" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("surface.list", p)
        }
        "current_surface" | "surface_current" => {
            ("surface.current", json!({}))
        }
        "focus_surface" | "surface_focus" => {
            let sid = args.first().or(panel_id)
                .map(|s| s.as_str()).unwrap_or("");
            ("surface.focus", json!({"surface": sid}))
        }
        "send" => {
            // send <text> — to focused surface
            let text = args.join(" ");
            ("surface.send_input", json!({"text": text}))
        }
        "send_surface" => {
            // send_surface <surface_id> <text>
            let sid = args.first().map(|s| s.as_str()).unwrap_or("");
            let text = args.get(1..).map(|a| a.join(" ")).unwrap_or_default();
            ("surface.send_input", json!({"surface": sid, "text": text}))
        }
        "send_key" => {
            let key = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"key": key});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("surface.send_key", p)
        }
        "send_key_surface" => {
            let sid = args.first().map(|s| s.as_str()).unwrap_or("");
            let key = args.get(1).map(|s| s.as_str()).unwrap_or("");
            ("surface.send_key", json!({"surface": sid, "key": key}))
        }
        "new_split" | "split" => {
            let direction = args.first().map(|s| s.as_str()).unwrap_or("right");
            let orientation = match direction {
                "down" | "vertical" | "v" => "vertical",
                _ => "horizontal",
            };
            let panel_type = flags.get("type").map(|s| s.as_str()).unwrap_or("terminal");
            let mut p = json!({"orientation": orientation, "panel_type": panel_type});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("pane.new", p)
        }
        "close_surface" | "surface_close" => {
            let sid = args.first().or(panel_id)
                .map(|s| s.as_str()).unwrap_or("");
            ("surface.close", json!({"surface": sid}))
        }
        "read_screen" | "read_text" => {
            let mut p = json!({});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            if let Some(point) = flags.get("point") {
                p["point"] = json!(point);
            }
            ("surface.read_text", p)
        }
        "clear_history" => {
            let mut p = json!({});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("surface.clear_history", p)
        }
        "surface_health" => {
            let sid = args.first().or(panel_id)
                .map(|s| s.as_str()).unwrap_or("");
            ("surface.health", json!({"surface": sid}))
        }
        "list_panes" | "pane_list" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("pane.list", p)
        }
        "focus_direction" => {
            let dir = args.first().map(|s| s.as_str()).unwrap_or("right");
            let mut p = json!({"direction": dir});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("pane.focus_direction", p)
        }
        "swap_panes" | "pane_swap" => {
            let a = args.first().map(|s| s.as_str()).unwrap_or("");
            let b = args.get(1).map(|s| s.as_str()).unwrap_or("");
            ("pane.swap", json!({"surface_a": a, "surface_b": b}))
        }
        "resize_pane" | "pane_resize" => {
            let delta = args.first()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let mut p = json!({"delta": delta});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("pane.resize", p)
        }
        "break_pane" | "pane_break" => {
            let mut p = json!({});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("pane.break", p)
        }
        "equalize" | "pane_equalize" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("pane.equalize", p)
        }

        // ── Notification commands ────────────────────────────────────
        "notify" => {
            let title = args.first().map(|s| s.as_str()).unwrap_or("");
            let body = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"title": title, "body": body});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("notification.create", p)
        }
        "notify_surface" => {
            let sid = args.first().map(|s| s.as_str()).unwrap_or("");
            let title = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let body = args.get(2).map(|s| s.as_str()).unwrap_or("");
            ("notification.create", json!({"surface": sid, "title": title, "body": body}))
        }
        "notify_target" => {
            let ws_id_arg = args.first().map(|s| s.as_str()).unwrap_or("");
            let sid = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let title = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let body = args.get(3).map(|s| s.as_str()).unwrap_or("");
            ("notification.create", json!({
                "workspace": ws_id_arg, "surface": sid,
                "title": title, "body": body
            }))
        }
        "list_notifications" => ("notification.list", json!({})),
        "clear_notifications" => ("notification.clear", json!({})),

        // ── Browser commands (V1 style) ──────────────────────────────
        "open_browser" => {
            let mut p = json!({"panel_type": "browser"});
            if let Some(url) = args.first() {
                p["url"] = json!(url);
            }
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("pane.new", p)
        }
        "navigate" => {
            let url = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"url": url});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("browser.navigate", p)
        }
        "browser_back" => {
            let mut p = json!({});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("browser.back", p)
        }
        "browser_forward" => {
            let mut p = json!({});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("browser.forward", p)
        }
        "browser_reload" => {
            let mut p = json!({});
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("browser.reload", p)
        }

        // ── Settings / Markdown ──────────────────────────────────────
        "settings" | "open_settings" => ("settings.open", json!({})),
        "open_markdown" | "markdown" => {
            let path = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"file_path": path});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("markdown.open", p)
        }

        // ── Agent / sidebar state ────────────────────────────────────
        "set_agent_pid" => {
            let pid = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"key": "agent_pid", "value": pid});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.set_status", p)
        }
        "clear_agent_pid" => {
            let mut p = json!({"key": "agent_pid"});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.clear_status", p)
        }
        "sidebar_state" => ("workspace.current", json!({})),
        "reset_sidebar" => {
            // Clear all status/metadata/logs for current workspace
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.clear_status", p)
        }
        "reload_config" => ("settings.open", json!({})),
        "report_review" => {
            // Alias for report_pr
            let status = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"status": status});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_pr", p)
        }
        "new_surface" => {
            let panel_type = flags.get("type").map(|s| s.as_str()).unwrap_or("terminal");
            let mut p = json!({"panel_type": panel_type});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("pane.new", p)
        }
        "focus_pane" => {
            let pid = args.first().map(|s| s.as_str()).unwrap_or("");
            ("pane.focus", json!({"surface": pid}))
        }
        "list_pane_surfaces" => {
            let pid = args.first().map(|s| s.as_str()).unwrap_or("");
            ("pane.surfaces", json!({"surface": pid}))
        }
        "refresh_surfaces" => {
            ("surface.refresh", json!({}))
        }

        // ── Auth ─────────────────────────────────────────────────────
        "auth" => {
            // Simple auth check — returns OK if the connection was accepted
            return "{\"ok\":true,\"result\":{\"authenticated\":true}}\n".to_string();
        }

        _ => {
            return format!(
                "{{\"ok\":false,\"error\":{{\"code\":\"unknown_v1_command\",\"message\":\"Unknown command: {}\"}}}}\n",
                command
            );
        }
    };

    // Build a V2 JSON request and dispatch it
    let v2_json = json!({
        "id": "v1",
        "method": method,
        "params": params,
    });

    let response = v2::dispatch(&v2_json.to_string(), state);
    match serde_json::to_string(&response) {
        Ok(mut s) => {
            s.push('\n');
            s
        }
        Err(_) => "{\"ok\":false,\"error\":{\"code\":\"internal\",\"message\":\"serialization failed\"}}\n".to_string(),
    }
}

fn ws_or_null(ws: Option<&String>) -> Value {
    ws.map(|s| json!(s)).unwrap_or(Value::Null)
}

const V1_HELP_TEXT: &str = "\
cmux V1 text protocol commands (send over Unix socket):

SYSTEM
  ping                              Heartbeat check
  capabilities                      List all V2 methods
  identify                          Server identity
  tree                              Full state tree
  auth                              Auth check
  help                              This help text

WINDOWS
  list_windows                      List all windows
  current_window                    Get focused window
  new_window                        Create window
  focus_window <id>                 Focus window
  close_window <id>                 Close window

WORKSPACES
  list                              List workspaces
  new [dir]                         Create workspace
  select <id|index>                 Select workspace
  current                           Current workspace info
  close [--tab=ID]                  Close workspace
  next / prev / last                Navigate workspaces
  latest_unread                     Jump to unread
  rename <name> [--tab=ID]          Rename workspace
  reorder <index> [--tab=ID]        Reorder workspace
  workspace_action <action>         Workspace action (pin/close_others/...)
  move_workspace_to_window <wid>    Move workspace to window

STATUS & METADATA
  set_status <key> <value> [--icon=X] [--color=X] [--url=X]
  clear_status [key]                Clear status entries
  list_status                       List status entries
  set_progress <0.0-1.0> [label]   Set progress bar
  clear_progress                    Clear progress
  log <message> [--level=X] [--source=X]  Append log
  clear_log / list_log              Manage log entries
  report_meta <key> <value> [--icon=X] [--color=X] [--priority=N]
  report_meta_block <key> <content> [--priority=N]
  clear_meta [key] / list_meta      Manage metadata
  clear_meta_block [key] / list_meta_blocks

SHELL INTEGRATION
  report_pwd <dir>                  Report working directory
  report_git_branch <branch>        Report git branch
  clear_git_branch                  Clear git info
  report_pr <status>                Report PR status
  clear_pr                          Clear PR
  report_tty <tty>                  Report TTY name
  report_shell_state <state>        Shell activity (prompt/running)
  report_ports <p1> <p2> ...        Report ports
  ports_kick                        Trigger port rescan

SURFACES & PANES
  list_surfaces                     List panels
  current_surface                   Current panel
  focus_surface <id>                Focus panel
  send <text>                       Send to focused terminal
  send_surface <id> <text>          Send to specific terminal
  send_key <key>                    Send key event
  split [right|down] [--type=browser|terminal]
  close_surface <id>                Close panel
  read_screen [--panel=ID]          Read terminal text
  clear_history                     Clear scrollback
  list_panes                        List panes
  focus_direction <left|right|up|down>
  swap_panes <a> <b>                Swap panes
  resize_pane <delta>               Resize split
  break_pane                        Break to new workspace
  equalize                          Reset dividers

NOTIFICATIONS
  notify <title> [body]             Send notification
  notify_surface <id> <title> [body]
  notify_target <ws> <srf> <title> [body]
  list_notifications                List all
  clear_notifications               Clear all

BROWSER
  open_browser [url]                Open browser split
  navigate <url>                    Navigate to URL
  browser_back / browser_forward / browser_reload

OTHER
  settings                          Open settings
  markdown <path>                   Open markdown file
";


/// Parse "arg1 arg2 --flag=value --other=val" into (positional_args, flags).
/// Supports quoted arguments: `"path with spaces"`.
fn parse_args(input: &str) -> (Vec<String>, std::collections::HashMap<String, String>) {
    let mut args = Vec::new();
    let mut flags = std::collections::HashMap::new();

    let mut chars = input.chars().peekable();
    while chars.peek().is_some() {
        // Skip whitespace
        while chars.peek() == Some(&' ') {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }

        let mut token = String::new();
        if chars.peek() == Some(&'"') {
            // Quoted string
            chars.next(); // consume opening quote
            while let Some(&ch) = chars.peek() {
                if ch == '"' {
                    chars.next();
                    break;
                }
                token.push(ch);
                chars.next();
            }
        } else {
            // Unquoted token
            while let Some(&ch) = chars.peek() {
                if ch == ' ' {
                    break;
                }
                token.push(ch);
                chars.next();
            }
        }

        if let Some(flag) = token.strip_prefix("--") {
            if let Some((key, value)) = flag.split_once('=') {
                flags.insert(key.to_string(), value.to_string());
            } else {
                flags.insert(flag.to_string(), String::new());
            }
        } else {
            args.push(token);
        }
    }

    (args, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_v1() {
        assert!(is_v1("report_pwd /home/user"));
        assert!(is_v1("ping"));
        assert!(!is_v1("{\"id\": 1}"));
        assert!(!is_v1("  {\"method\": \"test\"}  "));
    }

    #[test]
    fn test_parse_args() {
        let (args, flags) = parse_args(r#"/home/user --tab=abc --panel=def"#);
        assert_eq!(args, vec!["/home/user"]);
        assert_eq!(flags.get("tab").unwrap(), "abc");
        assert_eq!(flags.get("panel").unwrap(), "def");
    }

    #[test]
    fn test_parse_quoted_args() {
        let (args, flags) = parse_args(r#""/path with spaces" --tab=abc"#);
        assert_eq!(args, vec!["/path with spaces"]);
        assert_eq!(flags.get("tab").unwrap(), "abc");
    }
}
