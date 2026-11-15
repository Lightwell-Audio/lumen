//! Parallax's parameters (OR-8): their ids, the table the card, the lanes and
//! the engine all read, and the words a value is written in. Split out of
//! mod.rs, which keeps the DSP and had grown past 800 lines.

use crate::effect::Values;
use crate::params::{self, ParamInfo, Taper};

/// The time in milliseconds, when the note is Free.
pub const TIME: u32 = 1;
/// The time in notes: Free, or a note the song's tempo places.
pub const NOTE: u32 = 2;
/// How much of each echo returns to the loop, 0 to 99 %.
pub const FEEDBACK: u32 = 3;
/// Ping-pong, 0 to 1: at 1 the sound enters the lines summed to mono on the
/// left one and each echo crosses to the other side, so the repeats alternate.
pub const PING: u32 = 4;
/// A highpass in the loop, in Hz; 0 is off.
pub const LOW_CUT: u32 = 5;
/// A lowpass in the loop, in Hz; 20 kHz is off.
pub const HIGH_CUT: u32 = 6;
/// Dry to wet, 0 to 100 %.
pub const MIX: u32 = 7;
/// Hold what the loop carries and stop taking new input.
pub const FREEZE: u32 = 8;

/// The note choices, in order: Free first, then the note the tempo places.
pub(super) const NOTES: [(&str, f32); 7] = [
    ("Free", 0.0),
    ("1/32", 0.125),
    ("1/16", 0.25),
    ("1/8", 0.5),
    ("1/4", 1.0),
    ("1/2", 2.0),
    ("1/1", 4.0),
];

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

/// Parallax's parameters, in the order the card shows them: the eight on the
/// strip. Ids are stable and never reused.
pub static TABLE: &[ParamInfo] = &[
    param(
        TIME,
        "Time",
        "ms",
        1.0,
        4000.0,
        Taper::Power(2.0),
        375.0,
        0,
        &[],
        time_text,
        time_parse,
    ),
    param(
        NOTE,
        "Note",
        "",
        0.0,
        1.0,
        Taper::Linear,
        4.0 / 6.0,
        6,
        &["Free", "1/32", "1/16", "1/8", "1/4", "1/2", "1/1"],
        note_text,
        note_parse,
    ),
    param(
        FEEDBACK,
        "Repeats",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        35.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        PING,
        "Ping",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &["Off", "On"],
        on_off_text,
        on_off_parse,
    ),
    param(
        LOW_CUT,
        "Low cut",
        "Hz",
        0.0,
        2000.0,
        Taper::Power(2.0),
        0.0,
        0,
        &[],
        low_cut_text,
        low_cut_parse,
    ),
    param(
        HIGH_CUT,
        "High cut",
        "Hz",
        200.0,
        20_000.0,
        Taper::Power(2.0),
        20_000.0,
        0,
        &[],
        high_cut_text,
        high_cut_parse,
    ),
    param(
        MIX,
        "Mix",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        30.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        FREEZE,
        "Freeze",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &["Off", "On"],
        on_off_text,
        on_off_parse,
    ),
];

/// The note index (0 is Free) the values hold.
pub(super) fn note_index(values: Values<'_>) -> usize {
    let x = params::normalised_of(TABLE, NOTE, values);
    (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize
}

/// The value of parameter `id` in its own units.
pub(super) fn value(values: Values<'_>, id: u32) -> f32 {
    let x = params::normalised_of(TABLE, id, values);
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

/// "375 ms", "1.20 s".
fn time_text(ms: f32) -> String {
    if ms < 999.5 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// "375", "375 ms", "1.2 s", "1200".
fn time_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-").to_lowercase();
    if let Some(rest) = t.strip_suffix(" ms").or_else(|| t.strip_suffix("ms")) {
        return rest.trim().parse::<f32>().ok().filter(|v| v.is_finite());
    }
    if let Some(rest) = t.strip_suffix(" s").or_else(|| t.strip_suffix('s')) {
        return rest
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
            .map(|s| s * 1000.0);
    }
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// "1/4", "Free".
fn note_text(x: f32) -> String {
    let index = (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize;
    NOTES[index].0.to_owned()
}

/// "1/4", "free", "quarter": the note's word, or the note it spells.
fn note_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let index = match t.as_str() {
        "free" | "off" | "ms" => 0,
        "1/32" | "thirty-second" | "32nd" => 1,
        "1/16" | "sixteenth" | "16th" => 2,
        "1/8" | "eighth" | "8th" => 3,
        "1/4" | "quarter" | "4th" => 4,
        "1/2" | "half" => 5,
        "1/1" | "whole" => 6,
        _ => return None,
    };
    Some(index as f32 / (NOTES.len() - 1) as f32)
}

/// "35 %".
fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

/// "35", "35 %".
fn percent_parse(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// "Off", "120 Hz".
fn low_cut_text(hz: f32) -> String {
    if hz < 0.5 {
        "Off".to_owned()
    } else if hz < 999.5 {
        format!("{hz:.0} Hz")
    } else {
        format!("{:.2} kHz", hz / 1000.0)
    }
}

/// "off", "120", "120 Hz".
fn low_cut_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(0.0);
    }
    hertz(&t)
}

/// "Off" at the top of the range, else "8.0 kHz".
fn high_cut_text(hz: f32) -> String {
    if hz >= 19_999.5 {
        "Off".to_owned()
    } else if hz < 999.5 {
        format!("{hz:.0} Hz")
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

/// "off", "8000", "8 kHz".
fn high_cut_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(20_000.0);
    }
    hertz(&t)
}

/// A frequency with or without its unit.
fn hertz(text: &str) -> Option<f32> {
    let t = text.trim();
    if let Some(rest) = t.strip_suffix("khz") {
        return rest.trim().parse::<f32>().ok().map(|k| k * 1000.0);
    }
    if let Some(rest) = t.strip_suffix("hz") {
        return rest.trim().parse::<f32>().ok();
    }
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn on_off_text(value: f32) -> String {
    if value >= 0.5 { "On" } else { "Off" }.to_owned()
}

fn on_off_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "on" | "1" => Some(1.0),
        "off" | "0" => Some(0.0),
        _ => None,
    }
}

/// The value (in its own units) of the normalised `x` of parameter `id`.
pub(super) fn curve_value(id: u32, x: f32) -> f32 {
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

/// The note index a normalised value picks.
pub(super) fn curve_note(x: f32) -> usize {
    (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize
}
