//! What a value of Flare's reads as, and what a person types to set one: the
//! text of every parameter in [`super::table::TABLE`] (M8 slice B4).

use super::table::{DIVISION_WORDS, FILTERS, SHAPES, TARGETS};

/// The words of each division, for the table.
pub(super) fn division_words() -> &'static [&'static str] {
    DIVISION_WORDS
}

/// "Saw", "Square": the waveform's name.
pub(super) fn shape_text(v: f32) -> String {
    word(SHAPES, v, "Saw")
}

/// "−1", "+1": the octave.
pub(super) fn octave_text(v: f32) -> String {
    let n = v.round() as i32;
    if n > 0 {
        format!("+{n}")
    } else {
        format!("{n}")
    }
}

/// "0.80": the resonance.
pub(super) fn decimal_text(v: f32) -> String {
    format!("{v:.2}")
}

/// "2.40 kHz", "420 Hz".
pub(super) fn frequency_text(v: f32) -> String {
    if v >= 1_000.0 {
        format!("{:.2} kHz", v / 1_000.0)
    } else {
        format!("{:.0} Hz", v)
    }
}

/// "5.0 ms", "380 ms", "1.20 s".
pub(super) fn seconds_text(v: f32) -> String {
    if v < 0.1 {
        format!("{:.1} ms", v * 1_000.0)
    } else if v < 1.0 {
        format!("{:.0} ms", v * 1_000.0)
    } else {
        format!("{v:.2} s")
    }
}

/// "Off" at nothing, then a time.
pub(super) fn glide_text(v: f32) -> String {
    if v <= 0.0 {
        "Off".to_owned()
    } else {
        seconds_text(v)
    }
}

/// "16 %".
pub(super) fn percent_text(v: f32) -> String {
    format!("{:.0} %", v)
}

/// "−7 ct", "+7 ct".
pub(super) fn cents_text(v: f32) -> String {
    format!("{}{:.0} ct", sign(v), v.abs())
}

/// "−0.6 oct", "+2.0 oct".
pub(super) fn signed_octaves_text(v: f32) -> String {
    format!("{}{:.1} oct", sign(v), v.abs())
}

/// "0.60 oct".
pub(super) fn octaves_text(v: f32) -> String {
    format!("{v:.2} oct")
}

/// "L35", "C", "R20".
pub(super) fn pan_text(v: f32) -> String {
    let n = v.round() as i32;
    if n == 0 {
        "C".to_owned()
    } else if n < 0 {
        format!("L{}", -n)
    } else {
        format!("R{n}")
    }
}

/// "5.00 Hz".
pub(super) fn hertz_text(v: f32) -> String {
    format!("{v:.2} Hz")
}

/// "16".
pub(super) fn voices_text(v: f32) -> String {
    format!("{:.0}", v.round())
}

/// "Low 12", "Band 12": the filter's shape.
pub(super) fn filter_text(v: f32) -> String {
    word(FILTERS, v, "Low 12")
}

/// "Pitch", "Cutoff": what the LFO moves.
pub(super) fn target_text(v: f32) -> String {
    word(TARGETS, v, "Off")
}

/// "1/4": the LFO's cycle.
pub(super) fn division_text(v: f32) -> String {
    word(division_words(), v, "1/4")
}

/// "Free", "Sync".
pub(super) fn sync_text(v: f32) -> String {
    if v >= 0.5 { "Sync" } else { "Free" }.to_owned()
}

/// The word of a choice at `value`, or `fallback`.
pub(super) fn word(words: &[&str], value: f32, fallback: &str) -> String {
    let index = value.round() as i32;
    usize::try_from(index)
        .ok()
        .and_then(|i| words.get(i))
        .map_or_else(|| fallback.to_owned(), |w| (*w).to_owned())
}

/// A minus sign as the type uses it.
pub(super) fn sign(v: f32) -> &'static str {
    if v < 0.0 { "\u{2212}" } else { "+" }
}

/// The number in `text`, with the minus sign the type uses and a unit after
/// it allowed.
pub(super) fn number(text: &str) -> Option<f32> {
    let text = text.trim().replace('\u{2212}', "-").to_lowercase();
    let text = text.trim();
    let cut = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(cut);
    let value: Option<f32> = number.trim().parse().ok();
    let value = value.filter(|v: &f32| v.is_finite())?;
    Some(value * unit_scale(unit.trim()))
}

/// How much a unit multiplies a number by ("khz" a thousand, "ms" a
/// thousandth).
pub(super) fn unit_scale(unit: &str) -> f32 {
    match unit {
        "khz" | "k" => 1_000.0,
        "ms" => 0.001,
        _ => 1.0,
    }
}

pub(super) fn shape_parse(text: &str) -> Option<f32> {
    by_word(SHAPES, text)
}

pub(super) fn filter_parse(text: &str) -> Option<f32> {
    by_word(FILTERS, text)
}

pub(super) fn target_parse(text: &str) -> Option<f32> {
    by_word(TARGETS, text)
}

pub(super) fn division_parse(text: &str) -> Option<f32> {
    let text = text.trim().to_lowercase();
    division_words()
        .iter()
        .position(|w| w.to_lowercase() == text)
        .map(|i| i as f32)
}

pub(super) fn sync_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "sync" | "on" => Some(1.0),
        "free" | "off" => Some(0.0),
        _ => None,
    }
}

pub(super) fn octave_parse(text: &str) -> Option<f32> {
    number(text).map(|v| v.round())
}

pub(super) fn decimal_parse(text: &str) -> Option<f32> {
    number(text)
}

pub(super) fn frequency_parse(text: &str) -> Option<f32> {
    number(text)
}

pub(super) fn seconds_parse(text: &str) -> Option<f32> {
    number(text)
}

pub(super) fn glide_parse(text: &str) -> Option<f32> {
    if text.trim().eq_ignore_ascii_case("off") {
        return Some(0.0);
    }
    number(text)
}

pub(super) fn percent_parse(text: &str) -> Option<f32> {
    number(text)
}

pub(super) fn cents_parse(text: &str) -> Option<f32> {
    number(text)
}

pub(super) fn signed_octaves_parse(text: &str) -> Option<f32> {
    number(text)
}

pub(super) fn octaves_parse(text: &str) -> Option<f32> {
    number(text)
}

pub(super) fn hertz_parse(text: &str) -> Option<f32> {
    number(text)
}

pub(super) fn voices_parse(text: &str) -> Option<f32> {
    number(text).map(|v| v.round())
}

pub(super) fn pan_parse(text: &str) -> Option<f32> {
    let text = text.trim().to_lowercase();
    match text.as_str() {
        "c" | "centre" | "center" => return Some(0.0),
        _ => {}
    }
    let (side, rest) = text.split_at(1);
    match side {
        "l" => number(rest).map(|v| -v.abs()),
        "r" => number(rest).map(f32::abs),
        _ => number(&text),
    }
}

pub(super) fn by_word(words: &[&str], text: &str) -> Option<f32> {
    let text = text.trim().to_lowercase();
    words
        .iter()
        .position(|w| w.to_lowercase() == text)
        .map(|i| i as f32)
        .or_else(|| {
            // A number is a value too, so `filter 1` picks Low 24.
            number(&text).filter(|v| *v >= 0.0)
        })
}
