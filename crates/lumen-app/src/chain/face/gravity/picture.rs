use eframe::egui::{Align2, Color32, FontId, Mesh, Pos2, Shape, Stroke, Vec2, vec2};

use super::super::paint::{self, dashed_ellipse, glow_ladder, mesh_for};
use super::super::{Scene, meter};
use super::geometry::{
    BODY, BODY_UP, COLUMNS, Frame, GLOW_DOWN, ROWS, Readout, middle, ring, rows,
};
use super::well::{Membrane, Well};
use crate::fonts;
use crate::motion;
use crate::theme;
use crate::theme::personality as p;

const MINUS: char = '\u{2212}';
const BASELINE_DROP: f32 = 0.22;
const SMOOTH_STEPS: usize = 2;
const LINE_WIDTH: f32 = 1.0;
const ROW_ALPHA: (f32, f32) = (0.10, 0.22);
const COLUMN_ALPHA: f32 = 0.10;
const PRESSED_ALPHA: f32 = 0.5;
const PRESSED_PER_UNIT: f32 = 2.2;

const GLOW_REACH: f32 = 120.0;
const GLOW_AWAKE: f32 = 0.18;
const GLOW_DEPTH: f32 = 0.3;

const RING_ALPHA: (f32, f32) = (0.22, 0.6);
const RING_LIT_ALPHA: f32 = 0.9;
const RING_WIDTH: (f32, f32) = (1.2, 1.6);
const RING_DASH: (f32, f32) = (5.0, 4.0);
const RING_DEPTH_SHARE: f32 = 0.5;
const RING_WORDS_SIZE: f32 = 8.5;
const RING_WORDS_ALPHA: f32 = 0.55;
const RING_WORDS_OUT: f32 = 6.0;
const RING_WORDS_DOWN: f32 = 4.0;
const RING_WORDS_EDGE: f32 = 6.0;

const HALO: f32 = 44.0;
const HALO_DEPTH: f32 = 30.0;
const HALO_FLASH: f32 = 16.0;
const HALO_ALPHA: (f32, f32) = (0.5, 0.4);
const PALE_AT: f32 = 0.35;
const LIGHT: Vec2 = Vec2::new(-0.3, -0.4);
const LIGHT_REACH: f32 = 1.4;
const WEIGHT_AT: f32 = 4.0;
const WEIGHT_LIGHT: f32 = 0.2;
const WEIGHT_MOST: f32 = 2.0;
const LIT_RING: f32 = 3.0;
const BODY_RINGS: usize = 3;

const SUB_SIZE: f32 = p::READOUT_SIZE;
const REST_SIZE: f32 = 8.5;
const HERO_QUIET: f32 = 0.35;
const HERO_LOUD: f32 = 0.65;
const HERO_FULL_DB: f32 = 1.5;
const SUB_QUIET: f32 = 0.5;
const REST_ALPHA: f32 = 0.6;

fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

fn pressed(height: f32) -> f32 {
    (height.abs() * PRESSED_PER_UNIT).min(1.0)
}

pub(super) fn glow_in_well(scene: &Scene<'_>, frame: &Frame, well: &Well, awake: f32) {
    let centre = frame.at(middle(), well.focus_down() + GLOW_DOWN);
    let share = motion::GLOW_REST + GLOW_AWAKE * awake + GLOW_DEPTH * well.depth();
    glow_ladder(
        scene.painter,
        centre,
        GLOW_REACH * frame.s(),
        scene.ink(scene.hue),
        share,
        false,
    );
}

pub(super) fn membrane(scene: &Scene<'_>, frame: &Frame, membrane: &Membrane) {
    let ink = scene.ink(scene.hue);
    let rows = rows();
    let mut mesh = mesh_for(
        ROWS + COLUMNS,
        (COLUMNS * SMOOTH_STEPS).max(ROWS * SMOOTH_STEPS),
    );
    let mut places = [[Pos2::ZERO; COLUMNS]; ROWS];
    let mut row_press = [0.0_f32; ROWS];
    let mut column_press = [0.0_f32; COLUMNS];
    for (j, row) in rows.iter().enumerate() {
        for i in 0..COLUMNS {
            let height = membrane.height(i, j);
            places[j][i] = frame.grid(i, row, height);
            let hold = pressed(height);
            row_press[j] = row_press[j].max(hold);
            column_press[i] = column_press[i].max(hold);
        }
    }
    for (j, line) in places.iter().enumerate() {
        let t = j as f32 / (ROWS - 1) as f32;
        let alpha = ROW_ALPHA.0 + ROW_ALPHA.1 * t + PRESSED_ALPHA * row_press[j];
        let path = paint::smooth_path(line, SMOOTH_STEPS);
        paint::line(
            &mut mesh,
            path.len(),
            |k| path[k],
            LINE_WIDTH,
            ink.gamma_multiply(alpha.min(1.0)),
        );
    }
    for (i, press) in column_press.iter().enumerate() {
        let mut line = [Pos2::ZERO; ROWS];
        for (j, point) in line.iter_mut().enumerate() {
            *point = places[j][i];
        }
        let alpha = COLUMN_ALPHA + PRESSED_ALPHA * press;
        let path = paint::smooth_path(&line, SMOOTH_STEPS);
        paint::line(
            &mut mesh,
            path.len(),
            |k| path[k],
            LINE_WIDTH,
            ink.gamma_multiply(alpha.min(1.0)),
        );
    }
    scene.painter.add(Shape::mesh(mesh));
}

pub(super) fn threshold_ring(scene: &Scene<'_>, threshold_db: f32, well: &Well, lit: f32) {
    let (at, radii) = ring(scene.rect, threshold_db);
    let frame = Frame::of(scene.rect);
    let strength = (well.flash() + well.depth() * RING_DEPTH_SHARE).clamp(0.0, 1.0);
    let alpha = RING_ALPHA.0 + RING_ALPHA.1 * strength;
    let alpha = alpha + (RING_LIT_ALPHA - alpha).max(0.0) * lit;
    let width = RING_WIDTH.0 + (RING_WIDTH.1 - RING_WIDTH.0) * lit;
    let dash = frame.s().max(0.6);
    dashed_ellipse(
        scene.painter,
        at,
        radii,
        Stroke::new(width, scene.ink(p::GRAVITY_ORB[1]).gamma_multiply(alpha)),
        (RING_DASH.0 * dash, RING_DASH.1 * dash),
    );
}

pub(super) fn ring_words(scene: &Scene<'_>, threshold_db: f32, words: &str) {
    let (at, radii) = ring(scene.rect, threshold_db);
    let font = mono(RING_WORDS_SIZE);
    let wide = scene
        .painter
        .layout_no_wrap(words.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let from = (at.x + radii.x + RING_WORDS_OUT).min(scene.rect.right() - wide - RING_WORDS_EDGE);
    scene.painter.text(
        Pos2::new(
            from,
            at.y + RING_WORDS_DOWN + RING_WORDS_SIZE * BASELINE_DROP,
        ),
        Align2::LEFT_BOTTOM,
        words,
        font,
        scene
            .ink(p::GRAVITY_ORB[1])
            .gamma_multiply(RING_WORDS_ALPHA * scene.settle),
    );
}

pub(super) fn weight(ratio: f32) -> f32 {
    1.0 - WEIGHT_LIGHT + WEIGHT_LIGHT * (ratio / WEIGHT_AT).min(WEIGHT_MOST)
}

pub(super) fn body(scene: &Scene<'_>, frame: &Frame, well: &Well, ratio: f32, lit: f32) {
    let sb = frame.body_scale();
    let centre = frame.at(middle(), well.focus_down() - BODY_UP + well.lift());
    let radius = BODY * sb * weight(ratio);
    let hue = scene.ink(scene.hue);
    glow_ladder(
        scene.painter,
        centre,
        (HALO + HALO_DEPTH * well.depth() + HALO_FLASH * well.flash()) * sb,
        hue,
        (HALO_ALPHA.0 + HALO_ALPHA.1 * well.flash()).min(1.0),
        false,
    );
    let tall = 1.0 + well.squash();
    let squash = vec2(1.0 / tall, tall);
    let light = centre + LIGHT * radius;
    let (white, pale) = (scene.ink(p::GRAVITY_ORB[0]), scene.ink(p::GRAVITY_ORB[1]));
    let mut ball = Mesh::default();
    paint::disc(&mut ball, centre, radius, BODY_RINGS, |point| {
        let t = ((point - light).length() / (LIGHT_REACH * radius)).clamp(0.0, 1.0);
        if t < PALE_AT {
            white.lerp_to_gamma(pale, t / PALE_AT)
        } else {
            pale.lerp_to_gamma(hue, (t - PALE_AT) / (1.0 - PALE_AT))
        }
    });
    for vertex in &mut ball.vertices {
        vertex.pos = centre + (vertex.pos - centre) * squash;
    }
    scene.painter.add(Shape::mesh(ball));
    if lit > 0.0 {
        scene.painter.circle_stroke(
            centre,
            radius * tall.max(1.0 / tall) + LIT_RING,
            Stroke::new(theme::stroke::HAIRLINE, hue.gamma_multiply(lit)),
        );
    }
}

pub(super) fn readout(scene: &Scene<'_>, well: &Well, awake: f32, at: &Readout, hold_words: bool) {
    let rest = 1.0 - awake;
    let pulled = well.pulled();
    let clear = meter::is_none(pulled);
    let hero = if clear {
        "0.0".to_owned()
    } else {
        format!("{MINUS}{pulled:.1}")
    };
    let loud = HERO_QUIET + HERO_LOUD * awake * (pulled / HERO_FULL_DB).clamp(0.0, 1.0);
    scene.painter.text(
        pos(at.hero, at.hero_size),
        Align2::RIGHT_BOTTOM,
        hero,
        fonts::display(at.hero_size),
        scene
            .ink(Color32::WHITE)
            .gamma_multiply(loud * scene.settle),
    );
    scene.painter.text(
        pos(at.sub, SUB_SIZE),
        Align2::RIGHT_BOTTOM,
        if clear {
            "dB \u{b7} at rest"
        } else {
            "dB pulled"
        },
        mono(SUB_SIZE),
        scene
            .ink(scene.hue)
            .gamma_multiply((SUB_QUIET + (1.0 - SUB_QUIET) * awake) * scene.settle),
    );
    if hold_words && rest > 0.0 {
        scene.painter.text(
            pos(at.rest, REST_SIZE),
            Align2::RIGHT_BOTTOM,
            "nothing to pull",
            mono(REST_SIZE),
            scene
                .ink(p::GRAVITY_ORB[1])
                .gamma_multiply(rest * REST_ALPHA * scene.settle),
        );
    }
}

fn pos(at: Pos2, size: f32) -> Pos2 {
    Pos2::new(at.x, at.y + size * BASELINE_DROP)
}
