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
    Active, Bending, Boxing, Drawing, Gesture, Hands, Moving, Picked, Picks, Tool, Waving,
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
            .and_then(|t| t.lane_view(target))
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
    /// none) and picks it, on the arrangement's lanes. `gesture` joins the
    /// edit to the drag it starts.
    pub(super) fn add_auto_point(
        &mut self,
        track: Id,
        target: Target,
        at: Ticks,
        value: f32,
        gesture: bool,
    ) -> Option<Picked> {
        let mut hands = self.arrangement.auto.lend();
        let added = self.add_lane_point(&mut hands, track, target, at, value, gesture);
        self.arrangement.auto.restore(hands);
        added
    }

    /// Adds a point and picks it in `hands`.
    pub(crate) fn add_lane_point(
        &mut self,
        hands: &mut Hands,
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
                hands.picks = Some(Picks::only(here));
                Some(here)
            }
            Err(refused) => {
                self.arrangement.note = Some(refused.reason());
                None
            }
        }
    }

    /// A press that becomes a drag, on the arrangement's lanes.
    pub(super) fn auto_begin(
        &mut self,
        track: Id,
        target: Target,
        frame: &Frame,
        origin: Pos2,
        modifiers: Modifiers,
    ) {
        let mut hands = self.arrangement.auto.lend();
        self.lane_begin(&mut hands, track, target, frame, origin, modifiers);
        self.arrangement.auto.restore(hands);
    }

    /// A press that becomes a drag, with `hands`' tool, picks and gesture.
    pub(crate) fn lane_begin(
        &mut self,
        hands: &mut Hands,
        track: Id,
        target: Target,
        frame: &Frame,
        origin: Pos2,
        modifiers: Modifiers,
    ) {
        hands.focus = true;
        let lane = self.lane_points_of(track, target);
        let free = modifiers.alt;
        match hands.tool {
            Tool::Pointer => {
                let picks = hands
                    .picks
                    .as_ref()
                    .filter(|p| p.on(track, target))
                    .cloned();
                if let (Some(lane), Some(one)) = (&lane, picks.as_ref().and_then(Picks::single))
                    && let Some(dot) = frame.bend_dot(target, lane, one.index)
                    && dot.distance(origin) <= theme::size::AUTO_GRAB
                {
                    let point = lane.points()[one.index];
                    let handle = match point.shape {
                        Shape::Curve(curve) => curve.handle(),
                        _ => Curve::LINEAR.handle(),
                    };
                    Self::begin(
                        hands,
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
                        hands.toggle_pick(here);
                        return;
                    }
                    if !picks.as_ref().is_some_and(|p| p.contains(here)) {
                        hands.picks = Some(Picks::only(here));
                    }
                    self.begin_move(hands, frame, here, origin);
                    return;
                }
                if modifiers.shift {
                    hands.picks = None;
                    Self::begin(
                        hands,
                        track,
                        target,
                        Gesture::Box(Boxing {
                            from: origin,
                            to: origin,
                        }),
                    );
                    return;
                }
                let at = frame.lane(self.snapped_at_zoom(
                    frame.shown_at(origin.x),
                    free,
                    frame.view.zoom,
                ));
                let (height, _) =
                    Self::snap_to_rest(target, frame, frame.height_at(origin.y), free);
                let value = scale::value(target, height);
                if let Some(here) = self.add_lane_point(hands, track, target, at, value, true) {
                    self.begin_move(hands, frame, here, origin);
                }
            }
            Tool::Pencil => {
                let base = lane.unwrap_or_default();
                hands.picks = None;
                Self::begin(
                    hands,
                    track,
                    target,
                    Gesture::Draw(Drawing {
                        base,
                        raw: Vec::new(),
                    }),
                );
                self.lane_update(hands, None, origin, frame, modifiers);
            }
            Tool::Shape(kind) => {
                let base = lane.unwrap_or_default();
                hands.picks = None;
                let from = self
                    .snapped(frame.ticks_at(origin.x), free)
                    .max(Ticks::ZERO);
                let (start, _) = Self::snap_to_rest(target, frame, frame.height_at(origin.y), free);
                let (low, high) = self.grid_cell(from);
                Self::begin(
                    hands,
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

    fn begin(hands: &mut Hands, track: Id, target: Target, how: Gesture) {
        hands.active = Some(Active { track, target, how });
    }

    /// Takes hold of the picked points by `here`.
    fn begin_move(&mut self, hands: &mut Hands, frame: &Frame, here: Picked, origin: Pos2) {
        let Some(base) = self.lane_points_of(here.track, here.target) else {
            return;
        };
        let Some(point) = base.points().get(here.index).copied() else {
            return;
        };
        let picks = hands
            .picks
            .as_ref()
            .filter(|p| p.on(here.track, here.target))
            .map(Picks::list)
            .filter(|l| l.contains(&here.index))
            .unwrap_or_else(|| vec![here.index]);
        Self::begin(
            hands,
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

    /// The arrangement's gesture under way follows the pointer at `p`.
    /// `painter` draws the guides when there is one.
    pub(super) fn auto_update(
        &mut self,
        painter: Option<(&egui::Painter, Rect)>,
        p: Pos2,
        frame: &Frame,
        modifiers: Modifiers,
    ) {
        let mut hands = self.arrangement.auto.lend();
        self.lane_update(&mut hands, painter, p, frame, modifiers);
        self.arrangement.auto.restore(hands);
    }

    /// The gesture in `hands` follows the pointer at `p`.
    pub(crate) fn lane_update(
        &mut self,
        hands: &mut Hands,
        painter: Option<(&egui::Painter, Rect)>,
        p: Pos2,
        frame: &Frame,
        modifiers: Modifiers,
    ) {
        let Some(mut active) = hands.active.take() else {
            return;
        };
        let (track, target) = (active.track, active.target);
        let free = modifiers.alt;
        match &mut active.how {
            Gesture::Move(moving) => {
                // Snapped as it is drawn: the editor draws a clip's envelope where it is
                // heard (M9.5 D20).
                let want = frame.shown(moving.anchor_at)
                    + (frame.shown_at(p.x) - frame.shown(moving.press_at));
                let at = frame
                    .lane(self.snapped_at_zoom(want, free, frame.view.zoom))
                    .max(Ticks::ZERO);
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
                hands.picks = (!inside.is_empty()).then_some(Picks {
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
                let mut points = pure::wave(
                    waving.kind,
                    waving.from,
                    waving.to,
                    waving.period,
                    waving.start,
                    waving.end,
                    |h| scale::value(target, h),
                );
                // A mute lane holds each value until the next point.
                if target == Target::Mute {
                    for point in &mut points {
                        point.shape = Shape::Step;
                    }
                }
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
        hands.active = Some(active);
    }

    /// The release on the arrangement.
    pub(super) fn auto_finish(&mut self) {
        let mut hands = self.arrangement.auto.lend();
        Self::lane_finish(&mut hands);
        self.arrangement.auto.restore(hands);
    }

    /// The release: the gesture is done, and the document already holds it.
    pub(crate) fn lane_finish(hands: &mut Hands) {
        let Some(active) = hands.active.take() else {
            return;
        };
        // A stroke and a shape make new points: the old indices mean nothing.
        if matches!(active.how, Gesture::Draw(_) | Gesture::Wave(_)) {
            hands.picks = None;
        }
    }
}

/// How far a stroke may be thinned from what was drawn: a pixel and a half
/// of the lane's height, in the target's units.
fn stroke_tolerance(target: Target, frame: &Frame) -> f32 {
    let reach = 1.5 / frame.area().height().max(1.0);
    (scale::value(target, 0.5 + reach) - scale::value(target, 0.5)).abs()
}
