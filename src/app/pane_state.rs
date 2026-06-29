use std::collections::HashMap;

use crate::editor_group::{EditorGroup, GroupId, GroupNode, GroupRemoveResult};
use crate::pane_tree::{PaneNode, RemoveResult, SplitDir};

use super::pane::PaneEntry;

/// Returned by [`PaneState::close_leaf`] so the caller can perform session cleanup.
#[allow(dead_code)]
pub(super) struct CloseLeafInfo {
    pub removed_pane_ids: Vec<u32>,
    pub tree_removed: bool,
}

/// Returned by [`PaneState::close_pane_in_group`] so the caller can manage focus.
#[allow(dead_code)]
pub(super) struct CloseGroupResult {
    pub removed_pane_id: u32,
    pub group_collapsed: bool,
    pub focus_group_id: Option<GroupId>,
    pub focus_pane_id: Option<u32>,
}

/// Holds pane-related state extracted from `App`.
///
/// Contains the list of panes, the active pane ID, the next pane ID counter,
/// the pane split trees, and the next split ID counter.
pub(super) struct PaneState {
    pub(super) panes: Vec<PaneEntry>,
    pub(super) active_pane_id: Option<u32>,
    pub(super) next_pane_id: u32,
    /// Maps root_pane_id → layout tree for that tab.
    pub(super) pane_trees: HashMap<u32, PaneNode>,
    /// Monotonically-increasing counter for generating unique split node IDs.
    pub(super) next_split_id: u32,

    // ── Editor-group fields (new layout system) ─────────────────
    pub(super) groups: HashMap<GroupId, EditorGroup>,
    pub(super) group_layout: GroupNode,
    pub(super) focused_group_id: GroupId,
    pub(super) next_group_id: u32,
}

impl PaneState {
    pub(super) fn new() -> Self {
        PaneState {
            panes: Vec::new(),
            active_pane_id: None,
            next_pane_id: 0,
            pane_trees: HashMap::new(),
            next_split_id: 1,
            groups: HashMap::new(),
            group_layout: GroupNode::Leaf { group_id: 0 },
            focused_group_id: 0,
            next_group_id: 1,
        }
    }

    pub(super) fn find(&self, id: u32) -> Option<&PaneEntry> {
        self.panes.iter().find(|p| p.id == id)
    }

    pub(super) fn find_mut(&mut self, id: u32) -> Option<&mut PaneEntry> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    #[allow(dead_code)]
    pub(super) fn remove(&mut self, id: u32) -> Option<PaneEntry> {
        let pos = self.panes.iter().position(|p| p.id == id)?;
        Some(self.panes.remove(pos))
    }

    pub(super) fn root_of(&self, pane_id: u32) -> Option<u32> {
        if self.pane_trees.contains_key(&pane_id) {
            return Some(pane_id);
        }
        self.pane_trees
            .iter()
            .find(|(_, tree)| tree.leaf_ids().contains(&pane_id))
            .map(|(&rpid, _)| rpid)
    }

    #[allow(dead_code)]
    pub(super) fn active(&self) -> Option<&PaneEntry> {
        self.active_pane_id.and_then(|id| self.find(id))
    }

    #[allow(dead_code)]
    pub(super) fn active_mut(&mut self) -> Option<&mut PaneEntry> {
        let id = self.active_pane_id?;
        self.find_mut(id)
    }

    /// Remove a leaf pane from its tree, performing all necessary tree surgery.
    ///
    /// Returns `None` if the pane is not found. On success returns a
    /// [`CloseLeafInfo`] describing what was removed so the caller can clean up
    /// sessions and update focus.
    ///
    /// Tree re-keying: `pane_trees` is keyed by the root pane ID (which is also
    /// a leaf in the tree). When that root pane is removed from a multi-leaf
    /// tree, the tree must be re-keyed using `first_leaf_id()` of the surviving
    /// sibling subtree.
    pub(super) fn close_leaf(&mut self, pane_id: u32) -> Option<CloseLeafInfo> {
        let root_id = self.root_of(pane_id)?;
        let tree = self.pane_trees.get(&root_id)?;
        let leaf_ids = tree.leaf_ids();

        if leaf_ids.len() <= 1 {
            // Only leaf in the tree — remove the whole tree.
            self.pane_trees.remove(&root_id);
            self.panes.retain(|p| p.id != pane_id);
            return Some(CloseLeafInfo {
                removed_pane_ids: vec![pane_id],
                tree_removed: true,
            });
        }

        let tree = self.pane_trees.get_mut(&root_id).unwrap();
        let result = tree.remove_pane(pane_id);
        match result {
            RemoveResult::CollapseToSibling(replacement) => {
                if root_id == pane_id {
                    // The root pane itself was closed; re-key the tree.
                    let new_root_id = replacement.first_leaf_id();
                    self.pane_trees.remove(&root_id);
                    self.pane_trees.insert(new_root_id, replacement);
                } else {
                    self.pane_trees.insert(root_id, replacement);
                }
            }
            RemoveResult::Done => {}
            RemoveResult::IsTarget | RemoveResult::NotFound => {
                return None;
            }
        }

        self.panes.retain(|p| p.id != pane_id);
        Some(CloseLeafInfo {
            removed_pane_ids: vec![pane_id],
            tree_removed: false,
        })
    }

    /// Compute ordered indices into `self.panes` for all leaf panes visible
    /// in the tab bar.
    ///
    /// For each root pane (key in `pane_trees`), emit all leaf pane indices
    /// in tree-traversal order. Roots are visited in the order they appear
    /// in `self.panes`, so tab ordering is stable.
    pub(super) fn panes_referencing_session(&self, session_id: u32) -> usize {
        self.panes
            .iter()
            .filter(|p| matches!(p.content, super::pane::PaneContent::Terminal(sid) if sid == session_id))
            .count()
    }

    pub(super) fn find_file_editor(
        &self,
        path: &std::path::Path,
        active_group: Option<u64>,
    ) -> Option<u32> {
        use super::pane::PaneContent;
        self.panes
            .iter()
            .find(|p| {
                matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path && ed.workspace_id == active_group)
            })
            .map(|p| p.id)
    }

    pub(super) fn visible_leaf_indices(&self) -> Vec<usize> {
        let mut indices = Vec::new();
        for pane in &self.panes {
            let root_id = pane.id;
            let Some(tree) = self.pane_trees.get(&root_id) else {
                continue;
            };
            for leaf_id in tree.leaf_ids() {
                if let Some(leaf_idx) = self.panes.iter().position(|p| p.id == leaf_id) {
                    indices.push(leaf_idx);
                }
            }
        }
        indices
    }

    // ── Editor-group methods ────────────────────────────────────

    /// Find which group contains a pane.
    pub(super) fn group_of(&self, pane_id: u32) -> Option<GroupId> {
        self.groups
            .values()
            .find(|g| g.contains(pane_id))
            .map(|g| g.id)
    }

    /// Get the focused group.
    #[allow(dead_code)]
    pub(super) fn focused_group(&self) -> Option<&EditorGroup> {
        self.groups.get(&self.focused_group_id)
    }

    /// Get the focused group mutably.
    #[allow(dead_code)]
    pub(super) fn focused_group_mut(&mut self) -> Option<&mut EditorGroup> {
        self.groups.get_mut(&self.focused_group_id)
    }

    /// Create a new group with an initial pane and register it. Returns the new
    /// group ID. Does NOT modify `group_layout` — caller must do that.
    pub(super) fn create_group(&mut self, initial_pane_id: u32) -> GroupId {
        let id = self.next_group_id;
        self.next_group_id += 1;
        self.groups
            .insert(id, EditorGroup::new(id, initial_pane_id));
        id
    }

    /// Add a pane to an existing group at a specific position.
    #[allow(dead_code)]
    pub(super) fn add_pane_to_group(&mut self, group_id: GroupId, pane_id: u32, at: Option<usize>) {
        if let Some(group) = self.groups.get_mut(&group_id) {
            group.insert_pane(pane_id, at);
        }
    }

    /// Split the focused group: create a new group with `new_pane_id`, add it
    /// to the layout tree adjacent to `focused_group_id` in direction `dir`.
    /// Returns the new group's ID.
    #[allow(dead_code)]
    pub(super) fn split_focused_group(&mut self, new_pane_id: u32, dir: SplitDir) -> GroupId {
        let new_group_id = self.create_group(new_pane_id);
        let split_id = self.next_split_id;
        self.next_split_id += 1;
        self.group_layout
            .split_group(self.focused_group_id, new_group_id, split_id, dir);
        self.focused_group_id = new_group_id;
        self.active_pane_id = Some(new_pane_id);
        new_group_id
    }

    /// Remove a pane from its group. If the group becomes empty, collapse it
    /// from the layout. Returns info about what happened for the caller to
    /// manage focus.
    #[allow(dead_code)]
    pub(super) fn close_pane_in_group(&mut self, pane_id: u32) -> Option<CloseGroupResult> {
        let group_id = self.group_of(pane_id)?;
        let group = self.groups.get_mut(&group_id)?;
        let became_empty = group.remove_pane(pane_id);

        self.panes.retain(|p| p.id != pane_id);

        if became_empty {
            self.groups.remove(&group_id);
            let sibling = self.group_layout.sibling_of(group_id);
            match self.group_layout.remove_group(group_id) {
                GroupRemoveResult::CollapseToSibling(replacement) => {
                    self.group_layout = replacement;
                }
                GroupRemoveResult::IsTarget => {
                    self.focused_group_id = 0;
                }
                _ => {}
            }
            let focus_gid = sibling.or_else(|| {
                let first = self.group_layout.first_group_id();
                if self.groups.contains_key(&first) {
                    Some(first)
                } else {
                    self.groups.keys().next().copied()
                }
            });
            let focus_pid = focus_gid.and_then(|gid| self.groups.get(&gid)?.active_pane_id);
            if let Some(gid) = focus_gid {
                self.focused_group_id = gid;
            }
            self.active_pane_id = focus_pid;
            Some(CloseGroupResult {
                removed_pane_id: pane_id,
                group_collapsed: true,
                focus_group_id: focus_gid,
                focus_pane_id: focus_pid,
            })
        } else {
            let focus_pid = self.groups.get(&group_id)?.active_pane_id;
            if self.focused_group_id == group_id {
                self.active_pane_id = focus_pid;
            }
            Some(CloseGroupResult {
                removed_pane_id: pane_id,
                group_collapsed: false,
                focus_group_id: Some(group_id),
                focus_pane_id: focus_pid,
            })
        }
    }

    /// Reassign duplicate split_ids in the layout tree so every Split node
    /// has a unique ID, then update `next_split_id` to be above the max.
    pub(super) fn dedup_split_ids(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.group_layout
            .dedup_split_ids_inner(&mut seen, &mut self.next_split_id);
    }

    /// Synchronise editor groups with the actual panes list.
    ///
    /// Removes stale pane IDs (ones no longer in `self.panes`) from every group,
    /// collapses empty groups from the layout tree, and fixes `focused_group_id`.
    /// Call this after any bulk pane removal that bypasses `close_pane_in_group`.
    pub(super) fn sync_groups_with_panes(&mut self) {
        let live_ids: std::collections::HashSet<u32> = self.panes.iter().map(|p| p.id).collect();

        let mut empty_group_ids: Vec<GroupId> = Vec::new();
        for group in self.groups.values_mut() {
            let before = group.pane_ids.len();
            group.pane_ids.retain(|pid| live_ids.contains(pid));
            if group.pane_ids.is_empty() {
                empty_group_ids.push(group.id);
            } else if before != group.pane_ids.len() {
                if let Some(apid) = group.active_pane_id {
                    if !group.pane_ids.contains(&apid) {
                        group.active_pane_id = group.pane_ids.first().copied();
                    }
                }
            }
        }

        for gid in &empty_group_ids {
            self.groups.remove(gid);
            if let GroupRemoveResult::CollapseToSibling(replacement) =
                self.group_layout.remove_group(*gid)
            {
                self.group_layout = replacement;
            }
        }

        if !self.groups.contains_key(&self.focused_group_id) {
            self.focused_group_id = self.groups.keys().next().copied().unwrap_or(0);
        }

        if let Some(apid) = self.active_pane_id {
            if !live_ids.contains(&apid) {
                let focused = self.groups.get(&self.focused_group_id);
                self.active_pane_id = focused
                    .and_then(|g| g.active_pane_id)
                    .or_else(|| self.panes.last().map(|p| p.id));
            }
        }
    }

    /// Move a pane from its current group to a target group.
    #[allow(dead_code)]
    pub(super) fn move_pane_to_group(
        &mut self,
        pane_id: u32,
        target_group_id: GroupId,
        at: Option<usize>,
    ) {
        let source_group_id = match self.group_of(pane_id) {
            Some(gid) => gid,
            None => return,
        };
        if source_group_id == target_group_id {
            if let Some(group) = self.groups.get_mut(&target_group_id) {
                if let Some(idx) = at {
                    group.reorder(pane_id, idx);
                }
            }
            return;
        }
        // Remove from source
        let source_became_empty = self
            .groups
            .get_mut(&source_group_id)
            .map(|s| s.remove_pane(pane_id))
            .unwrap_or(false);
        if source_became_empty {
            self.groups.remove(&source_group_id);
            if let GroupRemoveResult::CollapseToSibling(replacement) =
                self.group_layout.remove_group(source_group_id)
            {
                self.group_layout = replacement;
            }
        }
        // Insert into target
        if let Some(target) = self.groups.get_mut(&target_group_id) {
            target.insert_pane(pane_id, at);
            target.activate(pane_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::pane::PaneContent;
    use crate::pane_tree::{PaneNode, SplitDir};

    fn make_pane(id: u32) -> PaneEntry {
        PaneEntry {
            id,
            content: PaneContent::Terminal(id),
            manual_width: None,
            last_size: (80, 24),
            labels: vec![],
            last_active_at: 0,
        }
    }

    #[test]
    fn test_new_state_is_empty() {
        let state = PaneState::new();
        assert!(state.panes.is_empty());
        assert_eq!(state.active_pane_id, None);
        assert_eq!(state.next_pane_id, 0);
        assert!(state.pane_trees.is_empty());
        assert_eq!(state.next_split_id, 1);
    }

    #[test]
    fn test_find_existing() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(5));
        let found = state.find(5);
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, 5);
    }

    #[test]
    fn test_find_nonexistent() {
        let state = PaneState::new();
        assert!(state.find(42).is_none());
    }

    #[test]
    fn test_find_mut_modifies() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        assert_eq!(state.find(1).unwrap().manual_width, None);

        state.find_mut(1).unwrap().manual_width = Some(200.0);
        assert_eq!(state.find(1).unwrap().manual_width, Some(200.0));
    }

    #[test]
    fn test_remove_existing() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        state.remove(1);
        assert!(state.find(1).is_none());
        assert!(state.find(2).is_some());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut state = PaneState::new();
        assert!(state.remove(99).is_none());
    }

    #[test]
    fn test_remove_returns_entry() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(7));
        let removed = state.remove(7);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, 7);
    }

    #[test]
    fn test_active_when_none() {
        let state = PaneState::new();
        assert!(state.active().is_none());
    }

    #[test]
    fn test_active_when_set() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(3));
        state.panes.push(make_pane(4));
        state.active_pane_id = Some(4);
        let active = state.active().unwrap();
        assert_eq!(active.id, 4);
    }

    #[test]
    fn test_active_mut() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(10));
        state.active_pane_id = Some(10);
        state.active_mut().unwrap().last_size = (120, 40);
        assert_eq!(state.find(10).unwrap().last_size, (120, 40));
    }

    #[test]
    fn test_root_of_direct() {
        let mut state = PaneState::new();
        state.pane_trees.insert(
            1,
            PaneNode::Leaf {
                pane_id: 1,
                last_size: (0, 0),
            },
        );
        assert_eq!(state.root_of(1), Some(1));
    }

    #[test]
    fn test_root_of_child() {
        let mut state = PaneState::new();
        let tree = PaneNode::Split {
            split_id: 1,
            dir: SplitDir::Horizontal,
            ratio: 0.5,
            a: Box::new(PaneNode::Leaf {
                pane_id: 2,
                last_size: (0, 0),
            }),
            b: Box::new(PaneNode::Leaf {
                pane_id: 3,
                last_size: (0, 0),
            }),
        };
        state.pane_trees.insert(1, tree);
        assert_eq!(state.root_of(2), Some(1));
        assert_eq!(state.root_of(3), Some(1));
    }

    #[test]
    fn visible_leaf_indices_no_splits() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        state.pane_trees.insert(
            1,
            PaneNode::Leaf {
                pane_id: 1,
                last_size: (0, 0),
            },
        );
        state.pane_trees.insert(
            2,
            PaneNode::Leaf {
                pane_id: 2,
                last_size: (0, 0),
            },
        );
        let result = state.visible_leaf_indices();
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn visible_leaf_indices_with_split() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        state.panes.push(make_pane(3));
        let mut tree1 = PaneNode::Leaf {
            pane_id: 1,
            last_size: (0, 0),
        };
        tree1.split_pane(1, 3, 10, SplitDir::Horizontal);
        state.pane_trees.insert(1, tree1);
        state.pane_trees.insert(
            2,
            PaneNode::Leaf {
                pane_id: 2,
                last_size: (0, 0),
            },
        );
        let result = state.visible_leaf_indices();
        assert_eq!(result, vec![0, 2, 1]);
    }

    #[test]
    fn visible_leaf_indices_shows_all_regardless_of_workspace() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        state.pane_trees.insert(
            1,
            PaneNode::Leaf {
                pane_id: 1,
                last_size: (0, 0),
            },
        );
        state.pane_trees.insert(
            2,
            PaneNode::Leaf {
                pane_id: 2,
                last_size: (0, 0),
            },
        );
        let result = state.visible_leaf_indices();
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn visible_leaf_indices_split_shows_all_leaves() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        state.panes.push(make_pane(3));
        let mut tree1 = PaneNode::Leaf {
            pane_id: 1,
            last_size: (0, 0),
        };
        tree1.split_pane(1, 3, 10, SplitDir::Horizontal);
        state.pane_trees.insert(1, tree1);
        state.pane_trees.insert(
            2,
            PaneNode::Leaf {
                pane_id: 2,
                last_size: (0, 0),
            },
        );
        let result = state.visible_leaf_indices();
        assert_eq!(result, vec![0, 2, 1]);
    }

    #[test]
    fn close_leaf_only_pane_in_tree() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.pane_trees.insert(
            1,
            PaneNode::Leaf {
                pane_id: 1,
                last_size: (0, 0),
            },
        );
        let result = state.close_leaf(1);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.removed_pane_ids, vec![1]);
        assert!(info.tree_removed);
        assert!(state.panes.is_empty());
        assert!(state.pane_trees.is_empty());
    }

    #[test]
    fn close_leaf_sibling_survives() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        let mut tree = PaneNode::Leaf {
            pane_id: 1,
            last_size: (0, 0),
        };
        tree.split_pane(1, 2, 10, SplitDir::Horizontal);
        state.pane_trees.insert(1, tree);
        let result = state.close_leaf(2);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.removed_pane_ids, vec![2]);
        assert!(!info.tree_removed);
        assert!(state.find(2).is_none());
        assert!(state.find(1).is_some());
        assert_eq!(state.pane_trees[&1].leaf_ids(), vec![1]);
    }

    #[test]
    fn close_leaf_root_pane_rekeys_tree() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        let mut tree = PaneNode::Leaf {
            pane_id: 1,
            last_size: (0, 0),
        };
        tree.split_pane(1, 2, 10, SplitDir::Horizontal);
        state.pane_trees.insert(1, tree);
        let result = state.close_leaf(1);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.removed_pane_ids, vec![1]);
        assert!(!info.tree_removed);
        assert!(state.find(1).is_none());
        assert!(state.find(2).is_some());
        assert!(!state.pane_trees.contains_key(&1));
        assert!(state.pane_trees.contains_key(&2));
        assert_eq!(state.pane_trees[&2].leaf_ids(), vec![2]);
    }

    #[test]
    fn close_leaf_root_pane_complex_tree_rekeys() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        state.panes.push(make_pane(3));
        let mut tree = PaneNode::Leaf {
            pane_id: 1,
            last_size: (0, 0),
        };
        tree.split_pane(1, 2, 10, SplitDir::Horizontal);
        tree.split_pane(2, 3, 11, SplitDir::Vertical);
        state.pane_trees.insert(1, tree);
        let result = state.close_leaf(1);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.removed_pane_ids, vec![1]);
        assert!(!info.tree_removed);
        assert!(!state.pane_trees.contains_key(&1));
        assert!(state.pane_trees.contains_key(&2));
        let remaining_leaves = state.pane_trees[&2].leaf_ids();
        assert_eq!(remaining_leaves, vec![2, 3]);
    }

    #[test]
    fn panes_referencing_session_none() {
        let state = PaneState::new();
        assert_eq!(state.panes_referencing_session(42), 0);
    }

    #[test]
    fn panes_referencing_session_single() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1)); // PaneContent::Terminal(1)
        assert_eq!(state.panes_referencing_session(1), 1);
        assert_eq!(state.panes_referencing_session(99), 0);
    }

    #[test]
    fn panes_referencing_session_multiple() {
        let mut state = PaneState::new();
        state.panes.push(PaneEntry {
            id: 1,
            content: PaneContent::Terminal(5),
            manual_width: None,
            last_size: (80, 24),
            labels: vec![],
            last_active_at: 0,
        });
        state.panes.push(PaneEntry {
            id: 2,
            content: PaneContent::Terminal(5),
            manual_width: None,
            last_size: (80, 24),
            labels: vec![],
            last_active_at: 0,
        });
        state.panes.push(PaneEntry {
            id: 3,
            content: PaneContent::Terminal(7),
            manual_width: None,
            last_size: (80, 24),
            labels: vec![],
            last_active_at: 0,
        });
        assert_eq!(state.panes_referencing_session(5), 2);
        assert_eq!(state.panes_referencing_session(7), 1);
    }

    #[test]
    fn panes_referencing_session_after_removal() {
        let mut state = PaneState::new();
        state.panes.push(PaneEntry {
            id: 1,
            content: PaneContent::Terminal(5),
            manual_width: None,
            last_size: (80, 24),
            labels: vec![],
            last_active_at: 0,
        });
        state.panes.push(PaneEntry {
            id: 2,
            content: PaneContent::Terminal(5),
            manual_width: None,
            last_size: (80, 24),
            labels: vec![],
            last_active_at: 0,
        });
        assert_eq!(state.panes_referencing_session(5), 2);
        state.panes.retain(|p| p.id != 1);
        assert_eq!(state.panes_referencing_session(5), 1);
        state.panes.retain(|p| p.id != 2);
        assert_eq!(state.panes_referencing_session(5), 0);
    }

    fn make_file_editor_pane(id: u32, path: &str, workspace_id: Option<u64>) -> PaneEntry {
        PaneEntry {
            id,
            content: PaneContent::FileEditor(super::super::pane::FileEditorState {
                path: std::path::PathBuf::from(path),
                content: String::new(),
                dirty: false,
                save_error: false,
                workspace_id,
                show_preview: false,
                stale: false,
                loading: false,
            }),
            manual_width: None,
            last_size: (0, 0),
            labels: vec![],
            last_active_at: 0,
        }
    }

    #[test]
    fn find_file_editor_same_workspace_matches() {
        let mut state = PaneState::new();
        state
            .panes
            .push(make_file_editor_pane(1, "/project/foo.rs", Some(42)));
        let found = state.find_file_editor(std::path::Path::new("/project/foo.rs"), Some(42));
        assert_eq!(found, Some(1));
    }

    #[test]
    fn find_file_editor_different_workspace_no_match() {
        let mut state = PaneState::new();
        state
            .panes
            .push(make_file_editor_pane(1, "/project/foo.rs", Some(42)));
        let found = state.find_file_editor(std::path::Path::new("/project/foo.rs"), Some(99));
        assert_eq!(found, None);
    }

    #[test]
    fn find_file_editor_none_workspace_no_match_when_active_is_some() {
        let mut state = PaneState::new();
        state
            .panes
            .push(make_file_editor_pane(1, "/project/foo.rs", None));
        let found = state.find_file_editor(std::path::Path::new("/project/foo.rs"), Some(42));
        assert_eq!(found, None);
    }

    #[test]
    fn find_file_editor_some_workspace_no_match_when_active_is_none() {
        let mut state = PaneState::new();
        state
            .panes
            .push(make_file_editor_pane(1, "/project/foo.rs", Some(42)));
        let found = state.find_file_editor(std::path::Path::new("/project/foo.rs"), None);
        assert_eq!(found, None);
    }

    #[test]
    fn find_file_editor_both_none_matches() {
        let mut state = PaneState::new();
        state
            .panes
            .push(make_file_editor_pane(1, "/project/foo.rs", None));
        let found = state.find_file_editor(std::path::Path::new("/project/foo.rs"), None);
        assert_eq!(found, Some(1));
    }

    #[test]
    fn find_file_editor_different_path_no_match() {
        let mut state = PaneState::new();
        state
            .panes
            .push(make_file_editor_pane(1, "/project/foo.rs", Some(42)));
        let found = state.find_file_editor(std::path::Path::new("/project/bar.rs"), Some(42));
        assert_eq!(found, None);
    }

    #[test]
    fn find_file_editor_picks_correct_among_multiple() {
        let mut state = PaneState::new();
        state
            .panes
            .push(make_file_editor_pane(1, "/project/foo.rs", Some(10)));
        state
            .panes
            .push(make_file_editor_pane(2, "/project/foo.rs", Some(20)));
        state
            .panes
            .push(make_file_editor_pane(3, "/project/bar.rs", Some(20)));

        assert_eq!(
            state.find_file_editor(std::path::Path::new("/project/foo.rs"), Some(10)),
            Some(1)
        );
        assert_eq!(
            state.find_file_editor(std::path::Path::new("/project/foo.rs"), Some(20)),
            Some(2)
        );
        assert_eq!(
            state.find_file_editor(std::path::Path::new("/project/bar.rs"), Some(20)),
            Some(3)
        );
        assert_eq!(
            state.find_file_editor(std::path::Path::new("/project/bar.rs"), Some(10)),
            None
        );
    }

    #[test]
    fn find_file_editor_ignores_terminal_panes() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state
            .panes
            .push(make_file_editor_pane(2, "/project/foo.rs", Some(42)));
        let found = state.find_file_editor(std::path::Path::new("/project/foo.rs"), Some(42));
        assert_eq!(found, Some(2));
    }

    #[test]
    fn find_file_editor_empty_panes_returns_none() {
        let state = PaneState::new();
        let found = state.find_file_editor(std::path::Path::new("/any/path.rs"), Some(1));
        assert_eq!(found, None);
    }

    // ── Editor-group tests ──────────────────────────────────────

    use crate::editor_group::GroupNode;

    fn make_grouped_state() -> PaneState {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        state.panes.push(make_pane(2));
        state.panes.push(make_pane(3));
        state.next_pane_id = 4;
        // Create two groups: g1 has panes [1,2], g2 has pane [3]
        let g1 = state.create_group(1);
        state.groups.get_mut(&g1).unwrap().insert_pane(2, None);
        let g2 = state.create_group(3);
        // Set up layout: Split(g1, g2)
        state.group_layout = GroupNode::Leaf { group_id: g1 };
        let split_id = state.next_split_id;
        state.next_split_id += 1;
        state
            .group_layout
            .split_group(g1, g2, split_id, SplitDir::Horizontal);
        state.focused_group_id = g1;
        state.active_pane_id = Some(1);
        state
    }

    #[test]
    fn group_of_finds_correct_group() {
        let state = make_grouped_state();
        // Pane 1 and 2 are in group 1, pane 3 is in group 2
        assert_eq!(state.group_of(1), Some(1));
        assert_eq!(state.group_of(2), Some(1));
        assert_eq!(state.group_of(3), Some(2));
    }

    #[test]
    fn group_of_returns_none_for_unknown() {
        let state = make_grouped_state();
        assert_eq!(state.group_of(99), None);
    }

    #[test]
    fn focused_group_returns_group() {
        let state = make_grouped_state();
        let fg = state.focused_group().unwrap();
        assert_eq!(fg.id, 1);
        assert!(fg.contains(1));
        assert!(fg.contains(2));
    }

    #[test]
    fn create_group_increments_id() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(10));
        state.panes.push(make_pane(20));
        let g1 = state.create_group(10);
        let g2 = state.create_group(20);
        assert_eq!(g1, 1);
        assert_eq!(g2, 2);
        assert_eq!(state.next_group_id, 3);
        assert!(state.groups.contains_key(&g1));
        assert!(state.groups.contains_key(&g2));
    }

    #[test]
    fn add_pane_to_group_works() {
        let mut state = make_grouped_state();
        state.panes.push(make_pane(4));
        state.add_pane_to_group(1, 4, Some(1));
        let g = state.groups.get(&1).unwrap();
        assert_eq!(g.pane_ids, vec![1, 4, 2]);
    }

    #[test]
    fn split_focused_group_creates_new_group_and_updates_layout() {
        let mut state = PaneState::new();
        state.panes.push(make_pane(1));
        let g1 = state.create_group(1);
        state.group_layout = GroupNode::Leaf { group_id: g1 };
        state.focused_group_id = g1;
        state.active_pane_id = Some(1);

        state.panes.push(make_pane(2));
        let g2 = state.split_focused_group(2, SplitDir::Horizontal);

        // New group created
        assert!(state.groups.contains_key(&g2));
        assert!(state.groups.get(&g2).unwrap().contains(2));
        // Focus moved to new group
        assert_eq!(state.focused_group_id, g2);
        assert_eq!(state.active_pane_id, Some(2));
        // Layout is now a split
        assert_eq!(state.group_layout.group_ids(), vec![g1, g2]);
    }

    #[test]
    fn close_pane_in_group_removes_pane() {
        let mut state = make_grouped_state();
        // Group 1 has panes [1, 2]. Close pane 2 — group should survive.
        let result = state.close_pane_in_group(2).unwrap();
        assert_eq!(result.removed_pane_id, 2);
        assert!(!result.group_collapsed);
        assert_eq!(result.focus_group_id, Some(1));
        assert!(state.find(2).is_none());
        assert!(state.groups.get(&1).unwrap().contains(1));
        assert!(!state.groups.get(&1).unwrap().contains(2));
    }

    #[test]
    fn close_pane_in_group_collapses_empty_group() {
        let mut state = make_grouped_state();
        // Group 2 has only pane 3. Closing it should collapse the group.
        let result = state.close_pane_in_group(3).unwrap();
        assert_eq!(result.removed_pane_id, 3);
        assert!(result.group_collapsed);
        assert!(!state.groups.contains_key(&2));
        // Layout should collapse to just group 1
        assert_eq!(state.group_layout.group_ids(), vec![1]);
    }

    #[test]
    fn close_pane_in_group_updates_focus_on_collapse() {
        let mut state = make_grouped_state();
        // Focus group 2, then close its only pane
        state.focused_group_id = 2;
        state.active_pane_id = Some(3);
        let result = state.close_pane_in_group(3).unwrap();
        assert!(result.group_collapsed);
        // Focus should move to the sibling (group 1)
        assert_eq!(result.focus_group_id, Some(1));
        assert_eq!(state.focused_group_id, 1);
        // active_pane_id should point to group 1's active pane
        assert_eq!(result.focus_pane_id, Some(1));
    }

    #[test]
    fn close_last_pane_in_last_group_resets_state() {
        let mut state = PaneState::new();
        state.next_pane_id = 2;
        state.panes.push(make_pane(1));
        let gid = state.create_group(1);
        state.group_layout = GroupNode::Leaf { group_id: gid };
        state.focused_group_id = gid;
        state.active_pane_id = Some(1);

        let result = state.close_pane_in_group(1).unwrap();
        assert!(result.group_collapsed);
        assert!(state.groups.is_empty());
        assert_eq!(state.focused_group_id, 0);
        assert_eq!(state.active_pane_id, None);
    }

    #[test]
    fn move_pane_to_group_moves_between_groups() {
        let mut state = make_grouped_state();
        // Move pane 2 from group 1 to group 2
        state.move_pane_to_group(2, 2, None);
        assert!(!state.groups.get(&1).unwrap().contains(2));
        assert!(state.groups.get(&2).unwrap().contains(2));
        assert_eq!(state.groups.get(&2).unwrap().pane_ids, vec![3, 2]);
    }

    #[test]
    fn move_pane_to_group_same_group_reorders() {
        let mut state = make_grouped_state();
        // Reorder pane 1 to index 1 within group 1
        state.move_pane_to_group(1, 1, Some(1));
        assert_eq!(state.groups.get(&1).unwrap().pane_ids, vec![2, 1]);
    }

    #[test]
    fn move_pane_to_group_collapses_empty_source() {
        let mut state = make_grouped_state();
        // Move pane 3 (sole occupant of group 2) to group 1
        state.move_pane_to_group(3, 1, None);
        // Group 2 should be removed
        assert!(!state.groups.contains_key(&2));
        // Group 1 should have all three panes
        assert_eq!(state.groups.get(&1).unwrap().pane_ids, vec![1, 2, 3]);
        // Layout should collapse to just group 1
        assert_eq!(state.group_layout.group_ids(), vec![1]);
    }

    // ── sync_groups_with_panes tests ───────────────────────

    #[test]
    fn sync_removes_stale_pane_ids() {
        let mut state = make_grouped_state();
        // Directly remove pane 2 from panes (simulating close-all bypass)
        state.panes.retain(|p| p.id != 2);
        state.sync_groups_with_panes();
        // Group 1 should only have pane 1 now
        assert_eq!(state.groups.get(&1).unwrap().pane_ids, vec![1]);
        // Group 2 still has pane 3
        assert_eq!(state.groups.get(&2).unwrap().pane_ids, vec![3]);
    }

    #[test]
    fn sync_collapses_empty_groups() {
        let mut state = make_grouped_state();
        // Remove pane 3 (only pane in group 2)
        state.panes.retain(|p| p.id != 3);
        state.sync_groups_with_panes();
        assert!(!state.groups.contains_key(&2));
        assert_eq!(state.group_layout.group_ids(), vec![1]);
    }

    #[test]
    fn sync_fixes_focused_group_id() {
        let mut state = make_grouped_state();
        state.focused_group_id = 2;
        state.active_pane_id = Some(3);
        state.panes.retain(|p| p.id != 3);
        state.sync_groups_with_panes();
        // Focus should move to a surviving group
        assert!(state.groups.contains_key(&state.focused_group_id));
    }

    #[test]
    fn sync_fixes_active_pane_id() {
        let mut state = make_grouped_state();
        state.active_pane_id = Some(3);
        state.panes.retain(|p| p.id != 3);
        state.sync_groups_with_panes();
        assert_ne!(state.active_pane_id, Some(3));
        assert!(state.active_pane_id.is_some());
    }

    #[test]
    fn sync_updates_active_pane_in_group() {
        let mut state = make_grouped_state();
        // Group 1 has panes [1, 2], active is 1
        state.groups.get_mut(&1).unwrap().activate(2);
        // Remove pane 2
        state.panes.retain(|p| p.id != 2);
        state.sync_groups_with_panes();
        // Group 1's active should be pane 1
        assert_eq!(state.groups.get(&1).unwrap().active_pane_id, Some(1));
    }

    #[test]
    fn sync_noop_when_consistent() {
        let state_before = make_grouped_state();
        let mut state = make_grouped_state();
        state.sync_groups_with_panes();
        assert_eq!(state.groups.len(), state_before.groups.len());
        assert_eq!(
            state.group_layout.group_ids(),
            state_before.group_layout.group_ids()
        );
    }

    #[test]
    fn sync_handles_all_panes_removed() {
        let mut state = make_grouped_state();
        state.panes.clear();
        state.sync_groups_with_panes();
        assert!(state.groups.is_empty());
    }
}
