use eframe::egui::{Color32, Rect, pos2, vec2};
use lumen_devices::MeterReading;

use super::geometry::{
    CELLS, COLUMNS, FOCUS, Frame, ROWS, SPECIMEN, body_box, body_value, focus_down, readout, ring,
    ring_box, ring_half, ring_value_db, rows, scale,
};
use super::well::{Membrane, Well, depth_of};
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

fn heard(reduction_db: f32) -> well::Heard {
    well::Heard {
        reduction_db,
        input_db: -8.0,
    }
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
fn the_grid_is_the_specimens_perspective_at_the_focus() {
    let frame = Frame::of(picture());
    let rows = rows();
    let focus = frame.grid(FOCUS.0, &rows[FOCUS.1], 0.0);
    assert!((focus.x - (picture().center().x)).abs() < 0.01, "{focus:?}");
    let left = frame.grid(0, &rows[ROWS - 1], 0.0);
    let right = frame.grid(COLUMNS - 1, &rows[ROWS - 1], 0.0);
    assert!(left.x < focus.x && right.x > focus.x);
    assert!((focus.x - left.x) - (right.x - focus.x) < 0.01);
    let far = frame.grid(0, &rows[0], 0.0);
    assert!(far.y < left.y, "the far row is higher up the screen");
    assert!((far.x - focus.x).abs() < (left.x - focus.x).abs());
}

#[test]
fn a_pull_lowers_the_grid_and_a_height_raises_it() {
    let frame = Frame::of(picture());
    let rows = rows();
    let flat = frame.grid(FOCUS.0, &rows[FOCUS.1], 0.0);
    assert!(frame.grid(FOCUS.0, &rows[FOCUS.1], -1.0).y > flat.y);
    assert!(frame.grid(FOCUS.0, &rows[FOCUS.1], 1.0).y < flat.y);
    assert!(focus_down(-1.0) > focus_down(0.0));
}

#[test]
fn the_picture_scales_down_to_a_band_and_never_up() {
    assert!((scale(picture()) - 1.0).abs() < 1e-4);
    let big = Rect::from_min_size(pos2(0.0, 0.0), vec2(704.0, 352.0));
    assert!((scale(big) - 1.0).abs() < 1e-4);
    let short = Rect::from_min_size(pos2(0.0, 0.0), vec2(204.0, 102.0));
    assert!((scale(short) - 204.0 / 352.0).abs() < 1e-3);
    let band = Rect::from_min_size(pos2(0.0, 0.0), vec2(204.0, 22.0));
    assert!((scale(band) - 0.3).abs() < 1e-4);
    assert!(Frame::of(band).body_scale() >= 0.4);
}

#[test]
fn the_ring_is_the_specimens_at_its_threshold_and_wider_for_a_lower_one() {
    let rect = picture();
    let (at, radii) = ring(rect, -18.0);
    assert!((radii.x - ring_half(-18.0)).abs() < 1e-3);
    assert!((radii.y - radii.x * 0.3).abs() < 1e-3);
    assert!(ring(rect, -36.0).1.x > ring(rect, -9.0).1.x);
    assert!((at.x - rect.center().x).abs() < 1e-3);
}

#[test]
fn the_rings_edge_maps_back_to_the_threshold() {
    let rect = picture();
    for db in [-40.0_f32, -27.0, -18.0, -6.0, -2.0] {
        let (at, radii) = ring(rect, db);
        let back = ring_value_db(at + vec2(radii.x, 0.0), rect);
        assert!((back - db).abs() < 0.05, "{db}: {back}");
        let up = ring_value_db(at + vec2(0.0, -radii.y), rect);
        assert!((up - db).abs() < 0.05, "{db}: {up}");
    }
}

#[test]
fn the_ring_and_the_body_are_inside_the_screen_and_the_body_stands_in_the_ring() {
    let rect = picture();
    let ring_box = ring_box(rect, -18.0);
    let ball = body_box(rect);
    assert!(rect.contains_rect(ring_box) && rect.contains_rect(ball));
    assert!(ring_box.contains(ball.center()));
}

#[test]
fn a_drag_down_is_a_bigger_ratio() {
    let rect = picture();
    assert!(body_value(pos2(100.0, 200.0), rect) > body_value(pos2(100.0, 100.0), rect));
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
fn the_readout_sits_inside_the_screen_at_each_step() {
    let rect = picture();
    for step in [Step::Whole, Step::Short] {
        let screen = if step == Step::Whole {
            rect
        } else {
            Rect::from_min_size(rect.min, vec2(204.0, 102.0))
        };
        let at = readout(screen, step);
        for place in [at.hero, at.sub, at.rest] {
            assert!(screen.contains(place), "{step:?}: {place:?}");
        }
        assert!(at.hero.y < at.sub.y && at.sub.y < at.rest.y);
    }
}

#[test]
fn the_depth_follows_the_reduction_and_stops_at_the_full_pull() {
    assert_eq!(depth_of(0.0), 0.0);
    assert!(depth_of(4.0) > 0.0 && depth_of(4.0) < depth_of(8.0));
    assert_eq!(depth_of(16.0), 1.0);
    assert_eq!(depth_of(40.0), 1.0);
}

fn settle(membrane: &mut Membrane, depth: f32, seconds: f32) {
    let mut left = seconds;
    while left > 0.0 {
        membrane.step(depth, 1.0 / 60.0);
        left -= 1.0 / 60.0;
    }
}

#[test]
fn a_held_depth_pulls_the_focus_down_and_leaves_the_border_where_it_was() {
    let mut membrane = Membrane::flat();
    settle(&mut membrane, 0.8, 3.0);
    assert!(membrane.focus() < -0.05, "{}", membrane.focus());
    assert_eq!(membrane.height(0, 0), 0.0);
    assert_eq!(membrane.height(COLUMNS - 1, ROWS - 1), 0.0);
}

#[test]
fn a_held_well_is_where_a_live_one_comes_to_rest() {
    let mut live = Membrane::flat();
    settle(&mut live, 0.8, 4.0);
    let mut held = Membrane::flat();
    held.hold(0.8);
    assert!(
        (live.focus() - held.focus()).abs() < 0.01,
        "{} {}",
        live.focus(),
        held.focus()
    );
}

#[test]
fn a_kick_rings_and_dies_away() {
    let mut membrane = Membrane::flat();
    membrane.kick(30.0);
    settle(&mut membrane, 0.0, 0.2);
    assert!(!membrane.at_rest());
    settle(&mut membrane, 0.0, 6.0);
    assert!(membrane.at_rest(), "{}", membrane.focus());
}

#[test]
fn the_membrane_stays_bounded_whatever_the_frame_time() {
    let mut membrane = Membrane::flat();
    for dt in [0.001, 0.016, 0.05, 0.5, 3.0] {
        membrane.kick(40.0);
        for _ in 0..40 {
            membrane.step(1.0, dt);
        }
        for k in 0..CELLS {
            let h = membrane.height(k % COLUMNS, k / COLUMNS);
            assert!(h.is_finite() && h.abs() <= 4.0 + 1e-3, "{h}");
        }
    }
}

#[test]
fn the_membrane_settles_to_the_same_place_at_any_frame_rate() {
    let mut fast = Membrane::flat();
    let mut slow = Membrane::flat();
    for _ in 0..360 {
        fast.step(0.7, 1.0 / 120.0);
    }
    for _ in 0..90 {
        slow.step(0.7, 1.0 / 30.0);
    }
    assert!(
        (fast.focus() - slow.focus()).abs() < 0.02,
        "{} {}",
        fast.focus(),
        slow.focus()
    );
}

#[test]
fn a_hard_hit_flashes_and_the_flash_dies_away() {
    let mut well = Well::new(0.0, 1.0);
    well.step(heard(0.0), 1.0, 1.0 / 60.0, false);
    assert!(well.flash() < 0.01);
    well.step(heard(9.0), 1.0, 1.0 / 60.0, false);
    assert!(well.flash() > 0.5, "{}", well.flash());
    for _ in 0..60 {
        well.step(heard(9.0), 1.0, 1.0 / 60.0, false);
    }
    assert!(well.flash() < 0.05, "{}", well.flash());
}

#[test]
fn the_same_hit_flashes_alike_at_thirty_and_a_hundred_and_twenty_frames() {
    let flash = |frame: f32| {
        let mut well = Well::new(0.0, 1.0);
        well.step(heard(0.0), 1.0, frame, false);
        well.step(heard(8.0), 1.0, frame, false);
        well.flash()
    };
    assert!((flash(1.0 / 30.0) - 1.0).abs() < 1e-3);
    assert!(flash(1.0 / 120.0) > 0.9);
}

#[test]
fn a_still_well_holds_its_depth_with_no_flash_and_no_squash() {
    let mut well = Well::new(0.0, 1.0);
    well.step(heard(9.0), 1.0, 1.0 / 60.0, true);
    assert_eq!(well.flash(), 0.0);
    assert_eq!(well.squash(), 0.0);
    assert!(well.membrane().focus() < -0.03);
}

#[test]
fn a_well_with_nothing_to_do_is_idle_and_a_pull_is_not() {
    let mut well = Well::new(0.0, 0.0);
    for _ in 0..240 {
        well.step(heard(0.0), 0.0, 1.0 / 60.0, false);
    }
    assert!(well.idle(0.0));
    well.step(heard(6.0), 1.0, 1.0 / 60.0, false);
    assert!(!well.idle(1.0));
}

#[test]
fn the_ball_rests_lower_asleep_and_lifts_awake() {
    let mut well = Well::new(0.0, 0.0);
    for _ in 0..120 {
        well.step(heard(0.0), 0.0, 1.0 / 60.0, false);
    }
    assert!((well.lift() - well::BALL_RESTING_LIFT).abs() < 0.2);
    for _ in 0..120 {
        well.step(heard(0.0), 1.0, 1.0 / 60.0, false);
    }
    assert!(well.lift().abs() < 0.2);
}

fn scene<'a>(
    painter: &'a eframe::egui::Painter,
    slots: &'a [Slot],
    step: Step,
    awake: f32,
    still: bool,
    reading: Option<MeterReading>,
) -> Scene<'a> {
    let rect = match step {
        Step::Whole => picture(),
        Step::Short => Rect::from_min_size(pos2(14.0, 60.0), vec2(204.0, 102.0)),
        Step::Compact => Rect::from_min_size(pos2(14.0, 60.0), vec2(204.0, 22.0)),
    };
    Scene {
        painter,
        rect,
        step,
        hue: Color32::WHITE,
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
        reduction_db: 4.1,
        input_db: -8.0,
        output_db: -12.1,
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

fn timed(ctx: &eframe::egui::Context, slots: &[Slot], time: f64, live: bool) -> f64 {
    let started = std::time::Instant::now();
    let input = eframe::egui::RawInput {
        time: Some(time),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        let painter = ui.ctx().layer_painter(eframe::egui::LayerId::background());
        let hit = live.then(|| MeterReading {
            reduction_db: 3.0 + 6.0 * ((time * 4.0).sin().abs() as f32),
            input_db: -6.0,
            output_db: -10.0,
        });
        draw(&scene(
            &painter,
            slots,
            Step::Whole,
            f32::from(u8::from(live)),
            false,
            hit,
        ));
    });
    out.textures_delta.clear();
    let meshes = ctx.tessellate(out.shapes, out.pixels_per_point);
    std::hint::black_box(meshes.len());
    started.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore = "a bench: prints the draw and tessellation time of the face while the well rings"]
fn the_cost_of_drawing_the_face_while_the_well_rings() {
    let slots = slots(&[]);
    for (name, live) in [("rest", false), ("live", true)] {
        let ctx = eframe::egui::Context::default();
        crate::fonts::install(&ctx);
        let mut time = 0.0;
        for _ in 0..200 {
            time += 1.0 / 60.0;
            timed(&ctx, &slots, time, live);
        }
        let mut samples: Vec<f64> = (0..300)
            .map(|_| {
                time += 1.0 / 60.0;
                timed(&ctx, &slots, time, live)
            })
            .collect();
        samples.sort_by(f64::total_cmp);
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        println!(
            "GRAVITY {name}: mean {mean:.3} p95 {:.3} worst {:.3} ms",
            samples[samples.len() * 95 / 100],
            samples[samples.len() - 1]
        );
    }
}
