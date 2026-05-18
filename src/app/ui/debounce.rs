use std::time::{Duration, Instant};

/// A simple debouncer that fires once after the query has been stable for `delay`.
///
/// Usage:
/// ```ignore
/// // On every frame where the query might have changed:
/// debouncer.update(&current_query);
///
/// // Later in the frame, check if it's time to fire:
/// if debouncer.ready() {
///     // execute the search / action
/// }
/// ```
pub(crate) struct Debouncer {
    last_query: String,
    trigger_at: Option<Instant>,
    delay: Duration,
}

impl Debouncer {
    pub fn new(delay: Duration) -> Self {
        Self {
            last_query: String::new(),
            trigger_at: None,
            delay,
        }
    }

    /// Call every frame with the current query string. If the query changed
    /// since the last call, the debounce timer resets.
    pub fn update(&mut self, current_query: &str) {
        if current_query != self.last_query {
            self.trigger_at = Some(Instant::now());
            self.last_query = current_query.to_owned();
        }
    }

    /// Returns `true` once when the debounce delay has elapsed since the last
    /// query change, then resets. Returns `false` while waiting or if no
    /// change has been registered.
    pub fn ready(&mut self) -> bool {
        if let Some(t) = self.trigger_at {
            if t.elapsed() >= self.delay {
                self.trigger_at = None;
                return true;
            }
        }
        false
    }

    /// Reset all state (query and timer). Call when the search is dismissed.
    pub fn reset(&mut self) {
        self.last_query.clear();
        self.trigger_at = None;
    }

    /// Returns `true` while a trigger is pending (timer started but not yet elapsed).
    /// Useful for deciding whether to request a repaint.
    pub fn pending(&self) -> bool {
        self.trigger_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_after_delay() {
        let mut d = Debouncer::new(Duration::from_millis(0));
        d.update("hello");
        // With a 0ms delay it should be ready immediately
        assert!(d.ready());
        // And not ready again until the query changes
        assert!(!d.ready());
    }

    #[test]
    fn does_not_fire_without_change() {
        let mut d = Debouncer::new(Duration::from_millis(0));
        assert!(!d.ready());
    }

    #[test]
    fn reset_clears_state() {
        let mut d = Debouncer::new(Duration::from_millis(0));
        d.update("hello");
        d.reset();
        assert!(!d.ready());
        assert!(d.last_query.is_empty());
    }
}
