//! Flare's table of parameters: the ids, the names, the ranges and defaults,
//! and the conversions the card and the render path read (M8 slice B4).
//!
//! The table is ordered by id, so a parameter's place in a voice's values is
//! its id less one and nothing on the audio thread has to search for it. The
//! words a value reads as are in [`super::words`]; the five presets are in
//! [`super::presets`].

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

/// The device's stable name in `PluginRef::Builtin`, as a project saves it.
/// Never the product name, and never changed.
pub const NAME: &str = "synth";

/// The label a drawn face's dial shows for `id` (the round-2 idea names,
/// `docs/specimens/m8/devices.html`), and the plain name in its tooltip. The
/// table's own name is the plain one: "Shape" on the dial of oscillator 1's
/// shape, "Oscillator 1 shape" in All parameters, a lane and a tooltip.
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

/// What the dial for `id` says: the round-2 label where the specimen names
/// one, and the table's plain name otherwise.
pub fn short_label(id: u32) -> &'static str {
    SHORT.iter().find(|(known, _)| *known == id).map_or_else(
        || find(id).map_or("", |info| info.name),
        |&(_, label)| label,
    )
}

/// Oscillator 1's waveform.
pub const OSC1_SHAPE: u32 = 1;
/// Oscillator 1's octave, −2 to +2.
pub const OSC1_OCTAVE: u32 = 2;
/// Oscillator 1's detune, in cents.
pub const OSC1_DETUNE: u32 = 3;
/// Oscillator 1's level, in per cent.
pub const OSC1_LEVEL: u32 = 4;
/// Oscillator 1's place in the stereo field, −100 to +100.
pub const OSC1_PAN: u32 = 5;
/// Oscillator 2's waveform.
pub const OSC2_SHAPE: u32 = 6;
/// Oscillator 2's octave, −2 to +2.
pub const OSC2_OCTAVE: u32 = 7;
/// Oscillator 2's detune, in cents.
pub const OSC2_DETUNE: u32 = 8;
/// Oscillator 2's level, in per cent.
pub const OSC2_LEVEL: u32 = 9;
/// Oscillator 2's place in the stereo field, −100 to +100.
pub const OSC2_PAN: u32 = 10;
/// The noise generator's level, in per cent.
pub const NOISE: u32 = 11;
/// The filter's shape: low-pass 12 or 24 dB, high-pass or band-pass.
pub const FILTER_TYPE: u32 = 12;
/// The filter's corner, in Hz (before the envelopes and key follow move it).
pub const FILTER_CUTOFF: u32 = 13;
/// The filter's resonance, as its Q.
pub const FILTER_RESO: u32 = 14;
/// How far the filter envelope opens the filter, in octaves.
pub const FILTER_ENV: u32 = 15;
/// How far the filter follows the key played, 0 to 100 %: a full follow
/// moves the corner an octave an octave.
pub const FILTER_KEY: u32 = 16;
/// How far velocity opens the filter, in octaves, either side of the middle.
pub const FILTER_VEL: u32 = 17;
/// The amp envelope's attack, in seconds.
pub const AMP_ATTACK: u32 = 18;
/// The amp envelope's decay, in seconds.
pub const AMP_DECAY: u32 = 19;
/// The amp envelope's sustain, in per cent.
pub const AMP_SUSTAIN: u32 = 20;
/// The amp envelope's release, in seconds.
pub const AMP_RELEASE: u32 = 21;
/// The filter envelope's attack, in seconds.
pub const FILTER_ATTACK: u32 = 22;
/// The filter envelope's decay, in seconds.
pub const FILTER_DECAY: u32 = 23;
/// The filter envelope's sustain, in per cent.
pub const FILTER_SUSTAIN: u32 = 24;
/// The filter envelope's release, in seconds.
pub const FILTER_RELEASE: u32 = 25;
/// Whether the LFO follows the tempo (`Sync`) or its own rate (`Free`).
pub const LFO_SYNC: u32 = 26;
/// The LFO's rate in Hz, when it is free.
pub const LFO_RATE: u32 = 27;
/// The LFO's cycle, in notes, when it follows the tempo.
pub const LFO_DIVISION: u32 = 28;
/// What the LFO moves: pitch, cutoff or amp.
pub const LFO_TARGET: u32 = 29;
/// How far the LFO moves it, in per cent of its reach.
pub const LFO_DEPTH: u32 = 30;
/// Portamento: the time a new note takes to reach its pitch from the last.
pub const GLIDE: u32 = 31;
/// How many notes sound at once, 1 to 16.
pub const VOICES: u32 = 32;
/// How much velocity changes the level, 0 to 100 %.
pub const VELOCITY: u32 = 33;
/// The synth's output level, in per cent.
pub const LEVEL: u32 = 34;

/// How many parameters the table holds, and how many values a voice keeps.
pub const COUNT: usize = 34;

/// The waveforms, in the order of [`OSC1_SHAPE`]'s choices.
pub const SHAPES: &[&str] = &["Sine", "Triangle", "Saw", "Square"];
/// The filter shapes, in the order of [`FILTER_TYPE`]'s choices.
pub const FILTERS: &[&str] = &["Low 12", "Low 24", "High 12", "Band 12"];
/// What the LFO moves, in the order of [`LFO_TARGET`]'s choices.
pub const TARGETS: &[&str] = &["Off", "Pitch", "Cutoff", "Amp"];
/// The words of [`LFO_DIVISION`]'s choices, in order.
pub(crate) const DIVISION_WORDS: &[&str] =
    &["1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/4T", "1/8T"];

/// The LFO's cycles, in notes, and how many quarter notes each is worth: the
/// order of [`LFO_DIVISION`]'s choices.
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
/// The octaves [`OSC1_OCTAVE`] offers, lowest first.
pub const OCTAVES: &[&str] = &["-2", "-1", "0", "+1", "+2"];

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

/// Flare's parameters, in the order the card shows them: the two
/// oscillators, the noise, the filter and its envelope, the amp envelope, the
/// LFO, and the voice settings. Its defaults are the **Poly** preset (the
/// owner's S1), so a device whose values a project does not name sounds as
/// Lumen always has.
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
    // LFO_RATE follows; the division's words are the frozen list above.
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

/// The waveform value of a sine.
pub const SHAPE_SINE: f32 = 0.0;
/// The waveform value of a triangle.
pub const SHAPE_TRIANGLE: f32 = 1.0;
/// The waveform value of a saw.
pub const SHAPE_SAW: f32 = 2.0;
/// The waveform value of a square.
pub const SHAPE_SQUARE: f32 = 3.0;

/// Flare's description: its table, and a timing no value of it moves (an
/// instrument is not held back by anything and its voices ring out through
/// the engine's own `active`).
pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

/// Frames Flare's output trails its input: none, whatever its values say.
pub fn latency(_values: Values<'_>) -> u32 {
    0
}

/// Frames Flare rings on after its input stops: none of its own, since the
/// engine keeps rendering a voice until its release ends.
pub fn tail(_values: Values<'_>) -> u32 {
    0
}

/// The parameter `id`, if the table names it.
pub fn find(id: u32) -> Option<&'static ParamInfo> {
    TABLE
        .get((id as usize).checked_sub(1)?)
        .filter(|p| p.id == id)
}

/// Every parameter at its default, normalised: the values a device nothing
/// has set plays (Poly).
pub fn defaults() -> [f32; COUNT] {
    let mut values = [0.0; COUNT];
    for (slot, info) in values.iter_mut().zip(TABLE) {
        *slot = info.normalised(info.default);
    }
    values
}

/// The values of a parameter as the DSP works with it, from its normalised
/// value: the taper in `f64`, so a value is the same on every platform.
pub fn dsp_value(id: u32, x: f32) -> f32 {
    let Some(info) = find(id) else {
        return 0.0;
    };
    taper_value(info.taper, info.range(), x)
}

/// [`ParamInfo::value`], worked out by the table's own `Taper`, which takes a
/// power in `f64` and rounds once (review L5: nothing on the render path is an
/// `f32` transcendental, and no `f32` `powf` is reachable at all).
pub fn taper_value(taper: Taper, range: (f32, f32), x: f32) -> f32 {
    taper.value(range, x)
}

/// A choice's steps of `id`, as a `f32` count.
fn steps_of(info: &ParamInfo) -> f32 {
    f32::from(i16::try_from(info.steps.max(1)).unwrap_or(i16::MAX))
}

// The conversions the render path makes every frame, written out so the audio
// thread does no table search and no library call: each is the table's own
// range and taper, which `the_fast_conversions_are_the_table_s` pins.

/// The filter's corner from its normalised value, in Hz.
#[inline(always)]
pub fn cutoff_hz(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    // Power(3) over 20 Hz to 20 kHz.
    20.0 + 19_980.0 * (x * x * x)
}

/// The filter's Q from its normalised value.
#[inline(always)]
pub fn resonance(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    // Power(2) over 0.3 to 8.
    0.3 + 7.7 * (x * x)
}

/// A 0 to 100 % control's value from its normalised value: per cent.
#[inline(always)]
pub fn percent(x: f32) -> f32 {
    x.clamp(0.0, 1.0) * 100.0
}

/// The filter envelope's amount, −5 to +5 octaves.
#[inline(always)]
pub fn signed_octaves(x: f32) -> f32 {
    -5.0 + 10.0 * x.clamp(0.0, 1.0)
}

/// A 0 to 100 % control as a fraction: the key follow, a level.
#[inline(always)]
pub fn key_follow(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// Velocity's opening of the filter, 0 to 2 octaves.
#[inline(always)]
pub fn vel_octaves(x: f32) -> f32 {
    2.0 * x.clamp(0.0, 1.0)
}

/// The parameter `id`'s value at normalised `x`, snapped to a choice.
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
