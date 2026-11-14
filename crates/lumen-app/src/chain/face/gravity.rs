//! Gravity's card (the compressor), from the round-2 specimen
//! (`docs/specimens/m8/devices.html`, `gravity()`): the signal is a body pulled
//! into a well, and the deeper it sinks the more it is pressed. A warped grid
//! is the well, a lit ball is the body, a dashed ring is the threshold, and the
//! gain pulled away is the big number in the corner.
//!
//! What the picture says comes from the device's own values and its meter:
//!
//! * **Pull** (the threshold) is the dashed ring: a lower threshold is a wider
//!   ring, and its words ("−18 dB") are the real value.
//! * **Ratio** is how heavy the body is: a bigger ratio makes the well deeper
//!   and the ball larger.
//! * **The gain reduction** is the ball's sink into the well, how far the grid
//!   is drawn in around it and the number ("−4.1", "dB pulled"): the most the
//!   compressor took away since the last frame, falling back over a moment.
//!
//! The screen is alive: the well deepens and the ball sinks while the device is
//! pulling, the ball glows with the level it hears, and it is calm when nothing
//! plays and still while motion is reduced.
//!
//! It is a control too: dragging the ball up or down is Ratio, and dragging
//! anywhere in the ring (the field the threshold marks), out or in, is Pull. Each
//! is the same edit as its dial, one undo step. Under the screen sit five lit
//! dials (Pull, Ratio, Attack, Release, Makeup); the knee, the mix, the
//! detector, the key's high-pass and Listen are in All parameters. When the
//! effect is keyed from another track a chip on the screen says so and opens the
//! sidechain choice (`card/screen/key.rs`).

use eframe::egui::{
    Align2, Color32, CursorIcon, FontId, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2,
};
use lumen_devices::dynamics::compressor as device;

use super::meter;
use super::paint::{self, dashed_ellipse, glow, mesh_for};
use super::{Control, Drawing, Handle, Scene, Values};
use crate::chain::geometry::Step;
use crate::fonts;
use crate::theme::personality as p;
use crate::theme::personality::card;

/// The card: 380 x 330 with a 176 pt screen, as the specimen draws it.
const WHOLE: Vec2 = Vec2::new(380.0, 330.0);
const SPECIMEN: Vec2 = Vec2::new(352.0, 176.0);
/// The dials' centres across the card and down from its top. The specimen
/// spreads them to 304; here the last stops at 284 so the "All n" link in the
/// card's corner keeps a clear gap from the Makeup value.
const DIAL_X: [f32; 5] = [44.0, 104.0, 164.0, 224.0, 284.0];
const DIALS_Y: f32 = 278.0;
/// How far "dB pulled" is paled toward white, so it reads on the light theme's
/// paler top of the screen as well as on the dark one.
const PULLED_PALE: f32 = 0.45;

/// Where the well's middle stands, as a share of the picture's width and
/// height.
const CENTRE: (f32, f32) = (0.42, 0.52);
/// The grid: rows (`-ROWS..=ROWS`) and columns (`-COLUMNS..=COLUMNS`), the gap
/// between them, how far the well reaches along each, and how far it sags a
/// row (in total at the middle, then the deepest row's share) or draws a
/// column in; all in the specimen's points, and how bright a line is (the
/// middle row's strength and what each row out loses).
const ROWS: i32 = 6;
const ROW_PITCH: f32 = 13.0;
const ROW_REACH: f32 = 70.0;
const ROW_SAG: (f32, f32) = (18.0, 14.0);
const ROW_ALPHA: (f32, f32) = (0.12, 0.06);
const COLUMNS: i32 = 9;
const COLUMN_PITCH: f32 = 20.0;
const COLUMN_REACH: f32 = 60.0;
const COLUMN_PINCH: f32 = 0.25;
const COLUMN_ALPHA: f32 = 0.12;
const GRID_WIDTH: f32 = 1.0;
/// Points on a row across the picture, and on a column down it.
const ROW_POINTS: usize = 29;
const COLUMN_POINTS: usize = 15;

/// The body: its radius, where it rests below the well's middle and how far it
/// sinks (`SINK_MOST * tanh(reduction / SINK_OVER)`, so the specimen's 4.1 dB
/// is its 12 pt), the halo round it and the focus of its highlight.
const BODY: f32 = 17.0;
const REST_DOWN: f32 = 4.0;
const SINK_MOST: f32 = 28.0;
const SINK_OVER: f32 = 9.0;
const HALO: f32 = 46.0;
const HALO_ALPHA: f32 = 0.55;
/// How the body's colours run: the pale ring at this share of the way, and
/// where the highlight stands (off the middle, as a share of the radius).
const BODY_PALE_AT: f32 = 0.35;
const BODY_LIGHT: Vec2 = Vec2::new(-0.3, -0.4);
const BODY_LIGHT_REACH: f32 = 1.4;
/// What the level the device hears adds to the halo.
const LEVEL_HALO: f32 = 0.2;
/// How much heavier a bigger Ratio makes the body (its radius, at the largest
/// ratio the screen shows): `1 - HEAVY + HEAVY * ratio / specimen ratio`.
const HEAVY: f32 = 0.2;

/// The well's depth: shallow at rest and 1 at the specimen's 4.1 dB (more
/// beyond it, up to this much), as `WELL_REST + WELL_GAIN * (1 - e^(-r / over))`
/// over the same at 4.1 dB.
const WELL_REST: f32 = 0.4;
const WELL_OVER: f32 = 3.5;
const WELL_AT: f32 = 4.1;
const WELL_MOST: f32 = 1.3;
/// A bigger Ratio digs the well deeper: 1 at the specimen's 4:1.
const RATIO_AT: f32 = 4.0;
const WELL_RATIO: (f32, f32) = (0.6, 1.8);

/// The threshold ring: its half width at a Pull of 0 and what each unit of
/// Pull adds (the specimen's 74 at its -18 dB), its height as a share of its
/// width, how far below the well's middle it stands, and its dashes.
const RING: (f32, f32) = (40.0, 113.0);
const RING_SQUASH: f32 = 22.0 / 74.0;
const RING_DOWN: f32 = 18.0;
const RING_DASH: (f32, f32) = (3.0, 4.0);
const RING_ALPHA: f32 = 0.45;
const RING_LIT_ALPHA: f32 = 0.9;
const RING_WIDTH: f32 = 1.0;
const RING_LIT_WIDTH: f32 = 1.6;
/// The words on the ring: how far right of it and below its middle.
const RING_WORDS: (f32, f32) = (4.0, 4.0);
const RING_WORDS_ALPHA: f32 = 0.7;
/// The ball's grab, as a half size, and how far a drag of the whole picture's
/// height moves Ratio.
const ORB_GRAB: f32 = 30.0;
const ORB_TRAVEL: f32 = 0.8;
/// The ring of light round the ball while it is lit.
const ORB_RING: f32 = 3.0;

/// The words in the corner: their inset from the right, the baseline of the
/// hero and of the line under it, from the picture's top, and the hero's size
/// on the whole card and the short one.
const WORDS_INSET: f32 = 14.0;
const HERO_DOWN: f32 = 30.0;
const SUB_DOWN: f32 = 46.0;
const HERO_SIZE: f32 = 26.0;
const HERO_SHORT: f32 = 18.0;
const HERO_SHORT_DOWN: f32 = 20.0;
const SUB_SHORT_DOWN: f32 = 32.0;
/// The smallest a picture is scaled to, so a thin band still has a well.
const LEAST_SCALE: f32 = 0.3;
const MINUS: char = '\u{2212}';

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: SPECIMEN.y,
    deep: p::GRAVITY_DEEP,
    reads: &[],
    draw,
    controls: &[
        Control::knob(device::THRESHOLD, "Pull", |c| dial(c, 0)).showing(tidy),
        Control::knob(device::RATIO, "Ratio", |c| dial(c, 1)).showing(tidy),
        Control::knob(device::ATTACK, "Attack", |c| dial(c, 2)).showing(tidy),
        Control::knob(device::RELEASE, "Release", |c| dial(c, 3)).showing(tidy),
        Control::knob(device::MAKEUP, "Makeup", |c| dial(c, 4)).showing(lower),
    ],
    // The ring's field is first, under the ball's: where they overlap the ball
    // is what is taken.
    handles: &[
        Handle {
            name: "Threshold ring",
            param: device::THRESHOLD,
            grab: |values, rect| ring_box(values, rect),
            value_at: |_, pointer, rect| ring_value(pointer, rect),
            chip: |values, norm| {
                format!(
                    "pull {}",
                    tidy(&db_words(values.plain_at(device::THRESHOLD, norm as f32)))
                )
            },
            cursor: CursorIcon::ResizeNeSw,
            follows: None,
            also: None,
        },
        Handle {
            name: "Body weight",
            param: device::RATIO,
            grab: |_, rect| orb_box(rect),
            value_at: |_, pointer, rect| orb_value(pointer, rect),
            chip: |values, norm| {
                format!(
                    "ratio {:.1} : 1",
                    values.plain_at(device::RATIO, norm as f32)
                )
            },
            cursor: CursorIcon::ResizeVertical,
            follows: None,
            also: None,
        },
    ],
};

/// The cell of dial `index` on the whole card, centred on its place.
fn dial(card_rect: Rect, index: usize) -> Rect {
    Rect::from_center_size(
        pos2(card_rect.left() + DIAL_X[index], card_rect.top() + DIALS_Y),
        Vec2::splat(2.0 * card::DIAL_RADIUS + 4.0),
    )
}

/// "-18.0 dB" as "-18 dB", "4.0:1" as "4:1", "5.00 ms" as "5 ms".
fn tidy(text: &str) -> String {
    text.replace(".00 ", " ")
        .replace(".0 ", " ")
        .replace(".0:", ":")
}

/// "Auto" as "auto".
fn lower(text: &str) -> String {
    text.to_lowercase()
}

/// A level as the screen words it: "−18.0 dB", with the minus sign the
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

/// The well's middle in the picture.
fn centre(rect: Rect) -> Pos2 {
    pos2(
        rect.left() + rect.width() * CENTRE.0,
        rect.top() + rect.height() * CENTRE.1,
    )
}

/// The ring's half width at a Pull (0 to 1: how far the threshold is below 0 dB)
/// of `pull`, in points.
fn ring_half(pull: f32, s: f32) -> f32 {
    (RING.0 + RING.1 * pull) * s
}

/// The ring's centre and half sizes, from the threshold's value (0 to 1).
fn ring(rect: Rect, threshold: f32) -> (Pos2, Vec2) {
    let s = scale(rect);
    let half = ring_half(1.0 - threshold, s);
    // A thin band (the compact card's) keeps the ring inside it.
    let tall = (half * RING_SQUASH).min(rect.height() * 0.42);
    let at = centre(rect) + vec2(0.0, RING_DOWN * s);
    (at, vec2(half, tall))
}

/// The field the ring marks, for its grab.
fn ring_box(values: &Values<'_>, rect: Rect) -> Rect {
    let (at, radii) = ring(rect, values.norm(device::THRESHOLD));
    Rect::from_center_size(at, radii * 2.0).intersect(rect)
}

/// The Threshold a pointer at `pointer` stands for: the ring's edge is where
/// the pointer is. Not held to 0 to 1: a drag is the change in it, and the
/// handle clamps what it adds up to.
fn ring_value(pointer: Pos2, rect: Rect) -> f64 {
    let s = scale(rect);
    let at = pointer - (centre(rect) + vec2(0.0, RING_DOWN * s));
    let reach = (at.x / (ring_half(0.3, s))).hypot(at.y / (ring_half(0.3, s) * RING_SQUASH));
    // A ring half as wide as the specimen's is a Pull of (74 * r - 40) / 113.
    let pull = (ring_half(0.3, 1.0) * reach - RING.0) / RING.1;
    f64::from(1.0 - pull)
}

/// Where the ball rests, and the box its grab is.
fn orb_centre(rect: Rect) -> Pos2 {
    centre(rect) + vec2(0.0, (REST_DOWN + SINK_MOST / 2.0) * scale(rect))
}

fn orb_box(rect: Rect) -> Rect {
    Rect::from_center_size(orb_centre(rect), Vec2::splat(2.0 * ORB_GRAB * scale(rect)))
        .intersect(rect)
}

/// The Ratio a pointer at `pointer` stands for: lower on the screen is heavier, as a
/// body is pulled further into the well. A drag of 80 % of the picture's height is the
/// whole range.
fn orb_value(pointer: Pos2, rect: Rect) -> f64 {
    f64::from((pointer.y - rect.top()) / (rect.height() * ORB_TRAVEL))
}

/// How deep the well is: shallow at rest, 1 at the specimen's reduction.
fn depth(reduction: f32, ratio: f32) -> f32 {
    let grown = |r: f32| 1.0 - (-r / WELL_OVER).exp();
    let well = WELL_REST + (1.0 - WELL_REST) * grown(reduction) / grown(WELL_AT);
    let heavy =
        (WELL_RATIO.0 + (1.0 - WELL_RATIO.0) * ratio / RATIO_AT).clamp(WELL_RATIO.0, WELL_RATIO.1);
    well.min(WELL_MOST) * heavy
}

fn draw(scene: &Scene<'_>) {
    let (painter, rect) = (scene.painter, scene.rect);
    let values = &scene.values;
    let band = scene.step == Step::Compact;
    let s = scale(rect);
    let reduction = meter::pulled(scene);
    let ratio = values.plain(device::RATIO);
    let depth = depth(reduction, ratio);
    let at = centre(rect);
    let hue = scene.hue;

    grid(scene, at, s, depth);

    // The body in its well: a halo, then the ball with its highlight.
    let sunk = SINK_MOST * (reduction / SINK_OVER).tanh();
    let body = pos2(at.x, at.y + (REST_DOWN + sunk) * s);
    let weight = 1.0 - HEAVY + HEAVY * (ratio / RATIO_AT).min(2.0);
    let radius = BODY * s * weight;
    let halo = HALO_ALPHA + LEVEL_HALO * scene.level;
    glow(
        painter,
        body,
        HALO * s,
        scene.ink(hue).gamma_multiply(halo.min(1.0)),
    );
    let light = body + BODY_LIGHT * radius;
    let (white, pale) = (scene.ink(p::GRAVITY_ORB[0]), scene.ink(p::GRAVITY_ORB[1]));
    let mut ball = mesh_for(0, 0);
    paint::disc(&mut ball, body, radius, 3, |point| {
        let t = ((point - light).length() / (BODY_LIGHT_REACH * radius)).clamp(0.0, 1.0);
        if t < BODY_PALE_AT {
            white.lerp_to_gamma(pale, t / BODY_PALE_AT)
        } else {
            pale.lerp_to_gamma(scene.ink(hue), (t - BODY_PALE_AT) / (1.0 - BODY_PALE_AT))
        }
    });
    painter.add(Shape::mesh(ball));
    let lit_orb = scene.handle_lit[1];
    if lit_orb > 0.0 {
        painter.circle_stroke(
            body,
            radius + ORB_RING,
            Stroke::new(
                crate::theme::stroke::HAIRLINE,
                scene.ink(hue).gamma_multiply(lit_orb),
            ),
        );
    }

    // The threshold ring and its words.
    let (ring_at, radii) = ring(rect, values.norm(device::THRESHOLD));
    let lit = scene.handle_lit[0];
    let white_ring = scene.ink(Color32::WHITE);
    dashed_ellipse(
        painter,
        ring_at,
        radii,
        Stroke::new(
            RING_WIDTH + (RING_LIT_WIDTH - RING_WIDTH) * lit,
            white_ring.gamma_multiply(RING_ALPHA + (RING_LIT_ALPHA - RING_ALPHA) * lit),
        ),
        (RING_DASH.0 * s.max(0.6), RING_DASH.1 * s.max(0.6)),
    );
    if band {
        return;
    }
    let words = tidy(&db_words(values.plain(device::THRESHOLD)));
    let font = FontId::monospace(p::READOUT_SIZE - 0.5);
    let wide = painter
        .layout_no_wrap(words.clone(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let words_at = pos2(
        (ring_at.x + radii.x + RING_WORDS.0).min(rect.right() - wide - 6.0),
        ring_at.y + RING_WORDS.1,
    );
    painter.text(
        words_at,
        Align2::LEFT_BOTTOM,
        words,
        font,
        white_ring.gamma_multiply(RING_WORDS_ALPHA),
    );

    // The hero: what the compressor is taking away, and what it is.
    let short = scene.step == Step::Short;
    let right = rect.right() - WORDS_INSET;
    let (hero_y, sub_y, size) = if short {
        (HERO_SHORT_DOWN, SUB_SHORT_DOWN, HERO_SHORT)
    } else {
        (HERO_DOWN, SUB_DOWN, HERO_SIZE)
    };
    let hero = if meter::is_none(reduction) {
        "0.0".to_owned()
    } else {
        format!("{MINUS}{reduction:.1}")
    };
    painter.text(
        pos2(right, rect.top() + hero_y),
        Align2::RIGHT_BOTTOM,
        hero,
        fonts::display(size),
        scene.ink(Color32::WHITE).gamma_multiply(scene.settle),
    );
    painter.text(
        pos2(right, rect.top() + sub_y),
        Align2::RIGHT_BOTTOM,
        "dB pulled",
        FontId::monospace(p::READOUT_SIZE),
        scene
            .ink(hue.lerp_to_gamma(Color32::WHITE, PULLED_PALE))
            .gamma_multiply(scene.settle),
    );
}

/// The warped grid, the well: one mesh of feathered lines, rows sagging and
/// columns drawing in toward the well's middle by `depth`.
fn grid(scene: &Scene<'_>, at: Pos2, s: f32, depth: f32) {
    let rect = scene.rect;
    let ink = scene.ink(scene.hue);
    let mut mesh = mesh_for(
        (2 * ROWS + 1) as usize + (2 * COLUMNS + 1) as usize,
        ROW_POINTS.max(COLUMN_POINTS),
    );
    for k in -ROWS..=ROWS {
        let near = (ROWS - k.abs()) as f32 / ROWS as f32;
        let alpha = ROW_ALPHA.0 + ROW_ALPHA.1 * near;
        let sag = ROW_SAG.0 * (1.0 - k.abs() as f32 / (ROWS + 1) as f32) + ROW_SAG.1;
        let base = at.y + k as f32 * ROW_PITCH * s;
        paint::line(
            &mut mesh,
            ROW_POINTS,
            |i| {
                let x = rect.left() + rect.width() * i as f32 / (ROW_POINTS - 1) as f32;
                let d = (-((x - at.x) / (ROW_REACH * s)).powi(2)).exp();
                pos2(x, base + d * sag * s * depth)
            },
            GRID_WIDTH,
            ink.gamma_multiply(alpha),
        );
    }
    for k in -COLUMNS..=COLUMNS {
        let x0 = at.x + k as f32 * COLUMN_PITCH * s;
        paint::line(
            &mut mesh,
            COLUMN_POINTS,
            |i| {
                let y = rect.top() + rect.height() * i as f32 / (COLUMN_POINTS - 1) as f32;
                let d = (-((y - at.y) / (COLUMN_REACH * s)).powi(2)).exp();
                pos2(x0 - (x0 - at.x) * d * COLUMN_PINCH * depth, y)
            },
            GRID_WIDTH,
            ink.gamma_multiply(COLUMN_ALPHA),
        );
    }
    scene.painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests;
