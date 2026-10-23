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
    use lumen_core::automation::{Lane, Point};
    use lumen_core::fade::Fade;
    use lumen_project::model::{ClipContent, ClipEvent, EventKind, Note};
    use std::sync::Arc;

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
                ..Default::default()
            };
            clip.fade_out = clip.fade_in;
            clip.envelopes.set(
                lumen_project::envelope::Kind::Gain,
                Lane::new(vec![
                    Point::new(Ticks::ZERO, -6.0),
                    Point::new(Ticks::quarters(3), 0.0),
                ])
                .unwrap(),
            );
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
        assert!(clips.iter().all(|clip| clip.envelopes.gain.is_some()));
        assert_eq!(clips[1].loop_phase, Ticks::quarters(1));
        assert_eq!(clips[1].envelopes, before.envelopes);
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
        assert_eq!(
            doc.split_clips(&[id], &[Ticks::ZERO, Ticks::quarters(4)]),
            [id]
        );
        assert_eq!(doc.project().tracks[0].clips[0], before);
        assert_eq!(
            doc.undo(),
            Some("lock".to_owned()),
            "no-op split adds no history step"
        );
    }

    #[test]
    fn midi_split_preserves_notes_events_and_hidden_note_provenance() {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![file(media, "MIDI", Ticks::ZERO)]);
        let id = doc.project().tracks[0].clips[0].id;
        doc.apply("make MIDI", None, |project| {
            let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
            clip.content = ClipContent::Midi {
                notes: vec![
                    Note::new(Ticks::ZERO, Ticks::quarters(3), 60, 90),
                    Note::new(Ticks::quarters(3), Ticks::quarters(3), 62, 80),
                    Note::new(Ticks::quarters(7), Ticks::quarters(1), 64, 70),
                ],
                events: vec![ClipEvent {
                    at: Ticks::quarters(3),
                    kind: EventKind::Controller {
                        number: 1,
                        value: 64,
                    },
                }],
            };
        });
        let parts = doc.split_clips(&[id], &[Ticks::quarters(2)]);
        assert_eq!(parts.len(), 2);
        let clips = &doc.project().tracks[0].clips;
        let ClipContent::Midi {
            notes: left,
            events: left_events,
        } = &clips[0].content
        else {
            panic!("MIDI content retained");
        };
        let ClipContent::Midi {
            notes: right,
            events: right_events,
        } = &clips[1].content
        else {
            panic!("MIDI content retained");
        };
        assert_eq!(
            left.len(),
            3,
            "hidden notes remain available after rejoining"
        );
        assert_eq!(left[0].start(), Ticks::ZERO);
        assert_eq!(left[1].start(), Ticks::quarters(3));
        assert_eq!(left[2].start(), Ticks::quarters(7));
        assert_eq!(right.len(), 3);
        assert_eq!(
            right[0].start(),
            Ticks::quarters(-2),
            "crossing note retains its hidden pre-seam part"
        );
        assert!(right[0].carries_on(), "split note retains seam provenance");
        assert_eq!(right[1].start(), Ticks::quarters(1));
        assert_eq!(
            left_events.len(),
            1,
            "hidden events remain available when rejoining"
        );
        assert_eq!(left_events[0].at, Ticks::quarters(3));
        assert_eq!(right_events[0].at, Ticks::quarters(1));
    }
}
