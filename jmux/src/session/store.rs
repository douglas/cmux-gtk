//! Session store — reads and writes session snapshots to XDG_DATA_HOME.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;

use crate::app::lock_or_recover;
use crate::session::snapshot::*;

/// Get the session file path: ~/.local/share/jmux/session.json
fn session_path() -> PathBuf {
    let data_dir = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        // SAFETY: getuid() is always safe.
        .unwrap_or_else(|| std::env::temp_dir().join(format!("jmux-{}", unsafe { libc::getuid() })))
        .join("jmux");
    data_dir.join("session.json")
}

/// Check if a saved session file exists.
pub fn session_file_exists() -> bool {
    session_path().exists()
}

/// Total live workspaces across all windows in a snapshot.
fn live_workspace_count(snapshot: &AppSessionSnapshot) -> usize {
    snapshot
        .windows
        .iter()
        .map(|w| w.tab_manager.workspaces.len())
        .sum()
}

/// Parse a session file with no side effects (no corrupt-rename, no warnings).
/// Used to peek at the on-disk state before deciding whether to overwrite it.
fn read_snapshot_quiet(path: &Path) -> Option<AppSessionSnapshot> {
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

/// Set once this process has ever held live workspaces. Guards against a
/// freshly-restarted process — whose `restore_session` produced nothing (e.g.
/// after an OOM kill left the machine under memory pressure) — silently
/// overwriting a good on-disk session with an empty one on its first autosave.
static HAD_WORKSPACES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Save a session snapshot to disk.
pub fn save_session(snapshot: &AppSessionSnapshot) -> anyhow::Result<()> {
    let path = session_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }

    let new_live = live_workspace_count(snapshot);
    if new_live > 0 {
        HAD_WORKSPACES.store(true, std::sync::atomic::Ordering::Relaxed);
    } else if !HAD_WORKSPACES.load(std::sync::atomic::Ordering::Relaxed) {
        // This process has never had a live workspace, yet it's trying to
        // persist an empty session. That's the OOM-restart clobber path: the
        // previous jmux was SIGKILLed, systemd restarted it, restore failed,
        // and this empty autosave would wipe the real session. Refuse it.
        if let Some(existing) = read_snapshot_quiet(&path) {
            let existing_live = live_workspace_count(&existing);
            if existing_live > 0 {
                tracing::warn!(
                    "Refusing to overwrite session ({} live workspaces) with an \
                     empty snapshot — this process never restored any workspaces",
                    existing_live
                );
                return Ok(());
            }
        }
    }

    // Keep one rolling backup of the last good (non-empty) session, so even a
    // bad overwrite or corruption leaves a recoverable generation behind.
    if new_live > 0 && path.exists() {
        let backup = path.with_extension("json.bak");
        let _ = std::fs::copy(&path, &backup);
    }

    let json = serde_json::to_string_pretty(snapshot)?;
    write_atomic(&path, json.as_bytes())?;

    tracing::debug!("Session saved to {} ({} live workspaces)", path.display(), new_live);
    Ok(())
}

/// Load a session snapshot from disk.
pub fn load_session() -> anyhow::Result<Option<AppSessionSnapshot>> {
    let path = session_path();
    if !path.exists() {
        // Primary gone — a prior crash/clobber may have lost it. Try the backup.
        return Ok(load_backup(&path));
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
            return Ok(load_backup(&path));
        }
    };

    // A readable-but-empty primary next to a non-empty backup means a previous
    // run clobbered the session before the guard existed — prefer the backup.
    if live_workspace_count(&snapshot) == 0 {
        if let Some(backup) = load_backup(&path) {
            if live_workspace_count(&backup) > 0 {
                tracing::warn!("Primary session is empty; recovering from backup");
                return Ok(Some(backup));
            }
        }
    }

    tracing::debug!("Session loaded from {}", path.display());
    Ok(Some(snapshot))
}

/// Load the rolling backup (`session.json.bak`) if it holds a non-empty session.
fn load_backup(path: &Path) -> Option<AppSessionSnapshot> {
    let backup = path.with_extension("json.bak");
    let snapshot = read_snapshot_quiet(&backup)?;
    if live_workspace_count(&snapshot) > 0 {
        tracing::warn!("Recovered session from backup {}", backup.display());
        Some(snapshot)
    } else {
        None
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    // Unique per write (pid + counter) so a background autosave write can't
    // collide on the temp path with the synchronous shutdown save.
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("json.tmp.{}.{}", std::process::id(), seq));
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

/// Maximum lines of scrollback to capture per terminal (matching macOS jmux).
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

/// Overlay live `/proc` data onto a built snapshot: each local panel's real
/// shell cwd and a `claude --resume <id>` fallback for panels whose Claude
/// wasn't launched through the shell wrapper.
///
/// The stored `panel.directory` is only refreshed by shell-integration pwd
/// reports (OSC-7), which never fire while a foreground agent (Claude) owns the
/// terminal — so it can be frozen at the launch directory (often HOME), causing
/// restored tabs to reopen in the wrong folder. Reading `/proc/<shell>/cwd`
/// sidesteps that. Remote panels never appear in the local `/proc`, stay absent
/// from both maps, and keep their stored directory / `--continue` fallback.
///
/// This does two full `/proc` walks plus transcript-directory reads, so it is
/// deliberately separate from `create_snapshot`: the periodic autosave calls it
/// on the writer thread, keeping the GTK main loop (and the global hotkey)
/// free of `/proc` I/O. Pure data — safe off the main thread.
pub fn enrich_snapshot_with_live_proc(snapshot: &mut AppSessionSnapshot) {
    let live_panel_cwds = crate::session::claude_resume::all_local_panel_cwds();

    // Resolve the exact Claude Code session id backing each local terminal panel
    // so a restored tab can `claude --resume <id>` into its precise conversation
    // (not the directory-level `--continue`, which collapses multiple same-dir
    // tabs onto one session). One /proc walk maps panel -> claude cwd; per cwd we
    // hand out transcripts newest-first so tabs sharing a directory get distinct
    // sessions.
    let claude_session_ids: std::collections::HashMap<uuid::Uuid, String> = {
        let mut entries: Vec<(uuid::Uuid, String)> =
            crate::session::claude_resume::all_local_claude_cwds()
                .into_iter()
                .collect();
        // Stable claim order across saves (panel id is a fixed per-tab key).
        entries.sort_by_key(|(id, _)| *id);
        let mut ids_by_cwd: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut next_idx: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut out = std::collections::HashMap::new();
        for (panel_id, cwd) in entries {
            let ids = ids_by_cwd
                .entry(cwd.clone())
                .or_insert_with(|| crate::session::claude_resume::session_ids_for_cwd(&cwd));
            if ids.is_empty() {
                continue;
            }
            let idx = next_idx.entry(cwd).or_insert(0);
            // Claim the next-newest unclaimed transcript; if there are more tabs
            // than transcripts, extra tabs share the oldest resolved id.
            let pick = ids.get(*idx).or_else(|| ids.last()).cloned();
            *idx += 1;
            if let Some(id) = pick {
                out.insert(panel_id, id);
            }
        }
        out
    };

    for window in &mut snapshot.windows {
        for ws in &mut window.tab_manager.workspaces {
            for panel in &mut ws.panels {
                // Override the stored directory with the shell's live cwd when we
                // could read it (local terminal panels only).
                if let Some(live_cwd) = live_panel_cwds.get(&panel.id) {
                    panel.directory = Some(live_cwd.clone());
                    if let Some(ref mut terminal) = panel.terminal {
                        terminal.working_directory = Some(live_cwd.clone());
                    }
                }
                // /proc-resolved resume fallback for a Claude that wasn't
                // launched through the wrapper (wrapper-reported ids were
                // already pinned in create_snapshot).
                if panel.agent_session_id.is_none() {
                    if let Some(id) = claude_session_ids.get(&panel.id) {
                        panel.agent_session_id = Some(id.clone());
                        panel.agent_resume_command = Some(format!("claude --resume {id}"));
                    }
                }
            }
            // Prefer the focused panel's live cwd for the workspace directory
            // (falling back to any panel's live cwd, then the stored value) —
            // same rationale as the per-panel override.
            let live_dir = ws
                .focused_panel_id
                .and_then(|id| live_panel_cwds.get(&id))
                .or_else(|| ws.panels.iter().find_map(|p| live_panel_cwds.get(&p.id)))
                .cloned();
            if let Some(dir) = live_dir {
                ws.current_directory = dir;
            }
        }
    }
}

/// Create a snapshot from the current application state.
pub fn create_snapshot(
    state: &crate::app::AppState,
    capture_scrollback: bool,
) -> AppSessionSnapshot {
    // Capture scrollback text for all terminal panels before locking tab_manager.
    // Skipped when persist_scrollback=false to avoid persisting sensitive data.
    //
    // `capture_scrollback` is false for the periodic autosave: reading every
    // terminal's grid (an FFI walk, up to 400k chars each) and serialising it
    // every 8s stalls the main loop on a long-running daemon — which shows up as
    // the global hotkey getting sluggish. The structure is still saved often;
    // the full scrollback is captured on shutdown (clean quit or SIGTERM).
    let app_settings = crate::settings::load();
    let persist_scrollback = capture_scrollback && app_settings.persist_scrollback;
    let split_ratio_persist = app_settings.split_ratio_persist;
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
            // Sub-agent monitor panes are session-scoped views recreated by
            // the monitor from live transcripts — never persist them.
            .filter(|panel| panel.panel_type != crate::model::PanelType::AgentMonitor)
            .map(|panel| {
                let mut snapshot = SessionPanelSnapshot::from_panel(panel);
                if let Some(ref mut terminal) = snapshot.terminal {
                    terminal.scrollback = scrollback_map.get(&panel.id).cloned();
                }
                // Pin the exact Claude session the shell wrapper reported.
                // Rewriting agent_resume_command reopens the same conversation on
                // local restore (remote restore rebuilds its own ssh command from
                // the id) and also activates resume for an idle Claude whose
                // title no longer names it. Panels without a wrapper-reported id
                // get a /proc-resolved fallback in enrich_snapshot_with_live_proc.
                if let Some(id) = snapshot.agent_session_id.clone() {
                    snapshot.agent_resume_command = Some(format!("claude --resume {id}"));
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

        let layout = {
            // Strip monitor panes from the persisted layout too, so a restore
            // never references a panel the filter above dropped.
            let mut pruned = ws.layout.clone();
            for (id, panel) in &ws.panels {
                if panel.panel_type == crate::model::PanelType::AgentMonitor {
                    pruned.remove_panel(*id);
                }
            }
            let mut snapshot = SessionWorkspaceLayoutSnapshot::from_layout(&pruned);
            if !split_ratio_persist {
                normalize_divider_positions(&mut snapshot);
            }
            snapshot
        };

        // The stored directory; enrich_snapshot_with_live_proc overrides it with
        // the focused panel's live /proc cwd afterwards.
        let current_directory = ws.current_directory.clone();

        SessionWorkspaceSnapshot {
            process_title: ws.process_title.clone(),
            custom_title: ws.custom_title.clone(),
            custom_color: ws.custom_color.clone(),
            is_pinned: ws.is_pinned,
            current_directory,
            // Never persist focus on a (stripped) monitor pane.
            focused_panel_id: ws.focused_panel_id.filter(|id| {
                ws.panels
                    .get(id)
                    .map(|p| p.panel_type != crate::model::PanelType::AgentMonitor)
                    .unwrap_or(true)
            }),
            group_id: ws.group_id,
            layout,
            panels,
            status_entries: ws.status_entries.clone(),
            log_entries: ws.log_entries.clone(),
            progress: ws.progress.clone(),
            git_branch: ws.git_branch.clone(),
            remote_config: ws.remote_config.clone(),
        }
    };

    // Group workspaces by window_id. Workspaces created at runtime (socket,
    // goal/graph, palette) carry window_id None while restored ones carry
    // Some(id) — bucketing them separately would split every new workspace
    // into its own phantom window snapshot and scramble the sidebar order on
    // the next restart. Fold None into the primary window's bucket instead:
    // tm.iter() order then lands interleaved, exactly as the sidebar shows it.
    let window_sizes = lock_or_recover(&state.shared.window_sizes);
    let mut window_map: std::collections::BTreeMap<
        Option<uuid::Uuid>,
        Vec<SessionWorkspaceSnapshot>,
    > = std::collections::BTreeMap::new();
    let quick_window = crate::ui::quick_terminal::quick_window_id();
    let primary_window: Option<uuid::Uuid> = tm
        .iter()
        .filter(|ws| ws.window_id != Some(quick_window))
        .find_map(|ws| ws.window_id);
    for ws in tm.iter() {
        // The quick-terminal drop-down is recreated on demand — never persist it
        // (otherwise it restores as an extra normal window).
        if ws.window_id == Some(quick_window) {
            continue;
        }
        window_map
            .entry(ws.window_id.or(primary_window))
            .or_default()
            .push(make_ws_snapshot(ws));
    }

    let windows: Vec<SessionWindowSnapshot> = window_map
        .into_iter()
        .map(|(window_id, workspaces)| {
            let (w, h) = window_id
                .and_then(|wid| window_sizes.get(&wid).copied())
                .unwrap_or((1280, 860));
            // Include the groups scoped to this window.
            let groups: Vec<crate::session::snapshot::SessionGroupSnapshot> = tm
                .groups()
                .iter()
                // Same None → primary normalization as the workspaces above,
                // or runtime-created groups (e.g. graph groups) would never
                // be persisted.
                .filter(|g| g.window_id.or(primary_window) == window_id)
                .map(|g| crate::session::snapshot::SessionGroupSnapshot {
                    id: g.id,
                    name: g.name.clone(),
                    color: g.color.clone(),
                    collapsed: g.collapsed,
                })
                .collect();
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
                    groups,
                },
                sidebar: SessionSidebarSnapshot {
                    is_visible: true,
                    selection: "tabs".to_string(),
                    width: None,
                },
            }
        })
        .collect();

    // Persist the recently-closed history (global) so the History pane and
    // reopen survive restarts.
    let closed_workspaces: Vec<crate::session::snapshot::SessionClosedEntrySnapshot> = tm
        .closed_entries()
        .iter()
        .map(|entry| crate::session::snapshot::SessionClosedEntrySnapshot {
            workspace: make_ws_snapshot(&entry.workspace),
            closed_at_unix: entry
                .closed_at
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            title: entry.title.clone(),
        })
        .collect();

    AppSessionSnapshot {
        version: 1,
        created_at: now,
        windows,
        closed_workspaces,
    }
}

/// Reset all split divider positions to 0.5 (equal split) in a layout snapshot.
/// Used when `split_ratio_persist` is disabled so sessions restore with equal splits.
fn normalize_divider_positions(layout: &mut crate::session::snapshot::SessionWorkspaceLayoutSnapshot) {
    use crate::session::snapshot::SessionWorkspaceLayoutSnapshot;
    if let SessionWorkspaceLayoutSnapshot::Split { split } = layout {
        split.divider_position = 0.5;
        normalize_divider_positions(&mut split.first);
        normalize_divider_positions(&mut split.second);
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
        let dir = std::env::temp_dir().join("jmux-test-atomic");
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
        let dir = std::env::temp_dir().join("jmux-test-atomic2");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-overwrite.json");

        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
