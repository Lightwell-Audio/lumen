use std::path::PathBuf;

use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::App;

impl App {
    pub fn audio_take_frames(&self, track: Id) -> Option<u64> {
        let take = self
            .arrangement
            .audio_takes
            .iter()
            .find(|t| t.track == track)?;
        let live = take.live.lock().ok()?;
        Some(live.written)
    }

    pub fn audio_take_drawn_frames(&self, track: Id) -> Option<u64> {
        let take = self
            .arrangement
            .audio_takes
            .iter()
            .find(|t| t.track == track)?;
        let live = take.live.lock().ok()?;
        Some(live.frames)
    }

    pub fn audio_takes_placing(&self) -> bool {
        self.audio_takes_finishing()
    }

    pub fn audio_tracks_recorded_over(&self) -> Vec<Id> {
        self.lanes.masked_tracks()
    }

    pub fn audio_recorded_over(&self, track: Id) -> Option<(Ticks, Option<Ticks>)> {
        self.lanes.masked_span(track)
    }

    pub fn media_file(&self, id: Id) -> Option<PathBuf> {
        self.document.media_files.get(&id).cloned()
    }
}

impl App {
    pub fn record_offset(&self) -> (f32, Option<(String, u32, u32)>) {
        (
            self.settings.record.offset_ms,
            self.settings
                .offset_measured
                .as_ref()
                .map(|w| (w.driver.clone(), w.sample_rate, w.buffer_frames)),
        )
    }
}
