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

    /// Whether what plays from `sample` is in memory: the next 50 ms, more
    /// than the clip player needs to start (40 ms).
    pub fn ready(&self, sample: i64) -> bool {
        self.scene.is_empty()
            || (self.cuer.adopted()
                && self
                    .scene
                    .ready(sample, i64::from(self.scene.rate().max(1)) / 20))
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
