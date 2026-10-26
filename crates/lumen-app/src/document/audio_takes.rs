//! Recorded audio (M9 slice R2): the document side. The files are written by
//! the take worker (`take_worker.rs`); where they go is
//! `lumen_project::edit::audio_take`.

use std::path::PathBuf;

use lumen_project::edit::audio_take::{self, Placed, Take};

use super::Document;
use super::clips::fresh_ids;

impl Document {
    /// Puts each recorded file on its track, over the span it was played in,
    /// as one step that undo takes back whole. Takes whose track is gone are
    /// left out; with none left nothing is recorded in the history.
    pub fn record_audio_takes(&mut self, takes: &[Take]) -> Placed {
        for take in takes {
            self.media_files
                .insert(take.media.id, PathBuf::from(&take.media.path));
        }
        let mut placed = Placed::default();
        let next = &mut self.next_id;
        let changed = self.history.apply("Record", None, |project| {
            placed = audio_take::apply(project, takes, &mut fresh_ids(next));
        });
        if changed { placed } else { Placed::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;
    use lumen_project::model::Media;

    /// The `n`th audio track of the project.
    fn audio(doc: &Document, n: usize) -> usize {
        doc.project()
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == lumen_project::model::TrackKind::Audio)
            .map(|(i, _)| i)
            .nth(n)
            .expect("an audio track")
    }

    fn take(doc: &mut Document, n: usize) -> Take {
        let track = audio(doc, n);
        Take {
            track: doc.project().tracks[track].id,
            media: Media {
                id: doc.fresh_id(),
                path: "Recordings/Vox 1.wav".into(),
                original_path: None,
                frames: 48_000,
                channels: 1,
                sample_rate: 48_000,
                size_bytes: 0,
            },
            name: "Vox 1".into(),
            start: Ticks::quarters(2),
            length: Ticks::quarters(2),
            passes: Vec::new(),
            active: 0,
        }
    }

    #[test]
    fn a_recording_is_one_undo_step_that_gives_the_track_back() {
        let mut doc = Document::new(lumen_project::testing::small_project(1));
        let before = doc.project().clone();
        let takes = [take(&mut doc, 0), take(&mut doc, 1)];
        let placed = doc.record_audio_takes(&takes);
        assert_eq!(placed.clips.len(), 2);
        assert!(doc.media_files.contains_key(&takes[0].media.id));
        assert!(doc.undo().is_some());
        assert_eq!(doc.project(), &before);
        assert!(doc.undo().is_none(), "one step");
    }

    #[test]
    fn nothing_placed_records_no_step() {
        let mut doc = Document::new(lumen_project::testing::small_project(1));
        let mut lost = take(&mut doc, 0);
        lost.track = lumen_project::model::Id(999_999);
        let before = doc.project().clone();
        assert!(doc.record_audio_takes(&[lost]).clips.is_empty());
        assert_eq!(doc.project(), &before);
        assert!(doc.undo().is_none());
    }
}
