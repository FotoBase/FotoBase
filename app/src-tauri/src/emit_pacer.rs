//! Time-coalescing for repeated events (TASK-01-004 perf run): emit the
//! first event immediately, then at most one per interval — with a trailing
//! flush for events that arrived inside a closed window.
//!
//! Why: the hash backfill finishes a 64-row batch every few ms and announced
//! each batch on `library-changed`. For a 50k library that is 781 emits in
//! ~31 s, each triggering a full 50k-row re-list over IPC — and the
//! frontend's (now leading-edge) throttle still re-lists at most once per
//! window, so the extra emits were pure IPC churn that also kept the JS heap
//! churning into the idle-RAM reading. The pacer cuts 781 emits to ~125.

use std::time::{Duration, Instant};

/// Decide whether an event may go out now. Pure and injectable-clock, so the
/// burst pattern is unit-testable without sleeping.
#[derive(Debug)]
pub struct EmitPacer {
    interval: Duration,
    last: Option<Instant>,
}

impl EmitPacer {
    pub fn new(interval: Duration) -> Self {
        Self { interval, last: None }
    }

    /// `true` when the caller should emit: nothing emitted yet, or the
    /// interval has elapsed since the last emit. Callers that suppress an
    /// event must remember it (see [`EmitPacer::flush_pending`]).
    pub fn fire(&mut self, now: Instant) -> bool {
        let due = self.last.is_none_or(|last| now.duration_since(last) >= self.interval);
        if due {
            self.last = Some(now);
        }
        due
    }

    /// Has the interval elapsed since the last emit (or nothing was ever
    /// emitted)? The caller uses this to flush one straggler event after the
    /// burst ends, so the final state is never left unannounced.
    pub fn flush_pending(&self, now: Instant) -> bool {
        self.last.is_none_or(|last| now.duration_since(last) >= self.interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn first_event_fires_immediately() {
        let mut p = EmitPacer::new(ms(250));
        assert!(p.fire(Instant::now()));
    }

    #[test]
    fn events_inside_the_window_are_suppressed() {
        let t0 = Instant::now();
        let mut p = EmitPacer::new(ms(250));
        assert!(p.fire(t0));
        assert!(!p.fire(t0 + ms(100)));
        assert!(!p.fire(t0 + ms(249)));
    }

    #[test]
    fn window_close_admits_the_next_event() {
        let t0 = Instant::now();
        let mut p = EmitPacer::new(ms(250));
        assert!(p.fire(t0));
        assert!(p.fire(t0 + ms(250)));
    }

    #[test]
    fn flush_pending_mirrors_the_window() {
        let t0 = Instant::now();
        let mut p = EmitPacer::new(ms(250));
        assert!(p.fire(t0));
        // A suppressed event inside the window: not yet flushable.
        assert!(!p.fire(t0 + ms(100)));
        assert!(!p.flush_pending(t0 + ms(100)));
        // After the window closes the straggler may go out.
        assert!(p.flush_pending(t0 + ms(250)));
    }
}
