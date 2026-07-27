//! Workspace model — a named collection of panels with layout and metadata.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::panel::{GitBranch, LayoutNode, Panel, PanelType, SplitOrientation};

/// A workspace contains one or more panels arranged in a split layout.
///
/// Each workspace appears as a tab in the sidebar.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: Uuid,
    pub process_title: String,
    pub custom_title: Option<String>,
    pub custom_color: Option<String>,
    pub is_pinned: bool,
    pub current_directory: String,
    pub focused_panel_id: Option<Uuid>,

    /// Previously focused panel ID (for pane.last).
    pub previous_focused_panel_id: Option<Uuid>,

    /// Zoomed panel ID (when set, only this panel renders full-size).
    pub zoomed_panel_id: Option<Uuid>,

    /// The layout tree describing pane arrangement.
    pub layout: LayoutNode,

    /// All panels in this workspace, keyed by UUID.
    pub panels: HashMap<Uuid, Panel>,

    /// Status entries (agent metadata, key-value pairs).
    pub status_entries: Vec<StatusEntry>,

    /// Rich metadata entries (key-value with priority, URL, format).
    pub metadata_entries: Vec<MetadataEntry>,

    /// Freeform metadata blocks (markdown content).
    pub metadata_blocks: Vec<MetadataBlock>,

    /// Log entries from agents/tools.
    pub log_entries: Vec<LogEntry>,

    /// Progress indicator.
    pub progress: Option<Progress>,

    /// Git branch for the workspace root.
    pub git_branch: Option<GitBranch>,

    /// Unread notification count.
    pub unread_count: u32,
    /// Sidebar summary for the latest notification in this workspace.
    pub latest_notification: Option<String>,
    /// Timestamp of the latest notification, used for latest-unread routing.
    pub latest_notification_at: Option<f64>,
    /// Panel that most recently requested attention, if known.
    pub attention_panel_id: Option<Uuid>,
    /// PR status (open, merged, closed, draft).
    pub pr_status: Option<String>,
    /// PR URL for the workspace.
    pub pr_url: Option<String>,
    /// Individual PR check results (name → conclusion).
    pub pr_checks: Vec<PrCheck>,
    /// Window this workspace belongs to (None = default/first window).
    pub window_id: Option<Uuid>,
    /// Sidebar group this workspace belongs to (None = ungrouped).
    pub group_id: Option<Uuid>,
    /// Remote SSH configuration (None for local workspaces).
    pub remote_config: Option<crate::remote::session::RemoteConfig>,
    /// Remote connection state (None for local workspaces).
    pub remote_state: Option<crate::remote::session::RemoteState>,
    /// When true, render log entries and metadata blocks as chat bubbles
    /// (iMessage-style conversation layout) in the sidebar detail area.
    pub imessage_mode: bool,
    /// Recently-closed panels (most recent last) for "reopen closed tab".
    /// Session-scoped (not persisted).
    pub closed_panels: Vec<Panel>,
    /// Per-workspace environment variables injected into every shell spawned in
    /// this workspace (from the jmux.json `workspace.env` map). Session-scoped.
    pub env: Vec<(String, String)>,
    /// Optional free-text description shown in the sidebar tooltip. Session-scoped.
    pub description: Option<String>,
    /// When true, the subagent monitor keeps read-only panes tiled into this
    /// workspace, one per live Claude Code subagent. Session-scoped.
    pub subagent_monitor: bool,
}

/// Individual PR check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrCheck {
    pub name: String,
    /// SUCCESS, FAILURE, PENDING, NEUTRAL, SKIPPED, etc.
    pub conclusion: String,
}

/// Status entry (agent metadata key-value pairs shown in sidebar).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEntry {
    pub key: String,
    pub value: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub url: Option<String>,
    pub timestamp: f64,
}

/// Rich metadata entry with priority ordering and format options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataEntry {
    pub key: String,
    pub value: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub url: Option<String>,
    pub priority: i32,
    pub format: MetadataFormat,
    pub timestamp: f64,
}

/// Freeform metadata block (markdown content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataBlock {
    pub key: String,
    pub content: String,
    pub priority: i32,
    pub timestamp: f64,
}

/// Format for metadata entry values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetadataFormat {
    Plain,
    Markdown,
}

/// Log entry from agents/tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub message: String,
    pub level: String,
    pub source: Option<String>,
    pub timestamp: f64,
}

/// Progress indicator for a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub value: f64,
    pub label: Option<String>,
}

/// Truncate a string to at most `max_bytes` bytes without splitting UTF-8.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Maximum recently-closed panels retained per workspace for reopen.
const CLOSED_PANELS_CAP: usize = 10;

impl Workspace {
    /// Create a new workspace with a single terminal panel.
    pub fn new() -> Self {
        let current_directory = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let panel = Panel::new_terminal();
        let panel_id = panel.id;
        let mut panels = HashMap::new();
        panels.insert(panel_id, panel);

        Self {
            id: Uuid::new_v4(),
            process_title: "Terminal".to_string(),
            custom_title: None,
            custom_color: None,
            is_pinned: false,
            current_directory,
            focused_panel_id: Some(panel_id),
            previous_focused_panel_id: None,
            zoomed_panel_id: None,
            layout: LayoutNode::single_pane(panel_id),
            panels,
            status_entries: Vec::new(),
            metadata_entries: Vec::new(),
            metadata_blocks: Vec::new(),
            log_entries: Vec::new(),
            progress: None,
            git_branch: None,
            unread_count: 0,
            latest_notification: None,
            latest_notification_at: None,
            attention_panel_id: None,
            pr_status: None,
            pr_url: None,
            pr_checks: Vec::new(),
            window_id: None,
            group_id: None,
            remote_config: None,
            remote_state: None,
            imessage_mode: false,
            closed_panels: Vec::new(),
            env: Vec::new(),
            description: None,
            subagent_monitor: false,
        }
    }

    /// Create a new workspace with a specific working directory.
    pub fn with_directory(directory: &str) -> Self {
        let mut ws = Self::new();
        ws.current_directory = directory.to_string();
        if let Some(panel_id) = ws.focused_panel_id {
            if let Some(panel) = ws.panels.get_mut(&panel_id) {
                panel.directory = Some(directory.to_string());
            }
        }
        ws
    }

    /// Display title: custom title if set, otherwise process title.
    pub fn display_title(&self) -> &str {
        self.custom_title.as_deref().unwrap_or(&self.process_title)
    }

    /// Working directory a newly-opened terminal should inherit: the focused
    /// panel's last-reported cwd, falling back to the workspace directory.
    /// Returns `None` only if neither is known (terminal starts in $HOME).
    pub fn inherited_terminal_directory(&self) -> Option<String> {
        self.focused_panel_id
            .and_then(|fid| self.panels.get(&fid))
            .and_then(|p| p.directory.clone())
            .filter(|d| !d.is_empty())
            .or_else(|| {
                Some(self.current_directory.clone()).filter(|d| !d.is_empty())
            })
    }

    /// Add a new panel by splitting the focused pane.
    pub fn split(&mut self, orientation: SplitOrientation, panel_type: PanelType) -> Uuid {
        let new_panel = match panel_type {
            PanelType::Terminal => Panel::new_terminal(),
            PanelType::Browser => Panel::new_browser(),
            PanelType::Markdown => Panel::new_markdown(""),
            PanelType::Diff => Panel::new_diff(None),
            PanelType::Project => Panel::new_project(None),
            PanelType::FilePreview => Panel::new_file_preview(""),
            PanelType::Notes => Panel::new_notes(""),
            PanelType::History => Panel::new_history(),
            PanelType::Vault => Panel::new_vault(),
            // Monitor panes are created only by the subagent monitor with a
            // real transcript path; a generic split falls back to a terminal.
            PanelType::AgentMonitor => Panel::new_terminal(),
            // Graph panels need a graph name; a generic split falls back too.
            PanelType::Graph => Panel::new_terminal(),
        };
        let new_id = new_panel.id;
        self.panels.insert(new_id, new_panel);

        // Find the focused pane and split it
        let mut split_done = false;
        if let Some(focused_id) = self.focused_panel_id {
            if let Some(pane) = self.layout.find_pane_with_panel(focused_id) {
                let old = std::mem::replace(
                    pane,
                    LayoutNode::Pane {
                        panel_ids: vec![],
                        selected_panel_id: None,
                    },
                );
                *pane = old.split(orientation, new_id);
                split_done = true;
            }
        }

        if !split_done {
            // No focused panel — just split the root
            let old = std::mem::replace(
                &mut self.layout,
                LayoutNode::Pane {
                    panel_ids: vec![],
                    selected_panel_id: None,
                },
            );
            self.layout = old.split(orientation, new_id);
        }

        self.previous_focused_panel_id = self.focused_panel_id;
        self.focused_panel_id = Some(new_id);
        new_id
    }

    /// Remove a panel by ID. Returns true if the panel existed.
    pub fn remove_panel(&mut self, panel_id: Uuid) -> bool {
        let Some(panel) = self.panels.remove(&panel_id) else {
            return false;
        };
        // Record for "reopen closed tab" (most recent last, capped).
        self.closed_panels.push(panel);
        if self.closed_panels.len() > CLOSED_PANELS_CAP {
            let drop = self.closed_panels.len() - CLOSED_PANELS_CAP;
            self.closed_panels.drain(0..drop);
        }

        // If we're closing the focused tab, shift focus to the tab on its left in
        // the same pane (or the right neighbour if it was leftmost) — captured
        // before the layout mutates. This keeps focus on an adjacent tab instead
        // of jumping to the first pane.
        let removing_focused = self.focused_panel_id == Some(panel_id);
        let neighbor = if removing_focused {
            self.layout
                .find_pane_with_panel_readonly(panel_id)
                .and_then(|pane| {
                    let idx = pane.iter().position(|&id| id == panel_id)?;
                    if idx > 0 {
                        Some(pane[idx - 1])
                    } else {
                        pane.get(idx + 1).copied()
                    }
                })
        } else {
            None
        };

        self.layout.remove_panel(panel_id);

        if removing_focused {
            self.focused_panel_id = neighbor
                .filter(|id| self.panels.contains_key(id))
                .or_else(|| self.layout.all_panel_ids().into_iter().next());
        }

        true
    }

    /// Reopen the most recently closed panel as a tab in the focused pane.
    /// Returns the reopened panel ID. Terminals get a fresh shell (live process
    /// state isn't captured), but the panel's type/dir/command are restored.
    pub fn reopen_last_closed_panel(&mut self) -> Option<Uuid> {
        let mut panel = self.closed_panels.pop()?;
        // Fresh identity + no stale runtime state.
        panel.id = Uuid::new_v4();
        panel.pending_scrollback = None;
        panel.tty_name = None;
        let new_id = panel.id;
        self.panels.insert(new_id, panel);
        let target = self
            .focused_panel_id
            .or_else(|| self.layout.all_panel_ids().into_iter().next());
        if let Some(target) = target {
            self.layout.add_panel_to_pane(target, new_id);
        } else {
            self.layout = LayoutNode::single_pane(new_id);
        }
        self.previous_focused_panel_id = self.focused_panel_id;
        self.focused_panel_id = Some(new_id);
        Some(new_id)
    }

    /// Detach a panel from the workspace, returning it.
    /// Removes the panel from both the panels map and the layout tree,
    /// but does NOT destroy it — the caller can re-insert it elsewhere.
    pub fn detach_panel(&mut self, panel_id: Uuid) -> Option<Panel> {
        let panel = self.panels.remove(&panel_id)?;
        self.layout.remove_panel(panel_id);

        // Update focused panel if we just detached the focused one
        if self.focused_panel_id == Some(panel_id) {
            self.focused_panel_id = self.layout.all_panel_ids().into_iter().next();
        }
        if self.previous_focused_panel_id == Some(panel_id) {
            self.previous_focused_panel_id = None;
        }
        if self.zoomed_panel_id == Some(panel_id) {
            self.zoomed_panel_id = None;
        }

        Some(panel)
    }

    /// Move a panel (tab) into the pane that currently contains
    /// `target_panel_id`, within this workspace. Layout-only — the panel stays
    /// in the panels map. Returns true on success; false if already in the same
    /// pane or either panel is missing.
    pub fn move_panel_to_pane(&mut self, source_panel_id: Uuid, target_panel_id: Uuid) -> bool {
        if source_panel_id == target_panel_id {
            return false;
        }
        if !self.panels.contains_key(&source_panel_id)
            || !self.panels.contains_key(&target_panel_id)
        {
            return false;
        }
        // No-op if they already share a pane (the caller handles reordering).
        if let Some(ids) = self.layout.find_pane_with_panel_readonly(target_panel_id) {
            if ids.contains(&source_panel_id) {
                return false;
            }
        }
        self.layout.remove_panel(source_panel_id);
        if self.layout.add_panel_to_pane(target_panel_id, source_panel_id) {
            self.previous_focused_panel_id = self.focused_panel_id;
            self.focused_panel_id = Some(source_panel_id);
            true
        } else {
            false
        }
    }

    /// Split the pane containing `target_panel_id`, moving `source_panel_id`
    /// into a new adjacent pane on the given side (within this workspace).
    /// Layout-only. Returns true on success.
    pub fn split_panel_into_pane(
        &mut self,
        source_panel_id: Uuid,
        target_panel_id: Uuid,
        orientation: SplitOrientation,
        direction: super::panel::Direction,
    ) -> bool {
        if source_panel_id == target_panel_id {
            return false;
        }
        if !self.panels.contains_key(&source_panel_id)
            || !self.panels.contains_key(&target_panel_id)
        {
            return false;
        }
        // Detach source from its current pane (keeps it in the panels map).
        self.layout.remove_panel(source_panel_id);
        if self
            .layout
            .split_pane_with_panel(target_panel_id, source_panel_id, orientation, direction)
        {
            self.previous_focused_panel_id = self.focused_panel_id;
            self.focused_panel_id = Some(source_panel_id);
            true
        } else {
            false
        }
    }

    /// Insert a panel into the workspace by splitting the focused pane.
    /// Returns true if the panel was inserted successfully.
    pub fn insert_panel(&mut self, panel: Panel, orientation: SplitOrientation) -> bool {
        let panel_id = panel.id;
        self.panels.insert(panel_id, panel);

        let mut split_done = false;
        if let Some(focused_id) = self.focused_panel_id {
            if let Some(pane) = self.layout.find_pane_with_panel(focused_id) {
                let old = std::mem::replace(
                    pane,
                    LayoutNode::Pane {
                        panel_ids: vec![],
                        selected_panel_id: None,
                    },
                );
                *pane = old.split(orientation, panel_id);
                split_done = true;
            }
        }

        if !split_done {
            let old = std::mem::replace(
                &mut self.layout,
                LayoutNode::Pane {
                    panel_ids: vec![],
                    selected_panel_id: None,
                },
            );
            self.layout = old.split(orientation, panel_id);
        }

        self.previous_focused_panel_id = self.focused_panel_id;
        self.focused_panel_id = Some(panel_id);
        true
    }

    /// Move a panel into a new split adjacent to the focused pane.
    /// The panel is removed from its current pane and placed in a new
    /// split in the given direction.
    pub fn drag_to_split(&mut self, panel_id: Uuid, direction: super::panel::Direction) -> bool {
        use super::panel::Direction;

        if !self.panels.contains_key(&panel_id) {
            return false;
        }

        let orientation = match direction {
            Direction::Left | Direction::Right => SplitOrientation::Horizontal,
            Direction::Up | Direction::Down => SplitOrientation::Vertical,
        };

        // Determine the target pane (focused panel's pane, or if the dragged
        // panel IS the focused panel, pick the first other panel's pane).
        let target_panel_id = if self.focused_panel_id == Some(panel_id) {
            self.layout
                .all_panel_ids()
                .into_iter()
                .find(|&id| id != panel_id)
        } else {
            self.focused_panel_id
        };

        let Some(target_panel_id) = target_panel_id else {
            return false; // Can't split — only one panel
        };

        // Remove the panel from its current position in the layout
        self.layout.remove_panel(panel_id);

        // Split the target pane with the panel
        if !self
            .layout
            .split_pane_with_panel(target_panel_id, panel_id, orientation, direction)
        {
            // Fallback: re-add as a tab (shouldn't happen, but be safe)
            self.layout.add_panel_to_pane(target_panel_id, panel_id);
        }

        self.previous_focused_panel_id = self.focused_panel_id;
        self.focused_panel_id = Some(panel_id);
        true
    }

    /// Get a reference to a panel by ID.
    pub fn panel(&self, id: Uuid) -> Option<&Panel> {
        self.panels.get(&id)
    }

    /// Get a mutable reference to a panel by ID.
    #[allow(dead_code)]
    pub fn panel_mut(&mut self, id: Uuid) -> Option<&mut Panel> {
        self.panels.get_mut(&id)
    }

    /// Get all panel IDs in layout order.
    pub fn panel_ids(&self) -> Vec<Uuid> {
        self.layout.all_panel_ids()
    }

    /// Check if the workspace has no panels.
    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }

    const MAX_STATUS_ENTRIES: usize = 100;
    const MAX_STATUS_KEY_LEN: usize = 256;
    const MAX_STATUS_VALUE_LEN: usize = 4096;

    /// Update the status entry for a key, creating it if it doesn't exist.
    #[allow(dead_code)]
    pub fn set_status(&mut self, key: &str, value: &str, icon: Option<&str>, color: Option<&str>) {
        self.set_status_with_url(key, value, icon, color, None);
    }

    /// Update the status entry for a key with an optional URL.
    pub fn set_status_with_url(
        &mut self,
        key: &str,
        value: &str,
        icon: Option<&str>,
        color: Option<&str>,
        url: Option<&str>,
    ) {
        let key = truncate_str(key, Self::MAX_STATUS_KEY_LEN);
        let value = truncate_str(value, Self::MAX_STATUS_VALUE_LEN);
        let icon = icon.map(|s| truncate_str(s, 256));
        let color = color.map(|s| truncate_str(s, 64));
        let url = url.map(|s| truncate_str(s, 2048));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        if let Some(entry) = self.status_entries.iter_mut().find(|e| e.key == key) {
            entry.value = value.to_string();
            entry.icon = icon.map(|s| s.to_string());
            entry.color = color.map(|s| s.to_string());
            entry.url = url.map(|s| s.to_string());
            entry.timestamp = now;
        } else {
            if self.status_entries.len() >= Self::MAX_STATUS_ENTRIES {
                if let Some(oldest_idx) = self
                    .status_entries
                    .iter()
                    .enumerate()
                    .min_by(|a, b| {
                        a.1.timestamp
                            .partial_cmp(&b.1.timestamp)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(idx, _)| idx)
                {
                    self.status_entries.remove(oldest_idx);
                }
            }
            self.status_entries.push(StatusEntry {
                key: key.to_string(),
                value: value.to_string(),
                icon: icon.map(|s| s.to_string()),
                color: color.map(|s| s.to_string()),
                url: url.map(|s| s.to_string()),
                timestamp: now,
            });
        }
    }

    const MAX_METADATA_ENTRIES: usize = 100;
    const MAX_METADATA_BLOCKS: usize = 50;
    const MAX_BLOCK_CONTENT_LEN: usize = 32768;

    /// Set or update a rich metadata entry (with priority, URL, format).
    #[allow(clippy::too_many_arguments)]
    pub fn set_metadata(
        &mut self,
        key: &str,
        value: &str,
        icon: Option<&str>,
        color: Option<&str>,
        url: Option<&str>,
        priority: i32,
        format: MetadataFormat,
    ) {
        let key = truncate_str(key, Self::MAX_STATUS_KEY_LEN);
        let value = truncate_str(value, Self::MAX_STATUS_VALUE_LEN);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        if let Some(entry) = self.metadata_entries.iter_mut().find(|e| e.key == key) {
            entry.value = value.to_string();
            entry.icon = icon.map(|s| s.to_string());
            entry.color = color.map(|s| s.to_string());
            entry.url = url.map(|s| s.to_string());
            entry.priority = priority;
            entry.format = format;
            entry.timestamp = now;
        } else {
            if self.metadata_entries.len() >= Self::MAX_METADATA_ENTRIES {
                // Evict lowest priority (then oldest)
                if let Some(idx) = self
                    .metadata_entries
                    .iter()
                    .enumerate()
                    .min_by(|a, b| {
                        a.1.priority.cmp(&b.1.priority).then(
                            a.1.timestamp
                                .partial_cmp(&b.1.timestamp)
                                .unwrap_or(std::cmp::Ordering::Equal),
                        )
                    })
                    .map(|(i, _)| i)
                {
                    self.metadata_entries.remove(idx);
                }
            }
            self.metadata_entries.push(MetadataEntry {
                key: key.to_string(),
                value: value.to_string(),
                icon: icon.map(|s| s.to_string()),
                color: color.map(|s| s.to_string()),
                url: url.map(|s| s.to_string()),
                priority,
                format,
                timestamp: now,
            });
        }
    }

    /// Remove a metadata entry by key.
    pub fn clear_metadata(&mut self, key: &str) {
        self.metadata_entries.retain(|e| e.key != key);
    }

    /// Set or update a freeform metadata block.
    pub fn set_metadata_block(&mut self, key: &str, content: &str, priority: i32) {
        let key = truncate_str(key, Self::MAX_STATUS_KEY_LEN);
        let content = truncate_str(content, Self::MAX_BLOCK_CONTENT_LEN);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        if let Some(block) = self.metadata_blocks.iter_mut().find(|b| b.key == key) {
            block.content = content.to_string();
            block.priority = priority;
            block.timestamp = now;
        } else {
            if self.metadata_blocks.len() >= Self::MAX_METADATA_BLOCKS {
                if let Some(idx) = self
                    .metadata_blocks
                    .iter()
                    .enumerate()
                    .min_by(|a, b| {
                        a.1.priority.cmp(&b.1.priority).then(
                            a.1.timestamp
                                .partial_cmp(&b.1.timestamp)
                                .unwrap_or(std::cmp::Ordering::Equal),
                        )
                    })
                    .map(|(i, _)| i)
                {
                    self.metadata_blocks.remove(idx);
                }
            }
            self.metadata_blocks.push(MetadataBlock {
                key: key.to_string(),
                content: content.to_string(),
                priority,
                timestamp: now,
            });
        }
    }

    /// Remove a metadata block by key.
    pub fn clear_metadata_block(&mut self, key: &str) {
        self.metadata_blocks.retain(|b| b.key != key);
    }

    const MAX_LOG_ENTRIES: usize = 1000;
    const MAX_LOG_MESSAGE_LEN: usize = 8192;

    /// Append a log entry.
    pub fn append_log(&mut self, message: &str, level: &str, source: Option<&str>) {
        let message = truncate_str(message, Self::MAX_LOG_MESSAGE_LEN);
        let level = truncate_str(level, 64);
        let source = source.map(|s| truncate_str(s, 256));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        if self.log_entries.len() >= Self::MAX_LOG_ENTRIES {
            self.log_entries.drain(..self.log_entries.len() / 4);
        }

        self.log_entries.push(LogEntry {
            message: message.to_string(),
            level: level.to_string(),
            source: source.map(|s| s.to_string()),
            timestamp: now,
        });
    }

    /// Most relevant status label for the sidebar.
    pub fn sidebar_status_label(&self) -> Option<&str> {
        self.status_entries
            .iter()
            .rev()
            .find(|entry| entry.key == "agent")
            .or_else(|| self.status_entries.last())
            .map(|entry| entry.value.as_str())
    }

    /// Record an attention event from a notification.
    pub fn record_notification(&mut self, title: &str, body: &str, panel_id: Option<Uuid>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        self.unread_count = self.unread_count.saturating_add(1);
        self.latest_notification = Some(notification_summary(title, body));
        self.latest_notification_at = Some(now);
        self.attention_panel_id = panel_id.filter(|id| self.panels.contains_key(id));
    }

    /// Mark all workspace notifications as read.
    pub fn mark_notifications_read(&mut self) {
        self.unread_count = 0;
    }

    /// Clear the attention ring (called when workspace is focused/selected).
    pub fn clear_attention(&mut self) {
        self.attention_panel_id = None;
    }

    /// Focus a specific panel and reveal its tab.
    pub fn focus_panel(&mut self, panel_id: Uuid) -> bool {
        if !self.panels.contains_key(&panel_id) {
            return false;
        }

        if self.layout.select_panel(panel_id) {
            if self.focused_panel_id != Some(panel_id) {
                self.previous_focused_panel_id = self.focused_panel_id;
            }
            self.focused_panel_id = Some(panel_id);
            true
        } else {
            false
        }
    }
}

fn notification_summary(title: &str, body: &str) -> String {
    let title = title.trim();
    let body = body.trim();
    let summary = match (title.is_empty(), body.is_empty()) {
        (false, false) if body == title => title.to_string(),
        (false, false) => format!("{title}: {body}"),
        (false, true) => title.to_string(),
        (true, false) => body.to_string(),
        (true, true) => "Notification".to_string(),
    };

    let single_line = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_for_sidebar(&single_line, 120)
}

fn truncate_for_sidebar(text: &str, max_chars: usize) -> String {
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_workspace() {
        let ws = Workspace::new();
        assert_eq!(ws.panels.len(), 1);
        assert!(ws.focused_panel_id.is_some());
        assert_eq!(ws.display_title(), "Terminal");
        let panel_id = ws.focused_panel_id.expect("workspace should have a panel");
        assert_eq!(
            ws.panels
                .get(&panel_id)
                .and_then(|panel| panel.directory.as_deref()),
            None
        );
    }

    #[test]
    fn test_move_panel_between_panes() {
        let mut ws = Workspace::new();
        let a = ws.focused_panel_id.unwrap();
        // Split into two panes: a | b.
        let b = ws.split(SplitOrientation::Horizontal, PanelType::Terminal);
        // a and b are in separate panes.
        assert!(!ws
            .layout
            .find_pane_with_panel_readonly(b)
            .unwrap()
            .contains(&a));
        // Move a into b's pane.
        assert!(ws.move_panel_to_pane(a, b));
        let pane = ws.layout.find_pane_with_panel_readonly(b).unwrap();
        assert!(pane.contains(&a) && pane.contains(&b));
        assert_eq!(ws.focused_panel_id, Some(a));
        assert_eq!(ws.panels.len(), 2);
        // Same-pane move is a no-op.
        assert!(!ws.move_panel_to_pane(a, b));
    }

    #[test]
    fn test_split_panel_into_pane() {
        use super::super::panel::Direction;
        let mut ws = Workspace::new();
        let a = ws.focused_panel_id.unwrap();
        // Add b as a tab in a's pane (single pane, two tabs).
        let b = Panel::new_terminal();
        let b_id = b.id;
        ws.panels.insert(b_id, b);
        ws.layout.add_panel_to_pane(a, b_id);
        // Same pane to start.
        assert!(ws
            .layout
            .find_pane_with_panel_readonly(a)
            .unwrap()
            .contains(&b_id));
        // Drag b to the right edge of a's pane → split off into a new pane.
        assert!(ws.split_panel_into_pane(b_id, a, SplitOrientation::Horizontal, Direction::Right));
        // Now a and b are in separate panes.
        assert!(!ws
            .layout
            .find_pane_with_panel_readonly(a)
            .unwrap()
            .contains(&b_id));
        assert_eq!(ws.focused_panel_id, Some(b_id));
        assert_eq!(ws.panels.len(), 2);
    }

    #[test]
    fn test_split_workspace() {
        let mut ws = Workspace::new();
        let new_id = ws.split(SplitOrientation::Horizontal, PanelType::Terminal);
        assert_eq!(ws.panels.len(), 2);
        assert_eq!(ws.focused_panel_id, Some(new_id));
        assert_eq!(
            ws.panels
                .get(&new_id)
                .and_then(|panel| panel.directory.as_deref()),
            None
        );
    }

    #[test]
    fn test_with_directory_updates_initial_terminal_panel() {
        let ws = Workspace::with_directory("/tmp/jmux-test");
        let panel_id = ws.focused_panel_id.expect("workspace should have a panel");
        assert_eq!(ws.current_directory, "/tmp/jmux-test");
        assert_eq!(
            ws.panels
                .get(&panel_id)
                .and_then(|panel| panel.directory.as_deref()),
            Some("/tmp/jmux-test")
        );
    }

    #[test]
    fn test_remove_panel() {
        let mut ws = Workspace::new();
        let new_id = ws.split(SplitOrientation::Horizontal, PanelType::Terminal);
        assert!(ws.remove_panel(new_id));
        assert_eq!(ws.panels.len(), 1);
    }

    #[test]
    fn test_reopen_closed_tab() {
        let mut ws = Workspace::new();
        let new_id = ws.split(SplitOrientation::Horizontal, PanelType::Terminal);
        assert_eq!(ws.panels.len(), 2);
        ws.remove_panel(new_id);
        assert_eq!(ws.panels.len(), 1);
        assert_eq!(ws.closed_panels.len(), 1);

        let reopened = ws.reopen_last_closed_panel().expect("a reopened panel");
        assert_eq!(ws.panels.len(), 2);
        assert_eq!(ws.closed_panels.len(), 0);
        assert_eq!(ws.focused_panel_id, Some(reopened));
        // Fresh identity (not the original id).
        assert_ne!(reopened, new_id);
        // Nothing left to reopen.
        assert!(ws.reopen_last_closed_panel().is_none());
    }

    #[test]
    fn test_status_entries() {
        let mut ws = Workspace::new();
        ws.set_status("agent", "claude-code", Some("robot"), None);
        assert_eq!(ws.status_entries.len(), 1);
        ws.set_status("agent", "claude-code v2", None, None);
        assert_eq!(ws.status_entries.len(), 1);
        assert_eq!(ws.status_entries[0].value, "claude-code v2");
    }

    #[test]
    fn test_status_entry_eviction_preserves_remaining_order() {
        let mut ws = Workspace::new();

        for i in 0..100 {
            ws.set_status(&format!("key-{i}"), &format!("value-{i}"), None, None);
        }

        ws.set_status("key-100", "value-100", None, None);

        assert_eq!(ws.status_entries.len(), 100);
        assert_eq!(
            ws.status_entries.first().map(|entry| entry.key.as_str()),
            Some("key-1")
        );
        assert_eq!(
            ws.status_entries.last().map(|entry| entry.key.as_str()),
            Some("key-100")
        );
    }

    #[test]
    fn test_record_notification_updates_unread_and_summary() {
        let mut ws = Workspace::new();
        let panel_id = ws.focused_panel_id;
        ws.record_notification("Codex", "Waiting for input", panel_id);

        assert_eq!(ws.unread_count, 1);
        assert_eq!(
            ws.latest_notification.as_deref(),
            Some("Codex: Waiting for input")
        );
        assert_eq!(ws.attention_panel_id, panel_id);
        assert!(ws.latest_notification_at.is_some());
    }

    #[test]
    fn test_record_notification_does_not_steal_focus() {
        let mut ws = Workspace::new();
        let original_focus = ws
            .focused_panel_id
            .expect("workspace should have a focused panel");
        let other_panel_id = ws.split(SplitOrientation::Horizontal, PanelType::Terminal);
        assert_eq!(ws.focused_panel_id, Some(other_panel_id));

        ws.focus_panel(original_focus);
        ws.record_notification("Codex", "Waiting for input", Some(other_panel_id));

        assert_eq!(ws.focused_panel_id, Some(original_focus));
        assert_eq!(ws.attention_panel_id, Some(other_panel_id));
    }

    #[test]
    fn test_mark_notifications_read_clears_unread_count() {
        let mut ws = Workspace::new();
        ws.record_notification("Claude Code", "Approval needed", None);
        assert_eq!(ws.unread_count, 1);

        ws.mark_notifications_read();
        assert_eq!(ws.unread_count, 0);
    }

    #[test]
    fn test_split_falls_back_to_root_when_focused_panel_is_stale() {
        let mut ws = Workspace::new();
        ws.focused_panel_id = Some(uuid::Uuid::new_v4());

        let new_id = ws.split(SplitOrientation::Horizontal, PanelType::Terminal);

        assert_eq!(ws.focused_panel_id, Some(new_id));
        assert!(ws.layout.all_panel_ids().contains(&new_id));
    }

    #[test]
    fn test_focus_panel_does_not_update_focus_when_layout_select_fails() {
        let mut ws = Workspace::new();
        let original_focus = ws.focused_panel_id;
        let panel_id = original_focus.expect("workspace should have a focused panel");

        ws.layout = LayoutNode::single_pane(uuid::Uuid::new_v4());

        assert!(!ws.focus_panel(panel_id));
        assert_eq!(ws.focused_panel_id, original_focus);
    }

    #[test]
    fn test_truncate_str_preserves_utf8_boundaries() {
        assert_eq!(truncate_str("abcdef", 4), "abcd");
        assert_eq!(truncate_str("あいう", 4), "あ");
    }
}
