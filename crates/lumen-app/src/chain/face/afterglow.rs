//! Afterglow's card (the reverb), from the round-2 specimen
//! (`docs/specimens/m8/devices.html`, `afterglow()`): a light that lingers
//! where the sound was. The room is a soft glow whose size and fade you shape,
//! with rings round it, and the time it takes to go quiet in the corner.
//!
//! What the picture says comes from the device's own values:
//!
//! * **Space** is the glow's size: a bigger room is a wider glow.
//! * **Fade** is how far the rings reach, and the words in the corner are the
//!   real decay ("2.4 s to silence").
//! * **Warmth** (the damping) turns the light from a cool white to an amber.
//! * **Mix** is how strong the glow is.
//!
//! The screen is alive: the glow swells with what the device hears, and when the
//! sound stops it fades away over the real decay, as the reverb's tail does. It
//! is still when nothing plays and while motion is reduced.
//!
//! It is a control too: dragging the glow is Space (the glow's edge follows the
//! pointer), and dragging outward from it, over the rings, is Fade. Each is the
//! same edit as its dial, one undo step. Under the screen sit four lit dials
//! (Space, Fade, Warmth, Mix); the pre-delay, the width and the early
//! reflections are in All parameters.

use eframe::egui::{
    Align2, Color32, CursorIcon, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2,
};
use eframe::epaint::Mesh;
use lumen_devices::reverb as device;

use super::{Control, Drawing, Handle, Scene, Values};
use crate::chain::geometry::Step;
use crate::theme::personality as p;
use crate::theme::personality::card;
use crate::theme::stroke;
use crate::{fonts, motion};

/// The card: 300 x 330 with a 176 pt screen, as the specimen draws it.
const WHOLE: Vec2 = Vec2::new(300.0, 330.0);
const SPECIMEN: Vec2 = Vec2::new(272.0, 176.0);
/// The dials' centres across the card and down from its top. The specimen
/// spreads them to 240; here the last stops at 214 so the "All n" link in the
/// card's corner keeps a clear gap from the Mix value, as Parallax's does.
const DIAL_X: [f32; 4] = [46.0, 102.0, 158.0, 214.0];
const DIALS_Y: f32 = 278.0;

/// Where the glow's centre stands, as a share of the picture's width and
/// height.
const CENTRE: (f32, f32) = (0.42, 0.55);
/// The glow's half width and height at the specimen's Space (62 %), and how
/// Space scales it: `SPACE.0 + SPACE.1 * space`, 1 at the specimen's.
const GLOW: Vec2 = Vec2::new(118.0, 72.0);
const SPACE: (f32, f32) = (0.5, 0.82);
/// The rings: how many, their half width and height at the specimen's Fade
/// (`a * k + b`), and their opacity (`a - b * k`).
const RINGS: usize = 4;
const RING_X: (f32, f32) = (30.0, 20.0);
const RING_Y: (f32, f32) = (18.0, 12.0);
const RING_ALPHA: (f32, f32) = (0.27, 0.045);
/// How Fade scales the rings: `FADE.0 + FADE.1 * fade`, 1 at the specimen's.
const FADE: (f32, f32) = (0.6, 0.9);
/// The glow's gradient: where each stop stands (a share of the radius) and how
/// strong the hue is at the third.
const STOPS: [f32; 4] = [0.0, 0.18, 0.5, 1.0];
const HUE_STRENGTH: f32 = 0.62;
/// The glow is drawn with this many sides.
const SIDES: usize = 56;
/// What Mix does to the glow's strength: from at 0 to at 100 %.
const GLOW_STRENGTH: (f32, f32) = (0.88, 1.0);
/// What Warmth does to the core: how far toward the warm colour it goes at the
/// most, and how much a cool core is of the way there at 0.
const WARMING: f32 = 0.85;

/// With the level at 1 the glow is this much wider, the core this much
/// brighter and the rings this much stronger.
const SWELL_WIDER: f32 = 0.16;
const SWELL_CORE: f32 = 0.3;
const SWELL_RINGS: f32 = 0.5;
/// The glow's fall back to rest takes the real decay: it is quiet (60 dB down,
/// as the reverb's decay is measured) after this many "time constants".
const SILENCE: f32 = 6.9;
/// A swell this small is rest.
const AT_REST: f32 = 0.004;
/// The longest a frame's step is taken as, so a stalled frame does not jump.
const LONGEST_STEP: f32 = 0.1;

/// The words in the corner: their inset from the right, the baseline of the
/// hero and of the line under it, from the picture's top, and the hero's size
/// on the whole card and the short one.
const WORDS_INSET: f32 = 12.0;
const HERO_DOWN: f32 = 30.0;
const SUB_DOWN: f32 = 46.0;
const HERO_SIZE: f32 = 22.0;
const HERO_SHORT: f32 = 16.0;
const SUB_SHORT_DOWN: f32 = 34.0;
const HERO_SHORT_DOWN: f32 = 22.0;
/// The smallest a picture is scaled to, so a thin band still has a glow.
const LEAST_SCALE: f32 = 0.3;
/// The grab for the glow is its box, this share of its full size; the grab for
/// the rings is the whole picture, less this inset.
const GLOW_GRAB: f32 = 0.9;
const RINGS_INSET: f32 = 4.0;

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: SPECIMEN.y,
    deep: p::REVERB_DEEP,
    reads: &[],
    draw,
    controls: &[
        Control::knob(device::SIZE, "Space", |c| dial(c, 0)).showing(without_space),
        Control::knob(device::DECAY, "Fade", |c| dial(c, 1)),
        Control::knob(device::DAMPING, "Warmth", |c| dial(c, 2)).showing(without_space),
        Control::knob(device::MIX, "Mix", |c| dial(c, 3)).showing(without_space),
    ],
    // The rings' grab is first, under the glow's: where they overlap the glow
    // is what is taken.
    handles: &[
        Handle {
            name: "Fade ring",
            param: device::DECAY,
            grab: |_, rect| rect.shrink(RINGS_INSET),
            value_at: |_, pointer, rect| ring_value(pointer, rect),
            chip: |values, norm| {
                format!(
                    "{} to silence",
                    decay_words(values.plain_at(device::DECAY, norm as f32))
                )
            },
            cursor: CursorIcon::ResizeHorizontal,
            follows: None,
            also: None,
        },
        Handle {
            name: "Glow size",
            param: device::SIZE,
            grab: |values, rect| glow_box(values, rect),
            value_at: |_, pointer, rect| glow_value(pointer, rect),
            chip: |values, norm| {
                format!(
                    "{:.0}% of the room",
                    values.plain_at(device::SIZE, norm as f32)
                )
            },
            cursor: CursorIcon::ResizeNeSw,
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

/// "20 %" as "20%".
fn without_space(text: &str) -> String {
    text.replace(' ', "")
}

/// The decay in words: "2.4 s".
fn decay_words(seconds: f32) -> String {
    format!("{seconds:.1} s")
}

/// How big the picture is against the specimen's, across and down.
fn scales(rect: Rect) -> Vec2 {
    vec2(
        (rect.width() / SPECIMEN.x).clamp(LEAST_SCALE, 1.0),
        (rect.height() / SPECIMEN.y).clamp(LEAST_SCALE, 1.0),
    )
}

/// The glow's centre in the picture.
fn centre(rect: Rect) -> Pos2 {
    pos2(
        rect.left() + rect.width() * CENTRE.0,
        rect.top() + rect.height() * CENTRE.1,
    )
}

/// How much bigger than the specimen's the glow is at Space `norm` (0 to 1).
fn space_of(norm: f32) -> f32 {
    SPACE.0 + SPACE.1 * norm
}

/// How much bigger than the specimen's the rings are at Fade `norm` (0 to 1).
fn fade_of(norm: f32) -> f32 {
    FADE.0 + FADE.1 * norm
}

/// The glow's half width and height, in points, at Space `norm`.
fn glow_radii(rect: Rect, norm: f32) -> Vec2 {
    let s = scales(rect);
    vec2(GLOW.x * s.x, GLOW.y * s.y) * space_of(norm)
}

/// The box the glow stands in, for its grab.
fn glow_box(values: &Values<'_>, rect: Rect) -> Rect {
    let radii = glow_radii(rect, values.norm(device::SIZE)) * GLOW_GRAB;
    Rect::from_center_size(centre(rect), radii * 2.0).intersect(rect)
}

/// The Space a pointer at `pointer` stands for: the glow's edge is where the
/// pointer is. Not held to 0 to 1: a drag is the change in it (the handle
/// clamps what it adds up to), so pressing inside the glow and pulling out
/// grows the room from where it was.
fn glow_value(pointer: Pos2, rect: Rect) -> f64 {
    let s = scales(rect);
    let at = pointer - centre(rect);
    let reach = (at.x / (GLOW.x * s.x)).hypot(at.y / (GLOW.y * s.y));
    f64::from((reach - SPACE.0) / SPACE.1)
}

/// The Fade a pointer at `pointer` stands for: the outer ring is where the
/// pointer is (not held to 0 to 1, as [`glow_value`]).
fn ring_value(pointer: Pos2, rect: Rect) -> f64 {
    let s = scales(rect);
    let at = pointer - centre(rect);
    let (x, y) = (
        RING_X.0 * RINGS as f32 + RING_X.1,
        RING_Y.0 * RINGS as f32 + RING_Y.1,
    );
    let reach = (at.x / (x * s.x)).hypot(at.y / (y * s.y));
    f64::from((reach - FADE.0) / FADE.1)
}

/// How far the glow has swelled, 0 to 1: it follows the level up at once and
/// falls back over the real decay (60 dB down in `decay` seconds). The state is
/// the card's own, kept between frames; with motion reduced there is none.
fn swell(scene: &Scene<'_>, decay: f32) -> f32 {
    let ctx = scene.painter.ctx();
    let key = scene.id.with("swell");
    if scene.still {
        ctx.data_mut(|d| d.remove::<(f32, f64)>(key));
        return 0.0;
    }
    let now = ctx.input(|i| i.time);
    let (was, at) = ctx
        .data(|d| d.get_temp::<(f32, f64)>(key))
        .unwrap_or((0.0, now));
    let step = ((now - at) as f32).clamp(0.0, LONGEST_STEP);
    let fallen = was * (-step * SILENCE / decay.max(0.05)).exp();
    let now_swell = scene.level.max(fallen);
    let kept = if now_swell < AT_REST { 0.0 } else { now_swell };
    ctx.data_mut(|d| d.insert_temp(key, (kept, now)));
    if kept > 0.0 {
        // Asks for the next frame while the glow is moving, and not otherwise.
        ctx.request_repaint_after(motion::METER_FRAME);
    }
    kept
}

fn draw(scene: &Scene<'_>) {
    let (painter, rect) = (scene.painter, scene.rect);
    let values = &scene.values;
    let decay = values.plain(device::DECAY);
    let swell = swell(scene, decay);
    let band = scene.step == Step::Compact;
    let space = values.norm(device::SIZE);
    let warmth = values.norm(device::DAMPING);
    let mix = values.norm(device::MIX);
    let strength = GLOW_STRENGTH.0 + (GLOW_STRENGTH.1 - GLOW_STRENGTH.0) * mix;
    let at = centre(rect);
    let s = scales(rect);
    let radii = glow_radii(rect, space) * (1.0 + SWELL_WIDER * swell);

    // The glow: the core, the warm ring and the hue, fading to nothing.
    let core = p::REVERB_CORE.lerp_to_gamma(p::REVERB_WARM, WARMING * warmth);
    let core_strength = (1.0 + SWELL_CORE * swell).min(1.0);
    let stops = [
        scene.ink(core).gamma_multiply(strength * core_strength),
        scene
            .ink(p::REVERB_WARM)
            .gamma_multiply(strength * core_strength),
        scene.ink(scene.hue).gamma_multiply(HUE_STRENGTH * strength),
        Color32::TRANSPARENT,
    ];
    radial(painter, at, radii, &stops);

    // The rings the tail leaves: how far they reach is the fade.
    let reach = fade_of(values.norm(device::DECAY)) * space_of(space);
    let rings = (1.0 + SWELL_RINGS * swell).min(2.0);
    for k in 1..=RINGS {
        let k = k as f32;
        let radii = vec2(
            (RING_X.0 * k + RING_X.1) * s.x,
            (RING_Y.0 * k + RING_Y.1) * s.y,
        ) * reach;
        painter.add(Shape::ellipse_stroke(
            at,
            radii,
            Stroke::new(
                stroke::HAIRLINE,
                scene
                    .ink(Color32::WHITE)
                    .gamma_multiply(((RING_ALPHA.0 - RING_ALPHA.1 * k) * rings).clamp(0.0, 1.0)),
            ),
        ));
    }
    if band {
        return;
    }

    // The hero: the real decay, and what it is the time to.
    let short = scene.step == Step::Short;
    let right = rect.right() - WORDS_INSET;
    let (hero_y, sub_y, size) = if short {
        (HERO_SHORT_DOWN, SUB_SHORT_DOWN, HERO_SHORT)
    } else {
        (HERO_DOWN, SUB_DOWN, HERO_SIZE)
    };
    painter.text(
        pos2(right, rect.top() + hero_y),
        Align2::RIGHT_BOTTOM,
        decay_words(decay),
        fonts::display(size),
        scene.ink(Color32::WHITE).gamma_multiply(scene.settle),
    );
    painter.text(
        pos2(right, rect.top() + sub_y),
        Align2::RIGHT_BOTTOM,
        "to silence",
        eframe::egui::FontId::monospace(p::READOUT_SIZE),
        scene.ink(p::REVERB_WARM).gamma_multiply(scene.settle),
    );
}

/// A radial gradient: an ellipse of half width and height `radii` round `at`
/// whose colour is `stops[k]` at `STOPS[k]` of the way out.
fn radial(painter: &Painter, at: Pos2, radii: Vec2, stops: &[Color32; 4]) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(at, stops[0]);
    for (k, colour) in stops.iter().enumerate().skip(1) {
        for side in 0..SIDES {
            let angle = std::f32::consts::TAU * side as f32 / SIDES as f32;
            let out = STOPS[k];
            mesh.colored_vertex(
                pos2(
                    at.x + radii.x * out * angle.cos(),
                    at.y + radii.y * out * angle.sin(),
                ),
                *colour,
            );
        }
    }
    let ring = |k: usize, side: usize| (1 + k * SIDES + side % SIDES) as u32;
    for side in 0..SIDES {
        mesh.add_triangle(0, ring(0, side), ring(0, side + 1));
    }
    for k in 0..stops.len() - 2 {
        for side in 0..SIDES {
            mesh.add_triangle(ring(k, side), ring(k + 1, side), ring(k + 1, side + 1));
            mesh.add_triangle(ring(k, side), ring(k + 1, side + 1), ring(k, side + 1));
        }
    }
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests;
