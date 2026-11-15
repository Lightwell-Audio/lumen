use eframe::egui::{CursorIcon, Pos2, Rect, Vec2, pos2, vec2};
use lumen_devices::dynamics::limiter as device;

use super::{Control, Drawing, Handle, Scene, Values};
use crate::chain::geometry::Step;
use crate::theme::personality as p;
use crate::theme::personality::card;

mod geometry;
mod history;
mod picture;

use history::Heard;

const WHOLE: Vec2 = Vec2::new(300.0, 330.0);
const DIAL_X: [f32; 3] = [56.0, 123.33, 190.67];
const DIALS_Y: f32 = 278.0;
const PILL: (f32, f32, f32, f32) = (236.0, 268.0, 44.0, 20.0);
const MINUS: char = '\u{2212}';

pub(crate) static DRAWING: Drawing = Drawing {
    card: WHOLE,
    screen: geometry::SPECIMEN.y,
    deep: p::HORIZON_DEEP,
    reads: &[],
    draw,
    controls: &[
        Control::knob(device::CEILING, "Ceiling", |c| dial(c, 0)).showing(bare),
        Control::knob(device::RELEASE, "Release", |c| dial(c, 1)).showing(tidy),
        Control::knob(device::LOOK, "Look", |c| dial(c, 2)).showing(tidy),
        Control::pill(device::DETECT, "True", |c| {
            Rect::from_min_size(
                pos2(c.left() + PILL.0, c.top() + PILL.1),
                vec2(PILL.2, PILL.3),
            )
        }),
    ],
    handles: &[Handle {
        name: "Ceiling line",
        param: device::CEILING,
        grab: |values, rect| line_box(values, rect),
        value_at: |values, pointer, rect| ceiling_at(values, pointer, rect),
        chip: |values, norm| {
            format!(
                "ceiling {}",
                db_words(values.plain_at(device::CEILING, norm as f32))
            )
        },
        cursor: CursorIcon::ResizeVertical,
        follows: None,
        also: None,
    }],
};

fn dial(card_rect: Rect, index: usize) -> Rect {
    Rect::from_center_size(
        pos2(card_rect.left() + DIAL_X[index], card_rect.top() + DIALS_Y),
        Vec2::splat(2.0 * card::DIAL_RADIUS + 4.0),
    )
}

fn bare(text: &str) -> String {
    text.replace(" dB", "")
}

fn tidy(text: &str) -> String {
    match text.split_once(' ') {
        Some((number, unit)) if number.contains('.') => {
            format!(
                "{} {unit}",
                number.trim_end_matches('0').trim_end_matches('.')
            )
        }
        _ => text.to_owned(),
    }
}

fn db_words(db: f32) -> String {
    format!("{db:.1} dB").replace('-', &MINUS.to_string())
}

fn line_box(values: &Values<'_>, rect: Rect) -> Rect {
    geometry::line_box(values.plain(device::CEILING), rect)
}

fn ceiling_at(values: &Values<'_>, pointer: Pos2, rect: Rect) -> f64 {
    values.norm_of(device::CEILING, geometry::level_at(pointer.y, rect))
}

fn corners(step: Step) -> (f32, f32) {
    (
        card::SCREEN_RADIUS,
        if step == Step::Short {
            0.0
        } else {
            card::SCREEN_RADIUS
        },
    )
}

fn draw(scene: &Scene<'_>) {
    let rect = scene.rect;
    let ceiling = scene.values.plain(device::CEILING);
    let y = geometry::y_of(ceiling, rect);
    let heard = scene.reading.map_or(Heard::SILENT, Heard::of);
    let sky = history::advance(scene, heard);
    let (top, foot) = corners(scene.step);
    let awake = if scene.still {
        f32::from(u8::from(heard.sounding()))
    } else {
        scene.awake
    };
    let lit = scene.handle_lit[0];
    let words = geometry::show_grid_words(scene.step);

    picture::backdrop(scene, y, sky.flash(), awake, (top, foot));
    picture::grid(scene, words);
    if scene.still {
        picture::resting(scene, 1.0 - awake, 0.0, false);
        if heard.sounding() {
            picture::still_bars(scene, heard);
        }
    } else {
        picture::resting(scene, 1.0 - awake, 1.0 - awake, words);
        picture::trace(scene, &sky, y, awake, foot);
    }
    picture::ceiling(scene, y, sky.flash(), lit);
    if scene.step == Step::Compact {
        return;
    }
    picture::ceiling_words(scene, y, &db_words(ceiling));
    picture::readout(scene, &sky, awake, &geometry::readout(rect, scene.step));
}

#[cfg(test)]
mod tests;
