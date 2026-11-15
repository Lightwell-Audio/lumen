use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const TAP_QUEUE: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Played {
    pub source: u8,
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
}

pub(crate) const NOTE_QUEUE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heard {
    pub source: u8,
    pub channel: u8,
    pub number: u8,
    pub value: u8,
    pub played: u64,
}

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

pub struct Tap {
    pub(crate) rx: rtrb::Consumer<Heard>,
    pub(crate) notes: rtrb::Consumer<Played>,
    pub(crate) shared: Arc<Shared>,
}

impl Tap {
    pub fn drain(&mut self, mut each: impl FnMut(Heard)) {
        while let Ok(heard) = self.rx.pop() {
            each(heard);
        }
    }

    pub fn drain_notes(&mut self, mut each: impl FnMut(Played)) {
        while let Ok(played) = self.notes.pop() {
            each(played);
        }
    }

    pub fn take(&self, controllers: impl IntoIterator<Item = (u8, u8)>) {
        self.shared.taken.set(controllers);
    }

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
        assert!(!shared.taken.contains(16, 0));
        assert!(!shared.taken.contains(0, 200));
        shared.taken.set([(1, 1)]);
        assert!(!shared.taken.contains(0, 7));
        assert!(shared.taken.contains(1, 1));
    }
}
