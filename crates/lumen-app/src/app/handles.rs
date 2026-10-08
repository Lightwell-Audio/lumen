//! Fade and gain handles on the lanes (M3 slice 6d, the owner's F3 and
//! G1): drag a fade's handle along to set its length, its middle dot up or
//! down to bend it, and the gain line up or down (Shift for fine steps).
//! A double-click resets each; the right-click menu picks any fade shape.
//! Every drag is one undo step. Where the handles are is worked out in
//! `arrangement::handles`.

use eframe::egui::{self, CursorIcon, Pos2, Rect, pos2};
use lumen_core::fade::FadeShape;
use lumen_core::time::Ticks;
use lumen_project::edit;
use lumen_project::model::{Clip, ClipContent, Id};

use crate::arrangement::handles::{ClipHandles, Handle, bend_shape, db_per_point};
use crate::arrangement::select::{ClipHit, Edge, Gesture, Half, Hover};
use crate::widgets::{self, HintMark};
use crate::{App, ruler};

/// Gain moves a tenth as far with Shift held.
const FINE: f32 = 0.1;

impl App {
    /// The clip `id`, if it exists.
    fn clip(&self, id: Id) -> Option<&Clip> {
        self.document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| c.id == id)
            .map(|c| &**c)
    }

    /// A drag starting at `origin` on the handle of `hit`: it selects the
    /// clip alone.
    pub(super) fn start_handle_gesture(&mut self, hit: &ClipHit, origin: Pos2) -> Option<Gesture> {
        let handle = hit.handle?;
        let clip = self.clip(hit.clip)?;
        let gesture = match handle {
            Handle::Fade(edge) => Gesture::Fade {
                clip: hit.clip,
                edge,
            },
            Handle::Bend(edge) => Gesture::Bend {
                clip: hit.clip,
                edge,
                start: match edge {
                    Edge::Start => clip.fade_in.shape,
                    Edge::End => clip.fade_out.shape,
                },
                y: origin.y,
            },
            Handle::Gain => {
                let ClipContent::Audio { gain_db, .. } = clip.content else {
                    return None;
                };
                let handles = ClipHandles::of(clip, hit.rect, |_| hit.rect.left())?;
                Gesture::Gain {
                    clip: hit.clip,
                    db: gain_db,
                    // From where it was pressed, so it does not jump.
                    y: origin.y,
                    per_point: db_per_point(handles.wave.height()),
                }
            }
        };
        self.arrangement.clips = std::iter::once(hit.clip).collect();
        self.arrangement.range = None;
        self.selected_clips_now();
        Some(gesture)
    }

    /// Continues a handle drag at `p`, showing the value beside the
    /// pointer. Other gestures are left alone.
    pub(super) fn continue_handle_gesture(
        &mut self,
        ctx: &egui::Context,
        gesture: Gesture,
        p: Pos2,
        canvas: Rect,
        painter: &egui::Painter,
    ) {
        let fine = ctx.input(|i| i.modifiers.shift);
        let at = ruler::ticks_at(self.nav.view.beat_at(canvas.left(), p.x));
        let (clip, handle, text) = match gesture {
            Gesture::Fade { clip, edge } => {
                let Some(c) = self.clip(clip) else {
                    return;
                };
                // Fades follow the hand freely: they are about sound, not
                // the grid.
                let length = match edge {
                    Edge::Start => at - c.start,
                    Edge::End => c.start + c.length - at,
                };
                self.document
                    .set_fade_length(clip, edge, length, Some("clip fade"));
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                (clip, Handle::Fade(edge), self.fade_text(clip, edge))
            }
            Gesture::Bend {
                clip,
                edge,
                start,
                y,
            } => {
                let shape = bend_shape(start, y - p.y);
                self.document
                    .set_fade_shape(&[clip], edge, shape, Some("clip fade shape"));
                ctx.set_cursor_icon(CursorIcon::ResizeVertical);
                (clip, Handle::Bend(edge), shape.name().to_owned())
            }
            Gesture::Gain {
                clip,
                db,
                y,
                per_point,
            } => {
                // Step by step, so Shift can come and go mid-drag.
                let step = if fine { per_point * FINE } else { per_point };
                let range = crate::document::CLIP_GAIN;
                let db = (db - (p.y - y) * step).clamp(*range.start(), *range.end());
                self.document.set_clip_gain(&[clip], db, Some("clip gain"));
                self.arrangement.gesture = Some(Gesture::Gain {
                    clip,
                    db,
                    y: p.y,
                    per_point,
                });
                ctx.set_cursor_icon(CursorIcon::ResizeVertical);
                (clip, Handle::Gain, widgets::db_text(db))
            }
            _ => return,
        };
        self.arrangement.hover = Some(Hover {
            clip,
            half: Half::Top,
            edge: None,
            handle: Some(handle),
        });
        widgets::draw_hint(
            painter,
            pos2(p.x + 14.0, p.y - 12.0),
            HintMark::None,
            &text,
            canvas,
        );
    }

    /// The pointer is over a handle: its cursor and a short hint.
    pub(super) fn handle_hover(&self, ctx: &egui::Context, hit: &ClipHit) -> Option<String> {
        let (cursor, text) = match hit.handle? {
            Handle::Fade(Edge::Start) => (CursorIcon::ResizeHorizontal, "fade in".to_owned()),
            Handle::Fade(Edge::End) => (CursorIcon::ResizeHorizontal, "fade out".to_owned()),
            Handle::Bend(_) => (CursorIcon::ResizeVertical, "bend".to_owned()),
            Handle::Gain => {
                let db = match self.clip(hit.clip)?.content {
                    ClipContent::Audio { gain_db, .. } => gain_db,
                    ClipContent::Midi { .. } => return None,
                };
                (
                    CursorIcon::ResizeVertical,
                    format!("gain {}", widgets::db_text(db)),
                )
            }
        };
        ctx.set_cursor_icon(cursor);
        Some(text)
    }

    /// A double-click on a handle resets it: no fade, a straight fade, or
    /// 0 dB.
    pub(super) fn reset_handle(&mut self, hit: &ClipHit) {
        match hit.handle {
            Some(Handle::Fade(edge)) => {
                self.document
                    .set_fade_length(hit.clip, edge, Ticks::ZERO, None)
            }
            Some(Handle::Bend(edge)) => {
                self.document
                    .set_fade_shape(&[hit.clip], edge, FadeShape::Linear, None);
            }
            Some(Handle::Gain) => self.document.set_clip_gain(&[hit.clip], 0.0, None),
            None => {}
        }
    }

    /// "125 ms" or "1.50 s": how long a fade lasts.
    fn fade_text(&self, clip: Id, edge: Edge) -> String {
        let Some(c) = self.clip(clip) else {
            return String::new();
        };
        let (from, length) = match edge {
            Edge::Start => (c.start, c.fade_in.length),
            Edge::End => (c.start + c.length - c.fade_out.length, c.fade_out.length),
        };
        let tempo = edit::tempo_of(self.document.project());
        let seconds = tempo.seconds_at(from + length) - tempo.seconds_at(from);
        if seconds < 1.0 {
            format!("{:.0} ms", seconds * 1_000.0)
        } else {
            format!("{seconds:.2} s")
        }
    }

    /// The fade shapes of the selected audio clips, for the lanes' menu:
    /// any shape for the fade in and the fade out.
    pub(super) fn fade_menu(&mut self, ui: &mut egui::Ui) {
        let project = self.document.project();
        let clips: Vec<&Clip> = project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| self.arrangement.clips.contains(&c.id))
            .filter(|c| matches!(c.content, ClipContent::Audio { .. }) && !c.locked)
            .map(|c| &**c)
            .collect();
        let Some(first) = clips.first() else {
            return;
        };
        let ids: Vec<Id> = clips.iter().map(|c| c.id).collect();
        let current = [first.fade_in.shape, first.fade_out.shape];
        for (edge, label, now) in [
            (Edge::Start, "Fade-in shape", current[0]),
            (Edge::End, "Fade-out shape", current[1]),
        ] {
            ui.menu_button(label, |ui| {
                for shape in FadeShape::ALL {
                    let mut chosen = now;
                    if ui.radio_value(&mut chosen, shape, shape.name()).clicked() {
                        self.document.set_fade_shape(&ids, edge, shape, None);
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
    }
}
