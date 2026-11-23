//! How wide the right panel (files, git, Markdown, notes) may be dragged.
//!
//! The upper bound is a share of the space beside the left panel, not a
//! fixed width, so a Markdown file can be read at nearly full width right
//! there, while the middle panes always keep a strip of their own. Pure: the
//! width egui reports is passed in.

use crate::theme;
use std::ops::RangeInclusive;

/// The narrowest the right panel may be dragged.
pub(in crate::app) const RIGHT_PANEL_MIN_W: f32 = 80.0;

/// The largest share of the space beside the left panel the right panel may
/// take; the rest stays with the middle panes.
pub(in crate::app) const RIGHT_PANEL_MAX_SHARE: f32 = 0.8;

/// The widths the right panel may take when `available_w` is the space it
/// shares with the middle panes this frame (the window less the left panel).
/// egui also caps a panel at the space it is given, so when that is less
/// than the minimum the panel fills it.
pub(in crate::app) fn right_panel_width_range(available_w: f32) -> RangeInclusive<f32> {
    let share = if available_w.is_finite() {
        available_w * RIGHT_PANEL_MAX_SHARE
    } else {
        RIGHT_PANEL_MIN_W
    };
    RIGHT_PANEL_MIN_W..=share.max(RIGHT_PANEL_MIN_W)
}

/// A saved width, made safe to restore: at least the minimum, the default
/// when it is not a number. The upper bound depends on the window, so the
/// panel applies it as it is shown (`right_panel_width_range`).
pub(in crate::app) fn restored_right_panel_width(saved: f32) -> f32 {
    if saved.is_finite() {
        saved.max(RIGHT_PANEL_MIN_W)
    } else {
        theme::RIGHT_SIDEBAR_W
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_may_take_most_of_the_space_beside_the_left_panel() {
        let range = right_panel_width_range(1000.0);
        assert_eq!(*range.start(), RIGHT_PANEL_MIN_W);
        assert_eq!(*range.end(), 800.0);
    }

    #[test]
    fn the_middle_panes_always_keep_a_share() {
        for available in [300.0, 1280.0, 2560.0, 5120.0] {
            let max = *right_panel_width_range(available).end();
            assert!(available - max >= available * (1.0 - RIGHT_PANEL_MAX_SHARE) - 0.01);
        }
    }

    #[test]
    fn a_tiny_or_unknown_space_keeps_a_valid_range() {
        for available in [0.0, 50.0, -10.0, f32::NAN, f32::INFINITY] {
            let range = right_panel_width_range(available);
            assert!(range.start() <= range.end(), "{available}");
            assert_eq!(*range.start(), RIGHT_PANEL_MIN_W);
        }
    }

    #[test]
    fn a_wide_saved_width_is_kept_for_the_panel_to_cap() {
        assert_eq!(restored_right_panel_width(1500.0), 1500.0);
    }

    #[test]
    fn a_saved_width_is_never_below_the_minimum_or_not_a_number() {
        assert_eq!(restored_right_panel_width(10.0), RIGHT_PANEL_MIN_W);
        assert_eq!(restored_right_panel_width(f32::NAN), theme::RIGHT_SIDEBAR_W);
        assert_eq!(
            restored_right_panel_width(f32::INFINITY),
            theme::RIGHT_SIDEBAR_W
        );
    }
}
