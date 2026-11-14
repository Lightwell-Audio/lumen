//! Horizon's card: where the line, the sun and the land stand, what the line's
//! drag writes, and what the land keeps of what the device heard.

use eframe::egui::{Rect, pos2};
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
fn the_card_has_three_dials_a_pill_and_one_handle() {
    assert_eq!(DRAWING.dials().count(), 3);
    assert_eq!(DRAWING.controls.len(), 4);
    assert_eq!(DRAWING.handles.len(), 1);
    assert_eq!(
        DRAWING.params(),
        vec![
            device::CEILING,
            device::RELEASE,
            device::LOOK,
            device::DETECT
        ]
    );
    assert_eq!(DRAWING.handles[0].param, device::CEILING);
    // The pill is right of the dials and above the "All n" link.
    let card = Rect::from_min_size(pos2(0.0, 0.0), WHOLE);
    let pill = (DRAWING.controls[3].place)(card);
    let last = (DRAWING.controls[2].place)(card);
    assert!(pill.left() > last.right());
    assert!(pill.bottom() < WHOLE.y - 14.0 - 16.0);
}

#[test]
fn the_specimens_ceiling_is_at_36_percent_down_and_the_scale_round_trips() {
    let rect = picture();
    let y = y_of(-0.3, rect);
    assert!((y - rect.top() - 0.36 * rect.height()).abs() < 1.0, "{y}");
    for db in [-0.3_f32, -6.0, -12.0, -24.0] {
        let back = level_at(y_of(db, rect), rect);
        assert!((back - db).abs() < 1e-3, "{db}: {back}");
    }
    // A louder level is higher on the screen.
    assert!(y_of(-3.0, rect) < y_of(-12.0, rect));
}

#[test]
fn the_lines_drag_maps_its_place_back_to_the_ceiling() {
    let rect = picture();
    for ceiling in [-0.3_f32, -3.0, -12.0, -24.0] {
        let slots = slots(&[(device::CEILING, ceiling)]);
        let v = values(&slots);
        let grab = (DRAWING.handles[0].grab)(&v, rect);
        assert!(rect.contains_rect(grab), "{grab:?} is on the screen");
        let back = ceiling_at(&v, pos2(100.0, y_of(ceiling, rect)), rect);
        assert!(
            (back - f64::from(v.norm(device::CEILING))).abs() < 0.01,
            "{ceiling}"
        );
        assert!(grab.contains(pos2(100.0, y_of(ceiling, rect))));
    }
    // Lower on the screen is a lower ceiling.
    let slots = slots(&[]);
    let v = values(&slots);
    assert!(ceiling_at(&v, pos2(0.0, 200.0), rect) < ceiling_at(&v, pos2(0.0, 100.0), rect));
}

#[test]
fn the_chip_says_the_ceiling_and_values_are_worded_plainly() {
    let slots = slots(&[]);
    let v = values(&slots);
    let norm = v.norm_of(device::CEILING, -0.3);
    assert_eq!(
        (DRAWING.handles[0].chip)(&v, norm),
        "ceiling \u{2212}0.3 dB"
    );
    assert_eq!(bare("-0.3 dB"), "-0.3");
    assert_eq!(tidy("60.0 ms"), "60 ms");
    assert_eq!(tidy("1.50 ms"), "1.5 ms");
    assert_eq!(tidy("100 ms"), "100 ms");
}

#[test]
fn the_resting_land_stays_under_the_line() {
    let rect = picture();
    let horizon = y_of(-0.3, rect);
    for column in 0..COLUMNS {
        let y = rest_y(column, rect, horizon);
        assert!(
            y >= horizon + 1.0 - 1e-3 && y <= rect.bottom(),
            "{column}: {y}"
        );
    }
}

/// A scene over `slots`, hearing `level`, with the meter's `reading`.
fn scene<'a>(
    painter: &'a eframe::egui::Painter,
    slots: &'a [Slot],
    level: f32,
    reading: Option<MeterReading>,
) -> Scene<'a> {
    Scene {
        painter,
        rect: picture(),
        step: Step::Whole,
        hue: Color32::WHITE,
        values: values(slots),
        power: 1.0,
        level,
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
fn the_land_keeps_what_the_device_heard_and_nothing_when_still_or_silent() {
    let ctx = eframe::egui::Context::default();
    let painter = ctx.layer_painter(eframe::egui::LayerId::background());
    let slots = slots(&[]);
    let reading = MeterReading {
        reduction_db: 2.0,
        input_db: -6.0,
    };
    // Still (no reading) or silent (no level): nothing was heard, nothing kept.
    let (levels, flow) = heard(&scene(&painter, &slots, 0.0, None));
    assert!(levels.iter().all(|l| l.is_nan()) && flow == 0.0);
    let (levels, _) = heard(&scene(&painter, &slots, 0.0, Some(reading)));
    assert!(levels.iter().all(|l| l.is_nan()));
    // Hearing something: the newest level is the last column.
    let (levels, _) = heard(&scene(&painter, &slots, 0.6, Some(reading)));
    assert_eq!(levels[COLUMNS - 1], -6.0);
    assert!(levels[COLUMNS - 2].is_nan());
}
