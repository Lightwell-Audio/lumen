//! Shutter's parameters (M8 slice B5): the table the card, All parameters,
//! the lanes and the engine read, and the words each value is shown and typed
//! in.

use crate::params::{ParamInfo, Taper};
use crate::sampler::FILTER_TOP_HZ;

/// Level, in decibels.
pub const LEVEL: u32 = 1;
/// Pitch, in semitones.
pub const PITCH: u32 = 2;
/// Fine pitch, in hundredths of a semitone.
pub const FINE: u32 = 3;
/// Attack, in seconds.
pub const ATTACK: u32 = 4;
/// Decay, in seconds.
pub const DECAY: u32 = 5;
/// Sustain, as a share of the note's own level.
pub const SUSTAIN: u32 = 6;
/// Release, in seconds.
pub const RELEASE: u32 = 7;
/// The filter: off, low pass or high pass.
pub const FILTER: u32 = 8;
/// The filter's cutoff, in Hz.
pub const CUTOFF: u32 = 9;
/// The filter's resonance, as a share.
pub const RESONANCE: u32 = 10;
/// How much a note's velocity sets its level.
pub const VELOCITY: u32 = 11;

/// How many parameters the table holds.
pub(crate) const PARAMS: usize = 11;

/// The lowest level: under it, silence.
pub(crate) const FLOOR_DB: f32 = -60.0;

/// What the columns of the table are for, so a reader sees the shape of a row.
#[allow(clippy::too_many_arguments)]
const fn param(
    id: u32,
    name: &'static str,
    unit: &'static str,
    low: f32,
    high: f32,
    taper: Taper,
    default: f32,
    steps: i32,
    choices: &'static [&'static str],
    text: fn(f32) -> String,
    parse: fn(&str) -> Option<f32>,
) -> ParamInfo {
    ParamInfo {
        id,
        name,
        unit,
        low,
        high,
        taper,
        default,
        steps,
        choices,
        text,
        parse,
    }
}

/// Shutter's parameters, in the order the card and All parameters show them.
/// Ids are stable and never reused. They hold what every face shares; the
/// regions, zones and pads are the sound's own ([`Sound`]), not parameters.
pub static TABLE: &[ParamInfo] = &[
    param(
        LEVEL,
        "Level",
        "dB",
        FLOOR_DB,
        12.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        gain_text,
        gain_parse,
    ),
    param(
        PITCH,
        "Pitch",
        "st",
        -24.0,
        24.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        semitone_text,
        semitone_parse,
    ),
    param(
        FINE,
        "Fine",
        "ct",
        -100.0,
        100.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        cents_text,
        cents_parse,
    ),
    param(
        ATTACK,
        "Attack",
        "s",
        0.0,
        10.0,
        Taper::Power(3.0),
        0.0,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        DECAY,
        "Decay",
        "s",
        0.0,
        10.0,
        Taper::Power(3.0),
        0.0,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        SUSTAIN,
        "Sustain",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        100.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        RELEASE,
        "Release",
        "s",
        0.0,
        10.0,
        Taper::Power(3.0),
        0.05,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        FILTER,
        "Filter",
        "",
        0.0,
        2.0,
        Taper::Linear,
        0.0,
        2,
        &["Off", "Low pass", "High pass"],
        filter_text,
        filter_parse,
    ),
    param(
        CUTOFF,
        "Cutoff",
        "Hz",
        20.0,
        FILTER_TOP_HZ,
        Taper::Power(3.0),
        FILTER_TOP_HZ,
        0,
        &[],
        cutoff_text,
        cutoff_parse,
    ),
    param(
        RESONANCE,
        "Res",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        VELOCITY,
        "Vel>Level",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
];

fn gain_text(db: f32) -> String {
    if db <= FLOOR_DB {
        "\u{2212}\u{221E} dB".to_owned()
    } else if db.abs() < 0.05 {
        "0.0 dB".to_owned()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

fn gain_parse(text: &str) -> Option<f32> {
    number_in(text, &["db"]).filter(|db| *db <= 12.0)
}

fn semitone_text(st: f32) -> String {
    if st.abs() < 0.005 {
        "0 st".to_owned()
    } else {
        format!("{st:+.2} st").replace('-', "\u{2212}")
    }
}

fn semitone_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches("st").trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn cents_text(ct: f32) -> String {
    if ct.abs() < 0.05 {
        "0 ct".to_owned()
    } else {
        format!("{ct:+.0} ct").replace('-', "\u{2212}")
    }
}

fn cents_parse(text: &str) -> Option<f32> {
    number_in(text, &["ct", "cents"])
}

fn seconds_text(s: f32) -> String {
    if s < 0.0005 {
        "0 ms".to_owned()
    } else if s < 1.0 {
        format!("{:.0} ms", s * 1000.0)
    } else {
        format!("{s:.2} s")
    }
}

fn seconds_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if let Some(ms) = t.strip_suffix("ms") {
        return ms.trim().parse::<f32>().ok().map(|v| v / 1000.0);
    }
    if let Some(s) = t.strip_suffix('s') {
        return s.trim().parse::<f32>().ok();
    }
    t.parse::<f32>().ok()
}

fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

fn percent_parse(text: &str) -> Option<f32> {
    number_in(text, &["%"])
}

fn filter_text(value: f32) -> String {
    match value.round() as i32 {
        1 => "Low pass",
        2 => "High pass",
        _ => "Off",
    }
    .to_owned()
}

fn filter_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "off" | "none" | "0" => Some(0.0),
        "low" | "low pass" | "lowpass" | "lp" | "1" => Some(1.0),
        "high" | "high pass" | "highpass" | "hp" | "2" => Some(2.0),
        _ => None,
    }
}

fn cutoff_text(hz: f32) -> String {
    if hz >= 1_000.0 {
        format!("{:.1} kHz", hz / 1_000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

fn cutoff_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-").to_lowercase();
    if let Some(k) = t.strip_suffix("khz") {
        return k.trim().parse::<f32>().ok().map(|v| v * 1_000.0);
    }
    number_in(&t, &["hz"])
}

/// A number in `text`, with any of `units` after it and either minus.
fn number_in(text: &str, units: &[&str]) -> Option<f32> {
    let mut t = text.trim().replace('\u{2212}', "-").to_lowercase();
    for unit in units {
        if let Some(rest) = t.strip_suffix(unit) {
            t = rest.trim_end().to_owned();
        }
    }
    t.trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}
