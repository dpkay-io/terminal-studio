#![allow(dead_code)]
use crate::theme;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SplitDir {
    /// Side-by-side: left | right
    Horizontal,
    /// Stacked: top / bottom
    Vertical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PaneNode {
    Leaf {
        pane_id: u32,
        last_size: (u16, u16),
    },
    Split {
        split_id: u32,
        dir: SplitDir,
        /// Fraction of available space given to `a`.  Clamped to [0.1, 0.9].
        ratio: f32,
        /// Left or top child.
        a: Box<PaneNode>,
        /// Right or bottom child.
        b: Box<PaneNode>,
    },
}

impl PaneNode {
    /// Return all leaf pane IDs in traversal order.
    pub fn leaf_ids(&self) -> Vec<u32> {
        match self {
            PaneNode::Leaf { pane_id, .. } => vec![*pane_id],
            PaneNode::Split { a, b, .. } => {
                let mut ids = a.leaf_ids();
                ids.extend(b.leaf_ids());
                ids
            }
        }
    }

    /// Update the recorded last_size for a specific leaf.
    pub fn update_size(&mut self, pane_id: u32, size: (u16, u16)) {
        match self {
            PaneNode::Leaf {
                pane_id: id,
                last_size,
            } if *id == pane_id => {
                *last_size = size;
            }
            PaneNode::Split { a, b, .. } => {
                a.update_size(pane_id, size);
                b.update_size(pane_id, size);
            }
            _ => {}
        }
    }

    /// Split `target_id` in `dir`, inserting `new_pane_id` as the second half.
    /// Returns `true` if the leaf was found and replaced.
    pub fn split_pane(
        &mut self,
        target_id: u32,
        new_pane_id: u32,
        split_id: u32,
        dir: SplitDir,
    ) -> bool {
        match self {
            PaneNode::Leaf { pane_id, last_size } if *pane_id == target_id => {
                let size = *last_size;
                let old_leaf = PaneNode::Leaf {
                    pane_id: *pane_id,
                    last_size: size,
                };
                let new_leaf = PaneNode::Leaf {
                    pane_id: new_pane_id,
                    last_size: size,
                };
                *self = PaneNode::Split {
                    split_id,
                    dir,
                    ratio: 0.5,
                    a: Box::new(old_leaf),
                    b: Box::new(new_leaf),
                };
                true
            }
            PaneNode::Split { a, b, .. } => {
                a.split_pane(target_id, new_pane_id, split_id, dir)
                    || b.split_pane(target_id, new_pane_id, split_id, dir)
            }
            _ => false,
        }
    }

    /// Split `target_id` in `dir`, inserting an existing subtree as the second half.
    /// Returns `true` if the leaf was found and replaced.
    pub fn split_pane_with_node(
        &mut self,
        target_id: u32,
        subtree: PaneNode,
        split_id: u32,
        dir: SplitDir,
    ) -> bool {
        match self {
            PaneNode::Leaf { pane_id, last_size } if *pane_id == target_id => {
                let old_leaf = PaneNode::Leaf {
                    pane_id: *pane_id,
                    last_size: *last_size,
                };
                *self = PaneNode::Split {
                    split_id,
                    dir,
                    ratio: 0.5,
                    a: Box::new(old_leaf),
                    b: Box::new(subtree),
                };
                true
            }
            PaneNode::Split { a, b, .. } => {
                if a.split_pane_with_node(target_id, subtree.clone(), split_id, dir) {
                    true
                } else {
                    b.split_pane_with_node(target_id, subtree, split_id, dir)
                }
            }
            _ => false,
        }
    }

    /// Remove a leaf from the tree, collapsing the parent split to the sibling.
    pub fn remove_pane(&mut self, target_id: u32) -> RemoveResult {
        match self {
            PaneNode::Leaf { pane_id, .. } if *pane_id == target_id => RemoveResult::IsTarget,
            PaneNode::Split { a, b, .. } => match a.remove_pane(target_id) {
                RemoveResult::IsTarget => RemoveResult::CollapseToSibling((**b).clone()),
                RemoveResult::CollapseToSibling(replacement) => {
                    **a = replacement;
                    RemoveResult::Done
                }
                RemoveResult::Done => RemoveResult::Done,
                RemoveResult::NotFound => match b.remove_pane(target_id) {
                    RemoveResult::IsTarget => RemoveResult::CollapseToSibling((**a).clone()),
                    RemoveResult::CollapseToSibling(replacement) => {
                        **b = replacement;
                        RemoveResult::Done
                    }
                    other => other,
                },
            },
            _ => RemoveResult::NotFound,
        }
    }

    /// Remap all pane_ids in the tree using the given mapping.
    /// IDs not present in the map are left unchanged.
    pub fn remap_ids(&mut self, id_map: &std::collections::HashMap<u32, u32>) {
        match self {
            PaneNode::Leaf { pane_id, .. } => {
                if let Some(&new_id) = id_map.get(pane_id) {
                    *pane_id = new_id;
                }
            }
            PaneNode::Split { a, b, .. } => {
                a.remap_ids(id_map);
                b.remap_ids(id_map);
            }
        }
    }

    /// Find a leaf by pane_id and return a mutable reference to its `ratio` parent.
    /// Used for drag-to-resize: finds the split node whose divider is being dragged.
    pub fn find_split_ratio_mut(&mut self, split_id: u32) -> Option<&mut f32> {
        match self {
            PaneNode::Split {
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
}

#[derive(Debug)]
pub enum RemoveResult {
    /// This node is the target — caller should replace self with sibling.
    IsTarget,
    /// A child was removed; caller should replace the split with `replacement`.
    CollapseToSibling(PaneNode),
    /// Removal handled deeper in the tree.
    Done,
    /// Target not found in this subtree.
    NotFound,
}

/// Split a rect into two according to direction and ratio.
/// Returns `(rect_a, divider_rect, rect_b)`.
pub fn split_rect(
    rect: egui::Rect,
    dir: SplitDir,
    ratio: f32,
) -> (egui::Rect, egui::Rect, egui::Rect) {
    let half = theme::DIVIDER_W / 2.0;
    match dir {
        SplitDir::Horizontal => {
            let x = (rect.min.x + rect.width() * ratio).round();
            let a = egui::Rect::from_min_max(rect.min, egui::pos2(x - half, rect.max.y));
            let div = egui::Rect::from_min_max(
                egui::pos2(x - half, rect.min.y),
                egui::pos2(x + half, rect.max.y),
            );
            let b = egui::Rect::from_min_max(egui::pos2(x + half, rect.min.y), rect.max);
            (a, div, b)
        }
        SplitDir::Vertical => {
            let y = rect.min.y + rect.height() * ratio;
            let a = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, y - half));
            let div = egui::Rect::from_min_max(
                egui::pos2(rect.min.x, y - half),
                egui::pos2(rect.max.x, y + half),
            );
            let b = egui::Rect::from_min_max(egui::pos2(rect.min.x, y + half), rect.max);
            (a, div, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: u32) -> PaneNode {
        PaneNode::Leaf {
            pane_id: id,
            last_size: (80, 24),
        }
    }

    #[test]
    fn leaf_ids_single() {
        assert_eq!(leaf(1).leaf_ids(), vec![1]);
    }

    #[test]
    fn split_and_leaf_ids_order() {
        let mut root = leaf(1);
        assert!(root.split_pane(1, 2, 10, SplitDir::Horizontal));
        assert_eq!(root.leaf_ids(), vec![1, 2]);
    }

    #[test]
    fn nested_split_leaf_ids() {
        let mut root = leaf(1);
        root.split_pane(1, 2, 10, SplitDir::Horizontal);
        root.split_pane(2, 3, 11, SplitDir::Vertical);
        assert_eq!(root.leaf_ids(), vec![1, 2, 3]);
    }

    #[test]
    fn split_unknown_pane_returns_false() {
        let mut root = leaf(1);
        assert!(!root.split_pane(99, 2, 10, SplitDir::Horizontal));
        assert_eq!(root.leaf_ids(), vec![1]);
    }

    #[test]
    fn remove_only_child_returns_is_target() {
        let mut root = leaf(1);
        assert!(matches!(root.remove_pane(1), RemoveResult::IsTarget));
    }

    #[test]
    fn remove_left_collapses_to_sibling() {
        let mut root = leaf(1);
        root.split_pane(1, 2, 10, SplitDir::Horizontal);
        let result = root.remove_pane(1);
        assert!(matches!(result, RemoveResult::CollapseToSibling(_)));
    }

    #[test]
    fn remove_right_and_tree_shrinks() {
        let mut root = leaf(1);
        root.split_pane(1, 2, 10, SplitDir::Horizontal);
        // Apply the collapse result
        if let RemoveResult::CollapseToSibling(replacement) = root.remove_pane(2) {
            root = replacement;
        }
        assert_eq!(root.leaf_ids(), vec![1]);
    }

    #[test]
    fn remove_unknown_returns_not_found() {
        let mut root = leaf(1);
        assert!(matches!(root.remove_pane(99), RemoveResult::NotFound));
    }

    #[test]
    fn find_split_ratio_mut_updates_ratio() {
        let mut root = leaf(1);
        root.split_pane(1, 2, 42, SplitDir::Horizontal);
        let ratio = root.find_split_ratio_mut(42).unwrap();
        *ratio = 0.3;
        if let PaneNode::Split { ratio, .. } = &root {
            assert!((*ratio - 0.3f32).abs() < 1e-6);
        }
    }

    #[test]
    fn find_split_ratio_unknown_id_returns_none() {
        let mut root = leaf(1);
        root.split_pane(1, 2, 42, SplitDir::Horizontal);
        assert!(root.find_split_ratio_mut(99).is_none());
    }

    #[test]
    fn update_size_updates_leaf() {
        let mut root = leaf(1);
        root.update_size(1, (40, 12));
        if let PaneNode::Leaf { last_size, .. } = &root {
            assert_eq!(*last_size, (40, 12));
        }
    }

    #[test]
    fn split_pane_with_node_inserts_subtree() {
        let mut root = leaf(1);
        root.split_pane(1, 2, 10, SplitDir::Horizontal);
        // Build a subtree with two leaves
        let mut subtree = leaf(3);
        subtree.split_pane(3, 4, 20, SplitDir::Vertical);
        // Insert the subtree alongside pane 2
        assert!(root.split_pane_with_node(2, subtree, 30, SplitDir::Vertical));
        assert_eq!(root.leaf_ids(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn split_pane_with_node_unknown_target() {
        let mut root = leaf(1);
        let subtree = leaf(2);
        assert!(!root.split_pane_with_node(99, subtree, 10, SplitDir::Horizontal));
        assert_eq!(root.leaf_ids(), vec![1]);
    }

    #[test]
    fn split_pane_with_node_single_leaf_target() {
        let mut root = leaf(1);
        let subtree = leaf(2);
        assert!(root.split_pane_with_node(1, subtree, 10, SplitDir::Horizontal));
        assert_eq!(root.leaf_ids(), vec![1, 2]);
    }

    #[test]
    fn split_rect_horizontal_widths() {
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 50.0));
        let (a, div, b) = split_rect(rect, SplitDir::Horizontal, 0.5);
        assert!(a.width() > 0.0);
        assert!(b.width() > 0.0);
        assert!(div.width() > 0.0);
        // a, divider, b tile the full width without gaps
        assert!((a.width() + div.width() + b.width() - 100.0).abs() < 1.0);
        assert!(a.max.x <= div.min.x + 0.1);
        assert!(div.max.x <= b.min.x + 0.1);
    }

    #[test]
    fn split_rect_vertical_heights() {
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 50.0));
        let (a, div, b) = split_rect(rect, SplitDir::Vertical, 0.5);
        assert!(a.height() > 0.0);
        assert!(b.height() > 0.0);
        assert!(div.height() > 0.0);
    }
}
