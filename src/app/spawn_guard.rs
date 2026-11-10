//! Runaway-spawn protection for terminal sessions.
//!
//! At most [`MAX_SPAWNS_IN_WINDOW`] sessions start per [`SPAWN_RATE_WINDOW`].
//! A window in which spawns were refused is one strike, however many
//! attempts it refused, so holding a new-tab shortcut (key repeat) costs one
//! strike, not dozens. Strikes clear after a full window without a refusal,
//! so separate bursts never add up. Only a loop that keeps spawning through
//! [`STRIKES_TO_TRIP`] consecutive windows trips the breaker, which then
//! refuses every spawn until the app restarts.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(crate) const SPAWN_RATE_WINDOW: Duration = Duration::from_secs(5);
pub(crate) const MAX_SPAWNS_IN_WINDOW: usize = 5;
pub(crate) const STRIKES_TO_TRIP: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpawnDecision {
    Allow,
    /// Too many spawns in this window; `strikes` of [`STRIKES_TO_TRIP`].
    RateLimited {
        strikes: usize,
        new_strike: bool,
    },
    /// The breaker tripped on this attempt or earlier.
    Tripped {
        just_now: bool,
    },
}

#[derive(Default)]
pub(crate) struct SpawnGuard {
    recent: VecDeque<Instant>,
    strikes: usize,
    last_strike: Option<Instant>,
    last_refusal: Option<Instant>,
    tripped: bool,
}

impl SpawnGuard {
    /// Decide whether a session may spawn at `now`, recording it if allowed.
    pub(crate) fn check(&mut self, now: Instant) -> SpawnDecision {
        if self.tripped {
            return SpawnDecision::Tripped { just_now: false };
        }
        let elapsed_since = |t: Instant| now.saturating_duration_since(t);
        self.recent
            .retain(|t| elapsed_since(*t) < SPAWN_RATE_WINDOW);
        if self
            .last_refusal
            .is_some_and(|t| elapsed_since(t) >= SPAWN_RATE_WINDOW)
        {
            self.strikes = 0;
            self.last_strike = None;
            self.last_refusal = None;
        }
        if self.recent.len() < MAX_SPAWNS_IN_WINDOW {
            self.recent.push_back(now);
            return SpawnDecision::Allow;
        }
        self.last_refusal = Some(now);
        let new_strike = self
            .last_strike
            .map_or(true, |t| elapsed_since(t) >= SPAWN_RATE_WINDOW);
        if new_strike {
            self.strikes += 1;
            self.last_strike = Some(now);
        }
        if self.strikes >= STRIKES_TO_TRIP {
            self.tripped = true;
            return SpawnDecision::Tripped { just_now: true };
        }
        SpawnDecision::RateLimited {
            strikes: self.strikes,
            new_strike,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(100);

    fn fill_window(guard: &mut SpawnGuard, start: Instant) {
        for i in 0..MAX_SPAWNS_IN_WINDOW {
            assert_eq!(guard.check(start + TICK * i as u32), SpawnDecision::Allow);
        }
    }

    #[test]
    fn allows_up_to_the_limit() {
        let mut guard = SpawnGuard::default();
        fill_window(&mut guard, Instant::now());
    }

    #[test]
    fn refuses_over_the_limit_with_one_strike_per_window() {
        let mut guard = SpawnGuard::default();
        let t0 = Instant::now();
        fill_window(&mut guard, t0);
        let burst = t0 + TICK * 10;
        assert_eq!(
            guard.check(burst),
            SpawnDecision::RateLimited {
                strikes: 1,
                new_strike: true
            }
        );
        // Key repeat: many more refused attempts in the same window.
        for i in 1..50 {
            assert_eq!(
                guard.check(burst + Duration::from_millis(30 * i)),
                SpawnDecision::RateLimited {
                    strikes: 1,
                    new_strike: false
                }
            );
        }
    }

    #[test]
    fn window_slides_so_spawning_resumes() {
        let mut guard = SpawnGuard::default();
        let t0 = Instant::now();
        fill_window(&mut guard, t0);
        assert_eq!(guard.check(t0 + SPAWN_RATE_WINDOW), SpawnDecision::Allow);
    }

    #[test]
    fn separate_bursts_never_trip() {
        let mut guard = SpawnGuard::default();
        let mut t = Instant::now();
        for _ in 0..10 {
            fill_window(&mut guard, t);
            assert!(matches!(
                guard.check(t + TICK * 6),
                SpawnDecision::RateLimited { strikes: 1, .. }
            ));
            // A pause of two windows: the slots free up and the strike clears.
            t += SPAWN_RATE_WINDOW * 2 + TICK * 6;
        }
    }

    #[test]
    fn runaway_loop_trips_after_consecutive_windows() {
        let mut guard = SpawnGuard::default();
        let t0 = Instant::now();
        let mut allowed = 0;
        let mut tripped_at = None;
        // A loop attempting a spawn every frame (~16 ms) for 30 s.
        for frame in 0..(30_000 / 16) {
            let now = t0 + Duration::from_millis(16 * frame);
            match guard.check(now) {
                SpawnDecision::Allow => allowed += 1,
                SpawnDecision::Tripped { just_now: true } => tripped_at = Some(now),
                _ => {}
            }
        }
        let tripped_at = tripped_at.expect("breaker should trip");
        assert!(tripped_at - t0 < SPAWN_RATE_WINDOW * STRIKES_TO_TRIP as u32);
        assert_eq!(allowed, MAX_SPAWNS_IN_WINDOW * STRIKES_TO_TRIP);
    }

    #[test]
    fn tripped_refuses_forever() {
        let mut guard = SpawnGuard::default();
        let t0 = Instant::now();
        for frame in 0..2_000u32 {
            guard.check(t0 + Duration::from_millis(16) * frame);
        }
        assert_eq!(
            guard.check(t0 + Duration::from_secs(3600)),
            SpawnDecision::Tripped { just_now: false }
        );
    }

    #[test]
    fn clock_going_backwards_does_not_panic() {
        let mut guard = SpawnGuard::default();
        let t0 = Instant::now() + Duration::from_secs(10);
        fill_window(&mut guard, t0);
        let _ = guard.check(t0 - Duration::from_secs(5));
    }
}
