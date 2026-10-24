//! Consolidate (M7b slice E3): the document side. The rendering is the
//! app's (`consolidate.rs`); the swap is `lumen_project::edit::consolidate`.

use std::path::PathBuf;

use lumen_project::edit::consolidate::{self, Baked};
use lumen_project::model::Id;

use super::Document;
use super::clips::fresh_ids;

impl Document {
    /// Replaces the clips each rendered file was made from with one clip that
    /// plays it, as one step. Returns the new clips. A part whose clips
    /// changed while it rendered is left alone; when none is applied nothing
    /// is recorded.
    pub fn consolidate_clips(&mut self, baked: &[Baked]) -> Vec<Id> {
        let current: Vec<Baked> = baked
            .iter()
            .filter(|b| consolidate::is_current(self.history.project(), &b.part))
            .cloned()
            .collect();
        if current.is_empty() {
            return Vec::new();
        }
        for b in &current {
            self.media_files
                .insert(b.media.id, PathBuf::from(&b.media.path));
        }
        let mut made = Vec::new();
        let next = &mut self.next_id;
        self.history.apply("Consolidate", None, |project| {
            made = consolidate::apply(project, &current, &mut fresh_ids(next));
        });
        made
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::edit::consolidate::plan;
    use lumen_project::model::Media;

    fn baked(doc: &mut Document, ids: &[Id]) -> Vec<Baked> {
        let media = doc.fresh_id();
        plan(doc.project(), ids)
            .into_iter()
            .map(|part| Baked {
                part,
                media: Media {
                    id: media,
                    path: "rendered.wav".into(),
                    original_path: None,
                    frames: 48_000,
                    channels: 2,
                    sample_rate: 48_000,
                    size_bytes: 0,
                },
            })
            .collect()
    }

    fn two_clips() -> (Document, Vec<Id>) {
        let doc = Document::new(lumen_project::testing::small_project(1));
        let ids: Vec<Id> = doc
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        (doc, ids)
    }

    #[test]
    fn consolidating_is_one_undo_step_that_gives_the_clips_back() {
        let (mut doc, ids) = two_clips();
        let before = doc.project().clone();
        let parts = baked(&mut doc, &ids);
        assert!(!parts.is_empty());
        let made = doc.consolidate_clips(&parts);
        assert_eq!(made.len(), parts.len());
        assert!(doc.media_files.contains_key(&parts[0].media.id));
        assert!(doc.undo().is_some());
        assert_eq!(doc.project(), &before);
        assert!(doc.undo().is_none(), "one step");
    }

    #[test]
    fn a_stale_result_is_not_applied_and_records_nothing() {
        let (mut doc, ids) = two_clips();
        let parts = baked(&mut doc, &ids);
        doc.delete_clips(&ids);
        let after = doc.project().clone();
        assert!(doc.consolidate_clips(&parts).is_empty());
        assert_eq!(doc.project(), &after);
    }
}
