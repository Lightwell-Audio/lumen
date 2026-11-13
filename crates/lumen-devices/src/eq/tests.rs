//! Iris's own tests: the table, the words it says, and that a still Iris
//! passes everything without touching it.

use super::*;
use crate::ParamBlock;

/// The table pinned by id, name and normalised default: an id may not be
/// renamed, reused or renumbered without failing here (as Prism's is).
#[test]
fn the_parameter_table_is_pinned_by_id_name_and_default() {
    let pinned: Vec<(u32, &str, u32)> = TABLE
        .iter()
        .map(|info| (info.id, info.name, (info.normalised(info.default) * 1e6) as u32))
        .collect();
    let want: Vec<(u32, &str, u32)> = vec![
        (1, "Output", 500_000),
        (11, "1 Freq", 58_697),
        (12, "1 Slope", 333_333),
        (15, "1 On", 0),
        (21, "2 Freq", 232_990),
        (22, "2 Gain", 500_000),
        (23, "2 Q", 434_588),
        (24, "2 Type", 0),
        (25, "2 On", 1_000_000),
        (31, "3 Freq", 365_636),
        (32, "3 Gain", 500_000),
        (33, "3 Q", 434_588),
        (34, "3 Type", 0),
        (35, "3 On", 0),
        (41, "4 Freq", 499_436),
        (42, "4 Gain", 500_000),
        (43, "4 Q", 434_588),
        (44, "4 Type", 0),
        (45, "4 On", 0),
        (51, "5 Freq", 634_363),
        (52, "5 Gain", 500_000),
        (53, "5 Q", 434_588),
        (54, "5 Type", 0),
        (55, "5 On", 0),
        (61, "6 Freq", 767_010),
        (62, "6 Gain", 500_000),
        (63, "6 Q", 434_588),
        (64, "6 Type", 0),
        (65, "6 On", 0),
        (71, "7 Freq", 899_656),
        (72, "7 Gain", 500_000),
        (73, "7 Q", 434_588),
        (74, "7 Type", 0),
        (75, "7 On", 0),
        (81, "8 Freq", 984_747),
        (82, "8 Slope", 333_333),
        (85, "8 On", 0),
        (2, "Analyzer", 1_000_000),
    ];
    assert_eq!(pinned.len(), want.len(), "{pinned:?}");
    for (got, want) in pinned.iter().zip(&want) {
        assert_eq!(got.0, want.0, "id of {}", got.1);
        assert_eq!(got.1, want.1);
        assert!((got.2 as i64 - want.2 as i64).abs() <= 1, "{got:?} against {want:?}");
    }
    // No id is reused, and none is zero.
    let ids: std::collections::HashSet<u32> = TABLE.iter().map(|info| info.id).collect();
    assert_eq!(ids.len(), TABLE.len());
    assert!(!ids.contains(&0));
    // Every id a band names is either in the table or a field that band does
    // not have.
    for band in 1..=BANDS {
        for field in 1..=5 {
            let id = field_of(band, field);
            let named = TABLE.iter().any(|info| info.id == id);
            let wanted = field == 1
                || field == 5
                || (field == 2)
                || (!is_cut(band) && (field == 3 || field == 4));
            assert_eq!(named, wanted, "band {band} field {field}");
        }
    }
}

#[test]
fn a_frequency_reads_short_and_is_read_back_in_either_unit() {
    assert_eq!(hz_text(20.0), "20.0 Hz");
    assert_eq!(hz_text(40.0), "40.0 Hz");
    assert_eq!(hz_text(120.0), "120 Hz");
    assert_eq!(hz_text(1_200.0), "1.20 kHz");
    assert_eq!(hz_text(12_500.0), "12.5 kHz");
    assert_eq!(hz_text(20_000.0), "20.0 kHz");
    assert_eq!(hz_parse("120"), Some(120.0));
    assert_eq!(hz_parse("1.2 kHz"), Some(1_200.0));
    assert_eq!(hz_parse("1.2k"), Some(1_200.0));
    assert_eq!(hz_parse(" 40 hz "), Some(40.0));
    assert_eq!(hz_parse("loud"), None);
    assert_eq!(hz_parse("-40"), None);
}

#[test]
fn the_other_words_read_and_are_read_back() {
    assert_eq!(db_text(-6.0), "\u{2212}6.0 dB");
    assert_eq!(db_parse("-6 dB"), Some(-6.0));
    assert_eq!(db_parse("\u{2212}6.0"), Some(-6.0));
    assert_eq!(q_text(0.707), "0.71");
    assert_eq!(q_parse("Q 1.5"), Some(1.5));
    assert_eq!(q_parse("0"), None);
    assert_eq!(slope_text(24.0), "24/oct");
    assert_eq!(slope_parse("24 dB/oct"), Some(24.0));
    assert_eq!(slope_parse("48/oct"), Some(48.0));
    assert_eq!(kind_parse("notch"), Some(3.0));
    assert_eq!(kind_parse("Shelf high"), Some(2.0));
    assert_eq!(kind_text(1.0), "Shelf low");
    assert_eq!(kind_parse("tube"), None);
    assert_eq!(on_parse("on"), Some(1.0));
    assert_eq!(on_text(0.0), "Off");
    assert_eq!(side_parse("pre"), Some(0.0));
    assert_eq!(side_text(1.0), "Output");
    // A choice stops on its own steps.
    let kind = params::find(TABLE, type_of(2)).unwrap();
    assert_eq!(kind.value(0.3), 1.0);
    assert_eq!(kind.normalised(2.0), 2.0 / 3.0);
}

/// The face's frequency axis is the one the response is drawn on: 20 Hz to
/// 20 kHz, evenly on a log scale.
#[test]
fn the_face_s_axis_runs_from_twenty_hertz_to_twenty_kilohertz() {
    assert!((axis(20.0) - 0.0).abs() < 1e-6);
    assert!((axis(20_000.0) - 1.0).abs() < 1e-6);
    assert!((axis_hz(0.5) - 632.45).abs() < 0.1);
    for hz in [30.0, 100.0, 1_000.0, 12_340.0] {
        assert!((axis_hz(axis(hz)) - hz).abs() < 0.01, "{hz}");
    }
}

/// Nothing set: the output is the input, to the last bit, at any block size.
#[test]
fn a_fresh_iris_passes_everything_untouched() {
    for block in [64usize, 97, 128, 512] {
        let mut eq = Eq::new();
        let input: Vec<f32> = (0..block)
            .map(|i| (i as f32 * 0.37).sin() * 0.5)
            .collect();
        let mut events = ParamBlock::new();
        events.start(block);
        let mut left = input.clone();
        let mut right = input.clone();
        eq.process(
            &Block {
                rate: 48_000,
                start: 0,
                law: lumen_core::pan::PanLaw::Balance,
                events: &events,
            },
            Stereo {
                left: &mut left,
                right: &mut right,
            },
            None,
        );
        assert_eq!(left, input, "block {block}");
        assert_eq!(right, input, "block {block}");
    }
}

/// The response the face draws is the bands' settings added up: a +6 dB bell
/// at 1 kHz reads 6 dB there and nothing far from it, and the output gain is
/// added at every point.
#[test]
fn the_drawn_response_is_the_bands_added_up() {
    let mut values: Vec<(u32, f32)> = TABLE
        .iter()
        .map(|info| (info.id, info.normalised(info.default)))
        .collect();
    let set = |values: &mut Vec<(u32, f32)>, id: u32, value: f32| {
        let info = params::find(TABLE, id).unwrap();
        if let Some(slot) = values.iter_mut().find(|(known, _)| *known == id) {
            slot.1 = info.normalised(value);
        } else {
            values.push((id, info.normalised(value)));
        }
    };
    set(&mut values, on_of(3), 1.0);
    set(&mut values, gain_of(3), 6.0);
    set(&mut values, freq_of(3), 1_000.0);
    set(&mut values, OUTPUT, -3.0);
    let mut out = [0.0f32; 64];
    response(&values, 48_000, &mut out, (20.0, 20_000.0));
    // The point nearest 1 kHz is the 6 dB the band gives, less the output.
    let nearest = out
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let hz = |i: usize| 20.0 * 1000f32.powf(i as f32 / 63.0);
            (hz(a.0) - 1_000.0)
                .abs()
                .total_cmp(&(hz(b.0) - 1_000.0).abs())
        })
        .map(|(at, _)| at)
        .unwrap();
    assert!((out[nearest] - 3.0).abs() < 0.3, "{}", out[nearest]);
    // 20 Hz is far from it: only the output gain is left.
    assert!((out[0] + 3.0).abs() < 0.1, "{}", out[0]);
    assert!((out[63] + 3.0).abs() < 0.1, "{}", out[63]);
}

/// A band that is off contributes nothing, whatever it is set to.
#[test]
fn a_band_that_is_off_is_not_in_the_response() {
    let mut values: Vec<(u32, f32)> = TABLE
        .iter()
        .map(|info| (info.id, info.normalised(info.default)))
        .collect();
    let gain = params::find(TABLE, gain_of(4)).unwrap();
    values.iter_mut()
        .find(|(id, _)| *id == gain_of(4))
        .unwrap()
        .1 = gain.normalised(12.0);
    let mut out = [0.0f32; 32];
    response(&values, 48_000, &mut out, (20.0, 20_000.0));
    assert!(out.iter().all(|db| db.abs() < 1e-4), "{out:?}");
}
