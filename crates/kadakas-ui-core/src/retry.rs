//! Retry with a back-off, for a save that failed (a settings file that cannot be written).
//!
//! A program that keeps its settings in memory writes the changes in the frames after they were
//! made. When the write fails, the changes stay unsaved, the failure is shown, and the write is
//! tried again after a pause that grows: 1, 2, 4, ... seconds, at most 30. A success starts over.

/// The state of the retries of one save.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Retry {
    failures: u32,
    next_try: f64,
}

/// The longest pause between two tries, in seconds.
pub const MAX_PAUSE: f64 = 30.0;

impl Retry {
    /// True when a try may be made at time `now` (seconds).
    pub fn due(&self, now: f64) -> bool {
        now >= self.next_try
    }

    /// A try failed at `now`: the next one is due later.
    pub fn failed(&mut self, now: f64) {
        self.failures = self.failures.saturating_add(1);
        self.next_try = now + self.pause();
    }

    /// A try worked.
    pub fn succeeded(&mut self) {
        *self = Retry::default();
    }

    /// The pause after the failures so far.
    pub fn pause(&self) -> f64 {
        2f64.powi(self.failures.saturating_sub(1).min(10) as i32)
            .min(MAX_PAUSE)
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pause_doubles_up_to_the_limit_and_a_success_starts_over() {
        let mut r = Retry::default();
        assert!(r.due(0.0));
        let mut now = 0.0;
        let mut pauses = Vec::new();
        for _ in 0..8 {
            r.failed(now);
            assert!(!r.due(now + r.pause() - 0.001));
            assert!(r.due(now + r.pause()));
            pauses.push(r.pause());
            now += r.pause();
        }
        assert_eq!(pauses, [1.0, 2.0, 4.0, 8.0, 16.0, 30.0, 30.0, 30.0]);
        assert_eq!(r.failures(), 8);
        r.succeeded();
        assert_eq!((r.failures(), r.pause()), (0, 1.0));
        assert!(r.due(0.0));
        // Very many failures do not overflow.
        for _ in 0..100 {
            r.failed(0.0);
        }
        assert_eq!(r.pause(), MAX_PAUSE);
    }
}
