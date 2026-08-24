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
}
