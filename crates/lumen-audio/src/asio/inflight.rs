//! Counts the driver callbacks running now, so a stream is torn down only
//! once none is (sweep M5a).
//!
//! A driver stops delivering callbacks when its stream is stopped, but the
//! last one may still be running: freeing the state under it, or disposing
//! its buffers, would be a use-after-free. The callback counts itself in on
//! entry and out on exit ([`InFlight::enter`]); the teardown waits for the
//! count to reach zero, bounded, before it frees anything. Leaking a state
//! that will not come back is safe; freeing one a callback still reads is not.
//!
//! Nothing here allocates or locks: the callback path is real-time code.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How often a wait looks again. Short, so a callback that ends at once does
/// not hold the teardown for a whole period.
const POLL_STEP: Duration = Duration::from_micros(200);

/// How many callbacks are running.
#[derive(Debug)]
pub(super) struct InFlight(AtomicUsize);

impl InFlight {
    pub(super) const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    /// Counts this callback as running until the returned guard is dropped.
    /// Real-time safe: no lock and no allocation.
    pub(super) fn enter(&self) -> Running<'_> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Running(self)
    }

    /// Whether no callback is running.
    pub(super) fn idle(&self) -> bool {
        self.0.load(Ordering::SeqCst) == 0
    }

    /// Waits, up to `bound`, for every callback to end; false when one is
    /// still running then. Called on the control thread, never from a
    /// callback.
    pub(super) fn wait(&self, bound: Duration) -> bool {
        let started = Instant::now();
        loop {
            if self.idle() {
                return true;
            }
            let left = bound.saturating_sub(started.elapsed());
            if left.is_zero() {
                return self.idle();
            }
            std::thread::sleep(POLL_STEP.min(left));
        }
    }
}

/// One running callback: the count [`InFlight::enter`] added, taken back when
/// it goes out of scope, on every path a callback can leave by.
pub(super) struct Running<'a>(&'a InFlight);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::mpsc;

    #[test]
    fn the_count_follows_the_guards_life() {
        let count = InFlight::new();
        assert!(count.idle());
        let running = count.enter();
        assert!(!count.idle());
        let also = count.enter();
        assert!(!count.idle());
        drop(also);
        assert!(!count.idle(), "one still runs");
        drop(running);
        assert!(count.idle());
    }

    #[test]
    fn a_wait_that_finds_nothing_running_returns_at_once() {
        let count = InFlight::new();
        assert!(count.wait(Duration::from_millis(50)));
    }

    #[test]
    fn a_wait_outlasts_a_callback_that_is_still_running_and_sees_it_end() {
        let count = Arc::new(InFlight::new());
        let (entered, seen) = mpsc::channel();
        let thread = std::thread::spawn({
            let count = Arc::clone(&count);
            move || {
                let _running = count.enter();
                entered.send(()).expect("the test is listening");
                // Long enough that the bounded wait below must give up.
                std::thread::sleep(Duration::from_millis(60));
            }
        });
        seen.recv().expect("the callback reports its start");
        assert!(
            !count.wait(Duration::from_millis(10)),
            "a wait shorter than the callback must say it is still running"
        );
        thread.join().expect("the callback thread ends");
        assert!(
            count.wait(Duration::from_millis(500)),
            "a callback that has ended leaves nothing in flight"
        );
    }

    #[test]
    fn a_panicking_callback_still_counts_out() {
        let count = Arc::new(InFlight::new());
        let thread = std::thread::spawn({
            let count = Arc::clone(&count);
            move || {
                let _running = count.enter();
                panic!("as a user callback might");
            }
        });
        assert!(thread.join().is_err(), "the panic came back out");
        assert!(count.idle(), "the guard ended the count on the way out");
    }
}
