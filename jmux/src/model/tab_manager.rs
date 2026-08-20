//! TabManager — manages the collection of workspaces.

use std::time::SystemTime;

use uuid::Uuid;

use super::workspace::Workspace;
use super::workspace_group::WorkspaceGroup;

/// A recently-closed workspace, retained for the History pane and reopen.
#[derive(Debug, Clone)]
pub struct ClosedEntry {
    pub workspace: Workspace,
    /// Wall-clock time the workspace was closed.
    pub closed_at: SystemTime,
    /// Display name captured at close time.
    pub title: String,
}

/// Manages all workspaces and tracks the currently selected one.
///
/// This is the top-level model for the sidebar workspace list.
#[derive(Debug)]
pub struct TabManager {
    workspaces: Vec<Workspace>,
    selected_index: Option<usize>,
    /// Sidebar groups. A workspace references its group via `group_id`.
    groups: Vec<WorkspaceGroup>,
    /// Focus history (workspace IDs, oldest first) for back/forward navigation.
    focus_history: Vec<Uuid>,
    /// Current position within `focus_history`.
    focus_pos: usize,
    /// Recently-closed workspaces (most recent last) for reopen + History.
    closed_stack: Vec<ClosedEntry>,
}

/// Maximum number of recently-closed workspaces retained for reopen.
const CLOSED_STACK_CAP: usize = 10;

/// Maximum number of entries retained in the focus history.
const FOCUS_HISTORY_CAP: usize = 50;

impl TabManager {
    /// Create a new TabManager with a single default workspace.
    pub fn new() -> Self {
        let ws = Workspace::new();
        Self {
            workspaces: vec![ws],
            selected_index: Some(0),
            groups: Vec::new(),
            focus_history: Vec::new(),
            focus_pos: 0,
            closed_stack: Vec::new(),
        }
    }

    /// Create an empty TabManager (for restoring from session).
    pub fn empty() -> Self {
        Self {
            workspaces: Vec::new(),
            selected_index: None,
            groups: Vec::new(),
            focus_history: Vec::new(),
            focus_pos: 0,
            closed_stack: Vec::new(),
        }
    }

    /// Number of workspaces.
    pub fn len(&self) -> usize {
        self.workspaces.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }

    /// Get the currently selected workspace index.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Get the currently selected workspace.
    pub fn selected(&self) -> Option<&Workspace> {
        self.selected_index.and_then(|i| self.workspaces.get(i))
    }

    /// Get the currently selected workspace ID.
    pub fn selected_id(&self) -> Option<Uuid> {
        self.selected().map(|ws| ws.id)
    }

    /// Get the currently selected workspace mutably.
    pub fn selected_mut(&mut self) -> Option<&mut Workspace> {
        self.selected_index.and_then(|i| self.workspaces.get_mut(i))
    }

    /// Select a workspace by index.
    pub fn select(&mut self, index: usize) -> bool {
        if index < self.workspaces.len() {
            self.selected_index = Some(index);
            true
        } else {
            false
        }
    }

    /// Select workspace by ID.
    pub fn select_by_id(&mut self, id: Uuid) -> bool {
        if let Some(index) = self.workspaces.iter().position(|w| w.id == id) {
            self.selected_index = Some(index);
            true
        } else {
            false
        }
    }

    /// The workspace a window should render. Every window (including the quake
    /// drop-down, which is the only window in quake-daemon mode) follows the
    /// global selection; the `window_id` arg is retained for call-site clarity.
    pub fn selected_for_window(&self, _window_id: Uuid) -> Option<&Workspace> {
        self.selected()
    }

    /// Select the next workspace (wrapping around).
    pub fn select_next(&mut self, wrap: bool) {
        if self.workspaces.is_empty() {
            return;
        }
        match self.selected_index {
            Some(i) if i + 1 < self.workspaces.len() => {
                self.selected_index = Some(i + 1);
            }
            Some(_) if wrap => {
                self.selected_index = Some(0);
            }
            None => {
                self.selected_index = Some(0);
            }
            _ => {}
        }
    }

    /// Select the previous workspace (wrapping around).
    pub fn select_previous(&mut self, wrap: bool) {
        if self.workspaces.is_empty() {
            return;
        }
        match self.selected_index {
            Some(0) if wrap => {
                self.selected_index = Some(self.workspaces.len() - 1);
            }
            Some(i) if i > 0 => {
                self.selected_index = Some(i - 1);
            }
            None => {
                self.selected_index = Some(self.workspaces.len() - 1);
            }
            _ => {}
        }
    }

    /// Select the last workspace.
    pub fn select_last(&mut self) {
        if !self.workspaces.is_empty() {
            self.selected_index = Some(self.workspaces.len() - 1);
        }
    }

    /// Add a new workspace. Returns the new workspace's ID.
    pub fn add_workspace(&mut self, workspace: Workspace) -> Uuid {
        let id = workspace.id;
        self.workspaces.push(workspace);
        self.selected_index = Some(self.workspaces.len() - 1);
        id
    }

    /// Add a new workspace at the top of the list.
    pub fn add_workspace_at_top(&mut self, workspace: Workspace) -> Uuid {
        let id = workspace.id;
        self.workspaces.insert(0, workspace);
        // Shift selection to follow the inserted workspace
        self.selected_index = Some(0);
        id
    }

    /// Add a new workspace after the current one.
    pub fn add_workspace_after_current(&mut self, workspace: Workspace) -> Uuid {
        let id = workspace.id;
        let insert_at = self.selected_index.map(|i| i + 1).unwrap_or(0);
        self.workspaces.insert(insert_at, workspace);
        self.selected_index = Some(insert_at);
        id
    }

    /// Add a workspace using a placement strategy.
    pub fn add_workspace_with_placement(
        &mut self,
        workspace: Workspace,
        placement: crate::settings::NewWorkspacePlacement,
    ) -> Uuid {
        match placement {
            crate::settings::NewWorkspacePlacement::End => self.add_workspace(workspace),
            crate::settings::NewWorkspacePlacement::AfterCurrent => {
                self.add_workspace_after_current(workspace)
            }
            crate::settings::NewWorkspacePlacement::Top => self.add_workspace_at_top(workspace),
        }
    }

    /// Add a workspace at the placement's position but leave the user where they
    /// are: the same workspace stays selected, so nothing steals their view.
    ///
    /// Used by the goal-graph scheduler, whose node workspaces run their agent
    /// headlessly and must not need to be visible. Inserting *before* the
    /// selected index shifts it by one — the selection follows the workspace,
    /// not the slot.
    pub fn add_workspace_keep_selection(
        &mut self,
        workspace: Workspace,
        placement: crate::settings::NewWorkspacePlacement,
    ) -> Uuid {
        use crate::settings::NewWorkspacePlacement as P;
        let id = workspace.id;
        let insert_at = match placement {
            P::End => self.workspaces.len(),
            P::Top => 0,
            P::AfterCurrent => self
                .selected_index
                .map(|i| i + 1)
                .unwrap_or(self.workspaces.len()),
        };
        self.workspaces.insert(insert_at, workspace);
        if let Some(sel) = self.selected_index {
            if insert_at <= sel {
                self.selected_index = Some(sel + 1);
            }
        }
        id
    }

    /// Remove a workspace by index. Returns the removed workspace.
    pub fn remove(&mut self, index: usize) -> Option<Workspace> {
        if index >= self.workspaces.len() {
            return None;
        }
        let ws = self.workspaces.remove(index);

        // Record for reopen — skip empty workspaces (e.g. an emptied move source).
        if !ws.panels.is_empty() {
            self.closed_stack.push(ClosedEntry {
                title: ws.display_title().to_string(),
                closed_at: SystemTime::now(),
                workspace: ws.clone(),
            });
            if self.closed_stack.len() > CLOSED_STACK_CAP {
                let drop = self.closed_stack.len() - CLOSED_STACK_CAP;
                self.closed_stack.drain(0..drop);
            }
        }

        // Adjust selection
        if self.workspaces.is_empty() {
            self.selected_index = None;
        } else if let Some(sel) = self.selected_index {
            if sel >= self.workspaces.len() {
                self.selected_index = Some(self.workspaces.len() - 1);
            } else if sel > index {
                self.selected_index = Some(sel - 1);
            }
        }

        Some(ws)
    }

    /// Remove a workspace by ID. Returns the removed workspace.
    pub fn remove_by_id(&mut self, id: Uuid) -> Option<Workspace> {
        let index = self.workspaces.iter().position(|w| w.id == id)?;
        self.remove(index)
    }

    /// Close the tab `panel_id` wherever it lives. If it was the workspace's last
    /// tab, remove the workspace too — "closing the last tab closes the
    /// workspace." Returns true if the panel was found and removed.
    pub fn close_panel(&mut self, panel_id: Uuid) -> bool {
        let Some(ws) = self.find_workspace_with_panel_mut(panel_id) else {
            return false;
        };
        let removed = ws.remove_panel(panel_id);
        if removed && ws.is_empty() {
            let ws_id = ws.id;
            self.remove_by_id(ws_id);
        }
        removed
    }

    /// Get a workspace by ID.
    pub fn workspace(&self, id: Uuid) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// Get a workspace by ID mutably.
    pub fn workspace_mut(&mut self, id: Uuid) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| w.id == id)
    }

    /// Get a workspace by index.
    pub fn get(&self, index: usize) -> Option<&Workspace> {
        self.workspaces.get(index)
    }

    /// Get a workspace by index mutably.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Workspace> {
        self.workspaces.get_mut(index)
    }

    /// Iterate over all workspaces.
    pub fn iter(&self) -> impl Iterator<Item = &Workspace> {
        self.workspaces.iter()
    }

    /// Iterate over all workspaces mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Workspace> {
        self.workspaces.iter_mut()
    }

    /// Select the workspace with the newest unread notification.
    pub fn select_latest_unread(&mut self) -> Option<Uuid> {
        let index = self.latest_unread_index()?;
        self.selected_index = Some(index);
        self.workspaces.get(index).map(|ws| ws.id)
    }

    /// Index of the workspace with the newest unread notification.
    pub fn latest_unread_index(&self) -> Option<usize> {
        self.workspaces
            .iter()
            .enumerate()
            .filter(|(_, ws)| ws.unread_count > 0)
            .max_by(|(_, a), (_, b)| {
                let a_ts = a.latest_notification_at.unwrap_or(0.0);
                let b_ts = b.latest_notification_at.unwrap_or(0.0);
                a_ts.total_cmp(&b_ts)
            })
            .map(|(index, _)| index)
    }

    /// Move a workspace from one index to another.
    pub fn move_workspace(&mut self, from: usize, to: usize) -> bool {
        if from >= self.workspaces.len() || to >= self.workspaces.len() || from == to {
            return from == to && from < self.workspaces.len();
        }
        let previous_selection = self.selected_index;
        let ws = self.workspaces.remove(from);
        self.workspaces.insert(to, ws);

        // Adjust selection to follow the moved workspace
        if let Some(selected) = previous_selection {
            self.selected_index = if selected == from {
                Some(to)
            } else if from < to && selected > from && selected <= to {
                Some(selected - 1)
            } else if from > to && selected >= to && selected < from {
                Some(selected + 1)
            } else {
                Some(selected)
            };
        }
        true
    }

    /// Find the index of a workspace by ID.
    pub fn workspace_index(&self, id: Uuid) -> Option<usize> {
        self.workspaces.iter().position(|w| w.id == id)
    }

    /// Close all non-pinned workspaces except the given one. Returns the count closed.
    pub fn close_others(&mut self, keep_id: Uuid) -> usize {
        let to_remove: Vec<usize> = self
            .workspaces
            .iter()
            .enumerate()
            .filter(|(_, ws)| ws.id != keep_id && !ws.is_pinned)
            .map(|(i, _)| i)
            .collect();
        let count = to_remove.len();
        for &i in to_remove.iter().rev() {
            self.workspaces.remove(i);
        }
        // Fix selection
        if let Some(new_idx) = self.workspaces.iter().position(|ws| ws.id == keep_id) {
            self.selected_index = Some(new_idx);
        } else if self.workspaces.is_empty() {
            self.selected_index = None;
        } else {
            self.selected_index = Some(0);
        }
        count
    }

    /// Close all non-pinned workspaces above (before) the given one. Returns the count closed.
    pub fn close_above(&mut self, workspace_id: Uuid) -> usize {
        let Some(target_idx) = self.workspace_index(workspace_id) else {
            return 0;
        };
        let to_remove: Vec<usize> = self.workspaces[..target_idx]
            .iter()
            .enumerate()
            .filter(|(_, ws)| !ws.is_pinned)
            .map(|(i, _)| i)
            .collect();
        let count = to_remove.len();
        for &i in to_remove.iter().rev() {
            self.workspaces.remove(i);
        }
        // Fix selection to follow the target workspace
        if let Some(new_idx) = self.workspaces.iter().position(|ws| ws.id == workspace_id) {
            self.selected_index = Some(new_idx);
        }
        count
    }

    /// Close all non-pinned workspaces below (after) the given one. Returns the count closed.
    pub fn close_below(&mut self, workspace_id: Uuid) -> usize {
        let Some(target_idx) = self.workspace_index(workspace_id) else {
            return 0;
        };
        let to_remove: Vec<usize> = self.workspaces[(target_idx + 1)..]
            .iter()
            .enumerate()
            .filter(|(_, ws)| !ws.is_pinned)
            .map(|(i, _)| target_idx + 1 + i)
            .collect();
        let count = to_remove.len();
        for &i in to_remove.iter().rev() {
            self.workspaces.remove(i);
        }
        // Fix selection
        if let Some(sel) = self.selected_index {
            if sel >= self.workspaces.len() {
                self.selected_index = Some(self.workspaces.len().saturating_sub(1));
            }
        }
        count
    }

    /// Find the workspace containing a panel with the given UUID.
    pub fn find_workspace_with_panel(&self, panel_id: Uuid) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|w| w.panels.contains_key(&panel_id))
    }

    /// Find the workspace containing a panel with the given UUID, mutably.
    pub fn find_workspace_with_panel_mut(&mut self, panel_id: Uuid) -> Option<&mut Workspace> {
        self.workspaces
            .iter_mut()
            .find(|w| w.panels.contains_key(&panel_id))
    }

    /// Move a panel from one workspace to another.
    ///
    /// Detaches the panel from the source workspace's layout and panel map and
    /// inserts it into the target workspace (splitting the focused pane
    /// horizontally). The source workspace is removed if it becomes empty.
    /// Returns the new workspace ID (target) on success, or `None` if the
    /// source/target/panel could not be found or the panel is already in the
    /// target workspace.
    pub fn move_panel_to_workspace(
        &mut self,
        panel_id: Uuid,
        target_workspace_id: Uuid,
    ) -> Option<Uuid> {
        use crate::model::panel::SplitOrientation;

        let source_ws_id = self.find_workspace_with_panel(panel_id).map(|ws| ws.id)?;

        // Reject move to same workspace
        if source_ws_id == target_workspace_id {
            return None;
        }

        // Ensure target exists
        self.workspace(target_workspace_id)?;

        // Detach from source
        let panel = self
            .workspace_mut(source_ws_id)?
            .detach_panel(panel_id)?;

        let source_empty = self
            .workspace(source_ws_id)
            .is_some_and(|ws| ws.is_empty());
        if source_empty {
            self.remove_by_id(source_ws_id);
        }

        // Insert into target
        let target_ws = self.workspace_mut(target_workspace_id)?;
        target_ws.insert_panel(panel, SplitOrientation::Horizontal);

        Some(target_workspace_id)
    }

    // -------------------------------------------------------------------
    // Workspace groups
    // -------------------------------------------------------------------

    /// All sidebar groups, in order.
    pub fn groups(&self) -> &[WorkspaceGroup] {
        &self.groups
    }

    /// Look up a group by ID.
    pub fn group(&self, id: Uuid) -> Option<&WorkspaceGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Look up a group by ID mutably.
    pub fn group_mut(&mut self, id: Uuid) -> Option<&mut WorkspaceGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Replace the group list wholesale (used by session restore).
    pub fn set_groups(&mut self, groups: Vec<WorkspaceGroup>) {
        self.groups = groups;
    }

    /// Create a new group with the given name (optionally scoped to a window).
    /// Returns the new group's ID.
    pub fn create_group(&mut self, name: impl Into<String>, window_id: Option<Uuid>) -> Uuid {
        let mut group = WorkspaceGroup::new(name);
        group.window_id = window_id;
        let id = group.id;
        self.groups.push(group);
        id
    }

    /// Delete a group, ungrouping any member workspaces. Returns true if found.
    pub fn remove_group(&mut self, id: Uuid) -> bool {
        let before = self.groups.len();
        self.groups.retain(|g| g.id != id);
        if self.groups.len() == before {
            return false;
        }
        for ws in self.workspaces.iter_mut() {
            if ws.group_id == Some(id) {
                ws.group_id = None;
            }
        }
        true
    }

    /// Rename a group. Returns true if found.
    pub fn rename_group(&mut self, id: Uuid, name: impl Into<String>) -> bool {
        match self.group_mut(id) {
            Some(g) => {
                g.name = name.into();
                true
            }
            None => false,
        }
    }

    /// Set a group's accent color (None clears it). Returns true if found.
    pub fn set_group_color(&mut self, id: Uuid, color: Option<String>) -> bool {
        match self.group_mut(id) {
            Some(g) => {
                g.color = color;
                true
            }
            None => false,
        }
    }

    /// Toggle (or set) a group's collapsed state. Returns the new state, or
    /// `None` if the group was not found.
    pub fn set_group_collapsed(&mut self, id: Uuid, collapsed: Option<bool>) -> Option<bool> {
        let g = self.group_mut(id)?;
        g.collapsed = collapsed.unwrap_or(!g.collapsed);
        Some(g.collapsed)
    }

    /// Assign a workspace to a group (or `None` to ungroup it). Member
    /// workspaces are kept contiguous by moving the workspace next to the
    /// group's existing members. Returns true if the workspace was found.
    pub fn assign_to_group(&mut self, workspace_id: Uuid, group_id: Option<Uuid>) -> bool {
        // Validate the target group exists when assigning.
        if let Some(gid) = group_id {
            if self.group(gid).is_none() {
                return false;
            }
        }
        let Some(idx) = self.workspace_index(workspace_id) else {
            return false;
        };
        self.workspaces[idx].group_id = group_id;

        // Keep members contiguous: move the workspace to sit just after the
        // last existing member of the target group (if any others exist).
        if let Some(gid) = group_id {
            if let Some(last_member) = self
                .workspaces
                .iter()
                .enumerate()
                .filter(|(i, ws)| *i != idx && ws.group_id == Some(gid))
                .map(|(i, _)| i)
                .next_back()
            {
                let to = if last_member > idx {
                    last_member
                } else {
                    last_member + 1
                };
                self.move_workspace(idx, to);
            }
        }
        true
    }

    /// Move a whole group (all its member workspaces, preserving their order)
    /// so the block sits immediately before `target_ws_id` (or at the end when
    /// `target_ws_id` is `None` or not found). Returns true if the group exists
    /// and has members.
    pub fn move_group_before(&mut self, group_id: Uuid, target_ws_id: Option<Uuid>) -> bool {
        if self.group(group_id).is_none() {
            return false;
        }
        let selected_id = self.selected_id();
        // Extract members in their current relative order.
        let mut members = Vec::new();
        let mut i = 0;
        while i < self.workspaces.len() {
            if self.workspaces[i].group_id == Some(group_id) {
                members.push(self.workspaces.remove(i));
            } else {
                i += 1;
            }
        }
        if members.is_empty() {
            return false;
        }
        // Where to re-insert (in the post-removal indexing).
        let insert_at = target_ws_id
            .and_then(|tid| self.workspaces.iter().position(|w| w.id == tid))
            .unwrap_or(self.workspaces.len());
        for (offset, m) in members.into_iter().enumerate() {
            self.workspaces.insert(insert_at + offset, m);
        }
        // Restore selection by id.
        if let Some(sid) = selected_id {
            self.selected_index = self.workspaces.iter().position(|w| w.id == sid);
        }
        true
    }

    /// Sum of unread counts across a group's member workspaces.
    pub fn group_unread_count(&self, group_id: Uuid) -> u32 {
        self.workspaces
            .iter()
            .filter(|ws| ws.group_id == Some(group_id))
            .map(|ws| ws.unread_count)
            .sum()
    }

    // -------------------------------------------------------------------
    // Focus history (back/forward navigation)
    // -------------------------------------------------------------------

    /// Record the currently selected workspace as the newest focus-history
    /// entry, unless it already is. Forward history (entries after the current
    /// position) is discarded, mirroring browser back/forward semantics.
    ///
    /// Intended to be called from a single chokepoint after any selection
    /// change (e.g. sidebar refresh); it is a no-op when the selection is
    /// unchanged, so calling it on every UI refresh is safe.
    pub fn record_focus_if_changed(&mut self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        // Drop stale entries that no longer correspond to a live workspace.
        if self.focus_history.get(self.focus_pos).copied() == Some(id) {
            return;
        }
        self.focus_history.truncate(self.focus_pos + 1);
        self.focus_history.push(id);
        if self.focus_history.len() > FOCUS_HISTORY_CAP {
            let drop = self.focus_history.len() - FOCUS_HISTORY_CAP;
            self.focus_history.drain(0..drop);
        }
        self.focus_pos = self.focus_history.len() - 1;
    }

    /// Reopen the most recently closed workspace (restoring its layout, panels,
    /// and directory; terminals get fresh shells). Returns the workspace ID, or
    /// `None` if nothing was closed.
    pub fn reopen_last_closed(&mut self) -> Option<Uuid> {
        let entry = self.closed_stack.pop()?;
        let ws = entry.workspace;
        let id = ws.id;
        // Avoid a duplicate if somehow still present.
        if self.workspace(id).is_some() {
            return Some(id);
        }
        self.add_workspace(ws);
        Some(id)
    }

    /// Recently-closed workspaces, most-recent last (for the History pane).
    pub fn closed_entries(&self) -> &[ClosedEntry] {
        &self.closed_stack
    }

    /// Reopen a specific closed workspace by ID (used by the History pane).
    /// Returns the reopened workspace ID, or `None` if not found.
    pub fn reopen_closed(&mut self, workspace_id: Uuid) -> Option<Uuid> {
        let pos = self
            .closed_stack
            .iter()
            .position(|e| e.workspace.id == workspace_id)?;
        let entry = self.closed_stack.remove(pos);
        let id = entry.workspace.id;
        if self.workspace(id).is_some() {
            return Some(id);
        }
        self.add_workspace(entry.workspace);
        Some(id)
    }

    /// Clear the recently-closed list ("Clear Closed" in the History pane).
    pub fn clear_closed(&mut self) {
        self.closed_stack.clear();
    }

    /// Replace the recently-closed list (used when restoring a session).
    /// Oldest entries are dropped if it exceeds the retention cap.
    pub fn set_closed_entries(&mut self, mut entries: Vec<ClosedEntry>) {
        if entries.len() > CLOSED_STACK_CAP {
            let drop = entries.len() - CLOSED_STACK_CAP;
            entries.drain(0..drop);
        }
        self.closed_stack = entries;
    }

    /// Focus history (workspace IDs, oldest first) for the History pane.
    pub fn focus_history(&self) -> &[Uuid] {
        &self.focus_history
    }

    /// Move one step back in focus history and select that workspace.
    /// Returns the now-selected workspace ID, or `None` if there is no earlier
    /// entry (or it no longer exists).
    pub fn focus_back(&mut self) -> Option<Uuid> {
        while self.focus_pos > 0 {
            self.focus_pos -= 1;
            if let Some(id) = self.focus_history.get(self.focus_pos).copied() {
                if self.select_by_id(id) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Move one step forward in focus history and select that workspace.
    pub fn focus_forward(&mut self) -> Option<Uuid> {
        while self.focus_pos + 1 < self.focus_history.len() {
            self.focus_pos += 1;
            if let Some(id) = self.focus_history.get(self.focus_pos).copied() {
                if self.select_by_id(id) {
                    return Some(id);
                }
            }
        }
        None
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tab_manager() {
        let tm = TabManager::new();
        assert_eq!(tm.len(), 1);
        assert_eq!(tm.selected_index(), Some(0));
    }

    #[test]
    fn test_add_and_select() {
        let mut tm = TabManager::new();
        let ws2 = Workspace::new();
        let id2 = tm.add_workspace(ws2);
        assert_eq!(tm.len(), 2);
        assert_eq!(tm.selected_index(), Some(1));

        tm.select(0);
        assert_eq!(tm.selected_index(), Some(0));

        tm.select_by_id(id2);
        assert_eq!(tm.selected_index(), Some(1));
    }

    #[test]
    fn test_remove() {
        let mut tm = TabManager::new();
        tm.add_workspace(Workspace::new());
        tm.add_workspace(Workspace::new());
        assert_eq!(tm.len(), 3);

        tm.select(1);
        tm.remove(0);
        assert_eq!(tm.len(), 2);
        // Selection should adjust
        assert_eq!(tm.selected_index(), Some(0));
    }

    #[test]
    fn test_navigation() {
        let mut tm = TabManager::new();
        tm.add_workspace(Workspace::new());
        tm.add_workspace(Workspace::new());
        tm.select(0);

        tm.select_next(false);
        assert_eq!(tm.selected_index(), Some(1));

        tm.select_next(true);
        assert_eq!(tm.selected_index(), Some(2));

        tm.select_next(true);
        assert_eq!(tm.selected_index(), Some(0));

        tm.select_previous(true);
        assert_eq!(tm.selected_index(), Some(2));

        tm.select_last();
        assert_eq!(tm.selected_index(), Some(2));
    }

    #[test]
    fn test_select_latest_unread_prefers_newest_notification() {
        let mut tm = TabManager::empty();

        let mut ws1 = Workspace::new();
        ws1.record_notification("Claude Code", "Waiting for input", None);
        let ws1_id = ws1.id;
        tm.add_workspace(ws1);

        std::thread::sleep(std::time::Duration::from_millis(1));

        let mut ws2 = Workspace::new();
        ws2.record_notification("Codex", "Approval needed", None);
        let ws2_id = ws2.id;
        tm.add_workspace(ws2);

        let selected = tm.select_latest_unread();
        assert_eq!(selected, Some(ws2_id));
        assert_ne!(selected, Some(ws1_id));
    }

    #[test]
    fn test_move_workspace_remaps_shifted_selection() {
        let mut tm = TabManager::new();
        tm.add_workspace(Workspace::new());
        tm.add_workspace(Workspace::new());
        tm.add_workspace(Workspace::new());

        tm.select(2);
        assert!(tm.move_workspace(0, 3));
        assert_eq!(tm.selected_index(), Some(1));

        tm.select(1);
        assert!(tm.move_workspace(3, 0));
        assert_eq!(tm.selected_index(), Some(2));
    }

    #[test]
    fn test_workspace_index() {
        let mut tm = TabManager::new();
        let ws2 = Workspace::new();
        let id2 = ws2.id;
        tm.add_workspace(ws2);
        assert_eq!(tm.workspace_index(id2), Some(1));
    }

    #[test]
    fn test_close_others_preserves_pinned() {
        let mut tm = TabManager::empty();
        let mut ws1 = Workspace::new();
        ws1.is_pinned = true;
        let ws1_id = ws1.id;
        tm.add_workspace(ws1);
        let ws2 = Workspace::new();
        let ws2_id = ws2.id;
        tm.add_workspace(ws2);
        let ws3 = Workspace::new();
        tm.add_workspace(ws3);

        let closed = tm.close_others(ws2_id);
        assert_eq!(closed, 1); // ws3 closed, ws1 pinned kept
        assert_eq!(tm.len(), 2);
        assert!(tm.workspace(ws1_id).is_some());
        assert!(tm.workspace(ws2_id).is_some());
    }

    #[test]
    fn test_close_above() {
        let mut tm = TabManager::empty();
        let ws1 = Workspace::new();
        tm.add_workspace(ws1);
        let ws2 = Workspace::new();
        tm.add_workspace(ws2);
        let ws3 = Workspace::new();
        let ws3_id = ws3.id;
        tm.add_workspace(ws3);
        let ws4 = Workspace::new();
        tm.add_workspace(ws4);

        let closed = tm.close_above(ws3_id);
        assert_eq!(closed, 2);
        assert_eq!(tm.len(), 2);
        assert_eq!(tm.workspace_index(ws3_id), Some(0));
    }

    #[test]
    fn test_close_below() {
        let mut tm = TabManager::empty();
        let ws1 = Workspace::new();
        let ws1_id = ws1.id;
        tm.add_workspace(ws1);
        let ws2 = Workspace::new();
        tm.add_workspace(ws2);
        let ws3 = Workspace::new();
        tm.add_workspace(ws3);

        let closed = tm.close_below(ws1_id);
        assert_eq!(closed, 2);
        assert_eq!(tm.len(), 1);
    }

    #[test]
    fn test_close_below_preserves_pinned() {
        let mut tm = TabManager::empty();
        let ws1 = Workspace::new();
        let ws1_id = ws1.id;
        tm.add_workspace(ws1);
        let mut ws2 = Workspace::new();
        ws2.is_pinned = true;
        let ws2_id = ws2.id;
        tm.add_workspace(ws2);
        let ws3 = Workspace::new();
        tm.add_workspace(ws3);

        let closed = tm.close_below(ws1_id);
        assert_eq!(closed, 1); // ws3 closed, ws2 pinned kept
        assert_eq!(tm.len(), 2);
        assert!(tm.workspace(ws2_id).is_some());
    }

    #[test]
    fn test_move_workspace_is_noop_when_from_equals_to() {
        let mut tm = TabManager::new();
        tm.add_workspace(Workspace::new());

        tm.select(1);
        assert!(tm.move_workspace(1, 1));
        assert_eq!(tm.selected_index(), Some(1));
        assert!(!tm.move_workspace(3, 3));
    }

    #[test]
    fn test_create_and_assign_group() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new());
        let gid = tm.create_group("Work", None);
        assert!(tm.assign_to_group(a, Some(gid)));
        assert!(tm.assign_to_group(b, Some(gid)));
        assert_eq!(tm.workspace(a).unwrap().group_id, Some(gid));
        assert_eq!(tm.workspace(b).unwrap().group_id, Some(gid));
    }

    #[test]
    fn test_assign_keeps_members_contiguous() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let _b = tm.add_workspace(Workspace::new());
        let c = tm.add_workspace(Workspace::new());
        let gid = tm.create_group("G", None);
        // Assign the two non-adjacent workspaces (index 0 and 2) to the group.
        assert!(tm.assign_to_group(a, Some(gid)));
        assert!(tm.assign_to_group(c, Some(gid)));
        // The group's members should now be adjacent in the workspace order.
        let positions: Vec<usize> = tm
            .iter()
            .enumerate()
            .filter(|(_, ws)| ws.group_id == Some(gid))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[1] - positions[0], 1, "members must be contiguous");
    }

    #[test]
    fn test_group_unread_sums_members() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new());
        let gid = tm.create_group("G", None);
        tm.assign_to_group(a, Some(gid));
        tm.assign_to_group(b, Some(gid));
        tm.workspace_mut(a).unwrap().unread_count = 3;
        tm.workspace_mut(b).unwrap().unread_count = 4;
        assert_eq!(tm.group_unread_count(gid), 7);
    }

    #[test]
    fn test_remove_group_ungroups_members() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let gid = tm.create_group("G", None);
        tm.assign_to_group(a, Some(gid));
        assert!(tm.remove_group(gid));
        assert_eq!(tm.workspace(a).unwrap().group_id, None);
        assert!(tm.group(gid).is_none());
    }

    #[test]
    fn test_collapse_toggle_and_color() {
        let mut tm = TabManager::empty();
        tm.add_workspace(Workspace::new());
        let gid = tm.create_group("G", None);
        assert_eq!(tm.set_group_collapsed(gid, None), Some(true));
        assert_eq!(tm.set_group_collapsed(gid, None), Some(false));
        assert_eq!(tm.set_group_collapsed(gid, Some(true)), Some(true));
        assert!(tm.set_group_color(gid, Some("blue".into())));
        assert_eq!(tm.group(gid).unwrap().color.as_deref(), Some("blue"));
    }

    #[test]
    fn test_reopen_last_closed() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new());
        assert_eq!(tm.len(), 2);
        // Close `a` (index 0).
        let idx = tm.workspace_index(a).unwrap();
        tm.remove(idx);
        assert_eq!(tm.len(), 1);
        assert!(tm.workspace(a).is_none());
        // Reopen restores it with the same id.
        let reopened = tm.reopen_last_closed();
        assert_eq!(reopened, Some(a));
        assert!(tm.workspace(a).is_some());
        assert_eq!(tm.len(), 2);
        // Nothing more to reopen (b is still open).
        let _ = b;
        assert_eq!(tm.reopen_last_closed(), None);
    }

    #[test]
    fn test_move_group_before_relocates_block() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new()); // ungrouped
        let g1 = tm.add_workspace(Workspace::new());
        let g2 = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new()); // ungrouped
        let grp = tm.create_group("G", None);
        tm.assign_to_group(g1, Some(grp));
        tm.assign_to_group(g2, Some(grp));
        // Order now: a, g1, g2, b. Move the group before `a`.
        assert!(tm.move_group_before(grp, Some(a)));
        let order: Vec<_> = tm.iter().map(|w| w.id).collect();
        assert_eq!(order, vec![g1, g2, a, b]);
        // Members stayed contiguous and in order.
        assert_eq!(tm.workspace(g1).unwrap().group_id, Some(grp));
    }

    #[test]
    fn test_assign_to_nonexistent_group_fails() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        assert!(!tm.assign_to_group(a, Some(Uuid::new_v4())));
    }

    #[test]
    fn test_focus_history_back_forward() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new());
        let c = tm.add_workspace(Workspace::new());

        // Simulate the UI chokepoint recording each focus change.
        tm.select_by_id(a);
        tm.record_focus_if_changed();
        tm.select_by_id(b);
        tm.record_focus_if_changed();
        tm.select_by_id(c);
        tm.record_focus_if_changed();

        // Back: c -> b -> a
        assert_eq!(tm.focus_back(), Some(b));
        assert_eq!(tm.focus_back(), Some(a));
        assert_eq!(tm.focus_back(), None); // no earlier entry

        // Forward: a -> b -> c
        assert_eq!(tm.focus_forward(), Some(b));
        assert_eq!(tm.focus_forward(), Some(c));
        assert_eq!(tm.focus_forward(), None);
    }

    #[test]
    fn test_focus_history_unchanged_is_noop() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        tm.select_by_id(a);
        tm.record_focus_if_changed();
        // Repeated recording of the same selection must not grow history.
        tm.record_focus_if_changed();
        tm.record_focus_if_changed();
        assert_eq!(tm.focus_back(), None);
    }

    #[test]
    fn test_focus_history_truncates_forward_branch() {
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new());
        let c = tm.add_workspace(Workspace::new());
        for id in [a, b, c] {
            tm.select_by_id(id);
            tm.record_focus_if_changed();
        }
        // Go back to a, then focus b fresh — the forward branch (c) is dropped.
        assert_eq!(tm.focus_back(), Some(b));
        assert_eq!(tm.focus_back(), Some(a));
        tm.select_by_id(b);
        tm.record_focus_if_changed();
        assert_eq!(tm.focus_forward(), None); // c no longer reachable forward
        assert_eq!(tm.focus_back(), Some(a));
    }

    // ── Background (headless) workspace insertion ────────────────────────
    // A goal-graph node workspace must land in the placement's slot without
    // moving the user's view; every `add_workspace*` above selects instead.

    #[test]
    fn keep_selection_end_placement_leaves_selection_put() {
        use crate::settings::NewWorkspacePlacement as P;
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new());
        tm.select_by_id(a);
        let node = tm.add_workspace_keep_selection(Workspace::new(), P::End);
        assert_eq!(tm.len(), 3);
        assert_eq!(tm.selected().map(|ws| ws.id), Some(a));
        assert_eq!(tm.workspaces[2].id, node);
        assert_eq!(tm.workspaces[1].id, b);
    }

    #[test]
    fn keep_selection_top_placement_shifts_the_selected_index() {
        use crate::settings::NewWorkspacePlacement as P;
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new());
        tm.select_by_id(b);
        assert_eq!(tm.selected_index(), Some(1));
        let node = tm.add_workspace_keep_selection(Workspace::new(), P::Top);
        // Inserted above the selection: the index moves, the selection doesn't.
        assert_eq!(tm.selected_index(), Some(2));
        assert_eq!(tm.selected().map(|ws| ws.id), Some(b));
        assert_eq!(tm.workspaces[0].id, node);
        assert_eq!(tm.workspaces[1].id, a);
    }

    #[test]
    fn keep_selection_after_current_inserts_below_without_selecting() {
        use crate::settings::NewWorkspacePlacement as P;
        let mut tm = TabManager::empty();
        let a = tm.add_workspace(Workspace::new());
        let b = tm.add_workspace(Workspace::new());
        tm.select_by_id(a);
        let node = tm.add_workspace_keep_selection(Workspace::new(), P::AfterCurrent);
        assert_eq!(tm.selected_index(), Some(0));
        assert_eq!(tm.selected().map(|ws| ws.id), Some(a));
        assert_eq!(tm.workspaces[1].id, node);
        assert_eq!(tm.workspaces[2].id, b);
    }

    #[test]
    fn keep_selection_into_empty_manager_selects_nothing() {
        use crate::settings::NewWorkspacePlacement as P;
        let mut tm = TabManager::empty();
        let node = tm.add_workspace_keep_selection(Workspace::new(), P::AfterCurrent);
        assert_eq!(tm.len(), 1);
        assert_eq!(tm.workspaces[0].id, node);
        // Nothing was selected before, so nothing becomes selected — the UI
        // refresh's no-empty-app invariant decides what to show.
        assert_eq!(tm.selected_index(), None);
    }
}
