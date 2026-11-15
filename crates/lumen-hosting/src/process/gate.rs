use super::*;

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

    pub(super) fn permit(&self) -> Permit<'_> {
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        while *free == 0 {
            free = self.freed.wait(free).unwrap_or_else(|e| e.into_inner());
        }
        *free -= 1;
        Permit(self)
    }
}

pub(super) struct Permit<'a>(&'a Gate);

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.0.freed.notify_one();
    }
}

pub(super) fn catching(work: impl FnOnce(), on_panic: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).is_err() {
        on_panic();
    }
}
