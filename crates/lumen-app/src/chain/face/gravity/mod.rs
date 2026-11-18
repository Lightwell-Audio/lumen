use eframe::egui::{CursorIcon, Pos2, Rect, Vec2, pos2};
use lumen_devices::dynamics::compressor as device;

use super::{Control, Drawing, Handle, Scene, Values, paint};
use crate::chain::geometry::Step;
use crate::theme::personality as p;
use crate::theme::personality::card;

mod geometry;
mod picture;
mod well;

use geometry::Frame;
use well::Heard;

const WHOLE: Vec2 = Vec2::new(380.0, 330.0);
const DIAL_X: [f32; 5] = [44.0, 104.0, 164.0, 224.0, 284.0];
const DIALS_Y: f32 = 278.0;
const MINUS: char = '\u{2212}';

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: geometry::SPECIMEN.y,
    deep: p::GRAVITY_DEEP,
    reads: &[],
    draw,
    controls: &[
        Control::knob(device::THRESHOLD, "Pull", |c| dial(c, 0)).showing(tidy),
        Control::knob(device::RATIO, "Ratio", |c| dial(c, 1)).showing(tidy),
        Control::knob(device::ATTACK, "Attack", |c| dial(c, 2)).showing(tidy),
        Control::knob(device::RELEASE, "Release", |c| dial(c, 3)).showing(tidy),
        Control::knob(device::MAKEUP, "Makeup", |c| dial(c, 4)).showing(lower),
    ],
    handles: &[
        Handle {
            name: "Threshold ring",
            param: device::THRESHOLD,
            grab: |values, rect| ring_box(values, rect),
            value_at: |values, pointer, rect| ring_value(values, pointer, rect),
            chip: |values, norm| {
                format!(
                    "pull {}",
                    tidy(&db_words(values.plain_at(device::THRESHOLD, norm as f32)))
                )
            },
            cursor: CursorIcon::ResizeNeSw,
            follows: None,
            also: None,
        },
        Handle {
            name: "Body weight",
            param: device::RATIO,
            grab: |_, rect| geometry::body_box(rect),
            value_at: |_, pointer, rect| geometry::body_value(pointer, rect),
            chip: |values, norm| {
                format!(
                    "ratio {:.1} : 1",
                    values.plain_at(device::RATIO, norm as f32)
                )
            },
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

fn tidy(text: &str) -> String {
    text.replace(".00 ", " ")
        .replace(".0 ", " ")
        .replace(".0:", ":")
}

fn lower(text: &str) -> String {
    text.to_lowercase()
}

fn db_words(db: f32) -> String {
    format!("{db:.1} dB").replace('-', &MINUS.to_string())
}

fn ring_box(values: &Values<'_>, rect: Rect) -> Rect {
    geometry::ring_box(rect, values.plain(device::THRESHOLD))
}

fn ring_value(values: &Values<'_>, pointer: Pos2, rect: Rect) -> f64 {
    values.norm_of(device::THRESHOLD, geometry::ring_value_db(pointer, rect))
}

fn draw(scene: &Scene<'_>) {
    let rect = scene.rect;
    let threshold = scene.values.plain(device::THRESHOLD);
    let ratio = scene.values.plain(device::RATIO);
    let heard = scene.reading.map_or(Heard::SILENT, Heard::of);
    let awake = if scene.still {
        f32::from(u8::from(heard.sounding()))
    } else {
        scene.awake
    };
    let well = well::advance(scene, heard, awake);
    let frame = Frame::of(rect);

    paint::wash(scene, p::GRAVITY_DEEP, paint::corners(scene.step));
    picture::glow_in_well(scene, &frame, &well, awake);
    picture::membrane(scene, &frame, well.membrane());
    picture::threshold_ring(scene, threshold, &well, scene.handle_lit[0]);
    picture::body(scene, &frame, &well, ratio, scene.handle_lit[1]);
    if scene.step == Step::Compact {
        return;
    }
    picture::ring_words(scene, threshold, &tidy(&db_words(threshold)));
    picture::readout(
        scene,
        &well,
        awake,
        &geometry::readout(rect, scene.step),
        scene.step == Step::Whole,
    );
}

#[cfg(test)]
mod tests;
