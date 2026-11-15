use crate::effect::Values;
use crate::params::{self, ParamInfo, Taper};

pub const TIME: u32 = 1;
pub const NOTE: u32 = 2;
pub const FEEDBACK: u32 = 3;
pub const PING: u32 = 4;
pub const LOW_CUT: u32 = 5;
pub const HIGH_CUT: u32 = 6;
pub const MIX: u32 = 7;
pub const FREEZE: u32 = 8;

pub(super) const NOTES: [(&str, f32); 7] = [
    ("Free", 0.0),
    ("1/32", 0.125),
    ("1/16", 0.25),
    ("1/8", 0.5),
    ("1/4", 1.0),
    ("1/2", 2.0),
    ("1/1", 4.0),
];

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

pub(super) fn note_index(values: Values<'_>) -> usize {
    let x = params::normalised_of(TABLE, NOTE, values);
    (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize
}

pub(super) fn value(values: Values<'_>, id: u32) -> f32 {
    let x = params::normalised_of(TABLE, id, values);
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

fn time_text(ms: f32) -> String {
    if ms < 999.5 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

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

fn note_text(x: f32) -> String {
    let index = (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize;
    NOTES[index].0.to_owned()
}

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

fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

fn percent_parse(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn low_cut_text(hz: f32) -> String {
    if hz < 0.5 {
        "Off".to_owned()
    } else if hz < 999.5 {
        format!("{hz:.0} Hz")
    } else {
        format!("{:.2} kHz", hz / 1000.0)
    }
}

fn low_cut_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(0.0);
    }
    hertz(&t)
}

fn high_cut_text(hz: f32) -> String {
    if hz >= 19_999.5 {
        "Off".to_owned()
    } else if hz < 999.5 {
        format!("{hz:.0} Hz")
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

fn high_cut_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(20_000.0);
    }
    hertz(&t)
}

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

pub(super) fn curve_value(id: u32, x: f32) -> f32 {
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

pub(super) fn curve_note(x: f32) -> usize {
    (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize
}
