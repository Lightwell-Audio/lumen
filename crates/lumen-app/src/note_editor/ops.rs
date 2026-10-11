//! The note edits the commands, keys and menus run (M5 slice 3c). Each is
//! one `Document` method, so each is one undo step; the selection follows
//! what the edit returns.

use lumen_core::time::Ticks;
use lumen_project::edit::{self, notes::MIN_NOTE};
use lumen_project::model::Id;

use super::{NoteEditor, notes_of};
use crate::App;

/// An edit of the open clip's picked notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoteOp {
    Delete,
    Duplicate,
    ToggleMute,
    SelectAll,
    /// Cut at this clip time.
    Split(Ticks),
    Glue,
    Legato,
    Transpose(i32),
    /// Move earlier (-1) or later (1) by a grid step, or finely.
    Nudge {
        direction: i32,
        fine: bool,
    },
}

impl App {
    /// Runs `op` on the editor's state, which is in `self`.
    pub(crate) fn note_op(&mut self, op: NoteOp) {
        let mut editor = std::mem::take(&mut self.note_editor);
        self.apply_note_op(&mut editor, op);
        self.note_editor = editor;
    }

    /// The grid step in ticks where the picked notes are: how far an arrow
    /// nudges.
    fn nudge_step(&self, clip: Id, editor: &NoteEditor) -> Ticks {
        let Some((clip, _)) = self.editor_clip(clip) else {
            return MIN_NOTE;
        };
        let first = editor
            .picked()
            .first()
            .and_then(|&i| notes_of(&clip).get(i))
            .map_or(Ticks::ZERO, |n| n.start());
        let (a, b) = self.grid_cell(clip.start + first);
        (b - a).max(MIN_NOTE)
    }

    /// Where Split cuts: the pointer's place in the editor, else the
    /// playhead, as clip time.
    pub(crate) fn split_point(&self) -> Option<Ticks> {
        let ed = &self.note_editor;
        let (clip, _) = self.editor_clip(ed.clip?)?;
        if let Some((local, _)) = ed.pointer_at.filter(|_| ed.hovered_before) {
            return Some(local);
        }
        edit::to_clip_time(&clip, self.transport.position()).map(|(local, _)| local)
    }

    pub(crate) fn apply_note_op(&mut self, editor: &mut NoteEditor, op: NoteOp) {
        let Some(clip) = editor.clip else {
            return;
        };
        let picked: Vec<usize> = editor.picked().to_vec();
        let doc = &mut self.document;
        let next = match op {
            NoteOp::SelectAll => self
                .editor_clip(clip)
                .map(|(c, _)| edit::notes::all(notes_of(&c)))
                .unwrap_or_default(),
            _ if picked.is_empty() => return,
            NoteOp::Delete => doc.delete_notes(clip, &picked),
            NoteOp::Duplicate => doc.duplicate_notes(clip, &picked),
            NoteOp::ToggleMute => doc.mute_notes(clip, &picked, None),
            NoteOp::Split(at) => doc.split_notes(clip, &picked, at),
            NoteOp::Glue => doc.glue_notes(clip, &picked),
            NoteOp::Legato => doc.legato_notes(clip, &picked),
            NoteOp::Transpose(semitones) => doc.transpose_notes(clip, &picked, semitones),
            NoteOp::Nudge { direction, fine } => {
                let step = if fine {
                    MIN_NOTE
                } else {
                    self.nudge_step(clip, editor)
                };
                self.document
                    .nudge_notes(clip, &picked, Ticks(step.0 * i64::from(direction)))
            }
        };
        if let Some(selection) = &mut editor.selection {
            selection.rebind(self.document.project(), next);
        }
    }

    /// A pitch sounds while a note is drawn or moved (slice 3d: audition).
    /// Nothing yet.
    pub(crate) fn audition(&mut self, _pitch: u8, _velocity: u8) {}

    /// The audition ends (slice 3d).
    pub(crate) fn release_audition(&mut self) {}
}
