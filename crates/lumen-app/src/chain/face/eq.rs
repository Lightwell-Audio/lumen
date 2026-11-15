use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use eframe::epaint::Mesh;
use lumen_devices::eq::{self, Kind};

use super::{Aim, Control, Drawing, Parts, Scene, Values};
use crate::chain::geometry::Step;
use crate::theme::personality as p;
use crate::theme::personality::card;

pub(crate) const LOW: f32 = eq::LOW_HZ;
pub(crate) const HIGH: f32 = eq::HIGH_HZ;
const MOST: usize = eq::curve::MOST;

const VIEW_DB: f32 = 16.0;
const OFF_SCREEN: f32 = 40.0;
const SPECIMEN_HEIGHT: f32 = 176.0;

const WHOLE: Vec2 = Vec2::new(380.0, 330.0);
const DIAL_X: [f32; 3] = [52.0, 124.0, 196.0];
const DIALS_Y: f32 = 278.0;
const PILL: f32 = 22.0;
const PILL_X: f32 = 244.0;
const PILL_Y: f32 = 252.0;
const PILL_PITCH: Vec2 = Vec2::new(28.0, 26.0);
const PILLS_ACROSS: usize = 4;

const GRID_HZ: [f32; 3] = [100.0, 1_000.0, 10_000.0];
const GRID_INSET: f32 = 8.0;
const GRID_ALPHA: f32 = 0.06;
const GRID_WORD_ALPHA: f32 = 0.35;
const MIDDLE_ALPHA: f32 = 0.08;

const MIST_FLOOR_DB: f32 = -84.0;
const MIST_CEIL_DB: f32 = -6.0;
const MIST_HEIGHT: f32 = 0.7;
const MIST_ALPHA: f32 = 0.16;
const MIST_FOOT: f32 = 0.5;

const WASH_ALPHA: f32 = 0.18;
const GLOW_HALF: f32 = 6.0;
const GLOW_ALPHA: f32 = 0.42;
const LINE_HALF: f32 = 1.1;
const LINE_FEATHER: f32 = 0.5;

const POINT: f32 = 6.0;
const POINT_PICKED: f32 = 7.5;
const POINT_RING: f32 = 1.0;
const POINT_RING_PICKED: f32 = 2.0;
const POINT_HALO: [(f32, f32); 2] = [(5.0, 0.12), (2.5, 0.26)];
const POINT_GROW: f32 = 1.5;
const POINT_OFF_ALPHA: f32 = 0.4;
const POINT_OFF_SIZE: f32 = 0.8;
const BLADES: usize = 6;
const BLADE_FROM: f32 = 12.0;
const BLADE_TO: f32 = 17.0;
const BLADE_TURN: f32 = 0.7;
const BLADE_START: f32 = 0.3;
const BLADE_WIDTH: f32 = 1.4;
const BLADE_ALPHA: f32 = 0.7;
const DOT: f32 = 2.5;
const DOT_PICKED: f32 = 3.5;

const CHIP_TOP: f32 = 10.0;
const CHIP_PAD: f32 = 10.0;
const ROOMY: f32 = 96.0;
const CHIP_ROOM: f32 = 130.0;
const WORD_EDGE: f32 = 30.0;

const KEY_OCTAVE: f32 = 1.0 / 24.0;
const KEY_DB: f32 = 0.5;
const WHEEL_OCTAVES: f32 = 0.8;
const FINE: f32 = 0.2;
const SLOPE_STEP: f32 = 12.0;
const SLOPE_LEAST: f32 = 12.0;
const SLOPE_MOST: f32 = 48.0;

const READS: [u32; 37] = reads();

const fn reads() -> [u32; 37] {
    let mut out = [0; 37];
    let mut at = 0;
    let mut number = 1;
    while number <= eq::BANDS {
        out[at] = eq::freq_of(number);
        out[at + 1] = eq::gain_of(number);
        out[at + 2] = eq::on_of(number);
        at += 3;
        if number != 1 && number != eq::BANDS {
            out[at] = eq::q_of(number);
            out[at + 1] = eq::type_of(number);
            at += 2;
        }
        number += 1;
    }
    out[at] = eq::OUTPUT;
    out
}

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: SPECIMEN_HEIGHT,
    deep: p::SCREEN,
    reads: &[],
    draw,
    controls: &[
        Control::knob(eq::freq_of(2), "Freq", |c| dial(c, 0)),
        Control::knob(eq::gain_of(2), "Gain", |c| dial(c, 1)).showing(without_db),
        Control::knob(eq::q_of(2), "Q", |c| dial(c, 2)),
    ],
    handles: &[],
};

pub(crate) static PARTS: Parts = Parts {
    count: eq::BANDS,
    reads: &READS,
    first,
    colour,
    is_on,
    aim,
    at,
    drag,
    nudge,
    wheel,
    reset_point,
    reset_all,
    switch,
    words,
    pill_words,
    number,
    pill,
    typed: 0,
};

fn dial(card_rect: Rect, index: usize) -> Rect {
    Rect::from_center_size(
        pos2(card_rect.left() + DIAL_X[index], card_rect.top() + DIALS_Y),
        Vec2::splat(2.0 * card::DIAL_RADIUS + 4.0),
    )
}

fn without_db(text: &str) -> String {
    text.trim_end_matches(" dB").to_owned()
}

fn pill(card_rect: Rect, index: usize) -> Rect {
    let (row, column) = (index / PILLS_ACROSS, index % PILLS_ACROSS);
    Rect::from_min_size(
        pos2(
            card_rect.left() + PILL_X + column as f32 * PILL_PITCH.x,
            card_rect.top() + PILL_Y + row as f32 * PILL_PITCH.y,
        ),
        Vec2::splat(PILL),
    )
}

#[derive(Debug, Clone, Copy)]
struct Band {
    number: usize,
    kind: Kind,
    on: bool,
    freq: f32,
    gain: f32,
    q: f32,
    slope: f32,
}

fn band(values: &Values<'_>, index: usize) -> Band {
    let number = index + 1;
    let kind = if number == 1 {
        Kind::LowCut
    } else if number == eq::BANDS {
        Kind::HighCut
    } else {
        Kind::of_choice(values.plain(eq::type_of(number)).max(0.0).round() as usize)
    };
    Band {
        number,
        kind,
        on: values.norm(eq::on_of(number)) > 0.5,
        freq: values.plain(eq::freq_of(number)),
        gain: if kind.has_gain() {
            values.plain(eq::gain_of(number))
        } else {
            0.0
        },
        q: if kind.has_q() {
            values.plain(eq::q_of(number))
        } else {
            1.0
        },
        slope: if kind.has_slope() {
            values.plain(eq::gain_of(number))
        } else {
            SLOPE_LEAST
        },
    }
}

impl Band {
    fn setting(&self) -> eq::bands::Setting {
        eq::bands::Setting {
            kind: self.kind,
            freq: self.freq,
            gain: self.gain,
            q: self.q,
            slope: self.slope,
        }
    }

    fn words(&self) -> String {
        let mut words = format!("Band {}, {}", self.number, self.kind.word());
        if !self.on {
            words.push_str(", off");
        }
        words.push_str(", ");
        words.push_str(&eq::hz_text(self.freq));
        if self.kind.has_gain() {
            words.push_str(", ");
            words.push_str(&eq::db_text(self.gain));
        }
        if self.kind.has_slope() {
            words.push_str(", ");
            words.push_str(&eq::slope_text(self.slope));
        }
        if self.kind.has_q() {
            words.push_str(&format!(", Q {}", eq::q_text(self.q)));
        }
        words
    }

    fn chip(&self) -> String {
        let mut words = eq::hz_text(self.freq);
        if self.kind.has_gain() {
            words.push(' ');
            words.push_str(&format!("{:+.1}", self.gain));
        } else if self.kind.has_slope() {
            words.push(' ');
            words.push_str(&eq::slope_text(self.slope));
        }
        if self.kind.has_q() && !self.kind.has_gain() {
            words.push_str(&format!(" Q {}", eq::q_text(self.q)));
        }
        words
    }
}

fn first(values: &Values<'_>) -> usize {
    (0..eq::BANDS)
        .find(|index| is_on(values, *index))
        .unwrap_or(0)
}

fn colour(index: usize) -> Color32 {
    p::EQ_BANDS[index.min(p::EQ_BANDS.len() - 1)]
}

fn is_on(values: &Values<'_>, index: usize) -> bool {
    values.norm(eq::on_of(index + 1)) > 0.5
}

fn aim(values: &Values<'_>, index: usize, control: usize) -> Option<Aim> {
    let band = band(values, index);
    let number = band.number;
    match control {
        0 => Some(Aim {
            param: eq::freq_of(number),
            name: "Freq",
        }),
        1 if band.kind.has_gain() => Some(Aim {
            param: eq::gain_of(number),
            name: "Gain",
        }),
        1 if band.kind.has_slope() => Some(Aim {
            param: eq::gain_of(number),
            name: "Slope",
        }),
        2 if band.kind.has_q() => Some(Aim {
            param: eq::q_of(number),
            name: "Q",
        }),
        _ => None,
    }
}

fn place(rect: Rect, hz: f32, db: f32) -> Pos2 {
    pos2(x_of(rect, hz), y_of(rect, db))
}

fn x_of(rect: Rect, hz: f32) -> f32 {
    rect.left() + rect.width() * eq::axis(hz).clamp(0.0, 1.0)
}

fn y_free(rect: Rect, db: f32) -> f32 {
    let half = rect.height() * 0.5;
    (rect.center().y - db / VIEW_DB * half)
        .clamp(rect.top() - OFF_SCREEN, rect.bottom() + OFF_SCREEN)
}

fn y_of(rect: Rect, db: f32) -> f32 {
    let half = rect.height() * 0.5;
    (rect.center().y - db / VIEW_DB * half).clamp(rect.top() + 2.0, rect.bottom() - 2.0)
}

fn at(values: &Values<'_>, index: usize, rect: Rect) -> Option<Pos2> {
    let band = band(values, index);
    Some(place(rect, band.freq, band.gain))
}

fn moved_to(values: &Values<'_>, band: &Band, freq: f32, gain: f32) -> Vec<(u32, f64)> {
    let number = band.number;
    let mut out = vec![(
        eq::freq_of(number),
        values.norm_of(eq::freq_of(number), freq),
    )];
    if band.kind.has_gain() {
        out.push((
            eq::gain_of(number),
            values.norm_of(eq::gain_of(number), gain),
        ));
    }
    if !band.on {
        out.push((eq::on_of(number), 1.0));
    }
    out
}

fn drag(values: &Values<'_>, index: usize, pointer: Pos2, rect: Rect) -> Vec<(u32, f64)> {
    let band = band(values, index);
    let across = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
    let db = (rect.center().y - pointer.y) / (rect.height() * 0.5).max(1.0) * VIEW_DB;
    moved_to(
        values,
        &band,
        eq::axis_hz(across),
        db.clamp(-eq::MOST_DB, eq::MOST_DB),
    )
}

fn nudge(values: &Values<'_>, index: usize, steps: Vec2, fine: bool) -> Vec<(u32, f64)> {
    let band = band(values, index);
    let share = if fine { FINE } else { 1.0 };
    let freq = (band.freq * 2f32.powf(steps.x * KEY_OCTAVE * share)).clamp(LOW, HIGH);
    let gain = (band.gain + steps.y * KEY_DB * share).clamp(-eq::MOST_DB, eq::MOST_DB);
    moved_to(values, &Band { on: true, ..band }, freq, gain)
}

fn wheel(values: &Values<'_>, index: usize, notches: f32, fine: bool) -> Option<(u32, f64)> {
    let band = band(values, index);
    let number = band.number;
    if band.kind.has_q() {
        let share = if fine { FINE } else { 1.0 };
        let q = (band.q * 2f32.powf(WHEEL_OCTAVES * share * notches)).clamp(eq::LOW_Q, eq::HIGH_Q);
        Some((eq::q_of(number), values.norm_of(eq::q_of(number), q)))
    } else if band.kind.has_slope() {
        let slope = (band.slope + SLOPE_STEP * notches.signum()).clamp(SLOPE_LEAST, SLOPE_MOST);
        Some((
            eq::gain_of(number),
            values.norm_of(eq::gain_of(number), slope),
        ))
    } else {
        None
    }
}

fn reset_point(values: &Values<'_>, index: usize) -> Vec<(u32, f64)> {
    let band = band(values, index);
    let id = if band.kind.has_gain() {
        eq::gain_of(band.number)
    } else {
        eq::freq_of(band.number)
    };
    vec![(id, values.default_norm(id))]
}

fn reset_all(values: &Values<'_>, index: usize) -> Vec<(u32, f64)> {
    let band = band(values, index);
    let number = band.number;
    let mut ids = vec![eq::freq_of(number)];
    if band.kind.has_gain() || band.kind.has_slope() {
        ids.push(eq::gain_of(number));
    }
    if band.kind.has_q() {
        ids.push(eq::q_of(number));
    }
    ids.into_iter()
        .map(|id| (id, values.default_norm(id)))
        .collect()
}

fn switch(index: usize) -> u32 {
    eq::on_of(index + 1)
}

fn words(values: &Values<'_>, index: usize) -> String {
    band(values, index).words()
}

fn pill_words(values: &Values<'_>, index: usize) -> String {
    let band = band(values, index);
    format!("Band {} ({})", band.number, band.kind.word())
}

fn number(index: usize) -> String {
    (index + 1).to_string()
}

fn sweep_at(share: f32) -> Color32 {
    let sweep = p::EQ_SWEEP;
    let at = (share.clamp(0.0, 1.0) * (sweep.len() - 1) as f32).min(sweep.len() as f32 - 1.001);
    let low = at.floor() as usize;
    sweep[low].lerp_to_gamma(sweep[low + 1], at - low as f32)
}

fn flat() -> eq::bands::Setting {
    eq::bands::Setting {
        kind: Kind::Bell,
        freq: 1_000.0,
        gain: 0.0,
        q: 1.0,
        slope: SLOPE_LEAST,
    }
}

fn response(scene: &Scene<'_>) -> eq::Curve {
    let values = &scene.values;
    let mut settings = [(flat(), false); eq::BANDS];
    for (index, slot) in settings.iter_mut().enumerate() {
        let band = band(values, index);
        *slot = (band.setting(), band.on);
    }
    eq::Curve::new(settings, values.plain(eq::OUTPUT), scene.rate.max(1))
}

fn ribbon(
    mesh: &mut Mesh,
    line: &[Pos2],
    profile: &[(f32, f32)],
    colour: impl Fn(Pos2) -> Color32,
) {
    let count = line.len();
    let width = profile.len() as u32;
    for (i, point) in line.iter().enumerate() {
        let before = line[i.saturating_sub(1)];
        let after = line[(i + 1).min(count - 1)];
        let along = (after - before).normalized();
        let normal = if along.x.is_finite() && along.length() > 0.0 {
            vec2(-along.y, along.x)
        } else {
            vec2(0.0, 1.0)
        };
        let ink = colour(*point);
        let first = mesh.vertices.len() as u32;
        for (distance, strength) in profile {
            mesh.colored_vertex(*point + normal * *distance, ink.gamma_multiply(*strength));
        }
        if i > 0 {
            let back = first - width;
            for column in 0..width - 1 {
                let (a, b) = (back + column, first + column);
                mesh.add_triangle(a, a + 1, b);
                mesh.add_triangle(a + 1, b + 1, b);
            }
        }
    }
}

fn draw(scene: &Scene<'_>) {
    let rect = scene.rect;
    let band_only = scene.step == Step::Compact;
    let scale = (rect.height() / SPECIMEN_HEIGHT).clamp(0.55, 1.0);
    if !band_only {
        grid(scene, rect);
        mist(scene, rect);
    }
    let mut path = [eq::Sample::default(); MOST];
    let mut line = [Pos2::ZERO; MOST];
    let width = rect.width() * scene.painter.ctx().pixels_per_point();
    let count = response(scene).path(width, &mut path);
    for (i, sample) in path[..count].iter().enumerate() {
        line[i] = pos2(
            rect.left() + rect.width() * sample.share,
            y_free(rect, sample.db),
        );
    }
    curve(scene, rect, &line[..count], scale, band_only);
    let picked = scene.picked.min(eq::BANDS - 1);
    for index in (0..eq::BANDS)
        .filter(|index| *index != picked)
        .chain(std::iter::once(picked))
    {
        point(scene, rect, index, index == picked, scale, band_only);
    }
    if !band_only && rect.height() >= CHIP_ROOM {
        chip(scene, rect, picked);
    }
}

fn grid(scene: &Scene<'_>, rect: Rect) {
    let (painter, white) = (scene.painter, scene.ink(Color32::WHITE));
    let words = rect.height() >= ROOMY;
    for hz in GRID_HZ {
        let x = x_of(rect, hz);
        painter.line_segment(
            [
                pos2(x, rect.top() + GRID_INSET),
                pos2(x, rect.bottom() - GRID_INSET),
            ],
            Stroke::new(
                crate::theme::stroke::HAIRLINE,
                white.gamma_multiply(GRID_ALPHA),
            ),
        );
        if words {
            let label = if hz >= 1_000.0 {
                format!("{}k", hz / 1_000.0)
            } else {
                format!("{hz}")
            };
            let left = rect.right() - x > WORD_EDGE;
            painter.text(
                pos2(if left { x + 3.0 } else { x - 3.0 }, rect.bottom() - 5.0),
                if left {
                    Align2::LEFT_BOTTOM
                } else {
                    Align2::RIGHT_BOTTOM
                },
                label,
                FontId::monospace(crate::theme::size::SCALE),
                white.gamma_multiply(GRID_WORD_ALPHA),
            );
        }
    }
    painter.line_segment(
        [
            pos2(rect.left() + GRID_INSET, rect.center().y),
            pos2(rect.right() - GRID_INSET, rect.center().y),
        ],
        Stroke::new(
            crate::theme::stroke::HAIRLINE,
            white.gamma_multiply(MIDDLE_ALPHA),
        ),
    );
}

fn mist(scene: &Scene<'_>, rect: Rect) {
    let Some(spectrum) = scene.spectrum.filter(|s| s.len() >= 2) else {
        return;
    };
    if scene.mist <= 0.0 {
        return;
    }
    let foot = rect.bottom() - 1.0;
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(spectrum.len() * 2);
    mesh.reserve_triangles(spectrum.len() * 2);
    for (i, db) in spectrum.iter().enumerate() {
        let share = i as f32 / (spectrum.len() - 1) as f32;
        let loud = ((db - MIST_FLOOR_DB) / (MIST_CEIL_DB - MIST_FLOOR_DB)).clamp(0.0, 1.0);
        let top = foot - rect.height() * MIST_HEIGHT * loud;
        let x = rect.left() + rect.width() * share;
        let ink = scene.ink(sweep_at(share));
        let first = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(x, top), ink.gamma_multiply(MIST_ALPHA * scene.mist));
        mesh.colored_vertex(
            pos2(x, foot),
            ink.gamma_multiply(MIST_ALPHA * MIST_FOOT * scene.mist),
        );
        if let Some(back) = first.checked_sub(2) {
            mesh.add_triangle(back, back + 1, first);
            mesh.add_triangle(back + 1, first + 1, first);
        }
    }
    scene.painter.add(Shape::mesh(mesh));
}

fn curve(scene: &Scene<'_>, rect: Rect, line: &[Pos2], scale: f32, thin: bool) {
    let share_of = |point: Pos2| ((point.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
    let sweep = |point: Pos2| scene.ink(sweep_at(share_of(point)));
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(line.len() * 7);
    mesh.reserve_triangles(line.len() * 12);
    if !thin {
        let middle = rect.center().y;
        for point in line {
            let ink = sweep(*point).gamma_multiply(WASH_ALPHA);
            let first = mesh.vertices.len() as u32;
            mesh.colored_vertex(*point, ink);
            mesh.colored_vertex(pos2(point.x, middle), ink);
            if let Some(back) = first.checked_sub(2) {
                mesh.add_triangle(back, back + 1, first);
                mesh.add_triangle(back + 1, first + 1, first);
            }
        }
    }
    let glow = GLOW_HALF * scale;
    ribbon(
        &mut mesh,
        line,
        &[
            (-glow, 0.0),
            (-glow / 2.0, GLOW_ALPHA * 0.4),
            (0.0, GLOW_ALPHA),
            (glow / 2.0, GLOW_ALPHA * 0.4),
            (glow, 0.0),
        ],
        sweep,
    );
    let half = LINE_HALF * if thin { 0.7 } else { 1.0 };
    let edge = half + LINE_FEATHER;
    ribbon(
        &mut mesh,
        line,
        &[(-edge, 0.0), (-half, 1.0), (half, 1.0), (edge, 0.0)],
        sweep,
    );
    scene.painter.add(Shape::mesh(mesh));
}

fn point(scene: &Scene<'_>, rect: Rect, index: usize, picked: bool, scale: f32, dot: bool) {
    let band = band(&scene.values, index);
    let painter: &Painter = scene.painter;
    let centre = place(rect, band.freq, band.gain);
    let lit = scene.parts_lit[index];
    let ink = scene.ink(colour(index));
    let strength = if band.on { 1.0 } else { POINT_OFF_ALPHA };
    let white = scene.ink(Color32::WHITE);
    if dot {
        let radius = if picked { DOT_PICKED } else { DOT };
        if band.on {
            painter.circle_filled(centre, radius, ink);
        } else {
            painter.circle_stroke(
                centre,
                radius,
                Stroke::new(crate::theme::stroke::HAIRLINE, ink.gamma_multiply(strength)),
            );
        }
        return;
    }
    let radius = (if picked { POINT_PICKED } else { POINT } + POINT_GROW * lit)
        * scale.max(0.7)
        * if band.on { 1.0 } else { POINT_OFF_SIZE };
    if band.on {
        for (more, alpha) in POINT_HALO {
            painter.circle_filled(
                centre,
                radius + more * scale,
                ink.gamma_multiply(alpha * (1.0 + lit)),
            );
        }
        painter.circle_filled(centre, radius, ink);
    } else {
        painter.circle_stroke(
            centre,
            radius,
            Stroke::new(crate::theme::stroke::LIT, ink.gamma_multiply(strength)),
        );
    }
    painter.circle_stroke(
        centre,
        radius,
        Stroke::new(
            if picked {
                POINT_RING_PICKED
            } else {
                POINT_RING
            },
            white.gamma_multiply(if band.on { 1.0 } else { strength }),
        ),
    );
    if picked {
        for blade in 0..BLADES {
            let angle = blade as f32 * std::f32::consts::TAU / BLADES as f32 + BLADE_START;
            let (from, to) = (angle, angle + BLADE_TURN);
            painter.line_segment(
                [
                    centre + vec2(from.cos(), from.sin()) * BLADE_FROM * scale,
                    centre + vec2(to.cos(), to.sin()) * BLADE_TO * scale,
                ],
                Stroke::new(BLADE_WIDTH, white.gamma_multiply(BLADE_ALPHA)),
            );
        }
    }
}

fn chip(scene: &Scene<'_>, rect: Rect, picked: usize) {
    let band = band(&scene.values, picked);
    let painter = scene.painter;
    let font = FontId::monospace(card::CHIP_TEXT);
    let words = band.chip();
    let text = scene
        .ink(Color32::WHITE)
        .gamma_multiply(scene.settle.clamp(0.0, 1.0));
    let galley = painter.layout_no_wrap(words, font, text);
    let size = vec2(galley.size().x + 2.0 * CHIP_PAD, card::CHIP_HEIGHT);
    let x = x_of(rect, band.freq);
    let left = (x - size.x / 2.0).clamp(
        rect.left() + 2.0,
        (rect.right() - size.x - 2.0).max(rect.left() + 2.0),
    );
    let chip = Rect::from_min_size(pos2(left, rect.top() + CHIP_TOP), size);
    let ink = scene.ink(colour(picked));
    painter.rect_filled(chip, size.y / 2.0, scene.ink(p::SCREEN));
    painter.rect_stroke(
        chip,
        size.y / 2.0,
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

#[cfg(test)]
mod tests;
