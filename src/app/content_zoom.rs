//! Zoom of the documents the middle panes show (file editors, Markdown
//! previews, diffs, notes): ⌘ / Ctrl + mouse wheel, or a trackpad pinch,
//! over a pane makes its text bigger or smaller, and the pane scrolls to
//! show all of it, as a browser page does. Each pane keeps its own zoom;
//! the side panels and every pane's header stay as they are. Terminals
//! keep their size (their text reflows into the grid instead). A pane's
//! zoom is saved with the session (`SavedPane.zoom`). Pure.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The smallest zoom.
pub(super) const ZOOM_MIN: f32 = 0.5;
/// The largest zoom.
pub(super) const ZOOM_MAX: f32 = 3.0;
/// A pane's zoom until it is zoomed.
pub(super) const ZOOM_DEFAULT: f32 = 1.0;
/// A zoom this close to 100 % lands on it, and zooming across 100 % stops
/// there, so the original size is always easy to get back to.
const SNAP_TO_DEFAULT: f32 = 0.04;
/// A zoom is saved once it has stayed the same this long, so a pinch or a
/// spin of the wheel writes the session file once, not on every step.
pub(super) const SAVE_SETTLE: Duration = Duration::from_secs(1);
/// Percent per whole zoom.
const PERCENT: f32 = 100.0;

/// What zooming with the pointer over a pane does to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ZoomRequest {
    /// Multiply the zoom by this factor (egui's `zoom_delta`).
    By(f32),
    /// Back to 100 %.
    Reset,
}

/// How the chord reads in hover text.
#[cfg(target_os = "macos")]
pub(super) const ZOOM_CHORD: &str = "\u{2318} + scroll or pinch";
#[cfg(not(target_os = "macos"))]
pub(super) const ZOOM_CHORD: &str = "Ctrl + scroll or pinch";

/// The zoom after multiplying `current` by `factor`: within
/// [`ZOOM_MIN`, `ZOOM_MAX`], stopping at 100 % on the way through it.
/// A factor that is not a finite positive number changes nothing.
pub(super) fn zoomed(current: f32, factor: f32) -> f32 {
    if !factor.is_finite() || factor <= 0.0 {
        return current;
    }
    let next = (current * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    let crossed = (current < ZOOM_DEFAULT && next > ZOOM_DEFAULT)
        || (current > ZOOM_DEFAULT && next < ZOOM_DEFAULT);
    if crossed || (next - ZOOM_DEFAULT).abs() < SNAP_TO_DEFAULT {
        ZOOM_DEFAULT
    } else {
        next
    }
}

/// A zoom read back from a session file: within the range, and `None` for
/// 100 % or a value that is not a number (a hand-edited file).
pub(super) fn restored_zoom(saved: f32) -> Option<f32> {
    if !saved.is_finite() {
        return None;
    }
    let zoom = saved.clamp(ZOOM_MIN, ZOOM_MAX);
    (zoom != ZOOM_DEFAULT).then_some(zoom)
}

/// `zoom` as the badge shows it ("125%").
pub(super) fn percent_label(zoom: f32) -> String {
    format!("{}%", (zoom * PERCENT).round() as i32)
}

/// `style`'s text sizes multiplied by `zoom` (text boxes and anything else
/// sized by a text style).
pub(super) fn scale_text_styles(style: &mut egui::Style, zoom: f32) {
    if zoom == ZOOM_DEFAULT {
        return;
    }
    for font in style.text_styles.values_mut() {
        font.size *= zoom;
    }
}

/// Each pane's zoom; panes at 100 % have no entry.
#[derive(Debug, Default)]
pub(super) struct PaneZoom {
    levels: HashMap<u32, f32>,
    /// When a zoom last changed, while that change is not saved yet.
    unsaved_since: Option<Instant>,
}

impl PaneZoom {
    /// `pane`'s zoom.
    pub(super) fn get(&self, pane: u32) -> f32 {
        self.levels.get(&pane).copied().unwrap_or(ZOOM_DEFAULT)
    }

    /// Applies `request` to `pane`.
    pub(super) fn apply(&mut self, pane: u32, request: ZoomRequest) {
        let next = match request {
            ZoomRequest::By(factor) => zoomed(self.get(pane), factor),
            ZoomRequest::Reset => ZOOM_DEFAULT,
        };
        if next != self.get(pane) {
            self.unsaved_since = Some(Instant::now());
        }
        if next == ZOOM_DEFAULT {
            self.levels.remove(&pane);
        } else {
            self.levels.insert(pane, next);
        }
    }

    /// What to save for `pane`: its zoom, or `None` at 100 %.
    pub(super) fn saved(&self, pane: u32) -> Option<f32> {
        self.levels.get(&pane).copied()
    }

    /// Gives `pane` the zoom a session file saved for it.
    pub(super) fn restore(&mut self, pane: u32, saved: f32) {
        match restored_zoom(saved) {
            Some(zoom) => self.levels.insert(pane, zoom),
            None => self.levels.remove(&pane),
        };
    }

    /// Whether a zoom change has settled (unchanged for [`SAVE_SETTLE`] at
    /// `now`) and should be saved; true once per change.
    pub(super) fn take_settled_change(&mut self, now: Instant) -> bool {
        match self.unsaved_since {
            Some(at) if now.saturating_duration_since(at) >= SAVE_SETTLE => {
                self.unsaved_since = None;
                true
            }
            _ => false,
        }
    }

    /// How long until an unsaved zoom change settles, while there is one.
    pub(super) fn save_wait(&self, now: Instant) -> Option<Duration> {
        self.unsaved_since
            .map(|at| SAVE_SETTLE.saturating_sub(now.saturating_duration_since(at)))
    }

    /// Forgets the panes `is_open` says are gone.
    pub(super) fn retain_open(&mut self, is_open: impl Fn(u32) -> bool) {
        if !self.levels.is_empty() {
            self.levels.retain(|pane, _| is_open(*pane));
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.levels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_stays_within_its_range() {
        assert_eq!(zoomed(2.9, 2.0), ZOOM_MAX);
        assert_eq!(zoomed(0.6, 0.5), ZOOM_MIN);
        assert!((zoomed(1.5, 1.2) - 1.8).abs() < 1e-5);
    }

    #[test]
    fn zooming_through_100_percent_stops_there() {
        assert_eq!(zoomed(0.9, 1.3), ZOOM_DEFAULT, "in, across 100 %");
        assert_eq!(zoomed(1.2, 0.7), ZOOM_DEFAULT, "out, across 100 %");
        assert_eq!(zoomed(1.0, 1.02), ZOOM_DEFAULT, "a tiny step snaps back");
        assert!(zoomed(ZOOM_DEFAULT, 1.1) > ZOOM_DEFAULT, "then moves on");
        assert!(zoomed(ZOOM_DEFAULT, 0.9) < ZOOM_DEFAULT);
    }

    #[test]
    fn a_broken_factor_changes_nothing() {
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(zoomed(1.5, bad), 1.5);
        }
    }

    #[test]
    fn the_badge_reads_in_percent() {
        assert_eq!(percent_label(1.25), "125%");
        assert_eq!(percent_label(0.5), "50%");
        assert_eq!(percent_label(1.0 / 3.0), "33%");
    }

    #[test]
    fn text_styles_scale_by_the_zoom() {
        let mut style = egui::Style::default();
        let body = style.text_styles[&egui::TextStyle::Body].size;
        let mono = style.text_styles[&egui::TextStyle::Monospace].size;
        scale_text_styles(&mut style, 2.0);
        assert_eq!(style.text_styles[&egui::TextStyle::Body].size, body * 2.0);
        assert_eq!(
            style.text_styles[&egui::TextStyle::Monospace].size,
            mono * 2.0
        );
    }

    #[test]
    fn each_pane_keeps_its_own_zoom() {
        let mut zoom = PaneZoom::default();
        zoom.apply(1, ZoomRequest::By(1.5));
        assert_eq!(zoom.get(1), 1.5);
        assert_eq!(zoom.get(2), ZOOM_DEFAULT, "other panes untouched");
        zoom.apply(1, ZoomRequest::Reset);
        assert_eq!(zoom.get(1), ZOOM_DEFAULT);
        assert_eq!(zoom.len(), 0, "100 % keeps no entry");
    }

    #[test]
    fn closed_panes_are_forgotten() {
        let mut zoom = PaneZoom::default();
        zoom.apply(1, ZoomRequest::By(1.5));
        zoom.apply(2, ZoomRequest::By(2.0));
        zoom.retain_open(|pane| pane == 2);
        assert_eq!(zoom.get(1), ZOOM_DEFAULT);
        assert_eq!(zoom.get(2), 2.0);
        assert_eq!(zoom.len(), 1);
    }

    #[test]
    fn a_saved_zoom_comes_back_within_range() {
        let mut zoom = PaneZoom::default();
        zoom.apply(4, ZoomRequest::By(1.5));
        assert_eq!(zoom.saved(4), Some(1.5));
        assert_eq!(zoom.saved(5), None, "100 % saves nothing");

        let mut restored = PaneZoom::default();
        restored.restore(9, 1.5);
        assert_eq!(restored.get(9), 1.5);
        restored.restore(9, 99.0);
        assert_eq!(restored.get(9), ZOOM_MAX, "a hand-edited value is clamped");
        for bad in [f32::NAN, f32::INFINITY, ZOOM_DEFAULT] {
            restored.restore(9, bad);
            assert_eq!(restored.get(9), ZOOM_DEFAULT);
            assert_eq!(restored.len(), 0);
        }
        assert_eq!(restored_zoom(0.0), Some(ZOOM_MIN));
    }

    #[test]
    fn a_zoom_change_is_saved_once_it_settles() {
        let mut zoom = PaneZoom::default();
        let start = Instant::now();
        assert!(
            !zoom.take_settled_change(start + SAVE_SETTLE),
            "nothing changed"
        );
        zoom.apply(1, ZoomRequest::By(1.5));
        assert!(!zoom.take_settled_change(Instant::now()), "still zooming");
        assert!(zoom
            .save_wait(Instant::now())
            .is_some_and(|w| w <= SAVE_SETTLE));
        let later = Instant::now() + SAVE_SETTLE;
        assert!(zoom.take_settled_change(later));
        assert!(!zoom.take_settled_change(later), "saved once");
        assert_eq!(zoom.save_wait(later), None);
        zoom.apply(1, ZoomRequest::By(1.0));
        assert!(
            !zoom.take_settled_change(Instant::now() + SAVE_SETTLE),
            "a step that changes nothing saves nothing"
        );
    }
}
