use eframe::egui::{Align2, Color32, CursorIcon, FontId, Rect, Shape, Stroke, Vec2, pos2, vec2};
use lumen_devices::utility as device;

use super::{Control, Drawing, Handle, Scene, Values};
use crate::chain::geometry::Step;
use crate::theme::personality as p;
use crate::theme::personality::card;

const SPECIMEN_HEIGHT: f32 = 136.0;
const SPECIMEN_WIDTH: f32 = 130.0;
const WIDEST: f32 = 200.0;
const GLASS_AT: f32 = 0.36;
const GLASS_HALF: f32 = 16.0;
const GLASS_BELOW: f32 = 28.0;
const GLASS_ABOVE: f32 = 26.0;
const BEAM_IN_FROM: f32 = 8.0;
const BEAM_IN_SHORT: f32 = 14.0;
const BEAM_IN_WIDTH: f32 = 3.0;
const BEAM_IN_ALPHA: f32 = 0.9;
const BEAMS_FROM: f32 = 10.0;
const BEAMS_TO: f32 = 8.0;
const HALO: [(f32, f32); 2] = [(7.0, 0.16), (4.0, 0.3)];
const CORE: f32 = 1.5;
const LEVEL_GROWTH: f32 = 0.6;
const GLASS_FILL: f32 = 0.12;
const GLASS_EDGE: f32 = 1.4;
const FAN_MARGIN: f32 = 8.0;
const GRAB: Vec2 = Vec2::new(20.0, 14.0);
const TIP_DOT: f32 = 3.0;
const TIP_REST: f32 = 0.5;

const WHOLE: Vec2 = Vec2::new(300.0, 290.0);
const DIALS_Y: f32 = 236.0;
const DIAL_X: [f32; 4] = [44.0, 100.0, 156.0, 212.0];
const PILL: (f32, f32, f32, f32) = (248.0, 218.0, 40.0, 18.0);

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: SPECIMEN_HEIGHT,
    deep: p::SCREEN,
    reads: &[],
    draw,
    controls: &[
        Control::knob(device::GAIN, "Gain", |c| dial(c, 0)).showing(without_unit),
        Control::knob(device::PAN, "Pan", |c| dial(c, 1)),
        Control::knob(device::WIDTH, "Width", |c| dial(c, 2))
            .lit(p::PRISM_BEAMS[4])
            .showing(without_space),
        Control::knob(device::BASS_MONO, "Bass mono", |c| dial(c, 3))
            .short("Bass")
            .showing(without_unit),
        Control::pill(device::POLARITY_L, "\u{00D8} L", |c| {
            Rect::from_min_size(
                pos2(c.left() + PILL.0, c.top() + PILL.1),
                vec2(PILL.2, PILL.3),
            )
        }),
    ],
    handles: &[
        Handle {
            name: "Beam spread, upper",
            param: device::WIDTH,
            grab: |values, rect| tip_grab(values, rect, true),
            value_at: |values, pointer, rect| value_at(values, pointer, rect, true),
            chip,
            cursor: CursorIcon::ResizeVertical,
            follows: None,
            also: None,
        },
        Handle {
            name: "Beam spread, lower",
            param: device::WIDTH,
            grab: |values, rect| tip_grab(values, rect, false),
            value_at: |values, pointer, rect| value_at(values, pointer, rect, false),
            chip,
            cursor: CursorIcon::ResizeVertical,
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

fn without_unit(text: &str) -> String {
    text.split(' ').next().unwrap_or(text).to_owned()
}

fn without_space(text: &str) -> String {
    text.replace(' ', "")
}

struct Frame {
    centre: f32,
    glass_x: f32,
    from: f32,
    length: f32,
    reach: f32,
    scale: f32,
    right: f32,
}

impl Frame {
    fn new(rect: Rect) -> Self {
        let scale = (rect.height() / SPECIMEN_HEIGHT).clamp(0.3, 1.0);
        let glass_x = rect.left() + rect.width() * GLASS_AT;
        let from = glass_x + BEAMS_FROM * scale;
        let length = (rect.right() - BEAMS_TO - from).max(1.0);
        let widest = length * fan(WIDEST, 2.0).tan();
        let room = (rect.height() / 2.0 - FAN_MARGIN * scale).max(1.0);
        Self {
            centre: rect.center().y,
            glass_x,
            from,
            length,
            reach: (room / widest).min(1.0),
            scale,
            right: rect.right() - BEAMS_TO,
        }
    }

    fn offset(&self, width: f32, index: usize) -> f32 {
        self.length * fan(width, index as f32 - 2.0).tan() * self.reach
    }

    fn width_for(&self, dy: f32) -> f32 {
        (dy / (self.length * self.reach)).atan() / (2.0 * p::PRISM_FAN) * SPECIMEN_WIDTH
    }
}

fn fan(width: f32, steps: f32) -> f32 {
    steps * p::PRISM_FAN * width / SPECIMEN_WIDTH
}

fn tip_grab(values: &Values<'_>, rect: Rect, upper: bool) -> Rect {
    let frame = Frame::new(rect);
    let offset = frame.offset(values.plain(device::WIDTH), 4);
    let y = frame.centre + if upper { -offset } else { offset };
    let (top, bottom) = if upper {
        (y - GRAB.y, y)
    } else {
        (y, y + GRAB.y)
    };
    Rect::from_min_max(
        pos2(frame.right - GRAB.x + 4.0, top),
        pos2(frame.right + 4.0, bottom),
    )
    .intersect(rect)
}

fn value_at(values: &Values<'_>, pointer: eframe::egui::Pos2, rect: Rect, upper: bool) -> f64 {
    let frame = Frame::new(rect);
    let dy = if upper {
        frame.centre - pointer.y
    } else {
        pointer.y - frame.centre
    };
    values.norm_of(device::WIDTH, frame.width_for(dy).clamp(0.0, WIDEST))
}

fn chip(values: &Values<'_>, norm: f64) -> String {
    format!("width {:.0}%", values.plain_at(device::WIDTH, norm as f32))
}

fn draw(scene: &Scene<'_>) {
    let (painter, rect) = (scene.painter, scene.rect);
    let width = scene.values.plain(device::WIDTH);
    let frame = Frame::new(rect);
    let (centre, scale) = (frame.centre, frame.scale);
    let white = scene.ink(Color32::WHITE);
    let band = scene.step == Step::Compact;
    let lively = 1.0 + LEVEL_GROWTH * scene.level;
    painter.line_segment(
        [
            pos2(rect.left() + BEAM_IN_FROM, centre),
            pos2(frame.glass_x - BEAM_IN_SHORT * scale, centre),
        ],
        Stroke::new(
            BEAM_IN_WIDTH * scale.max(0.6) * (1.0 + 0.3 * scene.level),
            white.gamma_multiply((BEAM_IN_ALPHA * lively).min(1.0)),
        ),
    );
    let from = pos2(frame.from, centre);
    for (index, colour) in p::PRISM_BEAMS.iter().enumerate() {
        let to = pos2(
            frame.from + frame.length,
            centre + frame.offset(width, index),
        );
        let colour = scene.ink(*colour);
        for (line, alpha) in HALO {
            painter.line_segment(
                [from, to],
                Stroke::new(
                    line * scale.max(0.6) * lively,
                    colour.gamma_multiply((alpha * lively).min(1.0)),
                ),
            );
        }
        painter.line_segment([from, to], Stroke::new(CORE, colour));
    }
    for (index, upper) in [(0, true), (1, false)]
        .into_iter()
        .take(if band { 0 } else { 2 })
    {
        let lit = scene.handle_lit[index];
        let y = centre
            + if upper {
                -frame.offset(width, 4)
            } else {
                frame.offset(width, 4)
            };
        let tip = pos2(frame.from + frame.length, y);
        let strength = TIP_REST + (1.0 - TIP_REST) * lit;
        if lit > 0.0 {
            painter.circle_stroke(
                tip,
                card::HANDLE_RING * lit,
                Stroke::new(
                    crate::theme::stroke::HAIRLINE,
                    white.gamma_multiply(card::HANDLE_RING_ALPHA * lit),
                ),
            );
        }
        painter.circle_filled(tip, TIP_DOT, white.gamma_multiply(strength));
    }
    painter.add(Shape::convex_polygon(
        vec![
            pos2(
                frame.glass_x - GLASS_HALF * scale,
                centre + GLASS_BELOW * scale,
            ),
            pos2(
                frame.glass_x + GLASS_HALF * scale,
                centre + GLASS_BELOW * scale,
            ),
            pos2(frame.glass_x, centre - GLASS_ABOVE * scale),
        ],
        white.gamma_multiply(GLASS_FILL * (1.0 + 0.5 * scene.level)),
        Stroke::new(GLASS_EDGE, white),
    ));
    if band {
        return;
    }
    painter.text(
        pos2(
            rect.right() - p::READOUT_INSET,
            rect.top() + p::READOUT_INSET + 2.0,
        ),
        Align2::RIGHT_CENTER,
        format!("width {width:.0}%"),
        FontId::monospace(p::READOUT_SIZE),
        white.gamma_multiply(scene.settle),
    );
}
