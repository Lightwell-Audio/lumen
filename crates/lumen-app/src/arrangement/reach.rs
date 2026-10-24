//! A crossfade as an overlap (M7b E2, the owner's look): the outgoing clip
//! reaches to the end of the crossing, with its title bar and edge carried
//! over the next clip, and the next clip's name starts after the crossing.
//! The clips still touch at the join in the project; this is only drawn.

use eframe::egui::{self, Color32, Painter, Rect, Stroke, StrokeKind, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, Id, Project};

use super::TITLE;
use crate::theme;
use crate::timeview::TimeView;

/// Clips narrower than this (in points) draw no reach: at fit-all, thousands
/// of clips are in view and each lookup scans its track.
pub(super) const MIN_WIDTH: f32 = 24.0;

/// How wide, in points, the crossing into the next clip is at `clip`'s end.
pub(super) fn outgoing_width(
    project: &Project,
    track: Id,
    clip: &Clip,
    time: TimeView,
    canvas_left: f32,
) -> f32 {
    let reach = crate::crossfades::outgoing_extent(project, track, clip);
    width(reach, clip.start + clip.length, time, canvas_left)
}

/// How wide, in points, the crossing at the start of `clip` is: where its
/// name starts after.
pub(super) fn incoming_width(
    project: &Project,
    track: Id,
    clip: &Clip,
    time: TimeView,
    canvas_left: f32,
) -> f32 {
    match crate::crossfades::fade(project, track, clip) {
        Some(fade) => width(fade.length, clip.start, time, canvas_left),
        None => 0.0,
    }
}

fn width(length: Ticks, from: Ticks, time: TimeView, canvas_left: f32) -> f32 {
    if length <= Ticks::ZERO {
        return 0.0;
    }
    time.x(canvas_left, (from + length).as_quarters()) - time.x(canvas_left, from.as_quarters())
}

/// Draws the outgoing clip's reach of `width` points from the right edge of
/// `rect`: its title bar in `bar`, and its edge in `outline` closing at the
/// end of the crossing.
pub(super) fn draw(painter: &Painter, rect: Rect, width: f32, bar: Color32, outline: Color32) {
    if width <= 0.0 {
        return;
    }
    let reach = Rect::from_min_size(rect.right_top(), vec2(width, rect.height()));
    let radius = theme::radius::SMALL;
    let round = egui::CornerRadius {
        nw: 0,
        ne: radius,
        sw: 0,
        se: radius,
    };
    // The bar starts a corner's width back, over the notch the rounded
    // corners of the two clips leave at the join.
    let title = Rect::from_min_max(
        pos2(reach.left() - f32::from(radius), reach.top()),
        pos2(reach.right(), reach.top() + TITLE),
    );
    painter.rect_filled(
        title,
        egui::CornerRadius {
            nw: 0,
            ne: radius,
            sw: 0,
            se: 0,
        },
        bar,
    );
    painter.rect_stroke(
        reach,
        round,
        Stroke::new(theme::stroke::HAIRLINE, outline),
        StrokeKind::Inside,
    );
}
