#![allow(dead_code)]
use crate::pane_tree::SplitDir;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDir {
    Left,
    Right,
    Up,
    Down,
}

pub type GroupId = u32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditorGroup {
    pub id: GroupId,
    pub pane_ids: Vec<u32>,
    pub active_pane_id: Option<u32>,
}

impl EditorGroup {
    pub fn new(id: GroupId, initial_pane_id: u32) -> Self {
        Self {
            id,
            pane_ids: vec![initial_pane_id],
            active_pane_id: Some(initial_pane_id),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pane_ids.is_empty()
    }

    pub fn contains(&self, pane_id: u32) -> bool {
        self.pane_ids.contains(&pane_id)
    }

    /// Removes pane from the group. Updates active to a neighbor if the removed
    /// pane was active (prefer next, fall back to last). Returns true if the
    /// group is now empty.
    pub fn remove_pane(&mut self, pane_id: u32) -> bool {
        let Some(idx) = self.pane_ids.iter().position(|&id| id == pane_id) else {
            return self.pane_ids.is_empty();
        };
        self.pane_ids.remove(idx);
        if self.active_pane_id == Some(pane_id) {
            self.active_pane_id = if self.pane_ids.is_empty() {
                None
            } else if idx < self.pane_ids.len() {
                Some(self.pane_ids[idx])
            } else {
                self.pane_ids.last().copied()
            };
        }
        self.pane_ids.is_empty()
    }

    pub fn insert_pane(&mut self, pane_id: u32, at: Option<usize>) {
        if self.pane_ids.contains(&pane_id) {
            return;
        }
        match at {
            Some(idx) => {
                let clamped = idx.min(self.pane_ids.len());
                self.pane_ids.insert(clamped, pane_id);
            }
            None => self.pane_ids.push(pane_id),
        }
    }

    pub fn activate(&mut self, pane_id: u32) {
        if self.pane_ids.contains(&pane_id) {
            self.active_pane_id = Some(pane_id);
        }
    }

    pub fn reorder(&mut self, pane_id: u32, to_index: usize) {
        let Some(from) = self.pane_ids.iter().position(|&id| id == pane_id) else {
            return;
        };
        self.pane_ids.remove(from);
        let clamped = to_index.min(self.pane_ids.len());
        self.pane_ids.insert(clamped, pane_id);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GroupNode {
    Leaf {
        group_id: GroupId,
    },
    Split {
        split_id: u32,
        dir: SplitDir,
        ratio: f32,
        a: Box<GroupNode>,
        b: Box<GroupNode>,
    },
}

#[derive(Debug)]
pub enum GroupRemoveResult {
    IsTarget,
    CollapseToSibling(GroupNode),
    Done,
    NotFound,
}

impl GroupNode {
    pub fn group_ids(&self) -> Vec<GroupId> {
        match self {
            GroupNode::Leaf { group_id } => vec![*group_id],
            GroupNode::Split { a, b, .. } => {
                let mut ids = a.group_ids();
                ids.extend(b.group_ids());
                ids
            }
        }
    }

    pub fn first_group_id(&self) -> GroupId {
        match self {
            GroupNode::Leaf { group_id } => *group_id,
            GroupNode::Split { a, .. } => a.first_group_id(),
        }
    }

    pub fn last_group_id(&self) -> GroupId {
        match self {
            GroupNode::Leaf { group_id } => *group_id,
            GroupNode::Split { b, .. } => b.last_group_id(),
        }
    }

    pub fn contains_group(&self, group_id: GroupId) -> bool {
        match self {
            GroupNode::Leaf { group_id: id } => *id == group_id,
            GroupNode::Split { a, b, .. } => {
                a.contains_group(group_id) || b.contains_group(group_id)
            }
        }
    }

    /// Replace the leaf with `target_group_id` with a Split having the target as
    /// `a` and a new leaf (`new_group_id`) as `b`. Returns true if found.
    pub fn split_group(
        &mut self,
        target_group_id: GroupId,
        new_group_id: GroupId,
        split_id: u32,
        dir: SplitDir,
    ) -> bool {
        match self {
            GroupNode::Leaf { group_id } if *group_id == target_group_id => {
                let old_leaf = GroupNode::Leaf {
                    group_id: *group_id,
                };
                let new_leaf = GroupNode::Leaf {
                    group_id: new_group_id,
                };
                *self = GroupNode::Split {
                    split_id,
                    dir,
                    ratio: 0.5,
                    a: Box::new(old_leaf),
                    b: Box::new(new_leaf),
                };
                true
            }
            GroupNode::Split { a, b, .. } => {
                a.split_group(target_group_id, new_group_id, split_id, dir)
                    || b.split_group(target_group_id, new_group_id, split_id, dir)
            }
            _ => false,
        }
    }

    pub fn remove_group(&mut self, target_group_id: GroupId) -> GroupRemoveResult {
        match self {
            GroupNode::Leaf { group_id } if *group_id == target_group_id => {
                GroupRemoveResult::IsTarget
            }
            GroupNode::Split { a, b, .. } => match a.remove_group(target_group_id) {
                GroupRemoveResult::IsTarget => GroupRemoveResult::CollapseToSibling((**b).clone()),
                GroupRemoveResult::CollapseToSibling(replacement) => {
                    **a = replacement;
                    GroupRemoveResult::Done
                }
                GroupRemoveResult::Done => GroupRemoveResult::Done,
                GroupRemoveResult::NotFound => match b.remove_group(target_group_id) {
                    GroupRemoveResult::IsTarget => {
                        GroupRemoveResult::CollapseToSibling((**a).clone())
                    }
                    GroupRemoveResult::CollapseToSibling(replacement) => {
                        **b = replacement;
                        GroupRemoveResult::Done
                    }
                    other => other,
                },
            },
            _ => GroupRemoveResult::NotFound,
        }
    }

    /// Walk the tree and reassign any split_id already in `seen`.
    /// Advances `next_id` past every assigned value.
    pub fn dedup_split_ids_inner(
        &mut self,
        seen: &mut std::collections::HashSet<u32>,
        next_id: &mut u32,
    ) {
        match self {
            GroupNode::Split { split_id, a, b, .. } => {
                if !seen.insert(*split_id) {
                    *split_id = *next_id;
                    *next_id += 1;
                    seen.insert(*split_id);
                }
                if *split_id >= *next_id {
                    *next_id = *split_id + 1;
                }
                a.dedup_split_ids_inner(seen, next_id);
                b.dedup_split_ids_inner(seen, next_id);
            }
            GroupNode::Leaf { .. } => {}
        }
    }

    pub fn find_split_ratio_mut(&mut self, split_id: u32) -> Option<&mut f32> {
        match self {
            GroupNode::Split {
                split_id: sid,
                ratio,
                a,
                b,
                ..
            } => {
                if *sid == split_id {
                    return Some(ratio);
                }
                a.find_split_ratio_mut(split_id)
                    .or_else(|| b.find_split_ratio_mut(split_id))
            }
            _ => None,
        }
    }

    /// Returns the first group ID from the sibling branch of the split
    /// containing `group_id`.
    pub fn sibling_of(&self, group_id: GroupId) -> Option<GroupId> {
        match self {
            GroupNode::Leaf { .. } => None,
            GroupNode::Split { a, b, .. } => {
                if a.contains_group(group_id) {
                    if let GroupNode::Leaf { group_id: gid } = a.as_ref() {
                        if *gid == group_id {
                            return Some(b.first_group_id());
                        }
                    }
                    return a.sibling_of(group_id);
                }
                if b.contains_group(group_id) {
                    if let GroupNode::Leaf { group_id: gid } = b.as_ref() {
                        if *gid == group_id {
                            return Some(a.first_group_id());
                        }
                    }
                    return b.sibling_of(group_id);
                }
                None
            }
        }
    }

    /// Spatially-aware group navigation. Finds the adjacent group in the given
    /// visual direction by walking the split tree:
    /// - Left/Right navigate through `SplitDir::Horizontal` splits
    /// - Up/Down navigate through `SplitDir::Vertical` splits
    pub fn spatial_neighbor(&self, from: GroupId, nav: NavigationDir) -> Option<GroupId> {
        match self {
            GroupNode::Leaf { .. } => None,
            GroupNode::Split { dir, a, b, .. } => {
                let axis_match = matches!(
                    (&nav, dir),
                    (
                        NavigationDir::Left | NavigationDir::Right,
                        SplitDir::Horizontal
                    ) | (NavigationDir::Up | NavigationDir::Down, SplitDir::Vertical)
                );
                let forward = matches!(nav, NavigationDir::Right | NavigationDir::Down);

                if a.contains_group(from) {
                    if let Some(gid) = a.spatial_neighbor(from, nav) {
                        return Some(gid);
                    }
                    if axis_match && forward {
                        return Some(b.first_group_id());
                    }
                    None
                } else if b.contains_group(from) {
                    if let Some(gid) = b.spatial_neighbor(from, nav) {
                        return Some(gid);
                    }
                    if axis_match && !forward {
                        return Some(a.last_group_id());
                    }
                    None
                } else {
                    None
                }
            }
        }
    }

    /// Linear neighbor navigation (depth-first order). Kept for backward
    /// compatibility; prefer `spatial_neighbor` for directional navigation.
    pub fn neighbor(&self, from_group_id: GroupId, dir: SplitDir) -> Option<GroupId> {
        let ids = self.group_ids();
        let idx = ids.iter().position(|&id| id == from_group_id)?;
        match dir {
            SplitDir::Horizontal => {
                if idx + 1 < ids.len() {
                    Some(ids[idx + 1])
                } else {
                    None
                }
            }
            SplitDir::Vertical => {
                if idx > 0 {
                    Some(ids[idx - 1])
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── EditorGroup tests ───────────────────────────────────────

    #[test]
    fn new_creates_single_pane_group() {
        let g = EditorGroup::new(1, 10);
        assert_eq!(g.id, 1);
        assert_eq!(g.pane_ids, vec![10]);
        assert_eq!(g.active_pane_id, Some(10));
    }

    #[test]
    fn contains_returns_true_for_member() {
        let g = EditorGroup::new(1, 10);
        assert!(g.contains(10));
    }

    #[test]
    fn contains_returns_false_for_non_member() {
        let g = EditorGroup::new(1, 10);
        assert!(!g.contains(99));
    }

    #[test]
    fn is_empty_false_when_has_panes() {
        let g = EditorGroup::new(1, 10);
        assert!(!g.is_empty());
    }

    #[test]
    fn remove_pane_middle_updates_active() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.insert_pane(30, None);
        g.activate(20);
        g.remove_pane(20);
        // Next pane after index 1 (where 20 was) is now 30 at index 1
        assert_eq!(g.active_pane_id, Some(30));
        assert_eq!(g.pane_ids, vec![10, 30]);
    }

    #[test]
    fn remove_pane_last_element_activates_prev() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.insert_pane(30, None);
        g.activate(30);
        g.remove_pane(30);
        assert_eq!(g.active_pane_id, Some(20));
    }

    #[test]
    fn remove_pane_non_active_keeps_active() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.activate(10);
        g.remove_pane(20);
        assert_eq!(g.active_pane_id, Some(10));
        assert_eq!(g.pane_ids, vec![10]);
    }

    #[test]
    fn remove_pane_only_pane_returns_true() {
        let mut g = EditorGroup::new(1, 10);
        assert!(g.remove_pane(10));
        assert!(g.is_empty());
        assert_eq!(g.active_pane_id, None);
    }

    #[test]
    fn remove_pane_nonexistent_does_nothing() {
        let mut g = EditorGroup::new(1, 10);
        let empty = g.remove_pane(99);
        assert!(!empty);
        assert_eq!(g.pane_ids, vec![10]);
        assert_eq!(g.active_pane_id, Some(10));
    }

    #[test]
    fn insert_pane_at_position() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.insert_pane(15, Some(1));
        assert_eq!(g.pane_ids, vec![10, 15, 20]);
    }

    #[test]
    fn insert_pane_append() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        assert_eq!(g.pane_ids, vec![10, 20]);
    }

    #[test]
    fn activate_existing_pane() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.activate(20);
        assert_eq!(g.active_pane_id, Some(20));
    }

    #[test]
    fn activate_nonexistent_pane_no_change() {
        let mut g = EditorGroup::new(1, 10);
        g.activate(99);
        assert_eq!(g.active_pane_id, Some(10));
    }

    #[test]
    fn reorder_moves_pane() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.insert_pane(30, None);
        g.reorder(10, 2);
        assert_eq!(g.pane_ids, vec![20, 30, 10]);
    }

    #[test]
    fn reorder_to_same_position_no_change() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.insert_pane(30, None);
        g.reorder(20, 1);
        assert_eq!(g.pane_ids, vec![10, 20, 30]);
    }

    // ─── GroupNode tests ─────────────────────────────────────────

    fn leaf(id: GroupId) -> GroupNode {
        GroupNode::Leaf { group_id: id }
    }

    #[test]
    fn group_ids_single_leaf() {
        assert_eq!(leaf(1).group_ids(), vec![1]);
    }

    #[test]
    fn group_ids_split_order() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        assert_eq!(root.group_ids(), vec![1, 2]);
    }

    #[test]
    fn first_group_id_returns_leftmost() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        assert_eq!(root.first_group_id(), 1);
    }

    #[test]
    fn contains_group_true_and_false() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        assert!(root.contains_group(1));
        assert!(root.contains_group(2));
        assert!(!root.contains_group(99));
    }

    #[test]
    fn split_group_creates_split() {
        let mut root = leaf(1);
        assert!(root.split_group(1, 2, 10, SplitDir::Horizontal));
        assert_eq!(root.group_ids(), vec![1, 2]);
        if let GroupNode::Split { dir, ratio, .. } = &root {
            assert_eq!(*dir, SplitDir::Horizontal);
            assert!((ratio - 0.5).abs() < f32::EPSILON);
        } else {
            panic!("expected Split");
        }
    }

    #[test]
    fn split_group_unknown_returns_false() {
        let mut root = leaf(1);
        assert!(!root.split_group(99, 2, 10, SplitDir::Horizontal));
        assert_eq!(root.group_ids(), vec![1]);
    }

    #[test]
    fn remove_group_left_collapses() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        let result = root.remove_group(1);
        assert!(matches!(result, GroupRemoveResult::CollapseToSibling(_)));
        if let GroupRemoveResult::CollapseToSibling(node) = result {
            assert_eq!(node.group_ids(), vec![2]);
        }
    }

    #[test]
    fn remove_group_right_collapses() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        let result = root.remove_group(2);
        assert!(matches!(result, GroupRemoveResult::CollapseToSibling(_)));
        if let GroupRemoveResult::CollapseToSibling(node) = result {
            assert_eq!(node.group_ids(), vec![1]);
        }
    }

    #[test]
    fn remove_group_only_group_is_target() {
        let mut root = leaf(1);
        assert!(matches!(root.remove_group(1), GroupRemoveResult::IsTarget));
    }

    #[test]
    fn remove_group_unknown_not_found() {
        let mut root = leaf(1);
        assert!(matches!(root.remove_group(99), GroupRemoveResult::NotFound));
    }

    #[test]
    fn find_split_ratio_mut_works() {
        let mut root = leaf(1);
        root.split_group(1, 2, 42, SplitDir::Horizontal);
        let ratio = root.find_split_ratio_mut(42).unwrap();
        *ratio = 0.3;
        if let GroupNode::Split { ratio, .. } = &root {
            assert!((*ratio - 0.3).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn find_split_ratio_mut_unknown_none() {
        let mut root = leaf(1);
        root.split_group(1, 2, 42, SplitDir::Horizontal);
        assert!(root.find_split_ratio_mut(99).is_none());
    }

    #[test]
    fn sibling_of_in_split() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        assert_eq!(root.sibling_of(1), Some(2));
        assert_eq!(root.sibling_of(2), Some(1));
    }

    #[test]
    fn sibling_of_not_found() {
        let root = leaf(1);
        assert_eq!(root.sibling_of(99), None);
    }

    #[test]
    fn neighbor_navigation() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        root.split_group(2, 3, 11, SplitDir::Horizontal);
        // Order: [1, 2, 3]
        assert_eq!(root.neighbor(1, SplitDir::Horizontal), Some(2));
        assert_eq!(root.neighbor(2, SplitDir::Horizontal), Some(3));
        assert_eq!(root.neighbor(3, SplitDir::Horizontal), None);
        assert_eq!(root.neighbor(3, SplitDir::Vertical), Some(2));
        assert_eq!(root.neighbor(2, SplitDir::Vertical), Some(1));
        assert_eq!(root.neighbor(1, SplitDir::Vertical), None);
    }

    #[test]
    fn remove_group_nested_collapses_correctly() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        root.split_group(2, 3, 11, SplitDir::Vertical);
        // Tree: Split(1, Split(2, 3))
        // Remove 2 — inner split collapses to 3, outer becomes Split(1, 3)
        let result = root.remove_group(2);
        assert!(matches!(result, GroupRemoveResult::Done));
        assert_eq!(root.group_ids(), vec![1, 3]);
    }

    #[test]
    fn insert_pane_at_clamped_position() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, Some(100));
        assert_eq!(g.pane_ids, vec![10, 20]);
    }

    #[test]
    fn insert_pane_duplicate_ignored() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.insert_pane(10, None);
        g.insert_pane(20, Some(0));
        assert_eq!(g.pane_ids, vec![10, 20]);
    }

    #[test]
    fn reorder_nonexistent_pane_no_change() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.reorder(99, 0);
        assert_eq!(g.pane_ids, vec![10, 20]);
    }

    #[test]
    fn reorder_clamps_to_end() {
        let mut g = EditorGroup::new(1, 10);
        g.insert_pane(20, None);
        g.insert_pane(30, None);
        g.reorder(10, 100);
        assert_eq!(g.pane_ids, vec![20, 30, 10]);
    }

    #[test]
    fn neighbor_single_leaf_returns_none() {
        let root = leaf(1);
        assert_eq!(root.neighbor(1, SplitDir::Horizontal), None);
        assert_eq!(root.neighbor(1, SplitDir::Vertical), None);
    }

    #[test]
    fn neighbor_unknown_group_returns_none() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        assert_eq!(root.neighbor(99, SplitDir::Horizontal), None);
    }

    #[test]
    fn sibling_of_nested_finds_immediate_sibling() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        root.split_group(1, 3, 11, SplitDir::Vertical);
        // Tree: Split(Split(1, 3), 2)
        // Sibling of 1 is 3 (immediate split partner)
        assert_eq!(root.sibling_of(1), Some(3));
        // Sibling of 3 is 1
        assert_eq!(root.sibling_of(3), Some(1));
    }

    // ─── last_group_id tests ─────────────────────────────────

    #[test]
    fn last_group_id_single() {
        assert_eq!(leaf(1).last_group_id(), 1);
    }

    #[test]
    fn last_group_id_split() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        assert_eq!(root.last_group_id(), 2);
    }

    #[test]
    fn last_group_id_nested() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        root.split_group(2, 3, 11, SplitDir::Vertical);
        // Tree: Split(1, Split(2, 3)) → last is 3
        assert_eq!(root.last_group_id(), 3);
    }

    // ─── spatial_neighbor tests ──────────────────────────────

    #[test]
    fn spatial_single_leaf_returns_none() {
        let root = leaf(1);
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Left), None);
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Right), None);
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Up), None);
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Down), None);
    }

    #[test]
    fn spatial_horizontal_split_left_right() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        // Layout: [1 | 2]
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Right), Some(2));
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Left), Some(1));
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Left), None);
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Right), None);
    }

    #[test]
    fn spatial_horizontal_split_ignores_up_down() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Up), None);
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Down), None);
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Up), None);
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Down), None);
    }

    #[test]
    fn spatial_vertical_split_up_down() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Vertical);
        // Layout: [1 / 2]
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Down), Some(2));
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Up), Some(1));
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Up), None);
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Down), None);
    }

    #[test]
    fn spatial_vertical_split_ignores_left_right() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Vertical);
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Left), None);
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Right), None);
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Left), None);
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Right), None);
    }

    #[test]
    fn spatial_mixed_layout_navigates_correctly() {
        // Layout:
        //   Split(Horizontal)
        //  /        \
        // 1    Split(Vertical)
        //      /        \
        //     2          3
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        root.split_group(2, 3, 11, SplitDir::Vertical);

        // Right from 1 → first in right subtree (2)
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Right), Some(2));
        // Left from 2 → last in left subtree (1)
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Left), Some(1));
        // Left from 3 → last in left subtree (1)
        assert_eq!(root.spatial_neighbor(3, NavigationDir::Left), Some(1));
        // Down from 2 → 3 (vertical split)
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Down), Some(3));
        // Up from 3 → 2 (vertical split)
        assert_eq!(root.spatial_neighbor(3, NavigationDir::Up), Some(2));
        // Up from 1 → None (no vertical split above)
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Up), None);
        // Down from 1 → None (no vertical split below)
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Down), None);
    }

    #[test]
    fn spatial_three_horizontal() {
        // Layout: [1 | 2 | 3]
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        root.split_group(2, 3, 11, SplitDir::Horizontal);

        assert_eq!(root.spatial_neighbor(1, NavigationDir::Right), Some(2));
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Right), Some(3));
        assert_eq!(root.spatial_neighbor(3, NavigationDir::Right), None);
        assert_eq!(root.spatial_neighbor(3, NavigationDir::Left), Some(2));
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Left), Some(1));
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Left), None);
    }

    #[test]
    fn spatial_unknown_group_returns_none() {
        let mut root = leaf(1);
        root.split_group(1, 2, 10, SplitDir::Horizontal);
        assert_eq!(root.spatial_neighbor(99, NavigationDir::Right), None);
    }

    #[test]
    fn spatial_complex_grid() {
        // Layout:
        //   Split(Vertical)
        //  /          \
        // Split(H)   Split(H)
        // /   \      /   \
        // 1    2    3     4
        let mut root = leaf(1);
        root.split_group(1, 3, 10, SplitDir::Vertical);
        root.split_group(1, 2, 11, SplitDir::Horizontal);
        root.split_group(3, 4, 12, SplitDir::Horizontal);

        // Horizontal navigation within top row
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Right), Some(2));
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Left), Some(1));
        // Horizontal navigation within bottom row
        assert_eq!(root.spatial_neighbor(3, NavigationDir::Right), Some(4));
        assert_eq!(root.spatial_neighbor(4, NavigationDir::Left), Some(3));
        // Vertical navigation from top to bottom
        assert_eq!(root.spatial_neighbor(1, NavigationDir::Down), Some(3));
        assert_eq!(root.spatial_neighbor(2, NavigationDir::Down), Some(3));
        // Vertical navigation from bottom to top
        assert_eq!(root.spatial_neighbor(3, NavigationDir::Up), Some(2));
        assert_eq!(root.spatial_neighbor(4, NavigationDir::Up), Some(2));
    }

    #[test]
    fn dedup_split_ids_no_duplicates() {
        let mut root = leaf(1);
        root.split_group(1, 2, 1, SplitDir::Horizontal);
        root.split_group(2, 3, 2, SplitDir::Vertical);
        let mut next_id = 3u32;
        let mut seen = std::collections::HashSet::new();
        root.dedup_split_ids_inner(&mut seen, &mut next_id);
        assert_eq!(next_id, 3);
    }

    #[test]
    fn dedup_split_ids_fixes_duplicates() {
        let mut root = GroupNode::Split {
            split_id: 1,
            dir: SplitDir::Horizontal,
            ratio: 0.5,
            a: Box::new(GroupNode::Split {
                split_id: 1,
                dir: SplitDir::Vertical,
                ratio: 0.5,
                a: Box::new(leaf(1)),
                b: Box::new(leaf(2)),
            }),
            b: Box::new(GroupNode::Split {
                split_id: 1,
                dir: SplitDir::Vertical,
                ratio: 0.5,
                a: Box::new(leaf(3)),
                b: Box::new(leaf(4)),
            }),
        };
        let mut next_id = 1u32;
        let mut seen = std::collections::HashSet::new();
        root.dedup_split_ids_inner(&mut seen, &mut next_id);
        let ids = collect_split_ids(&root);
        assert_eq!(ids.len(), 3);
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3);
        assert!(next_id > *ids.iter().max().unwrap());
    }

    fn collect_split_ids(node: &GroupNode) -> Vec<u32> {
        match node {
            GroupNode::Leaf { .. } => vec![],
            GroupNode::Split { split_id, a, b, .. } => {
                let mut ids = vec![*split_id];
                ids.extend(collect_split_ids(a));
                ids.extend(collect_split_ids(b));
                ids
            }
        }
    }
}
