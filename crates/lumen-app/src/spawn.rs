//! Starting a background thread is a value, not a panic (sweep M7). A system
//! that cannot make one (no thread handles or memory left) gets a message the
//! interface can show, instead of Lumen going down.

use std::io;
use std::thread::{self, JoinHandle};

/// Starts a thread named `name` running `work`, as `thread::Builder` does, and
/// returns the failure rather than panicking when the thread cannot start.
pub(crate) fn spawn_named<T>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<JoinHandle<T>>
where
    T: Send + 'static,
{
    thread::Builder::new().name(name.to_owned()).spawn(work)
}

/// What to tell the user when a worker could not be started: what did not
/// start, why, and the way forward.
pub(crate) fn start_failed(what: &str, error: &io::Error) -> String {
    format!("Could not start {what}: {error}. Try again, or restart if this keeps happening.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_thread_runs_its_work() {
        let thread = spawn_named("lumen-test", || 6 * 7).expect("the test thread starts");
        assert_eq!(thread.join().unwrap(), 42);
    }

    #[test]
    fn a_start_failure_says_what_happened_and_what_to_do() {
        let error = io::Error::new(io::ErrorKind::WouldBlock, "no more threads");
        let words = start_failed("the save", &error);
        assert!(
            words.starts_with("Could not start the save: no more threads."),
            "{words}"
        );
        assert!(
            words.ends_with("Try again, or restart if this keeps happening."),
            "{words}"
        );
    }
}
