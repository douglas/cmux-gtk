//! Panel model — represents a terminal or browser panel within a workspace.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// Panel type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelType {
    Terminal,
    Browser,
    Markdown,
}

/// A panel within a workspace pane.
///
/// Panels are the leaf nodes of the layout tree. Each panel is either a
/// terminal (backed by a ghostty surface), a browser (WebKit2GTK), or
/// a markdown viewer (rendered via pulldown-cmark → WebView).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Panel {
    pub id: Uuid,
    pub panel_type: PanelType,
    pub title: Option<String>,
    pub custom_title: Option<String>,
    pub directory: Option<String>,
    pub is_pinned: bool,
    pub is_manually_unread: bool,
    pub git_branch: Option<GitBranch>,
    pub listening_ports: Vec<u16>,
    pub tty_name: Option<String>,
    /// Current URL for browser panels (updated via socket or WebView URI changes).
    pub browser_url: Option<String>,
    /// File path for markdown panels.
    pub markdown_file: Option<String>,
    /// Scrollback text to replay when the terminal surface is first created (session restore).
    pub pending_scrollback: Option<String>,
    /// Browser zoom level to restore (session restore).
    pub pending_zoom: Option<f64>,
}

impl Panel {
    /// Create a new terminal panel.
    pub fn new_terminal() -> Self {
        Self {
            id: Uuid::new_v4(),
            panel_type: PanelType::Terminal,
            title: None,
            custom_title: None,
            directory: None,
            is_pinned: false,
            is_manually_unread: false,
            git_branch: None,
            listening_ports: Vec::new(),
            tty_name: None,
            browser_url: None,
            markdown_file: None,
            pending_scrollback: None,
            pending_zoom: None,
        }
    }

    /// Create a new browser panel.
    pub fn new_browser() -> Self {
        Self {
            id: Uuid::new_v4(),
            panel_type: PanelType::Browser,
            title: None,
            custom_title: None,
            directory: None,
            is_pinned: false,
            is_manually_unread: false,
            git_branch: None,
            listening_ports: Vec::new(),
            tty_name: None,
            browser_url: None,
            markdown_file: None,
            pending_scrollback: None,
            pending_zoom: None,
        }
    }

    /// Create a new markdown panel for viewing a `.md` file.
    pub fn new_markdown(file_path: &str) -> Self {
        let title = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from);
        Self {
            id: Uuid::new_v4(),
            panel_type: PanelType::Markdown,
            title,
            custom_title: None,
            directory: None,
            is_pinned: false,
            is_manually_unread: false,
            git_branch: None,
            listening_ports: Vec::new(),
            tty_name: None,
            browser_url: None,
            markdown_file: Some(file_path.to_string()),
            pending_scrollback: None,
            pending_zoom: None,
        }
    }

    /// Display title: custom title if set, otherwise process title, otherwise fallback by type.
    pub fn display_title(&self) -> &str {
        if let Some(ref t) = self.custom_title {
            return t;
        }
        if let Some(ref t) = self.title {
            return t;
        }
        match self.panel_type {
            PanelType::Terminal => "Terminal",
            PanelType::Browser => "Browser",
            PanelType::Markdown => {
                if let Some(ref f) = self.markdown_file {
                    if let Some(name) = std::path::Path::new(f).file_name() {
                        if let Some(s) = name.to_str() {
                            return s;
                        }
                    }
                }
                "Markdown"
            }
        }
    }
}

/// Git branch info for a panel or workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBranch {
    pub branch: String,
    pub is_dirty: bool,
}

/// Recursive layout tree for workspace pane arrangement.
///
/// A workspace's content area is described by a `LayoutNode`:
/// - `Pane`: a leaf containing one or more panels (tabs within a pane)
/// - `Split`: a binary split (horizontal or vertical) with two children
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LayoutNode {
    #[serde(rename = "pane")]
    Pane {
        /// Panel IDs in tab order within this pane.
        panel_ids: Vec<Uuid>,
        /// Currently selected panel in this pane.
        selected_panel_id: Option<Uuid>,
    },
    #[serde(rename = "split")]
    Split {
        orientation: SplitOrientation,
        /// Normalized divider position (0.0 to 1.0).
        divider_position: f64,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

/// Split orientation for layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitOrientation {
    Horizontal,
    Vertical,
}

/// Direction for directional pane navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl LayoutNode {
    /// Create a simple single-pane layout with one panel.
    pub fn single_pane(panel_id: Uuid) -> Self {
        LayoutNode::Pane {
            panel_ids: vec![panel_id],
            selected_panel_id: Some(panel_id),
        }
    }

    /// Split this node, placing the existing content in the first half
    /// and a new panel in the second half.
    pub fn split(self, orientation: SplitOrientation, new_panel_id: Uuid) -> Self {
        LayoutNode::Split {
            orientation,
            divider_position: 0.5,
            first: Box::new(self),
            second: Box::new(LayoutNode::Pane {
                panel_ids: vec![new_panel_id],
                selected_panel_id: Some(new_panel_id),
            }),
        }
    }

    /// Collect all panel IDs in this layout tree.
    pub fn all_panel_ids(&self) -> Vec<Uuid> {
        match self {
            LayoutNode::Pane { panel_ids, .. } => panel_ids.clone(),
            LayoutNode::Split { first, second, .. } => {
                let mut ids = first.all_panel_ids();
                ids.extend(second.all_panel_ids());
                ids
            }
        }
    }

    /// Find the pane containing the given panel ID and return a mutable reference.
    pub fn find_pane_with_panel(&mut self, panel_id: Uuid) -> Option<&mut LayoutNode> {
        match self {
            LayoutNode::Pane { panel_ids, .. } => {
                if panel_ids.contains(&panel_id) {
                    Some(self)
                } else {
                    None
                }
            }
            LayoutNode::Split { first, second, .. } => first
                .find_pane_with_panel(panel_id)
                .or_else(|| second.find_pane_with_panel(panel_id)),
        }
    }

    /// Find the panel IDs in the pane containing the given panel (immutable).
    pub fn find_pane_with_panel_readonly(&self, panel_id: Uuid) -> Option<Vec<Uuid>> {
        match self {
            LayoutNode::Pane { panel_ids, .. } => {
                if panel_ids.contains(&panel_id) {
                    Some(panel_ids.clone())
                } else {
                    None
                }
            }
            LayoutNode::Split { first, second, .. } => first
                .find_pane_with_panel_readonly(panel_id)
                .or_else(|| second.find_pane_with_panel_readonly(panel_id)),
        }
    }

    /// Select the given panel if it exists in this layout tree.
    pub fn select_panel(&mut self, panel_id: Uuid) -> bool {
        match self {
            LayoutNode::Pane {
                panel_ids,
                selected_panel_id,
            } => {
                if panel_ids.contains(&panel_id) {
                    *selected_panel_id = Some(panel_id);
                    true
                } else {
                    false
                }
            }
            LayoutNode::Split { first, second, .. } => {
                first.select_panel(panel_id) || second.select_panel(panel_id)
            }
        }
    }

    /// Remove a panel from the layout. If a pane becomes empty, the split
    /// is collapsed. Returns true if the panel was found and removed.
    pub fn remove_panel(&mut self, panel_id: Uuid) -> bool {
        match self {
            LayoutNode::Pane {
                panel_ids,
                selected_panel_id,
            } => {
                if let Some(pos) = panel_ids.iter().position(|&id| id == panel_id) {
                    panel_ids.remove(pos);
                    if *selected_panel_id == Some(panel_id) {
                        *selected_panel_id = panel_ids.first().copied();
                    }
                    true
                } else {
                    false
                }
            }
            LayoutNode::Split { first, second, .. } => {
                let removed = first.remove_panel(panel_id) || second.remove_panel(panel_id);
                if removed {
                    // Collapse if either side is now empty
                    if first.is_empty() {
                        *self = *second.clone();
                    } else if second.is_empty() {
                        *self = *first.clone();
                    }
                }
                removed
            }
        }
    }

    /// Update the divider position for the split identified by its child panel sets.
    pub fn set_divider_position_for_split(
        &mut self,
        first_panel_ids: &[Uuid],
        second_panel_ids: &[Uuid],
        divider_position: f64,
    ) -> bool {
        match self {
            LayoutNode::Pane { .. } => false,
            LayoutNode::Split {
                divider_position: current,
                first,
                second,
                ..
            } => {
                let is_target = same_panel_set(first, first_panel_ids)
                    && same_panel_set(second, second_panel_ids);
                if is_target {
                    *current = divider_position.clamp(0.0, 1.0);
                    true
                } else {
                    first.set_divider_position_for_split(
                        first_panel_ids,
                        second_panel_ids,
                        divider_position,
                    ) || second.set_divider_position_for_split(
                        first_panel_ids,
                        second_panel_ids,
                        divider_position,
                    )
                }
            }
        }
    }

    /// Adjust the divider position of the split containing the given panel.
    /// `amount` is a normalized delta (e.g. 0.05 to grow, -0.05 to shrink).
    pub fn resize_panel(&mut self, panel_id: Uuid, amount: f64) -> bool {
        match self {
            LayoutNode::Pane { .. } => false,
            LayoutNode::Split {
                divider_position,
                first,
                second,
                ..
            } => {
                let in_first = first.all_panel_ids().contains(&panel_id);
                let in_second = second.all_panel_ids().contains(&panel_id);
                if in_first || in_second {
                    // Try to resize in children first (deeper splits)
                    if in_first && first.resize_panel(panel_id, amount) {
                        return true;
                    }
                    if in_second && second.resize_panel(panel_id, amount) {
                        return true;
                    }
                    // This is the innermost split containing the panel
                    let delta = if in_first { amount } else { -amount };
                    *divider_position = (*divider_position + delta).clamp(0.05, 0.95);
                    return true;
                }
                false
            }
        }
    }

    /// Return the next panel ID after the given one in layout order,
    /// wrapping around to the first if at the end.
    pub fn next_panel_id(&self, current: Uuid) -> Option<Uuid> {
        let ids = self.all_panel_ids();
        let pos = ids.iter().position(|&id| id == current)?;
        let next = (pos + 1) % ids.len();
        Some(ids[next])
    }

    /// Return the previous panel ID before the given one in layout order,
    /// wrapping around to the last if at the beginning.
    pub fn prev_panel_id(&self, current: Uuid) -> Option<Uuid> {
        let ids = self.all_panel_ids();
        let pos = ids.iter().position(|&id| id == current)?;
        let prev = if pos == 0 { ids.len() - 1 } else { pos - 1 };
        Some(ids[prev])
    }

    /// Swap two panels within the layout tree.
    pub fn swap_panels(&mut self, a: Uuid, b: Uuid) -> bool {
        // Collect the pane node references by scanning and swapping in-place.
        // We need to find both panels and swap them in their respective pane nodes.
        self.swap_panels_recursive(a, b)
    }

    fn swap_panels_recursive(&mut self, a: Uuid, b: Uuid) -> bool {
        // Walk the tree and replace a→b and b→a in panel_ids/selected_panel_id
        match self {
            LayoutNode::Pane {
                panel_ids,
                selected_panel_id,
            } => {
                let mut changed = false;
                for id in panel_ids.iter_mut() {
                    if *id == a {
                        *id = b;
                        changed = true;
                    } else if *id == b {
                        *id = a;
                        changed = true;
                    }
                }
                if let Some(sel) = selected_panel_id {
                    if *sel == a {
                        *sel = b;
                    } else if *sel == b {
                        *sel = a;
                    }
                }
                changed
            }
            LayoutNode::Split { first, second, .. } => {
                let c1 = first.swap_panels_recursive(a, b);
                let c2 = second.swap_panels_recursive(a, b);
                c1 || c2
            }
        }
    }

    /// Serialize the layout tree to JSON with panel metadata.
    pub fn to_json_tree(&self, panels: &HashMap<Uuid, Panel>) -> Value {
        match self {
            LayoutNode::Pane {
                panel_ids,
                selected_panel_id,
            } => {
                let panels_json: Vec<Value> = panel_ids
                    .iter()
                    .filter_map(|id| {
                        panels.get(id).map(|p| {
                            serde_json::json!({
                                "id": id.to_string(),
                                "type": format!("{:?}", p.panel_type).to_lowercase(),
                                "title": p.display_title(),
                                "directory": p.directory,
                            })
                        })
                    })
                    .collect();
                serde_json::json!({
                    "type": "pane",
                    "panels": panels_json,
                    "selected": selected_panel_id.map(|id| id.to_string()),
                })
            }
            LayoutNode::Split {
                orientation,
                divider_position,
                first,
                second,
            } => {
                serde_json::json!({
                    "type": "split",
                    "orientation": format!("{:?}", orientation).to_lowercase(),
                    "divider_position": divider_position,
                    "first": first.to_json_tree(panels),
                    "second": second.to_json_tree(panels),
                })
            }
        }
    }

    /// Find the neighboring panel in the given direction relative to `panel_id`.
    /// Returns None if no neighbor exists in that direction.
    pub fn neighbor(&self, panel_id: Uuid, direction: Direction) -> Option<Uuid> {
        self.neighbor_recursive(panel_id, direction)
    }

    fn neighbor_recursive(&self, panel_id: Uuid, direction: Direction) -> Option<Uuid> {
        match self {
            LayoutNode::Pane { .. } => None,
            LayoutNode::Split {
                orientation,
                first,
                second,
                ..
            } => {
                let first_ids = first.all_panel_ids();
                let second_ids = second.all_panel_ids();
                let in_first = first_ids.contains(&panel_id);
                let in_second = second_ids.contains(&panel_id);

                let matches_axis = match (orientation, direction) {
                    (SplitOrientation::Horizontal, Direction::Left | Direction::Right) => true,
                    (SplitOrientation::Vertical, Direction::Up | Direction::Down) => true,
                    _ => false,
                };

                if matches_axis {
                    let want_second = matches!(direction, Direction::Right | Direction::Down);
                    if in_first && want_second {
                        // Try deeper first, then take first panel in second half
                        first
                            .neighbor_recursive(panel_id, direction)
                            .or_else(|| second.all_panel_ids().into_iter().next())
                    } else if in_second && !want_second {
                        second
                            .neighbor_recursive(panel_id, direction)
                            .or_else(|| first.all_panel_ids().into_iter().last())
                    } else if in_first {
                        first.neighbor_recursive(panel_id, direction)
                    } else {
                        second.neighbor_recursive(panel_id, direction)
                    }
                } else {
                    // Cross-axis: recurse into whichever child has the panel
                    if in_first {
                        first.neighbor_recursive(panel_id, direction)
                    } else if in_second {
                        second.neighbor_recursive(panel_id, direction)
                    } else {
                        None
                    }
                }
            }
        }
    }

    /// Reorder a panel within its pane to a new index.
    /// Returns true if the panel was found and moved.
    pub fn reorder_panel_in_pane(&mut self, panel_id: Uuid, new_index: usize) -> bool {
        if let Some(LayoutNode::Pane { panel_ids, .. }) = self.find_pane_with_panel(panel_id) {
            if let Some(pos) = panel_ids.iter().position(|&id| id == panel_id) {
                panel_ids.remove(pos);
                let clamped = new_index.min(panel_ids.len());
                panel_ids.insert(clamped, panel_id);
                return true;
            }
        }
        false
    }

    /// Add a new panel to the same pane as target_panel_id (tabbed, not split).
    /// The new panel is inserted after the target and selected.
    /// Returns true if the target pane was found and the panel was added.
    pub fn add_panel_to_pane(&mut self, target_panel_id: Uuid, new_panel_id: Uuid) -> bool {
        if let Some(LayoutNode::Pane {
            panel_ids,
            selected_panel_id,
        }) = self.find_pane_with_panel(target_panel_id)
        {
            if let Some(pos) = panel_ids.iter().position(|&id| id == target_panel_id) {
                panel_ids.insert(pos + 1, new_panel_id);
            } else {
                panel_ids.push(new_panel_id);
            }
            *selected_panel_id = Some(new_panel_id);
            return true;
        }
        false
    }

    /// Split the pane containing `target_panel_id`, placing `panel_id` in a
    /// new adjacent split in the given direction. Does NOT remove `panel_id`
    /// from any existing pane — the caller must do that first.
    /// Returns true if the target pane was found and split.
    pub fn split_pane_with_panel(
        &mut self,
        target_panel_id: Uuid,
        panel_id: Uuid,
        orientation: SplitOrientation,
        direction: Direction,
    ) -> bool {
        match self {
            LayoutNode::Pane { panel_ids, .. } => {
                if !panel_ids.contains(&target_panel_id) {
                    return false;
                }
                let new_pane = LayoutNode::Pane {
                    panel_ids: vec![panel_id],
                    selected_panel_id: Some(panel_id),
                };
                let existing = std::mem::replace(
                    self,
                    LayoutNode::Pane {
                        panel_ids: vec![],
                        selected_panel_id: None,
                    },
                );
                // left/up → new panel is first; right/down → new panel is second
                let new_first = matches!(direction, Direction::Left | Direction::Up);
                *self = LayoutNode::Split {
                    orientation,
                    divider_position: 0.5,
                    first: Box::new(if new_first {
                        new_pane.clone()
                    } else {
                        existing.clone()
                    }),
                    second: Box::new(if new_first { existing } else { new_pane }),
                };
                true
            }
            LayoutNode::Split { first, second, .. } => {
                first.split_pane_with_panel(target_panel_id, panel_id, orientation, direction)
                    || second
                        .split_pane_with_panel(target_panel_id, panel_id, orientation, direction)
            }
        }
    }

    /// Recursively set all split divider_positions to 0.5 (equalize).
    /// Returns true if any divider was changed.
    pub fn equalize(&mut self) -> bool {
        match self {
            LayoutNode::Pane { .. } => false,
            LayoutNode::Split {
                divider_position,
                first,
                second,
                ..
            } => {
                let changed = (*divider_position - 0.5).abs() > f64::EPSILON;
                *divider_position = 0.5;
                let c1 = first.equalize();
                let c2 = second.equalize();
                changed || c1 || c2
            }
        }
    }

    /// Next panel within the same pane (tab cycling, wraps around).
    pub fn next_panel_in_pane(&self, current: Uuid) -> Option<Uuid> {
        match self {
            LayoutNode::Pane { panel_ids, .. } => {
                let pos = panel_ids.iter().position(|&id| id == current)?;
                let next = (pos + 1) % panel_ids.len();
                Some(panel_ids[next])
            }
            LayoutNode::Split { first, second, .. } => first
                .next_panel_in_pane(current)
                .or_else(|| second.next_panel_in_pane(current)),
        }
    }

    /// Previous panel within the same pane (tab cycling, wraps around).
    pub fn prev_panel_in_pane(&self, current: Uuid) -> Option<Uuid> {
        match self {
            LayoutNode::Pane { panel_ids, .. } => {
                let pos = panel_ids.iter().position(|&id| id == current)?;
                let prev = if pos == 0 {
                    panel_ids.len() - 1
                } else {
                    pos - 1
                };
                Some(panel_ids[prev])
            }
            LayoutNode::Split { first, second, .. } => first
                .prev_panel_in_pane(current)
                .or_else(|| second.prev_panel_in_pane(current)),
        }
    }

    /// Check if this node contains no panels.
    pub fn is_empty(&self) -> bool {
        match self {
            LayoutNode::Pane { panel_ids, .. } => panel_ids.is_empty(),
            LayoutNode::Split { first, second, .. } => first.is_empty() && second.is_empty(),
        }
    }
}

fn same_panel_set(node: &LayoutNode, expected: &[Uuid]) -> bool {
    let mut actual = node.all_panel_ids();
    let mut expected = expected.to_vec();
    actual.sort_unstable();
    expected.sort_unstable();
    actual == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_pane() {
        let id = Uuid::new_v4();
        let node = LayoutNode::single_pane(id);
        assert_eq!(node.all_panel_ids(), vec![id]);
    }

    #[test]
    fn test_split() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let node = LayoutNode::single_pane(id1).split(SplitOrientation::Horizontal, id2);
        let ids = node.all_panel_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_remove_panel_collapses_split() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let mut node = LayoutNode::single_pane(id1).split(SplitOrientation::Horizontal, id2);
        assert!(node.remove_panel(id2));
        assert_eq!(node.all_panel_ids(), vec![id1]);
        // Should have collapsed back to a single pane
        assert!(matches!(node, LayoutNode::Pane { .. }));
    }

    #[test]
    fn test_set_divider_position_for_split_updates_matching_split() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let mut node = LayoutNode::single_pane(id1).split(SplitOrientation::Horizontal, id2);
        node = node.split(SplitOrientation::Vertical, id3);

        assert!(node.set_divider_position_for_split(&[id1, id2], &[id3], 0.75));

        match node {
            LayoutNode::Split {
                divider_position, ..
            } => assert_eq!(divider_position, 0.75),
            _ => panic!("expected split layout"),
        }
    }

    #[test]
    fn test_set_divider_position_for_split_updates_nested_split() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        let mut node = LayoutNode::Split {
            orientation: SplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(LayoutNode::single_pane(id1).split(SplitOrientation::Vertical, id2)),
            second: Box::new(LayoutNode::single_pane(id3)),
        };

        assert!(node.set_divider_position_for_split(&[id1], &[id2], 0.2));

        match node {
            LayoutNode::Split { first, .. } => match *first {
                LayoutNode::Split {
                    divider_position, ..
                } => assert_eq!(divider_position, 0.2),
                _ => panic!("expected nested split"),
            },
            _ => panic!("expected outer split"),
        }
    }

    #[test]
    fn test_layout_serialization_roundtrip() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let node = LayoutNode::single_pane(id1).split(SplitOrientation::Vertical, id2);
        let json = serde_json::to_string(&node).unwrap();
        let restored: LayoutNode = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.all_panel_ids().len(), 2);
    }

    #[test]
    fn test_reorder_panel_in_pane() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let mut node = LayoutNode::Pane {
            panel_ids: vec![id1, id2, id3],
            selected_panel_id: Some(id1),
        };
        assert!(node.reorder_panel_in_pane(id3, 0));
        if let LayoutNode::Pane { panel_ids, .. } = &node {
            assert_eq!(panel_ids, &[id3, id1, id2]);
        } else {
            panic!("expected pane");
        }
    }

    #[test]
    fn test_add_panel_to_pane() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let mut node = LayoutNode::single_pane(id1);
        assert!(node.add_panel_to_pane(id1, id2));
        if let LayoutNode::Pane {
            panel_ids,
            selected_panel_id,
        } = &node
        {
            assert_eq!(panel_ids, &[id1, id2]);
            assert_eq!(*selected_panel_id, Some(id2));
        } else {
            panic!("expected pane");
        }
    }

    #[test]
    fn test_equalize() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let mut node = LayoutNode::Split {
            orientation: SplitOrientation::Horizontal,
            divider_position: 0.3,
            first: Box::new(LayoutNode::single_pane(id1)),
            second: Box::new(LayoutNode::Split {
                orientation: SplitOrientation::Vertical,
                divider_position: 0.7,
                first: Box::new(LayoutNode::single_pane(id2)),
                second: Box::new(LayoutNode::single_pane(id3)),
            }),
        };
        assert!(node.equalize());
        match &node {
            LayoutNode::Split {
                divider_position,
                second,
                ..
            } => {
                assert_eq!(*divider_position, 0.5);
                if let LayoutNode::Split {
                    divider_position, ..
                } = second.as_ref()
                {
                    assert_eq!(*divider_position, 0.5);
                } else {
                    panic!("expected nested split");
                }
            }
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn test_next_panel_in_pane() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let node = LayoutNode::Pane {
            panel_ids: vec![id1, id2, id3],
            selected_panel_id: Some(id1),
        };
        assert_eq!(node.next_panel_in_pane(id1), Some(id2));
        assert_eq!(node.next_panel_in_pane(id3), Some(id1)); // wraps
    }

    #[test]
    fn test_prev_panel_in_pane() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let node = LayoutNode::Pane {
            panel_ids: vec![id1, id2, id3],
            selected_panel_id: Some(id1),
        };
        assert_eq!(node.prev_panel_in_pane(id1), Some(id3)); // wraps
        assert_eq!(node.prev_panel_in_pane(id2), Some(id1));
    }

    #[test]
    fn test_select_panel_in_split() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let mut node = LayoutNode::single_pane(id1).split(SplitOrientation::Vertical, id2);
        assert!(node.select_panel(id2));

        let mut selected = None;
        if let LayoutNode::Split { second, .. } = &node {
            if let LayoutNode::Pane {
                selected_panel_id, ..
            } = second.as_ref()
            {
                selected = *selected_panel_id;
            }
        }

        assert_eq!(selected, Some(id2));
    }
}
