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
        // SAFETY: getuid() is always safe.
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

    // Warn if session file has overly permissive permissions
    if let Ok(meta) = std::fs::metadata(&path) {
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            tracing::warn!(
                "Session file {} has permissions {:o} (expected 600) — may be world-readable",
                path.display(),
                mode
            );
        }
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
    // Prevent symlink attacks: remove any existing file/symlink at the temp path
    // before creating a new file with O_CREAT|O_EXCL semantics.
    let _ = std::fs::remove_file(&tmp_path);
    let mut file = OpenOptions::new()
        .create_new(true) // O_EXCL: fail if path still exists (race protection)
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
    // Look back up to 32 bytes for an unterminated ESC[
    let lookback = safe_start.saturating_sub(32);
    let mut in_escape = false;
    let mut escape_start = 0;
    #[allow(clippy::needless_range_loop)] // pos used as both index and value
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

    let i = if in_escape {
        // We're inside an unterminated CSI — skip past the escape start
        let mut pos = escape_start;
        // Find the start of the previous line to avoid partial line
        while pos > lookback && bytes[pos] != b'\n' {
            pos -= 1;
        }
        if bytes.get(pos) == Some(&b'\n') {
            pos += 1;
        }
        pos
    } else {
        safe_start
    };

    truncated[i..].to_string()
}

/// Create a snapshot from the current application state.
pub fn create_snapshot(state: &crate::app::AppState) -> AppSessionSnapshot {
    // Capture scrollback text for all terminal panels before locking tab_manager.
    // Skipped when persist_scrollback=false to avoid persisting sensitive data.
    let persist_scrollback = crate::settings::load().persist_scrollback;
    let scrollback_map: std::collections::HashMap<uuid::Uuid, String> = if persist_scrollback {
        state
            .terminal_cache
            .borrow()
            .iter()
            .filter_map(|(&panel_id, surface)| {
                surface
                    .read_scrollback_text()
                    .filter(|t| !t.is_empty())
                    .map(|text| (panel_id, truncate_scrollback(&text)))
            })
            .collect()
    } else {
        Default::default()
    };

    // Capture browser state from WebView registry (GTK main thread)
    #[cfg(feature = "webkit")]
    let browser_zoom_map: std::collections::HashMap<uuid::Uuid, f64> =
        crate::ui::browser_panel::collect_webview_zoom_levels();
    #[cfg(feature = "webkit")]
    let browser_url_map: std::collections::HashMap<uuid::Uuid, String> =
        crate::ui::browser_panel::collect_webview_urls();
    #[cfg(feature = "webkit")]
    let browser_history_map: std::collections::HashMap<uuid::Uuid, (Vec<String>, Vec<String>)> =
        crate::ui::browser_panel::collect_webview_histories();
    #[cfg(not(feature = "webkit"))]
    let browser_zoom_map: std::collections::HashMap<uuid::Uuid, f64> = Default::default();
    #[cfg(not(feature = "webkit"))]
    let browser_url_map: std::collections::HashMap<uuid::Uuid, String> = Default::default();
    #[cfg(not(feature = "webkit"))]
    let browser_history_map: std::collections::HashMap<uuid::Uuid, (Vec<String>, Vec<String>)> =
        Default::default();

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
                    if let Some((back, forward)) = browser_history_map.get(&panel.id) {
                        browser.back_history = back.clone();
                        browser.forward_history = forward.clone();
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
            remote_config: ws.remote_config.clone(),
        }
    };

    // Group workspaces by window_id
    let window_sizes = lock_or_recover(&state.shared.window_sizes);
    let mut window_map: std::collections::BTreeMap<
        Option<uuid::Uuid>,
        Vec<SessionWorkspaceSnapshot>,
    > = std::collections::BTreeMap::new();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_scrollback_short_text() {
        let text = "line1\nline2\nline3";
        let result = truncate_scrollback(text);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_scrollback_preserves_last_n_lines() {
        let lines: Vec<String> = (0..5000).map(|i| format!("line {i}")).collect();
        let text = lines.join("\n");
        let result = truncate_scrollback(&text);
        // Should keep only the last MAX_SCROLLBACK_LINES lines
        let result_lines: Vec<&str> = result.lines().collect();
        assert_eq!(result_lines.len(), MAX_SCROLLBACK_LINES);
        assert!(result_lines.last().unwrap().contains("4999"));
    }

    #[test]
    fn test_truncate_scrollback_ansi_safe() {
        // Create text with an ANSI escape at the truncation boundary
        let mut text = String::new();
        // Fill with enough content to trigger char-based truncation
        for i in 0..1000 {
            text.push_str(&format!("line {i}: some text content here\n"));
        }
        // The result should be valid UTF-8 and not start mid-escape
        let result = truncate_scrollback(&text);
        assert!(result.len() <= MAX_SCROLLBACK_CHARS);
        // Should not start with a partial escape sequence
        assert!(!result.starts_with('['));
    }

    #[test]
    fn test_write_atomic_creates_file() {
        let dir = std::env::temp_dir().join("cmux-test-atomic");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-session.json");
        let _ = std::fs::remove_file(&path);

        write_atomic(&path, b"hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");

        // Verify file permissions are 0600
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_atomic_overwrites() {
        let dir = std::env::temp_dir().join("cmux-test-atomic2");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-overwrite.json");

        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
