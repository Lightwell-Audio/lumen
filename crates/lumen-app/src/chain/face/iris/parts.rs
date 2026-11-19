use eframe::egui::{Pos2, Rect, Vec2};
use lumen_devices::eq;

use super::bands::{Band, SLOPE_LEAST, SLOPE_MOST, SLOPE_STEP, band};
use super::plot::Plot;
use super::response::response;
use crate::chain::face::{Aim, Values, Write, Writes};

const KEY_OCTAVE: f32 = 1.0 / 24.0;
pub(super) const KEY_DB: f32 = 0.5;
const WHEEL_OCTAVES: f32 = 0.8;
pub(super) const FINE: f32 = 0.2;

pub(super) fn first(values: &Values<'_>) -> usize {
    (0..eq::BANDS)
        .find(|index| is_on(values, *index))
        .unwrap_or(0)
}

pub(super) fn is_on(values: &Values<'_>, index: usize) -> bool {
    values.norm(eq::on_of(index + 1)) > 0.5
}

pub(super) fn aim(values: &Values<'_>, index: usize, control: usize) -> Option<Aim> {
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

pub(super) fn node_db(values: &Values<'_>, band: &Band) -> f32 {
    response(values).db_at_hz(band.freq)
}

pub(super) fn at(values: &Values<'_>, index: usize, rect: Rect) -> Option<Pos2> {
    let band = band(values, index);
    Some(Plot::of(rect).place(band.freq, node_db(values, &band)))
}

fn moved_to(values: &Values<'_>, band: &Band, freq: f32, gain: f32) -> Writes {
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

pub(super) fn drag(values: &Values<'_>, index: usize, pointer: Pos2, rect: Rect) -> Writes {
    let band = band(values, index);
    let plot = Plot::of(rect);
    let freq = eq::axis_hz(plot.share_of_x(pointer.x));
    let gain = if band.kind.has_gain() {
        response(values).gain_for(index, &band, freq, plot.db_of_y(pointer.y))
    } else {
        band.gain
    };
    moved_to(values, &band, freq, gain)
}

pub(super) fn nudge(values: &Values<'_>, index: usize, steps: Vec2, fine: bool) -> Writes {
    let band = band(values, index);
    let share = if fine { FINE } else { 1.0 };
    let freq = (band.freq * 2f32.powf(steps.x * KEY_OCTAVE * share)).clamp(eq::LOW_HZ, eq::HIGH_HZ);
    let gain = (band.gain + steps.y * KEY_DB * share).clamp(-eq::MOST_DB, eq::MOST_DB);
    moved_to(values, &Band { on: true, ..band }, freq, gain)
}

pub(super) fn wheel(values: &Values<'_>, index: usize, notches: f32, fine: bool) -> Option<Write> {
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

pub(super) fn reset_point(values: &Values<'_>, index: usize) -> Writes {
    let band = band(values, index);
    let id = if band.kind.has_gain() {
        eq::gain_of(band.number)
    } else {
        eq::freq_of(band.number)
    };
    vec![(id, values.default_norm(id))]
}

pub(super) fn reset_all(values: &Values<'_>, index: usize) -> Writes {
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

pub(super) fn switch(index: usize) -> u32 {
    eq::on_of(index + 1)
}

pub(super) fn words(values: &Values<'_>, index: usize) -> String {
    band(values, index).words()
}

pub(super) fn pill_words(values: &Values<'_>, index: usize) -> String {
    let band = band(values, index);
    format!("Band {} ({})", band.number, band.kind.word())
}

pub(super) fn number(index: usize) -> String {
    (index + 1).to_string()
}
