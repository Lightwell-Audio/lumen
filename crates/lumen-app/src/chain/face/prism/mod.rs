use eframe::egui::{CursorIcon, Pos2, Rect, Vec2, pos2, vec2};
use lumen_devices::utility as device;

use super::{Control, Drawing, Handle, Scene, Values, paint};
use crate::chain::geometry::Step;
use crate::theme::personality as p;
use crate::theme::personality::card;

mod geometry;
mod light;
mod picture;

use geometry::Frame;
use picture::Look;

const WHOLE: Vec2 = Vec2::new(300.0, 290.0);
const DIALS_Y: f32 = 236.0;
const DIAL_X: [f32; 4] = [44.0, 97.0, 150.0, 203.0];
const PILL: (f32, f32, f32, f32) = (236.0, 226.0, 40.0, 20.0);

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: geometry::SPECIMEN.y,
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
            value_at: |values, pointer, rect| width_at(values, pointer, rect),
            chip,
            cursor: CursorIcon::ResizeVertical,
            follows: None,
            also: None,
        },
        Handle {
            name: "Beam spread, lower",
            param: device::WIDTH,
            grab: |values, rect| tip_grab(values, rect, false),
            value_at: |values, pointer, rect| width_at(values, pointer, rect),
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

fn tip_grab(values: &Values<'_>, rect: Rect, upper: bool) -> Rect {
    Frame::of(values, rect, false).grab(upper)
}

fn width_at(values: &Values<'_>, pointer: Pos2, rect: Rect) -> f64 {
    let norm = Frame::of(values, rect, false).width_norm_at(pointer.y);
    values.norm_of(
        device::WIDTH,
        values.plain_at(device::WIDTH, norm.clamp(0.0, 1.0)),
    )
}

fn chip(values: &Values<'_>, norm: f64) -> String {
    format!("width {:.0}%", values.plain_at(device::WIDTH, norm as f32))
}

fn draw(scene: &Scene<'_>) {
    let compact = scene.step == Step::Compact;
    let frame = Frame::of(&scene.values, scene.rect, compact);
    let light = light::advance(scene);
    let awake = scene.awake;
    let width = scene.values.plain(device::WIDTH);
    let brightness = picture::brightness(awake, light.energy(), scene.values.plain(device::GAIN));
    let time = scene.painter.ctx().input(|i| i.time);

    paint::wash(scene, p::SCREEN, paint::corners(scene.step));
    picture::light(
        scene,
        &frame,
        &light,
        Look {
            awake,
            brightness,
            time,
            handles: !compact,
        },
    );
    picture::glass(scene, &frame, brightness);
    if compact {
        return;
    }
    picture::tip_handles(scene, &frame);
    picture::words(scene, &frame, width);
}

#[cfg(test)]
mod tests;
