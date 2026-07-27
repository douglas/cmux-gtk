//! Sub-agent monitor — mirrors a workspace's live Claude Code subagents as
//! read-only panes.
//!
//! When a workspace has `subagent_monitor` enabled, a periodic sync discovers
//! the subagent transcripts of the workspace's primary Claude session
//! (`~/.claude/projects/<slug>/<session-id>/subagents/agent-*.jsonl`) and
//! keeps one `PanelType::AgentMonitor` pane per recently-active subagent,
//! tiled into the layout with alternating horizontal/vertical splits. The
//! panes are non-interactive (see `ui::agent_monitor_panel`) — steering stays
//! with the primary agent — and are never persisted to session snapshots.

use std::path::PathBuf;
use std::sync::Arc;

use crate::app::{lock_or_recover, SharedState};
use crate::model::panel::{Direction, SplitOrientation};
use crate::model::{Panel, PanelType, Workspace};

/// How often the sync ticker re-scans monitored workspaces.
const SYNC_INTERVAL_MS: u64 = 2000;

/// A subagent whose transcript was written within this window is "live" and
/// gets (or keeps) a pane; older ones have finished and their panes close.
const ACTIVE_WINDOW_SECS: u64 = 10 * 60;

/// Upper bound on monitor panes per workspace — beyond this the newest agents
/// win. Keeps the tiling readable when a workflow fans out dozens of agents.
const MAX_MONITOR_PANES: usize = 6;

/// Toggle the monitor for the currently selected workspace. Wired to the
/// header button, the sidebar menu entry, the socket method, and the
/// `workspace.toggle_agent_monitor` shortcut.
pub fn toggle_selected(shared: &Arc<SharedState>) {
    let toast = {
        let mut tm = lock_or_recover(&shared.tab_manager);
        tm.selected_mut().map(|ws| {
            ws.subagent_monitor = !ws.subagent_monitor;
            sync_workspace(ws);
            toggle_toast(ws)
        })
    };
    // The toggle is silent UI-wise when no sub-agents are live, so always say
    // what happened — an armed-but-empty monitor otherwise looks broken.
    if let Some(msg) = toast {
        shared.send_ui_event(crate::app::UiEvent::ShowToast(msg));
    }
    shared.notify_ui_refresh();
}

/// Human-readable outcome of a toggle, from the workspace's post-sync state.
fn toggle_toast(ws: &Workspace) -> String {
    if !ws.subagent_monitor {
        return "Sub-agent monitor off".into();
    }
    let live = ws
        .panels
        .values()
        .filter(|p| p.panel_type == PanelType::AgentMonitor)
        .count();
    if live == 0 {
        "Sub-agent monitor on — panes appear when Claude runs sub-agents here".into()
    } else {
        format!("Sub-agent monitor on — showing {live} live sub-agent(s)")
    }
}

/// Install the periodic sync on the GTK main loop. Cheap when no workspace has
/// the monitor enabled (one flag check per workspace per tick) and the
/// `auto_subagent_monitor` setting is off.
///
/// With `auto_subagent_monitor` enabled, the monitor's visibility tracks live
/// sub-agents in every workspace: each tick opens the monitor where sub-agents
/// have appeared and closes it where they have finished, so no manual toggle is
/// needed. This costs a subagents directory scan per workspace per tick.
pub fn start_ticker(shared: Arc<SharedState>) {
    glib::timeout_add_local(
        std::time::Duration::from_millis(SYNC_INTERVAL_MS),
        move || {
            let auto = crate::settings::load().auto_subagent_monitor;
            let mut changed = false;
            {
                let mut tm = lock_or_recover(&shared.tab_manager);
                for ws in tm.iter_mut() {
                    if auto {
                        // Auto mode owns the flag: the monitor is on iff the
                        // workspace has live sub-agents right now.
                        let has_live = !discover_subagents(ws).is_empty();
                        if ws.subagent_monitor != has_live {
                            ws.subagent_monitor = has_live;
                            changed = true;
                        }
                    }
                    if ws.subagent_monitor {
                        changed |= sync_workspace(ws);
                    }
                }
            }
            if changed {
                shared.notify_ui_refresh();
            }
            glib::ControlFlow::Continue
        },
    );
}

/// One live subagent discovered on disk.
struct Subagent {
    transcript: PathBuf,
    title: String,
}

/// Reconcile a workspace's monitor panes against the subagents on disk.
/// Returns true when panels were added or removed (a layout refresh is due).
pub fn sync_workspace(ws: &mut Workspace) -> bool {
    let desired: Vec<Subagent> = if ws.subagent_monitor {
        discover_subagents(ws)
    } else {
        Vec::new()
    };

    let mut changed = false;

    // Remove panes whose agent is gone (or all of them when toggled off).
    let stale: Vec<uuid::Uuid> = ws
        .panels
        .iter()
        .filter(|(_, p)| p.panel_type == PanelType::AgentMonitor)
        .filter(|(_, p)| {
            !desired.iter().any(|d| {
                Some(d.transcript.to_string_lossy().as_ref()) == p.markdown_file.as_deref()
            })
        })
        .map(|(id, _)| *id)
        .collect();
    for id in stale {
        ws.layout.remove_panel(id);
        ws.panels.remove(&id);
        if ws.focused_panel_id == Some(id) {
            ws.focused_panel_id = ws.layout.all_panel_ids().into_iter().next();
        }
        if ws.previous_focused_panel_id == Some(id) {
            ws.previous_focused_panel_id = None;
        }
        if ws.attention_panel_id == Some(id) {
            ws.attention_panel_id = None;
        }
        if ws.zoomed_panel_id == Some(id) {
            ws.zoomed_panel_id = None;
        }
        changed = true;
    }

    // Add panes for new agents, tiling with alternating split orientations:
    // the first monitor splits the primary pane to the right; each subsequent
    // one splits the previous monitor pane, alternating down/right, which
    // yields the mixed vertical+horizontal grid the monitor is meant to show.
    // `desired` is oldest-first and panes were added in that order on earlier
    // syncs, so walking it while tracking the previous pane keeps the anchor
    // deterministic (ws.panels is a HashMap — its iteration order is not).
    let mut prev_monitor: Option<uuid::Uuid> = None;
    let mut monitor_count = ws
        .panels
        .values()
        .filter(|p| p.panel_type == PanelType::AgentMonitor)
        .count();
    for agent in &desired {
        let path = agent.transcript.to_string_lossy();
        if let Some(existing) = ws.panels.values().find(|p| {
            p.panel_type == PanelType::AgentMonitor
                && p.markdown_file.as_deref() == Some(path.as_ref())
        }) {
            prev_monitor = Some(existing.id);
            continue;
        }

        // Anchor: the previous monitor pane in `desired` order, else the
        // primary (focused non-monitor, else any non-monitor) pane.
        let anchor = prev_monitor
            .or_else(|| {
                ws.focused_panel_id.filter(|id| {
                    ws.panels
                        .get(id)
                        .map(|p| p.panel_type != PanelType::AgentMonitor)
                        .unwrap_or(false)
                })
            })
            .or_else(|| {
                ws.layout.all_panel_ids().into_iter().find(|id| {
                    ws.panels
                        .get(id)
                        .map(|p| p.panel_type != PanelType::AgentMonitor)
                        .unwrap_or(false)
                })
            });
        let Some(anchor) = anchor else { break };

        let (orientation, direction) = if monitor_count % 2 == 0 {
            (SplitOrientation::Horizontal, Direction::Right)
        } else {
            (SplitOrientation::Vertical, Direction::Down)
        };

        let panel = Panel::new_agent_monitor(&path, &agent.title);
        let new_id = panel.id;
        ws.panels.insert(new_id, panel);
        if !ws
            .layout
            .split_pane_with_panel(anchor, new_id, orientation, direction)
        {
            // Anchor vanished between discovery and split — drop the panel.
            ws.panels.remove(&new_id);
            continue;
        }
        prev_monitor = Some(new_id);
        monitor_count += 1;
        changed = true;
    }

    changed
}

/// List this workspace's recently-active subagents, oldest first (stable pane
/// order), capped at `MAX_MONITOR_PANES` newest.
fn discover_subagents(ws: &Workspace) -> Vec<Subagent> {
    let Some(dir) = subagents_dir(ws) else {
        return Vec::new();
    };
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let now = std::time::SystemTime::now();

    let mut agents: Vec<(std::time::SystemTime, Subagent)> = read_dir
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if !name.starts_with("agent-") || !name.ends_with(".jsonl") {
                return None;
            }
            let mtime = entry.metadata().ok()?.modified().ok()?;
            if now.duration_since(mtime).map(|d| d.as_secs()).unwrap_or(0) > ACTIVE_WINDOW_SECS {
                return None;
            }
            let title = meta_description(&path)
                .unwrap_or_else(|| name.trim_end_matches(".jsonl").to_string());
            Some((
                mtime,
                Subagent {
                    transcript: path,
                    title,
                },
            ))
        })
        .collect();

    // Newest first for the cap, then back to oldest-first for stable tiling.
    agents.sort_by(|a, b| b.0.cmp(&a.0));
    agents.truncate(MAX_MONITOR_PANES);
    agents.reverse();
    agents.into_iter().map(|(_, a)| a).collect()
}

/// The subagents directory of the workspace's primary Claude session:
/// `~/.claude/projects/<encoded-cwd>/<session-id>/subagents`.
///
/// The session id comes from the shell wrapper's report (`agent_session_id`
/// on a terminal panel — the same field session restore uses). Panels without
/// one (Claude launched outside the wrapper) fall back to the newest session
/// transcript for the panel's directory.
fn subagents_dir(ws: &Workspace) -> Option<PathBuf> {
    let root = crate::session::claude_resume::claude_projects_dir()?;

    // Prefer the focused panel, then any terminal panel with a session id.
    let focused = ws.focused_panel_id.and_then(|id| ws.panels.get(&id));
    let mut candidates: Vec<&Panel> = focused.into_iter().collect();
    let mut rest: Vec<&Panel> = ws
        .panels
        .values()
        .filter(|p| p.panel_type == PanelType::Terminal)
        .collect();
    rest.sort_by_key(|p| p.id);
    candidates.extend(rest);

    for panel in &candidates {
        if panel.panel_type != PanelType::Terminal {
            continue;
        }
        let Some(session_id) = &panel.agent_session_id else {
            continue;
        };
        let dir = panel
            .directory
            .clone()
            .unwrap_or_else(|| ws.current_directory.clone());
        let path = root
            .join(crate::session::claude_resume::encode_project_dir(&dir))
            .join(session_id)
            .join("subagents");
        if path.is_dir() {
            return Some(path);
        }
    }

    // Fallback: newest session for each terminal panel's directory.
    for panel in &candidates {
        if panel.panel_type != PanelType::Terminal {
            continue;
        }
        let dir = panel
            .directory
            .clone()
            .unwrap_or_else(|| ws.current_directory.clone());
        for session_id in crate::session::claude_resume::session_ids_for_cwd(&dir) {
            let path = root
                .join(crate::session::claude_resume::encode_project_dir(&dir))
                .join(&session_id)
                .join("subagents");
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

/// Read the one-line description from an agent's `.meta.json` sidecar.
fn meta_description(transcript: &std::path::Path) -> Option<String> {
    let meta_path = transcript.with_extension("").with_extension("meta.json");
    let content = std::fs::read_to_string(meta_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("description")
        .and_then(|d| d.as_str())
        .map(str::to_string)
}
