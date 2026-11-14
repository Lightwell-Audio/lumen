//! Gravity's card: what the ring, the ball and the well read, and what a drag on
//! them writes.

use eframe::egui::{Rect, pos2, vec2};
use lumen_devices::MeterReading;

use super::*;
use crate::chain::geometry::Step;
use crate::chain::strip::Slot;

fn slots(set: &[(u32, f32)]) -> Vec<Slot> {
    DRAWING
        .params()
        .into_iter()
        .map(|id| {
            let info = lumen_devices::params::find(device::TABLE, id).unwrap();
            let plain = set
                .iter()
                .find(|(known, _)| *known == id)
                .map_or(info.default, |(_, value)| *value);
            Slot {
                id,
                name: String::new(),
                text: String::new(),
                value: f64::from(info.normalised(plain)),
                default: f64::from(info.normalised(info.default)),
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
        table: device::TABLE,
    }
}

fn picture() -> Rect {
    Rect::from_min_size(pos2(14.0, 60.0), SPECIMEN)
}

#[test]
fn the_card_has_five_dials_and_two_handles() {
    assert_eq!(DRAWING.dials().count(), 5);
    assert_eq!(DRAWING.handles.len(), 2);
    assert_eq!(
        DRAWING.params(),
        vec![
            device::THRESHOLD,
            device::RATIO,
            device::ATTACK,
            device::RELEASE,
            device::MAKEUP
        ]
    );
    for id in DRAWING.params() {
        assert!(lumen_devices::params::find(device::TABLE, id).is_some());
    }
    assert_eq!(DRAWING.handles[0].param, device::THRESHOLD);
    assert_eq!(DRAWING.handles[1].param, device::RATIO);
}

#[test]
fn the_dials_clear_the_all_link_and_each_other() {
    // The link takes the card's lower right (60 x 16 pt, 14 pt in from the right
    // and 12 pt up): no dial's box, with the name and value it carries under it,
    // reaches it, and the dials are a name's width apart.
    let card = Rect::from_min_size(pos2(0.0, 0.0), WHOLE);
    let link = Rect::from_min_size(
        pos2(WHOLE.x - 14.0 - 60.0, WHOLE.y - 12.0 - 8.0),
        vec2(60.0, 16.0),
    );
    for control in DRAWING.controls {
        let dial = (control.place)(card).expand2(vec2(0.0, 8.0));
        assert!(!dial.intersects(link), "{dial:?} meets {link:?}");
    }
    for pair in DIAL_X.windows(2) {
        assert!(pair[1] - pair[0] >= 56.0);
    }
}

#[test]
fn the_ring_is_the_specimens_at_its_threshold_and_wider_for_a_lower_one() {
    let rect = picture();
    let slots = slots(&[]);
    let norm = values(&slots).norm(device::THRESHOLD);
    let (_, radii) = ring(rect, norm);
    assert!((radii.x - 74.0).abs() < 1.0, "{radii:?}");
    assert!((radii.y - 22.0).abs() < 1.0, "{radii:?}");
    assert!(ring(rect, 0.2).1.x > ring(rect, 0.9).1.x);
}

#[test]
fn the_rings_edge_maps_back_to_the_threshold() {
    let rect = picture();
    for norm in [0.1_f32, 0.4, 0.7, 0.9] {
        let (at, radii) = ring(rect, norm);
        let back = ring_value(at + vec2(radii.x, 0.0), rect);
        assert!((back - f64::from(norm)).abs() < 0.01, "{norm}: {back}");
    }
    // Not held to 0 to 1: a drag is the change in it.
    assert!(ring_value(pos2(-900.0, -900.0), rect) < 0.0);
}

#[test]
fn the_ball_is_over_the_field_and_a_drag_down_is_a_bigger_ratio() {
    let rect = picture();
    let slots = slots(&[]);
    let v = values(&slots);
    let field = (DRAWING.handles[0].grab)(&v, rect);
    let ball = (DRAWING.handles[1].grab)(&v, rect);
    assert!(rect.contains_rect(field) && rect.contains_rect(ball));
    assert!(field.contains(ball.center()), "the ball stands in the ring");
    assert!(orb_value(pos2(100.0, 200.0), rect) > orb_value(pos2(100.0, 100.0), rect));
}

#[test]
fn the_chips_say_what_the_drag_does() {
    let slots = slots(&[]);
    let v = values(&slots);
    let norm = v.norm_of(device::THRESHOLD, -18.0);
    assert_eq!((DRAWING.handles[0].chip)(&v, norm), "pull \u{2212}18 dB");
    let ratio = v.norm_of(device::RATIO, 4.0);
    assert_eq!((DRAWING.handles[1].chip)(&v, ratio), "ratio 4.0 : 1");
}

#[test]
fn values_are_worded_as_the_specimen_words_them() {
    assert_eq!(tidy("-18.0 dB"), "-18 dB");
    assert_eq!(tidy("4.0:1"), "4:1");
    assert_eq!(tidy("5.00 ms"), "5 ms");
    assert_eq!(tidy("120 ms"), "120 ms");
    assert_eq!(lower("Auto"), "auto");
}

#[test]
fn the_well_is_shallow_at_rest_the_specimens_at_its_reduction_and_deeper_beyond() {
    assert!(depth(0.0, 4.0) < 0.5);
    assert!((depth(4.1, 4.0) - 1.0).abs() < 0.01);
    assert!(depth(12.0, 4.0) > depth(4.1, 4.0));
    assert!(depth(4.1, 20.0) > depth(4.1, 4.0));
    assert!(depth(40.0, 20.0) <= WELL_MOST * WELL_RATIO.1 + 1e-4);
}

/// A scene over `slots`, with the meter's `reading`.
fn scene<'a>(
    painter: &'a eframe::egui::Painter,
    slots: &'a [Slot],
    reading: Option<MeterReading>,
) -> Scene<'a> {
    Scene {
        painter,
        rect: picture(),
        step: Step::Whole,
        hue: Color32::WHITE,
        values: values(slots),
        power: 1.0,
        level: 0.0,
        still: reading.is_none(),
        handle_lit: [0.0; super::super::MAX_HANDLES],
        settle: 1.0,
        picked: 0,
        parts_lit: [0.0; super::super::MAX_PARTS],
        spectrum: None,
        mist: 0.0,
        rate: 48_000,
        bpm: 120.0,
        reading,
        id: eframe::egui::Id::new("test card"),
    }
}

#[test]
fn the_pull_follows_the_meter_falls_back_and_is_nothing_when_reduced() {
    let ctx = eframe::egui::Context::default();
    let painter = ctx.layer_painter(eframe::egui::LayerId::background());
    let slots = slots(&[]);
    let reading = |db| MeterReading {
        reduction_db: db,
        input_db: -8.0,
    };
    // Reduced motion: no reading, so nothing is pulled and nothing is kept.
    assert_eq!(meter::pulled(&scene(&painter, &slots, None)), 0.0);
    // Pulling: it follows the meter up at once.
    let pulled = meter::pulled(&scene(&painter, &slots, Some(reading(4.1))));
    assert!((pulled - 4.1).abs() < 1e-6, "{pulled}");
    // The device lets go: it is kept for the next frame and has not jumped to
    // nothing (the frame clock has not moved in a test, so it holds).
    let after = meter::pulled(&scene(&painter, &slots, Some(reading(0.0))));
    assert!(after > 0.0 && after <= 4.1, "{after}");
    // And nothing is kept once motion is reduced again.
    assert_eq!(meter::pulled(&scene(&painter, &slots, None)), 0.0);
    assert_eq!(
        meter::pulled(&scene(&painter, &slots, Some(reading(0.0)))),
        0.0
    );
}
