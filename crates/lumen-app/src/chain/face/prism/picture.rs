use eframe::egui::{Align2, Color32, FontId, Mesh, Pos2, Shape, Stroke, pos2, vec2};

use super::super::Scene;
use super::super::paint::{self, glow_into};
use super::geometry::{BEAMS, Frame};
use super::light::Light;
use crate::theme::glow as ladder;
use crate::theme::personality as p;
use crate::theme::personality::card;

const BASELINE_DROP: f32 = 0.22;

const GAIN_CURVE: f32 = 0.6;
const GAIN_LEAST: f32 = 0.25;
const GAIN_MOST: f32 = 1.4;
const LIVE_REST: f32 = 0.14;
const LIVE_AWAKE: f32 = 0.6;
const LIVE_ENERGY: f32 = 0.3;

const LIGHT_IN: f32 = 1.8;
const LIGHT_IN_ALPHA: (f32, f32) = (0.35, 0.6);
const LIGHT_IN_GLOW: (f32, f32, f32) = (16.0, 10.0, 0.25);
const LIGHT_IN_GLOW_BACK: f32 = 10.0;

const WEDGE_ROOT: f32 = 1.2;
const WEDGE_TIP: f32 = 7.0;
const WEDGE_TIP_ENERGY: f32 = 5.0;
const WEDGE_STOPS: [(f32, f32); 3] = [(0.0, 0.78), (0.55, 0.42), (1.0, 0.06)];
const CORE: f32 = 1.4;
const CORE_ALPHA: f32 = 0.95;

const PACKETS: usize = 2;
const PACKET_SPEED: f32 = 150.0;
const PACKET_SKEW: f32 = 31.0;
const PACKET_RADIUS: (f32, f32) = (7.0, 5.0);
const PACKET_ALPHA: (f32, f32) = (0.2, 0.7);
const PACKET_FADE: f32 = 0.6;
const PACKETS_FROM: f32 = 0.02;

const TIP_GLOW: (f32, f32) = (6.0, 0.35);
const TIP_DOT: f32 = 3.0;

const STEM_GLOW: (f32, f32, f32) = (12.0, 14.0, 10.0);
const STEM_ALPHA: (f32, f32) = (0.35, 0.15);

const FILL_STOPS: [f32; 3] = [0.20, 0.05, 0.12];
const GLASS_EDGE: f32 = 1.2;
const GLASS_EDGE_ALPHA: f32 = 0.55;
const GLASS_SIDE: f32 = 1.8;
const GLASS_SIDE_ALPHA: (f32, f32) = (0.55, 0.4);
const THROUGH: f32 = 2.0;
const THROUGH_ALPHA: (f32, f32) = (0.25, 0.5);

const WORDS_SIZE: f32 = p::READOUT_SIZE;
const WORDS_ALPHA: f32 = 0.85;
const WORDS_TOP: f32 = 18.0;
const SUB_SIZE: f32 = 8.5;
const SUB_ALPHA: f32 = 0.4;
const SUB_TOP: f32 = 30.0;
const MONO_BELOW: f32 = 0.006;

pub(super) fn brightness(awake: f32, energy: f32, gain_db: f32) -> f32 {
    let live = LIVE_REST + LIVE_AWAKE * awake + LIVE_ENERGY * awake * energy;
    (live * gain_of(gain_db)).clamp(0.0, 1.0)
}

pub(super) fn gain_of(gain_db: f32) -> f32 {
    10.0_f32
        .powf(gain_db / 20.0)
        .powf(GAIN_CURVE)
        .clamp(GAIN_LEAST, GAIN_MOST)
}

fn lit(colour: Color32, alpha: f32) -> Color32 {
    ladder::additive(colour.gamma_multiply(alpha.clamp(0.0, 1.0)))
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Look {
    pub awake: f32,
    pub brightness: f32,
    pub time: f64,
    pub handles: bool,
}

pub(super) fn light(scene: &Scene<'_>, frame: &Frame, light: &Light, look: Look) {
    let Look {
        awake,
        brightness: a,
        time,
        handles,
    } = look;
    let (k, energy) = (frame.k(), light.energy());
    let white = scene.ink(Color32::WHITE);
    let mut mesh = Mesh::default();
    light_in(&mut mesh, frame, white, a);
    glow_into(
        &mut mesh,
        pos2(frame.entry().x - LIGHT_IN_GLOW_BACK * k, frame.entry().y),
        (LIGHT_IN_GLOW.0 + LIGHT_IN_GLOW.1 * energy) * k,
        white,
        LIGHT_IN_GLOW.2 * a,
        true,
    );
    let root = frame.root();
    let tip_half = (WEDGE_TIP + WEDGE_TIP_ENERGY * energy * awake) * k;
    for (index, colour) in p::PRISM_BEAMS.iter().enumerate().take(BEAMS) {
        let colour = scene.ink(*colour);
        let tip = frame.tip(index);
        wedge(
            &mut mesh,
            (root, tip),
            (WEDGE_ROOT * k, tip_half),
            colour,
            a,
        );
        paint::line(
            &mut mesh,
            2,
            |i| if i == 0 { root } else { tip },
            (CORE * k).max(1.0),
            lit(colour, CORE_ALPHA * a),
        );
        if awake > PACKETS_FROM && !scene.still {
            packets(
                &mut mesh,
                frame,
                (root, tip),
                (index, colour),
                (awake, energy),
                time,
            );
        }
    }
    for upper in [true, false].into_iter().filter(|_| handles) {
        glow_into(
            &mut mesh,
            frame.pinned_tip(upper),
            TIP_GLOW.0 * k,
            white,
            TIP_GLOW.1,
            true,
        );
    }
    let (back, ahead, bloom) = STEM_GLOW;
    glow_into(
        &mut mesh,
        root,
        (back + ahead * energy + bloom * light.bloom().max(0.0)) * k,
        white,
        STEM_ALPHA.0 * a + STEM_ALPHA.1 * light.flash(),
        true,
    );
    scene.painter.add(Shape::mesh(mesh));
}

fn light_in(mesh: &mut Mesh, frame: &Frame, white: Color32, a: f32) {
    let entry = frame.entry();
    let half = LIGHT_IN * frame.k();
    let from = frame.left();
    let (near, far) = (
        Color32::TRANSPARENT,
        lit(white, LIGHT_IN_ALPHA.0 + LIGHT_IN_ALPHA.1 * a),
    );
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(pos2(from, entry.y - half), near);
    mesh.colored_vertex(pos2(entry.x, entry.y - half), far);
    mesh.colored_vertex(pos2(entry.x, entry.y + half), far);
    mesh.colored_vertex(pos2(from, entry.y + half), near);
    mesh.add_triangle(base, base + 1, base + 2);
    mesh.add_triangle(base, base + 2, base + 3);
}

fn wedge(
    mesh: &mut Mesh,
    (from, to): (Pos2, Pos2),
    (root_half, tip_half): (f32, f32),
    colour: Color32,
    a: f32,
) {
    const FEATHER: f32 = 1.0;
    let along = to - from;
    let length = along.length().max(1e-3);
    let across = vec2(-along.y, along.x) / length;
    let base = mesh.vertices.len() as u32;
    for (stop, alpha) in WEDGE_STOPS {
        let centre = from + along * stop;
        let half = root_half + (tip_half - root_half) * stop;
        let body = lit(colour, alpha * a);
        mesh.colored_vertex(centre - across * (half + FEATHER), Color32::TRANSPARENT);
        mesh.colored_vertex(centre - across * half, body);
        mesh.colored_vertex(centre + across * half, body);
        mesh.colored_vertex(centre + across * (half + FEATHER), Color32::TRANSPARENT);
    }
    for part in 0..WEDGE_STOPS.len() as u32 - 1 {
        for side in 0..3 {
            let near = base + 4 * part + side;
            let far = near + 4;
            mesh.add_triangle(near, near + 1, far);
            mesh.add_triangle(near + 1, far + 1, far);
        }
    }
}

fn packets(
    mesh: &mut Mesh,
    frame: &Frame,
    (root, tip): (Pos2, Pos2),
    (index, colour): (usize, Color32),
    (awake, energy): (f32, f32),
    time: f64,
) {
    let (s, length) = (frame.s(), frame.length());
    for packet in 0..PACKETS {
        let along = (time as f32 * PACKET_SPEED * s
            + index as f32 * PACKET_SKEW * s
            + packet as f32 * length * 0.5)
            .rem_euclid(length)
            / length;
        glow_into(
            mesh,
            root + (tip - root) * along,
            (PACKET_RADIUS.0 + PACKET_RADIUS.1 * energy) * frame.k(),
            colour,
            awake * (PACKET_ALPHA.0 + PACKET_ALPHA.1 * energy) * (1.0 - along * PACKET_FADE),
            true,
        );
    }
}

fn fill_alpha(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        FILL_STOPS[0] + (FILL_STOPS[1] - FILL_STOPS[0]) * (t / 0.5)
    } else {
        FILL_STOPS[1] + (FILL_STOPS[2] - FILL_STOPS[1]) * ((t - 0.5) / 0.5)
    }
}

pub(super) fn glass(scene: &Scene<'_>, frame: &Frame, a: f32) {
    let white = scene.ink(Color32::WHITE);
    let corners = [
        frame.glass_point(0),
        frame.glass_point(2),
        frame.glass_point(1),
    ];
    let (from, to) = frame.glass_fill();
    let line = to - from;
    let mut fill = Mesh::default();
    for corner in corners {
        let t = (corner - from).dot(line) / line.length_sq();
        fill.colored_vertex(corner, white.gamma_multiply(fill_alpha(t)));
    }
    fill.add_triangle(0, 1, 2);
    scene.painter.add(Shape::mesh(fill));
    let k = frame.s().max(0.6);
    scene.painter.add(Shape::closed_line(
        corners.to_vec(),
        Stroke::new(
            (GLASS_EDGE * k).max(1.0),
            white.gamma_multiply(GLASS_EDGE_ALPHA),
        ),
    ));
    scene.painter.line_segment(
        [corners[0], corners[2]],
        Stroke::new(
            (GLASS_SIDE * k).max(1.0),
            white.gamma_multiply(GLASS_SIDE_ALPHA.0 + GLASS_SIDE_ALPHA.1 * a),
        ),
    );
    let mut through = Mesh::default();
    paint::line(
        &mut through,
        2,
        |i| if i == 0 { frame.entry() } else { frame.exit() },
        (THROUGH * frame.k()).max(1.0),
        lit(white, THROUGH_ALPHA.0 + THROUGH_ALPHA.1 * a),
    );
    scene.painter.add(Shape::mesh(through));
}

pub(super) fn tip_handles(scene: &Scene<'_>, frame: &Frame) {
    let white = scene.ink(Color32::WHITE);
    for (index, upper) in [(0, true), (1, false)] {
        let lit = scene.handle_lit[index];
        if lit <= 0.0 {
            continue;
        }
        let tip = frame.pinned_tip(upper);
        scene.painter.circle_stroke(
            tip,
            card::HANDLE_RING * lit,
            Stroke::new(
                crate::theme::stroke::HAIRLINE,
                white.gamma_multiply(card::HANDLE_RING_ALPHA * lit),
            ),
        );
        scene
            .painter
            .circle_filled(tip, TIP_DOT, white.gamma_multiply(lit));
    }
}

pub(super) fn words(scene: &Scene<'_>, frame: &Frame, width: f32) {
    let white = scene.ink(Color32::WHITE);
    let (right, top) = (scene.rect.right() - p::READOUT_INSET, scene.rect.top());
    scene.painter.text(
        pos2(right, top + WORDS_TOP + WORDS_SIZE * BASELINE_DROP),
        Align2::RIGHT_BOTTOM,
        format!("width {width:.0}%"),
        FontId::monospace(WORDS_SIZE),
        white.gamma_multiply(WORDS_ALPHA * scene.settle),
    );
    scene.painter.text(
        pos2(right, top + SUB_TOP + SUB_SIZE * BASELINE_DROP),
        Align2::RIGHT_BOTTOM,
        if frame.spread() < MONO_BELOW {
            "mono"
        } else {
            "stereo"
        },
        FontId::monospace(SUB_SIZE),
        white.gamma_multiply(SUB_ALPHA * scene.settle),
    );
}
