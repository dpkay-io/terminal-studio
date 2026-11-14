//! The left nav's row order: the top rows of each split section stay where
//! they are, and a session used from below them moves to row 1.
//!
//! Each section (one per split, plus "Other" for a pane in no split) keeps
//! its own order inside one list. When a pane becomes active:
//! - in the section's first `stable_rows` rows: nothing moves;
//! - below them, or new to the list: it moves to the section's first row and
//!   the rest slide down one. A new pane joining a section that still has
//!   room just takes the next free row.
//!
//! A pane that changes section goes to the end of its new section first, so
//! a split closing never reshuffles the section its tabs join. If the move
//! also activated it, the rule above then applies in the new section.
//!
//! Activity is read from each pane's `last_active_at`, which every activation
//! stamps (`App::activate_pane`), so no call site has to report it. While
//! `hold` is set (the pointer is over the list) closed rows still drop out
//! and new ones are appended, but nothing moves; held activations are applied
//! oldest first once it is released. Pure: no egui.

use crate::editor_group::GroupId;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

/// How many rows the numbered tab shortcuts (⌘1–9) reach.
pub(in crate::app) const NUMBERED_TAB_COUNT: usize = 9;

/// The left nav section a pane is listed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::app) enum SectionKey {
    Group(GroupId),
    /// A pane in no split.
    Other,
}

/// What the order needs to know about one pane, this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) struct NavPane {
    pub id: u32,
    pub section: SectionKey,
    pub last_active_at: u64,
}

#[derive(Debug, Clone, Copy)]
struct Seen {
    /// The `last_active_at` already applied; `None` for a pane not placed yet.
    stamp: Option<u64>,
    /// The section the pane is placed in, which the rule counts rows by.
    placed: SectionKey,
    /// The section the pane is in now, which may be ahead of `placed` while
    /// the order is held.
    live: SectionKey,
}

#[derive(Default)]
pub(in crate::app) struct NavOrder {
    order: Vec<u32>,
    seen: HashMap<u32, Seen>,
}

impl NavOrder {
    /// The saved order first (unknown and repeated ids dropped), then every
    /// pane it does not list, most recent first. Nothing counts as an
    /// activation until a pane's `last_active_at` changes.
    pub(in crate::app) fn restore(saved: &[u32], panes: &[NavPane]) -> Self {
        let present: HashSet<u32> = panes.iter().map(|p| p.id).collect();
        let mut listed: HashSet<u32> = HashSet::with_capacity(saved.len());
        let mut order: Vec<u32> = Vec::with_capacity(panes.len());
        for &id in saved {
            if present.contains(&id) && listed.insert(id) {
                order.push(id);
            }
        }
        let mut rest: Vec<&NavPane> = panes.iter().filter(|p| !listed.contains(&p.id)).collect();
        rest.sort_by_key(|p| (Reverse(p.last_active_at), p.id));
        order.extend(rest.iter().map(|p| p.id));
        let seen = panes
            .iter()
            .map(|p| {
                let seen = Seen {
                    stamp: Some(p.last_active_at),
                    placed: p.section,
                    live: p.section,
                };
                (p.id, seen)
            })
            .collect();
        NavOrder { order, seen }
    }

    /// Bring the order up to date with this frame's panes. Allocates only
    /// when something changed.
    pub(in crate::app) fn refresh(&mut self, panes: &[NavPane], stable_rows: usize, hold: bool) {
        if self.order.is_empty() && self.seen.is_empty() {
            *self = Self::restore(&[], panes);
            return;
        }
        self.sync_membership(panes);
        if !hold {
            self.apply_activity(panes, stable_rows);
        }
    }

    /// Pane ids, top row first, every section interleaved; bucket by section
    /// to draw.
    pub(in crate::app) fn order(&self) -> &[u32] {
        &self.order
    }

    /// Targets for ⌘1–9: the rows of `section` (`None` = the whole list) that
    /// `shown` lets through, top first.
    pub(in crate::app) fn numbered(
        &self,
        section: Option<SectionKey>,
        shown: impl Fn(u32) -> bool,
    ) -> Vec<u32> {
        self.order
            .iter()
            .copied()
            .filter(|id| section.map_or(true, |key| self.seen.get(id).is_some_and(|s| s.live == key)))
            .filter(|&id| shown(id))
            .take(NUMBERED_TAB_COUNT)
            .collect()
    }

    /// Drop closed panes, append new ones, track each pane's live section.
    fn sync_membership(&mut self, panes: &[NavPane]) {
        let unchanged = panes.len() == self.order.len()
            && panes.iter().all(|p| self.seen.contains_key(&p.id));
        if !unchanged {
            let present: HashSet<u32> = panes.iter().map(|p| p.id).collect();
            self.order.retain(|id| present.contains(id));
            self.seen.retain(|id, _| present.contains(id));
            for p in panes {
                if !self.seen.contains_key(&p.id) {
                    self.order.push(p.id);
                    let seen = Seen {
                        stamp: None,
                        placed: p.section,
                        live: p.section,
                    };
                    self.seen.insert(p.id, seen);
                }
            }
        }
        for p in panes {
            if let Some(seen) = self.seen.get_mut(&p.id) {
                seen.live = p.section;
            }
        }
    }

    /// Apply every activation and section move not applied yet, oldest first.
    fn apply_activity(&mut self, panes: &[NavPane], stable_rows: usize) {
        if !panes.iter().any(|p| self.is_pending(p)) {
            return;
        }
        let mut pending: Vec<NavPane> = panes.iter().filter(|p| self.is_pending(p)).copied().collect();
        pending.sort_by_key(|p| (p.last_active_at, p.id));
        for p in pending {
            let Some(seen) = self.seen.get(&p.id).copied() else {
                continue;
            };
            if seen.placed != p.section {
                self.move_to_section_end(p.id, p.section);
            }
            if seen.stamp != Some(p.last_active_at) && self.rank_in_section(p.id) >= stable_rows {
                self.move_to_section_front(p.id);
            }
            self.seen.insert(
                p.id,
                Seen {
                    stamp: Some(p.last_active_at),
                    placed: p.section,
                    live: p.section,
                },
            );
        }
    }

    fn is_pending(&self, p: &NavPane) -> bool {
        self.seen
            .get(&p.id)
            .is_some_and(|s| s.stamp != Some(p.last_active_at) || s.placed != p.section)
    }

    fn placed(&self, id: u32) -> Option<SectionKey> {
        self.seen.get(&id).map(|s| s.placed)
    }

    /// Rows above `id` in its own section.
    fn rank_in_section(&self, id: u32) -> usize {
        let section = self.placed(id);
        self.order
            .iter()
            .take_while(|&&other| other != id)
            .filter(|&&other| self.placed(other) == section)
            .count()
    }

    /// Re-place `id` after the last row of `section`, or at the very end
    /// when that section is empty.
    fn move_to_section_end(&mut self, id: u32, section: SectionKey) {
        let Some(from) = self.order.iter().position(|&other| other == id) else {
            return;
        };
        self.order.remove(from);
        if let Some(seen) = self.seen.get_mut(&id) {
            seen.placed = section;
        }
        let at = self
            .order
            .iter()
            .rposition(|&other| self.placed(other) == Some(section))
            .map_or(self.order.len(), |last| last + 1);
        self.order.insert(at, id);
    }

    /// Re-place `id` before the first row of its section.
    fn move_to_section_front(&mut self, id: u32) {
        let Some(from) = self.order.iter().position(|&other| other == id) else {
            return;
        };
        let section = self.placed(id);
        let at = self
            .order
            .iter()
            .position(|&other| self.placed(other) == section)
            .unwrap_or(from);
        if at < from {
            self.order.remove(from);
            self.order.insert(at, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: SectionKey = SectionKey::Group(1);
    const B: SectionKey = SectionKey::Group(2);
    const ALL: fn(u32) -> bool = |_| true;

    fn p(id: u32, section: SectionKey, t: u64) -> NavPane {
        NavPane {
            id,
            section,
            last_active_at: t,
        }
    }

    /// Panes 1..=n in section `A`, `NavOrder` seeded so the order is 1..=n.
    fn seeded(n: u32) -> (Vec<NavPane>, NavOrder) {
        let panes: Vec<NavPane> = (1..=n).map(|id| p(id, A, 0)).collect();
        let ids: Vec<u32> = (1..=n).collect();
        let order = NavOrder::restore(&ids, &panes);
        (panes, order)
    }

    fn bump(panes: &mut [NavPane], id: u32, t: u64) {
        panes.iter_mut().find(|p| p.id == id).unwrap().last_active_at = t;
    }

    fn move_to(panes: &mut [NavPane], id: u32, section: SectionKey) {
        panes.iter_mut().find(|p| p.id == id).unwrap().section = section;
    }

    #[test]
    fn first_refresh_seeds_by_recency() {
        let panes = [p(1, A, 100), p(2, A, 300), p(3, A, 200), p(4, A, 200)];
        let mut o = NavOrder::default();
        o.refresh(&panes, 7, false);
        assert_eq!(o.order(), [2, 3, 4, 1]);
    }

    #[test]
    fn restore_uses_saved_order_then_unlisted_by_recency() {
        let panes = [p(1, A, 100), p(2, A, 500), p(3, A, 100), p(4, A, 600)];
        let o = NavOrder::restore(&[3, 1], &panes);
        assert_eq!(o.order(), [3, 1, 4, 2]);
    }

    #[test]
    fn restore_drops_duplicates_and_unknown_ids() {
        let panes = [p(1, A, 0), p(2, A, 0)];
        let o = NavOrder::restore(&[1, 9, 1, 2], &panes);
        assert_eq!(o.order(), [1, 2]);
    }

    #[test]
    fn restored_panes_are_not_treated_as_activations() {
        let panes = [p(1, A, 100), p(2, A, 900)];
        let mut o = NavOrder::restore(&[1, 2], &panes);
        o.refresh(&panes, 0, false);
        assert_eq!(o.order(), [1, 2], "restore keeps the saved order");
    }

    #[test]
    fn activating_inside_block_moves_nothing() {
        let (mut panes, mut o) = seeded(5);
        bump(&mut panes, 2, 10);
        o.refresh(&panes, 3, false);
        assert_eq!(o.order(), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn activating_below_block_goes_to_row_one() {
        let (mut panes, mut o) = seeded(5);
        bump(&mut panes, 5, 10);
        o.refresh(&panes, 3, false);
        assert_eq!(o.order(), [5, 1, 2, 3, 4]);
    }

    #[test]
    fn new_pane_takes_next_free_row_while_block_has_room() {
        let (mut panes, mut o) = seeded(2);
        panes.push(p(3, A, 10));
        o.refresh(&panes, 3, false);
        assert_eq!(o.order(), [1, 2, 3]);
    }

    #[test]
    fn new_pane_goes_to_row_one_when_block_full() {
        let (mut panes, mut o) = seeded(3);
        panes.push(p(4, A, 10));
        o.refresh(&panes, 3, false);
        assert_eq!(o.order(), [4, 1, 2, 3]);
    }

    #[test]
    fn closing_a_block_row_pulls_the_next_row_up() {
        let (mut panes, mut o) = seeded(4);
        panes.retain(|p| p.id != 2);
        o.refresh(&panes, 3, false);
        assert_eq!(o.order(), [1, 3, 4]);
    }

    #[test]
    fn held_activations_apply_oldest_first_on_release() {
        let (mut panes, mut o) = seeded(3);
        bump(&mut panes, 3, 10);
        o.refresh(&panes, 1, true);
        bump(&mut panes, 2, 20);
        o.refresh(&panes, 1, true);
        assert_eq!(o.order(), [1, 2, 3], "nothing moves while held");
        o.refresh(&panes, 1, false);
        assert_eq!(o.order(), [2, 3, 1]);
    }

    #[test]
    fn close_while_held_drops_row_at_once() {
        let (mut panes, mut o) = seeded(3);
        panes.retain(|p| p.id != 2);
        o.refresh(&panes, 7, true);
        assert_eq!(o.order(), [1, 3]);
    }

    #[test]
    fn new_pane_while_held_appends_then_is_placed() {
        let (mut panes, mut o) = seeded(2);
        panes.push(p(3, A, 10));
        o.refresh(&panes, 1, true);
        assert_eq!(o.order(), [1, 2, 3]);
        o.refresh(&panes, 1, false);
        assert_eq!(o.order(), [3, 1, 2]);
    }

    #[test]
    fn zero_stable_rows_is_the_recency_sort() {
        let mut panes: Vec<NavPane> = (1..=5).map(|id| p(id, A, u64::from(id))).collect();
        let mut o = NavOrder::default();
        o.refresh(&panes, 0, false);
        let mut t = 100;
        let script: [(bool, u32); 10] = [
            (false, 1),
            (false, 4),
            (true, 6),
            (false, 2),
            (false, 4),
            (false, 6),
            (true, 7),
            (false, 3),
            (false, 1),
            (false, 5),
        ];
        for (add, id) in script {
            t += 1;
            if add {
                panes.push(p(id, A, t));
            } else {
                bump(&mut panes, id, t);
            }
            o.refresh(&panes, 0, false);
            let mut expected = panes.clone();
            expected.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
            let expected: Vec<u32> = expected.iter().map(|p| p.id).collect();
            assert_eq!(o.order(), expected.as_slice(), "after {add}/{id}");
        }
    }

    #[test]
    fn stable_rows_larger_than_list_never_moves() {
        let (mut panes, mut o) = seeded(4);
        for (t, id) in (1..=4u32).rev().enumerate() {
            bump(&mut panes, id, t as u64 + 10);
            o.refresh(&panes, 100, false);
        }
        assert_eq!(o.order(), [1, 2, 3, 4]);
    }

    #[test]
    fn rule_counts_rows_per_section() {
        let mut panes = vec![
            p(1, A, 0),
            p(4, B, 0),
            p(2, A, 0),
            p(5, B, 0),
            p(3, A, 0),
        ];
        let mut o = NavOrder::restore(&[1, 4, 2, 5, 3], &panes);
        bump(&mut panes, 3, 10);
        o.refresh(&panes, 2, false);
        assert_eq!(o.numbered(Some(A), ALL), [3, 1, 2]);
        assert_eq!(o.numbered(Some(B), ALL), [4, 5]);
    }

    #[test]
    fn moved_and_activated_is_active_in_new_section() {
        let mut panes = vec![p(1, A, 0), p(2, A, 0), p(3, B, 0), p(4, B, 0)];
        let mut o = NavOrder::restore(&[1, 2, 3, 4], &panes);
        move_to(&mut panes, 1, B);
        bump(&mut panes, 1, 10);
        o.refresh(&panes, 2, false);
        assert_eq!(o.numbered(Some(B), ALL), [1, 3, 4]);
        assert_eq!(o.numbered(Some(A), ALL), [2]);
    }

    #[test]
    fn moved_without_activation_goes_to_end_of_new_section() {
        let mut panes = vec![p(1, A, 0), p(2, A, 0), p(3, B, 0), p(4, B, 0)];
        let mut o = NavOrder::restore(&[1, 2, 3, 4], &panes);
        move_to(&mut panes, 3, A);
        move_to(&mut panes, 4, A);
        o.refresh(&panes, 2, false);
        assert_eq!(o.numbered(Some(A), ALL), [1, 2, 3, 4]);
    }

    #[test]
    fn moved_into_an_empty_section_lands_there() {
        let mut panes = vec![p(1, A, 0), p(2, A, 0)];
        let mut o = NavOrder::restore(&[1, 2], &panes);
        move_to(&mut panes, 1, B);
        bump(&mut panes, 1, 10);
        o.refresh(&panes, 2, false);
        assert_eq!(o.numbered(Some(B), ALL), [1]);
        assert_eq!(o.numbered(Some(A), ALL), [2]);
    }

    #[test]
    fn numbered_caps_at_nine_and_whole_order_when_no_section() {
        let (_, o) = seeded(12);
        let n = o.numbered(None, ALL);
        assert_eq!(n.len(), 9);
        assert_eq!(n.as_slice(), &o.order()[..9]);
    }

    #[test]
    fn numbered_skips_rows_the_filter_hides() {
        let (_, o) = seeded(4);
        assert_eq!(o.numbered(None, |id| id != 2), [1, 3, 4]);
    }

    #[test]
    fn unchanged_frame_keeps_order() {
        let (panes, mut o) = seeded(3);
        o.refresh(&panes, 1, false);
        o.refresh(&panes, 1, false);
        assert_eq!(o.order(), [1, 2, 3]);
    }

    #[test]
    fn empty_input() {
        let mut o = NavOrder::default();
        o.refresh(&[], 7, false);
        assert!(o.order().is_empty());
    }
}
