//! Session store — reads and writes session snapshots to XDG_DATA_HOME.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;

use crate::app::lock_or_recover;
use crate::session::snapshot::*;

/// Get the session file path: ~/.local/share/cmux/session.json
fn session_path() -> PathBuf {
    let data_dir = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(|| std::env::temp_dir().join(format!("cmux-{}", unsafe { libc::getuid() })))
        .join("cmux");
    data_dir.join("session.json")
}

/// Check if a saved session file exists.
pub fn session_file_exists() -> bool {
    session_path().exists()
}

/// Save a session snapshot to disk.
pub fn save_session(snapshot: &AppSessionSnapshot) -> anyhow::Result<()> {
    let path = session_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }

    let json = serde_json::to_string_pretty(snapshot)?;
    write_atomic(&path, json.as_bytes())?;

    tracing::debug!("Session saved to {}", path.display());
    Ok(())
}

/// Load a session snapshot from disk.
pub fn load_session() -> anyhow::Result<Option<AppSessionSnapshot>> {
    let path = session_path();
    if !path.exists() {
        return Ok(None);
    }

    let json = std::fs::read_to_string(&path)?;
    let snapshot: AppSessionSnapshot = match serde_json::from_str(&json) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::warn!(
                "Corrupt session file at {}, ignoring: {}",
                path.display(),
                error
            );
            let backup = path.with_extension("json.corrupt");
            let _ = std::fs::rename(&path, &backup);
            return Ok(None);
        }
    };

    tracing::debug!("Session loaded from {}", path.display());
    Ok(Some(snapshot))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let tmp_path = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp_path)?;
    file.write_all(bytes)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.sync_all()?;
    std::fs::rename(&tmp_path, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp_path);
    })?;
    Ok(())
}

/// Maximum lines of scrollback to capture per terminal (matching macOS cmux).
const MAX_SCROLLBACK_LINES: usize = 4000;
/// Maximum characters of scrollback to capture (matching macOS: 400,000).
const MAX_SCROLLBACK_CHARS: usize = 400_000;

/// Truncate text to at most `max_lines` lines from the end, then cap at
/// `MAX_SCROLLBACK_CHARS`. ANSI-safe: if truncation would split inside a
/// CSI escape sequence (ESC [ ... final_byte), back up to before the ESC.
fn truncate_scrollback(text: &str) -> String {
    // First: line-based truncation from the end
    let lines: Vec<&str> = text.lines().collect();
    let truncated = if lines.len() > MAX_SCROLLBACK_LINES {
        lines[lines.len() - MAX_SCROLLBACK_LINES..].join("\n")
    } else {
        text.to_string()
    };

    // Second: character-based truncation from the end
    if truncated.len() <= MAX_SCROLLBACK_CHARS {
        return truncated;
    }

    // Take the last MAX_SCROLLBACK_CHARS bytes, then ANSI-safe adjust the start
    let start = truncated.len() - MAX_SCROLLBACK_CHARS;
    // Find a safe UTF-8 boundary
    let mut safe_start = start;
    while safe_start < truncated.len() && !truncated.is_char_boundary(safe_start) {
        safe_start += 1;
    }

    // Check if we're splitting inside an ANSI CSI sequence (ESC [ ... letter).
    // Scan backward from safe_start looking for an ESC that hasn't been terminated.
    let bytes = truncated.as_bytes();
    let mut i = safe_start;
    // Look back up to 32 bytes for an unterminated ESC[
    let lookback = safe_start.saturating_sub(32);
    let mut in_escape = false;
    let mut escape_start = 0;
    for pos in lookback..safe_start {
        if bytes[pos] == 0x1b {
            in_escape = true;
            escape_start = pos;
        } else if in_escape {
            if bytes[pos] == b'[' {
                // CSI sequence — look for terminating byte (0x40-0x7e)
                continue;
            } else if (0x40..=0x7e).contains(&bytes[pos]) {
                // Found terminator — sequence is complete
                in_escape = false;
            }
        }
    }

    if in_escape {
        // We're inside an unterminated CSI — skip past the escape start
        i = escape_start;
        // Find the start of the previous line to avoid partial line
        while i > lookback && bytes[i] != b'\n' {
            i -= 1;
        }
        if bytes.get(i) == Some(&b'\n') {
            i += 1;
        }
    } else {
        i = safe_start;
    }

    truncated[i..].to_string()
}

/// Create a snapshot from the current application state.
pub fn create_snapshot(state: &crate::app::AppState) -> AppSessionSnapshot {
    // Capture scrollback text for all terminal panels before locking tab_manager
    let scrollback_map: std::collections::HashMap<uuid::Uuid, String> = state
        .terminal_cache
        .borrow()
        .iter()
        .filter_map(|(&panel_id, surface)| {
            surface
                .read_scrollback_text()
                .filter(|t| !t.is_empty())
                .map(|text| (panel_id, truncate_scrollback(&text)))
        })
        .collect();

    // Capture browser state from WebView registry (GTK main thread)
    let browser_zoom_map: std::collections::HashMap<uuid::Uuid, f64> =
        crate::ui::browser_panel::collect_webview_zoom_levels();
    let browser_url_map: std::collections::HashMap<uuid::Uuid, String> =
        crate::ui::browser_panel::collect_webview_urls();

    let tm = lock_or_recover(&state.shared.tab_manager);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    // Helper: create a workspace snapshot with scrollback/browser data attached
    let make_ws_snapshot = |ws: &crate::model::workspace::Workspace| -> SessionWorkspaceSnapshot {
        let panels: Vec<SessionPanelSnapshot> = ws
            .panels
            .values()
            .map(|panel| {
                let mut snapshot = SessionPanelSnapshot::from_panel(panel);
                if let Some(ref mut terminal) = snapshot.terminal {
                    terminal.scrollback = scrollback_map.get(&panel.id).cloned();
                }
                if let Some(ref mut browser) = snapshot.browser {
                    if let Some(&zoom) = browser_zoom_map.get(&panel.id) {
                        browser.page_zoom = zoom;
                    }
                    if let Some(url) = browser_url_map.get(&panel.id) {
                        browser.url_string = Some(url.clone());
                    }
                }
                snapshot
            })
            .collect();

        SessionWorkspaceSnapshot {
            process_title: ws.process_title.clone(),
            custom_title: ws.custom_title.clone(),
            custom_color: ws.custom_color.clone(),
            is_pinned: ws.is_pinned,
            current_directory: ws.current_directory.clone(),
            focused_panel_id: ws.focused_panel_id,
            layout: SessionWorkspaceLayoutSnapshot::from_layout(&ws.layout),
            panels,
            status_entries: ws.status_entries.clone(),
            log_entries: ws.log_entries.clone(),
            progress: ws.progress.clone(),
            git_branch: ws.git_branch.clone(),
        }
    };

    // Group workspaces by window_id
    let window_sizes = lock_or_recover(&state.shared.window_sizes);
    let mut window_map: std::collections::BTreeMap<Option<uuid::Uuid>, Vec<SessionWorkspaceSnapshot>> =
        std::collections::BTreeMap::new();
    for ws in tm.iter() {
        window_map
            .entry(ws.window_id)
            .or_default()
            .push(make_ws_snapshot(ws));
    }

    let windows: Vec<SessionWindowSnapshot> = window_map
        .into_iter()
        .map(|(window_id, workspaces)| {
            let (w, h) = window_id
                .and_then(|wid| window_sizes.get(&wid).copied())
                .unwrap_or((1280, 860));
            SessionWindowSnapshot {
                frame: Some(SessionRectSnapshot {
                    x: 0.0,
                    y: 0.0,
                    width: w as f64,
                    height: h as f64,
                }),
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces,
                },
                sidebar: SessionSidebarSnapshot {
                    is_visible: true,
                    selection: "tabs".to_string(),
                    width: None,
                },
            }
        })
        .collect();

    AppSessionSnapshot {
        version: 1,
        created_at: now,
        windows,
    }
}
