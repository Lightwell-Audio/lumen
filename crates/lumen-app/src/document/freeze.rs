//! Freeze (M7b slice E5): the document side. The rendering is the app's
//! (`freeze.rs`); which tracks go, over what span, and what the project
//! becomes is `lumen_project::edit::freeze`.

use std::path::PathBuf;

use lumen_project::edit::freeze::{self, Baked, Violation};
use lumen_project::model::Id;

use super::Document;
use super::clips::fresh_ids;

impl Document {
    /// Freezes each track with its rendered file as one step. A part whose
    /// track changed while it rendered is left alone; when nothing is applied
    /// nothing is recorded and the answer is empty.
    pub fn freeze_tracks(&mut self, baked: &[Baked]) -> Vec<Id> {
        let current: Vec<Baked> = baked
            .iter()
            .filter(|b| freeze::is_current(self.history.project(), &b.part))
            .cloned()
            .collect();
        for b in &current {
            self.media_files
                .insert(b.media.id, PathBuf::from(&b.media.path));
        }
        let mut done = Vec::new();
        let next = &mut self.next_id;
        self.history.apply("Freeze", None, |project| {
            done = freeze::apply(project, &current, &mut fresh_ids(next));
        });
        done
    }

    /// Unfreezes tracks as one step: their clips, notes and devices play
    /// again. Returns the tracks that were frozen.
    pub fn unfreeze_tracks(&mut self, tracks: &[Id]) -> Vec<Id> {
        let mut done = Vec::new();
        self.history.apply("Unfreeze", None, |project| {
            done = freeze::unfreeze(project, tracks);
        });
        done
    }

    /// Flattens frozen tracks into plain audio tracks as one step (owner's
    /// FR3). Returns the tracks flattened.
    pub fn flatten_tracks(&mut self, tracks: &[Id]) -> Vec<Id> {
        let mut done = Vec::new();
        self.history.apply("Flatten", None, |project| {
            done = freeze::flatten(project, tracks);
        });
        done
    }

    /// What the latest refused edit tried to change on a frozen track (owner's
    /// FR2), once: the interface says why nothing happened.
    pub fn take_refused(&mut self) -> Vec<Violation> {
        self.history.take_refused()
    }

    /// Names the tracks a recording is writing a take into (none when it has
    /// ended): edits to their clips are refused meanwhile, as `take_refused`
    /// reports. Called every frame, so a recording that ends any way at all
    /// lifts it.
    pub fn guard_recording(&mut self, tracks: &[Id]) {
        self.history.guard_recording(tracks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::edit::freeze::plan;
    use lumen_project::model::{Media, TrackKind};

    fn with_clips() -> Document {
        Document::new(lumen_project::testing::small_project(1))
    }

    fn baked(doc: &mut Document, tracks: &[Id]) -> Vec<Baked> {
        let media = doc.fresh_id();
        plan(doc.project(), tracks)
            .parts
            .into_iter()
            .map(|part| {
                let end = part.end;
                Baked {
                    part,
                    media: Media {
                        id: media,
                        path: "frozen.wav".into(),
                        original_path: None,
                        frames: 48_000,
                        channels: 2,
                        sample_rate: 48_000,
                        size_bytes: 0,
                    },
                    end,
                    devices: Vec::new(),
                }
            })
            .collect()
    }

    fn a_track_with_clips(doc: &Document, kind: TrackKind) -> Id {
        doc.project()
            .tracks
            .iter()
            .find(|t| t.kind == kind && !t.clips.is_empty())
            .expect("a track")
            .id
    }

    #[test]
    fn freezing_is_one_undo_step_that_gives_the_track_back() {
        let mut doc = with_clips();
        let track = a_track_with_clips(&doc, TrackKind::Audio);
        let before = doc.project().clone();
        let parts = baked(&mut doc, &[track]);
        assert_eq!(doc.freeze_tracks(&parts), vec![track]);
        assert!(doc.media_files.contains_key(&parts[0].media.id));
        assert!(doc.project().tracks.iter().any(|t| t.frozen.is_some()));
        assert_eq!(doc.undo().as_deref(), Some("Freeze"));
        assert_eq!(doc.project(), &before);
        assert!(doc.undo().is_none(), "one step");
    }

    #[test]
    fn unfreeze_and_flatten_are_single_steps() {
        let mut doc = with_clips();
        let track = a_track_with_clips(&doc, TrackKind::Midi);
        let parts = baked(&mut doc, &[track]);
        doc.freeze_tracks(&parts);
        let frozen = doc.project().clone();
        assert_eq!(doc.unfreeze_tracks(&[track]), vec![track]);
        assert!(doc.project().tracks.iter().all(|t| t.frozen.is_none()));
        assert_eq!(doc.undo().as_deref(), Some("Unfreeze"));
        assert_eq!(doc.project(), &frozen);
        assert_eq!(doc.flatten_tracks(&[track]), vec![track]);
        let flat = doc.project().tracks.iter().find(|t| t.id == track).unwrap();
        assert_eq!(flat.kind, TrackKind::Audio);
        assert_eq!(flat.clips.len(), 1);
        assert_eq!(doc.undo().as_deref(), Some("Flatten"));
        assert_eq!(doc.project(), &frozen);
        assert!(doc.unfreeze_tracks(&[Id(9_999)]).is_empty());
    }

    #[test]
    fn a_frozen_track_refuses_a_clip_edit_and_says_so_once() {
        let mut doc = with_clips();
        let track = a_track_with_clips(&doc, TrackKind::Audio);
        let parts = baked(&mut doc, &[track]);
        doc.freeze_tracks(&parts);
        let clips: Vec<Id> = doc
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .unwrap()
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
        let before = doc.project().clone();
        doc.delete_clips(&clips);
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.take_refused(), vec![Violation::Track(track)]);
        assert!(doc.take_refused().is_empty());
        doc.set_volume(track, -3.0);
        assert!(doc.take_refused().is_empty(), "the fader is live");
    }

    #[test]
    fn ids_after_reopening_do_not_reuse_the_frozen_clip_and_a_copy_has_its_own() {
        let mut doc = with_clips();
        let track = a_track_with_clips(&doc, TrackKind::Audio);
        let parts = baked(&mut doc, &[track]);
        doc.freeze_tracks(&parts);
        let frozen = doc.project().tracks.iter().find(|t| t.id == track).unwrap();
        let used = frozen.frozen.as_ref().unwrap().id;
        let reopened = Document::new(doc.project().clone());
        assert!(reopened.next_id > used.0, "a new id never repeats it");
        let copy = doc.duplicate_tracks(&[track]);
        let copied = doc
            .project()
            .tracks
            .iter()
            .find(|t| t.id == copy[0])
            .unwrap();
        assert_ne!(copied.frozen.as_ref().unwrap().id, used);
    }

    #[test]
    fn a_stale_result_is_not_applied_and_records_nothing() {
        let mut doc = with_clips();
        let track = a_track_with_clips(&doc, TrackKind::Audio);
        let parts = baked(&mut doc, &[track]);
        doc.set_volume(track, -9.0);
        let after = doc.project().clone();
        assert!(doc.freeze_tracks(&parts).is_empty());
        assert_eq!(doc.project(), &after);
    }
}
