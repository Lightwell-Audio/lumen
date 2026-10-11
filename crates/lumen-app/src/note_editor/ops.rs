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

    /// Where the keyboard's Split (Ctrl+E, the palette) cuts, as clip time:
    /// the playhead's place while the playhead is inside the clip. Only when
    /// it is outside does the pointer's place over the roll count (and only
    /// inside the clip); otherwise there is no split point. The context
    /// menu's "Split here" uses the pointer's place from when it opened
    /// (`NoteEditor::menu_at`), never this.
    pub(crate) fn split_point(&self) -> Option<Ticks> {
        let ed = &self.note_editor;
        let (clip, _) = self.editor_clip(ed.clip?)?;
        if let Some((local, _)) = edit::to_clip_time(&clip, self.transport.position()) {
            return Some(local);
        }
        ed.pointer_at
            .filter(|_| ed.hovered_before)
            .map(|(local, _)| local)
            .filter(|t| *t >= Ticks::ZERO && *t < clip.length)
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

    /// A pitch sounds on the track of `clip` (the clip being edited) while
    /// a note is picked, drawn or moved (slice 3d): one note at a time, let
    /// go after a moment or when the gesture ends.
    pub(crate) fn audition_note(&mut self, clip: Id, pitch: u8, velocity: u8) {
        let track = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip))
            .map(|t| t.id);
        if let Some(track) = track {
            self.audition_play(track, pitch, velocity);
        }
    }

    /// The audition ends: the gesture ended or was cancelled, or the
    /// editor shows another clip or none.
    pub(crate) fn end_audition(&mut self) {
        self.audition_release();
    }
}
