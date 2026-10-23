//! Clip commands of M7b slice E1: join, crop, unloop and nudge. Each is one
//! undo step; one that changes nothing adds none.

use lumen_core::time::Ticks;
use lumen_project::edit::clip_ops;
use lumen_project::model::Id;

use super::clips::fresh_ids;
use super::{ClipMove, Document, plural};

impl Document {
    /// Joins the selected clips of each track that follow one another (see
    /// `lumen_project::edit::clip_ops`): MIDI clips merge, audio clips only
    /// where they continue one another in their source.
    pub fn join_clips(&mut self, ids: &[Id]) -> clip_ops::Joined {
        let mut out = clip_ops::Joined::default();
        self.history.apply("Join clips", None, |project| {
            out = clip_ops::join(project, ids);
        });
        out
    }

    /// Keeps the time range `[a, b)` of `tracks` and removes the rest of
    /// their clips (Crop to selection).
    pub fn crop_clips(&mut self, tracks: &[Id], a: Ticks, b: Ticks) {
        let next = &mut self.next_id;
        self.history.apply("Crop to selection", None, |project| {
            clip_ops::crop(project, tracks, a, b, &mut fresh_ids(next));
        });
    }

    /// Turns looping clips into real clips, one for each repeat. Returns
    /// the ids that stand for the selection afterwards and whether a clip
    /// had too many repeats to unloop.
    pub fn unloop_clips(&mut self, ids: &[Id]) -> (Vec<Id>, bool) {
        let mut out = (Vec::new(), false);
        let next = &mut self.next_id;
        self.history.apply("Unloop clips", None, |project| {
            out = clip_ops::unloop(project, ids, &mut fresh_ids(next));
        });
        out
    }

    /// Moves the unlocked clips among `ids` by `by` (earlier when negative),
    /// never before the start of the song. The moved clips cut away what
    /// they land on, as a drag does.
    pub fn nudge_clips(&mut self, ids: &[Id], by: Ticks) {
        let base = self.project().clone();
        let moves: Vec<ClipMove> = base
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(move |c| (t.id, c)))
            .filter(|(_, c)| ids.contains(&c.id) && !c.locked)
            .map(|(track, c)| ClipMove {
                clip: c.id,
                track,
                start: (c.start + by).max(Ticks::ZERO),
            })
            .filter(|m| {
                base.tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .any(|c| c.id == m.clip && c.start != m.start)
            })
            .collect();
        if moves.is_empty() {
            return;
        }
        let label = plural(moves.len(), "Nudge clip", "Nudge {} clips");
        self.place_moves(&label, &base, &moves, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::tests::file;
    use lumen_project::model::ClipContent;

    fn doc_with_clip() -> (Document, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![file(media, "Loop", Ticks::quarters(4))]);
        let track = doc.project().tracks[0].id;
        let clip = doc.project().tracks[0].clips[0].id;
        (doc, track, clip)
    }

    #[test]
    fn split_then_join_is_the_original_and_each_is_one_undoable_step() {
        let (mut doc, _, id) = doc_with_clip();
        let original = doc.project().tracks[0].clips[0].clone();
        let parts = doc.split_clips(&[id], &[Ticks::quarters(6)]);
        assert_eq!(parts.len(), 2);
        let after_split = doc.project().clone();
        let joined = doc.join_clips(&parts);
        assert_eq!((joined.merged, joined.clips.as_slice()), (1, &[id][..]));
        assert_eq!(doc.project().tracks[0].clips[0], original);
        doc.undo();
        assert_eq!(doc.project(), &after_split, "undo gives the parts back");
        doc.redo();
        assert_eq!(doc.project().tracks[0].clips.len(), 1);
    }

    #[test]
    fn a_join_that_changes_nothing_adds_no_history_step() {
        let (mut doc, _, id) = doc_with_clip();
        doc.set_clips_muted(&[id], true);
        let before = doc.project().clone();
        let joined = doc.join_clips(&[id]);
        assert_eq!(joined.merged, 0);
        assert_eq!(doc.project(), &before);
        assert_ne!(doc.undo().as_deref(), Some("Join clips"));
    }

    #[test]
    fn unloop_crop_and_nudge_are_one_exact_undo_step_each() {
        let (mut doc, track, id) = doc_with_clip();
        doc.apply("loop", None, |project| {
            let t = std::sync::Arc::make_mut(&mut project.tracks[0]);
            let c = std::sync::Arc::make_mut(&mut t.clips[0]);
            c.loop_length = Some(Ticks::quarters(1));
            c.length = Ticks::quarters(4);
        });
        let looped = doc.project().clone();
        let (made, too_long) = doc.unloop_clips(&[id]);
        assert!(!too_long);
        assert_eq!(made.len(), 4);
        assert!(
            doc.project().tracks[0]
                .clips
                .iter()
                .all(|c| c.loop_length.is_none())
        );
        doc.undo();
        assert_eq!(doc.project(), &looped);

        doc.crop_clips(&[track], Ticks::quarters(5), Ticks::quarters(7));
        let c = &doc.project().tracks[0].clips[0];
        assert_eq!(
            (c.start, c.length),
            (Ticks::quarters(5), Ticks::quarters(2))
        );
        doc.undo();
        assert_eq!(doc.project(), &looped);

        doc.nudge_clips(&[id], Ticks::quarters(-10));
        assert_eq!(doc.project().tracks[0].clips[0].start, Ticks::ZERO);
        doc.undo();
        assert_eq!(doc.project(), &looped);
        assert!(matches!(
            doc.project().tracks[0].clips[0].content,
            ClipContent::Audio { .. }
        ));
    }

    #[test]
    fn nudging_locked_clips_or_by_nothing_changes_nothing() {
        let (mut doc, _, id) = doc_with_clip();
        let before = doc.project().clone();
        doc.nudge_clips(&[id], Ticks::ZERO);
        doc.set_clips_locked(&[id], true);
        let locked = doc.project().clone();
        doc.nudge_clips(&[id], Ticks::quarters(1));
        assert_eq!(doc.project(), &locked);
        assert_ne!(before, locked);
    }
}
