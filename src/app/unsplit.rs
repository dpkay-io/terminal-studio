//! Leaving split mode without closing anything.
//!
//! Unsplitting a split moves every tab it holds into the split beside it, in
//! order, and gives that split the space; unsplitting all puts every tab back
//! into one split. No session, file or note is closed, so nothing is asked
//! about unsaved changes and every terminal keeps running. The tab that was
//! in front stays in front, so what the user was looking at stays on screen.
//!
//! The info bar's unsplit button (right-click: Unsplit All), a split section
//! header's right-click menu in the left nav, the shortcuts and the command
//! palette all ask for an [`Unsplit`]; the frame runs it after the splits are
//! drawn (`App::apply_unsplit`), as it does a split.

use std::collections::HashSet;

use super::feedback::{FlashKind, FlashTarget};
use super::pane_state::PaneState;
use super::App;
use crate::editor_group::{GroupId, GroupNode};
use crate::shortcuts::AppAction;

/// Label for unsplitting one split, wherever it is offered.
pub(super) const UNSPLIT_LABEL: &str = "Unsplit";
/// Label for unsplitting every split, wherever it is offered.
pub(super) const UNSPLIT_ALL_LABEL: &str = "Unsplit All";

/// What to unsplit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Unsplit {
    /// This split's tabs join the split beside it.
    Split(GroupId),
    /// Every split's tabs join the focused split.
    All,
}

impl Unsplit {
    /// The request a shortcut or palette entry stands for, against the
    /// focused split. `None` for every other action.
    pub(super) fn for_action(action: AppAction, focused_group: GroupId) -> Option<Self> {
        match action {
            AppAction::Unsplit => Some(Self::Split(focused_group)),
            AppAction::UnsplitAll => Some(Self::All),
            _ => None,
        }
    }
}

impl PaneState {
    /// There is more than one split on screen, so there is something to
    /// unsplit.
    pub(super) fn has_splits(&self) -> bool {
        matches!(self.group_layout, GroupNode::Split { .. })
    }

    /// Run `request`. Returns the split everything went into, which is now
    /// focused, or `None` when there was nothing to unsplit.
    pub(super) fn unsplit(&mut self, request: Unsplit) -> Option<GroupId> {
        match request {
            Unsplit::Split(group_id) => self.unsplit_group(group_id),
            Unsplit::All => self.unsplit_all(),
        }
    }

    /// Move every tab of `group_id` to the end of the split beside it (its
    /// sibling in the layout, which takes its space), in order. The tab
    /// `group_id` had in front is in front there, and that split is focused.
    fn unsplit_group(&mut self, group_id: GroupId) -> Option<GroupId> {
        let target_id = self.group_layout.sibling_of(group_id)?;
        if target_id == group_id || !self.groups.contains_key(&target_id) {
            return None;
        }
        let source = self.groups.get(&group_id)?.clone();
        let target = self.groups.get_mut(&target_id)?;
        for &pane_id in &source.pane_ids {
            target.insert_pane(pane_id, None);
        }
        if let Some(front) = source.active_pane_id.filter(|&pid| target.contains(pid)) {
            target.activate(front);
        }
        let front = target.active_pane_id;
        self.erase_group(group_id);
        self.focus_after_unsplit(target_id, front);
        Some(target_id)
    }

    /// Put every tab into the focused split (the first one on screen when the
    /// focus names none), in the order the splits are on screen. The focused
    /// split keeps its tab in front and its info bar.
    fn unsplit_all(&mut self) -> Option<GroupId> {
        if !self.has_splits() && self.groups.len() <= 1 {
            return None;
        }
        let on_screen = self.group_layout.group_ids();
        let target_id = if self.groups.contains_key(&self.focused_group_id) {
            self.focused_group_id
        } else {
            on_screen
                .iter()
                .copied()
                .find(|gid| self.groups.contains_key(gid))?
        };
        // Splits missing from the layout come last, by id, so none of their
        // tabs is lost and the order never depends on the map's.
        let mut off_screen: Vec<GroupId> = self
            .groups
            .keys()
            .copied()
            .filter(|gid| !on_screen.contains(gid))
            .collect();
        off_screen.sort_unstable();

        let mut seen = HashSet::new();
        let pane_ids: Vec<u32> = on_screen
            .iter()
            .chain(&off_screen)
            .filter_map(|gid| self.groups.get(gid))
            .flat_map(|group| group.pane_ids.iter().copied())
            .filter(|pid| seen.insert(*pid))
            .collect();

        let mut target = self.groups.remove(&target_id)?;
        target.pane_ids = pane_ids;
        if !target
            .active_pane_id
            .is_some_and(|pid| target.contains(pid))
        {
            target.active_pane_id = target.pane_ids.first().copied();
        }
        let front = target.active_pane_id;
        self.groups.clear();
        self.groups.insert(target_id, target);
        self.group_layout = GroupNode::Leaf {
            group_id: target_id,
        };
        self.focus_after_unsplit(target_id, front);
        Some(target_id)
    }

    fn focus_after_unsplit(&mut self, group_id: GroupId, front: Option<u32>) {
        self.focused_group_id = group_id;
        if front.is_some() {
            self.active_pane_id = front;
        }
    }
}

impl App {
    /// Ask for `request` from a button or menu drawn this frame. It runs
    /// with the frame's other layout changes, after the splits are drawn.
    pub(super) fn request_unsplit(&mut self, request: Unsplit, ctx: &egui::Context) {
        self.deferred_unsplit = Some(request);
        // Taken at the top of the next frame, so make sure there is one.
        ctx.request_repaint();
    }

    /// Run an unsplit: the tab in front of the merged split gets the focus
    /// (session flags, recency, file reveal agree), a brief flash shows where
    /// everything went, and the new layout is saved.
    pub(super) fn apply_unsplit(&mut self, request: Unsplit) {
        let Some(group_id) = self.pane_state.unsplit(request) else {
            log::debug!("unsplit {request:?}: nothing to unsplit");
            return;
        };
        log::debug!("unsplit {request:?}: tabs now in split {group_id}");
        if let Some(pane_id) = self
            .pane_state
            .groups
            .get(&group_id)
            .and_then(|g| g.active_pane_id)
        {
            self.activate_pane(pane_id);
            self.flash
                .trigger(FlashTarget::Pane(pane_id), FlashKind::Success);
        }
        self.save_session();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::pane::{PaneContent, PaneEntry};
    use crate::editor_group::EditorGroup;
    use crate::pane_tree::SplitDir;

    fn pane(id: u32) -> PaneEntry {
        PaneEntry {
            id,
            content: PaneContent::Terminal(id),
            manual_width: None,
            last_size: (80, 24),
            labels: vec![],
            last_active_at: 0,
            workspace_id: None,
        }
    }

    fn group(id: GroupId, panes: &[u32], active: u32) -> EditorGroup {
        EditorGroup {
            id,
            pane_ids: panes.to_vec(),
            active_pane_id: Some(active),
            info_bar: None,
        }
    }

    fn leaf(group_id: GroupId) -> Box<GroupNode> {
        Box::new(GroupNode::Leaf { group_id })
    }

    fn split(split_id: u32, dir: SplitDir, a: Box<GroupNode>, b: Box<GroupNode>) -> GroupNode {
        GroupNode::Split {
            split_id,
            dir,
            ratio: 0.5,
            a,
            b,
        }
    }

    /// Split 1 holds panes 1, 2 (2 in front); split 2 holds 3, 4 (3 in
    /// front); split 3 holds 5. Layout: 1 | (2 / 3), split 2 focused.
    fn three_splits() -> PaneState {
        let mut ps = PaneState::new();
        ps.panes = (1..=5).map(pane).collect();
        for g in [
            group(1, &[1, 2], 2),
            group(2, &[3, 4], 3),
            group(3, &[5], 5),
        ] {
            ps.groups.insert(g.id, g);
        }
        ps.group_layout = split(
            1,
            SplitDir::Horizontal,
            leaf(1),
            Box::new(split(2, SplitDir::Vertical, leaf(2), leaf(3))),
        );
        ps.focused_group_id = 2;
        ps.active_pane_id = Some(3);
        ps.next_group_id = 4;
        ps.next_split_id = 3;
        ps
    }

    fn panes_of(ps: &PaneState, gid: GroupId) -> Vec<u32> {
        ps.groups[&gid].pane_ids.clone()
    }

    fn all_pane_ids(ps: &PaneState) -> Vec<u32> {
        let mut ids: Vec<u32> = ps.panes.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn unsplit_moves_every_tab_to_the_sibling_and_closes_nothing() {
        let mut ps = three_splits();
        assert_eq!(ps.unsplit(Unsplit::Split(2)), Some(3));
        assert_eq!(panes_of(&ps, 3), vec![5, 3, 4]);
        assert!(!ps.groups.contains_key(&2));
        assert_eq!(all_pane_ids(&ps), vec![1, 2, 3, 4, 5]);
        assert_eq!(ps.group_layout.group_ids(), vec![1, 3]);
    }

    #[test]
    fn unsplit_keeps_the_front_tab_in_front_and_focuses_its_split() {
        let mut ps = three_splits();
        ps.unsplit(Unsplit::Split(2));
        assert_eq!(ps.groups[&3].active_pane_id, Some(3));
        assert_eq!(ps.focused_group_id, 3);
        assert_eq!(ps.active_pane_id, Some(3));
    }

    #[test]
    fn unsplit_gives_the_space_to_the_split_it_shared_it_with() {
        let mut ps = three_splits();
        // Split 1 shares the root with (2 / 3): its tabs go to 2, the first
        // split of that side, and the layout is just (2 / 3).
        assert_eq!(ps.unsplit(Unsplit::Split(1)), Some(2));
        assert_eq!(panes_of(&ps, 2), vec![3, 4, 1, 2]);
        assert_eq!(ps.groups[&2].active_pane_id, Some(2));
        assert!(matches!(
            ps.group_layout,
            GroupNode::Split {
                dir: SplitDir::Vertical,
                ..
            }
        ));
        assert_eq!(ps.group_layout.group_ids(), vec![2, 3]);
    }

    #[test]
    fn unsplitting_the_last_split_leaves_one() {
        let mut ps = three_splits();
        ps.unsplit(Unsplit::Split(3));
        assert_eq!(ps.unsplit(Unsplit::Split(2)), Some(1));
        assert!(matches!(ps.group_layout, GroupNode::Leaf { group_id: 1 }));
        assert_eq!(panes_of(&ps, 1), vec![1, 2, 3, 4, 5]);
        assert!(!ps.has_splits());
        assert_eq!(ps.groups.len(), 1);
    }

    #[test]
    fn unsplit_without_splits_does_nothing() {
        let mut ps = PaneState::new();
        ps.panes = vec![pane(1), pane(2)];
        ps.groups.insert(1, group(1, &[1, 2], 1));
        ps.group_layout = *leaf(1);
        ps.focused_group_id = 1;
        assert!(!ps.has_splits());
        assert_eq!(ps.unsplit(Unsplit::Split(1)), None);
        assert_eq!(ps.unsplit(Unsplit::All), None);
        assert_eq!(panes_of(&ps, 1), vec![1, 2]);
    }

    #[test]
    fn unsplit_of_an_unknown_split_does_nothing() {
        let mut ps = three_splits();
        assert_eq!(ps.unsplit(Unsplit::Split(99)), None);
        assert_eq!(ps.groups.len(), 3);
        assert_eq!(ps.focused_group_id, 2);
    }

    #[test]
    fn unsplit_into_a_split_missing_from_the_map_does_nothing() {
        let mut ps = three_splits();
        ps.groups.remove(&3);
        assert_eq!(ps.unsplit(Unsplit::Split(2)), None);
        assert_eq!(panes_of(&ps, 2), vec![3, 4]);
    }

    #[test]
    fn unsplit_all_puts_every_tab_in_the_focused_split_in_screen_order() {
        let mut ps = three_splits();
        assert_eq!(ps.unsplit(Unsplit::All), Some(2));
        assert_eq!(panes_of(&ps, 2), vec![1, 2, 3, 4, 5]);
        assert_eq!(ps.groups.len(), 1);
        assert!(matches!(ps.group_layout, GroupNode::Leaf { group_id: 2 }));
        assert_eq!(ps.groups[&2].active_pane_id, Some(3));
        assert_eq!(ps.active_pane_id, Some(3));
        assert_eq!(all_pane_ids(&ps), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn unsplit_all_keeps_the_focused_splits_info_bar() {
        let mut ps = three_splits();
        ps.groups.get_mut(&2).unwrap().info_bar =
            Some(crate::app::settings::InfoBarPosition::Bottom);
        ps.unsplit(Unsplit::All);
        assert_eq!(
            ps.groups[&2].info_bar,
            Some(crate::app::settings::InfoBarPosition::Bottom)
        );
    }

    #[test]
    fn unsplit_all_with_no_valid_focus_uses_the_first_split_on_screen() {
        let mut ps = three_splits();
        ps.focused_group_id = 42;
        assert_eq!(ps.unsplit(Unsplit::All), Some(1));
        assert_eq!(panes_of(&ps, 1), vec![1, 2, 3, 4, 5]);
        assert_eq!(ps.focused_group_id, 1);
        assert_eq!(ps.active_pane_id, Some(2));
    }

    #[test]
    fn unsplit_all_keeps_tabs_of_splits_missing_from_the_layout() {
        let mut ps = three_splits();
        ps.panes.push(pane(6));
        ps.panes.push(pane(7));
        ps.groups.insert(9, group(9, &[7], 7));
        ps.groups.insert(8, group(8, &[6], 6));
        ps.unsplit(Unsplit::All);
        assert_eq!(panes_of(&ps, 2), vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn unsplit_all_lists_a_tab_held_twice_once() {
        let mut ps = three_splits();
        ps.groups.get_mut(&3).unwrap().pane_ids.push(1);
        ps.unsplit(Unsplit::All);
        assert_eq!(panes_of(&ps, 2), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn unsplit_all_with_a_stale_front_tab_shows_the_first() {
        let mut ps = three_splits();
        ps.groups.get_mut(&2).unwrap().active_pane_id = Some(99);
        ps.unsplit(Unsplit::All);
        assert_eq!(ps.groups[&2].active_pane_id, Some(1));
        assert_eq!(ps.active_pane_id, Some(1));
    }

    #[test]
    fn a_new_split_after_unsplit_all_gets_a_fresh_id() {
        let mut ps = three_splits();
        ps.unsplit(Unsplit::All);
        ps.panes.push(pane(6));
        let new_gid = ps.split_focused_group(6, SplitDir::Horizontal, false);
        assert!(new_gid >= 4);
        assert_eq!(ps.group_layout.group_ids(), vec![2, new_gid]);
    }

    #[test]
    fn shortcuts_map_to_unsplit_requests() {
        assert_eq!(
            Unsplit::for_action(AppAction::Unsplit, 7),
            Some(Unsplit::Split(7))
        );
        assert_eq!(
            Unsplit::for_action(AppAction::UnsplitAll, 7),
            Some(Unsplit::All)
        );
        assert_eq!(Unsplit::for_action(AppAction::SplitHorizontal, 7), None);
    }
}
