//! Test seams for recording audio (M9 slice R2): how far a take has got and
//! where a media file is.

use std::path::PathBuf;

use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::App;

impl App {
    /// Frames the take worker has written for `track` so far in the
    /// recording under way (none: that track is not recording audio).
    pub fn audio_take_frames(&self, track: Id) -> Option<u64> {
        let take = self
            .arrangement
            .audio_takes
            .iter()
            .find(|t| t.track == track)?;
        let live = take.live.lock().ok()?;
        Some(live.written)
    }

    /// The frames of the take under way that the arrangement draws: with
    /// Loop on, the pass under way, not everything written.
    pub fn audio_take_drawn_frames(&self, track: Id) -> Option<u64> {
        let take = self
            .arrangement
            .audio_takes
            .iter()
            .find(|t| t.track == track)?;
        let live = take.live.lock().ok()?;
        Some(live.frames)
    }

    /// Recording has stopped and its takes are not placed yet (their files
    /// are being closed).
    pub fn audio_takes_placing(&self) -> bool {
        self.audio_takes_finishing()
    }

    /// The audio tracks whose old clips are silenced for the take under way.
    pub fn audio_tracks_recorded_over(&self) -> Vec<Id> {
        self.lanes.masked_tracks()
    }

    /// The span of `track` silenced in what the engine was last sent, for a
    /// take under way or being placed: from, and until (`None`: no end).
    pub fn audio_recorded_over(&self, track: Id) -> Option<(Ticks, Option<Ticks>)> {
        self.lanes.masked_span(track)
    }

    /// Where the file of media `id` is on disk.
    pub fn media_file(&self, id: Id) -> Option<PathBuf> {
        self.document.media_files.get(&id).cloned()
    }
}

impl App {
    /// The recording offset in use, in milliseconds, and the driver, rate
    /// and buffer it was measured with when the loopback helper set it (M9
    /// slice R2b).
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
