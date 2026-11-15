use super::table::{
    AMP_ATTACK, AMP_DECAY, AMP_RELEASE, AMP_SUSTAIN, COUNT, FILTER_CUTOFF, FILTER_RESO, FILTER_VEL,
    LEVEL, OSC1_DETUNE, OSC1_PAN, OSC1_SHAPE, OSC2_DETUNE, OSC2_LEVEL, OSC2_OCTAVE, OSC2_PAN,
    OSC2_SHAPE, SHAPE_SQUARE, SHAPE_TRIANGLE, TABLE, defaults, find,
};

use crate::synth::Preset;

pub fn preset_values(preset: crate::synth::Preset) -> Vec<(u32, f32)> {
    let mut values = [0.0; COUNT];
    preset_values_into(&mut values, preset);
    TABLE
        .iter()
        .map(|info| (info.id, values[info.id as usize - 1]))
        .collect()
}

pub fn preset_values_into(out: &mut [f32; COUNT], preset: crate::synth::Preset) {
    *out = defaults();
    for &(id, value) in preset_overrides(preset) {
        if let Some(info) = find(id) {
            out[info.id as usize - 1] = info.normalised(value);
        }
    }
}

fn preset_overrides(preset: crate::synth::Preset) -> &'static [(u32, f32)] {
    match preset {
        Preset::Poly => &[],
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
