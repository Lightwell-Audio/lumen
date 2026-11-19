use eframe::egui::{Color32, Rect, Shape, pos2, vec2};

use super::geometry::{BEAMS, Frame, SPECIMEN, SPREAD_MOST, scale};
use super::light::{Light, amplitude_of};
use super::picture::{brightness, gain_of};
use super::*;
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

fn whole() -> Rect {
    Rect::from_min_size(pos2(14.0, 60.0), SPECIMEN)
}

fn short() -> Rect {
    Rect::from_min_size(pos2(14.0, 60.0), vec2(204.0, 102.0))
}

fn compact() -> Rect {
    Rect::from_min_size(pos2(14.0, 60.0), vec2(204.0, 22.0))
}

fn specimen_values() -> Vec<Slot> {
    slots(&[(device::WIDTH, 130.0)])
}

#[test]
fn the_card_has_four_dials_a_pill_and_two_handles_on_width() {
    assert_eq!(DRAWING.card, vec2(300.0, 290.0));
    assert_eq!(DRAWING.screen, 136.0);
    assert_eq!(DRAWING.dials().count(), 4);
    assert_eq!(
        DRAWING.params(),
        vec![
            device::GAIN,
            device::PAN,
            device::WIDTH,
            device::BASS_MONO,
            device::POLARITY_L
        ]
    );
    assert_eq!(DRAWING.handles.len(), 2);
    for handle in DRAWING.handles {
        assert_eq!(handle.param, device::WIDTH);
    }
    assert_eq!(DRAWING.handles[0].name, "Beam spread, upper");
    assert_eq!(DRAWING.handles[1].name, "Beam spread, lower");
}

#[test]
fn the_dials_sit_where_the_specimen_puts_them() {
    let card = Rect::from_min_size(pos2(0.0, 0.0), WHOLE);
    let centres: Vec<f32> = (0..4).map(|i| dial(card, i).center().x).collect();
    assert_eq!(centres, vec![44.0, 97.0, 150.0, 203.0]);
    for i in 0..4 {
        assert_eq!(dial(card, i).center().y, 236.0);
    }
    let pill = (DRAWING.controls[4].place)(card);
    assert_eq!(pill.center(), pos2(256.0, 236.0));
    assert_eq!(pill.size(), vec2(40.0, 20.0));
}

#[test]
fn the_frame_matches_the_specimens_numbers_at_its_own_size() {
    let slots = specimen_values();
    let frame = Frame::of(&values(&slots), whole(), false);
    assert_eq!(scale(whole()), 1.0);
    let glass_x = 14.0 + 272.0 * 0.33;
    assert!((frame.glass_point(0).x - glass_x).abs() < 1e-3);
    assert!((frame.entry().x - (glass_x - 12.0)).abs() < 1e-3);
    assert!((frame.exit().x - (glass_x + 13.0)).abs() < 1e-3);
    assert!((frame.exit().y - frame.entry().y + 2.0).abs() < 1e-3);
    assert!((frame.spread() - 0.65 * SPREAD_MOST).abs() < 1e-4);
    let stem = frame.root().x - frame.exit().x;
    let bass = values(&slots).norm(device::BASS_MONO);
    assert!((stem - (4.0 + 34.0 * bass)).abs() < 1e-3);
    assert!((frame.root().x + frame.length() - (14.0 + 272.0 - 6.0)).abs() < 1e-3);
}

#[test]
fn the_middle_beam_runs_level_and_the_outer_beams_leave_by_the_spread() {
    let slots = specimen_values();
    let frame = Frame::of(&values(&slots), whole(), false);
    assert!((frame.tip(2).y - frame.root().y).abs() < 1e-3);
    let up = frame.root().y - frame.tip(0).y;
    let down = frame.tip(BEAMS - 1).y - frame.root().y;
    assert!((up - down).abs() < 1e-3);
    let expected = frame.length() * (2.0 * 0.65 * SPREAD_MOST).sin();
    assert!((up - expected).abs() < 1e-2, "{up} against {expected}");
    assert!((frame.tip(1).y - frame.root().y).abs() < up);
}

#[test]
fn zero_width_folds_every_beam_onto_one() {
    let slots = slots(&[(device::WIDTH, 0.0)]);
    let frame = Frame::of(&values(&slots), whole(), false);
    for beam in 0..BEAMS {
        assert!((frame.tip(beam).y - frame.root().y).abs() < 1e-4);
        assert!((frame.tip(beam).x - frame.tip(2).x).abs() < 1e-4);
    }
}

#[test]
fn pan_moves_the_whole_picture_by_up_to_twelve_points() {
    let centre = slots(&[(device::PAN, 0.0)]);
    let right = slots(&[(device::PAN, 100.0)]);
    let left = slots(&[(device::PAN, -100.0)]);
    let at = |slots: &[Slot]| Frame::of(&values(slots), whole(), false).glass_point(0).y;
    assert!((at(&right) - at(&centre) - 12.0).abs() < 1e-3);
    assert!((at(&centre) - at(&left) - 12.0).abs() < 1e-3);
}

#[test]
fn the_scale_follows_the_picture_and_never_goes_below_a_fifth() {
    assert_eq!(scale(whole()), 1.0);
    assert!((scale(short()) - 0.75).abs() < 1e-4);
    assert!((scale(compact()) - 0.2).abs() < 1e-4);
    assert_eq!(
        scale(Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 400.0))),
        1.0
    );
}

#[test]
fn a_tip_maps_back_to_the_width_at_every_size_and_pan() {
    for rect in [whole(), short()] {
        for pan in [-100.0, 0.0, 60.0] {
            for width in [10.0, 60.0, 130.0, 180.0] {
                let slots = slots(&[(device::WIDTH, width), (device::PAN, pan)]);
                let frame = Frame::of(&values(&slots), rect, false);
                let tip = frame.tip(0);
                let back = frame.width_norm_at(tip.y);
                let want = width / 200.0;
                assert!((back - want).abs() < 1e-3, "{width}: {back} not {want}");
                let below = frame.width_norm_at(frame.tip(BEAMS - 1).y);
                assert!((below - want).abs() < 1e-3);
            }
        }
    }
}

#[test]
fn the_handles_value_is_the_normalised_width_and_clamps_past_the_ends() {
    let slots = specimen_values();
    let values = values(&slots);
    let frame = Frame::of(&values, whole(), false);
    let upper = &DRAWING.handles[0];
    let lower = &DRAWING.handles[1];
    let at_tip = (upper.value_at)(&values, frame.tip(0), whole());
    assert!((at_tip - 0.65).abs() < 1e-3, "{at_tip}");
    let at_lower = (lower.value_at)(&values, frame.tip(BEAMS - 1), whole());
    assert!((at_lower - 0.65).abs() < 1e-3, "{at_lower}");
    let far = (upper.value_at)(&values, pos2(200.0, -500.0), whole());
    let level = (upper.value_at)(&values, pos2(200.0, frame.root().y), whole());
    assert!((far - 1.0).abs() < 1e-6, "{far}");
    assert!(level.abs() < 1e-6, "{level}");
    assert_eq!((upper.chip)(&values, 0.0), "width 0%");
    assert_eq!((upper.chip)(&values, 0.65), "width 130%");
    assert_eq!((upper.chip)(&values, 1.0), "width 200%");
}

#[test]
fn a_grab_is_inside_the_picture_and_reachable_at_every_width_and_pan() {
    for rect in [whole(), short()] {
        for pan in [-100.0, 0.0, 100.0] {
            for width in [0.0, 50.0, 130.0, 200.0] {
                let slots = slots(&[(device::WIDTH, width), (device::PAN, pan)]);
                let values = values(&slots);
                let frame = Frame::of(&values, rect, false);
                for (index, upper) in [(0, true), (1, false)] {
                    let grab = (DRAWING.handles[index].grab)(&values, rect);
                    assert!(grab.width() > 20.0 && grab.height() > 5.0, "{grab:?}");
                    assert!(rect.contains_rect(grab), "{grab:?} in {rect:?}");
                    let pinned = frame.pinned_tip(upper);
                    assert!(rect.contains(pinned), "{pinned:?} in {rect:?}");
                    assert!(grab.contains(pinned), "{grab:?} holds {pinned:?}");
                }
            }
        }
    }
}

#[test]
fn a_grab_sits_on_the_tip_when_the_tip_is_on_the_screen() {
    let slots = specimen_values();
    let values = values(&slots);
    let frame = Frame::of(&values, whole(), false);
    let upper = (DRAWING.handles[0].grab)(&values, whole());
    let lower = (DRAWING.handles[1].grab)(&values, whole());
    assert!(upper.contains(frame.tip(0)));
    assert!(lower.contains(frame.tip(BEAMS - 1)));
    assert!(!upper.contains(frame.tip(BEAMS - 1)));
    assert!(upper.left() >= whole().left() + 0.6 * 272.0 - 1e-3);
}

#[test]
fn in_the_band_the_fan_is_fitted_so_every_tip_stays_inside() {
    for width in [20.0, 130.0, 200.0] {
        let slots = slots(&[(device::WIDTH, width)]);
        let frame = Frame::of(&values(&slots), compact(), true);
        for beam in 0..BEAMS {
            let tip = frame.tip(beam);
            assert!(
                compact().contains(tip),
                "width {width}, beam {beam}: {tip:?} in {:?}",
                compact()
            );
        }
    }
}

#[test]
fn only_the_band_bends_the_fan() {
    let slots = slots(&[(device::WIDTH, 200.0)]);
    let free = Frame::of(&values(&slots), compact(), false);
    let fitted = Frame::of(&values(&slots), compact(), true);
    let whole_free = Frame::of(&values(&slots), whole(), false);
    let whole_fitted = Frame::of(&values(&slots), whole(), true);
    assert!(free.root().y - free.tip(0).y > 15.0);
    assert!(fitted.root().y - fitted.tip(0).y < 9.0);
    assert!((whole_free.tip(0).y - whole_fitted.tip(0).y).abs() < 1e-3);
}

#[test]
fn a_level_reads_as_an_amplitude() {
    assert_eq!(amplitude_of(0.0), 0.0);
    assert!((amplitude_of(1.0) - 1.0).abs() < 1e-6);
    let half = amplitude_of(0.5);
    assert!((half - 10.0_f32.powf(-30.0 / 20.0)).abs() < 1e-4, "{half}");
    assert!(amplitude_of(0.9) > amplitude_of(0.8));
}

#[test]
fn the_light_rises_in_a_few_milliseconds_and_falls_over_a_few_hundred() {
    let mut light = Light::new(0.0);
    light.step(1.0, 0.016, false);
    assert!(light.energy() > 0.95, "attack: {}", light.energy());
    light.step(0.0, 0.016, false);
    assert!(light.energy() > 0.8, "release is slow: {}", light.energy());
    for _ in 0..60 {
        light.step(0.0, 0.016, false);
    }
    assert!(light.energy() < 0.01);
}

#[test]
fn a_loud_hit_flashes_and_the_stem_blooms_then_both_settle() {
    let mut light = Light::new(0.0);
    for _ in 0..10 {
        light.step(0.95, 1.0 / 60.0, false);
    }
    assert!(light.flash() > 0.9);
    assert!(light.bloom() > 0.05, "{}", light.bloom());
    assert!(light.bloom() <= 1.6);
    assert!(!light.idle());
    for _ in 0..240 {
        light.step(0.0, 1.0 / 60.0, false);
    }
    assert!(light.idle(), "{light:?}");
}

#[test]
fn a_quiet_signal_does_not_flash() {
    let mut light = Light::new(0.0);
    for _ in 0..30 {
        light.step(0.4, 1.0 / 60.0, false);
    }
    assert!(light.flash() < 1e-6);
    assert!(light.bloom().abs() < 1e-6);
}

#[test]
fn the_bloom_is_the_same_whatever_the_frame_rate() {
    let run = |frames: u32| {
        let mut light = Light::new(0.0);
        for _ in 0..frames {
            light.step(0.95, 0.12 / frames as f32, false);
        }
        light.bloom()
    };
    let (slow, fast) = (run(8), run(32));
    assert!((slow - fast).abs() < 0.1, "{slow} against {fast}");
}

#[test]
fn reduce_motion_snaps_the_light_and_makes_no_flash_or_bloom() {
    let mut light = Light::new(0.0);
    light.step(0.9, 1.0 / 60.0, true);
    assert!((light.energy() - 0.9).abs() < 1e-6);
    assert_eq!(light.flash(), 0.0);
    assert_eq!(light.bloom(), 0.0);
    assert!(!light.idle());
    light.step(0.0, 1.0 / 60.0, true);
    assert!(light.idle());
}

#[test]
fn a_stall_cannot_jump_the_light() {
    let mut light = Light::new(0.0);
    light.step(1.0, 5.0, false);
    assert!(light.energy() <= 1.0);
    assert!(light.bloom() <= 1.5 + 1e-6);
}

#[test]
fn brightness_is_dim_at_rest_and_brighter_awake_with_the_level() {
    let rest = brightness(0.0, 0.0, 0.0);
    let awake = brightness(1.0, 0.0, 0.0);
    let loud = brightness(1.0, 1.0, 0.0);
    assert!((rest - 0.14).abs() < 1e-6);
    assert!((awake - 0.74).abs() < 1e-6);
    assert!(loud > awake && loud <= 1.0);
}

#[test]
fn the_gain_dial_scales_the_beams_between_a_quarter_and_one_and_two_fifths() {
    assert!((gain_of(0.0) - 1.0).abs() < 1e-6);
    assert!((gain_of(-60.0) - 0.25).abs() < 1e-6);
    assert!((gain_of(24.0) - 1.4).abs() < 1e-6);
    assert!(gain_of(-6.0) < 1.0 && gain_of(-6.0) > 0.25);
    assert!(brightness(1.0, 0.0, -60.0) < brightness(1.0, 0.0, 0.0));
}

fn scene<'a>(
    painter: &'a eframe::egui::Painter,
    slots: &'a [Slot],
    step: Step,
    level: f32,
    awake: f32,
    still: bool,
) -> Scene<'a> {
    let rect = match step {
        Step::Whole => whole(),
        Step::Short => short(),
        Step::Compact => compact(),
    };
    Scene {
        painter,
        rect,
        step,
        hue: Color32::WHITE,
        values: values(slots),
        power: 1.0,
        level,
        awake,
        still,
        handle_lit: [0.0; super::super::MAX_HANDLES],
        settle: 1.0,
        picked: 0,
        parts_lit: [0.0; super::super::MAX_PARTS],
        spectrum: None,
        mist: 0.0,
        bpm: 120.0,
        reading: None,
        id: eframe::egui::Id::new("test card"),
    }
}

struct Painted {
    shapes: usize,
    vertices: usize,
}

fn frame(
    ctx: &eframe::egui::Context,
    slots: &[Slot],
    step: Step,
    level: f32,
    awake: f32,
    still: bool,
    time: f64,
) -> Painted {
    crate::fonts::install(ctx);
    let input = eframe::egui::RawInput {
        time: Some(time),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        let painter = ui.ctx().layer_painter(eframe::egui::LayerId::background());
        draw(&scene(&painter, slots, step, level, awake, still));
    });
    out.textures_delta.clear();
    let vertices = out
        .shapes
        .iter()
        .map(|clipped| match &clipped.shape {
            Shape::Mesh(mesh) => mesh.vertices.len(),
            _ => 0,
        })
        .sum();
    Painted {
        shapes: out.shapes.len(),
        vertices,
    }
}

#[test]
fn every_step_draws_at_rest_live_and_still() {
    let slots = specimen_values();
    for step in [Step::Whole, Step::Short, Step::Compact] {
        let ctx = eframe::egui::Context::default();
        let mut time = 0.0;
        for (level, awake, still) in [
            (0.0, 0.0, false),
            (0.9, 1.0, false),
            (0.0, 0.0, true),
            (0.9, 1.0, true),
        ] {
            for _ in 0..3 {
                time += 1.0 / 60.0;
                assert!(frame(&ctx, &slots, step, level, awake, still, time).shapes > 0);
            }
        }
    }
}

#[test]
fn a_frame_is_a_handful_of_shapes_and_a_bounded_number_of_vertices() {
    let slots = specimen_values();
    let ctx = eframe::egui::Context::default();
    let mut time = 0.0;
    let mut most = (0, 0);
    for _ in 0..30 {
        time += 1.0 / 60.0;
        let painted = frame(&ctx, &slots, Step::Whole, 1.0, 1.0, false, time);
        most = (most.0.max(painted.shapes), most.1.max(painted.vertices));
    }
    assert!(most.0 <= 16, "{} shapes", most.0);
    assert!(most.1 <= 1500, "{} vertices", most.1);
}

#[test]
fn reduce_motion_draws_no_packets_and_the_live_face_draws_them() {
    let slots = specimen_values();
    let live = {
        let ctx = eframe::egui::Context::default();
        frame(&ctx, &slots, Step::Whole, 0.6, 1.0, false, 1.0).vertices
    };
    let still = {
        let ctx = eframe::egui::Context::default();
        frame(&ctx, &slots, Step::Whole, 0.6, 1.0, true, 1.0).vertices
    };
    assert!(live > still + 300, "live {live} against still {still}");
}

#[test]
fn at_rest_the_wedges_are_still_drawn_and_no_packets_run() {
    let slots = specimen_values();
    let ctx = eframe::egui::Context::default();
    let rest = frame(&ctx, &slots, Step::Whole, 0.0, 0.0, false, 1.0).vertices;
    let ctx = eframe::egui::Context::default();
    let live = frame(&ctx, &slots, Step::Whole, 0.6, 1.0, false, 1.0).vertices;
    assert!(rest > 200, "the fan is drawn at rest: {rest}");
    assert!(live > rest + 300, "live {live} against rest {rest}");
}

#[test]
fn the_resting_light_leaves_nothing_behind() {
    let slots = specimen_values();
    let ctx = eframe::egui::Context::default();
    let mut time = 0.0;
    for _ in 0..5 {
        time += 1.0 / 60.0;
        frame(&ctx, &slots, Step::Whole, 0.9, 1.0, false, time);
    }
    for _ in 0..240 {
        time += 1.0 / 60.0;
        frame(&ctx, &slots, Step::Whole, 0.0, 0.0, false, time);
    }
    let key = eframe::egui::Id::new("test card").with("fan light");
    assert!(ctx.data(|d| d.get_temp::<Light>(key)).is_none());
}

fn timed(ctx: &eframe::egui::Context, slots: &[Slot], time: f64, live: bool) -> f64 {
    let started = std::time::Instant::now();
    let input = eframe::egui::RawInput {
        time: Some(time),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        let painter = ui.ctx().layer_painter(eframe::egui::LayerId::background());
        let level = if live {
            0.6 + 0.4 * ((time * 6.0).sin().abs() as f32)
        } else {
            0.0
        };
        draw(&scene(
            &painter,
            slots,
            Step::Whole,
            level,
            f32::from(u8::from(live)),
            false,
        ));
    });
    out.textures_delta.clear();
    let meshes = ctx.tessellate(out.shapes, out.pixels_per_point);
    std::hint::black_box(meshes.len());
    started.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore = "a bench: prints the draw and tessellation time of the face at rest and with signal"]
fn the_cost_of_drawing_the_face_at_rest_and_with_signal() {
    let slots = specimen_values();
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
            "PRISM {name}: mean {mean:.3} p95 {:.3} worst {:.3} ms",
            samples[samples.len() * 95 / 100],
            samples[samples.len() - 1]
        );
    }
}
