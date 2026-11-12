//! Starting a background thread is a value, not a panic (sweep M7). A system
//! that cannot make one (no thread handles or memory left) gets a message the
//! interface can show, instead of Lumen going down.

use std::io;
use std::sync::{Arc, Mutex};
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

/// Drops `value` on a thread of its own, for a value whose drop waits on
/// another thread: the streaming state joins its disk thread, which may be
/// in a page read (sweep M4d). Dropped on the calling thread instead when no
/// thread can start, which costs the wait but never the value.
pub(crate) fn drop_later<T>(value: T)
where
    T: Send + 'static,
{
    let held = Arc::new(Mutex::new(Some(value)));
    let shared = Arc::clone(&held);
    let spawned = spawn_named("lumen-drop", move || {
        // Taken and dropped here, on this thread.
        if let Ok(mut slot) = shared.lock() {
            slot.take();
        }
    });
    if spawned.is_err()
        && let Ok(mut slot) = held.lock()
    {
        slot.take();
    }
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

    /// A value that notes which thread dropped it.
    struct Notes(Arc<Mutex<Vec<thread::ThreadId>>>);

    impl Drop for Notes {
        fn drop(&mut self) {
            if let Ok(mut notes) = self.0.lock() {
                notes.push(thread::current().id());
            }
        }
    }

    #[test]
    fn a_value_whose_drop_waits_is_dropped_on_a_thread_of_its_own() {
        let notes = Arc::new(Mutex::new(Vec::new()));
        let caller = thread::current().id();
        drop_later(Notes(Arc::clone(&notes)));
        // The frame goes on at once; the drop happens on the worker.
        lumen_testkit::wait_until("the value to be dropped", || {
            notes.lock().is_ok_and(|notes| !notes.is_empty())
        });
        let dropped = notes.lock().expect("the notes read").clone();
        assert_ne!(dropped[0], caller, "not on the thread that asked");
    }
}
