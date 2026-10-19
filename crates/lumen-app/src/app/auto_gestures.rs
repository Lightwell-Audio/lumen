//! The gestures on an automation lane (M7 slices A1 and A2): moving the
//! picked points, dragging out a box, drawing freehand, drawing a shape and
//! bending a segment. A press starts one (`auto_begin`), every frame of the
//! drag rebuilds the lane from the lane as it was at the press plus the drag
//! (`auto_update`), and the release lets go (`auto_finish`). A gesture's
//! document edits are all one undo step.
//!
//! Time snaps to the grid and values to the places a control rests (0 dB,
//! the centre of pan...); Alt places freely. The pencil is the exception:
//! it follows the pointer to the pixel, because a stroke cut to the grid
//! would not be a stroke.

use std::collections::BTreeSet;

use eframe::egui::{self, CursorIcon, Modifiers, Pos2, Rect, vec2};
use lumen_core::automation::Shape;
use lumen_core::curve::Curve;
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::edit::automation as pure;
use lumen_project::model::Id;

use super::grid::draw_snap_guide;
use crate::App;
use crate::arrangement::auto_scale as scale;
use crate::arrangement::auto_state::{
    Active, Bending, Boxing, Drawing, Gesture, Moving, Picked, Picks, Tool, Waving,
};
use crate::arrangement::auto_view::{Frame, inserted};
use crate::arrangement::handles;
use crate::arrangement::select::Edge;
use crate::theme;

impl App {
    /// The lane's points as the document holds them.
    pub(super) fn lane_points_of(
        &self,
        track: Id,
        target: Target,
    ) -> Option<lumen_core::automation::Lane> {
        self.document
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.lane(target))
            .map(|l| l.lane.clone())
    }

    /// `height` taken to a resting place of the control if one is near.
    fn snap_to_rest(target: Target, frame: &Frame, height: f32, free: bool) -> (f32, bool) {
        if free {
            return (height, false);
        }
        let reach = theme::size::AUTO_SNAP / frame.area().height().max(1.0);
        scale::snap_height(target, height, reach)
    }

    /// Adds a point to `track`'s lane for `target` (a new lane if it has
    /// none) and picks it. `gesture` joins the edit to the drag it starts.
    pub(super) fn add_auto_point(
        &mut self,
        track: Id,
        target: Target,
        at: Ticks,
        value: f32,
        gesture: bool,
    ) -> Option<Picked> {
        let lane = self.lane_points_of(track, target);
        let Some((made, index)) = inserted(target, lane.as_ref(), at, value) else {
            self.arrangement.note = Some("A lane holds 65,536 points at most");
            return None;
        };
        match self.document.set_lane(track, target, made, gesture) {
            Ok(()) => {
                let here = Picked {
                    track,
                    target,
                    index,
                };
                self.arrangement.auto.picks = Some(Picks::only(here));
                Some(here)
            }
            Err(refused) => {
                self.arrangement.note = Some(refused.reason());
                None
            }
        }
    }

    /// A press that becomes a drag.
    pub(super) fn auto_begin(
        &mut self,
        track: Id,
        target: Target,
        frame: &Frame,
        origin: Pos2,
        modifiers: Modifiers,
    ) {
        self.arrangement.auto.focus = true;
        let lane = self.lane_points_of(track, target);
        let free = modifiers.alt;
        match self.arrangement.auto.tool {
            Tool::Pointer => {
                let picks = self.arrangement.auto.picked_on(track, target).cloned();
                if let (Some(lane), Some(one)) = (&lane, picks.as_ref().and_then(Picks::single))
                    && let Some(dot) = frame.bend_dot(target, lane, one.index)
                    && dot.distance(origin) <= theme::size::AUTO_GRAB
                {
                    let point = lane.points()[one.index];
                    let handle = match point.shape {
                        Shape::Curve(curve) => curve.handle(),
                        _ => Curve::LINEAR.handle(),
                    };
                    self.begin(
                        track,
                        target,
                        Gesture::Bend(Bending {
                            base: lane.clone(),
                            index: one.index,
                            handle,
                            press: origin,
                        }),
                    );
                    return;
                }
                if let Some(index) = lane.as_ref().and_then(|l| frame.nearest(target, l, origin)) {
                    let here = Picked {
                        track,
                        target,
                        index,
                    };
                    if modifiers.shift {
                        self.auto_toggle_pick(here);
                        return;
                    }
                    if !picks.as_ref().is_some_and(|p| p.contains(here)) {
                        self.arrangement.auto.picks = Some(Picks::only(here));
                    }
                    self.begin_move(frame, here, origin);
                    return;
                }
                if modifiers.shift {
                    self.arrangement.auto.picks = None;
                    self.begin(
                        track,
                        target,
                        Gesture::Box(Boxing {
                            from: origin,
                            to: origin,
                        }),
                    );
                    return;
                }
                let at = self.snapped(frame.ticks_at(origin.x), free);
                let (height, _) =
                    Self::snap_to_rest(target, frame, frame.height_at(origin.y), free);
                let value = scale::value(target, height);
                if let Some(here) = self.add_auto_point(track, target, at, value, true) {
                    self.begin_move(frame, here, origin);
                }
            }
            Tool::Pencil => {
                let base = lane.unwrap_or_default();
                self.arrangement.auto.picks = None;
                self.begin(
                    track,
                    target,
                    Gesture::Draw(Drawing {
                        base,
                        raw: Vec::new(),
                    }),
                );
                self.auto_update(None, origin, frame, modifiers);
            }
            Tool::Shape(kind) => {
                let base = lane.unwrap_or_default();
                self.arrangement.auto.picks = None;
                let from = self
                    .snapped(frame.ticks_at(origin.x), free)
                    .max(Ticks::ZERO);
                let (start, _) = Self::snap_to_rest(target, frame, frame.height_at(origin.y), free);
                let (low, high) = self.grid_cell(from);
                self.begin(
                    track,
                    target,
                    Gesture::Wave(Waving {
                        base,
                        kind,
                        from,
                        start,
                        end: start,
                        to: from,
                        period: (high - low).max(Ticks(1)),
                    }),
                );
            }
        }
    }

    fn begin(&mut self, track: Id, target: Target, how: Gesture) {
        self.arrangement.auto.active = Some(Active { track, target, how });
    }

    /// Takes hold of the picked points by `here`.
    fn begin_move(&mut self, frame: &Frame, here: Picked, origin: Pos2) {
        let Some(base) = self.lane_points_of(here.track, here.target) else {
            return;
        };
        let Some(point) = base.points().get(here.index).copied() else {
            return;
        };
        let picks = self
            .arrangement
            .auto
            .picked_on(here.track, here.target)
            .map(Picks::list)
            .filter(|l| l.contains(&here.index))
            .unwrap_or_else(|| vec![here.index]);
        self.begin(
            here.track,
            here.target,
            Gesture::Move(Moving {
                base,
                picks,
                anchor: here.index,
                anchor_at: point.at,
                anchor_height: scale::fraction(here.target, point.value),
                press_at: frame.ticks_at(origin.x),
                press_height: frame.height_at(origin.y),
            }),
        );
    }

    /// Shift on a point adds it to the picks, or takes it from them.
    pub(super) fn auto_toggle_pick(&mut self, here: Picked) {
        let auto = &mut self.arrangement.auto;
        match &mut auto.picks {
            Some(picks) if picks.on(here.track, here.target) => {
                if !picks.indices.remove(&here.index) {
                    picks.indices.insert(here.index);
                }
                if picks.indices.is_empty() {
                    auto.picks = None;
                }
            }
            _ => auto.picks = Some(Picks::only(here)),
        }
    }

    /// The gesture under way follows the pointer at `p`. `painter` draws the
    /// guides when there is one.
    pub(super) fn auto_update(
        &mut self,
        painter: Option<(&egui::Painter, Rect)>,
        p: Pos2,
        frame: &Frame,
        modifiers: Modifiers,
    ) {
        let Some(mut active) = self.arrangement.auto.active.take() else {
            return;
        };
        let (track, target) = (active.track, active.target);
        let free = modifiers.alt;
        match &mut active.how {
            Gesture::Move(moving) => {
                let want = moving.anchor_at + (frame.ticks_at(p.x) - moving.press_at);
                let at = self.snapped(want, free).max(Ticks::ZERO);
                let height = moving.anchor_height + (frame.height_at(p.y) - moving.press_height);
                let (height, rested) = Self::snap_to_rest(target, frame, height, free);
                let by = height - moving.anchor_height;
                let map = |v: f32| scale::value(target, scale::fraction(target, v) + by);
                if let Some(done) =
                    pure::shifted(&moving.base, &moving.picks, at - moving.anchor_at, map)
                {
                    let _ = self.document.set_lane(track, target, done.lane, true);
                    if let Some((painter, area)) = painter {
                        draw_snap_guide(painter, frame.x(moving.anchor_at + done.moved_by), area);
                        if rested {
                            let y = frame.y_at_height(height);
                            painter.hline(
                                frame.row.x_range(),
                                y,
                                egui::Stroke::new(
                                    theme::stroke::HAIRLINE,
                                    theme::p().accent.gamma_multiply(0.5),
                                ),
                            );
                        }
                    }
                }
                if let Some((painter, _)) = painter {
                    painter.ctx().set_cursor_icon(CursorIcon::Grabbing);
                }
            }
            Gesture::Box(boxing) => {
                boxing.to = frame.row.clamp(p);
                let rect = Rect::from_two_pos(boxing.from, boxing.to);
                let mut inside = BTreeSet::new();
                if let Some(lane) = self.lane_points_of(track, target) {
                    for (i, point) in lane.points().iter().enumerate() {
                        if rect.contains(frame.pos(target, point)) {
                            inside.insert(i);
                        }
                    }
                }
                self.arrangement.auto.picks = (!inside.is_empty()).then_some(Picks {
                    track,
                    target,
                    indices: inside,
                });
            }
            Gesture::Draw(drawing) => {
                let at = frame.ticks_at(p.x).max(Ticks::ZERO);
                let value = scale::value(target, frame.height_at(p.y));
                // Going back over what was drawn takes it back.
                drawing.raw.retain(|(t, _)| *t < at);
                drawing.raw.push((at, value));
                let steps = target == Target::Mute;
                let tolerance = stroke_tolerance(target, frame);
                if let Some(lane) = pure::stroke(&drawing.base, &drawing.raw, steps, tolerance) {
                    let _ = self.document.set_lane(track, target, lane, true);
                }
                if let Some((painter, _)) = painter {
                    painter.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
            }
            Gesture::Wave(waving) => {
                waving.to = self.snapped(frame.ticks_at(p.x), free).max(Ticks::ZERO);
                waving.end = Self::snap_to_rest(target, frame, frame.height_at(p.y), free).0;
                let points = pure::wave(
                    waving.kind,
                    waving.from,
                    waving.to,
                    waving.period,
                    waving.start,
                    waving.end,
                    |h| scale::value(target, h),
                );
                let (a, b) = (waving.from.min(waving.to), waving.from.max(waving.to));
                if !points.is_empty()
                    && let Some(lane) = pure::replace_span(&waving.base, a, b, &points)
                {
                    let _ = self.document.set_lane(track, target, lane, true);
                }
                if let Some((painter, area)) = painter {
                    draw_snap_guide(painter, frame.x(waving.to), area);
                    painter.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
            }
            Gesture::Bend(bending) => {
                let points = bending.base.points();
                if let (Some(from), Some(to)) =
                    (points.get(bending.index), points.get(bending.index + 1))
                {
                    let size = vec2(
                        (frame.x(to.at) - frame.x(from.at)).abs(),
                        (frame.y(target, to.value) - frame.y(target, from.value)).abs(),
                    );
                    // The dot's height is a share of the way from this value to
                    // the next, so a falling segment reads upside down.
                    let falling = to.value < from.value;
                    let mut by = p - bending.press;
                    if falling {
                        by.y = -by.y;
                    }
                    let handle =
                        handles::bend(bending.handle, Edge::Start, size, by, modifiers.alt);
                    let shape = Shape::Curve(Curve::from_handle(handle));
                    if let Some(lane) = pure::with_shape(&bending.base, &[bending.index], shape) {
                        let _ = self.document.set_lane(track, target, lane, true);
                    }
                }
            }
        }
        self.arrangement.auto.active = Some(active);
    }

    /// The release: the gesture is done, and the document already holds it.
    pub(super) fn auto_finish(&mut self) {
        let Some(active) = self.arrangement.auto.active.take() else {
            return;
        };
        // A stroke and a shape make new points: the old indices mean nothing.
        if matches!(active.how, Gesture::Draw(_) | Gesture::Wave(_)) {
            self.arrangement.auto.picks = None;
        }
    }
}

/// How far a stroke may be thinned from what was drawn: a pixel and a half
/// of the lane's height, in the target's units.
fn stroke_tolerance(target: Target, frame: &Frame) -> f32 {
    let reach = 1.5 / frame.area().height().max(1.0);
    (scale::value(target, 0.5 + reach) - scale::value(target, 0.5)).abs()
}
