//! Bounce (M7b slice E4): the document side. The rendering is the app's
//! (`bounce.rs`); where the files go is `lumen_project::edit::bounce`.

use std::path::PathBuf;

use lumen_project::edit::bounce::{self, Baked, Made};

use super::Document;
use super::clips::fresh_ids;

impl Document {
    /// Puts each rendered file where its part says (over the track, or on a
    /// new audio track) as one step. A part whose track changed while it
    /// rendered is left alone; when nothing is applied nothing is recorded and
    /// the answer is empty.
    pub fn bounce_tracks(&mut self, baked: &[Baked]) -> Made {
        let current: Vec<Baked> = baked
            .iter()
            .filter(|b| bounce::is_current(self.history.project(), &b.part))
            .cloned()
            .collect();
        for b in &current {
            self.media_files
                .insert(b.media.id, PathBuf::from(&b.media.path));
        }
        let mut made = Made {
            clips: Vec::new(),
            tracks: Vec::new(),
        };
        let next = &mut self.next_id;
        let changed = self.history.apply("Bounce", None, |project| {
            made = bounce::apply(project, &current, &mut fresh_ids(next));
        });
        if changed {
            made
        } else {
            Made {
                clips: Vec::new(),
                tracks: Vec::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::edit::bounce::{Mode, Options, Scope, plan};
    use lumen_project::model::{Id, Media};

    fn baked(doc: &mut Document, mode: Mode) -> Vec<Baked> {
        let tracks: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
        let media = doc.fresh_id();
        let options = Options {
            mode,
            effects: false,
            tail: true,
        };
        plan(doc.project(), &tracks, Scope::Track, options)
            .into_iter()
            .map(|part| {
                let end = part.end;
                Baked {
                    part,
                    media: Media {
                        id: media,
                        path: "bounced.wav".into(),
                        original_path: None,
                        frames: 48_000,
                        channels: 2,
                        sample_rate: 48_000,
                        size_bytes: 0,
                    },
                    end,
                }
            })
            .collect()
    }

    fn with_clips() -> Document {
        Document::new(lumen_project::testing::small_project(1))
    }

    #[test]
    fn bouncing_is_one_undo_step_that_gives_the_tracks_back() {
        let mut doc = with_clips();
        let before = doc.project().clone();
        let parts = baked(&mut doc, Mode::NewTrack);
        assert!(!parts.is_empty());
        let made = doc.bounce_tracks(&parts);
        assert_eq!(made.tracks.len(), parts.len());
        assert!(doc.media_files.contains_key(&parts[0].media.id));
        assert!(doc.undo().is_some());
        assert_eq!(doc.project(), &before);
        assert!(doc.undo().is_none(), "one step");
    }

    #[test]
    fn a_stale_result_is_not_applied_and_records_nothing() {
        let mut doc = with_clips();
        let parts = baked(&mut doc, Mode::Replace);
        let ids: Vec<Id> = doc
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        doc.delete_clips(&ids);
        let after = doc.project().clone();
        assert!(doc.bounce_tracks(&parts).clips.is_empty());
        assert_eq!(doc.project(), &after);
    }
}
