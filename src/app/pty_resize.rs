//! When a terminal's grid follows its pane's size.
//!
//! The PTY is told a new size only once the pane has kept it for `SETTLE`:
//! ConPTY clears the screen on every resize, so following each frame of a
//! drag would leave the terminal blank for as long as the running program
//! takes to redraw. A pane that goes back to the size its grid already has
//! cancels the wait, so a size it only passed through (a drag that returned,
//! a panel shown and hidden within the wait, a one-frame layout) is never
//! applied: that would leave the grid wider than the pane and cut text off
//! at its right edge until the next resize. Pure: time is passed in.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a pane must keep a new size before its terminal is resized.
pub(super) const SETTLE: Duration = Duration::from_millis(150);

/// When to look again while a size waits: just past `SETTLE`, so that
/// frame finds the wait over.
pub(super) const REPAINT_AFTER: Duration = SETTLE.saturating_add(Duration::from_millis(10));

/// A grid size: (columns, rows).
pub(super) type GridSize = (u16, u16);

/// A measured column or row count as the PTY takes it: at least 1, and
/// `u16::MAX` rather than wrapping for an absurdly large pane.
pub(super) fn grid_dim(cells: usize) -> u16 {
    u16::try_from(cells).unwrap_or(u16::MAX).max(1)
}

/// The sizes waiting to be applied, by session id.
#[derive(Default)]
pub(super) struct PendingResizes {
    pending: HashMap<u32, (GridSize, Instant)>,
}

impl PendingResizes {
    /// Session `sid`'s pane measured `target` this frame; its grid is
    /// `applied`. A size equal to the grid cancels any wait; a new size
    /// starts one; the size already waiting keeps its start time.
    pub(super) fn measure(&mut self, sid: u32, applied: GridSize, target: GridSize, now: Instant) {
        if target == applied {
            self.pending.remove(&sid);
            return;
        }
        match self.pending.get(&sid) {
            Some(&(waiting, _)) if waiting == target => {}
            _ => {
                self.pending.insert(sid, (target, now));
            }
        }
    }

    /// The sizes kept for at least `SETTLE`, removed from the wait.
    pub(super) fn take_settled(&mut self, now: Instant) -> Vec<(u32, GridSize)> {
        let settled: Vec<(u32, GridSize)> = self
            .pending
            .iter()
            .filter(|(_, &(_, since))| now.saturating_duration_since(since) >= SETTLE)
            .map(|(&sid, &(size, _))| (sid, size))
            .collect();
        for (sid, _) in &settled {
            self.pending.remove(sid);
        }
        settled
    }

    /// Forget a closed session.
    pub(super) fn remove(&mut self, sid: u32) {
        self.pending.remove(&sid);
    }

    /// Whether any size is still waiting (the frame keeps repainting then).
    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: u32 = 7;
    const NARROW: GridSize = (80, 24);
    const WIDE: GridSize = (120, 24);

    fn after(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn a_new_size_is_applied_once_it_has_settled() {
        let t0 = Instant::now();
        let mut p = PendingResizes::default();
        p.measure(SID, NARROW, WIDE, t0);
        assert!(p.take_settled(after(t0, 100)).is_empty());
        assert!(!p.is_empty());
        assert_eq!(p.take_settled(after(t0, 150)), vec![(SID, WIDE)]);
        assert!(p.is_empty());
    }

    #[test]
    fn measuring_the_waiting_size_again_keeps_its_start() {
        let t0 = Instant::now();
        let mut p = PendingResizes::default();
        p.measure(SID, NARROW, WIDE, t0);
        p.measure(SID, NARROW, WIDE, after(t0, 100));
        assert_eq!(p.take_settled(after(t0, 150)), vec![(SID, WIDE)]);
    }

    #[test]
    fn a_changing_size_restarts_the_wait() {
        let t0 = Instant::now();
        let mut p = PendingResizes::default();
        p.measure(SID, NARROW, WIDE, t0);
        p.measure(SID, NARROW, (100, 24), after(t0, 100));
        assert!(p.take_settled(after(t0, 150)).is_empty());
        assert_eq!(p.take_settled(after(t0, 250)), vec![(SID, (100, 24))]);
    }

    /// The right-edge bug: the pane passes through a wider size and comes
    /// back within the wait. Applying the wider size would leave the grid
    /// wider than the pane, cutting text off on the right.
    #[test]
    fn going_back_to_the_grid_size_cancels_the_wait() {
        let t0 = Instant::now();
        let mut p = PendingResizes::default();
        p.measure(SID, NARROW, WIDE, t0);
        p.measure(SID, NARROW, NARROW, after(t0, 50));
        assert!(p.is_empty());
        assert!(p.take_settled(after(t0, 500)).is_empty());
    }

    #[test]
    fn the_grid_size_alone_starts_nothing() {
        let t0 = Instant::now();
        let mut p = PendingResizes::default();
        p.measure(SID, NARROW, NARROW, t0);
        assert!(p.is_empty());
    }

    #[test]
    fn sessions_wait_independently_and_a_closed_one_is_forgotten() {
        let t0 = Instant::now();
        let mut p = PendingResizes::default();
        p.measure(1, NARROW, WIDE, t0);
        p.measure(2, NARROW, WIDE, after(t0, 100));
        p.remove(1);
        assert!(p.take_settled(after(t0, 200)).is_empty());
        assert_eq!(p.take_settled(after(t0, 250)), vec![(2, WIDE)]);
    }

    #[test]
    fn grid_dims_are_at_least_one_and_never_wrap() {
        assert_eq!(grid_dim(0), 1);
        assert_eq!(grid_dim(132), 132);
        assert_eq!(grid_dim(70_000), u16::MAX);
    }

    #[test]
    fn a_clock_that_goes_back_never_panics_or_applies_early() {
        let t0 = Instant::now() + Duration::from_secs(1);
        let mut p = PendingResizes::default();
        p.measure(SID, NARROW, WIDE, t0);
        assert!(p.take_settled(t0 - Duration::from_millis(500)).is_empty());
    }
}
