//! The short wait before audio from disk is heard (M9 slice R6, the owner's
//! L A). Play, Continue, Record and a locate while playing are held until the
//! pages the streamed clips play there are in memory, for at most
//! [`LONGEST`], and then sent anyway: MIDI, the metronome and the clips start
//! together, and nothing starts late or out of time. The disk thread is cued
//! to read there first.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::time::Ticks;
use lumen_engine::Command;

use crate::disk::Cuer;
use crate::disk::plan::Scene;

/// The longest a start waits for the disk.
pub const LONGEST: Duration = Duration::from_millis(150);

/// How much of what plays after a start is in memory before it goes: the
/// 40 ms the clip player reads ahead, and 100 ms for the disk thread to read
/// what follows before the player gets there. With only 10 ms over the
/// player's need, a disk thread that was not scheduled for one time slice of
/// a busy machine (15.6 ms on Windows) was late just after a start.
pub const READY: Duration = Duration::from_millis(140);

/// What a start waits for while audio plays from disk: the clip list the
/// disk thread reads for, and a way to cue it.
pub struct Gate {
    pub scene: Arc<Scene>,
    pub cuer: Cuer,
}

impl Gate {
    /// The sample `at` plays at.
    pub fn sample_at(&self, at: Ticks) -> i64 {
        self.scene.sample_at(at)
    }

    /// Whether what plays from `sample` is in memory: the next [`READY`].
    pub fn ready(&self, sample: i64) -> bool {
        let rate = i64::from(self.scene.rate().max(1));
        self.scene
            .ready(sample, rate * READY.as_millis() as i64 / 1_000)
    }
}

/// A start held for the disk.
pub struct Held {
    pub command: Command,
    pub sample: i64,
    pub since: Instant,
}

impl Held {
    /// Whether to send it now: its audio is ready, or it waited long enough.
    pub fn due(&self, gate: Option<&Gate>, now: Instant) -> bool {
        now.duration_since(self.since) >= LONGEST || gate.is_none_or(|g| g.ready(self.sample))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_waits_at_most_150_ms() {
        let since = Instant::now();
        let held = Held {
            command: Command::Play,
            sample: 0,
            since,
        };
        // Without a gate (streaming turned off meanwhile) it goes at once.
        assert!(held.due(None, since));
        assert!(held.due(None, since + LONGEST));
    }
}
