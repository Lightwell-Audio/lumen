use std::fmt::Display;
use std::time::{Duration, Instant};

pub const PATIENCE_ENV: &str = "LUMEN_TEST_PATIENCE";

pub const DEFAULT_PATIENCE: Duration = Duration::from_secs(60);

pub const POLL: Duration = Duration::from_millis(2);

pub fn patience() -> Duration {
    parse_patience(std::env::var(PATIENCE_ENV).ok().as_deref()).unwrap_or(DEFAULT_PATIENCE)
}

fn parse_patience(text: Option<&str>) -> Option<Duration> {
    let seconds: f64 = text?.trim().parse().ok()?;
    (seconds.is_finite() && seconds > 0.0).then(|| Duration::from_secs_f64(seconds))
}

#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    started: Instant,
    patience: Duration,
}

impl Deadline {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            patience: patience(),
        }
    }

    pub fn at(&self) -> Instant {
        self.started + self.patience
    }

    pub fn passed(&self) -> bool {
        self.started.elapsed() >= self.patience
    }

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

#[track_caller]
pub fn wait_until(what: impl Display, mut done: impl FnMut() -> bool) {
    wait_for(what, || done().then_some(()));
}

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
