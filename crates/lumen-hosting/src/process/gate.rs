//! How many plugins load at once, and a thread that fails.

use super::*;

/// A permit to load: only a few plugins load at once, as the scanner's.
pub(crate) struct Gate {
    free: Mutex<usize>,
    freed: Condvar,
}

impl Gate {
    pub(crate) fn new(permits: usize) -> Arc<Self> {
        Arc::new(Self {
            free: Mutex::new(permits),
            freed: Condvar::new(),
        })
    }

    /// Waits for a permit; it goes back when the returned value is dropped,
    /// however the load ends (a panic included).
    pub(super) fn permit(&self) -> Permit<'_> {
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        while *free == 0 {
            free = self.freed.wait(free).unwrap_or_else(|e| e.into_inner());
        }
        *free -= 1;
        Permit(self)
    }
}

/// A load's place at the gate.
pub(super) struct Permit<'a>(&'a Gate);

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.0.freed.notify_one();
    }
}

/// Runs `work`; if it panics, `on_panic` runs instead, so a device's thread
/// that fails says so rather than leaving the device "loading" for good.
pub(super) fn catching(work: impl FnOnce(), on_panic: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).is_err() {
        on_panic();
    }
}
