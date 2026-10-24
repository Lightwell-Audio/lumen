//! Fade and gain handles on the lanes (M3 slice 6d, the owner's F3, C3 and
//! G1): drag a fade's handle along to set its length; drag its dot
//! anywhere to bend it (the curve passes through the dot), with Alt to add
//! S-shape; drag the gain line up or down (Shift for fine steps). A
//! double-click resets each; the right-click menu has the preset shapes.
//! Every drag is one undo step. Where the handles are is worked out in
//! `arrangement::handles`.

use eframe::egui::{self, CursorIcon, Pos2, Rect, pos2};
use lumen_core::curve::Curve;
use lumen_core::fade::FadeShape;
use lumen_core::time::Ticks;
use lumen_project::edit;
use lumen_project::model::{Clip, ClipContent, Id};

use super::info::{Held, Info, Mouse};
use crate::arrangement::handles::{ClipHandles, Handle, bend, db_per_point};
use crate::arrangement::select::{ClipHit, ClipRegion, Edge, Gesture, Hover};
use crate::widgets;
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

    /// A temporary drawing/editing view with the eligible automatic fade
    /// resolved. The preview never changes the saved project.
    fn shown_clip(&self, id: Id) -> Option<Clip> {
        let project = self.document.project();
        let track = project
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == id))?;
        let clip = track.clips.iter().find(|c| c.id == id)?;
        Some(crate::crossfades::preview(project, track.id, clip))
    }

    /// A drag starting at `origin` on the handle of `hit`: it selects the
    /// clip alone.
    pub(super) fn start_handle_gesture(
        &mut self,
        hit: &ClipHit,
        origin: Pos2,
        canvas: Rect,
    ) -> Option<Gesture> {
        let handle = hit.handle?;
        let clip = self.shown_clip(hit.clip)?;
        let view = self.nav.view;
        let handles = ClipHandles::of(&clip, hit.rect, |t| view.x(canvas.left(), t.as_quarters()))?;
        let gesture = match handle {
            Handle::Fade(edge) => Gesture::Fade {
                clip: hit.clip,
                edge,
            },
            Handle::Bend(edge) => Gesture::Bend {
                clip: hit.clip,
                edge,
                handle: handles.fade(edge).curve.handle(),
                last: origin,
                size: handles.fade_size(edge),
            },
            Handle::Gain => {
                let ClipContent::Audio { gain_db, .. } = clip.content else {
                    return None;
                };
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
        let (fine, alt) = ctx.input(|i| (i.modifiers.shift, i.modifiers.alt));
        let at = ruler::ticks_at(self.nav.view.beat_at(canvas.left(), p.x));
        let (clip, handle, text) = match gesture {
            Gesture::Fade { clip, edge } => {
                let Some(c) = self.shown_clip(clip) else {
                    return;
                };
                // Fades follow the hand freely: they are about sound, not
                // the grid.
                let length = match edge {
                    Edge::Start => at - c.start,
                    Edge::End => c.start + c.length - at,
                };
                if edge == Edge::Start && c.crossfade {
                    self.document
                        .set_crossfade_length(clip, length, Some("clip fade"));
                } else {
                    self.document
                        .set_fade_length(clip, edge, length, Some("clip fade"));
                }
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                (clip, Handle::Fade(edge), self.fade_text(clip, edge))
            }
            Gesture::Bend {
                clip,
                edge,
                handle,
                last,
                size,
            } => {
                // Step by step, so Alt can come and go mid-drag.
                let handle = bend(handle, edge, size, p - last, alt);
                let curve = Curve::from_handle(handle);
                self.document
                    .set_fade_curve(&[clip], edge, curve, Some("clip fade curve"));
                self.arrangement.gesture = Some(Gesture::Bend {
                    clip,
                    edge,
                    handle,
                    last: p,
                    size,
                });
                ctx.set_cursor_icon(CursorIcon::Move);
                // A preset's name when the curve lands on one; the S-shape
                // while Alt sets it.
                let text = if alt {
                    format!("S {:.0}%", handle.s * 100.0)
                } else {
                    FadeShape::of(&curve).map_or_else(String::new, |s| s.name().to_owned())
                };
                (clip, Handle::Bend(edge), text)
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
            region: ClipRegion::Header,
            edge: None,
            handle: Some(handle),
        });
        if !text.is_empty() {
            let at = pos2(p.x + 14.0, p.y - 12.0);
            widgets::draw_hint(painter, at, &text, canvas);
        }
    }

    /// The pointer is over a handle: its cursor, and what it does for the
    /// info line (M4 slice 0).
    pub(super) fn handle_hover(&self, ctx: &egui::Context, hit: &ClipHit) -> Option<Info> {
        let (cursor, info) = match hit.handle? {
            Handle::Fade(edge) => (
                CursorIcon::ResizeHorizontal,
                Info::new(
                    if edge == Edge::Start && self.shown_clip(hit.clip).is_some_and(|c| c.crossfade)
                    {
                        "Crossfade"
                    } else if edge == Edge::Start
                        && self.clip(hit.clip).is_some_and(|c| c.crossfade)
                    {
                        "No source tail at this join"
                    } else if edge == Edge::Start {
                        "Fade in"
                    } else {
                        "Fade out"
                    },
                )
                .on(Held::NONE, Mouse::Drag, "set its length")
                .on(Held::NONE, Mouse::DoubleClick, "remove it")
                .on(Held::NONE, Mouse::RightClick, "preset shapes"),
            ),
            Handle::Bend(_) => (
                CursorIcon::Move,
                Info::new("Fade curve")
                    .on(Held::NONE, Mouse::Drag, "bend")
                    .on(Held::ALT, Mouse::Drag, "S-shape")
                    .on(Held::NONE, Mouse::DoubleClick, "straighten"),
            ),
            Handle::Gain => {
                let db = match self.clip(hit.clip)?.content {
                    ClipContent::Audio { gain_db, .. } => gain_db,
                    ClipContent::Midi { .. } => return None,
                };
                (
                    CursorIcon::ResizeVertical,
                    Info::new(format!("Clip gain {}", widgets::db_text(db)))
                        .on(Held::NONE, Mouse::Drag, "change")
                        .on(Held::SHIFT, Mouse::Drag, "fine")
                        .on(Held::NONE, Mouse::DoubleClick, "0 dB"),
                )
            }
        };
        ctx.set_cursor_icon(cursor);
        Some(info)
    }

    /// A double-click on a handle resets it: no fade, a straight fade, or
    /// 0 dB.
    pub(super) fn reset_handle(&mut self, hit: &ClipHit) {
        match hit.handle {
            Some(Handle::Fade(edge)) => {
                if edge == Edge::Start
                    && (self.shown_clip(hit.clip).is_some_and(|c| c.crossfade)
                        || self.clip(hit.clip).is_some_and(|c| c.crossfade))
                {
                    self.document
                        .set_crossfade_length(hit.clip, Ticks::ZERO, None);
                } else {
                    self.document
                        .set_fade_length(hit.clip, edge, Ticks::ZERO, None);
                }
            }
            Some(Handle::Bend(edge)) => {
                self.document
                    .set_fade_curve(&[hit.clip], edge, Curve::LINEAR, None);
            }
            Some(Handle::Gain) => self.document.set_clip_gain(&[hit.clip], 0.0, None),
            None => {}
        }
    }

    /// "125 ms" or "1.50 s": how long a fade lasts.
    fn fade_text(&self, clip: Id, edge: Edge) -> String {
        let Some(c) = self.shown_clip(clip) else {
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

    /// The preset fade shapes for the selected audio clips, in one Fades
    /// submenu of the clip menu (owner's A, MT-APP-14 run 2): the fade in's
    /// and the fade out's, under headings; the ones in use are ticked.
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
        let current = [
            FadeShape::of(&first.fade_in.curve),
            FadeShape::of(&first.fade_out.curve),
        ];
        widgets::submenu(ui, "Fades", |ui| {
            for (edge, label, now) in [
                (Edge::Start, "Fade in", current[0]),
                (Edge::End, "Fade out", current[1]),
            ] {
                widgets::menu_caption(ui, label);
                let names = FadeShape::ALL.map(|s| format!("{label}: {}", s.name()));
                let shapes: Vec<_> = FadeShape::ALL
                    .iter()
                    .zip(&names)
                    .map(|(s, name)| widgets::Choice::new(Some(*s), s.name()).named(name))
                    .collect();
                if let Some(Some(shape)) = widgets::segmented(ui, ui.id().with(label), now, &shapes)
                {
                    self.document
                        .set_fade_curve(&ids, edge, shape.curve(), None);
                    ui.close();
                }
            }
        });
    }
}
