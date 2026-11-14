//! What the table, its words and its presets must always be (M8 slice B4).

use super::*;
use crate::params::{ParamInfo, Taper};
use crate::synth::Preset;

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
    let poly = preset_values(Preset::Poly);
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
    assert_eq!(DIVISION_WORDS.len(), DIVISIONS.len());
    for (i, (name, _)) in DIVISIONS.iter().enumerate() {
        assert_eq!(find(LFO_DIVISION).unwrap().choice(i), Some(*name));
    }
}
