//! Horizon's card (the limiter), from the round-2 specimen
//! (`docs/specimens/m8/devices.html`, `horizon()`): a sunset. The ceiling is
//! the horizon line and the music is the land under it; where the music would
//! rise past the line it is flattened against it, and the sun glows where it
//! presses.
//!
//! What the picture says comes from the device's own values and its meter:
//!
//! * **Ceiling** is the line, and its words ("ceiling −0.3 dB") are the real
//!   value: a lower ceiling is a lower horizon, and more of the music presses
//!   against it.
//! * **The land** is what the device hears, a level a frame across the last few
//!   seconds, kept at or under the line. Where a peak reached the line it is
//!   marked with a white dot, and the newest flash.
//! * **The sun** glows brighter and wider as the limiter takes more away.
//!
//! The screen is alive: the land scrolls with the music, and it is calm when
//! nothing plays (the specimen's land stands there, at rest) and still while
//! motion is reduced.
//!
//! It is a control too: dragging the ceiling line up or down is Ceiling, the
//! same edit as its dial, one undo step. Under the screen sit three lit dials
//! (Ceiling, Release, Look) and the "True" pill (peak detection between the
//! samples); nothing else is in All parameters but they are the same four.

use eframe::egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Mesh, Pos2, Rect, Shape, Stroke, Vec2, pos2,
    vec2,
};
use lumen_devices::dynamics::limiter as device;

use super::paint::{self, glow};
use super::{Control, Drawing, Handle, Scene, Values, meter};
use crate::chain::card::gradient_panel;
use crate::chain::geometry::Step;
use crate::motion;
use crate::theme::personality as p;
use crate::theme::personality::card;

/// The card: 300 x 330 with a 176 pt screen, as the specimen draws it.
const WHOLE: Vec2 = Vec2::new(300.0, 330.0);
const SPECIMEN: Vec2 = Vec2::new(272.0, 176.0);
/// The dials' centres across the card and down from its top, and the pill's
/// place (left, top, width, height).
const DIAL_X: [f32; 3] = [56.0, 124.0, 192.0];
const DIALS_Y: f32 = 278.0;
const PILL: (f32, f32, f32, f32) = (232.0, 262.0, 52.0, 20.0);

/// How the levels stand on the screen: a level's place down it is `TOP` of the
/// height at 0 dB and `TOP + SPAN` at `RANGE` dB down (the specimen's line, at
/// −0.3 dB, is at 36 %).
const TOP: f32 = 0.355;
const SPAN: f32 = 0.62;
const RANGE: f32 = 36.0;
/// The sun: where it stands across the picture, its core and glow radii at the
/// specimen's size, and how much a full press (`PRESS` decibels taken away)
/// widens them and strengthens the glow.
const SUN_AT: f32 = 0.62;
const SUN_CORE: f32 = 16.0;
const SUN_GLOW: f32 = 44.0;
const SUN_GLOW_ALPHA: f32 = 0.8;
const PRESS: f32 = 6.0;
const PRESS_WIDER: f32 = 0.35;
const PRESS_CORE: f32 = 0.12;
const PRESS_BRIGHT: f32 = 0.2;
/// The ground under the horizon: how much of it covers the sky.
const GROUND_ALPHA: f32 = 0.55;
/// The land: the columns across the screen (the specimen's 136 steps of 2 pt),
/// the spacing in the specimen's points, and the outline's width.
const COLUMNS: usize = 137;
const COLUMN_STEP: f32 = 2.0;
const OUTLINE: f32 = 1.2;
/// The specimen's resting land: `REST_BASE + REST_SWELL * sin(x / 9) sin(x / 37)
/// + REST_RIPPLE * sin(x / 2.1)` of the way up, scaled by `REST_HEIGHT`.
const REST_BASE: f32 = 0.62;
const REST_SWELL: f32 = 0.3;
const REST_RIPPLE: f32 = 0.08;
const REST_HEIGHT: f32 = 1.25;
/// Where the specimen marks flattened peaks on its resting land, in its points.
const REST_PEAKS: [f32; 3] = [52.0, 118.0, 196.0];
/// A peak's dot, at the specimen's size, and how much the newest flash.
const DOT: f32 = 2.5;
const FLASH: f32 = 0.6;
/// How many of the newest columns flash.
const FRESH: usize = 4;
/// A peak is pressed when it is this far over the ceiling, in decibels.
const PRESSED: f32 = 0.05;
/// A frame's level is kept for this long before the next is taken, so the land
/// scrolls at the same pace at any frame rate.
const TAKE_EVERY: f64 = 0.033;
/// The line: its width, and what its light adds when it is lit. Its glow.
const LINE: f32 = 2.0;
const LINE_LIT: f32 = 1.5;
const LINE_GLOW: f32 = 5.0;
/// The words on the line: in from the left, and over the line.
const WORDS_INSET: f32 = 8.0;
const WORDS_OVER: f32 = 6.0;
/// The line's grab, a half height.
const GRAB: f32 = 8.0;
/// The smallest a picture is scaled to, so a thin band still has a sun.
const LEAST_SCALE: f32 = 0.3;
const MINUS: char = '\u{2212}';

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: SPECIMEN.y,
    deep: p::HORIZON_GROUND,
    reads: &[],
    draw,
    controls: &[
        Control::knob(device::CEILING, "Ceiling", |c| dial(c, 0)).showing(bare),
        Control::knob(device::RELEASE, "Release", |c| dial(c, 1)).showing(tidy),
        Control::knob(device::LOOK, "Look", |c| dial(c, 2)).showing(tidy),
        Control::pill(device::DETECT, "True", |c| {
            Rect::from_min_size(
                pos2(c.left() + PILL.0, c.top() + PILL.1),
                vec2(PILL.2, PILL.3),
            )
        }),
    ],
    handles: &[Handle {
        name: "Ceiling line",
        param: device::CEILING,
        grab: |values, rect| line_box(values, rect),
        value_at: |values, pointer, rect| ceiling_at(values, pointer, rect),
        chip: |values, norm| {
            format!(
                "ceiling {}",
                db_words(values.plain_at(device::CEILING, norm as f32))
            )
        },
        cursor: CursorIcon::ResizeVertical,
        follows: None,
        also: None,
    }],
};

/// The cell of dial `index` on the whole card, centred on its place.
fn dial(card_rect: Rect, index: usize) -> Rect {
    Rect::from_center_size(
        pos2(card_rect.left() + DIAL_X[index], card_rect.top() + DIALS_Y),
        Vec2::splat(2.0 * card::DIAL_RADIUS + 4.0),
    )
}

/// "−0.3 dB" as "−0.3": the unit is the card's.
fn bare(text: &str) -> String {
    text.replace(" dB", "")
}

/// "60.0 ms" as "60 ms" and "1.50 ms" as "1.5 ms".
fn tidy(text: &str) -> String {
    match text.split_once(' ') {
        Some((number, unit)) if number.contains('.') => {
            format!(
                "{} {unit}",
                number.trim_end_matches('0').trim_end_matches('.')
            )
        }
        _ => text.to_owned(),
    }
}

/// A level as the screen words it: "−0.3 dB", with the minus sign the
/// typesetting uses.
fn db_words(db: f32) -> String {
    format!("{db:.1} dB").replace('-', &MINUS.to_string())
}

/// How big the picture is against the specimen's.
fn scale(rect: Rect) -> f32 {
    (rect.width() / SPECIMEN.x)
        .min(rect.height() / SPECIMEN.y)
        .clamp(LEAST_SCALE, 1.0)
}

/// Where a level (dBFS, 0 or under) stands down the picture.
fn y_of(db: f32, rect: Rect) -> f32 {
    rect.top() + rect.height() * (TOP + SPAN * (-db).clamp(0.0, RANGE) / RANGE)
}

/// The level at a place down the picture: not held to the picture, as a drag is
/// the change in it.
fn level_at(y: f32, rect: Rect) -> f32 {
    -((y - rect.top()) / rect.height() - TOP) / SPAN * RANGE
}

/// The line's place, and the band its grab is.
fn line_box(values: &Values<'_>, rect: Rect) -> Rect {
    let y = y_of(values.plain(device::CEILING), rect);
    Rect::from_x_y_ranges(rect.x_range(), (y - GRAB)..=(y + GRAB)).intersect(rect)
}

/// The Ceiling a pointer at `pointer` stands for: the line is where it is.
fn ceiling_at(values: &Values<'_>, pointer: Pos2, rect: Rect) -> f64 {
    values.norm_of(device::CEILING, level_at(pointer.y, rect))
}

/// The corner radii of the picture: its top is the screen's, its foot is too
/// except on the short card, where the dials' strip is under it.
fn corners(step: Step) -> (f32, f32) {
    (
        card::SCREEN_RADIUS,
        if step == Step::Short {
            0.0
        } else {
            card::SCREEN_RADIUS
        },
    )
}

/// The specimen's resting land at column `column`, as a place down the picture.
fn rest_y(column: usize, rect: Rect, horizon: f32) -> f32 {
    let x = column as f32 * COLUMN_STEP;
    let v =
        REST_BASE + REST_SWELL * (x / 9.0).sin() * (x / 37.0).sin() + REST_RIPPLE * (x / 2.1).sin();
    (rect.bottom() - v * (rect.bottom() - horizon) * REST_HEIGHT).max(horizon + 1.0)
}

/// The levels the device heard the last frames, newest last, as decibels
/// (`NAN` before they were heard), and how present they are (0 to 1: they come
/// in as audio starts and go as it stops). The state is the card's own, kept
/// between frames; with motion reduced there is none.
fn heard(scene: &Scene<'_>) -> ([f32; COLUMNS], f32) {
    let ctx = scene.painter.ctx();
    let key = scene.id.with("heard");
    let flowing = scene.level > 0.0 && scene.reading.is_some();
    let flow = motion::fade(ctx, scene.id.with("flow"), flowing);
    let mut run = ctx
        .data(|d| d.get_temp::<([f32; COLUMNS], f64)>(key))
        .unwrap_or(([f32::NAN; COLUMNS], f64::NEG_INFINITY));
    if let (true, Some(reading)) = (flowing, scene.reading) {
        let now = ctx.input(|i| i.time);
        if now - run.1 >= TAKE_EVERY {
            run.0.copy_within(1.., 0);
            run.0[COLUMNS - 1] = reading.input_db;
            run.1 = now;
            ctx.data_mut(|d| d.insert_temp(key, run));
        }
        // Asks for the next frame while the land is moving, and not otherwise.
        ctx.request_repaint_after(motion::METER_FRAME);
    } else if flow <= 0.0 {
        ctx.data_mut(|d| d.remove::<([f32; COLUMNS], f64)>(key));
    }
    (run.0, flow)
}

fn draw(scene: &Scene<'_>) {
    let (painter, rect) = (scene.painter, scene.rect);
    let values = &scene.values;
    let s = scale(rect);
    let ceiling = values.plain(device::CEILING);
    let horizon = y_of(ceiling, rect);
    let (top, foot) = corners(scene.step);
    let pressed = (meter::pulled(scene) / PRESS).clamp(0.0, 1.0);
    let band = scene.step == Step::Compact;

    // The sky, from the deep above to the light at the horizon.
    let sky = p::HORIZON_SKY.map(|c| scene.ink(c));
    gradient_panel(painter, rect, top, foot, |at| {
        let t = ((at.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        let k = p::HORIZON_SKY_AT
            .iter()
            .rposition(|&stop| stop <= t)
            .unwrap_or(0)
            .min(sky.len() - 2);
        let (from, to) = (p::HORIZON_SKY_AT[k], p::HORIZON_SKY_AT[k + 1]);
        sky[k].lerp_to_gamma(sky[k + 1], ((t - from) / (to - from)).clamp(0.0, 1.0))
    });

    // The sun, standing on the line, and the light it throws; the ground then
    // dims everything under the line.
    let sun = pos2(rect.left() + rect.width() * SUN_AT, horizon);
    glow(
        painter,
        sun,
        SUN_GLOW * s * (1.0 + PRESS_WIDER * pressed),
        scene
            .ink(p::HORIZON_GLOW)
            .gamma_multiply((SUN_GLOW_ALPHA + PRESS_BRIGHT * pressed).min(1.0)),
    );
    painter.circle_filled(
        sun,
        SUN_CORE * s * (1.0 + PRESS_CORE * pressed),
        scene.ink(p::HORIZON_LIGHT),
    );
    let ground = Rect::from_min_max(pos2(rect.left(), horizon), rect.max);
    painter.rect_filled(
        ground,
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: foot as u8,
            se: foot as u8,
        },
        scene.ink(p::HORIZON_GROUND).gamma_multiply(GROUND_ALPHA),
    );

    // The land, and where its peaks pressed the line.
    let (levels, flow) = heard(scene);
    let x_of = |column: usize| rect.left() + rect.width() * column as f32 / (COLUMNS - 1) as f32;
    let top_of = |column: usize| {
        let rest = rest_y(column, rect, horizon);
        let live = levels[column];
        if live.is_nan() {
            rest
        } else {
            rest + (y_of(live.min(ceiling), rect) - rest) * flow
        }
    };
    let land = scene.ink(p::HORIZON_LAND);
    let mut fill = Mesh::default();
    fill.vertices.reserve(2 * COLUMNS);
    fill.indices.reserve(6 * COLUMNS);
    for column in 0..COLUMNS {
        let x = x_of(column);
        // The foot follows the screen's rounded corners.
        let inset = (x - rect.left()).min(rect.right() - x);
        let radius = foot.min(rect.height() / 2.0);
        let drop = if foot > 0.0 && inset < radius {
            radius - (radius * radius - (radius - inset).powi(2)).max(0.0).sqrt()
        } else {
            0.0
        };
        let bottom = rect.bottom() - drop;
        fill.colored_vertex(pos2(x, top_of(column).min(bottom)), land);
        fill.colored_vertex(pos2(x, bottom), land);
        if column > 0 {
            let a = 2 * (column as u32 - 1);
            fill.add_triangle(a, a + 1, a + 2);
            fill.add_triangle(a + 1, a + 3, a + 2);
        }
    }
    painter.add(Shape::mesh(fill));
    let mut outline = paint::mesh_for(1, COLUMNS);
    paint::line(
        &mut outline,
        COLUMNS,
        |column| pos2(x_of(column), top_of(column)),
        OUTLINE,
        scene.ink(scene.hue),
    );
    painter.add(Shape::mesh(outline));

    // The line, its glow, and the words on it.
    let lit = scene.handle_lit[0];
    let light = scene.ink(p::HORIZON_LIGHT);
    painter.line_segment(
        [pos2(rect.left(), horizon), pos2(rect.right(), horizon)],
        Stroke::new(LINE_GLOW * lit, light.gamma_multiply(0.25 * lit)),
    );
    painter.line_segment(
        [pos2(rect.left(), horizon), pos2(rect.right(), horizon)],
        Stroke::new(LINE + LINE_LIT * lit, light),
    );
    // The peaks that pressed it: a white dot on the line, brightest and biggest
    // for the newest.
    let dot = DOT * s.max(0.8);
    let white = scene.ink(Color32::WHITE);
    for column in 0..COLUMNS {
        let live = levels[column];
        let (here, before, after) = (
            live,
            levels[column.saturating_sub(1)],
            levels[(column + 1).min(COLUMNS - 1)],
        );
        // A neighbour not yet heard (NaN) never hides the peak.
        let pressed_here = !live.is_nan()
            && here > ceiling + PRESSED
            && (before.is_nan() || before <= here)
            && (after.is_nan() || after <= here);
        if pressed_here && flow > 0.0 {
            let fresh = if column >= COLUMNS - FRESH {
                FLASH
            } else {
                0.0
            };
            painter.circle_filled(
                pos2(x_of(column), horizon),
                dot * (1.0 + fresh),
                white.gamma_multiply(flow),
            );
        }
    }
    let at_rest = 1.0 - flow;
    if at_rest > 0.0 {
        for peak in REST_PEAKS {
            painter.circle_filled(
                pos2(rect.left() + rect.width() * peak / SPECIMEN.x, horizon),
                dot,
                white.gamma_multiply(at_rest),
            );
        }
    }
    if band {
        return;
    }
    painter.text(
        pos2(rect.left() + WORDS_INSET, horizon - WORDS_OVER),
        Align2::LEFT_BOTTOM,
        format!("ceiling {}", db_words(ceiling)),
        FontId::monospace(p::READOUT_SIZE - 0.5),
        light.gamma_multiply(scene.settle),
    );
}

#[cfg(test)]
mod tests;
