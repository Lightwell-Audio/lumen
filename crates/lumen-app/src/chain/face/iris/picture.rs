use eframe::egui::{Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use lumen_devices::eq::{self, Sample};

use super::bands::{Band, colour, sweep_at};
use super::plot::Plot;
use super::response::{FILL_STEP, Response, Shape as Own, THIN, thin};
use crate::chain::face::paint::{self, glow_into, line_with};
use crate::chain::face::{MAX_PARTS, Scene};
use crate::theme::personality as p;
use crate::theme::personality::card;

const LABEL_SIZE: f32 = 8.0;
const LABEL_ALPHA: f32 = 0.34;
const UNIT_ALPHA: f32 = 0.22;
const REST_ALPHA: f32 = 0.45;
const REST_TOP: f32 = 10.0;
const REST_INSET: f32 = 4.0;
const LABEL_GAP: f32 = 5.0;
const FOOT_WORD: f32 = 5.0;
const UNIT_ROOM: f32 = 34.0;

const GRID_HZ: [f32; 3] = [100.0, 1_000.0, 10_000.0];
const GRID_DB: [f32; 5] = [-12.0, -6.0, 0.0, 6.0, 12.0];
const GRID_MAJOR: f32 = 0.10;
const GRID_MINOR: f32 = 0.04;
const GRID_DB_ALPHA: f32 = 0.06;
const GRID_ZERO: f32 = 0.18;
const DECADES: [f32; 4] = [10.0, 100.0, 1_000.0, 10_000.0];

const MIST_FLOOR_DB: f32 = -84.0;
const MIST_CEIL_DB: f32 = -6.0;
const MIST_REACH: f32 = 0.66;
const MIST_STEP: f32 = 1.4;
const MIST_FILL: (f32, f32) = (0.34, 0.02);
const MIST_LINE: f32 = 0.55;
const MIST_LINE_WIDTH: f32 = 1.0;

const FILL: f32 = 0.09;
const FILL_PICKED: f32 = 0.20;

const GLOW_WIDTH: f32 = 6.0;
const GLOW_ALPHA: f32 = 0.22;
const CORE_WIDTH: f32 = 2.1;
const CORE_THIN: f32 = 1.4;

const NODE: f32 = 5.0;
const NODE_PICKED: f32 = 6.8;
const NODE_RING: f32 = 2.0;
const NODE_CORE: f32 = 2.2;
const NODE_OFF: f32 = 4.0;
const NODE_OFF_ALPHA: f32 = 0.4;
const NODE_GLOW: f32 = 22.0;
const NODE_GLOW_HOVER: f32 = 0.30;
const NODE_GLOW_PICKED: f32 = 0.50;
const NODE_GLOW_PICKED_LIT: f32 = 0.20;
const DOT: f32 = 2.5;
const DOT_PICKED: f32 = 3.5;

const STEM: f32 = 1.0;
const STEM_ALPHA: f32 = 0.28;
const STEM_PICKED_ALPHA: f32 = 0.55;
const BRACKET_ALPHA: f32 = 0.85;
const BRACKET_TICK: f32 = 4.0;

const CHIP_PAD: f32 = 10.0;
const CHIP_LIFT: f32 = 22.0;
const CHIP_TOP_ROOM: f32 = 8.0;
const CHIP_EDGE: f32 = 2.0;

fn pixel(painter: &Painter, at: f32) -> f32 {
    let ppp = painter.ctx().pixels_per_point();
    (at * ppp).round() / ppp
}

pub(super) fn draw(scene: &Scene<'_>) {
    let plot = Plot::of(scene.rect);
    paint::wash(scene, p::EQ_DEEP, paint::corners(scene.step));
    let values = &scene.values;
    let bands: [Band; eq::BANDS] = std::array::from_fn(|index| super::bands::band(values, index));
    let response = super::response::response(values);
    let picked = scene.picked.min(eq::BANDS - 1);
    if plot.labelled {
        grid(scene, &plot);
    }
    analyser(scene, &plot);
    fills(scene, &plot, &bands, picked);
    curve(scene, &plot, &response);
    nodes(scene, &plot, &response, &bands, picked);
    if plot.chip {
        chip(scene, &plot, &response, &bands[picked], picked);
    }
    if plot.labelled {
        rest_words(scene, &plot);
    }
}

fn grid(scene: &Scene<'_>, plot: &Plot) {
    let painter = scene.painter;
    let white = scene.ink(Color32::WHITE);
    let area = plot.area;
    let thick = crate::theme::stroke::HAIRLINE;
    let mut mesh = Mesh::default();
    for decade in DECADES {
        for multiple in 1..=9 {
            let hz = multiple as f32 * decade;
            if !(eq::LOW_HZ..=eq::HIGH_HZ).contains(&hz) {
                continue;
            }
            let alpha = if multiple == 1 {
                GRID_MAJOR
            } else {
                GRID_MINOR
            };
            let x = pixel(painter, plot.x_of(hz));
            mesh.add_colored_rect(
                Rect::from_min_max(pos2(x, area.top()), pos2(x + thick, area.bottom())),
                white.gamma_multiply(alpha),
            );
        }
    }
    for db in GRID_DB {
        let y = pixel(painter, plot.y_of(db));
        let alpha = if db == 0.0 { GRID_ZERO } else { GRID_DB_ALPHA };
        mesh.add_colored_rect(
            Rect::from_min_max(pos2(area.left(), y), pos2(area.right(), y + thick)),
            white.gamma_multiply(alpha),
        );
    }
    painter.add(Shape::mesh(mesh));
    let font = FontId::monospace(LABEL_SIZE);
    for db in GRID_DB {
        painter.text(
            pos2(area.left() - LABEL_GAP, plot.y_of(db)),
            Align2::RIGHT_CENTER,
            db_word(db),
            font.clone(),
            white.gamma_multiply(LABEL_ALPHA),
        );
    }
    let foot = scene.rect.bottom() - FOOT_WORD;
    for hz in GRID_HZ {
        painter.text(
            pos2(plot.x_of(hz), foot),
            Align2::CENTER_BOTTOM,
            hz_word(hz),
            font.clone(),
            white.gamma_multiply(LABEL_ALPHA),
        );
    }
    if area.right() - plot.x_of(10_000.0) >= UNIT_ROOM {
        painter.text(
            pos2(area.right(), foot),
            Align2::RIGHT_BOTTOM,
            "Hz",
            font,
            white.gamma_multiply(UNIT_ALPHA),
        );
    }
}

pub(super) fn db_word(db: f32) -> String {
    if db > 0.0 {
        format!("+{db:.0}")
    } else if db < 0.0 {
        format!("\u{2212}{:.0}", -db)
    } else {
        "0".to_owned()
    }
}

pub(super) fn hz_word(hz: f32) -> String {
    if hz >= 1_000.0 {
        format!("{}k", hz / 1_000.0)
    } else {
        format!("{hz}")
    }
}

pub(super) fn mist_points(plot: &Plot, spectrum: &[f32]) -> Vec<Pos2> {
    let n = spectrum.len();
    let step = ((n as f32 * MIST_STEP / plot.area.width().max(1.0)).round() as usize).max(1);
    let foot = plot.area.bottom();
    let reach = plot.area.height() * MIST_REACH;
    let at = |i: usize| {
        let loud = ((spectrum[i] - MIST_FLOOR_DB) / (MIST_CEIL_DB - MIST_FLOOR_DB)).clamp(0.0, 1.0);
        pos2(
            plot.x_of_share(i as f32 / (n - 1) as f32),
            foot - reach * loud,
        )
    };
    let mut points: Vec<Pos2> = (0..n).step_by(step).map(at).collect();
    if !(n - 1).is_multiple_of(step) {
        points.push(at(n - 1));
    }
    points
}

fn analyser(scene: &Scene<'_>, plot: &Plot) {
    let Some(spectrum) = scene.spectrum.filter(|s| s.len() >= 2) else {
        return;
    };
    if scene.mist <= 0.0 {
        return;
    }
    let points = mist_points(plot, spectrum);
    let (fill, line) = (scene.ink(p::EQ_MIST[0]), scene.ink(p::EQ_MIST[1]));
    let foot = plot.area.bottom();
    let reach = plot.area.height() * MIST_REACH;
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(points.len() * 2);
    mesh.reserve_triangles(points.len() * 2);
    for point in &points {
        let height = ((foot - point.y) / reach.max(1.0)).clamp(0.0, 1.0);
        let alpha = MIST_FILL.1 + (MIST_FILL.0 - MIST_FILL.1) * height;
        let first = mesh.vertices.len() as u32;
        mesh.colored_vertex(*point, fill.gamma_multiply(alpha * scene.mist));
        mesh.colored_vertex(
            pos2(point.x, foot),
            fill.gamma_multiply(MIST_FILL.1 * scene.mist),
        );
        if let Some(back) = first.checked_sub(2) {
            mesh.add_triangle(back, back + 1, first);
            mesh.add_triangle(back + 1, first + 1, first);
        }
    }
    line_with(
        &mut mesh,
        points.len(),
        |i| points[i],
        MIST_LINE_WIDTH,
        |_| line.gamma_multiply(MIST_LINE * scene.mist),
    );
    scene.painter.add(Shape::mesh(mesh));
}

pub(super) fn fill_points(plot: &Plot, own: &Own, node_hz: f32) -> Vec<Pos2> {
    let steps = (plot.area.width() / FILL_STEP).ceil().max(1.0) as usize;
    let mut points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let share = i as f32 / steps as f32;
            let hz = eq::axis_hz(share);
            pos2(plot.x_of_share(share), plot.y_of(own.db_at_hz(hz)))
        })
        .collect();
    let node = pos2(plot.x_of(node_hz), plot.y_of(own.db_at_hz(node_hz)));
    let at = points.partition_point(|point| point.x < node.x);
    points.insert(at, node);
    thin(&points, THIN)
}

fn fills(scene: &Scene<'_>, plot: &Plot, bands: &[Band; eq::BANDS], picked: usize) {
    let mut mesh = Mesh::default();
    let zero = plot.zero();
    for (index, band) in bands.iter().enumerate().filter(|(_, band)| band.on) {
        let own = Own::of(&band.setting(), scene.values.rate);
        let points = fill_points(plot, &own, band.freq);
        let ink = scene.ink(colour(index)).gamma_multiply(if index == picked {
            FILL_PICKED
        } else {
            FILL
        });
        for point in &points {
            let first = mesh.vertices.len() as u32;
            mesh.colored_vertex(*point, ink);
            mesh.colored_vertex(pos2(point.x, zero), ink);
            if let Some(back) = first.checked_sub(2) {
                mesh.add_triangle(back, back + 1, first);
                mesh.add_triangle(back + 1, first + 1, first);
            }
        }
    }
    scene.painter.add(Shape::mesh(mesh));
}

fn curve(scene: &Scene<'_>, plot: &Plot, response: &Response) {
    let ppp = scene.painter.ctx().pixels_per_point();
    let points = curve_line(plot, response, ppp);
    let sweep = |i: usize| scene.ink(sweep_at(plot.share_of_x(points[i].x)));
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(points.len() * 8);
    mesh.reserve_triangles(points.len() * 12);
    let bare = !plot.labelled;
    if !bare {
        line_with(
            &mut mesh,
            points.len(),
            |i| points[i],
            GLOW_WIDTH,
            |i| sweep(i).gamma_multiply(GLOW_ALPHA),
        );
    }
    line_with(
        &mut mesh,
        points.len(),
        |i| points[i],
        if bare { CORE_THIN } else { CORE_WIDTH },
        sweep,
    );
    scene.painter.add(Shape::mesh(mesh));
}

pub(super) fn curve_line(plot: &Plot, response: &Response, ppp: f32) -> Vec<Pos2> {
    let mut path = [Sample::default(); eq::curve::MOST];
    let count = response.path(plot.area.width() * ppp, &mut path);
    let points: Vec<Pos2> = path[..count]
        .iter()
        .map(|sample| pos2(plot.x_of_share(sample.share), plot.y_of(sample.db)))
        .collect();
    thin(&points, THIN)
}

pub(super) fn node_centre(plot: &Plot, response: &Response, band: &Band) -> Pos2 {
    plot.place(band.freq, response.db_at_hz(band.freq))
}

fn nodes(
    scene: &Scene<'_>,
    plot: &Plot,
    response: &Response,
    bands: &[Band; eq::BANDS],
    picked: usize,
) {
    let order: Vec<usize> = (0..eq::BANDS)
        .filter(|index| *index != picked)
        .chain(std::iter::once(picked))
        .collect();
    let centres: [Pos2; eq::BANDS] =
        std::array::from_fn(|index| node_centre(plot, response, &bands[index]));
    let painter = scene.painter;
    let mut under = Mesh::default();
    for index in order.iter().copied().filter(|i| bands[*i].on) {
        let lit = scene.parts_lit[index.min(MAX_PARTS - 1)];
        let share = if index == picked {
            NODE_GLOW_PICKED + NODE_GLOW_PICKED_LIT * lit
        } else {
            NODE_GLOW_HOVER * lit
        };
        glow_into(
            &mut under,
            centres[index],
            NODE_GLOW,
            scene.ink(colour(index)),
            share,
            false,
        );
        if plot.labelled {
            stem(
                scene,
                plot,
                &mut under,
                &bands[index],
                centres[index],
                index == picked,
            );
        }
    }
    painter.add(Shape::mesh(under));
    if plot.labelled {
        bracket(scene, plot, &bands[picked], centres[picked], picked);
    }
    for index in order {
        node(
            scene,
            plot,
            &bands[index],
            centres[index],
            index,
            index == picked,
        );
    }
}

pub(super) fn stem_rect(plot: &Plot, centre: Pos2, x: f32) -> Rect {
    let zero = plot.zero();
    Rect::from_min_max(
        pos2(x, centre.y.min(zero)),
        pos2(x + STEM, centre.y.max(zero)),
    )
}

fn stem(
    scene: &Scene<'_>,
    plot: &Plot,
    mesh: &mut Mesh,
    band: &Band,
    centre: Pos2,
    is_picked: bool,
) {
    if !band.has_stem() {
        return;
    }
    let ink = scene
        .ink(colour(band.number - 1))
        .gamma_multiply(if is_picked {
            STEM_PICKED_ALPHA
        } else {
            STEM_ALPHA
        });
    let x = centre.x - STEM / 2.0;
    mesh.add_colored_rect(stem_rect(plot, centre, x), ink);
}

pub(super) fn bracket_ends(plot: &Plot, band: &Band) -> Option<(f32, f32)> {
    let (low, high) = band.half_width_hz()?;
    Some((
        plot.x_of(low.max(eq::LOW_HZ)),
        plot.x_of(high.min(eq::HIGH_HZ)),
    ))
}

fn bracket(scene: &Scene<'_>, plot: &Plot, band: &Band, centre: Pos2, picked: usize) {
    if !band.on {
        return;
    }
    let Some((left, right)) = bracket_ends(plot, band) else {
        return;
    };
    let ink = scene.ink(colour(picked)).gamma_multiply(BRACKET_ALPHA);
    let stroke = Stroke::new(crate::theme::stroke::RING, ink);
    let painter = scene.painter;
    painter.line_segment([pos2(left, centre.y), pos2(right, centre.y)], stroke);
    for x in [left, right] {
        painter.line_segment(
            [
                pos2(x, centre.y - BRACKET_TICK),
                pos2(x, centre.y + BRACKET_TICK),
            ],
            stroke,
        );
    }
}

fn node(scene: &Scene<'_>, plot: &Plot, band: &Band, centre: Pos2, index: usize, is_picked: bool) {
    let painter = scene.painter;
    let ink = scene.ink(colour(index));
    if !plot.labelled {
        let radius = if is_picked { DOT_PICKED } else { DOT };
        if band.on {
            painter.circle_filled(centre, radius, ink);
        } else {
            painter.circle_stroke(
                centre,
                radius,
                Stroke::new(
                    crate::theme::stroke::HAIRLINE,
                    ink.gamma_multiply(NODE_OFF_ALPHA),
                ),
            );
        }
        return;
    }
    if !band.on {
        painter.circle_stroke(
            centre,
            NODE_OFF,
            Stroke::new(NODE_RING, ink.gamma_multiply(NODE_OFF_ALPHA)),
        );
        return;
    }
    let radius = if is_picked { NODE_PICKED } else { NODE };
    painter.circle_filled(
        centre,
        radius,
        if is_picked {
            ink
        } else {
            scene.ink(p::EQ_DEEP)
        },
    );
    painter.circle_stroke(centre, radius, Stroke::new(NODE_RING, ink));
    if is_picked {
        painter.circle_filled(centre, NODE_CORE, scene.ink(Color32::WHITE));
    }
}

pub(super) fn chip_rect(plot: &Plot, rect: Rect, centre: Pos2, width: f32) -> Rect {
    let area = plot.area;
    let half = width / 2.0;
    let x = centre.x.clamp(
        area.left() + half,
        (area.right() - half).max(area.left() + half),
    );
    let above = centre.y - CHIP_LIFT;
    let y = if above < area.top() + CHIP_TOP_ROOM {
        centre.y + CHIP_LIFT
    } else {
        above
    };
    let size = vec2(width, card::CHIP_HEIGHT);
    let at = pos2(x - half, y - size.y / 2.0);
    let rect = rect.shrink(CHIP_EDGE);
    Rect::from_min_size(
        pos2(
            at.x.clamp(rect.left(), (rect.right() - size.x).max(rect.left())),
            at.y.clamp(rect.top(), (rect.bottom() - size.y).max(rect.top())),
        ),
        size,
    )
}

fn chip(scene: &Scene<'_>, plot: &Plot, response: &Response, band: &Band, picked: usize) {
    let painter = scene.painter;
    let font = FontId::monospace(card::CHIP_TEXT);
    let text = scene
        .ink(Color32::WHITE)
        .gamma_multiply(scene.settle.clamp(0.0, 1.0));
    let galley = painter.layout_no_wrap(band.chip(), font, text);
    let width = galley.size().x + 2.0 * CHIP_PAD;
    let centre = node_centre(plot, response, band);
    let chip = chip_rect(plot, scene.rect, centre, width);
    let ink = scene.ink(colour(picked));
    painter.rect_filled(chip, chip.height() / 2.0, scene.ink(p::EQ_DEEP));
    painter.rect_stroke(
        chip,
        chip.height() / 2.0,
        Stroke::new(crate::theme::stroke::HAIRLINE, ink),
        eframe::egui::StrokeKind::Inside,
    );
    painter.galley(
        pos2(
            chip.left() + CHIP_PAD,
            chip.center().y - galley.size().y / 2.0,
        ),
        galley,
        text,
    );
}

fn rest_words(scene: &Scene<'_>, plot: &Plot) {
    let rest = (1.0 - scene.awake).clamp(0.0, 1.0) * (1.0 - scene.mist.clamp(0.0, 1.0));
    if rest <= 0.01 {
        return;
    }
    scene.painter.text(
        pos2(
            plot.area.right() - REST_INSET,
            plot.area.top() - (plot.area.top() - scene.rect.top()) + REST_TOP,
        ),
        Align2::RIGHT_CENTER,
        "no signal \u{00B7} analyser rests",
        FontId::monospace(LABEL_SIZE),
        scene.ink(Color32::WHITE).gamma_multiply(REST_ALPHA * rest),
    );
}
