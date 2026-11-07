//! How Lumen's tests wait for work on other threads (ADR-0122).
//!
//! A test that waits for a worker (an autosave, a plugin host, a port that
//! opens, the audio device) asks one question: has it happened yet? How long
//! it took is the machine's business, not the test's. A busy CI runner can be
//! ten or a hundred times slower than a developer's machine, so a wait sized
//! for one ("400 polls of 5 ms") fails on the other for no fault of the code.
//!
//! Every such wait goes through here, bounded only by [`patience`]: a guard
//! against a hang, never a measure of speed. A wait that is satisfied returns
//! at once, so a long patience costs nothing while the test passes; it only
//! lengthens how long a hung test takes to say so. CI raises it with
//! [`PATIENCE_ENV`].
//!
//! A test whose claim *is* about time (a block answered within its deadline,
//! a sender keeping its spacing) is a real-time test: it is listed in
//! `.github/timing-tests.txt` and does not gate a merge.
//!
//! `crates/lumen-app/tests/timing_tests.rs` fails on a counted polling loop
//! (`for _ in 0..N` with a sleep and an early exit) in test code.

use std::fmt::Display;
use std::time::{Duration, Instant};

/// Seconds a test waits before it calls the work hung, when set (a
/// decimal number): CI sets it well above [`DEFAULT_PATIENCE`].
pub const PATIENCE_ENV: &str = "LUMEN_TEST_PATIENCE";

/// How long a test waits before it calls the work hung, unless
/// [`PATIENCE_ENV`] says otherwise.
pub const DEFAULT_PATIENCE: Duration = Duration::from_secs(60);

/// How often a wait looks again.
pub const POLL: Duration = Duration::from_millis(2);

/// How long a test waits for work on another thread before it calls the
/// work hung: [`PATIENCE_ENV`] in seconds, or [`DEFAULT_PATIENCE`].
pub fn patience() -> Duration {
    parse_patience(std::env::var(PATIENCE_ENV).ok().as_deref()).unwrap_or(DEFAULT_PATIENCE)
}

fn parse_patience(text: Option<&str>) -> Option<Duration> {
    let seconds: f64 = text?.trim().parse().ok()?;
    (seconds.is_finite() && seconds > 0.0).then(|| Duration::from_secs_f64(seconds))
}

/// A wait's hang guard: started now, ending after [`patience`].
#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    started: Instant,
    patience: Duration,
}

impl Deadline {
    /// A guard that runs out after [`patience`].
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            patience: patience(),
        }
    }

    /// When the guard runs out.
    pub fn at(&self) -> Instant {
        self.started + self.patience
    }

    /// Whether the guard has run out.
    pub fn passed(&self) -> bool {
        self.started.elapsed() >= self.patience
    }

    /// Panics, saying what was waited for, if the guard has run out.
    #[track_caller]
    pub fn check(&self, what: impl Display) {
        if self.passed() {
            panic!(
                "{what}: still not done after {:.0?} (the hang guard; {PATIENCE_ENV} sets it)",
                self.patience
            );
        }
    }
}

impl Default for Deadline {
    fn default() -> Self {
        Self::new()
    }
}

/// Looks at `done` every [`POLL`] until it holds. Panics, saying `what` was
/// waited for, after [`patience`].
#[track_caller]
pub fn wait_until(what: impl Display, mut done: impl FnMut() -> bool) {
    wait_for(what, || done().then_some(()));
}

/// Looks at `found` every [`POLL`] until it gives a value, and returns it.
/// Panics, saying `what` was waited for, after [`patience`].
#[track_caller]
pub fn wait_for<T>(what: impl Display, mut found: impl FnMut() -> Option<T>) -> T {
    let deadline = Deadline::new();
    loop {
        if let Some(value) = found() {
            return value;
        }
        deadline.check(&what);
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patience_reads_seconds_and_ignores_nonsense() {
        assert_eq!(parse_patience(Some("180")), Some(Duration::from_secs(180)));
        assert_eq!(
            parse_patience(Some(" 0.5 ")),
            Some(Duration::from_millis(500))
        );
        for bad in [
            None,
            Some(""),
            Some("soon"),
            Some("0"),
            Some("-3"),
            Some("inf"),
        ] {
            assert_eq!(parse_patience(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_wait_returns_what_it_found_as_soon_as_it_is_there() {
        let mut polls = 0;
        let found = wait_for("the third look", || {
            polls += 1;
            (polls == 3).then_some("here")
        });
        assert_eq!((found, polls), ("here", 3));
    }

    #[test]
    fn a_wait_that_never_ends_names_what_it_waited_for() {
        let guard = Deadline {
            started: Instant::now() - Duration::from_secs(2),
            patience: Duration::from_secs(1),
        };
        let message = std::panic::catch_unwind(|| guard.check("the autosave"))
            .expect_err("a guard that ran out panics");
        let text = message
            .downcast_ref::<String>()
            .expect("a formatted message");
        assert!(text.starts_with("the autosave: still not done"), "{text}");
        assert!(text.contains(PATIENCE_ENV), "{text}");
    }
}
