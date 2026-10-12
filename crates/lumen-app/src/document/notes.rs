//! Note edits (M5 slice 3a): the piano roll's operations on a MIDI clip's
//! notes, each one undo step through the history. The arithmetic is
//! `lumen_project::edit::notes`; see its documentation for the selection
//! contract (notes are picked by index; every edit returns the indices of
//! the notes it made or changed, in the list after the edit).
//!
//! Gestures (drawing a note, moving, copying and resizing by dragging) take
//! `base`, the project as the gesture found it, and the indices as they are
//! in it, and redo their whole edit from there each frame; with the same
//! `merge` key the frames are one undo step, as clip drags are. One-shot
//! edits (delete, split, glue, mute, duplicate, nudge, transpose, legato)
//! work on the project as it is.
//!
//! A locked clip, a clip that is not there and an audio clip refuse every
//! edit: nothing changes and the selection given comes back. Only notes
//! change; the clip's controller, bend, pressure and program events stay.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::edit::{self, note_tools as tools, notes as ops, quantize};
use lumen_project::model::{Clip, ClipContent, Id, Note, Project};

use super::{Document, plural};

pub use ops::{End, MIN_NOTE};

/// A resize: which `end` of the notes is dragged and how far (`by`, later
/// when positive), none left shorter than `min` (the grid step; never below
/// [`MIN_NOTE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resize {
    pub end: End,
    pub by: Ticks,
    pub min: Ticks,
}

impl Document {
    /// Whether the notes of `clip` can be edited now: it is a MIDI clip and
    /// not locked.
    pub fn notes_editable(&self, clip: Id) -> bool {
        find(self.project(), clip).is_some()
    }

    /// Runs `change` on the notes of the unlocked MIDI clip `clip` (starting
    /// from `base` when given) as one step, and returns the selection it
    /// answers with.
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

    /// Draws `note` into the clip (clamped inside it), the way a click or a
    /// click-and-drag in the draw tool does: for a drag, pass the project
    /// from before it as `base` and the same `merge` key every frame, with
    /// the note's length growing. Returns the new note's index.
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

    /// Moves the picked notes `time` later and `pitch` semitones higher
    /// (negative for earlier and lower), as a group clamped at the clip's
    /// limits; a drag, from `base`.
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

    /// Copies the picked notes to a new place (Ctrl-drag), leaving the
    /// originals; from `base`. Returns the copies.
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

    /// Drags one end of the picked notes ([`Resize`]); a drag, from `base`.
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

    /// Deletes the picked notes. Nothing remains selected.
    pub fn delete_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        let label = plural(selection.len(), "Delete note", "Delete {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            ops::delete(notes, selection)
        })
    }

    /// Splits the picked notes that sound at `at` (clip time, see
    /// `edit::to_clip_time`).
    pub fn split_notes(&mut self, clip: Id, selection: &[usize], at: Ticks) -> Vec<usize> {
        let label = plural(selection.len(), "Split note", "Split {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            ops::split(notes, selection, at)
        })
    }

    /// Joins picked notes of one pitch that touch or overlap.
    pub fn glue_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        self.edit_notes("Glue notes", None, None, clip, selection, |notes, _| {
            ops::glue(notes, selection)
        })
    }

    /// Mutes the picked notes, unmutes them, or with `None` toggles (all
    /// unmuted if all were muted, else all muted).
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

    /// Copies the picked notes after themselves. Returns the copies.
    pub fn duplicate_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        let label = plural(selection.len(), "Duplicate note", "Duplicate {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            ops::duplicate(notes, selection, length)
        })
    }

    /// Moves the picked notes `by` in time (the arrow keys), as a group.
    pub fn nudge_notes(&mut self, clip: Id, selection: &[usize], by: Ticks) -> Vec<usize> {
        let label = plural(selection.len(), "Nudge note", "Nudge {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            ops::nudge(notes, selection, by, length)
        })
    }

    /// Transposes the picked notes by `semitones` (12 for an octave), as a
    /// group.
    pub fn transpose_notes(&mut self, clip: Id, selection: &[usize], semitones: i32) -> Vec<usize> {
        let label = plural(selection.len(), "Transpose note", "Transpose {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            ops::transpose(notes, selection, semitones)
        })
    }

    /// Lengthens each picked note to the next note's start.
    pub fn legato_notes(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        let label = plural(selection.len(), "Legato", "Legato on {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            ops::legato(notes, selection, length)
        })
    }

    /// Quantizes the picked notes with `settings` (slice 5), as one step.
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

    /// Loosens the timing and velocity of the picked notes at random,
    /// repeatably from `humanize.seed`.
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

    /// Gives every picked note the length `len`.
    pub fn fixed_length_notes(&mut self, clip: Id, selection: &[usize], len: Ticks) -> Vec<usize> {
        let label = plural(selection.len(), "Fix note length", "Fix length of {} notes");
        self.edit_notes(&label, None, None, clip, selection, |notes, length| {
            tools::fixed_length(notes, selection, length, len)
        })
    }

    /// Moves the picked notes onto the nearest pitch of `scale` from `root`.
    pub fn scale_constrain_notes(
        &mut self,
        clip: Id,
        selection: &[usize],
        root: u8,
        scale: tools::Scale,
    ) -> Vec<usize> {
        let label = plural(selection.len(), "Constrain note", "Constrain {} notes to scale");
        self.edit_notes(&label, None, None, clip, selection, |notes, _| {
            tools::scale_constrain(notes, selection, root, scale)
        })
    }

    /// Mirrors the picked notes in time.
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

/// The unlocked MIDI clip `id`.
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

    /// A document with a 16-quarter MIDI clip holding `notes` and one
    /// controller event; returns it with the clip's id.
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
        // Past the clip's end: the whole group stops with the last note's end.
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
        assert_eq!(doc.state(), state);
        assert_eq!(*doc.project(), before);
        // Unlocked again, the same edit works.
        doc.set_clips_locked(&[clip], false);
        doc.delete_notes(clip, &sel);
        assert_eq!(content(&doc).0.len(), 1);
    }

    #[test]
    fn a_missing_clip_or_an_audio_clip_is_refused() {
        let (mut doc, _) = doc_with(vec![n(0, 1, 60)]);
        let state = doc.state();
        assert_eq!(doc.delete_notes(Id(777), &[0]), [0]);
        assert_eq!(doc.state(), state);
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
        let tracks = crate::midi_play::engine_midi(
            &reopened,
            &crate::midi_play::Inputs::default(),
            &mut crate::midi_play::PortTable::default(),
            &mut crate::midi_play::Cache::default(),
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
        // The content is four quarters long: a group moved far stops there.
        let sel = doc.move_notes(&base, clip, &[0, 1], q(100), 0, None);
        let (notes, _) = content(&doc);
        assert_eq!(notes[sel[1]].end(), q(4));
        doc.undo();
        // And a note cannot be drawn in the second repeat.
        let none = doc.add_note(None, clip, n(5, 1, 60), None);
        assert!(none.is_empty());
        // Legato's last note runs to the loop's end.
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
        // The editor stores what the edit returned.
        selection.rebind(doc.project(), after.clone());
        assert!(selection.refresh(doc.project()));
        assert_eq!(selection.indices(), after.as_slice());
        // An undo changes the notes under it: cleared.
        doc.undo();
        assert!(!selection.refresh(doc.project()));
        assert!(selection.is_empty());
        // An edit that leaves the notes alone keeps the picks.
        selection.rebind(doc.project(), vec![2]);
        doc.rename_clip(clip, "Verse");
        assert!(selection.refresh(doc.project()));
        assert_eq!(selection.indices(), [2]);
        // The clip going away clears them too.
        doc.delete_clips(&[clip]);
        assert!(!selection.refresh(doc.project()));
        assert!(selection.is_empty());
    }
}
