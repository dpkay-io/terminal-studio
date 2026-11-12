use crate::editor_group::{GroupId, GroupNode};
use crate::pane_tree::SplitDir;
use std::collections::HashSet;

/// Ordered `(group_id, label)` pairs, one per split, for the left nav section
/// headers. Empty when there is only one group — the list renders flat then.
///
/// A two-leaf root gets spatial labels, which map onto what the user sees. Any
/// deeper tree falls back to indices, because "Left" stops being meaningful
/// once a side is itself subdivided.
pub(super) fn section_labels(layout: &GroupNode) -> Vec<(GroupId, String)> {
    let ids = layout.group_ids();
    if ids.len() < 2 {
        return Vec::new();
    }

    if let GroupNode::Split { dir, a, b, .. } = layout {
        if let (GroupNode::Leaf { group_id: ga }, GroupNode::Leaf { group_id: gb }) =
            (a.as_ref(), b.as_ref())
        {
            let (first, second) = match dir {
                SplitDir::Horizontal => ("Left", "Right"),
                SplitDir::Vertical => ("Top", "Bottom"),
            };
            return vec![(*ga, first.to_string()), (*gb, second.to_string())];
        }
    }

    ids.into_iter()
        .enumerate()
        .map(|(i, gid)| (gid, format!("Split {}", i + 1)))
        .collect()
}

/// How prominently a session row should be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum RowState {
    /// The pane the user is typing into.
    Focused,
    /// On screen right now, but in a split that does not have focus.
    Visible,
    /// Open but not currently displayed anywhere.
    Background,
}

/// `visible_pane_ids` is the `active_pane_id` of every group.
pub(in crate::app) fn row_state(
    pane_id: u32,
    active_pane_id: Option<u32>,
    visible_pane_ids: &[u32],
) -> RowState {
    if active_pane_id == Some(pane_id) {
        RowState::Focused
    } else if visible_pane_ids.contains(&pane_id) {
        RowState::Visible
    } else {
        RowState::Background
    }
}

/// Caches the recency sort order of the session list so that activating a row
/// does not reshuffle the list under the user's pointer mid-interaction.
///
/// The order is recomputed from recency whenever the pointer is outside the
/// list. While it is inside, the order is held: a closed session drops out
/// (it must never linger as a ghost row) and a new one is appended at the end,
/// but the rows that remain keep their relative order. Re-sorting instead
/// would move the row under the pointer, so the next click would land on a
/// different session than the one the user aimed at.
#[derive(Default)]
pub(in crate::app) struct SortCache {
    order: Vec<u32>,
    members: HashSet<u32>,
}

impl SortCache {
    /// `panes` is `(pane_id, last_active_at)`, in any order.
    pub(in crate::app) fn order(&mut self, panes: &[(u32, u64)], pointer_inside: bool) -> Vec<u32> {
        let members: HashSet<u32> = panes.iter().map(|(id, _)| *id).collect();
        let membership_changed = members != self.members;

        if pointer_inside && !self.order.is_empty() {
            if membership_changed {
                self.order.retain(|id| members.contains(id));
                let held: HashSet<u32> = self.order.iter().copied().collect();
                let added: Vec<(u32, u64)> = panes
                    .iter()
                    .copied()
                    .filter(|(id, _)| !held.contains(id))
                    .collect();
                self.order.extend(by_recency(added));
                self.members = members;
            }
            return self.order.clone();
        }

        self.order = by_recency(panes.to_vec());
        self.members = members;
        self.order.clone()
    }
}

/// Pane ids, most recently active first. Ties break on id so panes sharing a
/// timestamp keep a deterministic order — without it the list flickers
/// between frames.
fn by_recency(mut panes: Vec<(u32, u64)>) -> Vec<u32> {
    panes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    panes.into_iter().map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_group::GroupNode;
    use crate::pane_tree::SplitDir;

    fn leaf(id: u32) -> GroupNode {
        GroupNode::Leaf { group_id: id }
    }

    fn split(dir: SplitDir, a: GroupNode, b: GroupNode) -> GroupNode {
        GroupNode::Split {
            split_id: 1,
            dir,
            ratio: 0.5,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    #[test]
    fn single_group_has_no_sections() {
        assert!(section_labels(&leaf(1)).is_empty());
    }

    #[test]
    fn horizontal_two_way_is_left_right() {
        let root = split(SplitDir::Horizontal, leaf(1), leaf(2));
        assert_eq!(
            section_labels(&root),
            vec![(1, "Left".to_string()), (2, "Right".to_string())]
        );
    }

    #[test]
    fn vertical_two_way_is_top_bottom() {
        let root = split(SplitDir::Vertical, leaf(1), leaf(2));
        assert_eq!(
            section_labels(&root),
            vec![(1, "Top".to_string()), (2, "Bottom".to_string())]
        );
    }

    #[test]
    fn nested_tree_falls_back_to_numbered_splits() {
        let root = split(
            SplitDir::Horizontal,
            leaf(1),
            split(SplitDir::Vertical, leaf(2), leaf(3)),
        );
        assert_eq!(
            section_labels(&root),
            vec![
                (1, "Split 1".to_string()),
                (2, "Split 2".to_string()),
                (3, "Split 3".to_string()),
            ]
        );
    }

    #[test]
    fn labels_follow_layout_order_not_id_order() {
        let root = split(SplitDir::Horizontal, leaf(9), leaf(4));
        assert_eq!(
            section_labels(&root),
            vec![(9, "Left".to_string()), (4, "Right".to_string())]
        );
    }

    #[test]
    fn row_state_focused_beats_visible() {
        assert_eq!(row_state(7, Some(7), &[7, 9]), RowState::Focused);
    }

    #[test]
    fn row_state_visible_when_active_in_another_split() {
        assert_eq!(row_state(9, Some(7), &[7, 9]), RowState::Visible);
    }

    #[test]
    fn row_state_background_otherwise() {
        assert_eq!(row_state(3, Some(7), &[7, 9]), RowState::Background);
    }

    #[test]
    fn row_state_no_active_pane() {
        assert_eq!(row_state(3, None, &[]), RowState::Background);
    }

    #[test]
    fn sort_cache_freezes_order_while_pointer_inside() {
        let mut c = SortCache::default();
        let first = c.order(&[(1, 100), (2, 200)], true);
        assert_eq!(first, vec![2, 1]);
        // Pane 1 becomes most recent, but the pointer is still inside.
        let second = c.order(&[(1, 300), (2, 200)], true);
        assert_eq!(second, vec![2, 1], "order must not change while hovered");
    }

    #[test]
    fn sort_cache_recomputes_after_pointer_leaves() {
        let mut c = SortCache::default();
        assert_eq!(c.order(&[(1, 100), (2, 200)], true), vec![2, 1]);
        assert_eq!(c.order(&[(1, 300), (2, 200)], false), vec![1, 2]);
    }

    #[test]
    fn sort_cache_invalidates_on_membership_change_even_when_hovered() {
        let mut c = SortCache::default();
        assert_eq!(c.order(&[(1, 100), (2, 200)], true), vec![2, 1]);
        // Pane 2 closed while hovering — it must not linger as a ghost row.
        assert_eq!(c.order(&[(1, 100), (3, 50)], true), vec![1, 3]);
    }

    #[test]
    fn sort_cache_close_while_hovered_keeps_remaining_rows_in_place() {
        let mut c = SortCache::default();
        assert_eq!(
            c.order(&[(1, 100), (2, 200), (3, 300)], true),
            vec![3, 2, 1]
        );
        // Pane 1 was clicked (now most recent) and pane 2 then closed, both
        // while hovering. Re-sorting would put 1 on top, under the pointer
        // that was aimed at 3.
        assert_eq!(c.order(&[(1, 400), (3, 300)], true), vec![3, 1]);
    }

    #[test]
    fn sort_cache_new_rows_append_while_hovered() {
        let mut c = SortCache::default();
        assert_eq!(c.order(&[(1, 100), (2, 200)], true), vec![2, 1]);
        // Two sessions spawned while hovering: appended, newest first, and the
        // existing rows do not move.
        assert_eq!(
            c.order(&[(1, 100), (2, 200), (5, 500), (4, 400)], true),
            vec![2, 1, 5, 4]
        );
        // Membership is up to date again, so a later frame holds this order.
        assert_eq!(
            c.order(&[(1, 900), (2, 200), (5, 500), (4, 400)], true),
            vec![2, 1, 5, 4]
        );
    }

    #[test]
    fn sort_cache_resorts_once_pointer_leaves_after_changes() {
        let mut c = SortCache::default();
        c.order(&[(1, 100), (2, 200)], true);
        c.order(&[(1, 100), (2, 200), (3, 50)], true);
        assert_eq!(
            c.order(&[(1, 100), (2, 200), (3, 50)], false),
            vec![2, 1, 3]
        );
        assert_eq!(
            c.order(&[(1, 900), (2, 200), (3, 50)], false),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn sort_cache_ties_break_on_id() {
        let mut c = SortCache::default();
        assert_eq!(
            c.order(&[(9, 100), (3, 100), (5, 100)], false),
            vec![3, 5, 9]
        );
    }

    #[test]
    fn sort_cache_empty_input() {
        let mut c = SortCache::default();
        assert!(c.order(&[], true).is_empty());
    }
}
