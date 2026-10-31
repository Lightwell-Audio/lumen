//! Test seams for playing audio from disk (M9 slice R6): how each file
//! plays and what the disk thread reports, which the screen shows only in the
//! audio panel.

use crate::App;
use crate::launch::Launch;
use crate::media::State;

pub use crate::transport::DiskReading;

impl Launch {
    /// Starts with "Play audio from disk" on.
    pub fn playing_from_disk(mut self) -> Self {
        self.settings.playback.from_disk = true;
        self
    }

    /// Makes the disk thread wait `wait` before each page it reads, so
    /// streamed audio arrives late.
    pub fn with_slow_disk(mut self, wait: std::time::Duration) -> Self {
        self.slow_disk = Some(wait);
        self
    }
}

impl App {
    /// Turns "Play audio from disk" on or off, as the audio window's switch
    /// does.
    pub fn set_playing_from_disk(&mut self, on: bool) {
        self.settings.playback.from_disk = on;
        self.settings_changed = Some(std::time::Instant::now());
    }

    /// The loaded files by name, and whether each plays from disk.
    pub fn media_from_disk(&self) -> Vec<(String, bool)> {
        self.lanes
            .library
            .items()
            .iter()
            .filter_map(|item| match &item.state {
                State::Ready(audio) => Some((item.name.clone(), audio.memory().is_none())),
                _ => None,
            })
            .collect()
    }

    /// What the disk thread reports, while audio plays from disk.
    pub fn disk_reading(&self) -> Option<DiskReading> {
        self.transport.disk
    }
}
