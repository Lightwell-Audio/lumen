//! Automation lanes on the arrangement (M7 slice A1, the owner's L1 B): a
//! lane is drawn over its track's own clip lane, the clips step back while
//! it is shown, and a chip at the lane's top-left names the parameter and
//! opens the chooser. Each track shows one lane at a time; which one is
//! session state (it is not saved with the project).
//!
//! The pieces that decide things are pure and tested here: where a point
//! sits ([`Frame`]), what an edit makes of a lane's points ([`inserted`],
//! [`moved`], [`removed`]) and which point a pointer is over
//! ([`Frame::nearest`]). The pointer's gestures are `app/auto_editing.rs`.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, Ui, pos2};
use eframe::emath::GuiRounding as _;
use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_project::automation::{AutomationLane, Target};
use lumen_project::model::{Id, Project, Track};

use super::auto_scale as scale;
use super::clips::INSET;
use super::strip_output::row_button;
use super::{Arrangement, Row, RowKind, STEP, UNFOLDED};
use crate::document::Document;
use crate::timeview::TimeView;
use crate::{ruler, theme};

/// How far under the lane's top the chip sits.
const CHIP_TOP: f32 = INSET + 2.0;

/// A point of a lane the user has picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picked {
    pub track: Id,
    pub target: Target,
    /// Its place in the lane's points.
    pub index: usize,
}

/// What the arrangement knows of the lanes it shows.
#[derive(Debug, Default)]
pub(crate) struct AutoView {
    /// The lane each track shows, by track.
    pub shown: BTreeMap<Id, Target>,
    /// The point picked, for Delete and the arrows.
    pub picked: Option<Picked>,
    /// The point being dragged.
    pub dragging: Option<Picked>,
    /// The point under the pointer.
    pub hover: Option<Picked>,
}

impl AutoView {
    /// Shows the lanes of `tracks`, or hides them when every one already
    /// shows one. A track opens on the first lane it has, else volume.
    pub fn toggle(&mut self, project: &Project, tracks: &[Id]) {
        let all = !tracks.is_empty() && tracks.iter().all(|t| self.shown.contains_key(t));
        for &id in tracks {
            if all {
                self.shown.remove(&id);
            } else if !self.shown.contains_key(&id)
                && let Some(track) = project.tracks.iter().find(|t| t.id == id)
            {
                let target = track
                    .automation
                    .first()
                    .map_or(Target::Volume, |l| l.target);
                self.shown.insert(id, target);
            }
        }
        if all {
            self.picked = None;
        }
    }

    /// Forgets lanes of tracks that are gone, targets a track no longer
    /// has, and a point that is not there any more.
    pub fn prune(&mut self, project: &Project) {
        self.shown.retain(|id, target| {
            project
                .tracks
                .iter()
                .find(|t| t.id == *id)
                .is_some_and(|t| t.has_target(*target))
        });
        let exists = |p: &Picked| {
            project
                .tracks
                .iter()
                .find(|t| t.id == p.track)
                .and_then(|t| t.lane(p.target))
                .is_some_and(|l| p.index < l.lane.len())
                && self.shown.get(&p.track) == Some(&p.target)
        };
        if self.picked.as_ref().is_some_and(|p| !exists(p)) {
            self.picked = None;
        }
        if self.hover.as_ref().is_some_and(|p| !exists(p)) {
            self.hover = None;
        }
        if self.dragging.as_ref().is_some_and(|p| !exists(p)) {
            self.dragging = None;
        }
    }
}

/// `lane` with a point of `value` at `at` (replacing the value of one
/// already there), and where it is. A mute lane's points hold their value
/// until the next. `None` when the lane would hold too many points.
pub(crate) fn inserted(
    target: Target,
    lane: Option<&Lane>,
    at: Ticks,
    value: f32,
) -> Option<(Lane, usize)> {
    let mut points: Vec<Point> = lane.map_or_else(Vec::new, |l| l.points().to_vec());
    let at = at.max(Ticks::ZERO);
    let index = match points.binary_search_by_key(&at, |p| p.at) {
        Ok(index) => {
            points[index].value = value;
            index
        }
        Err(index) => {
            let mut point = Point::new(at, value);
            if target == Target::Mute {
                point = point.with_shape(Shape::Step);
            }
            points.insert(index, point);
            index
        }
    };
    Lane::new(points).ok().map(|lane| (lane, index))
}

/// `lane` with point `index` taken to `at` and `value`. It stays between
/// its neighbours, a tick clear of each; points do not pass one another.
pub(crate) fn moved(lane: &Lane, index: usize, at: Ticks, value: f32) -> Option<Lane> {
    let mut points = lane.points().to_vec();
    let point = points.get_mut(index)?;
    let low = index
        .checked_sub(1)
        .map_or(Ticks::ZERO, |i| lane.points()[i].at + Ticks(1));
    let at = at.max(low);
    let at = lane
        .points()
        .get(index + 1)
        .map_or(at, |next| at.min(next.at - Ticks(1)));
    point.at = at;
    point.value = value;
    Lane::new(points).ok()
}

/// `lane` without point `index`.
pub(crate) fn removed(lane: &Lane, index: usize) -> Lane {
    let mut points = lane.points().to_vec();
    if index < points.len() {
        points.remove(index);
    }
    Lane::new(points).unwrap_or_else(|_| lane.clone())
}

/// Where a track's lane is on screen: its row, and the view of time across
/// it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    /// The track's whole row.
    pub row: Rect,
    pub canvas_left: f32,
    pub view: TimeView,
}

impl Frame {
    /// Where values are drawn: the row under the chip that names the lane
    /// and inside the clips' inset, with room at the bottom so the lowest
    /// point is whole. The chip never covers a point.
    pub fn area(&self) -> Rect {
        Rect::from_x_y_ranges(
            self.row.x_range(),
            self.row.top() + CHIP_TOP + theme::size::CONTROL_SMALL + theme::space::XS
                ..=self.row.bottom() - INSET - theme::size::AUTO_PAD,
        )
    }

    pub fn x(&self, at: Ticks) -> f32 {
        self.view.x(self.canvas_left, at.as_quarters())
    }

    pub fn ticks_at(&self, x: f32) -> Ticks {
        ruler::ticks_at(self.view.beat_at(self.canvas_left, x))
    }

    pub fn y(&self, target: Target, value: f32) -> f32 {
        let area = self.area();
        area.bottom() - scale::fraction(target, value) * area.height()
    }

    /// The value at height `y`.
    pub fn value_at(&self, target: Target, y: f32) -> f32 {
        let area = self.area();
        scale::value(target, (area.bottom() - y) / area.height().max(1.0))
    }

    pub fn pos(&self, target: Target, point: &Point) -> Pos2 {
        pos2(self.x(point.at), self.y(target, point.value))
    }

    /// The point of `lane` nearest `pointer`, if one is within reach.
    pub fn nearest(&self, target: Target, lane: &Lane, pointer: Pos2) -> Option<usize> {
        let reach = theme::size::AUTO_GRAB;
        let points = lane.points();
        // Only points whose x is within reach can be it.
        let first = points.partition_point(|p| self.x(p.at) < pointer.x - reach);
        points[first..]
            .iter()
            .enumerate()
            .take_while(|(_, p)| self.x(p.at) <= pointer.x + reach)
            .map(|(i, p)| (first + i, self.pos(target, p).distance(pointer)))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

/// The line a lane draws across `[left, right]`, as screen points: flat
/// before the first point and after the last, a straight run, a step or
/// samples along a curve between. Only the part in view is made.
fn polyline(frame: &Frame, target: Target, lane: &Lane, (left, right): (f32, f32)) -> Vec<Pos2> {
    let points = lane.points();
    let mut line = Vec::new();
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return line;
    };
    let start = frame.pos(target, first);
    if start.x > left {
        line.push(pos2(left, start.y));
    }
    // The run of points that can touch the view: one before the left edge
    // to one after the right.
    let a = points
        .partition_point(|p| frame.x(p.at) < left)
        .saturating_sub(1);
    let b = points
        .partition_point(|p| frame.x(p.at) <= right)
        .min(points.len() - 1);
    for i in a..=b {
        let from = frame.pos(target, &points[i]);
        let Some(next) = points.get(i + 1).filter(|_| i < b) else {
            // The run's last point: the segment before it ended at the view's
            // edge if this point lies past it, so only a point in view (or
            // behind it, held flat) is added.
            if from.x <= right + 1.0 {
                line.push(pos2(from.x.max(left - 1.0), from.y));
            }
            continue;
        };
        let to = frame.pos(target, next);
        let (xa, xb) = (from.x.max(left - 1.0), to.x.min(right + 1.0));
        let at = |x: f32| ((x - from.x) / (to.x - from.x).max(f32::EPSILON)).clamp(0.0, 1.0);
        match points[i].shape {
            Shape::Step => {
                line.push(pos2(xa.min(to.x), from.y));
                if to.x <= right + 1.0 {
                    line.push(pos2(to.x, from.y));
                }
            }
            Shape::Linear => {
                line.push(pos2(xa, from.y + (to.y - from.y) * at(xa)));
                if xb > xa {
                    line.push(pos2(xb, from.y + (to.y - from.y) * at(xb)));
                }
            }
            Shape::Curve(_) => {
                let width = (xb - xa).max(0.0);
                let steps = ((width / theme::size::AUTO_STEP) as usize).clamp(1, 256);
                for s in 0..=steps {
                    let x = xa + width * s as f32 / steps as f32;
                    let y = frame.y(
                        target,
                        points[i]
                            .shape
                            .between(points[i].value, points[i + 1].value, at(x)),
                    );
                    line.push(pos2(x, y));
                }
            }
        }
    }
    if frame.x(last.at) < right {
        line.push(pos2(right, frame.pos(target, last).y));
    }
    line
}

impl Arrangement {
    /// The row that shows `track`'s lane, if the track is in view.
    fn lane_row(&self, rows: &[Row], project: &Project, track: Id) -> Option<Row> {
        let index = project.tracks.iter().position(|t| t.id == track)?;
        rows.iter()
            .find(|r| r.kind == RowKind::Track(index) && !r.clipped)
            .copied()
    }

    /// Where `track`'s lane is on screen, if it shows one and is in view.
    pub(crate) fn lane_frame(
        &self,
        ctx: &egui::Context,
        canvas: Rect,
        project: &Project,
        pending: usize,
        view: TimeView,
        track: Id,
    ) -> Option<(Target, Frame)> {
        let target = *self.auto.shown.get(&track)?;
        let rows = self.rows(ctx, project, pending);
        let row = self.lane_row(&rows, project, track)?;
        let top = canvas.top() + row.y - self.top;
        let rect = Rect::from_min_size(
            pos2(canvas.left(), top),
            egui::vec2(canvas.width(), row.height),
        );
        Some((
            target,
            Frame {
                row: rect,
                canvas_left: canvas.left(),
                view,
            },
        ))
    }

    /// The lane under `pointer`, if the track there shows one.
    pub(crate) fn lane_at(
        &self,
        ctx: &egui::Context,
        canvas: Rect,
        project: &Project,
        pending: usize,
        view: TimeView,
        pointer: Pos2,
    ) -> Option<(Id, Target, Frame)> {
        if self.auto.shown.is_empty() {
            return None;
        }
        let track = self.track_at(ctx, project, pending, pointer.y - canvas.top())?;
        let (target, frame) = self.lane_frame(ctx, canvas, project, pending, view, track)?;
        Some((track, target, frame))
    }

    /// Draws every lane being shown, over its track's clips, and the chips
    /// that name them. Called after the lanes' pointer handling so the chips
    /// are above it.
    pub(crate) fn lane_overlay(
        &mut self,
        ui: &mut Ui,
        painter: &Painter,
        canvas: Rect,
        doc: &mut Document,
        view: TimeView,
        pending: usize,
    ) {
        self.auto.prune(doc.project());
        if self.auto.shown.is_empty() {
            return;
        }
        let ctx = ui.ctx().clone();
        let project = doc.project().clone();
        let rows = self.rows(&ctx, &project, pending);
        let shown: Vec<(Id, Target)> = self.auto.shown.iter().map(|(i, t)| (*i, *t)).collect();
        for (id, target) in shown {
            let Some(row) = self.lane_row(&rows, &project, id) else {
                continue;
            };
            let Some(track) = project.tracks.iter().find(|t| t.id == id) else {
                continue;
            };
            let zone = self.zone_span(&ctx, &project, row.zone, canvas.y_range());
            let visible = Rect::from_x_y_ranges(canvas.x_range(), zone);
            let top = canvas.top() + row.y - self.top;
            let rect = Rect::from_min_size(
                pos2(canvas.left(), top),
                egui::vec2(canvas.width(), row.height),
            );
            if !visible.intersects(rect) {
                continue;
            }
            let painter = painter.with_clip_rect(visible.intersect(painter.clip_rect()));
            let frame = Frame {
                row: rect,
                canvas_left: canvas.left(),
                view,
            };
            let entry = track.lane(target);
            self.draw_lane(&painter, &frame, track, target, entry);
            self.lane_chip(ui, doc, &visible, &frame, track, target);
        }
    }

    /// One lane: its wash and edges, the rest line, the curve and its
    /// points.
    fn draw_lane(
        &self,
        painter: &Painter,
        frame: &Frame,
        track: &Track,
        target: Target,
        entry: Option<&AutomationLane>,
    ) {
        let palette = *theme::p();
        let rect = frame.row;
        let on = entry.is_none_or(|e| e.enabled);
        let ink = if on {
            palette.accent
        } else {
            palette.text_hint
        };
        let canvas_x = painter.clip_rect().x_range();
        painter.rect_filled(rect, 0.0, palette.accent.gamma_multiply(0.05));
        let edge = Stroke::new(theme::stroke::HAIRLINE, palette.accent.gamma_multiply(0.45));
        painter.hline(canvas_x, rect.top() + 0.5, edge);
        painter.hline(canvas_x, rect.bottom() - 1.5, edge);
        // Where the control rests, to read the curve against.
        painter.hline(
            canvas_x,
            frame.y(target, scale::default_value(target)),
            Stroke::new(
                theme::stroke::HAIRLINE,
                palette.border_strong.gamma_multiply(0.6),
            ),
        );
        let Some(entry) = entry else {
            return;
        };
        let line = polyline(frame, target, &entry.lane, (canvas_x.min, canvas_x.max));
        if line.len() >= 2 {
            painter.add(egui::Shape::line(
                line,
                Stroke::new(theme::stroke::LIT, ink),
            ));
        }
        let points = entry.lane.points();
        let first = points.partition_point(|p| frame.x(p.at) < canvas_x.min - 8.0);
        for (i, point) in points.iter().enumerate().skip(first) {
            let at = frame.pos(target, point);
            if at.x > canvas_x.max + 8.0 {
                break;
            }
            let here = Picked {
                track: track.id,
                target,
                index: i,
            };
            let picked = self.auto.picked == Some(here);
            let hot = self.auto.hover == Some(here) || self.auto.dragging == Some(here);
            draw_point(painter, at, ink, picked, hot);
        }
    }

    /// The chip at the lane's top-left: the parameter's name, opening the
    /// chooser; the lane's state when it is off.
    fn lane_chip(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        visible: &Rect,
        frame: &Frame,
        track: &Track,
        target: Target,
    ) {
        let project = doc.project();
        let entry = track.lane(target);
        let mut text = scale::name(project, target);
        if entry.is_some_and(|e| !e.enabled) {
            text.push_str(" \u{00B7} off");
        }
        let width = (ui
            .painter()
            .layout_no_wrap(
                text.clone(),
                egui::FontId::proportional(theme::size::CAPTION),
                theme::p().text,
            )
            .size()
            .x
            + theme::size::AUTO_CHIP_PAD)
            .clamp(theme::size::AUTO_CHIP_MIN, theme::size::AUTO_CHIP_MAX);
        let chip = Rect::from_min_size(
            pos2(
                frame.row.left() + theme::space::S,
                frame.row.top() + CHIP_TOP,
            ),
            egui::vec2(width, theme::size::CONTROL_SMALL),
        )
        .round_to_pixels(ui.pixels_per_point());
        if !visible.contains_rect(chip) {
            return;
        }
        let id = egui::Id::new(("automation chip", track.id));
        let name = format!("Automation lane of {}: {text}", track.name);
        let response = row_button(
            ui,
            chip,
            id,
            "lane",
            &text,
            &name,
            "Choose the parameter this lane draws (A shows or hides lanes)",
        );
        crate::widgets::Menu::below(&response).show(|ui| {
            ui.set_min_width(theme::size::AUTO_MENU);
            self.lane_menu(ui, doc, track.id, target);
        });
    }

    /// The chooser: the track's parameters (one per line, with how many
    /// points each lane holds), then what can be done to the shown lane.
    fn lane_menu(&mut self, ui: &mut Ui, doc: &mut Document, track: Id, target: Target) {
        let project = doc.project().clone();
        let Some(held) = project.tracks.iter().find(|t| t.id == track) else {
            return;
        };
        for option in scale::offered(held) {
            let count = held.lane(option).map_or(0, |l| l.lane.len());
            let label = match count {
                0 => scale::name(&project, option),
                1 => format!("{}  \u{00B7}  1 point", scale::name(&project, option)),
                n => format!("{}  \u{00B7}  {n} points", scale::name(&project, option)),
            };
            if ui.selectable_label(option == target, label).clicked() {
                self.auto.shown.insert(track, option);
                self.auto.picked = None;
                ui.close();
            }
        }
        ui.separator();
        if let Some(entry) = held.lane(target) {
            let label = if entry.enabled {
                "Turn the lane off"
            } else {
                "Turn the lane on"
            };
            if ui.button(label).clicked() {
                self.note_lane(doc.set_lane_enabled(track, target, !entry.enabled));
                ui.close();
            }
            if ui.button("Remove the lane").clicked() {
                self.note_lane(doc.remove_lane(track, target));
                self.auto.picked = None;
                ui.close();
            }
        }
        if ui.button("Hide automation").clicked() {
            self.auto.shown.remove(&track);
            self.auto.picked = None;
            ui.close();
        }
    }

    /// Keeps why a lane edit was refused, for a note on screen.
    pub(crate) fn note_lane(&mut self, result: Result<(), crate::document::LaneRefusal>) {
        if let Err(refused) = result {
            self.note = Some(refused.reason());
        }
    }

    /// The height a track shown with a lane needs: room for the chip and a
    /// readable curve under it.
    pub(super) fn lane_floor(&self, id: Id, height: f32) -> f32 {
        if self.auto.shown.contains_key(&id) {
            height.max(UNFOLDED + STEP)
        } else {
            height
        }
    }
}

/// A point: hollow at rest, solid when picked, and a little larger under
/// the pointer. Picked is also a ring, so it never rests on colour alone.
fn draw_point(painter: &Painter, at: Pos2, ink: Color32, picked: bool, hot: bool) {
    let palette = theme::p();
    let radius = theme::size::AUTO_POINT + if hot || picked { 1.0 } else { 0.0 };
    let fill = if picked { ink } else { palette.canvas };
    painter.circle(at, radius, fill, Stroke::new(theme::stroke::LIT, ink));
    if picked {
        painter.circle_stroke(
            at,
            radius + 3.0,
            Stroke::new(theme::stroke::HAIRLINE, ink.gamma_multiply(0.6)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(points: &[(i64, f32)]) -> Lane {
        Lane::new(
            points
                .iter()
                .map(|&(q, v)| Point::new(Ticks::quarters(q), v))
                .collect(),
        )
        .unwrap()
    }

    fn frame() -> Frame {
        // 10 points a beat, the lane 100 points tall and 600 wide, from x 0.
        Frame {
            row: Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(600.0, 100.0)),
            canvas_left: 0.0,
            view: TimeView {
                left: 0.0,
                zoom: 10.0,
            },
        }
    }

    #[test]
    fn a_new_point_lands_in_order_and_one_at_the_same_time_is_replaced() {
        let base = lane(&[(0, -6.0), (8, 0.0)]);
        let (made, at) = inserted(Target::Volume, Some(&base), Ticks::quarters(4), -3.0).unwrap();
        assert_eq!(at, 1);
        assert_eq!(made.len(), 3);
        let (again, same) = inserted(Target::Volume, Some(&made), Ticks::quarters(4), 2.0).unwrap();
        assert_eq!((again.len(), same), (3, 1));
        assert_eq!(again.points()[1].value, 2.0);
        // A first point makes a lane.
        let (first, i) = inserted(Target::Pan, None, Ticks::quarters(2), 0.5).unwrap();
        assert_eq!((first.len(), i), (1, 0));
    }

    #[test]
    fn a_mute_lane_holds_each_value_until_the_next_point() {
        let (made, _) = inserted(Target::Mute, None, Ticks::quarters(1), 1.0).unwrap();
        assert_eq!(made.points()[0].shape, Shape::Step);
        let (made, _) = inserted(Target::Volume, None, Ticks::quarters(1), 0.0).unwrap();
        assert_eq!(made.points()[0].shape, Shape::Linear);
    }

    #[test]
    fn a_point_stays_between_its_neighbours() {
        let base = lane(&[(0, 0.0), (4, 0.0), (8, 0.0)]);
        let far_right = moved(&base, 1, Ticks::quarters(20), 3.0).unwrap();
        assert_eq!(far_right.points()[1].at, Ticks::quarters(8) - Ticks(1));
        assert_eq!(far_right.points()[1].value, 3.0);
        let far_left = moved(&base, 1, Ticks::quarters(-5), 0.0).unwrap();
        assert_eq!(far_left.points()[1].at, Ticks(1));
        // The first point cannot go before the song starts.
        let first = moved(&base, 0, Ticks::quarters(-5), 0.0).unwrap();
        assert_eq!(first.points()[0].at, Ticks::ZERO);
        assert!(moved(&base, 9, Ticks::ZERO, 0.0).is_none());
    }

    #[test]
    fn a_point_can_be_taken_out() {
        let base = lane(&[(0, 0.0), (4, 1.0), (8, 2.0)]);
        let cut = removed(&base, 1);
        assert_eq!(cut.len(), 2);
        assert_eq!(cut.points()[1].value, 2.0);
        assert_eq!(removed(&base, 7).len(), 3, "nothing there to take");
        assert!(removed(&lane(&[(0, 0.0)]), 0).is_empty());
    }

    #[test]
    fn values_and_heights_agree_through_the_frame() {
        let f = frame();
        let y = f.y(Target::Pan, 0.0);
        assert!((f.value_at(Target::Pan, y)).abs() < 1e-4);
        // Louder is higher on the screen.
        assert!(f.y(Target::Volume, 0.0) < f.y(Target::Volume, -20.0));
        // The extremes stay inside the lane, under the chip.
        let area = f.area();
        assert!(f.y(Target::Volume, 6.0) >= area.top() - 0.01);
        assert!(f.y(Target::Volume, -100.0) <= area.bottom() + 0.01);
        assert!(area.top() > f.row.top() + CHIP_TOP + theme::size::CONTROL_SMALL);
        assert!(area.bottom() < f.row.bottom());
    }

    #[test]
    fn the_nearest_point_is_found_only_within_reach() {
        let f = frame();
        let l = lane(&[(0, 0.0), (10, 0.0), (20, 0.0)]);
        let target = Target::Pan;
        let at = f.pos(target, &l.points()[1]);
        assert_eq!(f.nearest(target, &l, at), Some(1));
        assert_eq!(f.nearest(target, &l, pos2(at.x + 3.0, at.y - 3.0)), Some(1));
        assert_eq!(
            f.nearest(target, &l, pos2(at.x + 40.0, at.y)),
            None,
            "too far to grab"
        );
        assert_eq!(f.nearest(target, &l, pos2(at.x, at.y + 40.0)), None);
        // Two near points: the closer wins.
        let close = lane(&[(0, 0.0), (1, 0.0)]);
        let p1 = f.pos(target, &close.points()[1]);
        assert_eq!(f.nearest(target, &close, pos2(p1.x - 2.0, p1.y)), Some(1));
    }

    #[test]
    fn the_line_runs_flat_before_the_first_point_and_after_the_last() {
        let f = frame();
        let l = lane(&[(10, -6.0), (20, 0.0)]);
        let line = polyline(&f, Target::Volume, &l, (0.0, 600.0));
        assert_eq!(line.first().map(|p| p.x), Some(0.0));
        assert_eq!(line.last().map(|p| p.x), Some(600.0));
        assert_eq!(line.first().map(|p| p.y), Some(f.y(Target::Volume, -6.0)));
        assert_eq!(line.last().map(|p| p.y), Some(f.y(Target::Volume, 0.0)));
        // An empty lane draws nothing.
        let none = Lane::new(Vec::new()).unwrap();
        assert!(polyline(&f, Target::Volume, &none, (0.0, 600.0)).is_empty());
    }

    #[test]
    fn a_lane_far_wider_than_the_view_draws_only_what_is_in_it() {
        // Zoomed in to the sample: the segment is millions of points wide.
        let f = Frame {
            view: TimeView {
                left: 1_000.0,
                zoom: 1.0e6,
            },
            ..frame()
        };
        let l = lane(&[(0, -6.0), (4_000, 0.0)]);
        let line = polyline(&f, Target::Volume, &l, (0.0, 600.0));
        assert!(
            line.len() <= 4,
            "a straight run is two points, not millions"
        );
        assert!(line.iter().all(|p| p.x >= -2.0 && p.x <= 602.0));
        // A curve is sampled across the view only.
        let curved = Lane::new(vec![
            Point::new(Ticks::quarters(0), -6.0)
                .with_shape(Shape::Curve(lumen_core::curve::Curve::default())),
            Point::new(Ticks::quarters(4_000), 0.0),
        ])
        .unwrap();
        let line = polyline(&f, Target::Volume, &curved, (0.0, 600.0));
        assert!(line.len() < 600, "{} samples", line.len());
    }

    #[test]
    fn toggling_shows_and_hides_lanes_and_opens_on_the_first_lane() {
        let mut project = crate::empty_project();
        let mut track = Track::new(
            Id(7),
            "T".to_owned(),
            lumen_project::model::TrackKind::Audio,
            lumen_project::model::Color(0),
        );
        track
            .automation
            .push(AutomationLane::new(Target::Pan, lane(&[(0, 0.0)])));
        project.tracks.push(std::sync::Arc::new(track));
        let mut view = AutoView::default();
        view.toggle(&project, &[Id(7)]);
        assert_eq!(view.shown.get(&Id(7)), Some(&Target::Pan));
        view.toggle(&project, &[Id(7)]);
        assert!(view.shown.is_empty());
        // A track that is gone is not shown.
        view.toggle(&project, &[Id(99)]);
        assert!(view.shown.is_empty());
    }

    #[test]
    fn lanes_and_picks_are_forgotten_with_what_they_point_at() {
        let mut project = crate::empty_project();
        let mut track = Track::new(
            Id(7),
            "T".to_owned(),
            lumen_project::model::TrackKind::Audio,
            lumen_project::model::Color(0),
        );
        track.automation.push(AutomationLane::new(
            Target::Volume,
            lane(&[(0, 0.0), (4, 0.0)]),
        ));
        project.tracks.push(std::sync::Arc::new(track));
        let mut view = AutoView::default();
        view.shown.insert(Id(7), Target::Volume);
        view.picked = Some(Picked {
            track: Id(7),
            target: Target::Volume,
            index: 1,
        });
        view.prune(&project);
        assert!(view.picked.is_some());
        view.picked = Some(Picked {
            index: 2,
            ..view.picked.unwrap()
        });
        view.prune(&project);
        assert!(view.picked.is_none(), "no third point");
        view.shown.insert(Id(7), Target::Send { to: Id(3) });
        view.prune(&project);
        assert!(view.shown.is_empty(), "the track makes no such send");
    }
}
