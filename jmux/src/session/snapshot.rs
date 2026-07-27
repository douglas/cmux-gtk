//! Session snapshot types — JSON-compatible with the macOS jmux format.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::panel::{GitBranch, LayoutNode, SplitOrientation};
use crate::model::workspace::{LogEntry, Progress, StatusEntry};

/// Root session snapshot.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionSnapshot {
    pub version: u32,
    pub created_at: f64,
    pub windows: Vec<SessionWindowSnapshot>,
    /// Recently-closed workspaces (global, for the History pane + reopen).
    #[serde(default)]
    pub closed_workspaces: Vec<SessionClosedEntrySnapshot>,
}

/// A persisted recently-closed workspace (History pane).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionClosedEntrySnapshot {
    pub workspace: SessionWorkspaceSnapshot,
    /// Unix seconds when the workspace was closed.
    pub closed_at_unix: u64,
    pub title: String,
}

/// Window snapshot (Linux has one window typically, but supports multiple).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWindowSnapshot {
    pub frame: Option<SessionRectSnapshot>,
    pub tab_manager: SessionTabManagerSnapshot,
    pub sidebar: SessionSidebarSnapshot,
}

/// Tab manager snapshot.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTabManagerSnapshot {
    pub selected_workspace_index: Option<usize>,
    pub workspaces: Vec<SessionWorkspaceSnapshot>,
    /// Sidebar workspace groups (scoped to this window).
    #[serde(default)]
    pub groups: Vec<SessionGroupSnapshot>,
}

/// Workspace group snapshot.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionGroupSnapshot {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
}

/// Workspace snapshot.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWorkspaceSnapshot {
    pub process_title: String,
    pub custom_title: Option<String>,
    pub custom_color: Option<String>,
    pub is_pinned: bool,
    pub current_directory: String,
    pub focused_panel_id: Option<Uuid>,
    /// Group this workspace belongs to (None = ungrouped).
    #[serde(default)]
    pub group_id: Option<Uuid>,
    pub layout: SessionWorkspaceLayoutSnapshot,
    pub panels: Vec<SessionPanelSnapshot>,
    pub status_entries: Vec<StatusEntry>,
    pub log_entries: Vec<LogEntry>,
    pub progress: Option<Progress>,
    pub git_branch: Option<GitBranch>,
    /// Remote SSH config for remote workspaces (None for local).
    #[serde(default)]
    pub remote_config: Option<crate::remote::session::RemoteConfig>,
}

/// Recursive layout snapshot (matches macOS JSON format).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionWorkspaceLayoutSnapshot {
    #[serde(rename = "pane")]
    Pane { pane: SessionPaneLayoutSnapshot },
    #[serde(rename = "split")]
    Split { split: SessionSplitLayoutSnapshot },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPaneLayoutSnapshot {
    pub panel_ids: Vec<Uuid>,
    pub selected_panel_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSplitLayoutSnapshot {
    pub orientation: SplitOrientation,
    pub divider_position: f64,
    pub first: Box<SessionWorkspaceLayoutSnapshot>,
    pub second: Box<SessionWorkspaceLayoutSnapshot>,
}

/// Panel snapshot.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPanelSnapshot {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub panel_type: String,
    pub title: Option<String>,
    pub custom_title: Option<String>,
    pub directory: Option<String>,
    pub is_pinned: bool,
    pub is_manually_unread: bool,
    pub git_branch: Option<GitBranch>,
    pub listening_ports: Vec<u16>,
    pub tty_name: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    /// Resume command to use when restoring an agent session (e.g. "claude --continue").
    /// Populated at save time when an agent process is detected running in this panel.
    #[serde(default)]
    pub agent_resume_command: Option<String>,
    /// Exact agent session id captured from the live process at save time (Claude
    /// Code only, local panels). When set, restore resumes this precise
    /// conversation (`claude --resume <id>`) instead of the directory-level
    /// `--continue`, so each tab reopens its own session. Resolved in
    /// `store::create_snapshot`; `from_panel` leaves it `None`.
    #[serde(default)]
    pub agent_session_id: Option<String>,
    pub terminal: Option<SessionTerminalPanelSnapshot>,
    pub browser: Option<SessionBrowserPanelSnapshot>,
    pub markdown: Option<SessionMarkdownPanelSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTerminalPanelSnapshot {
    pub working_directory: Option<String>,
    pub scrollback: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBrowserPanelSnapshot {
    pub url_string: Option<String>,
    pub should_render_web_view: bool,
    pub page_zoom: f64,
    pub developer_tools_visible: bool,
    /// Back-navigation history URLs (most recent first).
    #[serde(default)]
    pub back_history: Vec<String>,
    /// Forward-navigation history URLs (most recent first).
    #[serde(default)]
    pub forward_history: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMarkdownPanelSnapshot {
    pub file_path: String,
}

/// Window geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRectSnapshot {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Sidebar state.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSidebarSnapshot {
    pub is_visible: bool,
    pub selection: String,
    pub width: Option<f64>,
}

// -----------------------------------------------------------------------
// Conversion helpers
// -----------------------------------------------------------------------

impl SessionWorkspaceLayoutSnapshot {
    /// Convert from a model LayoutNode.
    pub fn from_layout(node: &LayoutNode) -> Self {
        match node {
            LayoutNode::Pane {
                panel_ids,
                selected_panel_id,
            } => SessionWorkspaceLayoutSnapshot::Pane {
                pane: SessionPaneLayoutSnapshot {
                    panel_ids: panel_ids.clone(),
                    selected_panel_id: *selected_panel_id,
                },
            },
            LayoutNode::Split {
                orientation,
                divider_position,
                first,
                second,
            } => SessionWorkspaceLayoutSnapshot::Split {
                split: SessionSplitLayoutSnapshot {
                    orientation: *orientation,
                    divider_position: *divider_position,
                    first: Box::new(Self::from_layout(first)),
                    second: Box::new(Self::from_layout(second)),
                },
            },
        }
    }

    /// Convert to a model LayoutNode.
    pub fn to_layout(&self) -> LayoutNode {
        match self {
            SessionWorkspaceLayoutSnapshot::Pane { pane: p } => LayoutNode::Pane {
                panel_ids: p.panel_ids.clone(),
                selected_panel_id: p.selected_panel_id,
            },
            SessionWorkspaceLayoutSnapshot::Split { split: s } => LayoutNode::Split {
                orientation: s.orientation,
                divider_position: if s.divider_position.is_finite() {
                    s.divider_position.clamp(0.0, 1.0)
                } else {
                    0.5
                },
                first: Box::new(s.first.to_layout()),
                second: Box::new(s.second.to_layout()),
            },
        }
    }
}

/// Detect whether a terminal panel is running a known AI agent CLI, and return
/// the appropriate resume command if so.
///
/// Detection checks both the process title (updated by SET_TITLE actions from the
/// terminal) and the panel's stored command (used when the panel was launched with
/// an explicit command).  The title takes precedence since it reflects the live
/// process name.
///
/// Recognised agents and their resume commands:
/// - Claude Code:     `claude --continue`  (auto-resumes the most recent
///   conversation in the panel's directory — no interactive picker, so a
///   restored tab reopens straight into its session after a crash/restart)
/// - Codex CLI:       `codex`
/// - OpenCode:        `opencode --resume`
/// - Gemini CLI:      `gemini`  (stateless; no explicit resume flag)
/// - Rovo Dev:        `rovo dev`
/// - Cursor:          `cursor`  (stateless IDE, just relaunch)
/// - Grok Build CLI:  `grok`
/// - Amp:             `amp`     (matched as whole word to avoid false positives)
/// - Pi Vault:        `pi`      (matched when combined with "vault" or "ai")
/// - Hermes:          `hermes`
/// - Antigravity:     `antigravity`
pub fn detect_agent_resume_command(
    title: Option<&str>,
    command: Option<&str>,
) -> Option<String> {
    // Build a single string to search: "<title> <command>" (lowercase).
    let haystack = {
        let mut s = String::new();
        if let Some(t) = title {
            s.push_str(&t.to_lowercase());
        }
        if let Some(c) = command {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&c.to_lowercase());
        }
        s
    };

    if haystack.is_empty() {
        return None;
    }

    // Match against known agent binary/process names.  The patterns are kept
    // intentionally simple — substring match is enough for process titles.
    if haystack.contains("claude") {
        // `--continue` resumes the most recent conversation in the current
        // directory automatically; `--resume` (no id) would instead drop the
        // restored tab at an interactive picker, which reads as "did not
        // resume". The Vault pane still uses `--resume <id>` for an explicit
        // past session the user picks by hand.
        Some("claude --continue".to_string())
    } else if haystack.contains("opencode") {
        Some("opencode --resume".to_string())
    } else if haystack.contains("codex") {
        Some("codex".to_string())
    } else if haystack.contains("gemini") {
        Some("gemini".to_string())
    } else if haystack.contains("rovo") {
        Some("rovo dev".to_string())
    } else if haystack.contains("cursor") {
        Some("cursor".to_string())
    } else if haystack.contains("grok") {
        Some("grok".to_string())
    } else if haystack.contains("antigravity") {
        Some("antigravity".to_string())
    } else if haystack.contains("hermes") {
        Some("hermes".to_string())
    } else if haystack.contains("pi vault")
        || haystack.contains("pi ai")
        || title.map(|t| t.to_lowercase() == "pi").unwrap_or(false)
        || command.map(|c| c.to_lowercase() == "pi").unwrap_or(false)
    {
        Some("pi".to_string())
    } else if is_word_match(&haystack, "amp") {
        Some("amp".to_string())
    } else {
        None
    }
}

/// Check whether `word` appears as a whole word in `haystack` (lowercase).
/// A word boundary is the start/end of string or a non-alphanumeric character.
fn is_word_match(haystack: &str, word: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(word) {
        let abs = start + pos;
        let before_ok = abs == 0
            || !haystack
                .as_bytes()
                .get(abs - 1)
                .copied()
                .map(|b| b.is_ascii_alphanumeric() || b == b'_')
                .unwrap_or(false);
        let after_ok = abs + word.len() >= haystack.len()
            || !haystack
                .as_bytes()
                .get(abs + word.len())
                .copied()
                .map(|b| b.is_ascii_alphanumeric() || b == b'_')
                .unwrap_or(false);
        if before_ok && after_ok {
            return true;
        }
        start = abs + 1;
        if start >= haystack.len() {
            break;
        }
    }
    false
}

impl SessionPanelSnapshot {
    /// Convert from a model Panel.
    pub fn from_panel(panel: &crate::model::panel::Panel) -> Self {
        let panel_type = match panel.panel_type {
            crate::model::PanelType::Terminal => "terminal".to_string(),
            crate::model::PanelType::Browser => "browser".to_string(),
            crate::model::PanelType::Markdown => "markdown".to_string(),
            crate::model::PanelType::Diff => "diff".to_string(),
            crate::model::PanelType::Project => "project".to_string(),
            crate::model::PanelType::FilePreview => "file_preview".to_string(),
            crate::model::PanelType::Notes => "notes".to_string(),
            crate::model::PanelType::History => "history".to_string(),
            crate::model::PanelType::Vault => "vault".to_string(),
            // Serialized for completeness, but monitor panes are stripped from
            // snapshots in create_snapshot — they never survive a restore.
            crate::model::PanelType::AgentMonitor => "agent_monitor".to_string(),
            crate::model::PanelType::Graph => "graph".to_string(),
        };

        // Detect agent resume command from live process title and stored command.
        let agent_resume_command = if panel.panel_type == crate::model::PanelType::Terminal {
            detect_agent_resume_command(panel.title.as_deref(), panel.command.as_deref())
        } else {
            None
        };

        Self {
            id: panel.id,
            panel_type,
            title: panel.title.clone(),
            custom_title: panel.custom_title.clone(),
            directory: panel.directory.clone(),
            is_pinned: panel.is_pinned,
            is_manually_unread: panel.is_manually_unread,
            git_branch: panel.git_branch.clone(),
            listening_ports: panel.listening_ports.clone(),
            tty_name: panel.tty_name.clone(),
            command: panel.command.clone(),
            agent_resume_command,
            // The shell wrapper's reported id (local or remote); create_snapshot()
            // falls back to /proc resolution for locally-launched Claude that
            // wasn't wrapped.
            agent_session_id: panel.agent_session_id.clone(),
            terminal: if panel.panel_type == crate::model::PanelType::Terminal {
                Some(SessionTerminalPanelSnapshot {
                    working_directory: panel.directory.clone(),
                    scrollback: None, // Filled in by create_snapshot() after surface read
                })
            } else {
                None
            },
            browser: if panel.panel_type == crate::model::PanelType::Browser {
                Some(SessionBrowserPanelSnapshot {
                    url_string: panel.browser_url.clone(),
                    should_render_web_view: true,
                    page_zoom: 1.0,
                    developer_tools_visible: false,
                    back_history: Vec::new(),
                    forward_history: Vec::new(),
                })
            } else {
                None
            },
            markdown: if panel.panel_type == crate::model::PanelType::Markdown {
                panel
                    .markdown_file
                    .as_ref()
                    .map(|f| SessionMarkdownPanelSnapshot {
                        file_path: f.clone(),
                    })
            } else {
                None
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_agent_resume_claude_by_title() {
        assert_eq!(
            detect_agent_resume_command(Some("claude"), None),
            Some("claude --continue".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_claude_code_by_command() {
        assert_eq!(
            detect_agent_resume_command(None, Some("claude")),
            Some("claude --continue".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_opencode() {
        assert_eq!(
            detect_agent_resume_command(Some("opencode"), None),
            Some("opencode --resume".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_codex() {
        assert_eq!(
            detect_agent_resume_command(Some("codex"), None),
            Some("codex".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_gemini() {
        assert_eq!(
            detect_agent_resume_command(Some("gemini"), None),
            Some("gemini".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_rovo() {
        assert_eq!(
            detect_agent_resume_command(Some("rovo dev"), None),
            Some("rovo dev".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_cursor() {
        assert_eq!(
            detect_agent_resume_command(Some("cursor"), None),
            Some("cursor".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_grok() {
        assert_eq!(
            detect_agent_resume_command(Some("grok"), None),
            Some("grok".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_amp_word_boundary() {
        assert_eq!(
            detect_agent_resume_command(Some("amp"), None),
            Some("amp".to_string())
        );
        // "amp" inside a word should NOT match
        assert_eq!(detect_agent_resume_command(Some("example"), None), None);
        assert_eq!(detect_agent_resume_command(Some("ampersand"), None), None);
    }

    #[test]
    fn test_detect_agent_resume_pi_vault() {
        assert_eq!(
            detect_agent_resume_command(Some("pi vault"), None),
            Some("pi".to_string())
        );
        assert_eq!(
            detect_agent_resume_command(None, Some("pi")),
            Some("pi".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_hermes() {
        assert_eq!(
            detect_agent_resume_command(Some("hermes"), None),
            Some("hermes".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_antigravity() {
        assert_eq!(
            detect_agent_resume_command(Some("antigravity"), None),
            Some("antigravity".to_string())
        );
    }

    #[test]
    fn test_detect_agent_resume_none_for_plain_shell() {
        assert_eq!(detect_agent_resume_command(Some("zsh"), None), None);
        assert_eq!(detect_agent_resume_command(Some("bash"), None), None);
        assert_eq!(
            detect_agent_resume_command(Some("vim"), Some("vim")),
            None
        );
    }

    #[test]
    fn test_detect_agent_resume_none_for_empty() {
        assert_eq!(detect_agent_resume_command(None, None), None);
        assert_eq!(detect_agent_resume_command(Some(""), Some("")), None);
    }

    #[test]
    fn test_detect_agent_resume_case_insensitive() {
        assert_eq!(
            detect_agent_resume_command(Some("Claude Code"), None),
            Some("claude --continue".to_string())
        );
        assert_eq!(
            detect_agent_resume_command(Some("CODEX"), None),
            Some("codex".to_string())
        );
    }

    #[test]
    fn test_agent_resume_command_serde_default() {
        // Snapshots without agentResumeCommand (old sessions) should deserialize fine
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "type": "terminal",
            "title": null,
            "customTitle": null,
            "directory": null,
            "isPinned": false,
            "isManuallyUnread": false,
            "gitBranch": null,
            "listeningPorts": [],
            "ttyName": null
        }"#;
        let snap: SessionPanelSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snap.agent_resume_command, None);
        assert_eq!(snap.command, None);
    }
}
