use crate::App;
use crate::launch::Launch;
use crate::media::State;

pub use crate::transport::DiskReading;

impl Launch {
    pub fn playing_from_disk(mut self) -> Self {
        self.settings.playback.from_disk = true;
        self
    }

    pub fn with_slow_disk(mut self, wait: std::time::Duration) -> Self {
        self.slow_disk = Some(wait);
        self
    }
}

impl App {
    pub fn set_playing_from_disk(&mut self, on: bool) {
        self.settings.playback.from_disk = on;
        self.settings_changed = Some(std::time::Instant::now());
    }

    pub fn plays_from_disk(&self) -> bool {
        self.settings.playback.from_disk
    }

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

    pub fn disk_reading(&self) -> Option<DiskReading> {
        self.transport.disk
    }
}
