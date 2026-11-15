use eframe::egui::{
    Align2, Color32, CursorIcon, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2,
};
use eframe::epaint::Mesh;
use lumen_devices::reverb as device;

use super::{Control, Drawing, Handle, Scene, Values, cadence};
use crate::chain::geometry::Step;
use crate::fonts;
use crate::theme::personality as p;
use crate::theme::personality::card;
use crate::theme::stroke;

const WHOLE: Vec2 = Vec2::new(300.0, 330.0);
const SPECIMEN: Vec2 = Vec2::new(272.0, 176.0);
const DIAL_X: [f32; 4] = [46.0, 102.0, 158.0, 214.0];
const DIALS_Y: f32 = 278.0;

const CENTRE: (f32, f32) = (0.42, 0.55);
const GLOW: Vec2 = Vec2::new(118.0, 72.0);
const SPACE: (f32, f32) = (0.5, 0.82);
const RINGS: usize = 4;
const RING_X: (f32, f32) = (30.0, 20.0);
const RING_Y: (f32, f32) = (18.0, 12.0);
const RING_ALPHA: (f32, f32) = (0.27, 0.045);
const FADE: (f32, f32) = (0.6, 0.9);
const STOPS: [f32; 4] = [0.0, 0.18, 0.5, 1.0];
const HUE_STRENGTH: f32 = 0.62;
const SIDES: usize = 56;
const GLOW_STRENGTH: (f32, f32) = (0.88, 1.0);
const WARMING: f32 = 0.85;

const SWELL_WIDER: f32 = 0.16;
const SWELL_CORE: f32 = 0.3;
const SWELL_RINGS: f32 = 0.5;
const SILENCE: f32 = 6.9;
const AT_REST: f32 = 0.004;
const LONGEST_STEP: f32 = 0.1;

const WORDS_INSET: f32 = 12.0;
const HERO_DOWN: f32 = 30.0;
const SUB_DOWN: f32 = 46.0;
const HERO_SIZE: f32 = 22.0;
const HERO_SHORT: f32 = 16.0;
const SUB_SHORT_DOWN: f32 = 34.0;
const HERO_SHORT_DOWN: f32 = 22.0;
const LEAST_SCALE: f32 = 0.3;
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

fn dial(card_rect: Rect, index: usize) -> Rect {
    Rect::from_center_size(
        pos2(card_rect.left() + DIAL_X[index], card_rect.top() + DIALS_Y),
        Vec2::splat(2.0 * card::DIAL_RADIUS + 4.0),
    )
}

fn without_space(text: &str) -> String {
    text.replace(' ', "")
}

fn decay_words(seconds: f32) -> String {
    format!("{seconds:.1} s")
}

fn scales(rect: Rect) -> Vec2 {
    vec2(
        (rect.width() / SPECIMEN.x).clamp(LEAST_SCALE, 1.0),
        (rect.height() / SPECIMEN.y).clamp(LEAST_SCALE, 1.0),
    )
}

fn centre(rect: Rect) -> Pos2 {
    pos2(
        rect.left() + rect.width() * CENTRE.0,
        rect.top() + rect.height() * CENTRE.1,
    )
}

fn space_of(norm: f32) -> f32 {
    SPACE.0 + SPACE.1 * norm
}

fn fade_of(norm: f32) -> f32 {
    FADE.0 + FADE.1 * norm
}

fn glow_radii(rect: Rect, norm: f32) -> Vec2 {
    let s = scales(rect);
    vec2(GLOW.x * s.x, GLOW.y * s.y) * space_of(norm)
}

fn glow_box(values: &Values<'_>, rect: Rect) -> Rect {
    let radii = glow_radii(rect, values.norm(device::SIZE)) * GLOW_GRAB;
    Rect::from_center_size(centre(rect), radii * 2.0).intersect(rect)
}

fn glow_value(pointer: Pos2, rect: Rect) -> f64 {
    let s = scales(rect);
    let at = pointer - centre(rect);
    let reach = (at.x / (GLOW.x * s.x)).hypot(at.y / (GLOW.y * s.y));
    f64::from((reach - SPACE.0) / SPACE.1)
}

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
        cadence::again(ctx, scene.still);
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
