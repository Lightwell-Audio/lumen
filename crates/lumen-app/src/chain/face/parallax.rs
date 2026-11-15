use eframe::egui::{
    Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2,
};
use lumen_devices::delay as device;

use super::{Aim, Also, Control, Drawing, Handle, Scene, Switch, Values};
use crate::chain::geometry::Step;
use crate::theme::personality as p;
use crate::theme::personality::card;
use crate::theme::stroke;
use crate::{fonts, motion};

const WHOLE: Vec2 = Vec2::new(300.0, 330.0);
const SPECIMEN: Vec2 = Vec2::new(272.0, 176.0);
const DIAL_X: [f32; 3] = [56.0, 124.0, 192.0];
const DIALS_Y: f32 = 278.0;
const PILLS: [(f32, f32); 2] = [(228.0, 286.0), (228.0, 262.0)];
const PILL_SIZE: Vec2 = Vec2::new(58.0, 20.0);

const VANISH: f32 = 28.0 / 176.0;
const SPOKES: usize = 8;
const SPOKE_ALPHA: f32 = 0.12;
const CROSS_LINES: usize = 6;
const CROSS_POWER: f32 = 1.8;
const CROSS_ALPHA: f32 = 0.10;
const NEAREST: f32 = 0.86;
const STEP: f32 = 1.0 / 8.2;
const DEPTH: f32 = 1.6;
const SPECIMEN_ECHOES: f32 = 7.0;
const SPECIMEN_FEEDBACK: f32 = 0.45;
const FEWEST: usize = 2;
const MOST: usize = 12;
const REACH: f32 = 0.8;
const LANE: (f32, f32) = (0.12, 0.40);
const LANE_TIMES: (f32, f32) = (0.03, 4.0);
const RADIUS: (f32, f32, f32) = (13.0, 1.4, 2.0);
const HALOS: [(f32, f32); 4] = [(6.0, 0.07), (3.5, 0.10), (1.0, 0.14), (-1.5, 0.17)];
const HALO_REACH: f32 = 1.6;
const CORE: (f32, f32) = (0.35, 0.65);
const LEAST_SCALE: f32 = 0.3;
const LABEL_INSET: f32 = 10.0;
const LABEL_DOWN: f32 = 15.0;
const HERO_SIZE: f32 = 13.0;
const HERO_SHORT: f32 = 11.0;
const HERO_INSET: f32 = 12.0;
const HERO_BASE: f32 = 9.0;

const PULSE_WIDTH: f32 = 0.38;
const PULSE_REST: f32 = 1.5;
const PULSE_LIGHT: f32 = 0.55;
const PULSE_GROW: f32 = 0.35;
const LEAST_BEAT: f32 = 0.06;

const BAND_INSET: f32 = 10.0;
const BAND_LANE: f32 = 0.22;

const TRAIL_INSET: f32 = 4.0;

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: SPECIMEN.y,
    deep: p::DELAY_DEEP,
    reads: &[device::NOTE, device::TIME],
    draw,
    controls: &[
        Control::knob(device::TIME, "Time", |c| dial(c, 0))
            .follows(time_aim)
            .showing(without_space),
        Control::knob(device::FEEDBACK, "Feedback", |c| dial(c, 1)).showing(without_space),
        Control::knob(device::MIX, "Mix", |c| dial(c, 2)).showing(without_space),
        Control::pill(device::NOTE, "Sync", |c| pill(c, 0)).switch(Switch {
            on: synced_norm,
            turn_on: |values| values.default_norm(device::NOTE),
        }),
        Control::pill(device::PING, "Ping", |c| pill(c, 1)),
    ],
    handles: &[Handle {
        name: "Echo trail",
        param: device::TIME,
        grab: |_, rect| rect.shrink(TRAIL_INSET),
        value_at: |_, pointer, rect| {
            f64::from(((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0))
        },
        chip,
        cursor: CursorIcon::Move,
        follows: Some(|values| time_aim(values).param),
        also: Some(Also {
            param: device::FEEDBACK,
            value_at: |_, pointer, rect| {
                f64::from(((rect.bottom() - pointer.y) / rect.height().max(1.0)).clamp(0.0, 1.0))
            },
        }),
    }],
};

fn dial(card_rect: Rect, index: usize) -> Rect {
    Rect::from_center_size(
        pos2(card_rect.left() + DIAL_X[index], card_rect.top() + DIALS_Y),
        Vec2::splat(2.0 * card::DIAL_RADIUS + 4.0),
    )
}

fn pill(card_rect: Rect, index: usize) -> Rect {
    let (x, y) = PILLS[index];
    Rect::from_min_size(card_rect.min + vec2(x, y), PILL_SIZE)
}

fn without_space(text: &str) -> String {
    text.replace(' ', "")
}

fn note_index(values: &Values<'_>) -> usize {
    (values.norm(device::NOTE) * (NOTES - 1) as f32)
        .round()
        .max(0.0) as usize
}

const NOTES: usize = 7;

fn synced_norm(norm: f64) -> bool {
    (norm * (NOTES - 1) as f64).round() >= 1.0
}

fn time_aim(values: &Values<'_>) -> Aim {
    Aim {
        param: if note_index(values) > 0 {
            device::NOTE
        } else {
            device::TIME
        },
        name: "Time",
    }
}

fn seconds(values: &Values<'_>, bpm: f32) -> f32 {
    device::seconds_of(
        &[
            (device::NOTE, values.norm(device::NOTE)),
            (device::TIME, values.norm(device::TIME)),
        ],
        bpm,
    )
}

fn time_words(values: &Values<'_>, note: usize, ms: f32) -> String {
    if note > 0 {
        lumen_devices::params::find(values.table, device::NOTE)
            .and_then(|info| info.choice(note))
            .unwrap_or("")
            .to_owned()
    } else if ms < 999.5 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

fn chip(values: &Values<'_>, norm: f64) -> String {
    let note = note_index(values);
    let time = if note > 0 {
        let index = (norm * (NOTES - 1) as f64).round() as usize;
        time_words(values, index, 0.0)
    } else {
        time_words(values, 0, values.plain_at(device::TIME, norm as f32))
    };
    format!("{time} \u{00B7} {:.0}%", values.plain(device::FEEDBACK))
}

fn echoes(feedback: f32) -> usize {
    let f = feedback.clamp(0.0, 0.99);
    if f < 0.01 {
        return FEWEST;
    }
    let count = SPECIMEN_ECHOES * SPECIMEN_FEEDBACK.ln() / f.ln();
    (count.round() as usize).clamp(FEWEST, MOST)
}

fn lane_of(seconds: f32) -> f32 {
    let (least, most) = LANE_TIMES;
    ((seconds / least).ln() / (most / least).ln()).clamp(0.0, 1.0)
}

struct Frame {
    rect: Rect,
    vanish: Pos2,
    scale: f32,
}

impl Frame {
    fn new(rect: Rect) -> Self {
        let scale = (rect.width() / SPECIMEN.x)
            .min(rect.height() / SPECIMEN.y)
            .clamp(LEAST_SCALE, 1.0);
        Self {
            rect,
            vanish: pos2(rect.center().x, rect.top() + rect.height() * VANISH),
            scale,
        }
    }

    fn at(&self, t: f32, side: f32, lane: f32) -> Pos2 {
        let depth = t.powf(DEPTH);
        pos2(
            self.vanish.x + side * self.rect.width() * lane * depth,
            self.vanish.y + (self.rect.bottom() - self.vanish.y) * depth,
        )
    }
}

struct Echo {
    index: usize,
    near: f32,
    left: f32,
}

fn draw(scene: &Scene<'_>) {
    let (painter, rect) = (scene.painter, scene.rect);
    let values = &scene.values;
    let frame = Frame::new(rect);
    let hue = scene.hue;
    let feedback = values.plain(device::FEEDBACK) / 100.0;
    let note = note_index(values);
    let ms = values.plain(device::TIME);
    let time = seconds(values, scene.bpm);
    let lane = LANE.0 + LANE.1 * lane_of(time);
    let band = scene.step == Step::Compact;
    let ping = motion::fade(
        painter.ctx(),
        scene.id.with("lanes"),
        values.norm(device::PING) > 0.5,
    );

    floor(painter, &frame, scene.ink(hue));
    let count = echoes(feedback);
    let step = STEP.min(REACH / (count as f32 - 1.0));
    let beat = pulse(scene, time, count);
    for index in (0..count).rev() {
        let echo = Echo {
            index,
            near: NEAREST - index as f32 * step,
            left: if index == 0 {
                1.0
            } else {
                feedback.powi(index as i32)
            },
        };
        let lit = beat(index, echo.left);
        let side = if index % 2 == 0 { -1.0 } else { 1.0 } * ping;
        if band {
            echo_in_band(painter, rect, &echo, count, side, lit, scene);
        } else {
            let at = frame.at(echo.near, side, lane);
            echo_at(painter, at, &echo, frame.scale, lit, scene);
        }
    }
    if band {
        return;
    }
    let mono = FontId::monospace(p::READOUT_SIZE);
    let ink = scene.ink(hue).gamma_multiply(scene.settle);
    painter.text(
        pos2(rect.left() + LABEL_INSET, rect.top() + LABEL_DOWN),
        Align2::LEFT_CENTER,
        "L",
        mono.clone(),
        ink,
    );
    painter.text(
        pos2(rect.right() - LABEL_INSET - 6.0, rect.top() + LABEL_DOWN),
        Align2::LEFT_CENTER,
        "R",
        mono,
        ink,
    );
    painter.text(
        pos2(rect.right() - HERO_INSET, rect.bottom() - HERO_BASE),
        Align2::RIGHT_BOTTOM,
        time_words(values, note, ms),
        fonts::display(if scene.step == Step::Short {
            HERO_SHORT
        } else {
            HERO_SIZE
        }),
        scene.ink(Color32::WHITE).gamma_multiply(scene.settle),
    );
}

fn floor(painter: &Painter, frame: &Frame, hue: Color32) {
    let rect = frame.rect;
    for k in 0..=SPOKES {
        let x = rect.left() + rect.width() * k as f32 / SPOKES as f32;
        painter.line_segment(
            [pos2(x, rect.bottom()), frame.vanish],
            Stroke::new(stroke::HAIRLINE, hue.gamma_multiply(SPOKE_ALPHA)),
        );
    }
    for k in 1..=CROSS_LINES {
        let t = (k as f32 / CROSS_LINES as f32).powf(CROSS_POWER);
        let y = frame.vanish.y + (rect.bottom() - frame.vanish.y) * t;
        painter.line_segment(
            [
                pos2(frame.vanish.x - (frame.vanish.x - rect.left()) * t, y),
                pos2(frame.vanish.x + (rect.right() - frame.vanish.x) * t, y),
            ],
            Stroke::new(stroke::HAIRLINE, hue.gamma_multiply(CROSS_ALPHA)),
        );
    }
}

fn pulse<'a>(scene: &'a Scene<'_>, time: f32, count: usize) -> impl Fn(usize, f32) -> f32 + 'a {
    let playing = scene.level > 0.0 && !scene.still;
    let along = if playing {
        scene
            .painter
            .ctx()
            .request_repaint_after(motion::METER_FRAME);
        let now = scene.painter.ctx().input(|i| i.time) as f32;
        (now / time.max(LEAST_BEAT)) % (count as f32 + PULSE_REST)
    } else {
        0.0
    };
    let level = scene.level;
    move |index, left| {
        if !playing {
            return 0.0;
        }
        let away = (along - index as f32) / PULSE_WIDTH;
        (-away * away).exp() * left * level.sqrt()
    }
}

fn echo_at(painter: &Painter, at: Pos2, echo: &Echo, scale: f32, lit: f32, scene: &Scene<'_>) {
    let t = echo.near;
    let radius = (RADIUS.0 * t.max(0.0).powf(RADIUS.1) * scale + RADIUS.2 * scale.max(0.6))
        * (1.0 + PULSE_GROW * lit);
    let colour = scene.ink(if echo.index == 0 {
        Color32::WHITE
    } else {
        scene.hue
    });
    let near = t.clamp(0.0, 1.0);
    for (more, strength) in HALOS {
        painter.circle_filled(
            at,
            (radius * HALO_REACH + more * scale).max(radius),
            colour.gamma_multiply(((strength * near) + PULSE_LIGHT * 0.06 * lit).min(1.0)),
        );
    }
    painter.circle_filled(
        at,
        radius,
        colour.gamma_multiply((CORE.0 + CORE.1 * near + PULSE_LIGHT * lit).min(1.0)),
    );
}

fn echo_in_band(
    painter: &Painter,
    rect: Rect,
    echo: &Echo,
    count: usize,
    side: f32,
    lit: f32,
    scene: &Scene<'_>,
) {
    let share = echo.index as f32 / (count as f32 - 1.0).max(1.0);
    let x = rect.left() + BAND_INSET + (rect.width() - 2.0 * BAND_INSET) * share;
    let y = rect.center().y + side * rect.height() * BAND_LANE;
    let size = (1.0 - 0.7 * share) * rect.height() * 0.2;
    let near = 1.0 - 0.7 * share;
    let colour = scene.ink(if echo.index == 0 {
        Color32::WHITE
    } else {
        scene.hue
    });
    let radius = size * (1.0 + PULSE_GROW * lit);
    for (more, strength) in HALOS {
        painter.circle_filled(
            pos2(x, y),
            (radius * HALO_REACH + more * rect.height() / SPECIMEN.y).max(radius),
            colour.gamma_multiply((strength * near + PULSE_LIGHT * 0.06 * lit).min(1.0)),
        );
    }
    painter.circle_filled(
        pos2(x, y),
        radius,
        colour.gamma_multiply((CORE.0 + CORE.1 * near + PULSE_LIGHT * lit).min(1.0)),
    );
}

#[cfg(test)]
mod tests;
