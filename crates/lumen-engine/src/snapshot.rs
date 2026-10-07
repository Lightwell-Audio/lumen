//! Transport state published by the audio thread for the UI, as a seqlock
//! over atomics: the audio thread never waits, and readers always see one
//! consistent snapshot.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering, fence};

use lumen_core::time::Ticks;

/// The transport as of the end of the last processed block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransportSnapshot {
    pub playing: bool,
    /// Timeline sample of the next frame to play.
    pub sample: i64,
    pub ticks: Ticks,
    /// The play marker: where Play starts from.
    pub marker: Ticks,
    pub sample_rate: u32,
    /// Playing inside an enabled punch region.
    pub punching: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Shared {
    /// Odd while the audio thread is writing.
    sequence: AtomicU64,
    playing: AtomicBool,
    sample: AtomicI64,
    ticks: AtomicI64,
    marker: AtomicI64,
    sample_rate: AtomicU32,
    punching: AtomicBool,
}

impl Shared {
    /// Audio thread only (single writer). Never blocks.
    pub(crate) fn publish(&self, s: &TransportSnapshot) {
        let sequence = self.sequence.load(Ordering::Relaxed);
        self.sequence.store(sequence + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.playing.store(s.playing, Ordering::Relaxed);
        self.sample.store(s.sample, Ordering::Relaxed);
        self.ticks.store(s.ticks.0, Ordering::Relaxed);
        self.marker.store(s.marker.0, Ordering::Relaxed);
        self.sample_rate.store(s.sample_rate, Ordering::Relaxed);
        self.punching.store(s.punching, Ordering::Relaxed);
        self.sequence.store(sequence + 2, Ordering::Release);
    }

    /// Any thread. Retries while a write is in progress (a few nanoseconds).
    pub(crate) fn read(&self) -> TransportSnapshot {
        loop {
            let before = self.sequence.load(Ordering::Acquire);
            if before % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let snapshot = TransportSnapshot {
                playing: self.playing.load(Ordering::Relaxed),
                sample: self.sample.load(Ordering::Relaxed),
                ticks: Ticks(self.ticks.load(Ordering::Relaxed)),
                marker: Ticks(self.marker.load(Ordering::Relaxed)),
                sample_rate: self.sample_rate.load(Ordering::Relaxed),
                punching: self.punching.load(Ordering::Relaxed),
            };
            fence(Ordering::Acquire);
            if self.sequence.load(Ordering::Relaxed) == before {
                return snapshot;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn readers_never_see_a_torn_snapshot() {
        let shared = Arc::new(Shared::default());
        let state = |k: i64| TransportSnapshot {
            playing: k % 2 == 0,
            sample: k,
            ticks: Ticks(k * 3),
            marker: Ticks(k * 5),
            sample_rate: k as u32,
            punching: k % 2 == 0,
        };
        shared.publish(&state(0));
        let writer = {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                for k in 1..200_000_i64 {
                    shared.publish(&state(k));
                }
            })
        };
        let mut reads = 0;
        while !writer.is_finished() || reads < 1_000 {
            let s = shared.read();
            assert_eq!(s.ticks.0, s.sample * 3, "{s:?}");
            assert_eq!(s.marker.0, s.sample * 5, "{s:?}");
            assert_eq!(i64::from(s.sample_rate), s.sample, "{s:?}");
            assert_eq!(s.playing, s.sample % 2 == 0, "{s:?}");
            assert_eq!(s.punching, s.playing, "{s:?}");
            reads += 1;
        }
        writer.join().unwrap();
    }
}
