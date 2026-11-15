use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const POLL_STEP: Duration = Duration::from_micros(200);

#[derive(Debug)]
pub(super) struct InFlight(AtomicUsize);

impl InFlight {
    pub(super) const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    pub(super) fn enter(&self) -> Running<'_> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Running(self)
    }

    pub(super) fn idle(&self) -> bool {
        self.0.load(Ordering::SeqCst) == 0
    }

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
