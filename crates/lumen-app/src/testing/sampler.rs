use lumen_project::model::{Id, Sampler};

use crate::App;

impl App {
    pub fn sampler_content(&self, device: Id) -> Option<Sampler> {
        self.document.sampler_sound(device).cloned()
    }

    pub fn sampler_media(&self, device: Id) -> Vec<Id> {
        self.document
            .sampler_sound(device)
            .map(lumen_project::model::Sampler::media)
            .unwrap_or_default()
    }

    pub fn sample_loaded(&self, media: Id) -> bool {
        self.lanes.samples.get(media).is_some()
    }

    pub fn sample_memory(&self) -> usize {
        self.lanes.samples.memory()
    }

    pub fn cycle_sampler_face(&mut self) {
        self.next_sampler_face();
    }
}
