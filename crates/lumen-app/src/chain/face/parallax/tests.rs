//! Parallax's card: what the picture reads, which parameter the Time dial and the
//! echo trail stand for, and what a drag on the trail writes.

use eframe::egui::{Color32, Rect, pos2};

use super::*;
use crate::chain::geometry::Step;
use crate::chain::strip::Slot;

/// Slots for every parameter the card has a control for, with the changes `set`
/// makes (in each parameter's own units; a note by its index).
fn slots(set: &[(u32, f32)]) -> Vec<Slot> {
    DRAWING
        .params()
        .into_iter()
        .map(|id| {
            let info = lumen_devices::params::find(device::TABLE, id).unwrap();
            let given = set.iter().find(|(known, _)| *known == id).map(|(_, v)| *v);
            let value = match (id, given) {
                (device::NOTE, Some(index)) => f64::from(index / (NOTES - 1) as f32),
                (device::NOTE, None) => f64::from(info.normalised(info.default)),
                (_, given) => f64::from(info.normalised(given.unwrap_or(info.default))),
            };
            Slot {
                id,
                name: String::new(),
                text: String::new(),
                value,
                default: value,
                steps: info.steps,
                read_only: false,
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
        table: device::TABLE,
    }
}

fn picture() -> Rect {
    Rect::from_min_size(pos2(14.0, 60.0), SPECIMEN)
}

#[test]
fn the_card_has_three_dials_two_pills_and_one_trail() {
    assert_eq!(DRAWING.dials().count(), 3);
    assert_eq!(DRAWING.controls.len(), 5);
    assert_eq!(DRAWING.handles.len(), 1);
    // Every parameter it draws is in the table, once.
    let params = DRAWING.params();
    for id in &params {
        assert!(lumen_devices::params::find(device::TABLE, *id).is_some());
    }
    let mut seen = params.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), params.len());
    assert!(params.contains(&device::FEEDBACK));
}

#[test]
fn the_time_dial_is_the_note_while_synced_and_the_milliseconds_while_free() {
    let free = slots(&[(device::NOTE, 0.0)]);
    assert_eq!(time_aim(&values(&free)).param, device::TIME);
    let synced = slots(&[(device::NOTE, 3.0)]);
    assert_eq!(time_aim(&values(&synced)).param, device::NOTE);
    assert_eq!(time_aim(&values(&synced)).name, "Time");
}

#[test]
fn the_hero_is_the_real_time() {
    let free = slots(&[(device::NOTE, 0.0), (device::TIME, 250.0)]);
    let v = values(&free);
    assert_eq!(
        time_words(&v, note_index(&v), v.plain(device::TIME)),
        "250 ms"
    );
    let long = slots(&[(device::NOTE, 0.0), (device::TIME, 1500.0)]);
    let v = values(&long);
    assert_eq!(
        time_words(&v, note_index(&v), v.plain(device::TIME)),
        "1.50 s"
    );
    let synced = slots(&[(device::NOTE, 3.0)]);
    let v = values(&synced);
    assert_eq!(time_words(&v, note_index(&v), v.plain(device::TIME)), "1/8");
}

#[test]
fn the_sync_pill_reads_a_note_and_turns_on_to_the_default_note() {
    let sync = &DRAWING.controls[3];
    let switch = sync.switch.unwrap();
    assert!(!(switch.on)(0.0));
    assert!((switch.on)(0.5));
    let free = slots(&[(device::NOTE, 0.0)]);
    let on = (switch.turn_on)(&values(&free));
    assert!((on - 4.0 / 6.0).abs() < 1e-6, "{on}");
    assert!((switch.on)(on));
}

#[test]
fn more_feedback_is_more_echoes_and_the_specimen_has_seven() {
    assert_eq!(echoes(0.45), 7);
    assert_eq!(echoes(0.0), FEWEST);
    assert_eq!(echoes(1.0), MOST);
    let mut last = 0;
    for f in [0.05, 0.2, 0.35, 0.5, 0.65, 0.8, 0.95] {
        let n = echoes(f);
        assert!(n >= last, "{f}: {n} after {last}");
        last = n;
    }
}

#[test]
fn a_longer_time_is_a_wider_trail() {
    assert!(lane_of(0.03) < 1e-6);
    assert!((lane_of(4.0) - 1.0).abs() < 1e-6);
    assert!(lane_of(0.1) < lane_of(0.5));
    assert!(lane_of(0.5) < lane_of(2.0));
    // The specimen's "1/8 dotted" is about a third of the width.
    let lane = LANE.0 + LANE.1 * lane_of(0.375);
    assert!((lane - 0.32).abs() < 0.03, "{lane}");
}

#[test]
fn the_trail_maps_across_to_time_and_up_to_feedback_and_clamps() {
    let handle = &DRAWING.handles[0];
    let free = slots(&[(device::NOTE, 0.0)]);
    let v = values(&free);
    let rect = (handle.grab)(&v, picture());
    assert!(picture().contains_rect(rect));
    let left = (handle.value_at)(&v, pos2(rect.left(), rect.center().y), rect);
    let right = (handle.value_at)(&v, pos2(rect.right(), rect.center().y), rect);
    assert!(left < 1e-6 && (right - 1.0).abs() < 1e-6, "{left} {right}");
    let also = handle.also.as_ref().unwrap();
    let top = (also.value_at)(&v, pos2(rect.center().x, rect.top()), rect);
    let bottom = (also.value_at)(&v, pos2(rect.center().x, rect.bottom()), rect);
    assert!((top - 1.0).abs() < 1e-6 && bottom < 1e-6, "{top} {bottom}");
    assert!((handle.value_at)(&v, pos2(-500.0, 0.0), rect) < 1e-6);
    assert!(((also.value_at)(&v, pos2(0.0, -900.0), rect) - 1.0).abs() < 1e-6);
}

#[test]
fn the_trail_follows_the_note_while_synced() {
    let follows = DRAWING.handles[0].follows.unwrap();
    assert_eq!(
        follows(&values(&slots(&[(device::NOTE, 0.0)]))),
        device::TIME
    );
    assert_eq!(
        follows(&values(&slots(&[(device::NOTE, 2.0)]))),
        device::NOTE
    );
}

#[test]
fn the_chip_says_the_time_then_the_feedback() {
    let handle = &DRAWING.handles[0];
    let free = slots(&[(device::NOTE, 0.0), (device::FEEDBACK, 40.0)]);
    let words = (handle.chip)(&values(&free), 0.0);
    assert!(words.ends_with("40%"), "{words}");
    assert!(words.contains("ms"), "{words}");
    let synced = slots(&[(device::NOTE, 3.0), (device::FEEDBACK, 40.0)]);
    let words = (handle.chip)(&values(&synced), 4.0 / 6.0);
    assert!(words.starts_with("1/4"), "{words}");
}

#[test]
fn the_pills_do_not_cross_the_dials_or_leave_the_card() {
    let card = Rect::from_min_size(pos2(0.0, 0.0), WHOLE);
    for index in 0..2 {
        let rect = pill(card, index);
        assert!(card.contains_rect(rect), "pill {index}: {rect:?}");
        for dial_index in 0..3 {
            assert!(
                !rect.intersects(dial(card, dial_index)),
                "{index} / {dial_index}"
            );
        }
    }
    assert!(!pill(card, 0).intersects(pill(card, 1)));
}

/// A scene over `slots`, hearing `level`, with motion reduced as `still` says.
fn scene<'a>(
    painter: &'a eframe::egui::Painter,
    slots: &'a [Slot],
    level: f32,
    still: bool,
) -> Scene<'a> {
    Scene {
        painter,
        rect: picture(),
        step: Step::Whole,
        hue: Color32::WHITE,
        values: values(slots),
        power: 1.0,
        level,
        still,
        handle_lit: [0.0; super::super::MAX_HANDLES],
        settle: 1.0,
        picked: 0,
        parts_lit: [0.0; super::super::MAX_PARTS],
        spectrum: None,
        mist: 0.0,
        rate: 48_000,
        bpm: 120.0,
        id: eframe::egui::Id::new("test card"),
    }
}

#[test]
fn the_light_travels_the_trail_only_while_audio_plays_and_motion_is_not_reduced() {
    let ctx = eframe::egui::Context::default();
    let painter = ctx.layer_painter(eframe::egui::LayerId::background());
    let slots = slots(&[]);
    let lit_somewhere = |scene: &Scene<'_>| {
        let beat = pulse(scene, 0.375, 7);
        (0..7).any(|index| beat(index, 1.0) > 0.0)
    };
    // Nothing plays: the trail is still.
    assert!(!lit_somewhere(&scene(&painter, &slots, 0.0, false)));
    // Motion is reduced: still, whatever plays.
    assert!(!lit_somewhere(&scene(&painter, &slots, 0.9, true)));
    // Audio plays: the light is on an echo (the clock is at 0, the near one).
    assert!(lit_somewhere(&scene(&painter, &slots, 0.9, false)));
}
