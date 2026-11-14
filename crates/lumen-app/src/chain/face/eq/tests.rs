//! Iris's parts: what a point says, where it sits, what a drag, a key and the
//! wheel write, and what the three dials aim at.

use eframe::egui::{Rect, pos2, vec2};

use super::*;
use crate::chain::strip::Slot;

/// Slots for every parameter Iris reads, at the table's defaults, with the
/// changes `set` makes (in each parameter's own units).
fn slots(set: &[(u32, f32)]) -> Vec<Slot> {
    READS
        .iter()
        .chain(DRAWING.controls.iter().map(|c| &c.param))
        .map(|id| {
            let info = lumen_devices::params::find(eq::TABLE, *id).unwrap();
            let plain = set
                .iter()
                .find(|(known, _)| known == id)
                .map_or(info.default, |(_, value)| *value);
            Slot {
                id: *id,
                name: String::new(),
                text: String::new(),
                value: f64::from(info.normalised(plain)),
                default: f64::from(info.default),
                steps: info.steps,
                read_only: false,
                automatable: true,
                choices: Vec::new(),
                over: false,
                mapped: None,
                waiting: false,
            }
        })
        .collect()
}

fn values(slots: &[Slot]) -> Values<'_> {
    Values {
        slots,
        table: eq::TABLE,
    }
}

fn screen() -> Rect {
    Rect::from_min_size(pos2(14.0, 60.0), vec2(352.0, 176.0))
}

#[test]
fn every_parameter_the_parts_read_is_in_the_table_once() {
    for id in READS {
        assert!(
            lumen_devices::params::find(eq::TABLE, id).is_some(),
            "{id} is not in the table"
        );
    }
    let mut seen = READS.to_vec();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), READS.len());
    for control in DRAWING.controls {
        assert!(lumen_devices::params::find(eq::TABLE, control.param).is_some());
    }
}

#[test]
fn a_point_says_its_band_in_words() {
    let held = slots(&[(eq::freq_of(3), 1_200.0), (eq::gain_of(3), 3.0)]);
    let v = values(&held);
    let words = words(&v, 2);
    assert!(
        words.starts_with("Band 3, bell, 1.20 kHz, +3.0 dB"),
        "{words}"
    );
    assert!(words.contains("Q 1.00"), "{words}");
    let off = super::words(&v, 0);
    assert!(off.contains("low cut"), "{off}");
    assert!(off.contains("24/oct"), "{off}");
    assert_eq!(pill_words(&v, 2), "Band 3 (bell)");
}

#[test]
fn the_chip_names_what_the_band_is() {
    let held = slots(&[
        (eq::freq_of(2), 180.0),
        (eq::gain_of(2), 5.0),
        (eq::freq_of(1), 40.0),
    ]);
    let v = values(&held);
    assert_eq!(band(&v, 1).chip(), "180 Hz +5.0");
    assert_eq!(band(&v, 0).chip(), "40.0 Hz 24/oct");
    let notch = slots(&[
        (eq::type_of(4), 3.0),
        (eq::freq_of(4), 2_500.0),
        (eq::q_of(4), 4.0),
    ]);
    assert_eq!(band(&values(&notch), 3).chip(), "2.50 kHz Q 4.00");
}

/// The axis the picture is drawn on and the one a drag reads agree: a point
/// dragged to where another sits writes that place's values.
#[test]
fn a_drag_reads_the_axis_the_curve_is_drawn_on() {
    let rect = screen();
    let held = slots(&[(eq::freq_of(3), 1_000.0), (eq::gain_of(3), 6.0)]);
    let v = values(&held);
    let place = at(&v, 2, rect).unwrap();
    let written = drag(&v, 2, place, rect);
    let get = |id: u32| written.iter().find(|(k, _)| *k == id).map(|(_, x)| *x);
    let hz = v.plain_at(eq::freq_of(3), get(eq::freq_of(3)).unwrap() as f32);
    let db = v.plain_at(eq::gain_of(3), get(eq::gain_of(3)).unwrap() as f32);
    assert!((hz - 1_000.0).abs() < 15.0, "{hz}");
    assert!((db - 6.0).abs() < 0.2, "{db}");
}

#[test]
fn a_drag_turns_an_off_band_on_and_a_cut_moves_only_its_frequency() {
    let rect = screen();
    let held = slots(&[(eq::on_of(3), 0.0)]);
    let v = values(&held);
    let written = drag(&v, 2, pos2(100.0, 100.0), rect);
    assert!(written.contains(&(eq::on_of(3), 1.0)));
    let on = slots(&[(eq::on_of(1), 1.0)]);
    let cut = drag(&values(&on), 0, pos2(100.0, 100.0), rect);
    assert!(cut.iter().all(|(id, _)| *id == eq::freq_of(1)), "{cut:?}");
}

#[test]
fn the_dials_aim_at_what_the_picked_band_has() {
    let held = slots(&[]);
    let v = values(&held);
    let aimed = |band: usize, control: usize| aim(&v, band, control).map(|a| (a.param, a.name));
    // A bell: Freq, Gain, Q.
    assert_eq!(aimed(1, 0), Some((eq::freq_of(2), "Freq")));
    assert_eq!(aimed(1, 1), Some((eq::gain_of(2), "Gain")));
    assert_eq!(aimed(1, 2), Some((eq::q_of(2), "Q")));
    // A cut: Freq and Slope, no Q.
    assert_eq!(aimed(0, 1), Some((eq::gain_of(1), "Slope")));
    assert_eq!(aimed(0, 2), None);
    // A notch has no gain; a shelf has no Q.
    let notch = slots(&[(eq::type_of(4), 3.0)]);
    let shelf = slots(&[(eq::type_of(5), 1.0)]);
    assert_eq!(aim(&values(&notch), 3, 1).map(|a| a.name), None);
    assert_eq!(aim(&values(&notch), 3, 2).map(|a| a.name), Some("Q"));
    assert_eq!(aim(&values(&shelf), 4, 1).map(|a| a.name), Some("Gain"));
    assert_eq!(aim(&values(&shelf), 4, 2).map(|a| a.name), None);
}

#[test]
fn the_wheel_is_q_for_a_bell_and_slope_for_a_cut() {
    let held = slots(&[]);
    let v = values(&held);
    let (id, norm) = wheel(&v, 1, 1.0, false).unwrap();
    assert_eq!(id, eq::q_of(2));
    assert!(v.plain_at(id, norm as f32) > v.plain(id));
    let (id, norm) = wheel(&v, 0, 1.0, false).unwrap();
    assert_eq!(id, eq::gain_of(1));
    assert!((v.plain_at(id, norm as f32) - 36.0).abs() < 0.01);
    let notch = slots(&[(eq::type_of(4), 3.0)]);
    assert!(wheel(&values(&notch), 3, 1.0, false).is_some());
    let shelf = slots(&[(eq::type_of(5), 1.0)]);
    assert!(wheel(&values(&shelf), 4, 1.0, false).is_none());
}

#[test]
fn a_nudge_moves_by_a_step_and_shift_by_a_fifth_of_it() {
    let held = slots(&[(eq::freq_of(2), 1_000.0), (eq::gain_of(2), 0.0)]);
    let v = values(&held);
    let plain = |written: &[(u32, f64)], id: u32| {
        v.plain_at(id, written.iter().find(|(k, _)| *k == id).unwrap().1 as f32)
    };
    let coarse = nudge(&v, 1, vec2(0.0, 1.0), false);
    let fine = nudge(&v, 1, vec2(0.0, 1.0), true);
    assert!((plain(&coarse, eq::gain_of(2)) - KEY_DB).abs() < 0.01);
    assert!((plain(&fine, eq::gain_of(2)) - KEY_DB * FINE).abs() < 0.01);
    let right = nudge(&v, 1, vec2(1.0, 0.0), false);
    assert!(plain(&right, eq::freq_of(2)) > 1_000.0);
}

#[test]
fn a_reset_writes_only_what_the_band_has() {
    let held = slots(&[(eq::gain_of(2), 7.0), (eq::gain_of(1), 48.0)]);
    let v = values(&held);
    let ids = |written: Vec<(u32, f64)>| written.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    assert_eq!(ids(reset_point(&v, 1)), [eq::gain_of(2)]);
    assert_eq!(ids(reset_point(&v, 0)), [eq::freq_of(1)]);
    assert_eq!(
        ids(reset_all(&v, 1)),
        [eq::freq_of(2), eq::gain_of(2), eq::q_of(2)]
    );
    assert_eq!(ids(reset_all(&v, 0)), [eq::freq_of(1), eq::gain_of(1)]);
}

#[test]
fn the_pills_sit_in_two_rows_of_four_inside_the_card() {
    let card = Rect::from_min_size(pos2(0.0, 0.0), WHOLE);
    let mut seen: Vec<Rect> = Vec::new();
    for index in 0..eq::BANDS {
        let rect = pill(card, index);
        assert!(card.contains_rect(rect), "{index}: {rect:?}");
        assert!(seen.iter().all(|other| !other.intersects(rect)));
        seen.push(rect);
    }
    assert_eq!(seen[0].top(), seen[3].top());
    assert!(seen[4].top() > seen[0].bottom());
}

#[test]
fn eight_bands_have_eight_colours_and_the_first_band_is_the_first_on() {
    let colours: std::collections::HashSet<_> = (0..eq::BANDS).map(colour).collect();
    assert_eq!(colours.len(), eq::BANDS);
    let held = slots(&[
        (eq::on_of(1), 0.0),
        (eq::on_of(2), 0.0),
        (eq::on_of(3), 1.0),
    ]);
    assert_eq!(first(&values(&held)), 2);
    let none = slots(&[
        (eq::on_of(1), 0.0),
        (eq::on_of(2), 0.0),
        (eq::on_of(3), 0.0),
        (eq::on_of(4), 0.0),
        (eq::on_of(5), 0.0),
        (eq::on_of(6), 0.0),
        (eq::on_of(7), 0.0),
        (eq::on_of(8), 0.0),
    ]);
    assert_eq!(first(&values(&none)), 0);
}
