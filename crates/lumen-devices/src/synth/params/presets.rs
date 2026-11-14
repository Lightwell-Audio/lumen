//! The five presets, as the values a project holds for them: the table's
//! defaults (Poly) with each preset's own parameters over it (M8 slice B4).

use super::table::{
    AMP_ATTACK, AMP_DECAY, AMP_RELEASE, AMP_SUSTAIN, COUNT, FILTER_CUTOFF, FILTER_RESO, FILTER_VEL,
    LEVEL, OSC1_DETUNE, OSC1_PAN, OSC1_SHAPE, OSC2_DETUNE, OSC2_LEVEL, OSC2_OCTAVE, OSC2_PAN,
    OSC2_SHAPE, SHAPE_SQUARE, SHAPE_TRIANGLE, TABLE, defaults, find,
};

use crate::synth::Preset;

/// The five presets, as the values a project holds for them: the table's
/// defaults (Poly) with each preset's own parameters over it, in the table's
/// normalised units.
///
/// A preset is nothing but values, so a person can dial any control after
/// loading one and the project keeps it; the name a preset leaves in the
/// device's state is only the record of where the sound started.
pub fn preset_values(preset: crate::synth::Preset) -> Vec<(u32, f32)> {
    let mut values = [0.0; COUNT];
    preset_values_into(&mut values, preset);
    TABLE
        .iter()
        .map(|info| (info.id, values[info.id as usize - 1]))
        .collect()
}

/// The values of `preset` into `out`, in the table's order: no allocation, so
/// a program change may load one on the audio thread (M8 slice B4, review H1).
pub fn preset_values_into(out: &mut [f32; COUNT], preset: crate::synth::Preset) {
    *out = defaults();
    for &(id, value) in preset_overrides(preset) {
        if let Some(info) = find(id) {
            out[info.id as usize - 1] = info.normalised(value);
        }
    }
}

/// What `preset` changes of the table's defaults, in the table's own units.
fn preset_overrides(preset: crate::synth::Preset) -> &'static [(u32, f32)] {
    match preset {
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
    }
}
