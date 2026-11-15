use std::io;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

pub(crate) fn spawn_named<T>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<JoinHandle<T>>
where
    T: Send + 'static,
{
    thread::Builder::new().name(name.to_owned()).spawn(work)
}

pub(crate) fn start_failed(what: &str, error: &io::Error) -> String {
    format!("Could not start {what}: {error}. Try again, or restart if this keeps happening.")
}

pub(crate) fn drop_later<T>(value: T)
where
    T: Send + 'static,
{
    let held = Arc::new(Mutex::new(Some(value)));
    let shared = Arc::clone(&held);
    let spawned = spawn_named("lumen-drop", move || {
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
        lumen_testkit::wait_until("the value to be dropped", || {
            notes.lock().is_ok_and(|notes| !notes.is_empty())
        });
        let dropped = notes.lock().expect("the notes read").clone();
        assert_ne!(dropped[0], caller, "not on the thread that asked");
    }
}
