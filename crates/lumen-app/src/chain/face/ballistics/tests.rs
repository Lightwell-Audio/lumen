use super::*;

fn run_at(fps: f32, seconds: f32, mut each: impl FnMut(f32, f32)) {
    let dt = 1.0 / fps;
    let frames = (seconds * fps).round() as usize;
    for frame in 0..frames {
        each(frame as f32 * dt, dt);
    }
}

#[test]
fn a_spring_lands_on_its_target_and_the_presets_overshoot_by_their_names() {
    let mut max = 0.0_f32;
    let mut spring = Spring::new(motion::SNAP, 0.0);
    run_at(240.0, 1.5, |_, dt| {
        max = max.max(spring.step(1.0, dt, false));
    });
    assert!(spring.settled_on(1.0), "{}", spring.at());
    assert!((max - 1.0 - 0.126).abs() < 0.01, "SNAP overshot {}", max - 1.0);

    let mut max = 0.0_f32;
    let mut pop = Spring::new(motion::POP, 0.0);
    run_at(240.0, 1.5, |_, dt| {
        max = max.max(pop.step(1.0, dt, false));
    });
    assert!((max - 1.0 - 0.28).abs() < 0.02, "POP overshot {}", max - 1.0);

    let mut soft = Spring::new(motion::SOFT, 0.0);
    let mut max = 0.0_f32;
    run_at(240.0, 1.5, |_, dt| {
        max = max.max(soft.step(1.0, dt, false));
    });
    assert!(max <= 1.0 + 1e-4, "SOFT never overshoots: {max}");
}

#[test]
fn a_spring_agrees_across_frame_rates() {
    let target = |t: f32| if t < 0.4 { 1.0 } else { 0.25 };
    let at = |fps: f32| {
        let mut spring = Spring::new(motion::SNAP, 0.0);
        let mut last = 0.0;
        run_at(fps, 0.8, |t, dt| {
            last = spring.step(target(t), dt, false);
        });
        last
    };
    let (slow, fast) = (at(30.0), at(120.0));
    assert!((slow - fast).abs() < 0.02, "30 fps {slow}, 120 fps {fast}");
}

#[test]
fn a_spring_with_still_motion_snaps_and_a_stalled_frame_does_not_jump() {
    let mut spring = Spring::new(motion::POP, 0.0);
    assert_eq!(spring.step(0.7, 0.016, true), 0.7);
    assert!(spring.settled_on(0.7));

    let mut stalled = Spring::new(motion::SNAP, 0.0);
    let jumped = stalled.step(1.0, 5.0, false);
    let mut held = Spring::new(motion::SNAP, 0.0);
    let capped = held.step(1.0, motion::DT_MAX, false);
    assert_eq!(jumped, capped);
}

#[test]
fn a_meter_jumps_up_and_eases_down() {
    let mut meter = Ballistic::default();
    let up = meter.step(1.0, 0.016, false);
    assert!(up > 0.95, "attack: {up}");
    let mut was = meter.value();
    let mut ticks = 0;
    while meter.value() > 0.1 {
        let now = meter.step(0.0, 0.016, false);
        assert!(now < was, "falls every frame");
        was = now;
        ticks += 1;
        assert!(ticks < 200);
    }
    assert!(ticks >= 8, "release is eased, not instant: {ticks} frames");
}

#[test]
fn the_peak_holds_then_falls() {
    let mut meter = Ballistic::default();
    meter.step(1.0, 0.016, false);
    let held = meter.peak();
    run_at(60.0, 0.5, |_, dt| {
        meter.step(0.0, dt, false);
    });
    assert_eq!(meter.peak(), held, "held through the hold");
    run_at(60.0, 0.5, |_, dt| {
        meter.step(0.0, dt, false);
    });
    assert!(meter.peak() < held, "falling after the hold");
    run_at(60.0, 2.0, |_, dt| {
        meter.step(0.0, dt, false);
    });
    assert!(meter.rests_at(0.01), "{} {}", meter.value(), meter.peak());
}

#[test]
fn a_meter_agrees_across_frame_rates() {
    let level = |t: f32| if (t * 4.0).fract() < 0.25 { 0.9 } else { 0.0 };
    let at = |fps: f32| {
        let mut meter = Ballistic::default();
        let mut sum = 0.0;
        let mut n = 0.0;
        run_at(fps, 2.0, |t, dt| {
            sum += meter.step(level(t), dt, false);
            n += 1.0;
        });
        (sum / n, meter.peak())
    };
    let (slow, fast) = (at(30.0), at(120.0));
    assert!((slow.0 - fast.0).abs() < 0.03, "mean {slow:?} vs {fast:?}");
    assert!((slow.1 - fast.1).abs() < 0.1, "peak {slow:?} vs {fast:?}");
}

#[test]
fn a_meter_with_still_motion_shows_the_value_as_it_is() {
    let mut meter = Ballistic::default();
    assert_eq!(meter.step(0.6, 0.016, true), 0.6);
    assert_eq!(meter.step(0.2, 0.016, true), 0.2);
    assert_eq!(meter.peak(), 0.2);
}

#[test]
fn a_gain_reduction_meter_can_let_go_as_its_device_does() {
    let mut slow = Ballistic::new(motion::METER_ATTACK, 0.6);
    let mut quick = Ballistic::default();
    slow.step(1.0, 0.016, false);
    quick.step(1.0, 0.016, false);
    slow.step(0.0, 0.1, false);
    quick.step(0.0, 0.1, false);
    assert!(slow.value() > quick.value());
}

#[test]
fn a_face_sleeps_after_the_hold_and_wakes_at_once() {
    let mut rest = Rest::default();
    assert!(rest.asleep());
    rest.step(-20.0, 0.016, false);
    assert!(rest.awake() > 0.2, "waking: {}", rest.awake());
    for _ in 0..10 {
        rest.step(-20.0, 0.016, false);
    }
    assert!(rest.awake() > 0.9);

    let mut quiet_for = 0.0;
    while !rest.asleep() {
        rest.step(f32::NEG_INFINITY, 0.016, false);
        quiet_for += 0.016;
        assert!(quiet_for < 3.0, "settles");
    }
    assert!(quiet_for > motion::SLEEP_HOLD, "held for the pause: {quiet_for}");
    assert_eq!(rest.awake(), 0.0);
}

#[test]
fn a_pause_between_notes_does_not_put_a_face_to_sleep() {
    let mut rest = Rest::default();
    for _ in 0..10 {
        rest.step(-20.0, 0.016, false);
    }
    run_at(60.0, 0.3, |_, dt| {
        rest.step(f32::NEG_INFINITY, dt, false);
    });
    assert!(rest.awake() > 0.9, "{}", rest.awake());
    rest.step(-20.0, 0.016, false);
    assert!(!rest.asleep());
}

#[test]
fn rest_agrees_across_frame_rates_and_still_motion_decides_at_once() {
    let at = |fps: f32| {
        let mut rest = Rest::default();
        run_at(fps, 0.3, |_, dt| {
            rest.step(-20.0, dt, false);
        });
        run_at(fps, 0.6, |_, dt| {
            rest.step(f32::NEG_INFINITY, dt, false);
        });
        rest.awake()
    };
    let (slow, fast) = (at(30.0), at(120.0));
    assert!((slow - fast).abs() < 0.05, "{slow} vs {fast}");

    let mut still = Rest::default();
    assert_eq!(still.step(-20.0, 0.016, true), 1.0);
    for _ in 0..40 {
        still.step(f32::NEG_INFINITY, 0.016, true);
    }
    assert!(still.asleep());
}

#[test]
fn the_meter_scale_maps_to_decibels_and_nothing_to_silence() {
    assert_eq!(db_of_level(0.0), f32::NEG_INFINITY);
    assert!((db_of_level(1.0)).abs() < 1e-4);
    assert!((db_of_level(0.5) - c::LEVEL_FLOOR_DB / 2.0).abs() < 1e-4);
    assert!(db_of_level(0.001) > motion::SILENCE_DB);
}

#[test]
fn glow_follows_energy_while_awake_and_a_hit_reaches_full() {
    let rest = glow_share(0.0, 1.0, 0.0);
    assert!((rest - motion::GLOW_REST).abs() < 1e-6, "{rest}");
    let quiet = glow_share(1.0, 0.0, 0.0);
    assert!((quiet - motion::GLOW_LIVE).abs() < 1e-6);
    let loud = glow_share(1.0, 1.0, 0.0);
    assert!((loud - (motion::GLOW_LIVE + motion::GLOW_ENERGY)).abs() < 1e-6);
    assert!(rest < quiet && quiet < loud);
    assert_eq!(glow_share(1.0, 0.2, 1.0), motion::GLOW_HIT);
    assert!(glow_share(5.0, 5.0, 5.0) <= 1.0);
}
