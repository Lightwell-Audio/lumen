use eframe::egui::{Rect, pos2};
use lumen_devices::MeterReading;

use super::geometry::{
    BOTTOM_DB, COLUMNS, GRID_DB, SPECIMEN, TAKE_EVERY, TOP_DB, bars, column_x, foot_drop, level_at,
    line_box, readout, y_of,
};
use super::history::{Heard, Skyline};
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
        rate: 48_000,
    }
}

fn picture() -> Rect {
    Rect::from_min_size(pos2(14.0, 60.0), SPECIMEN)
}

fn loud() -> Heard {
    Heard {
        input_db: -3.0,
        output_db: -6.0,
        reduction_db: 3.0,
    }
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
    let card = Rect::from_min_size(pos2(0.0, 0.0), WHOLE);
    let pill = (DRAWING.controls[3].place)(card);
    let last = (DRAWING.controls[2].place)(card);
    assert!(pill.left() > last.right());
    assert!((pill.width() - 44.0).abs() < 1e-3 && (pill.height() - 20.0).abs() < 1e-3);
    assert!(pill.bottom() < WHOLE.y - 14.0 - 16.0);
}

#[test]
fn the_dials_are_spread_evenly_under_the_screen() {
    let card = Rect::from_min_size(pos2(0.0, 0.0), WHOLE);
    let xs: Vec<f32> = (0..3)
        .map(|k| (DRAWING.controls[k].place)(card).center().x)
        .collect();
    assert!(((xs[1] - xs[0]) - (xs[2] - xs[1])).abs() < 0.01, "{xs:?}");
    assert!((xs[0] - 56.0).abs() < 0.01);
}

#[test]
fn the_scale_runs_from_plus_four_at_the_top_to_minus_thirty_six_at_the_foot() {
    let rect = picture();
    assert!((y_of(TOP_DB, rect) - rect.top()).abs() < 1e-3);
    assert!((y_of(BOTTOM_DB, rect) - rect.bottom()).abs() < 1e-3);
    assert!((y_of(-6.0, rect) - rect.top() - 0.25 * rect.height()).abs() < 1e-3);
    for db in [-0.3_f32, -6.0, -12.0, -24.0] {
        let back = level_at(y_of(db, rect), rect);
        assert!((back - db).abs() < 1e-3, "{db}: {back}");
    }
    assert!(y_of(-3.0, rect) < y_of(-12.0, rect));
    for pair in GRID_DB.windows(2) {
        assert!(y_of(pair[0], rect) < y_of(pair[1], rect));
    }
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
    let slots = slots(&[]);
    let v = values(&slots);
    assert!(ceiling_at(&v, pos2(0.0, 200.0), rect) < ceiling_at(&v, pos2(0.0, 100.0), rect));
}

#[test]
fn the_line_leaves_the_edges_free_for_the_card() {
    let rect = picture();
    let grab = line_box(-6.0, rect);
    assert!(grab.left() > rect.left() && grab.right() < rect.right());
    assert!(grab.height() > 8.0);
    let top = line_box(TOP_DB, rect);
    assert!(rect.contains_rect(top));
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
fn the_newest_column_rides_the_right_edge_and_the_oldest_leaves_the_left() {
    let rect = picture();
    let newest = column_x(rect, COLUMNS, COLUMNS - 1, 0.0);
    assert!((newest - (rect.right() + super::geometry::column_step(rect))).abs() < 1e-3);
    let oldest = column_x(rect, COLUMNS, 0, 1.0);
    assert!((oldest - rect.left()).abs() < 1e-2, "{oldest}");
    let half = column_x(rect, COLUMNS, 10, 0.5);
    let none = column_x(rect, COLUMNS, 10, 0.0);
    assert!((none - half - 0.5 * super::geometry::column_step(rect)).abs() < 1e-3);
}

#[test]
fn the_foot_rounds_only_inside_the_corner_radius() {
    let rect = picture();
    assert_eq!(foot_drop(rect.center().x, rect, 10.0), 0.0);
    assert!((foot_drop(rect.left(), rect, 10.0) - 10.0).abs() < 1e-3);
    assert_eq!(foot_drop(rect.left(), rect, 0.0), 0.0);
}

#[test]
fn the_still_bar_shows_the_output_and_what_was_pressed_above_it() {
    let rect = picture();
    let (output, pressed) = bars(rect, -3.0, -6.0);
    assert!(output.top() > pressed.top());
    assert!((output.top() - pressed.bottom()).abs() < 1e-3);
    assert!(pressed.height() > 0.0 && output.bottom() <= rect.bottom());
    let (_, same) = bars(rect, -6.0, -6.0);
    assert!(same.height().abs() < 1e-3);
}

#[test]
fn the_readout_plate_sits_inside_the_screen_at_each_step() {
    let rect = picture();
    for step in [Step::Whole, Step::Short] {
        let at = readout(rect, step);
        assert!(rect.contains_rect(at.plate), "{step:?}");
        assert!(at.plate.contains(at.hero) && at.plate.contains(at.sub));
        assert!(at.hero.y < at.sub.y);
    }
    assert!(readout(rect, Step::Short).hero_size < readout(rect, Step::Whole).hero_size);
}

#[test]
fn a_reading_below_the_floor_is_not_sounding() {
    assert!(!Heard::SILENT.sounding());
    assert!(loud().sounding());
    let heard = Heard::of(MeterReading {
        reduction_db: -2.0,
        input_db: -6.0,
        output_db: -6.0,
    });
    assert_eq!(heard.reduction_db, 0.0);
}

#[test]
fn the_history_takes_a_column_every_33_ms_and_keeps_the_newest_137() {
    let mut sky = Skyline::new(0.0);
    for _ in 0..40 {
        sky.step(loud(), TAKE_EVERY + 0.0001, false);
    }
    assert_eq!(sky.input().len(), 40);
    assert_eq!(sky.output().len(), 40);
    assert!(sky.input().iter().all(|db| (*db + 3.0).abs() < 0.01));
    assert!(sky.output().iter().all(|db| (*db + 6.0).abs() < 0.01));
    for _ in 0..300 {
        sky.step(Heard::SILENT, TAKE_EVERY + 0.0001, false);
    }
    assert_eq!(sky.input().len(), COLUMNS);
    assert!(sky.input()[COLUMNS - 1] < -100.0);
    assert!(sky.shift() >= 0.0 && sky.shift() <= 1.0);
}

#[test]
fn a_column_keeps_the_loudest_of_what_arrived_between_two_takes() {
    let mut sky = Skyline::new(0.0);
    let quiet = Heard {
        input_db: -30.0,
        output_db: -30.0,
        reduction_db: 0.0,
    };
    sky.step(loud(), 0.01, false);
    sky.step(quiet, 0.01, false);
    sky.step(quiet, 0.02, false);
    assert_eq!(sky.input().len(), 1);
    assert!((sky.input()[0] + 3.0).abs() < 0.01);
}

#[test]
fn a_hard_hit_flashes_and_the_flash_dies_away() {
    let mut sky = Skyline::new(0.0);
    sky.step(loud(), 0.016, false);
    let first = sky.flash();
    assert!(first > 0.5, "{first}");
    for _ in 0..30 {
        sky.step(Heard::SILENT, 0.016, false);
    }
    assert!(sky.flash() < 0.02, "{}", sky.flash());
    let mut slow = Skyline::new(0.0);
    let mut fast = Skyline::new(0.0);
    slow.step(loud(), 0.016, false);
    fast.step(loud(), 0.016, false);
    for _ in 0..10 {
        slow.step(Heard::SILENT, 0.016, false);
    }
    for _ in 0..20 {
        fast.step(Heard::SILENT, 0.008, false);
    }
    assert!(
        (slow.flash() - fast.flash()).abs() < 0.02,
        "the same time gives the same flash at any frame rate: {} {}",
        slow.flash(),
        fast.flash()
    );
}

#[test]
fn a_still_face_keeps_no_history_and_no_flash() {
    let mut sky = Skyline::new(0.0);
    for _ in 0..10 {
        sky.step(loud(), TAKE_EVERY + 0.0001, false);
    }
    sky.step(loud(), 0.1, true);
    assert!(sky.input().is_empty());
    assert_eq!(sky.flash(), 0.0);
    for _ in 0..10 {
        sky.step(loud(), 0.1, true);
    }
    assert!(sky.input().is_empty());
    assert!((sky.pulled() - 3.0).abs() < 0.01, "{}", sky.pulled());
}

fn scene<'a>(
    painter: &'a eframe::egui::Painter,
    slots: &'a [Slot],
    step: Step,
    awake: f32,
    still: bool,
    reading: Option<MeterReading>,
) -> Scene<'a> {
    Scene {
        painter,
        rect: picture(),
        step,
        hue: crate::theme::personality::HORIZON_LINE,
        values: values(slots),
        power: 1.0,
        level: awake,
        awake,
        still,
        handle_lit: [0.0; super::super::MAX_HANDLES],
        settle: 1.0,
        picked: 0,
        parts_lit: [0.0; super::super::MAX_PARTS],
        spectrum: None,
        mist: 0.0,
        bpm: 120.0,
        reading,
        id: eframe::egui::Id::new("test card"),
    }
}

fn frame(
    ctx: &eframe::egui::Context,
    slots: &[Slot],
    step: Step,
    awake: f32,
    still: bool,
    reading: Option<MeterReading>,
) -> usize {
    crate::fonts::install(ctx);
    let mut out = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
        let painter = ui.ctx().layer_painter(eframe::egui::LayerId::background());
        draw(&scene(&painter, slots, step, awake, still, reading));
    });
    out.textures_delta.clear();
    out.shapes.len()
}

fn reading() -> MeterReading {
    MeterReading {
        reduction_db: 3.0,
        input_db: -3.0,
        output_db: -6.0,
    }
}

#[test]
fn every_step_draws_at_rest_live_and_still() {
    let slots = slots(&[]);
    for step in [Step::Whole, Step::Short, Step::Compact] {
        let ctx = eframe::egui::Context::default();
        for (awake, still, reading) in [
            (0.0, false, None),
            (1.0, false, Some(reading())),
            (0.0, true, None),
            (1.0, true, Some(reading())),
        ] {
            for _ in 0..3 {
                assert!(frame(&ctx, &slots, step, awake, still, reading) > 0);
            }
        }
    }
}

#[test]
fn a_live_face_drops_the_rest_picture_s_dashes_and_a_still_one_draws_a_bar() {
    let slots = slots(&[]);
    let rest_ctx = eframe::egui::Context::default();
    let rest = frame(&rest_ctx, &slots, Step::Whole, 0.0, false, None);
    let live_ctx = eframe::egui::Context::default();
    let mut live = 0;
    for _ in 0..4 {
        live = frame(&live_ctx, &slots, Step::Whole, 1.0, false, Some(reading()));
    }
    let still_ctx = eframe::egui::Context::default();
    let still = frame(&still_ctx, &slots, Step::Whole, 1.0, true, Some(reading()));
    assert!(rest > 3);
    assert!(
        live < rest,
        "the dashed rest line is gone while live: {live} {rest}"
    );
    assert!(still > 3);
}

fn timed(ctx: &eframe::egui::Context, slots: &[Slot], time: f64, live: bool) -> f64 {
    let started = std::time::Instant::now();
    let input = eframe::egui::RawInput {
        time: Some(time),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        let painter = ui.ctx().layer_painter(eframe::egui::LayerId::background());
        let reading = live.then(reading);
        draw(&scene(
            &painter,
            slots,
            Step::Whole,
            f32::from(u8::from(live)),
            false,
            reading,
        ));
    });
    out.textures_delta.clear();
    let meshes = ctx.tessellate(out.shapes, out.pixels_per_point);
    std::hint::black_box(meshes.len());
    started.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore = "a bench: prints the draw and tessellation time of the face with a full history"]
fn the_cost_of_drawing_the_face_with_a_full_history() {
    let slots = slots(&[]);
    for (name, live) in [("rest", false), ("live", true)] {
        let ctx = eframe::egui::Context::default();
        crate::fonts::install(&ctx);
        let mut time = 0.0;
        for _ in 0..200 {
            time += 0.034;
            timed(&ctx, &slots, time, live);
        }
        let mut samples: Vec<f64> = (0..300)
            .map(|_| {
                time += 0.034;
                timed(&ctx, &slots, time, live)
            })
            .collect();
        samples.sort_by(f64::total_cmp);
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        println!(
            "HORIZON {name}: mean {mean:.3} p95 {:.3} worst {:.3} ms",
            samples[samples.len() * 95 / 100],
            samples[samples.len() - 1]
        );
    }
}
