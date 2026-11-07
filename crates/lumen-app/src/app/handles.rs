//! Fade and gain handles on the lanes (M3 slice 6d, the owner's F3, C3 and
//! G1): drag a fade's handle along to set its length; drag its dot
//! anywhere to bend it (the curve passes through the dot), with Alt to add
//! S-shape; drag the gain line up or down (Shift for fine steps). A
//! double-click resets each; the right-click menu has the preset shapes.
//! Every drag is one undo step. Where the handles are is worked out in
//! `arrangement::handles`.
//!
//! The audio editor makes the same edits (M9.5 D10): the steps here take a
//! [`GestureFrame`], the view and lane the gesture is in, and hand back what
//! changed, so each side keeps its own gesture state and hover.

use std::sync::Arc;

use eframe::egui::{self, CursorIcon, Pos2, Rect, pos2};
use lumen_core::curve::Curve;
use lumen_core::fade::FadeShape;
use lumen_core::time::Ticks;
use lumen_project::edit;
use lumen_project::model::{Clip, ClipContent, Id, Project};

use super::grid::draw_snap_guide;
use super::info::{Held, Info, Mouse};
use crate::arrangement::handles::{ClipHandles, Handle, bend, db_per_point};
use crate::arrangement::select::{ClipHit, Edge, Gesture};
use crate::timeview::TimeView;
use crate::widgets;
use crate::{App, ruler};

/// Gain moves a tenth as far with Shift held.
const FINE: f32 = 0.1;

/// Where a clip gesture is happening: the view positions are read through,
/// and the lane its guides and values are drawn in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GestureFrame {
    pub view: TimeView,
    pub lane: Rect,
}

impl GestureFrame {
    pub(crate) fn new(view: TimeView, lane: Rect) -> Self {
        Self { view, lane }
    }

    pub(crate) fn ticks_at(self, x: f32) -> Ticks {
        ruler::ticks_at(self.view.beat_at(self.lane.left(), x))
    }

    pub(crate) fn x(self, t: Ticks) -> f32 {
        self.view.x(self.lane.left(), t.as_quarters())
    }
}

/// One trim, from the project as it was when the drag began.
pub(crate) struct TrimStep<'a> {
    pub base: &'a Project,
    pub clip: Id,
    pub edge: Edge,
    pub looping: bool,
    pub grab: Ticks,
}

/// What one step of a handle drag left: the gesture to carry on with, and
/// what to show as hot.
pub(crate) struct HandleStep {
    pub gesture: Gesture,
    pub clip: Id,
    pub handle: Handle,
}

impl App {
    /// The FR2 message: `track` is frozen, and how to thaw it.
    pub(crate) fn say_frozen(&mut self, track: &str) {
        self.say(format!(
            "{track} is frozen, so its clips can't be changed. \
             Click the lit Frozen badge on its header (or use Unfreeze track) to unfreeze it"
        ));
    }

    /// A trim of `clip`'s `edge` picked up at `x`.
    pub(crate) fn trim_gesture(
        &self,
        clip: &Clip,
        edge: Edge,
        looping: bool,
        x: f32,
        frame: GestureFrame,
    ) -> Gesture {
        let side = match edge {
            Edge::Start => clip.start,
            Edge::End => clip.start + clip.length,
        };
        Gesture::Trim {
            base: Arc::new(self.document.project().clone()),
            clip: clip.id,
            edge,
            looping,
            grab: side - frame.ticks_at(x),
        }
    }

    /// Moves a trim to the pointer at `p`, on the grid unless `free`. Draws
    /// the guide where the edge landed.
    pub(crate) fn step_trim(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        step: &TrimStep<'_>,
        p: Pos2,
        (frame, free): (GestureFrame, bool),
    ) {
        // The edge keeps its distance from the pointer: picked up a few
        // pixels inside, it does not jump to the pointer.
        let at = frame.ticks_at(p.x) + step.grab;
        let to = self.snapped_at_zoom(at, free, frame.view.zoom);
        self.document.trim_clip(
            step.base,
            step.clip,
            step.edge,
            to,
            step.looping,
            Some("clip trim"),
        );
        if let Some(c) = self.clip(step.clip) {
            let side = match step.edge {
                Edge::Start => c.start,
                Edge::End => c.start + c.length,
            };
            draw_snap_guide(painter, frame.x(side), frame.lane);
        }
        ctx.set_cursor_icon(if step.looping {
            CursorIcon::ResizeEast
        } else {
            CursorIcon::ResizeHorizontal
        });
    }

    /// Slides `clip`'s content to the pointer at `p`, picked up at `at`.
    /// Slipping is free: the audio follows the hand exactly.
    pub(crate) fn step_slip(
        &mut self,
        ctx: &egui::Context,
        base: &Project,
        clip: Id,
        at: Ticks,
        p: Pos2,
        frame: GestureFrame,
    ) {
        let by = frame.ticks_at(p.x) - at;
        self.document.slip_clip(base, clip, by, Some("clip slip"));
        ctx.set_cursor_icon(CursorIcon::ResizeColumn);
    }

    /// The clip `id`, if it exists.
    pub(crate) fn clip(&self, id: Id) -> Option<&Clip> {
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
    pub(crate) fn shown_clip(&self, id: Id) -> Option<Clip> {
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
        let gesture = self.handle_gesture(&clip, handle, &handles, origin)?;
        self.arrangement.clips = std::iter::once(hit.clip).collect();
        self.arrangement.range = None;
        self.selected_clips_now();
        Some(gesture)
    }

    /// A drag starting at `origin` on `clip`'s `handle`, drawn at `handles`.
    /// `clip` is as shown ([`Self::shown_clip`]).
    pub(crate) fn handle_gesture(
        &self,
        clip: &Clip,
        handle: Handle,
        handles: &ClipHandles,
        origin: Pos2,
    ) -> Option<Gesture> {
        Some(match handle {
            Handle::Fade(edge) => Gesture::Fade {
                clip: clip.id,
                edge,
                from: self.fade_begins(clip.id, edge),
            },
            Handle::Bend(edge) => Gesture::Bend {
                clip: clip.id,
                edge,
                crossing: clip.crossfade && edge == Edge::End,
                handle: handles.fade(edge).curve.handle(),
                last: origin,
                size: handles.fade_size(edge),
            },
            Handle::Gain => {
                let ClipContent::Audio { gain_db, .. } = clip.content else {
                    return None;
                };
                Gesture::Gain {
                    clip: clip.id,
                    db: gain_db,
                    // From where it was pressed, so it does not jump.
                    y: origin.y,
                    per_point: db_per_point(handles.wave.height()),
                }
            }
        })
    }

    /// Where a crossfade moved by this fade's handle begins: the fade-in
    /// handle of a clip a crossfade runs into lengthens that crossfade to the
    /// right, as pulling the clip before it over this one does, and where it
    /// begins stays.
    pub(crate) fn fade_begins(&self, clip: Id, edge: Edge) -> Option<Ticks> {
        (edge == Edge::Start)
            .then(|| self.clip(clip).filter(|c| c.crossfade))
            .flatten()
            .map(|c| c.start - c.fade_in.length)
    }

    /// Moves the handle of `clip`'s fade at `edge` to `at`, under `merge`.
    /// `from` is where a crossfade being made or lengthened by this handle
    /// begins. Returns it, for the next step, and the value to show.
    pub(crate) fn fade_to(
        &mut self,
        clip: Id,
        edge: Edge,
        from: Option<Ticks>,
        at: Ticks,
        merge: Option<&str>,
    ) -> Option<(Option<Ticks>, String)> {
        let c = self.shown_clip(clip)?;
        let end = c.start + c.length;
        let project = self.document.project();
        let existing = (from.is_none() && edge == Edge::End && c.crossfade)
            .then(|| self.crossing_into(clip))
            .flatten();
        let next = (edge == Edge::End && existing.is_none())
            .then(|| crate::crossfades::incoming(project, clip))
            .flatten();
        Some(if let (Edge::Start, Some(begin)) = (edge, from) {
            // The incoming clip's handle lengthens the crossfade into it to
            // the right: the clip before it is extended and this one
            // trimmed, the start of the crossfade staying where it is.
            self.document.crossfade_across(clip, begin, at, merge);
            (from, self.crossfade_text(clip))
        } else if let Some(right) = existing {
            // The handle of a crossfade sets how long it lasts, the clips
            // staying where they are.
            self.document
                .set_crossfade_length(right, (end - at).max(Ticks::ZERO), merge);
            (from, self.crossing_text(clip))
        } else if let Some(right) = next
            && (from.is_some() || at > end)
        {
            // A fade-out dragged past the end of its clip into the next one
            // makes a crossfade there: the clip is extended by the drag and
            // the next one trimmed by it. Dragged back it is as it was.
            let from = from.unwrap_or(end);
            self.document.crossfade_across(right, from, at, merge);
            (Some(from), self.crossing_text(clip))
        } else {
            let length = match edge {
                Edge::Start => at - c.start,
                Edge::End => end - at,
            };
            self.document.set_fade_length(clip, edge, length, merge);
            (from, self.fade_text(clip, edge))
        })
    }

    /// Continues a handle drag at `p`, showing the value beside the
    /// pointer. Other gestures are left alone.
    pub(crate) fn continue_handle_gesture(
        &mut self,
        ctx: &egui::Context,
        gesture: Gesture,
        p: Pos2,
        frame: GestureFrame,
        painter: &egui::Painter,
    ) -> Option<HandleStep> {
        let (fine, alt) = ctx.input(|i| (i.modifiers.shift, i.modifiers.alt));
        let at = frame.ticks_at(p.x);
        let (step, text) = match gesture {
            Gesture::Fade { clip, edge, from } => {
                // Fades follow the hand freely: they are about sound, not
                // the grid.
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                let (from, text) = self.fade_to(clip, edge, from, at, Some("clip fade"))?;
                (
                    HandleStep {
                        gesture: Gesture::Fade { clip, edge, from },
                        clip,
                        handle: Handle::Fade(edge),
                    },
                    text,
                )
            }
            Gesture::Bend {
                clip,
                edge,
                crossing,
                handle,
                last,
                size,
            } => {
                // Step by step, so Alt can come and go mid-drag. A
                // crossfade is curved like the incoming clip's fade-in.
                let mirrored = if crossing { Edge::Start } else { edge };
                let handle = bend(handle, mirrored, size, p - last, alt);
                let curve = Curve::from_handle(handle);
                self.document
                    .set_fade_curve(&[clip], edge, curve, Some("clip fade curve"));
                ctx.set_cursor_icon(CursorIcon::Move);
                // A preset's name when the curve lands on one; the S-shape
                // while Alt sets it.
                let text = if alt {
                    format!("S {:.0}%", handle.s * 100.0)
                } else {
                    FadeShape::of(&curve).map_or_else(String::new, |s| s.name().to_owned())
                };
                (
                    HandleStep {
                        gesture: Gesture::Bend {
                            clip,
                            edge,
                            crossing,
                            handle,
                            last: p,
                            size,
                        },
                        clip,
                        handle: Handle::Bend(edge),
                    },
                    text,
                )
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
                ctx.set_cursor_icon(CursorIcon::ResizeVertical);
                (
                    HandleStep {
                        gesture: Gesture::Gain {
                            clip,
                            db,
                            y: p.y,
                            per_point,
                        },
                        clip,
                        handle: Handle::Gain,
                    },
                    widgets::db_text(db),
                )
            }
            _ => return None,
        };
        if !text.is_empty() {
            let at = pos2(p.x + 14.0, p.y - 12.0);
            widgets::draw_hint(painter, at, &text, frame.lane);
        }
        Some(step)
    }

    /// The pointer is over a handle: its cursor, and what it does for the
    /// info line (M4 slice 0).
    pub(super) fn handle_hover(&self, ctx: &egui::Context, hit: &ClipHit) -> Option<Info> {
        self.handle_info(ctx, hit.clip, hit.handle?)
    }

    /// [`Self::handle_hover`] for `handle` of `clip`.
    pub(crate) fn handle_info(
        &self,
        ctx: &egui::Context,
        clip: Id,
        handle: Handle,
    ) -> Option<Info> {
        let (cursor, info) = match handle {
            Handle::Fade(edge) => {
                let crossing = match edge {
                    Edge::End => self.crossing_into(clip).is_some(),
                    Edge::Start => self.clip(clip).is_some_and(|c| c.crossfade),
                };
                let title = if crossing {
                    "Crossfade"
                } else if edge == Edge::Start {
                    "Fade in"
                } else {
                    "Fade out"
                };
                let can_cross = edge == Edge::End
                    && !crossing
                    && crate::crossfades::incoming(self.document.project(), clip).is_some();
                (
                    CursorIcon::ResizeHorizontal,
                    Info::new(title)
                        .on(
                            Held::NONE,
                            Mouse::Drag,
                            if can_cross {
                                "set its length, or drag into the next clip to crossfade"
                            } else {
                                "set its length"
                            },
                        )
                        .on(Held::NONE, Mouse::DoubleClick, "remove it")
                        .on(Held::NONE, Mouse::RightClick, "preset shapes"),
                )
            }
            Handle::Bend(_) => (
                CursorIcon::Move,
                Info::new("Fade curve")
                    .on(Held::NONE, Mouse::Drag, "bend")
                    .on(Held::ALT, Mouse::Drag, "S-shape")
                    .on(Held::NONE, Mouse::DoubleClick, "straighten"),
            ),
            Handle::Gain => {
                let db = match self.clip(clip)?.content {
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
        if let Some(handle) = hit.handle {
            self.reset_clip_handle(hit.clip, handle);
        }
    }

    /// Resets `clip`'s `handle`: no fade, a straight fade, or 0 dB.
    pub(crate) fn reset_clip_handle(&mut self, clip: Id, handle: Handle) {
        match handle {
            Handle::Fade(edge) => {
                if let Some(right) = (edge == Edge::End)
                    .then(|| self.crossing_into(clip))
                    .flatten()
                {
                    // The crossing at this end is what the handle shows.
                    self.document.set_crossfade_length(right, Ticks::ZERO, None);
                } else {
                    self.document.set_fade_length(clip, edge, Ticks::ZERO, None);
                }
            }
            Handle::Bend(edge) => {
                self.document
                    .set_fade_curve(&[clip], edge, Curve::LINEAR, None);
            }
            Handle::Gain => self.document.set_clip_gain(&[clip], 0.0, None),
        }
    }

    /// "125 ms" or "1.50 s": how long a fade lasts.
    pub(crate) fn fade_text(&self, clip: Id, edge: Edge) -> String {
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

    /// How long the crossfade into `right` lasts, for beside the pointer.
    pub(crate) fn crossfade_text(&self, right: Id) -> String {
        let Some(c) = self.clip(right) else {
            return String::new();
        };
        let tempo = edit::tempo_of(self.document.project());
        let seconds = tempo.seconds_at(c.start) - tempo.seconds_at(c.start - c.fade_in.length);
        if seconds < 1.0 {
            format!("Crossfade {:.0} ms", seconds * 1_000.0)
        } else {
            format!("Crossfade {seconds:.2} s")
        }
    }

    /// The clip that `left`'s crossfade by hand goes into, if it ends one.
    pub(crate) fn crossing_into(&self, left: Id) -> Option<Id> {
        let project = self.document.project();
        let track = project
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == left))?;
        let clip = track.clips.iter().find(|c| c.id == left)?;
        crate::crossfades::outgoing(project, track.id, clip).map(|(right, _)| right)
    }

    /// What a crossfade at the end of `left` shows beside the pointer: how
    /// long it lasts.
    pub(crate) fn crossing_text(&self, left: Id) -> String {
        if self.crossing_into(left).is_some() {
            format!("Crossfade {}", self.fade_text(left, Edge::End))
        } else {
            "No audio to crossfade".to_owned()
        }
    }

    /// The preset shapes of `clip`'s fade at `edge`, and Straighten, for the
    /// audio editor's menu on a fade (M9.5 D19): named as in the Fades submenu.
    pub(crate) fn fade_shape_rows(&mut self, ui: &mut egui::Ui, clip: Id, edge: Edge) {
        let Some(curve) = self.document.fade_curve(clip, edge) else {
            return;
        };
        let now = FadeShape::of(&curve);
        let label = match edge {
            Edge::Start => "Fade in",
            Edge::End => "Fade out",
        };
        widgets::menu_caption(ui, label);
        let names = FadeShape::ALL.map(|s| format!("{label}: {}", s.name()));
        let shapes: Vec<_> = FadeShape::ALL
            .iter()
            .zip(&names)
            .map(|(s, name)| widgets::Choice::new(Some(*s), s.name()).named(name))
            .collect();
        if let Some(Some(shape)) = widgets::segmented(ui, ui.id().with(label), now, &shapes) {
            self.document
                .set_fade_curve(&[clip], edge, shape.curve(), None);
            ui.close();
        }
        if widgets::menu_item(ui, "Straighten", None, true).clicked() {
            self.document
                .set_fade_curve(&[clip], edge, Curve::LINEAR, None);
            ui.close();
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
        let current = [Edge::Start, Edge::End].map(|edge| {
            let curve = self.document.fade_curve(first.id, edge);
            FadeShape::of(&curve.unwrap_or(Curve::LINEAR))
        });
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
