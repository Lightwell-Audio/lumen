//! Flare's controls (M8 slice B4): the parameter table a project saves, the
//! words a value reads as, and the five presets as parameter sets.
//!
//! Each parameter has a stable `u32` id that is never reused (a new meaning
//! gets a new id), the name a person reads, the plain name a drawn face puts
//! in a tooltip ([`SHORT`], the round-2 labels), its range and taper, its
//! default and how its value reads and is read back. The table is ordered by
//! id, so a parameter's place in a voice's values is its id less one, and
//! nothing in the audio thread has to search for it.
//!
//! The five presets are **parameter sets**: [`preset_values`] gives the
//! values a project holds for one, in the table's own normalised units, so a
//! preset is exactly what a person could have dialled by hand, and changing
//! any control afterwards is an ordinary edit to the device's values.

use crate::effect::{Description, Values};
use crate::params::{ParamInfo, Taper};

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
const DIVISION_WORDS: &[&str] = &["1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/4T", "1/8T"];

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

/// [`ParamInfo::value`] worked out in `f64` (a taper's power is a power, not
/// the library's `powf`), and snapped to a choice's steps.
pub fn taper_value(taper: Taper, (low, high): (f32, f32), x: f32) -> f32 {
    let x = f64::from(x.clamp(0.0, 1.0));
    let (low, high) = (f64::from(low), f64::from(high));
    let value = match taper {
        Taper::Linear => low + (high - low) * x,
        Taper::Power(power) => {
            let power = f64::from(power);
            // A whole power is a power: no library call, the same bits
            // everywhere. The tapers this build uses are 2 and 3.
            let scale = if (power - power.round()).abs() < 1e-9 {
                let mut out = 1.0;
                for _ in 0..(power.round() as i32).max(0) {
                    out *= x;
                }
                out
            } else {
                x.powf(power)
            };
            low + (high - low) * scale
        }
    };
    value as f32
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

/// The five presets, as the values a project holds for them: the table's
/// defaults (Poly) with each preset's own parameters over it, in the table's
/// normalised units.
///
/// A preset is nothing but values, so a person can dial any control after
/// loading one and the project keeps it; the name a preset leaves in the
/// device's state is only the record of where the sound started.
pub fn preset_values(preset: super::Preset) -> Vec<(u32, f32)> {
    use super::Preset;
    let mut values = defaults();
    let over: &[(u32, f32)] = match preset {
        // Poly is the table's defaults (the owner's S1): two saws detuned ±7
        // cents, low-pass at 2.4 kHz with a little velocity to cutoff, A 5 ms,
        // D 300 ms, S 70 %, R 250 ms.
        Preset::Poly => &[],
        // A triangle and its octave, brighter and shorter, with a longer
        // decay and a lower sustain.
        Preset::Keys => &[
            (OSC1_SHAPE, SHAPE_TRIANGLE),
            (OSC1_DETUNE, -4.0),
            (OSC1_PAN, -20.0),
            (OSC2_SHAPE, SHAPE_TRIANGLE),
            (OSC2_OCTAVE, 1.0),
            (OSC2_DETUNE, 3.0),
            (OSC2_LEVEL, 60.0),
            (OSC2_PAN, 20.0),
            (FILTER_CUTOFF, 5_000.0),
            (FILTER_RESO, 0.7),
            (FILTER_VEL, 1.0),
            (AMP_ATTACK, 0.002),
            (AMP_DECAY, 0.7),
            (AMP_SUSTAIN, 35.0),
            (AMP_RELEASE, 0.2),
            (LEVEL, 24.0),
        ],
        // A saw with a square an octave under it, a low resonant corner and a
        // strong velocity to cutoff.
        Preset::Bass => &[
            (OSC1_DETUNE, 0.0),
            (OSC1_PAN, 0.0),
            (OSC2_SHAPE, SHAPE_SQUARE),
            (OSC2_OCTAVE, -1.0),
            (OSC2_DETUNE, 0.0),
            (OSC2_LEVEL, 70.0),
            (OSC2_PAN, 0.0),
            (FILTER_CUTOFF, 700.0),
            (FILTER_RESO, 1.6),
            (FILTER_VEL, 1.2),
            (AMP_ATTACK, 0.002),
            (AMP_DECAY, 0.2),
            (AMP_SUSTAIN, 80.0),
            (AMP_RELEASE, 0.1),
            (LEVEL, 22.0),
        ],
        // Two saws a little apart, a bright corner and no sustain: the note
        // decays away as a pluck does.
        Preset::Pluck => &[
            (OSC1_DETUNE, -5.0),
            (OSC1_PAN, -30.0),
            (OSC2_DETUNE, 5.0),
            (OSC2_LEVEL, 80.0),
            (OSC2_PAN, 30.0),
            (FILTER_CUTOFF, 3_500.0),
            (FILTER_RESO, 1.0),
            (FILTER_VEL, 1.0),
            (AMP_ATTACK, 0.001),
            (AMP_DECAY, 0.25),
            (AMP_SUSTAIN, 0.0),
            (AMP_RELEASE, 0.15),
            (LEVEL, 20.0),
        ],
        // Two saws twelve cents either side, a dark corner, a slow attack and
        // a long release.
        Preset::Pad => &[
            (OSC1_DETUNE, -12.0),
            (OSC1_PAN, -50.0),
            (OSC2_DETUNE, 12.0),
            (OSC2_PAN, 50.0),
            (FILTER_CUTOFF, 1_600.0),
            (FILTER_RESO, 0.7),
            (FILTER_VEL, 0.3),
            (AMP_ATTACK, 0.4),
            (AMP_DECAY, 0.5),
            (AMP_SUSTAIN, 80.0),
            (AMP_RELEASE, 0.9),
            (LEVEL, 14.0),
        ],
    };
    for &(id, value) in over {
        if let Some(info) = find(id)
            && let Some(slot) = values.get_mut(info.id as usize - 1)
        {
            *slot = info.normalised(value);
        }
    }
    TABLE
        .iter()
        .map(|info| (info.id, values[info.id as usize - 1]))
        .collect()
}

/// The words of each division, for the table.
fn division_words() -> &'static [&'static str] {
    DIVISION_WORDS
}

/// "Saw", "Square": the waveform's name.
fn shape_text(v: f32) -> String {
    word(SHAPES, v, "Saw")
}

/// "−1", "+1": the octave.
fn octave_text(v: f32) -> String {
    let n = v.round() as i32;
    if n > 0 {
        format!("+{n}")
    } else {
        format!("{n}")
    }
}

/// "0.80": the resonance.
fn decimal_text(v: f32) -> String {
    format!("{v:.2}")
}

/// "2.40 kHz", "420 Hz".
fn frequency_text(v: f32) -> String {
    if v >= 1_000.0 {
        format!("{:.2} kHz", v / 1_000.0)
    } else {
        format!("{:.0} Hz", v)
    }
}

/// "5.0 ms", "380 ms", "1.20 s".
fn seconds_text(v: f32) -> String {
    if v < 0.1 {
        format!("{:.1} ms", v * 1_000.0)
    } else if v < 1.0 {
        format!("{:.0} ms", v * 1_000.0)
    } else {
        format!("{v:.2} s")
    }
}

/// "Off" at nothing, then a time.
fn glide_text(v: f32) -> String {
    if v <= 0.0 {
        "Off".to_owned()
    } else {
        seconds_text(v)
    }
}

/// "16 %".
fn percent_text(v: f32) -> String {
    format!("{:.0} %", v)
}

/// "−7 ct", "+7 ct".
fn cents_text(v: f32) -> String {
    format!("{}{:.0} ct", sign(v), v.abs())
}

/// "−0.6 oct", "+2.0 oct".
fn signed_octaves_text(v: f32) -> String {
    format!("{}{:.1} oct", sign(v), v.abs())
}

/// "0.60 oct".
fn octaves_text(v: f32) -> String {
    format!("{v:.2} oct")
}

/// "L35", "C", "R20".
fn pan_text(v: f32) -> String {
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
fn hertz_text(v: f32) -> String {
    format!("{v:.2} Hz")
}

/// "16".
fn voices_text(v: f32) -> String {
    format!("{:.0}", v.round())
}

/// "Low 12", "Band 12": the filter's shape.
fn filter_text(v: f32) -> String {
    word(FILTERS, v, "Low 12")
}

/// "Pitch", "Cutoff": what the LFO moves.
fn target_text(v: f32) -> String {
    word(TARGETS, v, "Off")
}

/// "1/4": the LFO's cycle.
fn division_text(v: f32) -> String {
    word(division_words(), v, "1/4")
}

/// "Free", "Sync".
fn sync_text(v: f32) -> String {
    if v >= 0.5 { "Sync" } else { "Free" }.to_owned()
}

/// The word of a choice at `value`, or `fallback`.
fn word(words: &[&str], value: f32, fallback: &str) -> String {
    let index = value.round() as i32;
    usize::try_from(index)
        .ok()
        .and_then(|i| words.get(i))
        .map_or_else(|| fallback.to_owned(), |w| (*w).to_owned())
}

/// A minus sign as the type uses it.
fn sign(v: f32) -> &'static str {
    if v < 0.0 { "\u{2212}" } else { "+" }
}

/// The number in `text`, with the minus sign the type uses and a unit after
/// it allowed.
fn number(text: &str) -> Option<f32> {
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
fn unit_scale(unit: &str) -> f32 {
    match unit {
        "khz" | "k" => 1_000.0,
        "ms" => 0.001,
        _ => 1.0,
    }
}

fn shape_parse(text: &str) -> Option<f32> {
    by_word(SHAPES, text)
}

fn filter_parse(text: &str) -> Option<f32> {
    by_word(FILTERS, text)
}

fn target_parse(text: &str) -> Option<f32> {
    by_word(TARGETS, text)
}

fn division_parse(text: &str) -> Option<f32> {
    let text = text.trim().to_lowercase();
    division_words()
        .iter()
        .position(|w| w.to_lowercase() == text)
        .map(|i| i as f32)
}

fn sync_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "sync" | "on" => Some(1.0),
        "free" | "off" => Some(0.0),
        _ => None,
    }
}

fn octave_parse(text: &str) -> Option<f32> {
    number(text).map(|v| v.round())
}

fn decimal_parse(text: &str) -> Option<f32> {
    number(text)
}

fn frequency_parse(text: &str) -> Option<f32> {
    number(text)
}

fn seconds_parse(text: &str) -> Option<f32> {
    number(text)
}

fn glide_parse(text: &str) -> Option<f32> {
    if text.trim().eq_ignore_ascii_case("off") {
        return Some(0.0);
    }
    number(text)
}

fn percent_parse(text: &str) -> Option<f32> {
    number(text)
}

fn cents_parse(text: &str) -> Option<f32> {
    number(text)
}

fn signed_octaves_parse(text: &str) -> Option<f32> {
    number(text)
}

fn octaves_parse(text: &str) -> Option<f32> {
    number(text)
}

fn hertz_parse(text: &str) -> Option<f32> {
    number(text)
}

fn voices_parse(text: &str) -> Option<f32> {
    number(text).map(|v| v.round())
}

fn pan_parse(text: &str) -> Option<f32> {
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

fn by_word(words: &[&str], text: &str) -> Option<f32> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamInfo;

    #[test]
    fn the_table_is_ordered_by_id_and_names_every_parameter() {
        for (i, info) in TABLE.iter().enumerate() {
            assert_eq!(info.id as usize, i + 1, "{} is out of place", info.name);
            assert!(!info.name.is_empty());
            assert!(!short_label(info.id).is_empty());
        }
        assert_eq!(TABLE.len(), COUNT);
        assert!(find(0).is_none(), "0 is not a parameter");
        assert!(find(COUNT as u32 + 1).is_none(), "past the table");
        assert!(find(COUNT as u32).is_some(), "the last one is");
        assert_eq!(find(OSC1_SHAPE).map(|p| p.name), Some("Oscillator 1 shape"));
    }

    #[test]
    fn a_value_reads_and_is_read_back() {
        for info in TABLE {
            let value = info.default;
            let text = info.text_of(info.normalised(value));
            let back = info
                .value_of(&text)
                .unwrap_or_else(|| panic!("{} does not read {text:?}", info.name));
            let read = info.value(back);
            assert!(
                (read - value).abs() <= (value.abs() * 0.02).max(0.002),
                "{}: {value} reads {text:?} and comes back {read}",
                info.name
            );
        }
        // And a few by hand, in the words a person types.
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        let cutoff = find(FILTER_CUTOFF).unwrap();
        assert!(near(
            cutoff.value_of("2.4 kHz").unwrap(),
            cutoff.normalised(2_400.0)
        ));
        let attack = find(AMP_ATTACK).unwrap();
        assert!(near(
            attack.value_of("5 ms").unwrap(),
            attack.normalised(0.005)
        ));
        assert_eq!(find(OSC1_SHAPE).unwrap().value_of("square"), Some(1.0));
        assert_eq!(
            find(LFO_TARGET).unwrap().value_of("Cutoff"),
            Some(2.0 / 3.0)
        );
        assert_eq!(find(GLIDE).unwrap().value_of("Off"), Some(0.0));
        let pan = find(OSC1_PAN).unwrap();
        assert!(near(pan.value_of("L35").unwrap(), pan.normalised(-35.0)));
        assert!(find(FILTER_CUTOFF).unwrap().value_of("bright").is_none());
    }

    /// The table's defaults are Poly, exactly: a state the project does not
    /// name sounds as Lumen always has.
    #[test]
    fn poly_is_the_table_s_defaults() {
        let poly = preset_values(super::super::Preset::Poly);
        for info in TABLE {
            let (id, value) = poly[(info.id - 1) as usize];
            assert_eq!(id, info.id);
            assert_eq!(value, info.normalised(info.default), "{}", info.name);
        }
    }

    /// A preset is a set of values and nothing else: the same values twice
    /// are the same preset, and each of the five differs from the others.
    #[test]
    fn the_presets_are_values() {
        use super::super::Preset;
        for preset in Preset::ALL {
            let values = preset_values(preset);
            assert_eq!(values.len(), COUNT);
            let again = preset_values(preset);
            assert_eq!(values, again);
            for (id, value) in values {
                assert!((0.0..=1.0).contains(&value), "{} {id}", preset.name());
            }
        }
        let poly = preset_values(Preset::Poly);
        for preset in Preset::ALL {
            if preset == Preset::Poly {
                continue;
            }
            assert_ne!(preset_values(preset), poly, "{}", preset.name());
        }
    }

    /// The round-2 labels are data, and the plain names are the table's.
    #[test]
    fn the_labels_are_the_round_two_ones() {
        assert_eq!(short_label(OSC1_SHAPE), "Shape");
        assert_eq!(short_label(FILTER_CUTOFF), "Cutoff");
        assert_eq!(short_label(FILTER_RESO), "Reso");
        assert_eq!(short_label(AMP_ATTACK), "Attack");
        assert_eq!(short_label(AMP_RELEASE), "Release");
        // The plain name stays for a tooltip, All parameters and a lane.
        assert_eq!(find(AMP_ATTACK).unwrap().name, "Amp attack");
        assert_eq!(find(FILTER_RESO).unwrap().name, "Resonance");
        // A parameter with no dial of its own says its plain name.
        assert_eq!(short_label(OSC1_DETUNE), "Oscillator 1 detune");
    }

    #[test]
    fn a_taper_s_power_is_a_power() {
        let range = (20.0, 20_000.0);
        let x = 0.5;
        let cube = 20.0 + 19_980.0 * 0.125;
        assert!((taper_value(Taper::Power(3.0), range, x) - cube).abs() < 1e-3);
        let square = 20.0 + 19_980.0 * 0.25;
        assert!((taper_value(Taper::Power(2.0), range, x) - square).abs() < 1e-3);
        assert_eq!(taper_value(Taper::Linear, range, x), 10_010.0);
        // A choice snaps to its steps.
        assert_eq!(snapped(LFO_TARGET, 0.6), 2.0);
        assert_eq!(snapped(VOICES, 0.0), 1.0);
        assert_eq!(snapped(VOICES, 1.0), 16.0);
    }

    /// The description is the table, and Flare reports no timing of its own.
    #[test]
    fn the_description_is_the_table() {
        assert_eq!(DESCRIPTION.name, NAME);
        assert_eq!(DESCRIPTION.table.len(), COUNT);
        let defaults = defaults();
        let values: Vec<(u32, f32)> = TABLE
            .iter()
            .map(|p| (p.id, defaults[(p.id - 1) as usize]))
            .collect();
        assert_eq!(latency(&values), 0);
        assert_eq!(tail(&values), 0);
    }

    /// The taper works the arithmetic, not the library's `powf`.
    #[test]
    fn a_power_taper_is_the_same_bits_every_time() {
        let whatever = |k: u32| {
            let x = k as f32 / 255.0;
            taper_value(Taper::Power(3.0), (20.0, 20_000.0), x).to_bits()
        };
        let first: Vec<u32> = (0..256).map(whatever).collect();
        let again: Vec<u32> = (0..256).map(whatever).collect();
        assert_eq!(first, again);
        // The words of a stepped parameter are the table's own list.
        let target: &ParamInfo = find(LFO_TARGET).unwrap();
        assert_eq!(target.choices, TARGETS);
        assert_eq!(division_words().len(), DIVISIONS.len());
        for (i, (name, _)) in DIVISIONS.iter().enumerate() {
            assert_eq!(find(LFO_DIVISION).unwrap().choice(i), Some(*name));
        }
    }
}
