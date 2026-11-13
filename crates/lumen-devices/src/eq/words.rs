//! How a parameter's value reads as words, and how what a person types is read
//! back (M8 slice B1, ADR-0146): one pair of functions a parameter, so the
//! face, the card and All parameters say the same thing.
//!
//! The words are short enough for a card's strip ("24/oct", "1.20 kHz") and the
//! full meaning is the name and the unit beside them.

use super::{Kind, MOST_DB, SIDES};

/// "40 Hz", "1.20 kHz", "20.0 kHz".
pub fn hz_text(hz: f32) -> String {
    if hz < 999.5 {
        if hz < 99.95 {
            format!("{hz:.1} Hz")
        } else {
            format!("{hz:.0} Hz")
        }
    } else if hz < 9_999.5 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

/// "40", "40 Hz", "1.2 kHz", "1200": a frequency in hertz.
pub fn hz_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-").to_lowercase();
    let (body, scale) = if let Some(rest) = t.strip_suffix("khz") {
        (rest.trim(), 1000.0)
    } else if let Some(rest) = t.strip_suffix('k') {
        (rest.trim(), 1000.0)
    } else if let Some(rest) = t.strip_suffix("hz") {
        (rest.trim(), 1.0)
    } else {
        (t.as_str(), 1.0)
    };
    body.trim()
        .parse::<f32>()
        .ok()
        .filter(|hz| hz.is_finite())
        .map(|hz| hz * scale)
        .filter(|hz| *hz > 0.0)
}

/// "0.0 dB", "âˆ’6.0 dB".
pub fn db_text(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0 dB".to_owned()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

/// "âˆ’6", "âˆ’6.0 dB": decibels, either minus, with or without the unit.
pub(super) fn db_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-");
    let t = t
        .trim_end_matches("dB")
        .trim_end_matches("db")
        .trim_end_matches("DB")
        .trim();
    t.parse::<f32>()
        .ok()
        .filter(|db| db.is_finite() && db.abs() <= MOST_DB * 2.0)
}

/// "1.00", "0.71", "12.0".
pub fn q_text(q: f32) -> String {
    if q < 9.995 {
        format!("{q:.2}")
    } else {
        format!("{q:.1}")
    }
}

/// "1.4", "4": a Q.
pub(super) fn q_parse(text: &str) -> Option<f32> {
    let t = text.trim();
    let t = t
        .strip_prefix('Q')
        .or_else(|| t.strip_prefix('q'))
        .unwrap_or(t)
        .trim();
    t.parse::<f32>().ok().filter(|q| q.is_finite() && *q > 0.0)
}

/// "24/oct": short enough for a strip's value line; All parameters shows the
/// choice's own words.
pub fn slope_text(slope: f32) -> String {
    format!("{slope:.0}/oct")
}

/// "24", "24 dB/oct", "24/oct".
pub(super) fn slope_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let t = t
        .strip_suffix("db/oct")
        .or_else(|| t.strip_suffix("/oct"))
        .or_else(|| t.strip_suffix("db"))
        .unwrap_or(&t)
        .trim();
    t.parse::<f32>().ok().filter(|s| s.is_finite() && *s > 0.0)
}

/// The band type's word.
pub(super) fn kind_text(index: f32) -> String {
    let index = index.round().max(0.0) as usize;
    Kind::CHOICES
        .get(index)
        .copied()
        .unwrap_or(Kind::CHOICES[0])
        .to_owned()
}

/// "Bell", "notch", "shelf high".
pub(super) fn kind_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    Kind::CHOICES
        .iter()
        .position(|word| word.to_lowercase() == t)
        .map(|index| index as f32)
}

pub(super) fn on_text(value: f32) -> String {
    if value >= 0.5 { "On" } else { "Off" }.to_owned()
}

pub(super) fn on_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "on" | "1" => Some(1.0),
        "off" | "0" => Some(0.0),
        _ => None,
    }
}

pub(super) fn side_text(value: f32) -> String {
    let index = if value >= 0.5 { 1 } else { 0 };
    SIDES[index].to_owned()
}

pub(super) fn side_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "input" | "pre" | "0" => Some(0.0),
        "output" | "post" | "1" => Some(1.0),
        _ => None,
    }
}
