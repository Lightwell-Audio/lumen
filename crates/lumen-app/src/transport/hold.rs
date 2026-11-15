use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::time::Ticks;
use lumen_engine::Command;

use crate::disk::Cuer;
use crate::disk::plan::Scene;

pub const LONGEST: Duration = Duration::from_millis(150);

pub const READY: Duration = Duration::from_millis(140);

pub struct Gate {
    pub scene: Arc<Scene>,
    pub cuer: Cuer,
}

impl Gate {
    pub fn sample_at(&self, at: Ticks) -> i64 {
        self.scene.sample_at(at)
    }

    pub fn ready(&self, sample: i64) -> bool {
        let rate = i64::from(self.scene.rate().max(1));
        self.scene
            .ready(sample, rate * READY.as_millis() as i64 / 1_000)
    }
}

pub struct Held {
    pub command: Command,
    pub sample: i64,
    pub since: Instant,
}

impl Held {
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
        assert!(held.due(None, since));
        assert!(held.due(None, since + LONGEST));
    }
}
