use std::collections::HashMap;

use crate::pane_tree::{PaneNode, RemoveResult};

use super::pane::PaneEntry;

/// Returned by [`PaneState::close_leaf`] so the caller can perform session cleanup.
#[allow(dead_code)]
pub(super) struct CloseLeafInfo {
    pub removed_pane_ids: Vec<u32>,
    pub tree_removed: bool,
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
}

impl PaneState {
    pub(super) fn new() -> Self {
        PaneState {
            panes: Vec::new(),
            active_pane_id: None,
            next_pane_id: 0,
            pane_trees: HashMap::new(),
            next_split_id: 1,
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

    /// Compute ordered indices into `self.panes` for all leaf panes that should
    /// be visible in the tab bar given the active workspace group.
    ///
    /// For each root pane (key in `pane_trees`) whose group matches
    /// `active_group`, emit all leaf pane indices in tree-traversal order.
    /// Roots are visited in the order they appear in `self.panes`, so tab
    /// ordering is stable.
    pub(super) fn panes_referencing_session(&self, session_id: u32) -> usize {
        self.panes
            .iter()
            .filter(|p| matches!(p.content, super::pane::PaneContent::Terminal(sid) if sid == session_id))
            .count()
    }

    pub(super) fn visible_leaf_indices(
        &self,
        pane_groups: &[Option<u64>],
        active_group: Option<u64>,
    ) -> Vec<usize> {
        let mut indices = Vec::new();
        for (root_idx, pane) in self.panes.iter().enumerate() {
            let root_id = pane.id;
            let Some(tree) = self.pane_trees.get(&root_id) else {
                continue;
            };
            if pane_groups.get(root_idx).copied().flatten() != active_group {
                continue;
            }
            for leaf_id in tree.leaf_ids() {
                if let Some(leaf_idx) = self.panes.iter().position(|p| p.id == leaf_id) {
                    indices.push(leaf_idx);
                }
            }
        }
        indices
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
        let groups = vec![None, None];
        let result = state.visible_leaf_indices(&groups, None);
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
        let groups = vec![None, None, None];
        let result = state.visible_leaf_indices(&groups, None);
        assert_eq!(result, vec![0, 2, 1]);
    }

    #[test]
    fn visible_leaf_indices_filters_by_group() {
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
        let groups = vec![Some(100), Some(200)];
        let result = state.visible_leaf_indices(&groups, Some(100));
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn visible_leaf_indices_split_uses_root_group() {
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
        let groups = vec![Some(100), Some(200), Some(200)];
        let result = state.visible_leaf_indices(&groups, Some(100));
        assert_eq!(result, vec![0, 2]);
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
        });
        state.panes.push(PaneEntry {
            id: 2,
            content: PaneContent::Terminal(5),
            manual_width: None,
            last_size: (80, 24),
        });
        state.panes.push(PaneEntry {
            id: 3,
            content: PaneContent::Terminal(7),
            manual_width: None,
            last_size: (80, 24),
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
        });
        state.panes.push(PaneEntry {
            id: 2,
            content: PaneContent::Terminal(5),
            manual_width: None,
            last_size: (80, 24),
        });
        assert_eq!(state.panes_referencing_session(5), 2);
        state.panes.retain(|p| p.id != 1);
        assert_eq!(state.panes_referencing_session(5), 1);
        state.panes.retain(|p| p.id != 2);
        assert_eq!(state.panes_referencing_session(5), 0);
    }
}
