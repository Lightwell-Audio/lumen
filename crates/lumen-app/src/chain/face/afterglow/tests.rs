//! Afterglow's card: what the glow and the rings read, and what a drag on them
//! writes.

use eframe::egui::{Color32, Rect, pos2, vec2};

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
fn the_card_has_four_dials_and_two_handles() {
    assert_eq!(DRAWING.dials().count(), 4);
    assert_eq!(DRAWING.handles.len(), 2);
    assert_eq!(
        DRAWING.params(),
        vec![device::SIZE, device::DECAY, device::DAMPING, device::MIX]
    );
    for id in DRAWING.params() {
        assert!(lumen_devices::params::find(device::TABLE, id).is_some());
    }
}

#[test]
fn the_glow_grows_with_space_and_the_specimen_is_the_specimen() {
    let at_62 = glow_radii(picture(), 0.62);
    assert!(
        (at_62.x - GLOW.x).abs() < 2.0 && (at_62.y - GLOW.y).abs() < 2.0,
        "{at_62:?}"
    );
    assert!(glow_radii(picture(), 0.2).x < glow_radii(picture(), 0.8).x);
    // A thinner picture is a flatter glow, never an empty one.
    let band = Rect::from_min_size(pos2(0.0, 0.0), vec2(272.0, 20.0));
    assert!(glow_radii(band, 0.62).y > 0.0);
}

#[test]
fn the_glow_edge_maps_back_to_space() {
    let rect = picture();
    for norm in [0.1_f32, 0.4, 0.62, 0.9] {
        let edge = centre(rect) + vec2(glow_radii(rect, norm).x, 0.0);
        let back = glow_value(edge, rect);
        assert!((back - f64::from(norm)).abs() < 0.01, "{norm}: {back}");
    }
    // Not held to 0 to 1 (a drag is the change in it): out beyond the edge is
    // more than 1, in at the middle is less than 0.
    assert!(glow_value(pos2(-900.0, -900.0), rect) > 1.0);
    assert!(glow_value(centre(rect), rect) < 0.0);
}

#[test]
fn the_outer_ring_maps_back_to_fade() {
    let rect = picture();
    let s = scales(rect);
    for norm in [0.0_f32, 0.3, 0.45, 1.0] {
        let x = (RING_X.0 * RINGS as f32 + RING_X.1) * s.x * fade_of(norm);
        let back = ring_value(centre(rect) + vec2(x, 0.0), rect);
        assert!((back - f64::from(norm)).abs() < 0.01, "{norm}: {back}");
    }
}

#[test]
fn the_glow_is_over_the_rings_and_both_are_on_the_screen() {
    let slots = slots(&[]);
    let v = values(&slots);
    let rings = (DRAWING.handles[0].grab)(&v, picture());
    let glow = (DRAWING.handles[1].grab)(&v, picture());
    assert!(picture().contains_rect(rings) && picture().contains_rect(glow));
    assert!(
        rings.contains_rect(glow),
        "the glow's grab is inside the rings'"
    );
    assert_eq!(DRAWING.handles[1].param, device::SIZE);
    assert_eq!(DRAWING.handles[0].param, device::DECAY);
}

#[test]
fn the_chips_say_what_the_drag_does() {
    let slots = slots(&[]);
    let v = values(&slots);
    let norm = v.norm_of(device::DECAY, 2.4);
    assert_eq!((DRAWING.handles[0].chip)(&v, norm), "2.4 s to silence");
    let space = v.norm_of(device::SIZE, 62.0);
    assert_eq!((DRAWING.handles[1].chip)(&v, space), "62% of the room");
}

#[test]
fn the_decay_is_worded_in_seconds() {
    assert_eq!(decay_words(2.4), "2.4 s");
    assert_eq!(decay_words(8.0), "8.0 s");
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
fn the_glow_swells_with_the_level_falls_with_the_decay_and_is_still_when_reduced() {
    let ctx = eframe::egui::Context::default();
    let painter = ctx.layer_painter(eframe::egui::LayerId::background());
    let slots = slots(&[]);
    // Reduced motion: the swell is nothing whatever plays, and keeps nothing.
    assert_eq!(swell(&scene(&painter, &slots, 0.9, true), 2.4), 0.0);
    // Hearing something: it follows the level up at once.
    let loud = swell(&scene(&painter, &slots, 0.8, false), 2.4);
    assert!((loud - 0.8).abs() < 1e-6, "{loud}");
    // With the sound gone, it is kept for the next frame and has not jumped to
    // nothing (the frame clock has not moved in a test, so it holds).
    let after = swell(&scene(&painter, &slots, 0.0, false), 2.4);
    assert!(after > 0.0 && after <= 0.8, "{after}");
}
