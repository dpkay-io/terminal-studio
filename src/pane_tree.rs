#![allow(dead_code)]
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
    const DIV: f32 = 4.0;
    let half = DIV / 2.0;
    match dir {
        SplitDir::Horizontal => {
            let x = rect.min.x + rect.width() * ratio;
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
