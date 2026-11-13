//! Test seams for Shutter, the built-in sampler (M8 slice B5): what the
//! project holds for it and what the interface has decoded, which a frame
//! shows only as pixels.

use lumen_project::model::{Id, Sampler};

use crate::App;

impl App {
    /// The sound the device holds: its face, regions, zones and pads.
    pub fn sampler_content(&self, device: Id) -> Option<Sampler> {
        self.document.sampler_sound(device).cloned()
    }

    /// Every media file the device's sound names.
    pub fn sampler_media(&self, device: Id) -> Vec<Id> {
        self.document
            .sampler_sound(device)
            .map(lumen_project::model::Sampler::media)
            .unwrap_or_default()
    }

    /// Whether the sample of `media` is decoded and ready to play.
    pub fn sample_loaded(&self, media: Id) -> bool {
        self.lanes.samples.get(media).is_some()
    }

    /// How many bytes of samples the library holds.
    pub fn sample_memory(&self) -> usize {
        self.lanes.samples.memory()
    }

    /// Cycles the chosen sampler's face as its command does, for a test that
    /// needs a face without pressing the card.
    pub fn cycle_sampler_face(&mut self) {
        self.next_sampler_face();
    }
}
