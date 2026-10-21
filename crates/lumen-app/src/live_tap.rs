//! Controller messages for MIDI learn and mapped controls (M7 slice A5, D8).
//!
//! The live feed copies each controller message it takes from an input into
//! a bounded queue the interface drains every frame, and leaves out of the
//! engine's input those controllers a mapping moves: they are not notes for a
//! track to record. The audio thread knows no mappings, only which controller
//! numbers the interface says are taken (one bit each, set by the interface).
//! A message the queue has no room for is counted, never waited for.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Controller messages waiting for the interface.
pub(crate) const TAP_QUEUE: usize = 1024;

/// One controller message an input played.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heard {
    /// The input's slot.
    pub source: u8,
    /// The MIDI channel, 0 to 15.
    pub channel: u8,
    /// The controller number, 0 to 127.
    pub number: u8,
    /// The value, 0 to 127.
    pub value: u8,
    /// The device frame it was played at (the clock's, before the feed's
    /// block of delay).
    pub played: u64,
}

/// Which controller numbers on which channels a mapping moves: 2,048 bits.
#[derive(Debug)]
pub(crate) struct Taken {
    words: [AtomicU64; 32],
}

impl Taken {
    fn new() -> Self {
        Self {
            words: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn slot(channel: u8, number: u8) -> Option<(usize, u64)> {
        (channel < 16 && number < 128).then(|| {
            let bit = usize::from(channel) * 128 + usize::from(number);
            (bit / 64, 1 << (bit % 64))
        })
    }

    /// Whether controller `number` on `channel` is taken.
    pub(crate) fn contains(&self, channel: u8, number: u8) -> bool {
        Self::slot(channel, number)
            .is_some_and(|(word, bit)| self.words[word].load(Ordering::Relaxed) & bit != 0)
    }

    fn set(&self, taken: impl IntoIterator<Item = (u8, u8)>) {
        let mut words = [0_u64; 32];
        for (channel, number) in taken {
            if let Some((word, bit)) = Self::slot(channel, number) {
                words[word] |= bit;
            }
        }
        for (shared, new) in self.words.iter().zip(words) {
            shared.store(new, Ordering::Relaxed);
        }
    }
}

/// What the feed and the interface share.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) taken: Taken,
    pub(crate) dropped: AtomicU64,
}

impl Shared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            taken: Taken::new(),
            dropped: AtomicU64::new(0),
        })
    }
}

/// The interface's side: the controller messages heard, and which numbers
/// the mappings take.
pub struct Tap {
    pub(crate) rx: rtrb::Consumer<Heard>,
    pub(crate) shared: Arc<Shared>,
}

impl Tap {
    /// Gives each message heard since the last call, oldest first.
    pub fn drain(&mut self, mut each: impl FnMut(Heard)) {
        while let Ok(heard) = self.rx.pop() {
            each(heard);
        }
    }

    /// Says which controllers (channel, number) mappings move: the engine is
    /// not given those.
    pub fn take(&self, controllers: impl IntoIterator<Item = (u8, u8)>) {
        self.shared.taken.set(controllers);
    }

    /// How many messages there was no room for, since the start.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taken_controllers_are_remembered_by_channel_and_number() {
        let shared = Shared::new();
        shared.taken.set([(0, 7), (15, 127), (9, 74)]);
        assert!(shared.taken.contains(0, 7));
        assert!(shared.taken.contains(15, 127));
        assert!(shared.taken.contains(9, 74));
        assert!(!shared.taken.contains(1, 7));
        assert!(!shared.taken.contains(0, 8));
        // Out of MIDI's range is never taken, and does not panic.
        assert!(!shared.taken.contains(16, 0));
        assert!(!shared.taken.contains(0, 200));
        // A new list replaces the old.
        shared.taken.set([(1, 1)]);
        assert!(!shared.taken.contains(0, 7));
        assert!(shared.taken.contains(1, 1));
    }
}
