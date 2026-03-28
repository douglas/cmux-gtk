//! Surface handler functions for the v2 JSON protocol.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{lock_or_recover, SharedState, UiEvent};
use crate::model::panel::{Direction, SplitOrientation};

use super::helpers::*;
use super::{Response, MAX_SURFACE_INPUT_LEN};

pub(super) fn handle_surface_send_input(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(input) = params.get("input").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'input'");
    };
    // Limit input size to prevent unbounded memory growth via the channel
    let input = crate::model::workspace::truncate_str(input, MAX_SURFACE_INPUT_LEN);

    let explicit_panel_id = match params
        .get("surface")
        .or_else(|| params.get("panel"))
        .and_then(|v| if v.is_null() { None } else { Some(v) })
    {
        Some(v) => {
            let Some(s) = v.as_str() else {
                return Response::error(id, "invalid_params", "surface/panel must be a string");
            };
            match uuid::Uuid::parse_str(s) {
                Ok(uuid) => Some(uuid),
                Err(_) => {
                    return Response::error(
                        id,
                        "invalid_params",
                        "Invalid surface/panel UUID format",
                    )
                }
            }
        }
        None => None,
    };

    let panel_id = {
        let tab_manager = lock_or_recover(&state.tab_manager);
        if let Some(panel_id) = explicit_panel_id {
            if tab_manager.find_workspace_with_panel(panel_id).is_none() {
                return Response::error(id, "not_found", "Surface not found");
            }
            panel_id
        } else if let Some(workspace) = tab_manager.selected() {
            let Some(panel_id) = workspace
                .focused_panel_id
                .or_else(|| workspace.panel_ids().into_iter().next())
            else {
                return Response::error(id, "not_found", "No focused surface");
            };
            panel_id
        } else {
            return Response::error(id, "not_found", "No workspace selected");
        }
    };

    if !state.send_ui_event(UiEvent::SendInput {
        panel_id,
        text: input.to_string(),
    }) {
        return Response::error(id, "not_ready", "UI is not ready");
    }

    Response::success(
        id,
        serde_json::json!({
            "sent": true,
            "surface": panel_id.to_string(),
        }),
    )
}

pub(super) fn handle_surface_list(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    // Alias for pane.list
    super::pane::handle_pane_list(id, params, state)
}

pub(super) fn handle_surface_current(id: Value, state: &Arc<SharedState>) -> Response {
    let tm = lock_or_recover(&state.tab_manager);
    let Some(ws) = tm.selected() else {
        return Response::error(id, "not_found", "No workspace selected");
    };

    let Some(panel_id) = ws.focused_panel_id else {
        return Response::error(id, "not_found", "No focused surface");
    };

    let panel = ws.panels.get(&panel_id);
    Response::success(
        id,
        serde_json::json!({
            "id": panel_id.to_string(),
            "type": panel.map(|p| match p.panel_type {
                crate::model::PanelType::Terminal => "terminal",
                crate::model::PanelType::Browser => "browser",
                crate::model::PanelType::Markdown => "markdown",
            }).unwrap_or("unknown"),
            "title": panel.map(|p| p.display_title()).unwrap_or("?"),
            "directory": panel.and_then(|p| p.directory.as_deref()),
        }),
    )
}

pub(super) fn handle_surface_focus(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    // Alias for pane.focus
    super::pane::handle_pane_focus(id, params, state)
}

pub(super) fn handle_surface_send_key(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let key_name = params.get("key").and_then(|v| v.as_str());
    let mods_arr = params.get("mods").and_then(|v| v.as_array());

    let Some(key_name) = key_name else {
        return Response::error(
            id,
            "invalid_params",
            "Provide 'key' (e.g. 'c', 'Return', 'Escape')",
        );
    };

    // Parse modifier names to ghostty mods bitmask
    let mut mods: u32 = 0;
    if let Some(arr) = mods_arr {
        for m in arr {
            if let Some(s) = m.as_str() {
                match s.to_lowercase().as_str() {
                    "ctrl" | "control" => {
                        mods |= ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_CTRL as u32;
                    }
                    "shift" => {
                        mods |= ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_SHIFT as u32;
                    }
                    "alt" => {
                        mods |= ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_ALT as u32;
                    }
                    "super" | "meta" => {
                        mods |= ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_SUPER as u32;
                    }
                    _ => {}
                }
            }
        }
    }

    // Convert key name to GDK keyval + XKB keycode.
    let (keyval, keycode) = match resolve_key_name(key_name) {
        Some(pair) => pair,
        None => {
            return Response::error(
                id,
                "invalid_params",
                &format!("Unknown key name: '{key_name}'"),
            );
        }
    };

    // Resolve the panel
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str());
    let panel_id = if let Some(s) = panel_str {
        match uuid::Uuid::parse_str(s) {
            Ok(pid) => pid,
            Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID"),
        }
    } else {
        let tm = lock_or_recover(&state.tab_manager);
        let Some(ws) = tm.selected() else {
            return Response::error(id, "not_found", "No workspace selected");
        };
        let Some(pid) = ws.focused_panel_id else {
            return Response::error(id, "not_found", "No focused panel");
        };
        pid
    };

    state.send_ui_event(UiEvent::SendKey {
        panel_id,
        keyval,
        keycode,
        mods,
    });
    Response::success(id, serde_json::json!({"sent": true}))
}

/// Resolve a key name string to a (GDK keyval, XKB keycode) pair.
///
/// Ghostty uses the native keycode to look up the physical key in its
/// keycode table. Without a valid keycode, keys resolve to `.unidentified`
/// and produce no terminal output. XKB keycodes = evdev scancode + 8.
fn resolve_key_name(name: &str) -> Option<(u32, u32)> {
    // Single character -> use its unicode value as keyval, keycode 0
    // (Ghostty uses the text field for printable characters)
    let mut chars = name.chars();
    if let Some(ch) = chars.next() {
        if chars.next().is_none() && (ch.is_ascii_graphic() || ch == ' ') {
            return Some((ch as u32, 0));
        }
    }

    // Common key name aliases: (GDK keyval, XKB keycode)
    match name.to_lowercase().as_str() {
        "return" | "enter" => Some((0xff0d, 36)),
        "escape" | "esc" => Some((0xff1b, 9)),
        "tab" => Some((0xff09, 23)),
        "backspace" => Some((0xff08, 22)),
        "delete" | "del" => Some((0xffff, 119)),
        "space" => Some((0x0020, 65)),
        "up" | "arrow_up" => Some((0xff52, 111)),
        "down" | "arrow_down" => Some((0xff54, 116)),
        "left" | "arrow_left" => Some((0xff51, 113)),
        "right" | "arrow_right" => Some((0xff53, 114)),
        "home" => Some((0xff50, 110)),
        "end" => Some((0xff57, 115)),
        "page_up" | "pageup" => Some((0xff55, 112)),
        "page_down" | "pagedown" => Some((0xff56, 117)),
        "insert" => Some((0xff63, 118)),
        "f1" => Some((0xffbe, 67)),
        "f2" => Some((0xffbf, 68)),
        "f3" => Some((0xffc0, 69)),
        "f4" => Some((0xffc1, 70)),
        "f5" => Some((0xffc2, 71)),
        "f6" => Some((0xffc3, 72)),
        "f7" => Some((0xffc4, 73)),
        "f8" => Some((0xffc5, 74)),
        "f9" => Some((0xffc6, 75)),
        "f10" => Some((0xffc7, 76)),
        "f11" => Some((0xffc8, 95)),
        "f12" => Some((0xffc9, 96)),
        _ => None,
    }
}

pub(super) fn handle_surface_read_text(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str());

    let panel_id = if let Some(s) = panel_str {
        match uuid::Uuid::parse_str(s) {
            Ok(pid) => pid,
            Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID"),
        }
    } else {
        let tm = lock_or_recover(&state.tab_manager);
        let Some(ws) = tm.selected() else {
            return Response::error(id, "not_found", "No workspace selected");
        };
        let Some(pid) = ws.focused_panel_id else {
            return Response::error(id, "not_found", "No focused panel");
        };
        pid
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    state.send_ui_event(UiEvent::ReadText {
        panel_id,
        reply: tx,
    });

    // Block waiting for the GTK thread to reply.
    // The socket handler runs on a tokio thread so this is safe.
    match rx.blocking_recv() {
        Ok(Some(text)) => Response::success(
            id,
            serde_json::json!({
                "text": text,
            }),
        ),
        Ok(None) => Response::error(id, "not_found", "Surface not ready or not found"),
        Err(_) => Response::error(id, "internal", "GTK thread did not reply"),
    }
}

pub(super) fn handle_surface_action(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let action = params.get("action").and_then(|v| v.as_str());
    let Some(action) = action else {
        return Response::error(id, "invalid_params", "Provide 'action'");
    };

    match action {
        "toggle_zoom" => {
            let mut tm = lock_or_recover(&state.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                if ws.zoomed_panel_id == Some(panel_id) {
                    ws.zoomed_panel_id = None;
                } else {
                    ws.zoomed_panel_id = Some(panel_id);
                }
                let zoomed = ws.zoomed_panel_id.is_some();
                drop(tm);
                state.notify_ui_refresh();
                Response::success(id, serde_json::json!({"zoomed": zoomed}))
            } else {
                Response::error(id, "not_found", "Panel not found")
            }
        }
        "clear_screen" => {
            state.send_ui_event(UiEvent::ClearHistory { panel_id });
            Response::success(id, serde_json::json!({"cleared": true}))
        }
        "refresh" => {
            state.send_ui_event(UiEvent::RefreshSurface { panel_id });
            Response::success(id, serde_json::json!({"refreshed": true}))
        }
        "flash" => {
            state.send_ui_event(UiEvent::TriggerFlash { panel_id });
            Response::success(id, serde_json::json!({"flashed": true}))
        }
        _ => Response::error(
            id,
            "invalid_params",
            "Unknown action. Use: toggle_zoom, clear_screen, refresh, flash",
        ),
    }
}

pub(super) fn handle_surface_health(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let tm = lock_or_recover(&state.tab_manager);
    let exists = tm.find_workspace_with_panel(panel_id).is_some();
    drop(tm);

    Response::success(
        id,
        serde_json::json!({
            "panel_id": panel_id.to_string(),
            "exists": exists,
            "healthy": exists,
        }),
    )
}

pub(super) fn handle_surface_refresh(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = resolve_panel_id(&id, params, state);
    let panel_id = match panel_id {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    state.send_ui_event(UiEvent::RefreshSurface { panel_id });
    Response::success(id, serde_json::json!({"refreshed": true}))
}

pub(super) fn handle_surface_clear_history(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = resolve_panel_id(&id, params, state);
    let panel_id = match panel_id {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    state.send_ui_event(UiEvent::ClearHistory { panel_id });
    Response::success(id, serde_json::json!({"cleared": true}))
}

pub(super) fn handle_surface_trigger_flash(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str());

    let panel_id = if let Some(s) = panel_str {
        match uuid::Uuid::parse_str(s) {
            Ok(id) => id,
            Err(_) => return Response::error(id, "invalid_params", "Invalid panel UUID"),
        }
    } else {
        let tm = lock_or_recover(&state.tab_manager);
        let Some(ws) = tm.selected() else {
            return Response::error(id, "not_found", "No workspace selected");
        };
        let Some(pid) = ws.focused_panel_id else {
            return Response::error(id, "not_found", "No focused panel");
        };
        pid
    };

    state.send_ui_event(UiEvent::TriggerFlash { panel_id });
    Response::success(id, serde_json::json!({"flashed": true}))
}

pub(super) fn handle_surface_move(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let target_ws_str = params
        .get("workspace")
        .or_else(|| params.get("workspace_id"))
        .and_then(|v| v.as_str());
    let Some(target_ws_str) = target_ws_str else {
        return Response::error(id, "invalid_params", "Provide 'workspace' target UUID");
    };
    let target_ws_id = match uuid::Uuid::parse_str(target_ws_str) {
        Ok(wid) => wid,
        Err(_) => return Response::error(id, "invalid_params", "Invalid workspace UUID"),
    };

    let orientation = match params.get("orientation").and_then(|v| v.as_str()) {
        Some("vertical") => SplitOrientation::Vertical,
        _ => SplitOrientation::Horizontal,
    };

    let mut tm = lock_or_recover(&state.tab_manager);

    // Find source workspace
    let source_ws_id = tm.find_workspace_with_panel(panel_id).map(|ws| ws.id);
    let Some(source_ws_id) = source_ws_id else {
        return Response::error(id, "not_found", "Panel not found in any workspace");
    };

    if source_ws_id == target_ws_id {
        return Response::error(
            id,
            "invalid_params",
            "Panel is already in the target workspace",
        );
    }

    // Detach from source
    let source_ws = tm
        .workspace_mut(source_ws_id)
        .expect("source workspace validated");
    let panel = source_ws.detach_panel(panel_id);
    let Some(panel) = panel else {
        return Response::error(id, "not_found", "Panel not found");
    };
    let source_empty = tm.workspace(source_ws_id).is_some_and(|ws| ws.is_empty());
    if source_empty {
        tm.remove_by_id(source_ws_id);
    }

    // Insert into target workspace
    let Some(target_ws) = tm.workspace_mut(target_ws_id) else {
        return Response::error(id, "not_found", "Target workspace not found");
    };
    target_ws.insert_panel(panel, orientation);

    drop(tm);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "panel_id": panel_id.to_string(),
            "workspace_id": target_ws_id.to_string(),
            "moved": true,
        }),
    )
}

pub(super) fn handle_surface_reorder(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let Some(index) = params.get("index").and_then(|v| v.as_u64()) else {
        return Response::error(id, "invalid_params", "Provide 'index' (integer)");
    };
    let index = index as usize;

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = tm.find_workspace_with_panel_mut(panel_id);
    let Some(ws) = ws else {
        return Response::error(id, "not_found", "Panel not found in any workspace");
    };

    if !ws.layout.reorder_panel_in_pane(panel_id, index) {
        return Response::error(id, "not_found", "Panel not found in any pane");
    }

    drop(tm);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "panel_id": panel_id.to_string(),
            "index": index,
            "reordered": true,
        }),
    )
}

pub(super) fn handle_surface_create(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_type = match params.get("type").and_then(|v| v.as_str()) {
        Some("browser") => crate::model::PanelType::Browser,
        _ => crate::model::PanelType::Terminal,
    };

    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut new_panel = match panel_type {
        crate::model::PanelType::Terminal => crate::model::Panel::new_terminal(),
        crate::model::PanelType::Browser => crate::model::Panel::new_browser(),
        crate::model::PanelType::Markdown => crate::model::Panel::new_markdown(""),
    };
    if panel_type == crate::model::PanelType::Browser {
        new_panel.browser_url = url;
    }
    let new_panel_id = new_panel.id;

    let mut tm = lock_or_recover(&state.tab_manager);
    let Some(ws) = tm.selected_mut() else {
        return Response::error(id, "not_found", "No workspace selected");
    };

    let focused = ws.focused_panel_id;
    ws.panels.insert(new_panel_id, new_panel);

    let added = if let Some(focused_id) = focused {
        ws.layout.add_panel_to_pane(focused_id, new_panel_id)
    } else {
        false
    };

    if !added {
        // Fallback: replace root with a single pane containing the new panel
        ws.layout = crate::model::panel::LayoutNode::single_pane(new_panel_id);
    }

    ws.previous_focused_panel_id = ws.focused_panel_id;
    ws.focused_panel_id = Some(new_panel_id);

    drop(tm);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "panel_id": new_panel_id.to_string(),
            "created": true,
        }),
    )
}

pub(super) fn handle_surface_drag_to_split(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let panel_id = match resolve_panel_id(&id, params, state) {
        Ok(pid) => pid,
        Err(resp) => return resp,
    };

    let dir_str = params.get("direction").and_then(|v| v.as_str());
    let Some(dir_str) = dir_str else {
        return Response::error(
            id,
            "invalid_params",
            "Provide 'direction': left, right, up, down",
        );
    };

    let direction = match dir_str {
        "left" => Direction::Left,
        "right" => Direction::Right,
        "up" => Direction::Up,
        "down" => Direction::Down,
        _ => {
            return Response::error(
                id,
                "invalid_params",
                "direction must be: left, right, up, down",
            )
        }
    };

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws = tm.find_workspace_with_panel_mut(panel_id);
    let Some(ws) = ws else {
        return Response::error(id, "not_found", "Panel not found in any workspace");
    };

    if ws.panels.len() < 2 {
        return Response::error(
            id,
            "invalid_params",
            "Need at least 2 panels to drag to split",
        );
    }

    if ws.drag_to_split(panel_id, direction) {
        drop(tm);
        state.notify_ui_refresh();
        Response::success(
            id,
            serde_json::json!({
                "panel_id": panel_id.to_string(),
                "direction": dir_str,
                "moved": true,
            }),
        )
    } else {
        Response::error(id, "not_found", "Could not split panel")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_return_key() {
        let (keyval, keycode) = resolve_key_name("Return").unwrap();
        assert_eq!(keyval, 0xff0d, "Return keyval should be GDK_KEY_Return");
        assert_eq!(keycode, 36, "Return XKB keycode should be 36");
    }

    #[test]
    fn resolve_key_aliases() {
        assert_eq!(resolve_key_name("Enter"), resolve_key_name("Return"));
        assert_eq!(resolve_key_name("Esc"), resolve_key_name("Escape"));
        assert_eq!(resolve_key_name("Del"), resolve_key_name("Delete"));
        assert_eq!(resolve_key_name("PageUp"), resolve_key_name("Page_Up"));
        assert_eq!(resolve_key_name("PageDown"), resolve_key_name("Page_Down"));
        assert_eq!(resolve_key_name("Arrow_Up"), resolve_key_name("Up"));
        assert_eq!(resolve_key_name("Arrow_Down"), resolve_key_name("Down"));
        assert_eq!(resolve_key_name("Arrow_Left"), resolve_key_name("Left"));
        assert_eq!(resolve_key_name("Arrow_Right"), resolve_key_name("Right"));
    }

    #[test]
    fn resolve_case_insensitive() {
        assert_eq!(resolve_key_name("return"), resolve_key_name("RETURN"));
        assert_eq!(resolve_key_name("escape"), resolve_key_name("Escape"));
        assert_eq!(resolve_key_name("tab"), resolve_key_name("TAB"));
    }

    #[test]
    fn resolve_special_keys_have_nonzero_keycode() {
        let keys = [
            "Return",
            "Escape",
            "Tab",
            "Backspace",
            "Delete",
            "Space",
            "Up",
            "Down",
            "Left",
            "Right",
            "Home",
            "End",
            "Page_Up",
            "Page_Down",
            "Insert",
            "F1",
            "F2",
            "F3",
            "F4",
            "F5",
            "F6",
            "F7",
            "F8",
            "F9",
            "F10",
            "F11",
            "F12",
        ];
        for name in keys {
            let (keyval, keycode) = resolve_key_name(name)
                .unwrap_or_else(|| panic!("resolve_key_name({name}) returned None"));
            assert_ne!(keyval, 0, "{name} keyval should not be 0");
            assert_ne!(keycode, 0, "{name} XKB keycode should not be 0");
        }
    }

    #[test]
    fn resolve_single_ascii_char() {
        let (keyval, keycode) = resolve_key_name("a").unwrap();
        assert_eq!(keyval, 'a' as u32);
        assert_eq!(keycode, 0, "single char keys use keycode 0 (text-based)");
    }

    #[test]
    fn resolve_unknown_returns_none() {
        assert!(resolve_key_name("NonExistentKey").is_none());
        assert!(resolve_key_name("").is_none());
    }

    #[test]
    fn reject_ascii_control_chars() {
        assert!(resolve_key_name("\x01").is_none());
        assert!(resolve_key_name("\x0d").is_none()); // raw CR
        assert!(resolve_key_name("\x1b").is_none()); // raw ESC
        assert!(resolve_key_name("\x00").is_none()); // null
    }

    #[test]
    fn xkb_keycodes_match_standard_evdev_plus_8() {
        // Verify a few well-known evdev scancodes + 8 = XKB keycode
        let cases = [
            ("Escape", 1 + 8),     // evdev 1
            ("Backspace", 14 + 8), // evdev 14
            ("Tab", 15 + 8),       // evdev 15
            ("Return", 28 + 8),    // evdev 28
            ("Space", 57 + 8),     // evdev 57
            ("Up", 103 + 8),       // evdev 103
            ("Down", 108 + 8),     // evdev 108
            ("Left", 105 + 8),     // evdev 105
            ("Right", 106 + 8),    // evdev 106
            ("F11", 87 + 8),       // evdev 87 (non-contiguous with F10)
            ("F12", 88 + 8),       // evdev 88
        ];
        for (name, expected_xkb) in cases {
            let (_, keycode) = resolve_key_name(name).unwrap();
            assert_eq!(keycode, expected_xkb, "{name} XKB keycode mismatch");
        }
    }
}
