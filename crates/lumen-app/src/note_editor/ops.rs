use lumen_core::time::Ticks;
use lumen_project::edit::{self, notes::MIN_NOTE};
use lumen_project::model::Id;

use super::{NoteEditor, notes_of};
use crate::App;
use crate::audition::Tone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoteOp {
    Delete,
    Duplicate,
    ToggleMute,
    SelectAll,
    Split(Ticks),
    Glue,
    Legato,
    Transpose(i32),
    Nudge {
        direction: i32,
        fine: bool,
    },
    Velocity(i32),
    Quantize,
    Humanize,
    FixedLength,
    ScaleConstrain,
    Reverse,
}

pub(crate) const VELOCITY_STEP: i32 = 8;

impl App {
    pub(crate) fn note_op(&mut self, op: NoteOp) {
        let mut editor = std::mem::take(&mut self.note_editor);
        self.apply_note_op(&mut editor, op);
        self.note_editor = editor;
    }

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
                        let lanes = self.drum_quantize_steps(editor, clip);
                        let next = match &lanes {
                            Some(steps) => self
                                .document
                                .quantize_notes_each(clip, &targets, &settings, steps),
                            None => self.document.quantize_notes(clip, &targets, &settings),
                        };
                        let used_lane = lanes.as_ref().is_some_and(|steps| {
                            let notes = notes_of(&open);
                            targets.iter().any(|&i| {
                                notes.get(i).is_some_and(|note| {
                                    steps.iter().any(|(input, _)| *input == note.pitch())
                                })
                            })
                        });
                        let what = if used_lane {
                            "each lane's own grid".to_owned()
                        } else {
                            editor.quantize.options.summary(step)
                        };
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

    pub(crate) fn audition_note(&mut self, clip: Id, pitch: u8, velocity: u8) {
        self.audition_notes(clip, &[Tone { pitch, velocity }]);
    }

    pub(crate) fn audition_notes(&mut self, clip: Id, tones: &[Tone]) {
        if let Some(track) = self.clip_track(clip) {
            self.audition_tones(track, tones, true);
        }
    }

    pub(crate) fn audition_moved(&mut self, clip: Id, tones: &[Tone]) {
        if let Some(track) = self.clip_track(clip) {
            self.audition_tones(track, tones, false);
        }
    }

    fn clip_track(&self, clip: Id) -> Option<Id> {
        self.document
            .project()
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip))
            .map(|t| t.id)
    }

    pub(crate) fn end_audition(&mut self) {
        self.audition_release();
    }
}
