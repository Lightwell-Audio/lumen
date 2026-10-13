//! The note edits the commands, keys and menus run (M5 slice 3c). Each is
//! one `Document` method, so each is one undo step; the selection follows
//! what the edit returns.

use lumen_core::time::Ticks;
use lumen_project::edit::{self, notes::MIN_NOTE};
use lumen_project::model::Id;

use super::{NoteEditor, notes_of};
use crate::App;
use crate::audition::Tone;

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
    /// Raise (positive) or lower the velocities by this much.
    Velocity(i32),
    /// Quantize with the last settings (slice 5); with nothing picked, every
    /// note. So are the four below.
    Quantize,
    Humanize,
    FixedLength,
    ScaleConstrain,
    Reverse,
}

/// How much Raise velocity and Lower velocity change a velocity.
pub(crate) const VELOCITY_STEP: i32 = 8;

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
        let (a, b) = self.grid_cell_at_zoom(clip.start + first, editor.time.zoom);
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
                .map(|(c, _)| (0..notes_of(&c).len()).collect())
                .unwrap_or_default(),
            NoteOp::Quantize
            | NoteOp::Humanize
            | NoteOp::FixedLength
            | NoteOp::ScaleConstrain
            | NoteOp::Reverse => {
                let Some((open, _)) = self.editor_clip(clip) else {
                    return;
                };
                let all = picked.is_empty();
                let targets = super::quantize::targets(&picked, &open);
                let step = self.editing_step(editor, &open);
                let next = match op {
                    NoteOp::Quantize => {
                        let settings = editor.quantize.options.settings(step);
                        let next = self.document.quantize_notes(clip, &targets, &settings);
                        let what = editor.quantize.options.summary(step);
                        self.toast = Some((
                            format!(
                                "Quantized {} to {what}",
                                if targets.len() == 1 {
                                    "1 note".to_owned()
                                } else {
                                    format!("{} notes", targets.len())
                                }
                            ),
                            std::time::Instant::now(),
                        ));
                        next
                    }
                    NoteOp::Humanize => {
                        let humanize = self.humanize_with(editor);
                        self.document.humanize_notes(clip, &targets, &humanize)
                    }
                    NoteOp::FixedLength => self.document.fixed_length_notes(clip, &targets, step),
                    NoteOp::ScaleConstrain => self.document.scale_constrain_notes(
                        clip,
                        &targets,
                        editor.quantize.root,
                        editor.quantize.scale.0,
                    ),
                    _ => self.document.reverse_notes(clip, &targets),
                };
                // Nothing was picked: nothing is afterwards.
                let next = if all { Vec::new() } else { next };
                if let Some(selection) = &mut editor.selection {
                    selection.rebind(self.document.project(), next);
                }
                return;
            }
            _ if picked.is_empty() => return,
            NoteOp::Delete => doc.delete_notes(clip, &picked),
            NoteOp::Duplicate => doc.duplicate_notes(clip, &picked),
            NoteOp::ToggleMute => doc.mute_notes(clip, &picked, None),
            NoteOp::Split(at) => doc.split_notes(clip, &picked, at),
            NoteOp::Glue => doc.glue_notes(clip, &picked),
            NoteOp::Legato => doc.legato_notes(clip, &picked),
            NoteOp::Transpose(semitones) => doc.transpose_notes(clip, &picked, semitones),
            NoteOp::Velocity(delta) => doc.offset_velocities(clip, &picked, delta),
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
    /// a note is picked or drawn (slice 3d): one note at a time, let go after
    /// a moment or when the gesture ends.
    pub(crate) fn audition_note(&mut self, clip: Id, pitch: u8, velocity: u8) {
        self.audition_notes(clip, &[Tone { pitch, velocity }]);
    }

    /// The notes `tones` sound on the track of `clip` as a chord: a fresh
    /// preview, letting go of the last (a press on picked notes).
    pub(crate) fn audition_notes(&mut self, clip: Id, tones: &[Tone]) {
        if let Some(track) = self.clip_track(clip) {
            self.audition_tones(track, tones, true);
        }
    }

    /// The moved notes of a gesture now sound at `tones`, in the order the
    /// gesture gave them: only the ones whose pitch changed are struck again.
    pub(crate) fn audition_moved(&mut self, clip: Id, tones: &[Tone]) {
        if let Some(track) = self.clip_track(clip) {
            self.audition_tones(track, tones, false);
        }
    }

    /// The track whose clips hold `clip`.
    fn clip_track(&self, clip: Id) -> Option<Id> {
        self.document
            .project()
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip))
            .map(|t| t.id)
    }

    /// The audition ends: the gesture ended or was cancelled, or the
    /// editor shows another clip or none.
    pub(crate) fn end_audition(&mut self) {
        self.audition_release();
    }
}
