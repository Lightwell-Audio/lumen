use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::edit::{self, note_tools as tools, notes as ops, quantize};
use lumen_project::model::{Clip, ClipContent, Id, Note, Project};

use super::{Document, plural};

pub use ops::{End, MIN_NOTE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resize {
    pub end: End,
    pub by: Ticks,
    pub min: Ticks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneMove<'a> {
    pub time: Ticks,
    pub pitches: &'a [u8],
    pub copy: bool,
}

impl Document {
    pub fn notes_editable(&self, clip: Id) -> bool {
        find(self.project(), clip).is_some()
    }

    fn edit_notes(
        &mut self,
        label: &str,
        base: Option<&Project>,
        merge: Option<&str>,
        clip: Id,
        selection: &[usize],
        change: impl FnOnce(&mut Vec<Note>, Ticks) -> Vec<usize>,
    ) -> Vec<usize> {
        let source = base.unwrap_or_else(|| self.project());
        if find(source, clip).is_none() {
            return selection.to_vec();
        }
        let mut result = selection.to_vec();
        self.history.apply(label, merge, |project| {
            if let Some(b) = base {
                *project = b.clone();
            }
            for track in &mut project.tracks {
                let Some(at) = track
                    .clips
                    .iter()
                    .position(|c| c.id == clip && !c.locked && is_midi(c))
                else {
                    continue;
                };
                let track = Arc::make_mut(track);
                let clip = Arc::make_mut(&mut track.clips[at]);
                let length = edit::content_length(clip);
                if let ClipContent::Midi { notes, .. } = &mut clip.content {
                    result = change(notes, length);
                }
                break;
            }
        });
        result
    }

    pub fn add_note(
        &mut self,
        base: Option<&Project>,
        clip: Id,
        note: Note,
        merge: Option<&str>,
    ) -> Vec<usize> {
        self.edit_notes("Draw note", base, merge, clip, &[], |notes, length| {
            ops::add(notes, note, length)
        })
    }

    pub fn move_notes(
        &mut self,
        base: &Project,
        clip: Id,
        selection: &[usize],
        time: Ticks,
        pitch: i32,
        merge: Option<&str>,
    ) -> Vec<usize> {
        let label = plural(selection.len(), "Move note", "Move {} notes");
        self.edit_notes(
            &label,
            Some(base),
            merge,
            clip,
            selection,
            |notes, length| ops::move_notes(notes, selection, time, pitch, length),
        )
    }

    pub fn copy_notes(
        &mut self,
        base: &Project,
        clip: Id,
        selection: &[usize],
        time: Ticks,
        pitch: i32,
        merge: Option<&str>,
    ) -> Vec<usize> {
        let label = plural(selection.len(), "Copy note", "Copy {} notes");
        self.edit_notes(
            &label,
            Some(base),
            merge,
            clip,
            selection,
            |notes, length| ops::copy_moved(notes, selection, time, pitch, length),
        )
    }

    pub fn resize_notes(
        &mut self,
        base: &Project,
        clip: Id,
        selection: &[usize],
        resize: Resize,
        merge: Option<&str>,
    ) -> Vec<usize> {
        let label = plural(selection.len(), "Resize note", "Resize {} notes");
        self.edit_notes(
            &label,
            Some(base),
            merge,
            clip,
            selection,
            |notes, length| {
                ops::resize(
                    notes,
                    selection,
                    resize.end,
                    resize.by,
                    resize.min.max(MIN_NOTE),
                    length,
                )
            },
        )
    }

    pub fn delete_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        let label = plural(selection.len(), "Delete note", "Delete {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            ops::delete(notes, selection)
        })
    }

    pub fn split_notes(&mut self, clip: Id, selection: &[usize], at: Ticks) -> Vec<usize> {
        let label = plural(selection.len(), "Split note", "Split {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            ops::split(notes, selection, at)
        })
    }

    pub fn glue_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        self.edit_notes("Join", None, None, clip, selection, |notes, _| {
            ops::glue(notes, selection)
        })
    }

    pub fn mute_notes(&mut self, clip: Id, selection: &[usize], muted: Option<bool>) -> Vec<usize> {
        let label = match muted {
            Some(true) => plural(selection.len(), "Mute note", "Mute {} notes"),
            Some(false) => plural(selection.len(), "Unmute note", "Unmute {} notes"),
            None => plural(
                selection.len(),
                "Toggle note mute",
                "Toggle mute of {} notes",
            ),
        };
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            ops::mute(notes, selection, muted)
        })
    }

    pub fn duplicate_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        let label = plural(selection.len(), "Duplicate note", "Duplicate {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            ops::duplicate(notes, selection, length)
        })
    }

    pub fn move_notes_to_pitches(
        &mut self,
        base: &Project,
        clip: Id,
        selection: &[usize],
        to: LaneMove<'_>,
        merge: Option<&str>,
    ) -> Vec<usize> {
        let LaneMove {
            time,
            pitches,
            copy,
        } = to;
        let label = if copy {
            plural(selection.len(), "Copy note", "Copy {} notes")
        } else {
            plural(selection.len(), "Move note", "Move {} notes")
        };
        self.edit_notes(
            &label,
            Some(base),
            merge,
            clip,
            selection,
            |notes, length| ops::move_to_pitches(notes, selection, time, pitches, copy, length),
        )
    }

    pub fn nudge_notes(&mut self, clip: Id, selection: &[usize], by: Ticks) -> Vec<usize> {
        let label = plural(selection.len(), "Nudge note", "Nudge {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            ops::nudge(notes, selection, by, length)
        })
    }

    pub fn transpose_notes(&mut self, clip: Id, selection: &[usize], semitones: i32) -> Vec<usize> {
        let label = plural(selection.len(), "Transpose note", "Transpose {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            ops::transpose(notes, selection, semitones)
        })
    }

    pub fn legato_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        let label = plural(selection.len(), "Legato", "Legato on {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            ops::legato(notes, selection, length)
        })
    }

    pub fn quantize_notes(
        &mut self,
        clip: Id,
        selection: &[usize],
        settings: &quantize::Settings,
    ) -> Vec<usize> {
        let label = plural(selection.len(), "Quantize note", "Quantize {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            quantize::quantize(notes, selection, length, settings)
        })
    }

    pub fn quantize_notes_each(
        &mut self,
        clip: Id,
        selection: &[usize],
        settings: &quantize::Settings,
        steps: &[(u8, Ticks)],
    ) -> Vec<usize> {
        let label = plural(selection.len(), "Quantize note", "Quantize {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            quantize::quantize_each(notes, selection, length, settings, &|pitch| {
                steps
                    .iter()
                    .find(|(input, _)| *input == pitch)
                    .map(|(_, step)| *step)
            })
        })
    }

    pub fn humanize_notes(
        &mut self,
        clip: Id,
        selection: &[usize],
        humanize: &tools::Humanize,
    ) -> Vec<usize> {
        let label = plural(selection.len(), "Humanize note", "Humanize {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            tools::humanize(notes, selection, length, humanize)
        })
    }

    pub fn fixed_length_notes(&mut self, clip: Id, selection: &[usize], len: Ticks) -> Vec<usize> {
        let label = plural(selection.len(), "Fix note length", "Fix length of {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            tools::fixed_length(notes, selection, length, len)
        })
    }

    pub fn scale_constrain_notes(
        &mut self,
        clip: Id,
        selection: &[usize],
        root: u8,
        scale: tools::Scale,
    ) -> Vec<usize> {
        let label = plural(
            selection.len(),
            "Constrain note",
            "Constrain {} notes to scale",
        );
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            tools::scale_constrain(notes, selection, root, scale)
        })
    }

    pub fn reverse_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        let label = plural(selection.len(), "Reverse note", "Reverse {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            tools::reverse(notes, selection)
        })
    }
}

fn is_midi(clip: &Clip) -> bool {
    matches!(clip.content, ClipContent::Midi { .. })
}

fn find(project: &Project, id: Id) -> Option<&Arc<Clip>> {
    project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id && !c.locked && is_midi(c))
}

#[cfg(test)]
mod tests {
    use lumen_core::time::PPQ;
    use lumen_project::edit::NoteSelection;
    use lumen_project::model::{ClipEvent, EventKind, TrackKind};

    use super::super::midi_clips::NewClip;
    use super::*;

    fn q(n: i64) -> Ticks {
        Ticks(PPQ * n)
    }

    fn n(start: i64, len: i64, pitch: u8) -> Note {
        Note::new(q(start), q(len), pitch, 100)
    }

    fn doc_with(notes: Vec<Note>) -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let clip = doc
            .create_midi_clip(track, NewClip::Range(q(4), q(20)))
            .unwrap();
        doc.apply("fill", None, |p| {
            let t = Arc::make_mut(&mut p.tracks[0]);
            if let ClipContent::Midi {
                notes: n, events, ..
            } = &mut Arc::make_mut(&mut t.clips[0]).content
            {
                *n = notes;
                events.push(ClipEvent {
                    at: q(1),
                    kind: EventKind::Controller {
                        number: 1,
                        value: 64,
                    },
                });
            }
        });
        (doc, clip)
    }

    fn content(doc: &Document) -> (Vec<Note>, Vec<ClipEvent>) {
        match &doc.project().tracks[0].clips[0].content {
            ClipContent::Midi { notes, events } => (notes.clone(), events.clone()),
            _ => unreachable!(),
        }
    }

    #[test]
    fn every_edit_is_one_undo_step_that_restores_the_exact_state() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(2, 2, 60), n(5, 1, 64)]);
        let (notes0, events0) = content(&doc);
        let start = doc.project().clone();
        type Edit = Box<dyn Fn(&mut Document)>;
        let edits: Vec<Edit> = vec![
            Box::new(move |d| {
                d.delete_notes(clip, &[0]);
            }),
            Box::new(move |d| {
                d.split_notes(clip, &[0, 1, 2], q(1));
            }),
            Box::new(move |d| {
                d.glue_notes(clip, &[0, 1]);
            }),
            Box::new(move |d| {
                d.mute_notes(clip, &[0, 2], None);
            }),
            Box::new(move |d| {
                d.duplicate_notes(clip, &[0]);
            }),
            Box::new(move |d| {
                d.nudge_notes(clip, &[1], q(1));
            }),
            Box::new(move |d| {
                d.transpose_notes(clip, &[1], 12);
            }),
            Box::new(move |d| {
                d.legato_notes(clip, &[1]);
            }),
            Box::new(move |d| {
                let grid = quantize::Settings {
                    step: q(4),
                    ..quantize::Settings::default()
                };
                d.quantize_notes(clip, &[2], &grid);
            }),
            Box::new(move |d| {
                let timing = tools::Humanize {
                    timing: q(1),
                    velocity: 10,
                    seed: 3,
                };
                d.humanize_notes(clip, &[0, 1, 2], &timing);
            }),
            Box::new(move |d| {
                d.fixed_length_notes(clip, &[0, 1], q(1));
            }),
            Box::new(move |d| {
                d.scale_constrain_notes(clip, &[2], 0, tools::Scale::Minor);
            }),
            Box::new(move |d| {
                d.reverse_notes(clip, &[0, 1, 2]);
            }),
        ];
        for edit in edits {
            let before = doc.project().clone();
            edit(&mut doc);
            assert_ne!(*doc.project(), before, "the edit changed something");
            let (_, events) = content(&doc);
            assert_eq!(events, events0, "events are untouched by note edits");
            doc.undo();
            assert_eq!(*doc.project(), before, "undo restores the exact state");
            doc.redo();
            doc.undo();
            assert_eq!(*doc.project(), before);
        }
        assert_eq!(*doc.project(), start);
        assert_eq!(content(&doc).0, notes0);
    }

    #[test]
    fn a_drag_is_one_step_redone_from_its_base() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(4, 2, 64)]);
        let base = doc.project().clone();
        let mut selected = vec![0];
        for beats in [1, 2, 3] {
            selected = doc.move_notes(&base, clip, &[0], q(beats), beats as i32, Some("note drag"));
        }
        doc.seal();
        let (notes, _) = content(&doc);
        assert_eq!(notes[selected[0]], n(3, 2, 63));
        assert_eq!(notes.len(), 2);
        doc.undo();
        assert_eq!(*doc.project(), base, "the whole drag was one step");
    }

    #[test]
    fn moving_returns_where_the_notes_went_and_clamps_the_group() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(2, 2, 62), n(10, 1, 70)]);
        let base = doc.project().clone();
        let sel = doc.move_notes(&base, clip, &[0, 1], q(-4), 100, None);
        assert_eq!(sel, [0, 1]);
        let (notes, _) = content(&doc);
        assert_eq!((notes[0].start(), notes[1].start()), (q(0), q(2)));
        assert_eq!((notes[0].pitch(), notes[1].pitch()), (125, 127));
        doc.seal();
        let base = doc.project().clone();
        let sel = doc.move_notes(&base, clip, &[0, 1], q(100), 0, None);
        let (notes, _) = content(&doc);
        assert_eq!(sel.len(), 2);
        assert_eq!(notes[sel[1]].end(), q(16));
        assert_eq!(notes[sel[1]].start() - notes[sel[0]].start(), q(2));
    }

    #[test]
    fn drawing_a_note_grows_with_the_drag_in_one_step() {
        let (mut doc, clip) = doc_with(vec![]);
        let base = doc.project().clone();
        let mut last = vec![];
        for len in [1, 2, 4, 40] {
            last = doc.add_note(Some(&base), clip, n(3, len, 67), Some("draw"));
        }
        let (notes, _) = content(&doc);
        assert_eq!(notes, [n(3, 13, 67)], "cut where the clip ends");
        assert_eq!(last, [0]);
        doc.undo();
        assert_eq!(*doc.project(), base);
    }

    #[test]
    fn resizing_and_copying_work_from_the_base() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(4, 2, 64)]);
        let base = doc.project().clone();
        let sel = doc.resize_notes(
            &base,
            clip,
            &[0],
            Resize {
                end: End::End,
                by: q(2),
                min: Ticks(PPQ / 4),
            },
            None,
        );
        assert_eq!(content(&doc).0[sel[0]].length(), q(4));
        doc.undo();
        let sel = doc.resize_notes(
            &base,
            clip,
            &[1],
            Resize {
                end: End::Start,
                by: q(-1),
                min: MIN_NOTE,
            },
            None,
        );
        let (notes, _) = content(&doc);
        assert_eq!((notes[sel[0]].start(), notes[sel[0]].end()), (q(3), q(6)));
        doc.undo();
        let sel = doc.copy_notes(&base, clip, &[0], q(8), 1, None);
        let (notes, _) = content(&doc);
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[sel[0]], n(8, 2, 61));
        doc.undo();
        assert_eq!(*doc.project(), base);
    }

    #[test]
    fn a_locked_clip_refuses_every_edit_and_gives_the_selection_back() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(2, 2, 62)]);
        doc.set_clips_locked(&[clip], true);
        assert!(!doc.notes_editable(clip));
        let before = doc.project().clone();
        let state = doc.state();
        let base = before.clone();
        let sel = [1_usize];
        assert_eq!(doc.delete_notes(clip, &sel), sel);
        assert_eq!(doc.split_notes(clip, &sel, q(3)), sel);
        assert_eq!(doc.glue_notes(clip, &sel), sel);
        assert_eq!(doc.mute_notes(clip, &sel, Some(true)), sel);
        assert_eq!(doc.duplicate_notes(clip, &sel), sel);
        assert_eq!(doc.nudge_notes(clip, &sel, q(1)), sel);
        assert_eq!(doc.transpose_notes(clip, &sel, 1), sel);
        assert_eq!(doc.legato_notes(clip, &sel), sel);
        assert_eq!(doc.move_notes(&base, clip, &sel, q(1), 1, None), sel);
        assert_eq!(doc.copy_notes(&base, clip, &sel, q(1), 1, None), sel);
        assert_eq!(
            doc.resize_notes(
                &base,
                clip,
                &sel,
                Resize {
                    end: End::End,
                    by: q(1),
                    min: MIN_NOTE
                },
                None
            ),
            sel
        );
        assert!(
            doc.add_note(Some(&base), clip, n(5, 1, 60), None)
                .is_empty(),
            "no note drawn"
        );
        let both = [0_usize, 1];
        let grid = quantize::Settings {
            step: q(4),
            ..quantize::Settings::default()
        };
        assert_eq!(doc.quantize_notes(clip, &both, &grid), both);
        let humanize = tools::Humanize {
            timing: q(1),
            velocity: 20,
            seed: 5,
        };
        assert_eq!(doc.humanize_notes(clip, &both, &humanize), both);
        assert_eq!(doc.fixed_length_notes(clip, &both, q(1)), both);
        assert_eq!(
            doc.scale_constrain_notes(clip, &both, 1, tools::Scale::Major),
            both
        );
        assert_eq!(doc.reverse_notes(clip, &both), both);
        assert_eq!(doc.state(), state);
        assert_eq!(*doc.project(), before);
        doc.set_clips_locked(&[clip], false);
        doc.delete_notes(clip, &sel);
        assert_eq!(content(&doc).0.len(), 1);
    }

    fn with_audio_clip(doc: &mut Document) -> Id {
        doc.add_track(TrackKind::Audio, None);
        let clip = doc.fresh_id();
        doc.apply("audio clip", None, |p| {
            if let Some(track) = p.tracks.last_mut() {
                Arc::make_mut(track).clips.push(Arc::new(Clip {
                    id: clip,
                    name: "Audio".into(),
                    start: q(0),
                    length: q(4),
                    muted: false,
                    color: None,
                    locked: false,
                    fade_in: Default::default(),
                    crossfade: false,
                    fade_out: Default::default(),
                    loop_length: None,
                    loop_phase: Default::default(),
                    envelopes: Default::default(),
                    audio_phase_seconds: 0.0,
                    reversed: false,
                    inverted: false,
                    takes: None,
                    warp: None,
                    content: ClipContent::Audio {
                        media: Id(0),
                        offset: 0,
                        gain_db: 0.0,
                    },
                }));
            }
        });
        clip
    }

    #[test]
    fn a_missing_clip_or_an_audio_clip_is_refused() {
        let (mut doc, _) = doc_with(vec![n(0, 1, 60), n(2, 1, 62)]);
        let audio = with_audio_clip(&mut doc);
        let state = doc.state();
        let before = doc.project().clone();
        let sel = [0_usize, 1];
        let grid = quantize::Settings {
            step: q(4),
            ..quantize::Settings::default()
        };
        let humanize = tools::Humanize {
            timing: q(1),
            velocity: 20,
            seed: 5,
        };
        for clip in [Id(777), audio] {
            assert_eq!(doc.delete_notes(clip, &sel), sel);
            assert_eq!(doc.quantize_notes(clip, &sel, &grid), sel);
            assert_eq!(doc.humanize_notes(clip, &sel, &humanize), sel);
            assert_eq!(doc.fixed_length_notes(clip, &sel, q(1)), sel);
            assert_eq!(
                doc.scale_constrain_notes(clip, &sel, 1, tools::Scale::Major),
                sel
            );
            assert_eq!(doc.reverse_notes(clip, &sel), sel);
        }
        assert_eq!(doc.state(), state);
        assert_eq!(*doc.project(), before);
    }

    #[test]
    fn muting_survives_save_and_reopen_and_is_not_played() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(2, 2, 62)]);
        doc.mute_notes(clip, &[1], Some(true));
        let bytes = lumen_project::to_bytes(doc.project()).unwrap();
        let reopened = lumen_project::from_bytes(&bytes).unwrap();
        assert_eq!(reopened, *doc.project());
        let (notes, _) = content(&doc);
        assert!(!notes[0].muted() && notes[1].muted());
        let tracks = crate::midi::play::engine_midi(
            &reopened,
            &crate::midi::play::Inputs::default(),
            &mut crate::midi::play::PortTable::default(),
            &mut crate::midi::play::Cache::default(),
        );
        let pitches: Vec<_> = tracks[0].clips[0].notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, [60]);
        doc.mute_notes(clip, &[1], None);
        assert!(!content(&doc).0[1].muted(), "toggled back");
    }

    #[test]
    fn notes_in_a_looping_clip_edit_against_the_loop_length() {
        let (mut doc, clip) = doc_with(vec![n(0, 1, 60), n(2, 1, 62)]);
        doc.apply("loop", None, |p| {
            let t = Arc::make_mut(&mut p.tracks[0]);
            Arc::make_mut(&mut t.clips[0]).loop_length = Some(q(4));
        });
        let base = doc.project().clone();
        let sel = doc.move_notes(&base, clip, &[0, 1], q(100), 0, None);
        let (notes, _) = content(&doc);
        assert_eq!(notes[sel[1]].end(), q(4));
        doc.undo();
        let none = doc.add_note(None, clip, n(5, 1, 60), None);
        assert!(none.is_empty());
        doc.legato_notes(clip, &[1]);
        assert_eq!(content(&doc).0[1].end(), q(4));
    }

    #[test]
    fn a_selection_follows_its_own_edits_and_clears_after_undo() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(2, 2, 62), n(8, 2, 64)]);
        let mut selection = NoteSelection::in_clip(doc.project(), clip).unwrap();
        selection.rebind(doc.project(), vec![0, 1]);
        assert!(selection.refresh(doc.project()));
        let after = doc.duplicate_notes(clip, selection.indices());
        selection.rebind(doc.project(), after.clone());
        assert!(selection.refresh(doc.project()));
        assert_eq!(selection.indices(), after.as_slice());
        doc.undo();
        assert!(!selection.refresh(doc.project()));
        assert!(selection.is_empty());
        selection.rebind(doc.project(), vec![2]);
        doc.rename_clip(clip, "Verse");
        assert!(selection.refresh(doc.project()));
        assert_eq!(selection.indices(), [2]);
        doc.delete_clips(&[clip]);
        assert!(!selection.refresh(doc.project()));
        assert!(selection.is_empty());
    }

    fn tenths(tenths: i64) -> Ticks {
        Ticks(PPQ * tenths / 10)
    }

    #[test]
    fn quantize_snaps_starts_to_the_grid_and_leaves_events() {
        let (mut doc, clip) = doc_with(vec![Note::new(tenths(1), q(1), 60, 100)]);
        assert_eq!(
            doc.quantize_notes(clip, &[0], &quantize::Settings::default()),
            [0]
        );
        assert_eq!(content(&doc).0, [Note::new(q(0), q(1), 60, 100)]);

        let (mut doc, clip) = doc_with(vec![
            Note::new(tenths(1), q(1), 60, 100),
            Note::new(tenths(26), q(1), 62, 100),
        ]);
        let events0 = content(&doc).1;
        let grid = quantize::Settings {
            step: q(1),
            ..quantize::Settings::default()
        };
        assert_eq!(doc.quantize_notes(clip, &[0, 1], &grid), [0, 1]);
        let (notes, events) = content(&doc);
        assert_eq!(
            notes,
            [
                Note::new(q(0), q(1), 60, 100),
                Note::new(q(3), q(1), 62, 100),
            ]
        );
        assert_eq!(events, events0);
        doc.undo();
        assert_eq!(content(&doc).0[1].start(), tenths(26));
    }

    #[test]
    fn fixed_length_gives_each_picked_note_the_length_and_leaves_the_rest() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(4, 3, 62), n(8, 4, 64)]);
        let events0 = content(&doc).1;
        assert_eq!(doc.fixed_length_notes(clip, &[0, 1], q(1)), [0, 1]);
        let (notes, events) = content(&doc);
        assert_eq!(notes, [n(0, 1, 60), n(4, 1, 62), n(8, 4, 64)]);
        assert_eq!(events, events0);
    }

    #[test]
    fn reverse_mirrors_the_picked_notes_within_their_span() {
        let (mut doc, clip) = doc_with(vec![n(0, 2, 60), n(2, 2, 62), n(5, 1, 64)]);
        let events0 = content(&doc).1;
        assert_eq!(doc.reverse_notes(clip, &[0, 1, 2]), [0, 1, 2]);
        let (notes, events) = content(&doc);
        assert_eq!(notes, [n(0, 1, 64), n(2, 2, 62), n(4, 2, 60)]);
        assert_eq!(events, events0);
    }

    #[test]
    fn scale_constrain_moves_c_sharp_to_c_in_c_major() {
        let (mut doc, clip) = doc_with(vec![n(0, 1, 61), n(2, 1, 62)]);
        let events0 = content(&doc).1;
        assert_eq!(
            doc.scale_constrain_notes(clip, &[0, 1], 0, tools::Scale::Major),
            [0, 1]
        );
        let (notes, events) = content(&doc);
        assert_eq!(notes, [n(0, 1, 60), n(2, 1, 62)]);
        assert_eq!(events, events0);

        let (mut doc, clip) = doc_with(vec![n(0, 1, 60)]);
        doc.scale_constrain_notes(clip, &[0], 2, tools::Scale::Major);
        assert_eq!(content(&doc).0, [n(0, 1, 59)]);
    }

    #[test]
    fn humanize_without_a_range_changes_nothing_and_timing_stays_in_the_clip() {
        let (mut doc, clip) = doc_with(vec![n(0, 1, 60), n(4, 1, 62), n(15, 1, 64)]);
        let events0 = content(&doc).1;
        let before = doc.project().clone();
        let none = tools::Humanize {
            timing: Ticks::ZERO,
            velocity: 0,
            seed: 1,
        };
        assert_eq!(doc.humanize_notes(clip, &[0, 1, 2], &none), [0, 1, 2]);
        assert_eq!(*doc.project(), before, "no range, so no change and no step");

        let timing = tools::Humanize {
            timing: q(2),
            velocity: 0,
            seed: 1,
        };
        let sel = doc.humanize_notes(clip, &[0, 1, 2], &timing);
        let (notes, events) = content(&doc);
        assert_eq!(events, events0);
        assert_eq!(sel.len(), 3);
        let mut pitches: Vec<u8> = sel.iter().map(|&i| notes[i].pitch()).collect();
        pitches.sort_unstable();
        assert_eq!(pitches, [60, 62, 64]);
        let original = |pitch: u8| match pitch {
            60 => q(0),
            62 => q(4),
            _ => q(15),
        };
        assert!(
            notes
                .iter()
                .any(|note| note.start() != original(note.pitch())),
            "at least one start moved"
        );
        for note in &notes {
            assert!(
                note.start() >= Ticks::ZERO && note.end() <= q(16),
                "{note:?} stays in the clip"
            );
            assert_eq!(note.length(), q(1));
            assert_eq!(note.velocity(), 100, "velocity has no range");
        }
    }
}
