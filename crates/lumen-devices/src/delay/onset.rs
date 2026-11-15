//! The interface's end of Parallax's repeat clock (OR-8): how long it is since
//! sound last entered the delay's lines. The audio thread writes it once a
//! block, the interface reads it once a frame, and neither waits for the
//! other.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The interface's end of Parallax's repeat clock: how long it is since sound
/// last entered the delay's lines, which is where the card's picture starts
/// its trail (OR-8).
///
/// The audio side [`record`](Onset::record)s the number at the end of every
/// block (one atomic store) and the interface [`seconds`](Onset::seconds) it
/// once a frame. No lock, no queue, no allocation after the handle is made: a
/// slow interface only reads a value a block old.
#[derive(Debug, Clone)]
pub struct Onset(Arc<AtomicU32>);

/// The number a clock that has heard nothing yet holds: not a time.
pub(super) const NOTHING_YET: f32 = f32::NAN;

impl Default for Onset {
    fn default() -> Self {
        Self::new()
    }
}

impl Onset {
    /// A clock nothing is written to yet. Allocates once: build it on the
    /// interface side.
    pub fn new() -> Self {
        Self(Arc::new(AtomicU32::new(NOTHING_YET.to_bits())))
    }

    /// Seconds from the last sound entering the lines to the end of the last
    /// block, or `None` before any sound has entered (and after a reset).
    pub fn seconds(&self) -> Option<f32> {
        let seconds = f32::from_bits(self.0.load(Ordering::Relaxed));
        seconds.is_finite().then_some(seconds)
    }

    /// Records where the sound that entered the lines at the clock's zero is
    /// now. Real-time safe.
    pub(super) fn record(&self, seconds: f32) {
        let held = if seconds.is_finite() && seconds >= 0.0 {
            seconds
        } else {
            NOTHING_YET
        };
        self.0.store(held.to_bits(), Ordering::Relaxed);
    }
}
