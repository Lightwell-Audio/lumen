//! Drawing the lane (M5 slice 4b): the plotting area under the piano roll,
//! with stems (velocity) or a step trace (controllers), the picks, the hover
//! and the gesture's preview. Like `note_editor::paint` it draws from data
//! only; the document is never changed to show a preview.

use eframe::egui::{
    Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2,
};
use lumen_core::curve::Curve;
use lumen_core::grid::{Level, Line};
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use super::geometry::{HEAD_RADIUS, LaneKind, LaneView};
use super::gesture::Resolved;
use crate::theme;

/// Stems: the margin they may sit beyond the body, width, head radii, and the
/// spacing under which only the louder of two is drawn.
const STEM_MARGIN: f32 = 8.0;
const STEM_WIDTH: f32 = 2.0;
const STEM_RADIUS: f32 = HEAD_RADIUS;
const STEM_RADIUS_HOVER: f32 = 5.0;
const STEM_SPACING: f32 = 3.0;
/// The controller trace: its stroke, and its fill's alpha over the accent.
const TRACE_WIDTH: f32 = 1.6;
const TRACE_FILL_ALPHA: f32 = 0.16;
/// Event dots: radii; drawn only below `DOT_LIMIT` visible points, on average
/// `DOT_GAP` points apart.
const POINT_R: f32 = 2.5;
const POINT_R_HOVER: f32 = 4.0;
const DOT_LIMIT: usize = 250;
const DOT_GAP: f32 = 5.0;
/// The preview's stroke, and the bend handle's stroke and dot radius.
const PREVIEW_WIDTH: f32 = 1.4;
const BEND_WIDTH: f32 = 1.6;
const BEND_RADIUS: f32 = 5.0;
/// Samples along a span or ramp curve; the share of the app colour that
/// dims the time outside the clip.
const CURVE_STEPS: usize = 64;
const DIM: f32 = 0.55;
/// Guides sit at these fractions of the range.
const GUIDES: [f64; 3] = [0.25, 0.5, 0.75];
/// Scale labels: inset from the column's right edge, and half a label's
/// height (their centres stay that far from the column's ends).
const SCALE_INSET: f32 = 6.0;
const LABEL_HALF: f32 = theme::size::SCALE * 0.6;

/// One event of the open lane: clip time, value in the lane's units and its
/// index in the clip's `events`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LanePoint {
    pub at: Ticks,
    pub value: i32,
    pub index: usize,
}

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LaneHover {
    #[default]
    None,
    /// The velocity head of `note`, on the stem `LaneScene::stems[stem]`.
    Head { stem: usize, note: usize },
    /// An index into `LaneScene::points`.
    Point(usize),
}

/// A line or curve just drawn, whose shape can still be bent by dragging its
/// dot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BendHandle {
    pub from: (Ticks, i32),
    pub to: (Ticks, i32),
    pub curve: Curve,
}

/// Where the handle's dot is: the curve's middle point mapped onto the
/// line's ends.
pub(crate) fn bend_dot(view: &LaneView, bend: &BendHandle) -> Pos2 {
    let [fx, fy] = bend.curve.point(0.5);
    let span = (bend.to.0 - bend.from.0).0 as f64;
    let at = Ticks(bend.from.0.0 + (span * f64::from(fx)).round() as i64);
    let value = f64::from(bend.from.1) + f64::from(bend.to.1 - bend.from.1) * f64::from(fy);
    pos2(view.x_of(at), view.y_of(value.round() as i32))
}

/// Everything [`paint_lane`] needs, as data.
pub(crate) struct LaneScene<'a> {
    pub notes: &'a [Note],
    /// The notes grouped by start (`velocity::stems`), in time order.
    pub stems: &'a [(Ticks, Vec<usize>)],
    /// The picked notes (the velocity lane shows them lit).
    pub picked_notes: &'a [usize],
    /// The open controller lane's events in time order (empty for velocity).
    pub points: &'a [LanePoint],
    /// The picked events (`LanePoint::index`).
    pub picked_points: &'a [usize],
    /// The note colour: the clip's.
    pub colour: Color32,
    pub clip_length: Ticks,
    /// The grid lines of the visible time, as the ruler draws them.
    pub lines: &'a [Line],
    pub hover: LaneHover,
    /// The gesture's result so far (`Resolved::Nothing` when none).
    pub preview: &'a Resolved,
    pub bend: Option<&'a BendHandle>,
}

/// Draws the lane inside `view.body`.
pub(crate) fn paint_lane(painter: &Painter, view: &LaneView, scene: &LaneScene<'_>) {
    let palette = theme::p();
    let body = view.body;
    let painter = painter.with_clip_rect(body);
    painter.rect_filled(body, 0.0, palette.canvas);
    draw_grid(&painter, view, scene.lines);
    draw_guides(&painter, view);
    draw_outside_clip(&painter, view, scene.clip_length);
    match view.kind {
        LaneKind::Velocity => draw_stems(&painter, view, scene),
        _ => draw_trace(&painter, view, scene),
    }
    draw_preview(&painter, view, scene);
    if let Some(bend) = scene.bend {
        painter.line(
            sample_span(view, bend.from, bend.to, bend.curve),
            Stroke::new(BEND_WIDTH, palette.accent),
        );
        painter.circle(
            bend_dot(view, bend),
            BEND_RADIUS,
            palette.text,
            Stroke::new(2.0, palette.accent),
        );
    }
}

/// Draws the value scale in `column` (the strip left of the plot): the
/// range's top and bottom, and the centre line's 0 for pitch bend.
pub(crate) fn paint_scale(painter: &Painter, view: &LaneView, column: Rect) {
    let palette = theme::p();
    painter.rect_filled(column, 0.0, palette.raised);
    let painter = painter.with_clip_rect(column);
    for (y, text) in scale_labels(view) {
        painter.text(
            pos2(column.right() - SCALE_INSET, label_y(y, column)),
            Align2::RIGHT_CENTER,
            text,
            FontId::monospace(theme::size::SCALE),
            palette.text_secondary,
        );
    }
    painter.vline(
        column.right() - 0.5,
        column.top()..=column.bottom(),
        Stroke::new(1.0, palette.border),
    );
}

/// The grid lines the ruler draws, at the same x (the lines are in
/// arrangement time, as the ruler reads them).
fn draw_grid(painter: &Painter, view: &LaneView, lines: &[Line]) {
    let palette = theme::p();
    let body = view.body;
    for line in lines {
        let x = view.time.x(body.left(), line.at.as_quarters());
        if !(body.left()..=body.right()).contains(&x) {
            continue;
        }
        let colour = match line.level {
            Level::Bar => palette.grid_bar,
            Level::Beat => palette.grid_beat,
            Level::Sub => palette.grid_sub,
        };
        painter.vline(x, body.top()..=body.bottom(), Stroke::new(1.0, colour));
    }
}

/// Guides at the range's quarters; pitch bend's centre line replaces its half.
fn draw_guides(painter: &Painter, view: &LaneView) {
    let palette = theme::p();
    let (low, high) = view.kind.range();
    let x_range = view.body.left()..=view.body.right();
    for frac in GUIDES {
        if view.kind == LaneKind::PitchBend && frac == 0.5 {
            continue;
        }
        let value = (f64::from(low) + f64::from(high - low) * frac).round() as i32;
        painter.hline(
            x_range.clone(),
            view.y_of(value),
            Stroke::new(1.0, palette.grid_beat),
        );
    }
    if view.kind == LaneKind::PitchBend {
        painter.hline(
            x_range,
            view.y_of(0),
            Stroke::new(1.0, palette.border_strong),
        );
    }
}

/// Dims the time before the clip and after it, as the piano roll does.
fn draw_outside_clip(painter: &Painter, view: &LaneView, clip_length: Ticks) {
    let palette = theme::p();
    let body = view.body;
    let dim = palette.app.gamma_multiply(DIM);
    let clip_left = view.x_of(Ticks::ZERO);
    let clip_right = view.x_of(clip_length);
    if clip_left > body.left() {
        painter.rect_filled(
            Rect::from_min_max(body.min, pos2(clip_left.min(body.right()), body.bottom())),
            0.0,
            dim,
        );
    }
    if clip_right < body.right() {
        painter.rect_filled(
            Rect::from_min_max(pos2(clip_right.max(body.left()), body.top()), body.max),
            0.0,
            dim,
        );
    }
}

/// The velocity lane: one stem per visible start, thinned so no more than one
/// stem sits in each [`STEM_SPACING`], with a head for every note of the
/// stem at its own velocity (lit when picked, greyed when muted, bigger when
/// hovered). Heads are drawn in note order, so the higher index is on top.
fn draw_stems(painter: &Painter, view: &LaneView, scene: &LaneScene<'_>) {
    let body = view.body;
    let picked = Members::new(scene.picked_notes);
    let range = view.stems_between(
        scene.stems,
        body.left() - STEM_MARGIN,
        body.right() + STEM_MARGIN,
    );
    let marks = range.filter_map(|index| {
        let (at, members) = scene.stems.get(index)?;
        let velocity = members
            .iter()
            .filter_map(|&note| scene.notes.get(note))
            .map(|n| n.velocity())
            .max()?;
        Some(StemMark {
            index,
            x: view.x_of(*at),
            velocity,
        })
    });
    let (hovered_stem, hovered_note) = match scene.hover {
        LaneHover::Head { stem, note } => (Some(stem), Some(note)),
        _ => (None, None),
    };
    for mark in thin_stems(marks, hovered_stem) {
        draw_chord(painter, view, scene, &picked, mark, hovered_note);
    }
}

/// One stem from the baseline up to its loudest head (lit up to the loudest
/// picked one), and the heads of its notes.
fn draw_chord(
    painter: &Painter,
    view: &LaneView,
    scene: &LaneScene<'_>,
    picked: &Members<'_>,
    mark: StemMark,
    hovered: Option<usize>,
) {
    let palette = theme::p();
    let Some((_, members)) = scene.stems.get(mark.index) else {
        return;
    };
    let base = pos2(mark.x, view.y_of(view.kind.rest()));
    let head_of = |velocity: u8| pos2(mark.x, view.y_of(i32::from(velocity)));
    let all_muted = members
        .iter()
        .all(|&n| scene.notes.get(n).is_some_and(|n| n.muted()));
    let stem = if all_muted {
        theme::CLIP_MUTED
    } else {
        scene.colour
    };
    painter.line_segment(
        [base, head_of(mark.velocity)],
        Stroke::new(STEM_WIDTH, stem),
    );
    let loudest_picked = members
        .iter()
        .filter(|&&n| picked.contains(n))
        .filter_map(|&n| scene.notes.get(n))
        .map(|n| n.velocity())
        .max();
    if let Some(velocity) = loudest_picked {
        painter.line_segment(
            [base, head_of(velocity)],
            Stroke::new(STEM_WIDTH, palette.accent),
        );
    }
    for &n in members {
        let Some(note) = scene.notes.get(n) else {
            continue;
        };
        let colour = if note.muted() {
            theme::CLIP_MUTED
        } else {
            scene.colour
        };
        let radius = if hovered == Some(n) {
            STEM_RADIUS_HOVER
        } else {
            STEM_RADIUS
        };
        draw_head(
            painter,
            head_of(note.velocity()),
            colour,
            radius,
            picked.contains(n),
        );
    }
}

/// One head; lit, a light disc with an accent ring.
fn draw_head(painter: &Painter, head: Pos2, colour: Color32, radius: f32, lit: bool) {
    let palette = theme::p();
    if lit {
        painter.circle(
            head,
            radius,
            palette.text,
            Stroke::new(theme::stroke::LIT, palette.accent),
        );
    } else {
        painter.circle_filled(head, radius, colour);
    }
}

/// The controller lanes: the step trace, its fill and the event dots.
fn draw_trace(painter: &Painter, view: &LaneView, scene: &LaneScene<'_>) {
    let palette = theme::p();
    let body = view.body;
    let base = view.y_of(view.kind.rest());
    let end = view.x_of(scene.clip_length);
    let runs = step_runs(
        view,
        scene.points,
        |p| (p.at, p.value),
        end,
        body.left(),
        body.right(),
    );
    let fill = palette.accent.gamma_multiply(TRACE_FILL_ALPHA);
    fill_runs(painter, &runs, base, fill);
    painter.line(
        step_polyline(&runs),
        Stroke::new(TRACE_WIDTH, palette.accent),
    );
    draw_points(painter, view, scene);
}

/// The event dots of the visible points, when [`dots_visible`] allows.
fn draw_points(painter: &Painter, view: &LaneView, scene: &LaneScene<'_>) {
    let palette = theme::p();
    let (first, end) = visible_points(view, scene.points);
    let visible = &scene.points[first..end];
    let span = match (visible.first(), visible.last()) {
        (Some(a), Some(b)) => view.x_of(b.at) - view.x_of(a.at),
        _ => 0.0,
    };
    if !dots_visible(visible.len(), span) {
        return;
    }
    let picked = Members::new(scene.picked_points);
    for (offset, point) in visible.iter().enumerate() {
        let hovered = scene.hover == LaneHover::Point(first + offset);
        let radius = if hovered { POINT_R_HOVER } else { POINT_R };
        let ring = if picked.contains(point.index) {
            Stroke::new(1.5, palette.text)
        } else {
            Stroke::NONE
        };
        painter.circle(
            pos2(view.x_of(point.at), view.y_of(point.value)),
            radius,
            palette.accent,
            ring,
        );
    }
}

/// The gesture's preview, drawn in text colour over the lane. It never
/// touches the document.
fn draw_preview(painter: &Painter, view: &LaneView, scene: &LaneScene<'_>) {
    let palette = theme::p();
    let ink = Stroke::new(PREVIEW_WIDTH, palette.text);
    let body = view.body;
    match scene.preview {
        Resolved::Nothing => {}
        Resolved::Velocities(changes) => {
            if view.kind != LaneKind::Velocity {
                return;
            }
            for &(note, velocity) in changes {
                if let Some(note) = scene.notes.get(note) {
                    preview_head(painter, view, note.start(), i32::from(velocity), ink);
                }
            }
        }
        Resolved::Points(points) => {
            let end = view.x_of(scene.clip_length);
            let runs = step_runs(
                view,
                points.as_slice(),
                |p| p,
                end,
                body.left(),
                body.right(),
            );
            painter.line(step_polyline(&runs), ink);
        }
        Resolved::Span { from, to, curve } => {
            if scene.bend.is_none() {
                painter.line(sample_span(view, *from, *to, *curve), ink);
            }
        }
        Resolved::Ramp {
            from,
            to,
            notes,
            curve,
        } => {
            let from = (from.0, i32::from(from.1));
            let to = (to.0, i32::from(to.1));
            // The bend handle draws this same curve while it is dragged.
            if scene.bend.is_none() {
                painter.line(sample_span(view, from, to, *curve), ink);
            }
            for &note in notes {
                if let Some(note) = scene.notes.get(note) {
                    let start = note.start();
                    let level = curve_level(from, to, curve, fraction(from, to, start));
                    preview_head(painter, view, start, level.round() as i32, ink);
                }
            }
        }
        Resolved::Shift { time, value } => {
            let picked = Members::new(scene.picked_points);
            for point in scene.points.iter().filter(|p| picked.contains(p.index)) {
                let moved = pos2(view.x_of(point.at + *time), view.y_of(point.value + *value));
                painter.circle_filled(clamp_into(body, moved), POINT_R, palette.text);
            }
        }
        Resolved::Box(rect) => {
            painter.rect(
                *rect,
                theme::radius::SMALL,
                palette.accent.gamma_multiply(0.12),
                Stroke::new(1.0, palette.accent),
                StrokeKind::Inside,
            );
        }
    }
}

/// A velocity preview: a stem and a hollow head, skipped when out of view.
fn preview_head(painter: &Painter, view: &LaneView, at: Ticks, value: i32, ink: Stroke) {
    if !view.visible(at) {
        return;
    }
    let x = view.x_of(at);
    let head = pos2(x, view.y_of(value));
    painter.line_segment([pos2(x, view.y_of(view.kind.rest())), head], ink);
    painter.circle(head, STEM_RADIUS, Color32::TRANSPARENT, ink);
}

/// The fill of a step trace: a rectangle per run, in one mesh.
fn fill_runs(painter: &Painter, runs: &[StepRun], base: f32, colour: Color32) {
    let mut mesh = Mesh::default();
    for run in runs {
        let rect = Rect::from_min_max(pos2(run.x0, run.y.min(base)), pos2(run.x1, run.y.max(base)));
        mesh.add_colored_rect(rect, colour);
    }
    painter.add(Shape::from(mesh));
}

/// The range of `points` (time order) whose x lies in the body, as
/// `first..end`. `end` is at least `first`.
fn visible_points(view: &LaneView, points: &[LanePoint]) -> (usize, usize) {
    let (lo, hi) = (view.body.left(), view.body.right());
    let first = points.partition_point(|p| view.x_of(p.at) < lo);
    let end = points.partition_point(|p| view.x_of(p.at) <= hi).max(first);
    (first, end)
}

/// One run of a step trace: the value `y` holds from `x0` to `x1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StepRun {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
}

/// The step trace of `points` (time order) as runs over the x range `lo..=hi`.
/// A value holds to the next point, the last to `end_x`. The run from before
/// the left edge starts at `lo`; nothing is held before the first point.
pub(crate) fn step_runs<P: Copy>(
    view: &LaneView,
    points: &[P],
    pick: impl Fn(P) -> (Ticks, i32),
    end_x: f32,
    lo: f32,
    hi: f32,
) -> Vec<StepRun> {
    let first = points.partition_point(|&p| view.x_of(pick(p).0) < lo);
    let mut runs = Vec::new();
    for i in first.saturating_sub(1)..points.len() {
        let (at, value) = pick(points[i]);
        let left = view.x_of(at).max(lo);
        if left > hi {
            break;
        }
        let next = points.get(i + 1).map_or(end_x, |&p| view.x_of(pick(p).0));
        let right = next.max(left).min(hi);
        runs.push(StepRun {
            x0: left,
            x1: right,
            y: view.y_of(value),
        });
    }
    runs
}

/// The polyline of `runs`: consecutive runs join by a vertical step.
pub(crate) fn step_polyline(runs: &[StepRun]) -> Vec<Pos2> {
    runs.iter()
        .flat_map(|run| [pos2(run.x0, run.y), pos2(run.x1, run.y)])
        .collect()
}

/// Whether event dots show: fewer than [`DOT_LIMIT`] visible points, on
/// average [`DOT_GAP`] points apart. `span` is their x distance, first to last.
pub(crate) fn dots_visible(count: usize, span: f32) -> bool {
    count < DOT_LIMIT && (count < 2 || span / (count - 1) as f32 >= DOT_GAP)
}

/// A stem as the velocity lane thins them: its index into `LaneScene::stems`,
/// its x and its head's velocity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StemMark {
    pub index: usize,
    pub x: f32,
    pub velocity: u8,
}

/// Thins stems (in x order) to none closer than [`STEM_SPACING`]: the louder
/// of two is kept (the earlier on a tie); the hovered stem always wins.
pub(crate) fn thin_stems(
    marks: impl Iterator<Item = StemMark>,
    hovered: Option<usize>,
) -> Vec<StemMark> {
    let rank = |m: StemMark| (Some(m.index) == hovered, m.velocity);
    let mut kept: Vec<StemMark> = Vec::new();
    for mark in marks {
        match kept.last_mut() {
            Some(last) if mark.x - last.x < STEM_SPACING => {
                if rank(mark) > rank(*last) {
                    *last = mark;
                }
            }
            _ => kept.push(mark),
        }
    }
    kept
}

/// The sampled line of a span shaped by `curve`: [`CURVE_STEPS`] + 1 points.
pub(crate) fn sample_span(
    view: &LaneView,
    from: (Ticks, i32),
    to: (Ticks, i32),
    curve: Curve,
) -> Vec<Pos2> {
    let span = (to.0 - from.0).0 as f64;
    (0..=CURVE_STEPS)
        .map(|step| {
            let t = step as f32 / CURVE_STEPS as f32;
            let at = Ticks(from.0.0 + (span * f64::from(t)).round() as i64);
            let value = curve_level(from, to, &curve, t).round() as i32;
            pos2(view.x_of(at), view.y_of(value))
        })
        .collect()
}

/// The lane value `t` of the way along a span or ramp (`t` by time, 0 to 1),
/// unrounded: the curve's level at `t`, between the two ends.
fn curve_level(from: (Ticks, i32), to: (Ticks, i32), curve: &Curve, t: f32) -> f64 {
    f64::from(from.1) + f64::from(to.1 - from.1) * f64::from(curve.at(t))
}

/// How far `at` is from `from` towards `to`, as 0 to 1 by time, clamped.
fn fraction(from: (Ticks, i32), to: (Ticks, i32), at: Ticks) -> f32 {
    let span = (to.0 - from.0).0;
    if span == 0 {
        return 0.0;
    }
    (((at - from.0).0 as f64) / span as f64).clamp(0.0, 1.0) as f32
}

/// A scale label's y, kept inside the column.
pub(crate) fn label_y(y: f32, column: Rect) -> f32 {
    let (top, bottom) = (column.top() + LABEL_HALF, column.bottom() - LABEL_HALF);
    y.max(top).min(bottom)
}

/// Whether a label at `y` has room beside labels at `others`.
fn clear_of(y: f32, others: &[f32]) -> bool {
    others.iter().all(|o| (o - y).abs() >= 2.0 * LABEL_HALF)
}

/// The scale's `(y, text)` labels: the range's ends and pitch bend's 0.
pub(crate) fn scale_labels(view: &LaneView) -> Vec<(f32, String)> {
    let (low, high) = view.kind.range();
    let top = view.y_of(high);
    let bottom = view.y_of(low);
    let mut labels = vec![(top, high.to_string()), (bottom, low.to_string())];
    let centre = view.y_of(0);
    let clear = |y: f32| (centre - y).abs() >= 2.0 * LABEL_HALF;
    if view.kind == LaneKind::PitchBend && clear(top) && clear(bottom) {
        labels.push((centre, "0".to_owned()));
    } else if view.kind != LaneKind::PitchBend {
        // The middle of 0 to 127, as the mock-up's 127 / 64 / 0 scale.
        let middle = view.y_of((low + high + 1) / 2);
        if clear_of(middle, &[top, bottom]) {
            labels.push((middle, ((low + high + 1) / 2).to_string()));
        }
    }
    labels
}

/// A point clamped into `body`.
fn clamp_into(body: Rect, p: Pos2) -> Pos2 {
    pos2(
        p.x.max(body.left()).min(body.right()),
        p.y.max(body.top()).min(body.bottom()),
    )
}

/// Membership in a pick list: bisection when sorted, a scan otherwise.
struct Members<'a> {
    list: &'a [usize],
    sorted: bool,
}

impl<'a> Members<'a> {
    fn new(list: &'a [usize]) -> Self {
        Self {
            list,
            sorted: list.windows(2).all(|pair| pair[0] <= pair[1]),
        }
    }

    fn contains(&self, value: usize) -> bool {
        if self.sorted {
            self.list.binary_search(&value).is_ok()
        } else {
            self.list.contains(&value)
        }
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::time::PPQ;

    use super::*;
    use crate::timeview::TimeView;

    /// The geometry tests' view: 40 points a beat from x 100, y 50 to 250.
    fn lane(kind: LaneKind) -> LaneView {
        LaneView {
            body: Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 250.0)),
            time: TimeView {
                left: 0.0,
                zoom: 40.0,
            },
            clip_start: Ticks::ZERO,
            kind,
        }
    }

    fn beats(n: f64) -> Ticks {
        Ticks((n * PPQ as f64) as i64)
    }

    fn point(beat: f64, value: i32, index: usize) -> LanePoint {
        LanePoint {
            at: beats(beat),
            value,
            index,
        }
    }

    fn run(x0: f32, x1: f32, y: f32) -> StepRun {
        StepRun { x0, x1, y }
    }

    fn mark(index: usize, x: f32, velocity: u8) -> StemMark {
        StemMark { index, x, velocity }
    }

    fn pick(p: LanePoint) -> (Ticks, i32) {
        (p.at, p.value)
    }

    fn labels(view: &LaneView) -> Vec<String> {
        scale_labels(view)
            .into_iter()
            .map(|(_, text)| text)
            .collect()
    }

    #[test]
    fn the_bend_dot_of_a_linear_curve_lies_on_the_line_between_its_ends() {
        let view = lane(LaneKind::Controller(7));
        // Each midpoint value is whole, so the dot is exact.
        let ends = [
            ((Ticks::ZERO, 0), (beats(4.0), 100)),
            ((beats(1.0), 90), (beats(3.0), 10)),
            ((beats(0.5), 127), (beats(6.5), 1)),
        ];
        for (from, to) in ends {
            let bend = BendHandle {
                from,
                to,
                curve: Curve::LINEAR,
            };
            let a = pos2(view.x_of(from.0), view.y_of(from.1));
            let b = pos2(view.x_of(to.0), view.y_of(to.1));
            let dot = bend_dot(&view, &bend);
            let cross = (b.x - a.x) * (dot.y - a.y) - (b.y - a.y) * (dot.x - a.x);
            assert!(cross.abs() < 1e-2, "{dot:?} is off the line {a:?}-{b:?}");
            let middle = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            assert!(dot.distance(middle) < 0.01, "{dot:?} is not at {middle:?}");
        }
    }

    #[test]
    fn a_step_trace_holds_each_value_until_the_next_point() {
        let view = lane(LaneKind::Controller(7));
        let points = [point(0.0, 10, 0), point(1.0, 60, 1), point(2.0, 30, 2)];
        let end = view.x_of(beats(4.0));
        let (y10, y60, y30) = (view.y_of(10), view.y_of(60), view.y_of(30));
        let runs = step_runs(&view, &points, pick, end, 100.0, 500.0);
        assert_eq!(
            runs,
            [
                run(100.0, 140.0, y10),
                run(140.0, 180.0, y60),
                run(180.0, 260.0, y30)
            ]
        );
        assert_eq!(
            step_polyline(&runs),
            [
                pos2(100.0, y10),
                pos2(140.0, y10),
                pos2(140.0, y60),
                pos2(180.0, y60),
                pos2(180.0, y30),
                pos2(260.0, y30),
            ]
        );
        // The level held from before the left edge starts at the edge.
        let edge = step_runs(&view, &points, pick, end, 150.0, 170.0);
        assert_eq!(edge, [run(150.0, 170.0, y60)]);
        // Nothing is held before the first point.
        let late = step_runs(&view, &points[1..2], pick, end, 100.0, 500.0);
        assert_eq!(late, [run(140.0, 260.0, y60)]);
    }

    #[test]
    fn dots_show_only_when_few_enough_and_not_crowded() {
        assert!(dots_visible(0, 0.0));
        assert!(dots_visible(2, 5.0), "a gap of exactly the minimum");
        assert!(!dots_visible(2, 4.9));
        assert!(!dots_visible(3, 8.0), "four points on average");
        assert!(dots_visible(249, 249.0 * 5.0));
        assert!(!dots_visible(250, 1.0e6), "at the limit, not below it");
    }

    #[test]
    fn crowded_stems_keep_the_louder_one_and_the_hovered_one_always() {
        let marks = [
            mark(0, 100.0, 40),
            mark(1, 101.5, 90),
            mark(2, 104.0, 50),
            mark(3, 120.0, 70),
        ];
        let kept = thin_stems(marks.iter().copied(), None);
        assert_eq!(kept, [mark(1, 101.5, 90), mark(3, 120.0, 70)]);
        let hovered = thin_stems(marks.iter().copied(), Some(0));
        // Stem 1 yields to the hovered 0, and stem 2 is then 4 points from it.
        assert_eq!(
            hovered,
            [mark(0, 100.0, 40), mark(2, 104.0, 50), mark(3, 120.0, 70)]
        );
        let tie = [mark(0, 100.0, 60), mark(1, 102.0, 60)];
        assert_eq!(thin_stems(tie.iter().copied(), None), [mark(0, 100.0, 60)]);
    }

    #[test]
    fn a_sampled_span_runs_from_one_end_to_the_other() {
        let view = lane(LaneKind::Controller(7));
        let (from, to) = ((Ticks::ZERO, 0), (beats(4.0), 100));
        let points = sample_span(&view, from, to, Curve::LINEAR);
        assert_eq!(points.len(), CURVE_STEPS + 1);
        assert_eq!(points[0], pos2(view.x_of(from.0), view.y_of(0)));
        assert_eq!(points[CURVE_STEPS], pos2(view.x_of(to.0), view.y_of(100)));
        let middle = points[CURVE_STEPS / 2];
        assert!((middle.x - view.x_of(beats(2.0))).abs() < 1e-3);
        assert!((middle.y - view.y_of(50)).abs() < 1e-3);
    }

    #[test]
    fn the_fraction_along_a_span_is_clamped_and_safe_for_no_time() {
        let (from, to) = ((beats(1.0), 0), (beats(3.0), 100));
        let at = [beats(0.0), beats(2.0), beats(9.0)].map(|b| fraction(from, to, b));
        assert_eq!(at, [0.0, 0.5, 1.0]);
    }

    #[test]
    fn scale_labels_name_the_range_and_the_centre_and_stay_in_the_column() {
        assert_eq!(labels(&lane(LaneKind::Controller(7))), ["127", "0", "64"]);
        assert_eq!(labels(&lane(LaneKind::Velocity)), ["127", "1", "64"]);
        assert_eq!(labels(&lane(LaneKind::PitchBend)), ["8191", "-8192", "0"]);
        // Too short for a centre label beside the ends: it is left out.
        let short = Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 60.0));
        let crowded = LaneView {
            body: short,
            ..lane(LaneKind::PitchBend)
        };
        assert_eq!(labels(&crowded), ["8191", "-8192"]);
        let column = Rect::from_min_max(pos2(0.0, 50.0), pos2(40.0, 250.0));
        assert_eq!(label_y(-100.0, column), 50.0 + LABEL_HALF);
        assert_eq!(label_y(1000.0, column), 250.0 - LABEL_HALF);
        assert_eq!(label_y(120.0, column), 120.0);
    }
}
