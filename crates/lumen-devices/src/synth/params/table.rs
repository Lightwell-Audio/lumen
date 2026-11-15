use crate::effect::{Description, Values};
use crate::params::{ParamInfo, Taper};

use super::words::{
    cents_parse, cents_text, decimal_parse, decimal_text, division_parse, division_text,
    filter_parse, filter_text, frequency_parse, frequency_text, glide_parse, glide_text,
    hertz_parse, hertz_text, octave_parse, octave_text, octaves_parse, octaves_text, pan_parse,
    pan_text, percent_parse, percent_text, seconds_parse, seconds_text, shape_parse, shape_text,
    signed_octaves_parse, signed_octaves_text, sync_parse, sync_text, target_parse, target_text,
    voices_parse, voices_text,
};

pub const NAME: &str = "synth";

pub const SHORT: &[(u32, &str)] = &[
    (OSC1_SHAPE, "Shape"),
    (OSC1_LEVEL, "Level"),
    (NOISE, "Noise"),
    (FILTER_CUTOFF, "Cutoff"),
    (FILTER_RESO, "Reso"),
    (FILTER_ENV, "Env"),
    (AMP_ATTACK, "Attack"),
    (AMP_RELEASE, "Release"),
    (LFO_RATE, "Rate"),
    (LFO_DEPTH, "Depth"),
    (GLIDE, "Glide"),
    (VOICES, "Voices"),
];

pub fn short_label(id: u32) -> &'static str {
    SHORT.iter().find(|(known, _)| *known == id).map_or_else(
        || find(id).map_or("", |info| info.name),
        |&(_, label)| label,
    )
}

pub const OSC1_SHAPE: u32 = 1;
pub const OSC1_OCTAVE: u32 = 2;
pub const OSC1_DETUNE: u32 = 3;
pub const OSC1_LEVEL: u32 = 4;
pub const OSC1_PAN: u32 = 5;
pub const OSC2_SHAPE: u32 = 6;
pub const OSC2_OCTAVE: u32 = 7;
pub const OSC2_DETUNE: u32 = 8;
pub const OSC2_LEVEL: u32 = 9;
pub const OSC2_PAN: u32 = 10;
pub const NOISE: u32 = 11;
pub const FILTER_TYPE: u32 = 12;
pub const FILTER_CUTOFF: u32 = 13;
pub const FILTER_RESO: u32 = 14;
pub const FILTER_ENV: u32 = 15;
pub const FILTER_KEY: u32 = 16;
pub const FILTER_VEL: u32 = 17;
pub const AMP_ATTACK: u32 = 18;
pub const AMP_DECAY: u32 = 19;
pub const AMP_SUSTAIN: u32 = 20;
pub const AMP_RELEASE: u32 = 21;
pub const FILTER_ATTACK: u32 = 22;
pub const FILTER_DECAY: u32 = 23;
pub const FILTER_SUSTAIN: u32 = 24;
pub const FILTER_RELEASE: u32 = 25;
pub const LFO_SYNC: u32 = 26;
pub const LFO_RATE: u32 = 27;
pub const LFO_DIVISION: u32 = 28;
pub const LFO_TARGET: u32 = 29;
pub const LFO_DEPTH: u32 = 30;
pub const GLIDE: u32 = 31;
pub const VOICES: u32 = 32;
pub const VELOCITY: u32 = 33;
pub const LEVEL: u32 = 34;

pub const COUNT: usize = 34;

pub const SHAPES: &[&str] = &["Sine", "Triangle", "Saw", "Square"];
pub const FILTERS: &[&str] = &["Low 12", "Low 24", "High 12", "Band 12"];
pub const TARGETS: &[&str] = &["Off", "Pitch", "Cutoff", "Amp"];
pub(crate) const DIVISION_WORDS: &[&str] =
    &["1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/4T", "1/8T"];

pub const DIVISIONS: &[(&str, f32)] = &[
    ("1/1", 4.0),
    ("1/2", 2.0),
    ("1/4", 1.0),
    ("1/8", 0.5),
    ("1/16", 0.25),
    ("1/32", 0.125),
    ("1/4T", 2.0 / 3.0),
    ("1/8T", 1.0 / 3.0),
];
pub const OCTAVES: &[&str] = &["-2", "-1", "0", "+1", "+2"];

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
        OSC1_SHAPE,
        "Oscillator 1 shape",
        "",
        0.0,
        3.0,
        Taper::Linear,
        SHAPE_SAW,
        3,
        SHAPES,
        shape_text,
        shape_parse,
    ),
    param(
        OSC1_OCTAVE,
        "Oscillator 1 octave",
        "",
        -2.0,
        2.0,
        Taper::Linear,
        0.0,
        4,
        OCTAVES,
        octave_text,
        octave_parse,
    ),
    param(
        OSC1_DETUNE,
        "Oscillator 1 detune",
        "ct",
        -50.0,
        50.0,
        Taper::Linear,
        -7.0,
        0,
        &[],
        cents_text,
        cents_parse,
    ),
    param(
        OSC1_LEVEL,
        "Oscillator 1 level",
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
        OSC1_PAN,
        "Oscillator 1 pan",
        "",
        -100.0,
        100.0,
        Taper::Linear,
        -35.0,
        0,
        &[],
        pan_text,
        pan_parse,
    ),
    param(
        OSC2_SHAPE,
        "Oscillator 2 shape",
        "",
        0.0,
        3.0,
        Taper::Linear,
        SHAPE_SAW,
        3,
        SHAPES,
        shape_text,
        shape_parse,
    ),
    param(
        OSC2_OCTAVE,
        "Oscillator 2 octave",
        "",
        -2.0,
        2.0,
        Taper::Linear,
        0.0,
        4,
        OCTAVES,
        octave_text,
        octave_parse,
    ),
    param(
        OSC2_DETUNE,
        "Oscillator 2 detune",
        "ct",
        -50.0,
        50.0,
        Taper::Linear,
        7.0,
        0,
        &[],
        cents_text,
        cents_parse,
    ),
    param(
        OSC2_LEVEL,
        "Oscillator 2 level",
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
        OSC2_PAN,
        "Oscillator 2 pan",
        "",
        -100.0,
        100.0,
        Taper::Linear,
        35.0,
        0,
        &[],
        pan_text,
        pan_parse,
    ),
    param(
        NOISE,
        "Noise",
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
        FILTER_TYPE,
        "Filter type",
        "",
        0.0,
        3.0,
        Taper::Linear,
        0.0,
        3,
        FILTERS,
        filter_text,
        filter_parse,
    ),
    param(
        FILTER_CUTOFF,
        "Filter cutoff",
        "Hz",
        20.0,
        20_000.0,
        Taper::Power(3.0),
        2_400.0,
        0,
        &[],
        frequency_text,
        frequency_parse,
    ),
    param(
        FILTER_RESO,
        "Resonance",
        "",
        0.3,
        8.0,
        Taper::Power(2.0),
        0.8,
        0,
        &[],
        decimal_text,
        decimal_parse,
    ),
    param(
        FILTER_ENV,
        "Filter envelope",
        "oct",
        -5.0,
        5.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        signed_octaves_text,
        signed_octaves_parse,
    ),
    param(
        FILTER_KEY,
        "Filter key follow",
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
        FILTER_VEL,
        "Filter velocity",
        "oct",
        0.0,
        2.0,
        Taper::Linear,
        0.6,
        0,
        &[],
        octaves_text,
        octaves_parse,
    ),
    param(
        AMP_ATTACK,
        "Amp attack",
        "s",
        0.001,
        20.0,
        Taper::Power(3.0),
        0.005,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        AMP_DECAY,
        "Amp decay",
        "s",
        0.001,
        20.0,
        Taper::Power(3.0),
        0.3,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        AMP_SUSTAIN,
        "Amp sustain",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        70.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        AMP_RELEASE,
        "Amp release",
        "s",
        0.001,
        20.0,
        Taper::Power(3.0),
        0.25,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        FILTER_ATTACK,
        "Filter attack",
        "s",
        0.001,
        20.0,
        Taper::Power(3.0),
        0.005,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        FILTER_DECAY,
        "Filter decay",
        "s",
        0.001,
        20.0,
        Taper::Power(3.0),
        0.3,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        FILTER_SUSTAIN,
        "Filter sustain",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        50.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        FILTER_RELEASE,
        "Filter release",
        "s",
        0.001,
        20.0,
        Taper::Power(3.0),
        0.25,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        LFO_SYNC,
        "LFO sync",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &["Free", "Sync"],
        sync_text,
        sync_parse,
    ),
    param(
        LFO_RATE,
        "LFO rate",
        "Hz",
        0.05,
        20.0,
        Taper::Power(2.0),
        5.0,
        0,
        &[],
        hertz_text,
        hertz_parse,
    ),
    param(
        LFO_DIVISION,
        "LFO division",
        "",
        0.0,
        7.0,
        Taper::Linear,
        2.0,
        7,
        DIVISION_WORDS,
        division_text,
        division_parse,
    ),
    param(
        LFO_TARGET,
        "LFO target",
        "",
        0.0,
        3.0,
        Taper::Linear,
        0.0,
        3,
        TARGETS,
        target_text,
        target_parse,
    ),
    param(
        LFO_DEPTH,
        "LFO depth",
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
        GLIDE,
        "Glide",
        "s",
        0.0,
        5.0,
        Taper::Power(2.0),
        0.0,
        0,
        &[],
        glide_text,
        glide_parse,
    ),
    param(
        VOICES,
        "Voices",
        "",
        1.0,
        16.0,
        Taper::Linear,
        16.0,
        15,
        &[],
        voices_text,
        voices_parse,
    ),
    param(
        VELOCITY,
        "Velocity",
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
        LEVEL,
        "Output level",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        16.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
];

pub const SHAPE_SINE: f32 = 0.0;
pub const SHAPE_TRIANGLE: f32 = 1.0;
pub const SHAPE_SAW: f32 = 2.0;
pub const SHAPE_SQUARE: f32 = 3.0;

pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

pub fn latency(_values: Values<'_>) -> u32 {
    0
}

pub fn tail(_values: Values<'_>) -> u32 {
    0
}

pub fn find(id: u32) -> Option<&'static ParamInfo> {
    TABLE
        .get((id as usize).checked_sub(1)?)
        .filter(|p| p.id == id)
}

pub fn defaults() -> [f32; COUNT] {
    let mut values = [0.0; COUNT];
    for (slot, info) in values.iter_mut().zip(TABLE) {
        *slot = info.normalised(info.default);
    }
    values
}

pub fn dsp_value(id: u32, x: f32) -> f32 {
    let Some(info) = find(id) else {
        return 0.0;
    };
    taper_value(info.taper, info.range(), x)
}

pub fn taper_value(taper: Taper, range: (f32, f32), x: f32) -> f32 {
    taper.value(range, x)
}

fn steps_of(info: &ParamInfo) -> f32 {
    f32::from(i16::try_from(info.steps.max(1)).unwrap_or(i16::MAX))
}

#[inline(always)]
pub fn cutoff_hz(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    20.0 + 19_980.0 * (x * x * x)
}

#[inline(always)]
pub fn resonance(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    0.3 + 7.7 * (x * x)
}

#[inline(always)]
pub fn percent(x: f32) -> f32 {
    x.clamp(0.0, 1.0) * 100.0
}

#[inline(always)]
pub fn signed_octaves(x: f32) -> f32 {
    -5.0 + 10.0 * x.clamp(0.0, 1.0)
}

#[inline(always)]
pub fn key_follow(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

#[inline(always)]
pub fn vel_octaves(x: f32) -> f32 {
    2.0 * x.clamp(0.0, 1.0)
}

pub fn snapped(id: u32, x: f32) -> f32 {
    let Some(info) = find(id) else {
        return 0.0;
    };
    let x = x.clamp(0.0, 1.0);
    if info.is_choice() {
        let steps = steps_of(info);
        taper_value(info.taper, info.range(), (x * steps).round() / steps)
    } else {
        taper_value(info.taper, info.range(), x)
    }
}
