//! Drawing the lane (M5 slice 4b): the plotting area under the piano roll,
//! with stems (velocity) or a step trace (controllers), the picks, the hover
//! and the gesture's preview. Like `note_editor::paint` it draws from data
//! only; the document is never changed to show a preview.

use eframe::egui::{Color32, Painter, Pos2, Rect, pos2};
use lumen_core::curve::Curve;
use lumen_core::grid::Line;
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use super::geometry::LaneView;
use super::gesture::Resolved;

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
    /// An index into `LaneScene::stems`.
    Stem(usize),
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
    let _ = (painter, view, scene);
}

/// Draws the value scale in `column` (the strip left of the plot): the
/// range's top and bottom, and the centre line's 0 for pitch bend.
pub(crate) fn paint_scale(painter: &Painter, view: &LaneView, column: Rect) {
    let _ = (painter, view, column);
}
