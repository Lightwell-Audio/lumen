//! What the tempo and meter lanes look like (M7 slice A6). The lanes are
//! tinted like automation lanes, so they read as lanes and not as tracks;
//! bar lines run through both so a change is seen against the bars it
//! moves.

use eframe::egui::{Align2, FontId, Painter, Rect, Stroke, pos2};
use lumen_core::meter::MeterMap;
use lumen_core::time::{Ticks, bpm_at};
use lumen_project::edit::maps::tempo_in;
use lumen_project::model::Project;

use super::scale::{Plane, Scale, bpm_text, chip, chip_width};
use super::{Band, Picked, Which};
use crate::arrangement::auto_view::draw_point;
use crate::theme;

/// What the lanes need to be drawn.
pub(crate) struct Look<'a> {
    pub project: &'a Project,
    /// The song's bars, as the transport has them.
    pub meter: &'a MeterMap,
    pub playhead: Ticks,
    pub picked: Option<Picked>,
    pub hover: Option<Picked>,
    pub scale: Scale,
}

/// Bars narrower than this (points) get no line, so a zoomed-out song is not
/// a wall of lines.
const LEAST_BAR: f32 = 8.0;
/// The most bar lines drawn.
const MOST_BARS: i64 = 600;

pub(crate) fn draw(painter: &Painter, band: &Band, look: &Look<'_>) {
    corner(painter, band, look);
    let lanes = Rect::from_x_y_ranges(
        band.tempo.rect.x_range(),
        band.tempo.rect.top()..=band.meter.rect.bottom(),
    );
    let painter = painter.with_clip_rect(lanes);
    tempo_lane(&painter, &band.tempo, look);
    meter_lane(&painter, &band.meter, look);
    bar_lines(&painter, band, look);
}

/// The name column: each lane's name and what is in force at the playhead.
fn corner(painter: &Painter, band: &Band, look: &Look<'_>) {
    let palette = theme::p();
    painter.rect_filled(band.corner, 0.0, palette.raised);
    let tempo = tempo_in(&look.project.tempo, look.playhead);
    let meter = look.meter.meter_at(look.playhead);
    let rows = [
        ("Tempo", bpm_text(tempo), band.tempo.rect),
        ("Meter", meter.to_string(), band.meter.rect),
    ];
    for (name, value, rect) in rows {
        let y = rect.center().y;
        painter.text(
            pos2(band.corner.left() + theme::space::M, y),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(theme::size::CAPTION),
            palette.text_secondary,
        );
        painter.text(
            pos2(band.corner.right() - theme::space::M, y),
            Align2::RIGHT_CENTER,
            value,
            FontId::monospace(theme::size::MONO_SMALL),
            palette.text_hint,
        );
    }
    painter.hline(
        band.corner.x_range(),
        band.corner.bottom() - 0.5,
        Stroke::new(theme::stroke::HAIRLINE, palette.border),
    );
}

/// The lane's wash and its two edges.
fn wash(painter: &Painter, rect: Rect, strength: f32) {
    let palette = theme::p();
    painter.rect_filled(rect, 0.0, palette.accent.gamma_multiply(strength));
    let edge = Stroke::new(theme::stroke::HAIRLINE, palette.accent.gamma_multiply(0.45));
    painter.hline(rect.x_range(), rect.top() + 0.5, edge);
    painter.hline(rect.x_range(), rect.bottom() - 0.5, edge);
}

fn tempo_lane(painter: &Painter, plane: &Plane, look: &Look<'_>) {
    let palette = theme::p();
    let rect = plane.rect;
    wash(painter, rect, 0.05);
    let points = &look.project.tempo;
    if points.is_empty() {
        return;
    }
    let scale = look.scale;
    // The line: the tempo playing at the left edge, then each change. A step
    // holds its tempo and jumps; a ramp slopes to the next change.
    let first = points.partition_point(|p| plane.x(p.at) < rect.left());
    let playing = |x: f32| plane.y(scale, bpm_at(points, plane.ticks_at(x)));
    let mut y = playing(rect.left());
    let mut path = vec![pos2(rect.left(), y)];
    for (i, point) in points.iter().enumerate().skip(first) {
        let x = plane.x(point.at);
        if x > rect.right() {
            break;
        }
        let ramped = i > 0 && points[i - 1].ramp;
        if !ramped {
            path.push(pos2(x, y));
        }
        y = plane.y(scale, point.bpm);
        path.push(pos2(x, y));
    }
    path.push(pos2(rect.right(), playing(rect.right())));
    painter.add(eframe::egui::Shape::line(
        path,
        Stroke::new(theme::stroke::LIT, palette.accent),
    ));
    let font = FontId::monospace(theme::size::MONO_SMALL);
    for (i, point) in points.iter().enumerate().skip(first) {
        let at = plane.pos(scale, point);
        if at.x > rect.right() + theme::size::AUTO_POINT {
            break;
        }
        let me = Picked {
            which: Which::Tempo,
            index: i,
        };
        draw_point(
            painter,
            at,
            palette.accent,
            look.picked == Some(me),
            look.hover == Some(me),
        );
        // The value, beside the point, unless the next change is too close.
        let text = bpm_text(point.bpm);
        let room = points
            .get(i + 1)
            .map_or(f32::MAX, |next| plane.x(next.at) - at.x);
        if room > chip_width(&text) + theme::space::S {
            let below = at.y - theme::size::AUTO_POINT - 12.0 < rect.top();
            let (anchor, align) = if below {
                (pos2(at.x + 7.0, at.y + 4.0), Align2::LEFT_TOP)
            } else {
                (pos2(at.x + 7.0, at.y - 4.0), Align2::LEFT_BOTTOM)
            };
            painter.text(anchor, align, text, font.clone(), palette.text_secondary);
        }
    }
}

fn meter_lane(painter: &Painter, plane: &Plane, look: &Look<'_>) {
    let palette = theme::p();
    let rect = plane.rect;
    wash(painter, rect, 0.03);
    let points = &look.project.meter;
    let font = FontId::monospace(theme::size::MONO_SMALL);
    let radius = theme::radius::SMALL;
    // The meter in force at the left edge, when its chip has scrolled away.
    let in_force = points
        .partition_point(|p| plane.x(p.at) <= rect.left())
        .saturating_sub(1);
    if let Some(point) = points.get(in_force)
        && chip(plane, point).left() < rect.left()
        && points
            .get(in_force + 1)
            .is_none_or(|next| plane.x(next.at) > rect.left() + 2.0 * chip_width("00/00"))
    {
        painter.text(
            pos2(rect.left() + theme::size::SONG_CHIP_PAD, rect.center().y),
            Align2::LEFT_CENTER,
            point.meter.to_string(),
            font.clone(),
            palette.text_hint,
        );
    }
    for (i, point) in points.iter().enumerate() {
        let box_ = chip(plane, point);
        if box_.right() < rect.left() {
            continue;
        }
        if box_.left() > rect.right() {
            break;
        }
        let me = Picked {
            which: Which::Meter,
            index: i,
        };
        let picked = look.picked == Some(me);
        let (fill, edge) = if picked {
            (palette.accent_muted, palette.accent)
        } else if look.hover == Some(me) {
            (palette.control_hover, palette.border_strong)
        } else {
            (palette.control, palette.border)
        };
        painter.rect(
            box_,
            radius,
            fill,
            Stroke::new(theme::stroke::HAIRLINE, edge),
            eframe::egui::StrokeKind::Inside,
        );
        painter.text(
            box_.center(),
            Align2::CENTER_CENTER,
            point.meter.to_string(),
            font.clone(),
            palette.text,
        );
    }
}

/// A line at each bar, through both lanes; none when bars are too narrow or
/// too many to tell apart.
fn bar_lines(painter: &Painter, band: &Band, look: &Look<'_>) {
    let rect = Rect::from_x_y_ranges(
        band.tempo.rect.x_range(),
        band.tempo.rect.top()..=band.meter.rect.bottom(),
    );
    let map = look.meter;
    let plane = &band.tempo;
    let from = map.bbt_at(plane.ticks_at(rect.left())).bar;
    let to = map.bbt_at(plane.ticks_at(rect.right())).bar + 1;
    if to - from > MOST_BARS || to - from < 1 {
        return;
    }
    let width = plane.x(map.bar_start(from + 1)) - plane.x(map.bar_start(from));
    if width < LEAST_BAR {
        return;
    }
    let stroke = Stroke::new(
        theme::stroke::HAIRLINE,
        theme::p().border.gamma_multiply(0.6),
    );
    for bar in from..=to {
        let x = plane.x(map.bar_start(bar));
        if (rect.left()..=rect.right()).contains(&x) {
            painter.vline(x, rect.y_range(), stroke);
        }
    }
}
