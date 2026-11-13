//! Prism's card (the utility device), from the round-2 specimen
//! (`docs/specimens/m8/devices.html`, `prism()`): light split by a prism. A
//! white beam enters, the glass splits it into five coloured beams, and Width
//! spreads them: at 0 % they are one beam (mono), at 130 % they fan as in the
//! specimen, at 200 % half as wide again. The screen reads "width 130%" from
//! the device's value.
//!
//! The screen is alive: the beams burn brighter with the level of what the
//! device hears, and calm at rest. It is a control too: the two outer beams'
//! tips are handles, and dragging one up or down is the same edit as the Width
//! dial. Under the screen sit four lit dials (Gain, Pan, Width in violet, Bass
//! mono) and the left polarity pill "Ø L"; swap, the right polarity and the DC
//! block are in All parameters.

use eframe::egui::{Align2, Color32, CursorIcon, FontId, Rect, Shape, Stroke, Vec2, pos2, vec2};
use lumen_devices::utility as device;

use super::{Control, Drawing, Handle, Scene, Values};
use crate::chain::geometry::Step;
use crate::theme::personality as p;
use crate::theme::personality::card;

/// The specimen's screen was 136 pt tall; a shorter screen scales the glass and
/// the lines down by what fits.
const SPECIMEN_HEIGHT: f32 = 136.0;
/// The percent the specimen's fan was drawn at, and the widest Width.
const SPECIMEN_WIDTH: f32 = 130.0;
const WIDEST: f32 = 200.0;
/// How far across the screen the glass stands, as a share.
const GLASS_AT: f32 = 0.36;
/// The glass's half width and its height above and below the beam, in the
/// specimen's points.
const GLASS_HALF: f32 = 16.0;
const GLASS_BELOW: f32 = 28.0;
const GLASS_ABOVE: f32 = 26.0;
/// The beam in starts this far from the screen's left and stops short of the
/// glass; its width and opacity.
const BEAM_IN_FROM: f32 = 8.0;
const BEAM_IN_SHORT: f32 = 14.0;
const BEAM_IN_WIDTH: f32 = 3.0;
const BEAM_IN_ALPHA: f32 = 0.9;
/// The beams out leave this far right of the glass's stand and stop this far
/// from the screen's right.
const BEAMS_FROM: f32 = 10.0;
const BEAMS_TO: f32 = 8.0;
/// The beams' halo (width, opacity) and core width.
const HALO: [(f32, f32); 2] = [(7.0, 0.16), (4.0, 0.3)];
const CORE: f32 = 1.5;
/// With the level at 1 a halo is this much wider and stronger.
const LEVEL_GROWTH: f32 = 0.6;
/// The glass: its fill and edge.
const GLASS_FILL: f32 = 0.12;
const GLASS_EDGE: f32 = 1.4;
/// The widest fan stops this far from the screen's top and bottom.
const FAN_MARGIN: f32 = 8.0;
/// The handles at the outer beams' tips: their grab, how far a tip's dot is
/// drawn, at rest and lit.
const GRAB: Vec2 = Vec2::new(20.0, 14.0);
const TIP_DOT: f32 = 3.0;
const TIP_REST: f32 = 0.5;
/// How strong the picture stays under the compact card's dials.
const BAND_DIM: f32 = 0.45;

/// The card: 300 x 290 with a 136 pt screen, as the specimen draws it.
const WHOLE: Vec2 = Vec2::new(300.0, 290.0);
/// The dials' centres across the card and down from its top (the screen's foot
/// and 40 pt), and the pill's place.
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
        Control::knob(device::BASS_MONO, "Bass mono", |c| dial(c, 3)).showing(without_unit),
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
        },
        Handle {
            name: "Beam spread, lower",
            param: device::WIDTH,
            grab: |values, rect| tip_grab(values, rect, false),
            value_at: |values, pointer, rect| value_at(values, pointer, rect, false),
            chip,
            cursor: CursorIcon::ResizeVertical,
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

/// "0.0 dB" as "0.0" and "120 Hz" as "120": the unit is in the tooltip.
fn without_unit(text: &str) -> String {
    text.split(' ').next().unwrap_or(text).to_owned()
}

/// "130 %" as "130%".
fn without_space(text: &str) -> String {
    text.replace(' ', "")
}

/// Where the picture's lines run, for one picture rectangle.
struct Frame {
    /// The beams' middle line, and the glass's stand.
    centre: f32,
    glass_x: f32,
    /// Where the beams out start, and how far they run.
    from: f32,
    length: f32,
    /// The vertical scale that makes the widest fan just fit the picture.
    reach: f32,
    /// How much the glass and the lines are scaled down (1 on the whole card).
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
        let room = (rect.height() / 2.0 - FAN_MARGIN).max(1.0);
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

    /// How far beam `index` (0 to 4) is from the middle line, at `width` percent.
    fn offset(&self, width: f32, index: usize) -> f32 {
        self.length * fan(width, index as f32 - 2.0).tan() * self.reach
    }

    /// The Width (percent) that puts the outer beams `dy` from the middle line.
    fn width_for(&self, dy: f32) -> f32 {
        (dy / (self.length * self.reach)).atan() / (2.0 * p::PRISM_FAN) * SPECIMEN_WIDTH
    }
}

/// The angle of the beam `steps` from the middle at `width` percent.
fn fan(width: f32, steps: f32) -> f32 {
    steps * p::PRISM_FAN * width / SPECIMEN_WIDTH
}

/// The grab at the tip of the upper or the lower outer beam.
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

/// The Width (0 to 1) a pointer on the screen stands for.
fn value_at(values: &Values<'_>, pointer: eframe::egui::Pos2, rect: Rect, upper: bool) -> f64 {
    let frame = Frame::new(rect);
    let dy = if upper {
        frame.centre - pointer.y
    } else {
        pointer.y - frame.centre
    };
    values.norm_of(device::WIDTH, frame.width_for(dy).clamp(0.0, WIDEST))
}

/// The live chip's words while a handle is dragged.
fn chip(values: &Values<'_>, norm: f64) -> String {
    format!("width {:.0}%", values.plain_at(device::WIDTH, norm as f32))
}

fn draw(scene: &Scene<'_>) {
    let (painter, rect) = (scene.painter, scene.rect);
    let width = scene.values.plain(device::WIDTH);
    let frame = Frame::new(rect);
    let (centre, scale) = (frame.centre, frame.scale);
    let white = scene.ink(Color32::WHITE);
    // Under the compact card's dials the picture is a calm band: no tips, no
    // readout, the beams dimmer.
    let band = scene.step == Step::Compact;
    let calm = if band { BAND_DIM } else { 1.0 };
    let lively = 1.0 + LEVEL_GROWTH * scene.level;
    // The beam in.
    painter.line_segment(
        [
            pos2(rect.left() + BEAM_IN_FROM, centre),
            pos2(frame.glass_x - BEAM_IN_SHORT * scale, centre),
        ],
        Stroke::new(
            BEAM_IN_WIDTH * scale.max(0.6) * (1.0 + 0.3 * scene.level),
            white.gamma_multiply((BEAM_IN_ALPHA * lively).min(1.0) * calm),
        ),
    );
    // The beams out: five, fanning with the width, brighter with the level.
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
                    colour.gamma_multiply((alpha * lively).min(1.0) * calm),
                ),
            );
        }
        painter.line_segment([from, to], Stroke::new(CORE, colour.gamma_multiply(calm)));
    }
    // The handles' tips: a dot at rest, lit under the pointer.
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
                Stroke::new(1.0, white.gamma_multiply(0.6 * lit)),
            );
        }
        painter.circle_filled(tip, TIP_DOT, white.gamma_multiply(strength));
    }
    // The glass, over the beams.
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
        white.gamma_multiply(GLASS_FILL * (1.0 + 0.5 * scene.level) * calm),
        Stroke::new(GLASS_EDGE, white.gamma_multiply(calm)),
    ));
    // The readout, from the device's own value; it comes back with a changed
    // preset.
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
