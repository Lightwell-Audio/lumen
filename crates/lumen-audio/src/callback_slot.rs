use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, Instant};

use crate::types::AudioCallback;

const WAIT_STEP: Duration = Duration::from_millis(1);

pub(crate) struct CallbackSlot {
    inner: AtomicPtr<Box<dyn AudioCallback>>,
}

impl CallbackSlot {
    pub(crate) fn new(callback: Box<dyn AudioCallback>) -> Self {
        Self {
            inner: AtomicPtr::new(Box::into_raw(Box::new(callback))),
        }
    }

    pub(crate) fn with<R>(&self, run: impl FnOnce(&mut dyn AudioCallback) -> R) -> Option<R> {
        let raw = self.inner.swap(ptr::null_mut(), Ordering::Acquire);
        if raw.is_null() {
            return None;
        }
        let _back = Return { slot: self, raw };
        Some(run(unsafe { &mut **raw }))
    }

    pub(crate) fn take(&self) -> Option<Box<dyn AudioCallback>> {
        let raw = self.inner.swap(ptr::null_mut(), Ordering::Acquire);
        if raw.is_null() {
            return None;
        }
        Some(unsafe { *Box::from_raw(raw) })
    }

    pub(crate) fn take_waiting(&self, bound: Duration) -> Option<Box<dyn AudioCallback>> {
        let started = Instant::now();
        loop {
            if let Some(callback) = self.take() {
                return Some(callback);
            }
            let left = bound.saturating_sub(started.elapsed());
            if left.is_zero() {
                return None;
            }
            std::thread::sleep(WAIT_STEP.min(left));
        }
    }
}

impl Drop for CallbackSlot {
    fn drop(&mut self) {
        let raw = *self.inner.get_mut();
        if raw.is_null() {
            return;
        }
        drop(unsafe { Box::from_raw(raw) });
    }
}

struct Return<'a> {
    slot: &'a CallbackSlot,
    raw: *mut Box<dyn AudioCallback>,
}

impl Drop for Return<'_> {
    fn drop(&mut self) {
        self.slot.inner.store(self.raw, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;
    use std::sync::Arc;
    use std::sync::mpsc;

    use crate::types::Block;

    fn block(outputs: &mut [f32]) -> Block<'_> {
        Block {
            sample_rate: 48_000,
            frames: outputs.len(),
            inputs: &[],
            outputs,
            input_channels: 0,
            output_channels: 1,
            position: None,
        }
    }

    #[test]
    fn a_block_runs_without_a_lock_or_an_allocation() {
        assert!(
            lumen_rt::guard_is_active(),
            "the guarded allocator is this test binary's"
        );
        let slot = CallbackSlot::new(Box::new(|block: &mut Block<'_>| {
            block.outputs.fill(1.0);
        }));
        let mut outputs = [0.0_f32; 8];
        let (_, allocated) = lumen_rt::count_rt_violations(|| {
            for _ in 0..4 {
                let mut block = block(&mut outputs);
                assert!(
                    slot.with(|callback| callback.process(&mut block)).is_some(),
                    "a free slot runs the block"
                );
            }
        });
        assert_eq!(allocated, 0, "filling a block allocates nothing");
        assert!(outputs.iter().all(|&x| x == 1.0));
    }

    #[test]
    fn a_block_that_is_running_is_not_waited_for() {
        let slot = CallbackSlot::new(Box::new(|_: &mut Block<'_>| {}));
        let nested = Cell::new(false);
        assert!(
            slot.with(|_| nested.set(slot.with(|_| ()).is_none()))
                .is_some()
        );
        assert!(nested.get(), "the second block found the slot empty");
        assert!(
            slot.with(|_| ()).is_some(),
            "the callback is back for the next block"
        );
    }

    #[test]
    fn taking_the_callback_waits_for_a_running_block() {
        let slot = Arc::new(CallbackSlot::new(Box::new(|_: &mut Block<'_>| {})));
        let (entered, running) = mpsc::channel();
        let (leave, left) = mpsc::channel::<()>();
        let thread = std::thread::spawn({
            let slot = Arc::clone(&slot);
            move || {
                slot.with(|_| {
                    entered.send(()).expect("the test is listening");
                    left.recv().expect("the test lets the block end");
                });
            }
        });
        running.recv().expect("the block is running");
        assert!(
            slot.take_waiting(Duration::from_millis(20)).is_none(),
            "a block still running outlasts a short wait"
        );
        leave.send(()).expect("the block can end");
        thread.join().expect("the block's thread ends");
        let callback = slot
            .take_waiting(Duration::from_secs(1))
            .expect("the callback comes back");
        assert!(slot.with(|_| ()).is_none(), "and it is taken once");
        drop(callback);
    }

    #[test]
    fn a_panicking_block_leaves_the_callback_in_the_slot() {
        let slot = CallbackSlot::new(Box::new(|_: &mut Block<'_>| {}));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            slot.with(|_| panic!("a block that dies"));
        }));
        assert!(result.is_err(), "the panic came back out");
        assert!(
            slot.with(|_| ()).is_some(),
            "the callback is back after the block died"
        );
        assert!(slot.take().is_some());
        assert!(slot.with(|_| ()).is_none(), "a taken slot runs nothing");
    }

    #[test]
    fn dropping_the_slot_frees_the_callback() {
        let slot = CallbackSlot::new(Box::new(|_: &mut Block<'_>| {}));
        assert!(slot.with(|_| ()).is_some());
        drop(slot);
    }
}
