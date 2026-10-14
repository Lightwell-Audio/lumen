//! The pointer in the notes (M5 slice 3c): presses, drags and releases,
//! and what a double-click is.

use std::sync::Arc;

use eframe::egui::{self, PointerButton, Pos2};
use lumen_core::time::Ticks;
use lumen_project::edit::notes::{End, MIN_NOTE};
use lumen_project::model::Clip;

use super::gesture::{DRAG_THRESHOLD, Env, Gesture, Kind, Resolved, tones_of};
use super::view::{EditorView, Zone};
use super::{NoteEditor, Tool, notes_of};
use crate::App;
use crate::document::Resize;

impl App {
    /// Presses, drags and releases in the notes: the gesture state machine.
    pub(super) fn editor_pointer(
        &mut self,
        ctx: &egui::Context,
        ed: &mut NoteEditor,
        view: &EditorView,
        clip: &Arc<Clip>,
        over_body: Option<Pos2>,
        modifiers: egui::Modifiers,
    ) {
        let (pressed, down, latest) = ctx.input(|i| {
            (
                i.pointer.button_pressed(PointerButton::Primary),
                i.pointer.primary_down(),
                i.pointer.latest_pos(),
            )
        });
        let notes = notes_of(clip);
        let free = modifiers.alt;
        if ed.gesture.is_none()
            && pressed
            && let Some(p) = over_body
            && !egui::Popup::is_any_open(ctx)
        {
            let (delay, dist) = ctx.options(|o| {
                (
                    o.input_options.max_double_click_delay,
                    o.input_options.max_click_dist,
                )
            });
            let timing = Timing {
                now: ctx.input(|i| i.time),
                delay,
                dist,
            };
            self.start_note_gesture(ed, view, clip, p, modifiers, timing);
        }
        let Some(gesture) = &mut ed.gesture else {
            return;
        };
        let at = latest.unwrap_or(gesture.origin);
        if (at - gesture.origin).length() > DRAG_THRESHOLD {
            gesture.dragging = true;
            // A drag is not the first press of a double-click.
            ed.last_empty_press = None;
        }
        let cell = self.grid_cell_at_zoom(clip.start, view.time.zoom);
        let step = cell.1 - cell.0;
        let content = lumen_project::edit::content_length(clip);
        let snap = |t: Ticks| self.snapped_at_zoom(t, free, view.time.zoom);
        let env = Env {
            view,
            notes,
            length: content,
            step,
            snap: &snap,
        };
        if down {
            // A box selects as it grows.
            if let Kind::Marquee { base } = &gesture.kind
                && gesture.dragging
                && let Resolved::Marquee(rect) = gesture.resolve(at, &env)
            {
                let mut picked = base.clone();
                for (i, n) in notes.iter().enumerate() {
                    if view.shows(*n) && view.note_rect(*n).intersects(rect) {
                        picked.push(i);
                    }
                }
                picked.sort_unstable();
                picked.dedup();
                ed.boxed = picked;
            }
            return;
        }
        // Released.
        let resolved = gesture.resolve(at, &env);
        let gesture = ed.gesture.take().expect("a gesture was just used");
        self.end_audition();
        let base = self.document.project().clone();
        let id = clip.id;
        let doc = &mut self.document;
        let result = match resolved {
            Resolved::Move { time, pitch, copy } if time != Ticks::ZERO || pitch != 0 => {
                Some(if copy {
                    doc.copy_notes(&base, id, &gesture.picked, time, pitch, None)
                } else {
                    doc.move_notes(&base, id, &gesture.picked, time, pitch, None)
                })
            }
            Resolved::Resize { end, by, min } if by != Ticks::ZERO => {
                Some(doc.resize_notes(&base, id, &gesture.picked, Resize { end, by, min }, None))
            }
            Resolved::Draw(note) => Some(doc.add_note(None, id, note, None)),
            _ => None,
        };
        match result {
            Some(indices) => self.select_notes(ed, indices),
            None if matches!(gesture.kind, Kind::Marquee { .. }) && gesture.dragging => {
                // The box picks what it held, once, so the roll's boxes are
                // rebuilt once for it.
                let picked = std::mem::take(&mut ed.boxed);
                self.select_notes(ed, picked);
            }
            None => {
                // A click without a drag. On one of several picked notes it
                // picks that one alone.
                let shift = modifiers.shift;
                if !gesture.dragging
                    && !shift
                    && matches!(gesture.kind, Kind::Move { .. } | Kind::Resize { .. })
                {
                    self.select_notes(ed, vec![gesture.grabbed]);
                }
            }
        }
    }

    fn start_note_gesture(
        &mut self,
        ed: &mut NoteEditor,
        view: &EditorView,
        clip: &Arc<Clip>,
        p: Pos2,
        modifiers: egui::Modifiers,
        timing: Timing,
    ) {
        let notes = notes_of(clip);
        let hit = view.note_at(notes, p);
        let free = modifiers.alt;
        let id = clip.id;
        // Only a press on empty space with the pointer can start a
        // double-click, so any other press forgets the last one.
        let previous = ed.last_empty_press.take();
        match (ed.tool, hit) {
            (Tool::Draw, Some((i, _))) => {
                // A click on a note removes it.
                let removed = self.document.delete_notes(id, &[i]);
                self.select_notes(ed, removed);
            }
            (Tool::Draw, None) => {
                self.start_draw(ed, view, clip, p, free);
            }
            (Tool::Pointer, Some((i, zone))) => {
                let mut picked = ed.picked().to_vec();
                if modifiers.shift {
                    if let Ok(at) = picked.binary_search(&i) {
                        // Shift on a picked note lets it go.
                        picked.remove(at);
                        self.select_notes(ed, picked);
                        return;
                    }
                    picked.push(i);
                } else if !picked.contains(&i) {
                    picked = vec![i];
                }
                picked.sort_unstable();
                self.select_notes(ed, picked.clone());
                let kind = match zone {
                    Zone::Start => Kind::Resize { end: End::Start },
                    Zone::End => Kind::Resize { end: End::End },
                    Zone::Body => Kind::Move {
                        copy: modifiers.command,
                    },
                };
                // A move previews every picked note as a chord; a resize, the
                // note grabbed.
                let heard: Vec<_> = match zone {
                    Zone::Body => lumen_project::edit::notes::picked(&picked, notes.len())
                        .iter()
                        .map(|&j| notes[j])
                        .collect(),
                    _ => vec![notes[i]],
                };
                self.audition_notes(clip.id, &tones_of(&heard));
                ed.gesture = Some(Gesture {
                    kind,
                    origin: p,
                    clip: clip.clone(),
                    grabbed: i,
                    picked,
                    dragging: false,
                });
            }
            (Tool::Pointer, None) => {
                // A second press on empty space, soon and near enough, draws
                // a note as the Draw tool does.
                if !modifiers.shift
                    && timing.is_double(previous, p)
                    && self.start_draw(ed, view, clip, p, free)
                {
                    return;
                }
                ed.last_empty_press = Some((timing.now, p));
                let base = if modifiers.shift {
                    ed.picked().to_vec()
                } else {
                    self.select_notes(ed, Vec::new());
                    Vec::new()
                };
                ed.gesture = Some(Gesture {
                    kind: Kind::Marquee { base },
                    origin: p,
                    clip: clip.clone(),
                    grabbed: 0,
                    picked: Vec::new(),
                    dragging: false,
                });
            }
        }
    }

    /// Starts drawing a note where the pointer is (the Draw tool's press,
    /// and a double-click with the pointer): in the grid cell under it, or
    /// exactly there with Alt or no snapping; dragging sets its length. The
    /// note is made on release. False when the press is outside the clip.
    fn start_draw(
        &mut self,
        ed: &mut NoteEditor,
        view: &EditorView,
        clip: &Arc<Clip>,
        p: Pos2,
        free: bool,
    ) -> bool {
        let abs = view.absolute_at(p.x);
        let (cell_start, cell_end) = self.grid_cell_at_zoom(abs, view.time.zoom);
        let at = if free || !self.grid.snap {
            abs
        } else {
            cell_start
        };
        let start = at - clip.start;
        let end = if free || !self.grid.snap {
            start
        } else {
            cell_end - clip.start
        };
        let content = lumen_project::edit::content_length(clip);
        if start < Ticks::ZERO || start >= content {
            return false;
        }
        let pitch = view.pitch_at(p.y).clamp(0, 127) as u8;
        self.audition_note(clip.id, pitch, super::gesture::DRAW_VELOCITY);
        ed.gesture = Some(Gesture {
            kind: Kind::Draw {
                start,
                end,
                pitch,
                step: (cell_end - cell_start).max(MIN_NOTE),
            },
            origin: p,
            clip: clip.clone(),
            grabbed: 0,
            picked: Vec::new(),
            dragging: true,
        });
        self.select_notes(ed, Vec::new());
        true
    }
}

/// When a press happened, and what egui counts as a double-click (its
/// input options): the longest wait between presses, and the farthest they
/// may be apart.
#[derive(Debug, Clone, Copy)]
struct Timing {
    now: f64,
    delay: f64,
    dist: f32,
}

impl Timing {
    /// Whether a press at `at` now is the second of a double-click that
    /// began with `previous`, the time and place of the last press on empty
    /// space.
    fn is_double(self, previous: Option<(f64, Pos2)>, at: Pos2) -> bool {
        previous.is_some_and(|(time, place)| {
            (0.0..=self.delay).contains(&(self.now - time)) && (at - place).length() <= self.dist
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn timing(now: f64) -> Timing {
        Timing {
            now,
            delay: 0.3,
            dist: 6.0,
        }
    }

    #[test]
    fn a_second_press_soon_and_near_is_a_double_click() {
        let first = Some((10.0, pos2(100.0, 100.0)));
        assert!(timing(10.2).is_double(first, pos2(103.0, 102.0)));
        assert!(
            timing(10.29).is_double(first, pos2(100.0, 100.0)),
            "just inside the delay"
        );
        assert!(
            !timing(10.31).is_double(first, pos2(100.0, 100.0)),
            "too late"
        );
        assert!(
            !timing(10.1).is_double(first, pos2(110.0, 100.0)),
            "too far"
        );
        assert!(
            !timing(9.0).is_double(first, pos2(100.0, 100.0)),
            "before it"
        );
        assert!(
            !timing(10.1).is_double(None, pos2(100.0, 100.0)),
            "no first press"
        );
    }
}
