//! TabManager — manages the collection of workspaces.

use uuid::Uuid;

use super::workspace::Workspace;

/// Manages all workspaces and tracks the currently selected one.
///
/// This is the top-level model for the sidebar workspace list.
#[derive(Debug)]
pub struct TabManager {
    workspaces: Vec<Workspace>,
    selected_index: Option<usize>,
}

impl TabManager {
    /// Create a new TabManager with a single default workspace.
    pub fn new() -> Self {
        let ws = Workspace::new();
        Self {
            workspaces: vec![ws],
            selected_index: Some(0),
        }
    }

    /// Create an empty TabManager (for restoring from session).
    pub fn empty() -> Self {
        Self {
            workspaces: Vec::new(),
            selected_index: None,
        }
    }

    /// Number of workspaces.
    pub fn len(&self) -> usize {
        self.workspaces.len()
    }

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

    /// Add a new workspace after the current one.
    pub fn add_workspace_after_current(&mut self, workspace: Workspace) -> Uuid {
        let id = workspace.id;
        let insert_at = self.selected_index.map(|i| i + 1).unwrap_or(0);
        self.workspaces.insert(insert_at, workspace);
        self.selected_index = Some(insert_at);
        id
    }

    /// Remove a workspace by index. Returns the removed workspace.
    pub fn remove(&mut self, index: usize) -> Option<Workspace> {
        if index >= self.workspaces.len() {
            return None;
        }
        let ws = self.workspaces.remove(index);

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
                self.selected_index =
                    Some(self.workspaces.len().saturating_sub(1));
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
}
