use eframe::egui::{Rect, Vec2, pos2};
use lumen_devices::eq;

use super::{Control, Drawing, Parts, Scene};
use crate::theme::personality as p;
use crate::theme::personality::card;

mod bands;
mod parts;
mod picture;
mod plot;
mod response;

pub(crate) const LOW: f32 = eq::LOW_HZ;
pub(crate) const HIGH: f32 = eq::HIGH_HZ;

const SPECIMEN_HEIGHT: f32 = 176.0;

const WHOLE: Vec2 = Vec2::new(380.0, 330.0);
const DIAL_X: [f32; 3] = [52.0, 124.0, 196.0];
const DIALS_Y: f32 = 278.0;
const PILL: f32 = 22.0;
const PILL_X: f32 = 244.0;
const PILL_Y: f32 = 252.0;
const PILL_PITCH: Vec2 = Vec2::new(28.0, 26.0);
const PILLS_ACROSS: usize = 4;

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
    deep: p::EQ_DEEP,
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
    first: parts::first,
    colour: bands::colour,
    is_on: parts::is_on,
    aim: parts::aim,
    at: parts::at,
    drag: parts::drag,
    nudge: parts::nudge,
    wheel: parts::wheel,
    reset_point: parts::reset_point,
    reset_all: parts::reset_all,
    switch: parts::switch,
    words: parts::words,
    pill_words: parts::pill_words,
    number: parts::number,
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

fn draw(scene: &Scene<'_>) {
    picture::draw(scene);
}

#[cfg(test)]
mod tests;
