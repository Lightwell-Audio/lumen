use eframe::egui::{
    Align2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Shape, Stroke, pos2, vec2,
};

use super::super::paint::{self, dashed_line, mesh_for};
use super::super::{Scene, meter};
use super::geometry::{self, GRID_DB, Readout};
use super::history::{Heard, Skyline};
use crate::chain::geometry::Step;
use crate::fonts;
use crate::motion;
use crate::theme;
use crate::theme::glow as ladder;
use crate::theme::personality as p;

const MINUS: char = '\u{2212}';
const BASELINE_DROP: f32 = 0.22;

const WALL_ALPHA: f32 = 0.30;
const DUSK_DEPTH: f32 = 78.0;
const DUSK_FLASH: f32 = 0.30;
const DUSK_AWAKE: f32 = 0.10;

const GRID_ALPHA: f32 = 0.06;
const GRID_WORDS_ALPHA: f32 = 0.32;
const GRID_WORDS_SIZE: f32 = 8.0;
const GRID_WORDS_INSET: f32 = 6.0;
const GRID_WORDS_UP: f32 = 3.0;

const CEILING_WIDTH: f32 = 1.5;
const CEILING_LIT: f32 = 1.5;
const CEILING_WHITE: f32 = 0.8;
const SHADOW_PEAK: f32 = 1.2;
const SHADOW_REST: f32 = 4.0;
const SHADOW_FLASH: f32 = 12.0;
const SHADOW_LIT: f32 = 0.25;
const WORDS_SIZE: f32 = 8.0;
const WORDS_INSET: f32 = 8.0;
const WORDS_UP: f32 = 6.0;
const WORDS_DOWN: f32 = 4.0;
const WORDS_ALPHA: f32 = 0.85;

const OUTLINE: f32 = 1.5;
const AREA_ALPHA: [f32; 2] = [0.62, 0.03];
const SMOOTH_STEPS: usize = 2;
const IN_OVER: f32 = 4.0;
const OUT_UNDER: f32 = 2.0;
const GAP_REACH: f32 = 4.0;
const GAP_LEAST: f32 = 0.1;

const PLATE_RADIUS: f32 = 10.0;
const WORDS_BACKING: (f32, f32) = (4.0, 1.5);
const WORDS_BACKING_RADIUS: f32 = 3.0;
const WORDS_BACKING_ALPHA: f32 = 0.82;
const HERO_QUIET: f32 = 0.45;
const HERO_FULL_DB: f32 = 1.2;
const SUB_SIZE: f32 = p::READOUT_SIZE;
const HERO_WORDS_ALPHA: f32 = 0.35;

const REST_DASH: (f32, f32) = (2.0, 5.0);
const REST_DASH_ALPHA: f32 = 0.45;
const REST_DASH_UP: f32 = 12.0;
const REST_DASH_INSET: f32 = 8.0;
const REST_WORDS_AT: f32 = 0.62;
const REST_WORDS_ALPHA: f32 = 0.7;
const REST_WORDS_SIZE: f32 = 9.0;

const BAR_ALPHA: f32 = 0.85;

fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

fn band(scene: &Scene<'_>, x: (f32, f32), y: (f32, f32), colours: (Color32, Color32)) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(pos2(x.0, y.0), colours.0);
    mesh.colored_vertex(pos2(x.1, y.0), colours.0);
    mesh.colored_vertex(pos2(x.0, y.1), colours.1);
    mesh.colored_vertex(pos2(x.1, y.1), colours.1);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    scene.painter.add(Shape::mesh(mesh));
}

pub(super) fn backdrop(scene: &Scene<'_>, y: f32, flash: f32, awake: f32, (top, foot): (f32, f32)) {
    let rect = scene.rect;
    paint::wash(scene, p::HORIZON_DEEP, (top, foot));
    let wall = Rect::from_min_max(
        rect.min,
        pos2(rect.right(), y.clamp(rect.top(), rect.bottom())),
    );
    scene.painter.rect_filled(
        wall,
        CornerRadius {
            nw: top as u8,
            ne: top as u8,
            sw: 0,
            se: 0,
        },
        Color32::BLACK.gamma_multiply(WALL_ALPHA),
    );
    let share = motion::GLOW_REST + DUSK_FLASH * flash + DUSK_AWAKE * awake;
    let hue = scene.ink(scene.hue);
    band(
        scene,
        (rect.left(), rect.right()),
        (y, y + DUSK_DEPTH * geometry::depth(rect)),
        (hue.gamma_multiply(share.min(1.0)), Color32::TRANSPARENT),
    );
}

pub(super) fn grid(scene: &Scene<'_>, words: bool) {
    let rect = scene.rect;
    let white = scene.ink(Color32::WHITE);
    for db in GRID_DB {
        let y = geometry::y_of(db, rect);
        if y > rect.bottom() {
            continue;
        }
        scene.painter.rect_filled(
            Rect::from_x_y_ranges(rect.x_range(), y..=y + theme::stroke::HAIRLINE),
            0,
            white.gamma_multiply(GRID_ALPHA),
        );
        if words {
            scene.painter.text(
                pos2(
                    rect.left() + GRID_WORDS_INSET,
                    y - GRID_WORDS_UP + GRID_WORDS_SIZE * BASELINE_DROP,
                ),
                Align2::LEFT_BOTTOM,
                format!("{MINUS}{}", -db as i32),
                mono(GRID_WORDS_SIZE),
                white.gamma_multiply(GRID_WORDS_ALPHA),
            );
        }
    }
}

fn gap_colour(scene: &Scene<'_>, y: f32, ceiling_y: f32, alpha: f32) -> Color32 {
    let rect = scene.rect;
    let t = ((y - rect.top()) / (ceiling_y + GAP_REACH - rect.top())).clamp(0.0, 1.0);
    let top = scene
        .ink(p::HORIZON_GAP[0])
        .gamma_multiply(p::HORIZON_GAP_ALPHA[0]);
    let foot = scene
        .ink(p::HORIZON_GAP[1])
        .gamma_multiply(p::HORIZON_GAP_ALPHA[1]);
    top.lerp_to_gamma(foot, t).gamma_multiply(alpha)
}

fn area_colour(scene: &Scene<'_>, y: f32, ceiling_y: f32, alpha: f32) -> Color32 {
    let rect = scene.rect;
    let t = ((y - ceiling_y) / (rect.bottom() - ceiling_y).max(1.0)).clamp(0.0, 1.0);
    let share = AREA_ALPHA[0] + (AREA_ALPHA[1] - AREA_ALPHA[0]) * t;
    scene.ink(scene.hue).gamma_multiply(share * alpha)
}

pub(super) fn trace(scene: &Scene<'_>, sky: &Skyline, ceiling_y: f32, alpha: f32, foot: f32) {
    let (input, output) = (sky.input(), sky.output());
    let count = output.len();
    if count < 3 || alpha <= 0.01 {
        return;
    }
    let rect = scene.rect;
    let shift = sky.shift();
    let x = |i: usize| geometry::column_x(rect, count, i, shift);
    let y_out = |i: usize| geometry::y_of(output[i], rect).min(rect.bottom() + OUT_UNDER);
    let y_in = |i: usize| {
        geometry::y_of(input[i].max(output[i]), rect)
            .clamp(rect.top() - IN_OVER, rect.bottom() + IN_OVER)
            .min(y_out(i))
    };

    if (0..count).any(|i| y_out(i) - y_in(i) > GAP_LEAST) {
        let mut gap = Mesh::default();
        gap.vertices.reserve(2 * count);
        gap.indices.reserve(6 * count);
        for i in 0..count {
            let (a, b) = (y_in(i), y_out(i));
            gap.colored_vertex(pos2(x(i), a), gap_colour(scene, a, ceiling_y, alpha));
            gap.colored_vertex(pos2(x(i), b), gap_colour(scene, b, ceiling_y, alpha));
            if i > 0 {
                let k = 2 * (i as u32 - 1);
                gap.add_triangle(k, k + 1, k + 2);
                gap.add_triangle(k + 1, k + 3, k + 2);
            }
        }
        scene.painter.add(Shape::mesh(gap));
    }

    let mut points = [Pos2::ZERO; geometry::COLUMNS];
    for (i, point) in points.iter_mut().enumerate().take(count) {
        *point = pos2(x(i), y_out(i));
    }
    let path = paint::smooth_path(&points[..count], SMOOTH_STEPS);
    let radius = foot.min(rect.height() / 2.0);
    let mut area = Mesh::default();
    area.vertices.reserve(2 * path.len());
    area.indices.reserve(6 * path.len());
    for (k, point) in path.iter().enumerate() {
        let floor = rect.bottom() - geometry::foot_drop(point.x, rect, radius);
        area.colored_vertex(*point, area_colour(scene, point.y, ceiling_y, alpha));
        area.colored_vertex(
            pos2(point.x, floor.max(point.y)),
            area_colour(scene, rect.bottom(), ceiling_y, alpha),
        );
        if k > 0 {
            let a = 2 * (k as u32 - 1);
            area.add_triangle(a, a + 1, a + 2);
            area.add_triangle(a + 1, a + 3, a + 2);
        }
    }
    scene.painter.add(Shape::mesh(area));

    let mut outline = mesh_for(1, path.len());
    paint::line(
        &mut outline,
        path.len(),
        |k| path[k],
        OUTLINE,
        scene.ink(p::HORIZON_LINE).gamma_multiply(alpha),
    );
    scene.painter.add(Shape::mesh(outline));
}

pub(super) fn ceiling(scene: &Scene<'_>, y: f32, flash: f32, lit: f32) {
    let rect = scene.rect;
    let hue = scene.ink(scene.hue);
    let blur = SHADOW_REST + SHADOW_FLASH * flash;
    let peak = (SHADOW_PEAK / blur + SHADOW_LIT * lit).min(1.0);
    let rows = ladder::STOPS.len();
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(2 * (2 * rows - 1));
    let mut row = |offset: f32, alpha: f32| {
        let colour = hue.gamma_multiply(peak * alpha);
        mesh.colored_vertex(pos2(rect.left(), y + offset), colour);
        mesh.colored_vertex(pos2(rect.right(), y + offset), colour);
    };
    for k in (1..rows).rev() {
        row(-ladder::STOPS[k] * blur, ladder::ALPHAS[k]);
    }
    for k in 0..rows {
        row(ladder::STOPS[k] * blur, ladder::ALPHAS[k]);
    }
    for k in 0..2 * rows as u32 - 2 {
        let a = 2 * k;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    scene.painter.add(Shape::mesh(mesh));
    let colour = scene.ink(p::HORIZON_LINE).lerp_to_gamma(
        scene.ink(Color32::WHITE),
        (flash * CEILING_WHITE).clamp(0.0, 1.0),
    );
    let half = (CEILING_WIDTH + CEILING_LIT * lit) / 2.0;
    scene.painter.rect_filled(
        Rect::from_x_y_ranges(rect.x_range(), (y - half)..=(y + half)),
        0,
        colour,
    );
}

pub(super) fn ceiling_words(scene: &Scene<'_>, y: f32, db_text: &str) {
    let rect = scene.rect;
    let below = geometry::label_below(y, rect);
    let (at, align) = if below {
        (y + WORDS_DOWN, Align2::LEFT_TOP)
    } else {
        (
            y - WORDS_UP + WORDS_SIZE * BASELINE_DROP,
            Align2::LEFT_BOTTOM,
        )
    };
    let colour = scene
        .ink(p::HORIZON_WORDS)
        .gamma_multiply(WORDS_ALPHA * scene.settle);
    let galley =
        scene
            .painter
            .layout_no_wrap(format!("ceiling {db_text}"), mono(WORDS_SIZE), colour);
    let place = align.anchor_size(pos2(rect.left() + WORDS_INSET, at), galley.size());
    if scene.step == Step::Short {
        scene.painter.rect_filled(
            place.expand2(vec2(WORDS_BACKING.0, WORDS_BACKING.1)),
            WORDS_BACKING_RADIUS,
            scene
                .ink(p::HORIZON_DEEP)
                .gamma_multiply(WORDS_BACKING_ALPHA),
        );
    }
    scene.painter.galley(place.min, galley, colour);
}

pub(super) fn readout(scene: &Scene<'_>, sky: &Skyline, awake: f32, at: &Readout) {
    let rest = 1.0 - awake;
    let pulled = sky.pulled();
    let clear = meter::is_none(pulled);
    scene.painter.rect_filled(
        at.plate,
        PLATE_RADIUS,
        scene
            .ink(p::HORIZON_PLATE)
            .gamma_multiply(p::HORIZON_PLATE_ALPHA),
    );
    let white = scene.ink(Color32::WHITE);
    let hero_font = fonts::display(at.hero_size);
    let hero_at = pos2(at.hero.x, at.hero.y + at.hero_size * BASELINE_DROP);
    let hero = format!("{MINUS}{pulled:.1}");
    let loud = HERO_QUIET + (1.0 - HERO_QUIET) * (pulled / HERO_FULL_DB).clamp(0.0, 1.0);
    scene.painter.text(
        hero_at,
        Align2::RIGHT_BOTTOM,
        hero,
        hero_font.clone(),
        white.gamma_multiply(awake * loud * scene.settle),
    );
    if rest > 0.0 {
        scene.painter.text(
            hero_at,
            Align2::RIGHT_BOTTOM,
            "\u{2014}",
            hero_font,
            white.gamma_multiply(rest * HERO_WORDS_ALPHA),
        );
    }
    scene.painter.text(
        pos2(at.sub.x, at.sub.y + SUB_SIZE * BASELINE_DROP),
        Align2::RIGHT_BOTTOM,
        if clear {
            "dB \u{b7} clear"
        } else {
            "dB limiting"
        },
        mono(SUB_SIZE),
        scene.ink(scene.hue).gamma_multiply(awake * scene.settle),
    );
}

pub(super) fn resting(scene: &Scene<'_>, quiet: f32, waiting: f32, words: bool) {
    let rect = scene.rect;
    let white = scene.ink(Color32::WHITE);
    if quiet > 0.0 && rect.height() > 2.0 * REST_DASH_UP {
        let y = rect.bottom() - REST_DASH_UP;
        dashed_line(
            scene.painter,
            pos2(rect.left() + REST_DASH_INSET, y),
            pos2(rect.right() - REST_DASH_INSET, y),
            Stroke::new(
                theme::stroke::HAIRLINE,
                white.gamma_multiply(quiet * REST_DASH_ALPHA),
            ),
            REST_DASH,
        );
    }
    if words && waiting > 0.0 {
        scene.painter.text(
            pos2(
                rect.center().x,
                rect.top() + rect.height() * REST_WORDS_AT + REST_WORDS_SIZE * BASELINE_DROP,
            ),
            Align2::CENTER_BOTTOM,
            "waiting for signal",
            mono(REST_WORDS_SIZE),
            scene
                .ink(p::HORIZON_WORDS)
                .gamma_multiply(waiting * REST_WORDS_ALPHA),
        );
    }
}

pub(super) fn still_bars(scene: &Scene<'_>, heard: Heard) {
    let (output, pressed) = geometry::bars(scene.rect, heard.input_db, heard.output_db);
    scene.painter.rect_filled(
        output,
        0,
        scene.ink(p::HORIZON_LINE).gamma_multiply(BAR_ALPHA),
    );
    scene.painter.rect_filled(
        pressed,
        0,
        scene.ink(p::HORIZON_GAP[0]).gamma_multiply(BAR_ALPHA),
    );
}
