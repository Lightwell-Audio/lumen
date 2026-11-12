//! The tempo, meter and sample-rate maps in use: swapping in a new timeline,
//! and keeping the playhead and the regions on their musical positions.

use lumen_core::time::Ticks;

use crate::declick::fade_frames;
use crate::source::{Jump, Source};

use super::{Engine, Region};

impl<S: Source> Engine<S> {
    pub(super) fn swap_timeline(&mut self, channels: usize) {
        // One swap per block, and only when the old timeline has a home.
        if self.stash.is_some() {
            return;
        }
        let Ok(incoming) = self.timelines.pop() else {
            return;
        };
        let at = self.ticks_now();
        let ahead = self.timeline.ticks_at(self.ahead, self.sample_rate);
        // If the render position moves (a tempo change before it),
        // crossfade from what the old timeline would have played, as every
        // jump does.
        let rate = self.sample_rate;
        if self.lookahead == 0 && incoming.sample_at(ahead, rate) != self.ahead {
            self.capture_tail(channels);
        }
        let timeline = &self.timeline;
        let convert = |s| incoming.sample_at(timeline.ticks_at(s, rate), rate);
        self.playhead
            .reposition(incoming.sample_at(at, rate), convert);
        let old = std::mem::replace(&mut self.timeline, incoming);
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
        self.reposition(at, ahead);
    }

    pub(super) fn retry_stash(&mut self) {
        if let Some(old) = self.stash.take()
            && let Err(rtrb::PushError::Full(old)) = self.retired.push(old)
        {
            self.stash = Some(old);
        }
    }

    /// After a tempo or rate change: the playhead at `at` and the render
    /// position at `ahead`, the same musical positions in new samples.
    /// When the render position is the same sample (an unchanged timeline,
    /// or a tempo change after it), the audio carries on untouched. Call
    /// after `Playhead::reposition`.
    pub(super) fn reposition(&mut self, at: Ticks, ahead: Ticks) {
        self.position = self.sample_at(at);
        let ahead = self.sample_at(ahead);
        let moved = ahead != self.ahead;
        self.ahead = ahead;
        self.refresh_regions();
        if self.playing && moved {
            self.locate_source(Jump::Reposition);
            if self.lookahead == 0 {
                self.declick.fade_in(fade_frames(self.sample_rate));
            }
        }
    }

    pub(super) fn refresh_regions(&mut self) {
        self.loop_samples = self.region_samples(self.loop_region);
        self.punch_samples = self.region_samples(self.punch);
    }

    fn region_samples(&self, region: Region) -> Option<(i64, i64)> {
        region
            .active()
            .then(|| (self.sample_at(region.start), self.sample_at(region.end)))
            .filter(|(start, end)| end > start)
    }
}
