//! Splitting selected clips at arrangement times.

use lumen_core::time::Ticks;
use lumen_project::edit::split;
use lumen_project::model::Id;

use super::Document;

impl Document {
    /// Splits selected unlocked clips at each interior tick in `at`.
    ///
    /// Each source clip keeps its id on the first part; later parts get fresh
    /// ids. The returned ids cover the resulting parts in timeline order.
    /// Cuts on clip boundaries and clips that cannot be found are ignored.
    /// All splits are one undo step.
    pub fn split_clips(&mut self, ids: &[Id], at: &[Ticks]) -> Vec<Id> {
        if ids.is_empty() || at.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();
        let next_id = &mut self.next_id;
        self.history.apply("Split clips", None, |project| {
            result = split::clips(project, ids, at, &mut || {
                let id = Id(*next_id);
                *next_id += 1;
                id
            });
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::tests::file;
    use lumen_core::fade::Fade;
    use lumen_project::model::{ClipContent, ClipEvent, EventKind, Note};

    #[test]
    fn split_preserves_source_id_and_is_one_exact_undoable_step() {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![file(media, "Split", Ticks::ZERO)]);
        let id = doc.project().tracks[0].clips[0].id;
        doc.apply("clip details", None, |project| {
            let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
            clip.loop_length = Some(Ticks::quarters(2));
            clip.fade_in = Fade {
                length: Ticks::quarters(1),
                curve: lumen_core::curve::Curve::LINEAR,
            };
            clip.fade_out = clip.fade_in;
        });
        let before = doc.project().tracks[0].clips[0].clone();

        let parts = doc.split_clips(&[id], &[Ticks::quarters(2), Ticks::quarters(1)]);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], id, "the first part keeps the source id");
        let clips = &doc.project().tracks[0].clips;
        assert_eq!(
            clips.iter().map(|c| c.start).collect::<Vec<_>>(),
            [Ticks::ZERO, Ticks::quarters(1), Ticks::quarters(2)]
        );
        assert!(clips.iter().all(|c| c.loop_length == before.loop_length));
        assert_eq!(clips[0].fade_in, before.fade_in);
        assert!(clips[0].fade_out.is_none());
        assert!(clips[1].fade_in.is_none() && clips[1].fade_out.is_none());
        assert_eq!(clips[2].fade_out, before.fade_out);
        assert!(matches!(clips[0].content, ClipContent::Audio { .. }));

        doc.undo();
        assert_eq!(doc.project().tracks[0].clips[0], before);
        doc.redo();
        assert_eq!(
            doc.project().tracks[0]
                .clips
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>(),
            parts
        );
    }

    #[test]
    fn split_ignores_locked_clips_and_boundary_ticks() {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![file(media, "Locked", Ticks::ZERO)]);
        let id = doc.project().tracks[0].clips[0].id;
        doc.apply("lock", None, |project| {
            Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]).locked = true;
        });
        let before = doc.project().tracks[0].clips[0].clone();
        assert!(doc.split_clips(&[id], &[Ticks::ZERO, Ticks::quarters(4)]).is_empty());
        assert_eq!(doc.project().tracks[0].clips[0], before);
        assert_eq!(
            doc.undo(),
            Some("lock".to_owned()),
            "no-op split adds no history step"
        );
    }
}
