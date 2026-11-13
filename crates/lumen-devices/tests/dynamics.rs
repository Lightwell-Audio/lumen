//! Gravity, the compressor, and Horizon, the limiter (M8 slice B2): what they
//! do to the audio, that they do the same thing at every block size, that the
//! limiter never lets a sample past its ceiling, and that neither allocates
//! while it runs.

use lumen_core::loudness::TruePeak;
use lumen_core::pan::PanLaw;
use lumen_devices::dynamics::compressor;
use lumen_devices::dynamics::limiter;
use lumen_devices::params;
use lumen_devices::{Block, Compressor, Effect, Key, Limiter, ParamBlock, Stereo};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;

/// The normalised value of `id` at `value` in its own units, from `table`.
fn at(table: &'static [lumen_devices::ParamInfo], id: u32, value: f32) -> f32 {
    params::find(table, id)
        .expect("a parameter of the table")
        .normalised(value)
}

/// A block: every parameter at rest as `held` says, then the lanes' points.
struct BlockBuilder {
    frames: usize,
    points: Vec<(u32, Vec<(u32, f64)>)>,
}

impl BlockBuilder {
    fn new(frames: usize) -> Self {
        Self {
            frames,
            points: Vec::new(),
        }
    }

    /// Parameter `id` at rest at `value` (normalised).
    fn held(mut self, id: u32, value: f32) -> Self {
        self.points.push((id, vec![(0, f64::from(value))]));
        self
    }

    /// Parameter `id` driven by a lane of `points`.
    fn lane(mut self, id: u32, points: &[(u32, f32)]) -> Self {
        self.points.push((
            id,
            points
                .iter()
                .map(|&(at, value)| (at, f64::from(value)))
                .collect(),
        ));
        self
    }

    fn build(&self) -> ParamBlock {
        let mut events = ParamBlock::new();
        events.start(self.frames);
        for (id, points) in &self.points {
            events.open(*id);
            for &(at, value) in points {
                events.point(at, value);
            }
        }
        events
    }
}

/// Every parameter of `table` at rest at its default, with `set`'s values
/// (normalised).
fn defaults(
    table: &'static [lumen_devices::ParamInfo],
    frames: usize,
    set: &[(u32, f32)],
) -> BlockBuilder {
    let mut block = BlockBuilder::new(frames);
    for info in table {
        let value = set
            .iter()
            .find(|(id, _)| *id == info.id)
            .map_or_else(|| info.normalised(info.default), |(_, value)| *value);
        block = block.held(info.id, value);
    }
    block
}

/// A lane's value at an absolute frame: straight between its points.
fn curve_at(points: &[(u32, f32)], frame: usize) -> f32 {
    let after = points.partition_point(|p| p.0 as usize <= frame);
    let Some(before) = after.checked_sub(1) else {
        return points[0].1;
    };
    let (at, value) = points[before];
    match points.get(after) {
        Some(&(next, next_value)) if next as usize > frame => {
            let span = (next - at) as f32;
            let into = (frame - at as usize) as f32;
            value + (next_value - value) * (into / span)
        }
        _ => value,
    }
}

/// The points a lane gives one block, as the engine fills them.
fn window(points: &[(u32, f32)], done: usize, n: usize) -> Vec<(u32, f32)> {
    let mut entries: Vec<(usize, f32)> = vec![(0, curve_at(points, done))];
    for &(at, value) in points {
        let at = at as usize;
        if at > done && at < done + n {
            entries.push((at - done, value));
        }
    }
    if n > 1 && !entries.iter().any(|&(offset, _)| offset == n - 1) {
        entries.push((n - 1, curve_at(points, done + n - 1)));
    }
    entries
        .into_iter()
        .map(|(offset, value)| (offset as u32, value))
        .collect()
}

/// Runs `device` over `left` and `right`, a block of at most 256 frames each
/// (the most a device is given), with every parameter at rest from `set` and
/// `lanes`' points. One argument for each of the things a block carries (the
/// table it is read through, the values at rest, the lanes and the key), as
/// the engine hands them to a device.
#[allow(clippy::too_many_arguments)]
fn run(
    device: &mut dyn Effect,
    left: &mut [f32],
    right: &mut [f32],
    table: &'static [lumen_devices::ParamInfo],
    block: usize,
    set: &[(u32, f32)],
    lanes: &[(u32, Vec<(u32, f32)>)],
    key: Option<(&[f32], &[f32])>,
) {
    let block = block.clamp(1, 256);
    let mut done = 0;
    while done < left.len() {
        let n = block.min(left.len() - done);
        let mut events = defaults(table, n, set);
        for (id, points) in lanes {
            events = events.lane(*id, &window(points, done, n));
        }
        let events = events.build();
        let block = Block {
            rate: RATE,
            start: done as i64,
            law: PanLaw::Balance,
            events: &events,
        };
        let key = key.map(|(left, right)| Key {
            left: &left[done..done + n],
            right: &right[done..done + n],
        });
        device.process(
            &block,
            Stereo {
                left: &mut left[done..done + n],
                right: &mut right[done..done + n],
            },
            key,
        );
        done += n;
    }
}

/// A constant level on both sides, at `amplitude`.
fn level(frames: usize, amplitude: f32) -> (Vec<f32>, Vec<f32>) {
    (vec![amplitude; frames], vec![amplitude; frames])
}

/// The loudest sample of a stretch.
fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn db_of(amplitude: f32) -> f32 {
    20.0 * amplitude.max(1e-12).log10()
}

// ---------------------------------------------------------------- Gravity

#[test]
fn at_its_defaults_every_parameter_names_its_bounds() {
    assert_eq!(compressor::NAME, "compressor");
    assert!(lumen_devices::effect_params(compressor::NAME).is_some());
    // The four the face shows come first, so the compact card keeps them.
    let first: Vec<&str> = compressor::TABLE.iter().take(4).map(|p| p.name).collect();
    assert_eq!(first, ["Pull", "Ratio", "Attack", "Release"]);
    // Its defaults compress −12 dBFS by the curve, plus the automatic make-up.
    let frames = 24_000;
    let (mut left, mut right) = level(frames, 0.25);
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut left,
        &mut right,
        compressor::TABLE,
        256,
        &[],
        &[],
        None,
    );
    let want =
        0.25 * 10f32.powf((compressor::curve_db(-12.04, -18.0, 4.0, 6.0) + 12.04 + 13.5) / 20.0);
    assert!(
        (left[frames - 1] - want).abs() < 1e-3,
        "{} against {want}",
        left[frames - 1]
    );
}

/// The static curve, end to end: a steady level comes out where the gain
/// computer says, to within a tenth of a decibel, at every level across the
/// knee (make-up off, so the gain is the curve alone).
#[test]
fn a_steady_level_follows_the_static_curve_within_a_tenth_of_a_decibel() {
    let frames = 96_000;
    for input_db in [-30.0f32, -21.0, -18.0, -15.0, -12.0, -6.0, -1.0] {
        for (ratio, knee) in [(4.0f32, 6.0f32), (4.0, 0.0), (12.0, 6.0), (1.0, 6.0)] {
            let amplitude = 10f32.powf(input_db / 20.0);
            let (mut left, mut right) = level(frames, amplitude);
            let mut device = Compressor::new();
            run(
                &mut device,
                &mut left,
                &mut right,
                compressor::TABLE,
                256,
                &[
                    (
                        compressor::THRESHOLD,
                        at(compressor::TABLE, compressor::THRESHOLD, -18.0),
                    ),
                    (
                        compressor::RATIO,
                        at(compressor::TABLE, compressor::RATIO, ratio),
                    ),
                    (
                        compressor::KNEE,
                        at(compressor::TABLE, compressor::KNEE, knee),
                    ),
                    (compressor::MAKEUP, 0.0),
                ],
                &[],
                None,
            );
            let got_db = db_of(left[frames - 1]);
            let want_db = compressor::curve_db(input_db, -18.0, ratio, knee);
            assert!(
                (got_db - want_db).abs() < 0.1,
                "{input_db} dB at {ratio}:1, {knee} dB knee: {got_db} against {want_db}"
            );
        }
    }
}

/// Automatic make-up gives back what the loud end loses, so a level well over
/// the threshold comes out at the ceiling's own level.
#[test]
fn automatic_make_up_keeps_the_loud_end_where_it_was() {
    let frames = 96_000;
    let (mut left, mut right) = level(frames, 1.0);
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut left,
        &mut right,
        compressor::TABLE,
        256,
        &[
            (
                compressor::THRESHOLD,
                at(compressor::TABLE, compressor::THRESHOLD, -18.0),
            ),
            (
                compressor::RATIO,
                at(compressor::TABLE, compressor::RATIO, 4.0),
            ),
            (compressor::KNEE, 0.0),
        ],
        &[],
        None,
    );
    // 0 dBFS at −18 dB and 4:1 comes out at −13.5 dB, and the make-up is
    // exactly those 13.5 dB.
    let got = left[frames - 1];
    assert!((got - 1.0).abs() < 0.005, "{got}");
}

/// The meter the card's face reads says how much it took away, and the level
/// that drove it.
#[test]
fn the_gain_reduction_meter_says_what_it_took_away() {
    let mut device = Compressor::new();
    let meter = device.meter();
    let (mut left, mut right) = level(4_800, 1.0);
    run(
        &mut device,
        &mut left,
        &mut right,
        compressor::TABLE,
        256,
        &[
            (
                compressor::THRESHOLD,
                at(compressor::TABLE, compressor::THRESHOLD, -18.0),
            ),
            (
                compressor::RATIO,
                at(compressor::TABLE, compressor::RATIO, 4.0),
            ),
            (compressor::KNEE, 0.0),
            (compressor::MAKEUP, 0.0),
        ],
        &[],
        None,
    );
    let reading = meter.take();
    assert!(
        (reading.reduction_db - 13.5).abs() < 0.2,
        "{}",
        reading.reduction_db
    );
    assert!(reading.input_db > -0.1, "{}", reading.input_db);
    // Read: the count starts again.
    assert_eq!(meter.take().reduction_db, 0.0);
    // Under the threshold nothing is taken away, on a device that is new (the
    // one above is still letting go of what it took).
    let mut device = Compressor::new();
    let meter = device.meter();
    let (mut left, mut right) = level(2_048, 0.05);
    run(
        &mut device,
        &mut left,
        &mut right,
        compressor::TABLE,
        256,
        &[],
        &[],
        None,
    );
    assert_eq!(meter.take().reduction_db, 0.0);
}

/// A key from another track ducks the input: the same audio is pulled down
/// by what arrives on the sidechain, and `Listen` plays the key.
#[test]
fn a_key_ducks_the_input_and_listen_plays_the_key() {
    // The input is quiet (under the threshold); the key is loud.
    let frames = 48_000;
    let (quiet_l, quiet_r) = level(frames, 0.05);
    let (loud_l, loud_r) = level(frames, 1.0);
    let set = |listen: f32| {
        [
            (
                compressor::THRESHOLD,
                at(compressor::TABLE, compressor::THRESHOLD, -30.0),
            ),
            (
                compressor::RATIO,
                at(compressor::TABLE, compressor::RATIO, 8.0),
            ),
            (compressor::KNEE, 0.0),
            (compressor::MAKEUP, 0.0),
            (compressor::LISTEN, listen),
        ]
    };
    let mut keyed = (quiet_l.clone(), quiet_r.clone());
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut keyed.0,
        &mut keyed.1,
        compressor::TABLE,
        256,
        &set(0.0),
        &[],
        Some((&loud_l, &loud_r)),
    );
    // 40 dB under the key, at 8:1: about 35 dB of reduction.
    let ducked = db_of(keyed.0[frames - 1]);
    assert!(ducked < -40.0, "the key ducks it: {ducked}");
    // With no key, the audio is only what its own level asks for: nothing of
    // the key pulls it down.
    let mut plain = (quiet_l.clone(), quiet_r.clone());
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut plain.0,
        &mut plain.1,
        compressor::TABLE,
        256,
        &set(0.0),
        &[],
        None,
    );
    let alone = db_of(plain.0[frames - 1]);
    assert!(alone > -30.0, "unkeyed it passes: {alone}");
    assert!(alone - ducked > 20.0, "{alone} against {ducked}");
    // Listening plays the key over the output, at its own level.
    let mut heard = (quiet_l.clone(), quiet_r.clone());
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut heard.0,
        &mut heard.1,
        compressor::TABLE,
        256,
        &set(1.0),
        &[],
        Some((&loud_l, &loud_r)),
    );
    assert!(
        (heard.0[frames - 1] - 1.0).abs() < 0.05,
        "the key is heard: {}",
        heard.0[frames - 1]
    );
}

/// The key's high-pass takes the low end out of what keys the compressor, so a
/// kick's body stops ducking it while its click still does.
#[test]
fn the_key_high_pass_takes_the_low_end_out_of_the_key() {
    let frames = 48_000;
    // The input is silent, so what is heard back is the key alone.
    let (quiet_l, quiet_r) = level(frames, 0.0);
    let (low_l, low_r) = level(frames, 1.0);
    let mut listen = (quiet_l.clone(), quiet_r.clone());
    let mut device = Compressor::new();
    let set = [
        (compressor::LISTEN, 1.0),
        (
            compressor::KEY_HPF,
            at(compressor::TABLE, compressor::KEY_HPF, 500.0),
        ),
        (compressor::MAKEUP, 0.0),
    ];
    run(
        &mut device,
        &mut listen.0,
        &mut listen.1,
        compressor::TABLE,
        256,
        &set,
        &[],
        Some((&low_l, &low_r)),
    );
    // A constant key is all low end: the high-pass takes it to nothing.
    assert!(
        listen.0[frames - 1].abs() < 1e-3,
        "the low end is gone: {}",
        listen.0[frames - 1]
    );
}

/// Every parameter moving at once (a lane for each) plays the same audio at
/// every block size.
#[test]
fn gravity_is_the_same_at_any_block_size() {
    let frames = 4_800;
    let lane = |scale: f32| {
        (0..40)
            .map(|k| {
                (
                    (k * 120) as u32,
                    (0.3 + 0.5 * ((k as f32 * scale).sin() * 0.5 + 0.5)).clamp(0.0, 1.0),
                )
            })
            .collect::<Vec<(u32, f32)>>()
    };
    let threshold = lane(0.7);
    let ratio = lane(1.1);
    let attack = lane(1.7);
    let release = lane(0.4);
    let knee = lane(2.3);
    let mix = lane(1.3);
    let input = |k: usize| {
        let x = k as f32 * 0.017;
        ((x * 31.0).sin() * 0.4, (x * 17.0).cos() * 0.3)
    };
    let render = |block: usize| {
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (0..frames).map(input).unzip();
        let mut device = Compressor::new();
        let lanes = [
            (compressor::THRESHOLD, threshold.clone()),
            (compressor::RATIO, ratio.clone()),
            (compressor::ATTACK, attack.clone()),
            (compressor::RELEASE, release.clone()),
            (compressor::KNEE, knee.clone()),
            (compressor::MIX, mix.clone()),
        ];
        run(
            &mut device,
            &mut left,
            &mut right,
            compressor::TABLE,
            block,
            &[],
            &lanes,
            None,
        );
        left.into_iter().zip(right).collect::<Vec<(f32, f32)>>()
    };
    let whole = render(256);
    for block in [64usize, 97, 128, 512, 1024] {
        let parts = render(block);
        for (k, ((a, b), (c, d))) in whole.iter().zip(&parts).enumerate() {
            assert!(
                (a - c).abs() < 2e-6 && (b - d).abs() < 2e-6,
                "block {block} frame {k}: ({a}, {b}) against ({c}, {d})"
            );
        }
    }
}

/// The audio thread's rules: a block with automation and a key allocates
/// nothing.
#[test]
fn gravity_never_touches_the_heap() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is installed"
    );
    let mut device = Compressor::new();
    let mut left = vec![0.3; 256];
    let mut right = vec![-0.3; 256];
    let key = (vec![0.2; 256], vec![0.2; 256]);
    let events = defaults(compressor::TABLE, 256, &[(compressor::MIX, 0.5)])
        .lane(
            compressor::THRESHOLD,
            &[(0, 0.1), (40, 0.9), (41, 0.2), (200, 0.6)],
        )
        .lane(compressor::RATIO, &[(0, 0.1), (128, 0.9)])
        .lane(compressor::ATTACK, &[(0, 0.5), (255, 0.4)])
        .build();
    let block = Block {
        rate: RATE,
        start: 0,
        law: PanLaw::ConstantPower,
        events: &events,
    };
    let (_, violations) = lumen_rt::count_rt_violations(|| {
        device.process(
            &block,
            Stereo {
                left: &mut left,
                right: &mut right,
            },
            Some(Key {
                left: &key.0,
                right: &key.1,
            }),
        );
    });
    assert_eq!(violations, 0, "the audio thread allocated");
}

// ---------------------------------------------------------------- Horizon

/// The table's four, its lookahead's latency, and the description and the
/// running device agreeing at more than the defaults.
#[test]
fn horizon_reports_the_lookahead_it_runs_at() {
    assert_eq!(limiter::NAME, "limiter");
    let latency_at = |ms: f32| {
        lumen_devices::effect_latency(
            limiter::NAME,
            &[(limiter::LOOK, at(limiter::TABLE, limiter::LOOK, ms))],
        )
        .expect("a device")
    };
    assert_eq!(latency_at(1.5), 72);
    assert_eq!(latency_at(0.0), 0);
    assert_eq!(latency_at(10.0), 480);
    // The tail is the line draining, and it is the same number.
    for ms in [0.0f32, 1.5, 4.0, 10.0] {
        let values = [(limiter::LOOK, at(limiter::TABLE, limiter::LOOK, ms))];
        assert_eq!(
            lumen_devices::effect_tail(limiter::NAME, &values),
            Some(latency_at(ms))
        );
        // A fresh device at those values reports the same (review N2).
        let mut device = Limiter::new();
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        run(
            &mut device,
            &mut left,
            &mut right,
            limiter::TABLE,
            64,
            &values,
            &[],
            None,
        );
        assert_eq!(device.latency(), latency_at(ms), "{ms} ms");
        assert_eq!(device.tail(), latency_at(ms), "{ms} ms");
    }
}

/// The ceiling holds: on sines, noise, steps and a burst, at several ceilings
/// and releases, no sample leaves above it.
#[test]
fn the_limiter_never_lets_a_sample_past_its_ceiling() {
    let frames = 24_000;
    let signals: Vec<(&str, Vec<f32>)> = vec![
        (
            "a full-scale sine",
            (0..frames)
                .map(|k| (std::f32::consts::TAU * 997.0 * k as f32 / RATE as f32).sin())
                .collect(),
        ),
        (
            "a sine at +6 dB",
            (0..frames)
                .map(|k| 2.0 * (std::f32::consts::TAU * 3_100.0 * k as f32 / RATE as f32).sin())
                .collect(),
        ),
        ("a step", {
            let mut v = vec![0.1; frames];
            for (k, x) in v.iter_mut().enumerate() {
                *x = if (4_000..8_000).contains(&k) {
                    1.5
                } else {
                    0.1
                };
            }
            v
        }),
        ("noise", {
            let mut seed = 0x2545_f491u32;
            (0..frames)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    (seed as f32 / u32::MAX as f32 - 0.5) * 4.0
                })
                .collect()
        }),
    ];
    for ceiling_db in [-0.3f32, -6.0, -18.0] {
        for release in [10.0f32, 60.0, 400.0] {
            let ceiling = 10f32.powf(ceiling_db / 20.0);
            for (name, samples) in &signals {
                let mut left = samples.clone();
                let mut right = samples.clone();
                let mut device = Limiter::new();
                run(
                    &mut device,
                    &mut left,
                    &mut right,
                    limiter::TABLE,
                    256,
                    &[
                        (
                            limiter::CEILING,
                            at(limiter::TABLE, limiter::CEILING, ceiling_db),
                        ),
                        (
                            limiter::RELEASE,
                            at(limiter::TABLE, limiter::RELEASE, release),
                        ),
                    ],
                    &[],
                    None,
                );
                // Past the 20 ms glide from the table's default to the value
                // the project holds (every built-in's values glide), so what is
                // measured is the setting that was asked for.
                let settled = &left[4_800..];
                let loudest = peak(settled).max(peak(&right[4_800..]));
                assert!(
                    loudest <= ceiling + 1e-6,
                    "{name} at {ceiling_db} dB, {release} ms: {loudest} against {ceiling}"
                );
            }
        }
    }
}

/// True-peak mode measures the peaks between the samples: what leaves is under
/// the ceiling by the meter's own true peak (within the fifth of a decibel the
/// acceptance allows).
#[test]
fn true_peak_mode_keeps_the_points_between_the_samples_under_the_ceiling() {
    let frames = 24_000;
    // A high tone whose peaks fall between the samples.
    for frequency in [11_000.0f32, 15_000.0, 19_000.0] {
        let tone: Vec<f32> = (0..frames)
            .map(|k| 1.5 * (std::f32::consts::TAU * frequency * k as f32 / RATE as f32).sin())
            .collect();
        for detect in [0.0f32, 1.0] {
            let ceiling_db = -1.0f32;
            let mut left = tone.clone();
            let mut right = tone.clone();
            let mut device = Limiter::new();
            run(
                &mut device,
                &mut left,
                &mut right,
                limiter::TABLE,
                256,
                &[
                    (
                        limiter::CEILING,
                        at(limiter::TABLE, limiter::CEILING, ceiling_db),
                    ),
                    (limiter::DETECT, at(limiter::TABLE, limiter::DETECT, detect)),
                ],
                &[],
                None,
            );
            let mut meter = TruePeak::new();
            // Past the glide from the default ceiling to the one asked for.
            let between = meter
                .process(&left[4_800..])
                .max(meter.process(&right[4_800..]));
            let over = db_of(between) - ceiling_db;
            let allowed = if detect >= 0.5 { 0.2 } else { 1.0 };
            assert!(
                over <= allowed,
                "{frequency} Hz, detect {detect}: {over} dB over the ceiling"
            );
            // And the samples themselves are always under it.
            assert!(peak(&left[4_800..]) <= 10f32.powf(ceiling_db / 20.0) + 1e-6);
        }
    }
}

/// The meter the card's face reads follows what it took away.
#[test]
fn the_limiter_s_meter_says_how_much_it_took_away() {
    let frames = 24_000;
    let loud = (0..frames)
        .map(|k| 2.0 * (std::f32::consts::TAU * 500.0 * k as f32 / RATE as f32).sin())
        .collect::<Vec<f32>>();
    let mut left = loud.clone();
    let mut right = loud;
    let mut device = Limiter::new();
    let meter = device.meter();
    run(
        &mut device,
        &mut left,
        &mut right,
        limiter::TABLE,
        256,
        &[(limiter::CEILING, at(limiter::TABLE, limiter::CEILING, -6.0))],
        &[],
        None,
    );
    let reading = meter.take();
    // +6 dB in, −6 dB out: twelve decibels to give back.
    assert!(
        (reading.reduction_db - 12.0).abs() < 0.5,
        "{}",
        reading.reduction_db
    );
    assert!(reading.input_db > 5.0, "{}", reading.input_db);
    assert_eq!(meter.take().reduction_db, 0.0);
}

/// Horizon is the same at every block size, its lookahead automation included.
#[test]
fn horizon_is_the_same_at_any_block_size() {
    let frames = 4_800;
    let lane = |scale: f32| {
        (0..40)
            .map(|k| {
                (
                    (k * 120) as u32,
                    (0.1 + 0.7 * ((k as f32 * scale).sin() * 0.5 + 0.5)).clamp(0.0, 1.0),
                )
            })
            .collect::<Vec<(u32, f32)>>()
    };
    let ceiling = lane(0.9);
    let release = lane(1.4);
    let input = |k: usize| {
        let x = k as f32 * 0.021;
        ((x * 23.0).sin() * 0.9, (x * 13.0).cos() * 0.9)
    };
    let render = |block: usize| {
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (0..frames).map(input).unzip();
        let mut device = Limiter::new();
        let lanes = [
            (limiter::CEILING, ceiling.clone()),
            (limiter::RELEASE, release.clone()),
        ];
        run(
            &mut device,
            &mut left,
            &mut right,
            limiter::TABLE,
            block,
            &[],
            &lanes,
            None,
        );
        left.into_iter().zip(right).collect::<Vec<(f32, f32)>>()
    };
    let whole = render(256);
    for block in [64usize, 97, 128, 512, 1024] {
        let parts = render(block);
        for (k, ((a, b), (c, d))) in whole.iter().zip(&parts).enumerate() {
            assert!(
                (a - c).abs() < 2e-6 && (b - d).abs() < 2e-6,
                "block {block} frame {k}: ({a}, {b}) against ({c}, {d})"
            );
        }
    }
}

/// A lookahead change takes effect on the block that carries it, and the
/// frames it delays by are the frames the routing would be told about.
#[test]
fn a_new_lookahead_is_what_the_next_block_delays_by() {
    let frames = 2_048;
    let (mut left, mut right) = level(frames, 0.0);
    // An impulse at frame 0, so the output's first sample says the delay.
    left[0] = 0.5;
    right[0] = 0.5;
    let mut device = Limiter::new();
    run(
        &mut device,
        &mut left,
        &mut right,
        limiter::TABLE,
        256,
        &[
            (limiter::LOOK, at(limiter::TABLE, limiter::LOOK, 2.0)),
            (limiter::RELEASE, at(limiter::TABLE, limiter::RELEASE, 10.0)),
        ],
        &[],
        None,
    );
    let first = left
        .iter()
        .position(|x| x.abs() > 1e-6)
        .expect("the impulse came out");
    let want =
        limiter::latency(&[(limiter::LOOK, at(limiter::TABLE, limiter::LOOK, 2.0))]) as usize;
    assert_eq!(first, want, "delayed by its lookahead");
    assert_eq!(device.lookahead(), want);
}

/// The audio thread's rules: a block with automation allocates nothing.
#[test]
fn horizon_never_touches_the_heap() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is installed"
    );
    let mut device = Limiter::new();
    let mut left = vec![0.9; 256];
    let mut right = vec![-0.9; 256];
    let events = defaults(limiter::TABLE, 256, &[(limiter::DETECT, 1.0)])
        .lane(
            limiter::CEILING,
            &[(0, 0.9), (40, 0.3), (41, 0.8), (200, 0.5)],
        )
        .lane(limiter::RELEASE, &[(0, 0.1), (128, 0.9)])
        .build();
    let block = Block {
        rate: RATE,
        start: 0,
        law: PanLaw::Balance,
        events: &events,
    };
    let (_, violations) = lumen_rt::count_rt_violations(|| {
        device.process(
            &block,
            Stereo {
                left: &mut left,
                right: &mut right,
            },
            None,
        );
    });
    assert_eq!(violations, 0, "the audio thread allocated");
}

// ---------------------------------------------------------------- golden

/// What a golden render is pinned by: twenty samples at fixed frames, the sum
/// of the magnitudes and the sum of the squares. Compared within a millionth
/// (relative), never by their bits: the platforms' maths libraries round `exp`
/// and `powf` a last bit apart.
fn pinned(left: &[f32]) -> ([f32; 20], f64, f64) {
    let mut samples = [0.0f32; 20];
    for (k, sample) in samples.iter_mut().enumerate() {
        *sample = left[k * (left.len() / 20)];
    }
    let level: f64 = left.iter().map(|x| f64::from(x.abs())).sum();
    let energy: f64 = left.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    (samples, level, energy)
}

#[track_caller]
fn close(got: f64, want: f64) {
    assert!(
        (got - want).abs() <= 1e-6 * want.abs().max(1e-6),
        "{got:e} against the pinned {want:e}"
    );
}

/// The golden render of a compressor whose threshold and ratio are automated:
/// a frozen sound, pinned by its samples. One that differs from its pin was
/// changed on purpose, and the pin moves in the same commit.
#[test]
fn a_golden_render_of_automated_gravity_is_pinned_to_its_samples() {
    let frames = 4_000;
    let threshold = vec![
        (0u32, at(compressor::TABLE, compressor::THRESHOLD, -6.0)),
        (1_000, at(compressor::TABLE, compressor::THRESHOLD, -30.0)),
        (2_500, at(compressor::TABLE, compressor::THRESHOLD, -40.0)),
    ];
    let ratio = vec![
        (0u32, at(compressor::TABLE, compressor::RATIO, 2.0)),
        (1_500, at(compressor::TABLE, compressor::RATIO, 12.0)),
    ];
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    for k in 0..frames {
        let x = k as f32 * 0.031;
        left[k] = (x * 29.0).sin() * 0.8;
        right[k] = (x * 17.0).cos() * 0.7;
    }
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut left,
        &mut right,
        compressor::TABLE,
        128,
        &[
            (
                compressor::ATTACK,
                at(compressor::TABLE, compressor::ATTACK, 2.0),
            ),
            (
                compressor::RELEASE,
                at(compressor::TABLE, compressor::RELEASE, 80.0),
            ),
            (
                compressor::KNEE,
                at(compressor::TABLE, compressor::KNEE, 6.0),
            ),
        ],
        &[
            (compressor::THRESHOLD, threshold),
            (compressor::RATIO, ratio),
        ],
        None,
    );
    let (samples, level, energy) = pinned(&left);
    // A golden is pinned within a millionth, never by its bits: the platforms'
    // maths libraries round `exp` and `powf` a last bit apart.
    const PINS: [f32; 20] = [
        0.0,
        -1.028_189_8,
        1.768_813_8,
        -1.476_835_1,
        0.385_838_54,
        0.585_212_4,
        -0.772_780_4,
        0.603_449_5,
        -0.236_174_3,
        -0.128_298_01,
        0.341_473_22,
        -0.344_071_9,
        0.189_203_74,
        0.014_736_537,
        -0.180_108_34,
        0.247_255_62,
        -0.191_338_72,
        0.040_941_395,
        0.128_075_15,
        -0.231_129_05,
    ];
    for (k, want) in PINS.iter().enumerate() {
        close(f64::from(samples[k]), f64::from(*want));
    }
    close(level, 1.799_342_877_928_589_6e3);
    close(energy, 1.665_881_039_358_343e3);
}

/// The golden render of a limiter in true-peak mode, pinned the same way.
#[test]
fn a_golden_render_of_horizon_is_pinned_to_its_samples() {
    let frames = 4_000;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    for k in 0..frames {
        let x = k as f32 * 0.043;
        left[k] = (x * 37.0).sin() * 1.4;
        right[k] = (x * 21.0).cos() * 1.2;
    }
    let mut device = Limiter::new();
    run(
        &mut device,
        &mut left,
        &mut right,
        limiter::TABLE,
        128,
        &[
            (limiter::CEILING, at(limiter::TABLE, limiter::CEILING, -3.0)),
            (limiter::RELEASE, at(limiter::TABLE, limiter::RELEASE, 40.0)),
            (limiter::DETECT, 1.0),
        ],
        &[],
        None,
    );
    let (samples, level, energy) = pinned(&left);
    const PINS: [f32; 20] = [
        0.0,
        0.484_289_85,
        0.286_868_63,
        -0.766_592_6,
        0.631_256_46,
        -0.070_986_61,
        -0.507_166_56,
        0.702_226_04,
        -0.366_763_77,
        -0.245_715_25,
        0.672_580_7,
        -0.591_314_44,
        0.063_339_03,
        0.512_479_1,
        -0.701_172_35,
        0.360_093_56,
        0.252_798_35,
        -0.674_914_4,
        0.586_971_3,
        -0.055_779_774,
    ];
    for (k, want) in PINS.iter().enumerate() {
        close(f64::from(samples[k]), f64::from(*want));
    }
    close(level, 1.837_719_892_391_571_2e3);
    close(energy, 1.067_801_608_622_248_8e3);
}

/// The tables, pinned (M8 §6 E2): each parameter's id, the name a person reads
/// and the normalised default. An id may not be reused or renumbered and a name
/// may not change without moving this list on purpose: a project saves values
/// by id, so a changed table is a changed sound.
#[test]
fn the_parameter_tables_are_pinned_by_id_name_and_default() {
    let pinned: Vec<(u32, &str, f32)> = vec![
        (1, "Pull", 0.7),
        (2, "Ratio", 3.0 / 19.0),
        (3, "Attack", 0.157_341),
        (4, "Release", 0.240_092),
        (5, "Knee", 0.25),
        (6, "Makeup", 1.0),
        (7, "Mix", 1.0),
        (8, "Detect", 0.0),
        (9, "Key HPF", 0.0),
        (10, "Listen", 0.0),
    ];
    let table: Vec<(u32, &str, f32)> = compressor::TABLE
        .iter()
        .map(|p| (p.id, p.name, p.normalised(p.default)))
        .collect();
    assert_eq!(table.len(), pinned.len());
    for (got, want) in table.iter().zip(&pinned) {
        assert_eq!((got.0, got.1), (want.0, want.1));
        assert!((got.2 - want.2).abs() < 1e-5, "{}: {}", got.1, got.2);
    }
    let pinned: Vec<(u32, &str, f32)> = vec![
        (1, "Ceiling", 0.987_5),
        (2, "Release", 0.224_733),
        (3, "Look", 0.15),
        (4, "Detect", 0.0),
    ];
    let table: Vec<(u32, &str, f32)> = limiter::TABLE
        .iter()
        .map(|p| (p.id, p.name, p.normalised(p.default)))
        .collect();
    assert_eq!(table.len(), pinned.len());
    for (got, want) in table.iter().zip(&pinned) {
        assert_eq!((got.0, got.1), (want.0, want.1));
        assert!((got.2 - want.2).abs() < 1e-5, "{}: {}", got.1, got.2);
    }
    // No id is reused, none is zero, and no two names collide.
    for table in [compressor::TABLE, limiter::TABLE] {
        let ids: std::collections::BTreeSet<u32> = table.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), table.len(), "an id is used twice");
        assert!(!ids.contains(&0), "0 is not a parameter");
        for p in table {
            assert!(!p.name.is_empty());
        }
    }
}
