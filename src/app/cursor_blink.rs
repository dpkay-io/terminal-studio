//! Terminal cursor blinking, in the style of GTK, VS Code and iTerm2.
//!
//! The cursor switches between shown and hidden, so a repaint is needed only
//! at each switch, not every frame. Any input restarts the cycle with the
//! cursor shown, so it never disappears while the user types. After
//! [`BLINK_IDLE_TIMEOUT`] without input the cursor stays shown and blinking
//! stops, so an idle window schedules no repaints at all.

use std::time::{Duration, Instant};

/// How long each shown/hidden phase lasts (GTK's `gtk-cursor-blink-time` of
/// 1200 ms is one full cycle).
pub(crate) const BLINK_PHASE: Duration = Duration::from_millis(600);

/// Blinking stops after this long without input (GTK's
/// `gtk-cursor-blink-timeout`).
pub(crate) const BLINK_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// Alpha the renderer gets while the cursor is in its shown phase.
pub(crate) const CURSOR_SHOWN: f32 = 1.0;
/// Alpha the renderer gets while the cursor is in its hidden phase.
pub(crate) const CURSOR_HIDDEN: f32 = 0.0;

/// What to draw this frame and when the next blink switch is due.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlinkFrame {
    pub alpha: f32,
    /// `None` when the cursor will not change again without new input.
    pub next_change: Option<Duration>,
}

impl BlinkFrame {
    const STEADY: BlinkFrame = BlinkFrame {
        alpha: CURSOR_SHOWN,
        next_change: None,
    };
}

/// Blink state for the focused terminal. Pure apart from the `Instant`s the
/// caller passes in, so it is fully testable.
pub(crate) struct CursorBlink {
    cycle_start: Instant,
    /// The terminal pane the cycle belongs to; switching panes restarts it.
    pane: Option<u32>,
}

impl CursorBlink {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            cycle_start: now,
            pane: None,
        }
    }

    /// Restart the cycle with the cursor shown.
    pub(crate) fn restart(&mut self, now: Instant) {
        self.cycle_start = now;
    }

    /// Advance to `now` and return what to draw.
    ///
    /// `blinking` is false when blinking is off in settings, the window is not
    /// focused, or no terminal pane has focus. `had_input` is true when this
    /// frame carried keyboard input. `pane` is the focused terminal pane.
    pub(crate) fn frame(
        &mut self,
        now: Instant,
        blinking: bool,
        had_input: bool,
        pane: Option<u32>,
    ) -> BlinkFrame {
        if had_input || pane != self.pane || !blinking {
            self.pane = pane;
            self.restart(now);
        }
        if !blinking {
            return BlinkFrame::STEADY;
        }
        let elapsed = now.saturating_duration_since(self.cycle_start);
        if elapsed >= BLINK_IDLE_TIMEOUT {
            return BlinkFrame::STEADY;
        }
        let phase_nanos = BLINK_PHASE.as_nanos();
        let phase_index = elapsed.as_nanos() / phase_nanos;
        let shown = phase_index % 2 == 0;
        let next_switch = Duration::from_nanos(
            u64::try_from(phase_nanos - elapsed.as_nanos() % phase_nanos).unwrap_or(u64::MAX),
        );
        // The final wake-up lands exactly on the timeout, where the cursor
        // settles shown.
        let until_timeout = BLINK_IDLE_TIMEOUT - elapsed;
        BlinkFrame {
            alpha: if shown { CURSOR_SHOWN } else { CURSOR_HIDDEN },
            next_change: Some(next_switch.min(until_timeout)),
        }
    }
}

/// True when the frame's input should restart the blink cycle.
pub(crate) fn is_blink_restarting_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { pressed: true, .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Ime(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: Option<u32> = Some(1);

    fn started() -> (CursorBlink, Instant) {
        let t0 = Instant::now();
        let mut blink = CursorBlink::new(t0);
        blink.frame(t0, true, false, PANE);
        (blink, t0)
    }

    #[test]
    fn starts_shown_and_switches_at_phase_end() {
        let (mut blink, t0) = started();
        let f = blink.frame(t0, true, false, PANE);
        assert_eq!(f.alpha, CURSOR_SHOWN);
        assert_eq!(f.next_change, Some(BLINK_PHASE));
    }

    #[test]
    fn hidden_during_second_phase() {
        let (mut blink, t0) = started();
        let f = blink.frame(
            t0 + BLINK_PHASE + Duration::from_millis(100),
            true,
            false,
            PANE,
        );
        assert_eq!(f.alpha, CURSOR_HIDDEN);
        assert_eq!(
            f.next_change,
            Some(BLINK_PHASE - Duration::from_millis(100))
        );
    }

    #[test]
    fn shown_again_in_third_phase() {
        let (mut blink, t0) = started();
        let f = blink.frame(t0 + BLINK_PHASE * 2, true, false, PANE);
        assert_eq!(f.alpha, CURSOR_SHOWN);
    }

    #[test]
    fn input_restarts_cycle_shown() {
        let (mut blink, t0) = started();
        let later = t0 + BLINK_PHASE + Duration::from_millis(50);
        assert_eq!(blink.frame(later, true, false, PANE).alpha, CURSOR_HIDDEN);
        let f = blink.frame(later, true, true, PANE);
        assert_eq!(f.alpha, CURSOR_SHOWN);
        assert_eq!(f.next_change, Some(BLINK_PHASE));
    }

    #[test]
    fn switching_panes_restarts_cycle() {
        let (mut blink, t0) = started();
        let later = t0 + BLINK_PHASE + Duration::from_millis(50);
        let f = blink.frame(later, true, false, Some(2));
        assert_eq!(f.alpha, CURSOR_SHOWN);
        assert_eq!(f.next_change, Some(BLINK_PHASE));
    }

    #[test]
    fn stops_blinking_after_idle_timeout() {
        let (mut blink, t0) = started();
        let f = blink.frame(t0 + BLINK_IDLE_TIMEOUT, true, false, PANE);
        assert_eq!(f, BlinkFrame::STEADY);
        let f = blink.frame(t0 + BLINK_IDLE_TIMEOUT * 3, true, false, PANE);
        assert_eq!(f, BlinkFrame::STEADY);
    }

    #[test]
    fn last_wake_up_lands_on_timeout() {
        let (mut blink, t0) = started();
        let near_end = t0 + BLINK_IDLE_TIMEOUT - Duration::from_millis(10);
        let f = blink.frame(near_end, true, false, PANE);
        assert_eq!(f.next_change, Some(Duration::from_millis(10)));
    }

    #[test]
    fn input_after_timeout_resumes_blinking() {
        let (mut blink, t0) = started();
        let idle = t0 + BLINK_IDLE_TIMEOUT * 2;
        assert_eq!(blink.frame(idle, true, false, PANE), BlinkFrame::STEADY);
        let f = blink.frame(idle, true, true, PANE);
        assert_eq!(f.alpha, CURSOR_SHOWN);
        assert_eq!(f.next_change, Some(BLINK_PHASE));
    }

    #[test]
    fn not_blinking_is_steady_and_restarts_on_resume() {
        let (mut blink, t0) = started();
        let later = t0 + BLINK_PHASE + Duration::from_millis(50);
        assert_eq!(blink.frame(later, false, false, PANE), BlinkFrame::STEADY);
        // Focus coming back starts a fresh cycle, cursor shown.
        let f = blink.frame(later + Duration::from_millis(5), true, false, PANE);
        assert_eq!(f.alpha, CURSOR_SHOWN);
        assert_eq!(f.next_change, Some(BLINK_PHASE - Duration::from_millis(5)));
    }

    #[test]
    fn clock_before_cycle_start_is_shown() {
        let t0 = Instant::now() + Duration::from_secs(1);
        let mut blink = CursorBlink::new(t0);
        blink.frame(t0, true, false, PANE);
        let f = blink.frame(t0 - Duration::from_millis(500), true, false, PANE);
        assert_eq!(f.alpha, CURSOR_SHOWN);
    }

    #[test]
    fn restarting_events() {
        use egui::{Event, Key, Modifiers};
        let key = |pressed| Event::Key {
            key: Key::A,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        assert!(is_blink_restarting_event(&key(true)));
        assert!(!is_blink_restarting_event(&key(false)));
        assert!(is_blink_restarting_event(&Event::Text("a".into())));
        assert!(is_blink_restarting_event(&Event::Paste("a".into())));
        assert!(!is_blink_restarting_event(&Event::PointerGone));
    }
}
