use crate::editor_group::{GroupId, GroupNode};
use crate::pane_tree::SplitDir;

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
}
