use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone)]
pub struct Onset(Arc<AtomicU32>);

pub(super) const NOTHING_YET: f32 = f32::NAN;

impl Default for Onset {
    fn default() -> Self {
        Self::new()
    }
}

impl Onset {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU32::new(NOTHING_YET.to_bits())))
    }

    pub fn seconds(&self) -> Option<f32> {
        let seconds = f32::from_bits(self.0.load(Ordering::Relaxed));
        seconds.is_finite().then_some(seconds)
    }

    pub(super) fn record(&self, seconds: f32) {
        let held = if seconds.is_finite() && seconds >= 0.0 {
            seconds
        } else {
            NOTHING_YET
        };
        self.0.store(held.to_bits(), Ordering::Relaxed);
    }
}
