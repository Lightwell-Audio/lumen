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
            bpm: 120.0,
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

/// Every automatable parameter moving at once (a lane for each, from the table
/// itself, so a parameter added later is covered) plays the same audio at every
/// block size.
#[test]
fn gravity_is_the_same_at_any_block_size() {
    let frames = 4_800;
    // A lane for every automatable parameter of the table, each moving.
    let lanes = |scale: f32| -> Vec<(u32, Vec<(u32, f32)>)> {
        compressor::TABLE
            .iter()
            .filter(|p| lumen_devices::automatable(compressor::NAME, p.id))
            .enumerate()
            .map(|(i, p)| {
                let scale = scale + i as f32 * 0.37;
                let points = (0..40)
                    .map(|k| {
                        (
                            (k * 120) as u32,
                            (0.3 + 0.5 * ((k as f32 * scale).sin() * 0.5 + 0.5)).clamp(0.0, 1.0),
                        )
                    })
                    .collect();
                (p.id, points)
            })
            .collect()
    };
    let input = |k: usize| {
        let x = k as f32 * 0.017;
        ((x * 31.0).sin() * 0.4, (x * 17.0).cos() * 0.3)
    };
    // A key too, so the key filter's own corner is under the same test (review
    // N2: without one the high-pass never ran).
    let key: Vec<f32> = (0..frames)
        .map(|k| (k as f32 * 0.011).sin() * 0.6)
        .collect();
    let render = |block: usize| {
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (0..frames).map(input).unzip();
        let mut device = Compressor::new();
        let moving = lanes(0.7);
        run(
            &mut device,
            &mut left,
            &mut right,
            compressor::TABLE,
            block,
            &[],
            &moving,
            Some((&key, &key)),
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
        bpm: 120.0,
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

/// The ceiling holds: on sines, noise, steps and a burst, at several ceilings,
/// releases and lookaheads — including none — no sample leaves above it.
///
/// Every signal is silent for the first [`SETTLE`] frames, which is longer than
/// the 20 ms glide from the table's default ceiling to the value asked for, so
/// what is measured is the setting, onset and all.
#[test]
fn the_limiter_never_lets_a_sample_past_its_ceiling() {
    const SETTLE: usize = 4_800;
    let frames = 24_000;
    let signals: Vec<(&str, Vec<f32>)> = vec![
        (
            "a full-scale sine",
            (0..frames)
                .map(|k| {
                    if k < SETTLE {
                        0.0
                    } else {
                        (std::f32::consts::TAU * 997.0 * k as f32 / RATE as f32).sin()
                    }
                })
                .collect(),
        ),
        (
            "a sine at +6 dB",
            (0..frames)
                .map(|k| {
                    if k < SETTLE {
                        0.0
                    } else {
                        2.0 * (std::f32::consts::TAU * 3_100.0 * k as f32 / RATE as f32).sin()
                    }
                })
                .collect(),
        ),
        ("a step", {
            let mut v = vec![0.0; frames];
            for (k, x) in v.iter_mut().enumerate() {
                *x = if k < SETTLE {
                    0.0
                } else if k < SETTLE + 4_000 {
                    0.1
                } else {
                    1.5
                };
            }
            v
        }),
        ("noise", {
            let mut seed = 0x2545_f491u32;
            let mut v: Vec<f32> = (0..frames)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    (seed as f32 / u32::MAX as f32 - 0.5) * 4.0
                })
                .collect();
            v[..SETTLE].fill(0.0);
            v
        }),
    ];
    for ceiling_db in [-0.3f32, -6.0, -18.0] {
        for release in [10.0f32, 60.0, 400.0] {
            for look_ms in [0.0f32, 1.5, 10.0] {
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
                            (limiter::LOOK, at(limiter::TABLE, limiter::LOOK, look_ms)),
                        ],
                        &[],
                        None,
                    );
                    // From the onset on: the whole span after the glide.
                    let settled = &left[SETTLE..];
                    let loudest = peak(settled).max(peak(&right[SETTLE..]));
                    assert!(
                        loudest <= ceiling + 1e-6,
                        "{name} at {ceiling_db} dB, {release} ms, look {look_ms}: {loudest} \
                         against {ceiling}"
                    );
                }
            }
        }
    }
}

/// The peaks between the samples, found by a long reference interpolation: 64
/// points between each pair of samples, each a Kaiser-windowed sinc of 25 taps
/// (the design `lumen_core::loudness` measures true peaks with, widened for the
/// reference), so the grid between samples is 64 times finer than the limiter's
/// own detector and a tolerance is not spent on that. The ends of the slice are
/// left out: outside them the window reads zeros, and interpolating a signal cut
/// off like that overshoots.
fn reference_true_peak(samples: &[f32]) -> f32 {
    const PHASES: usize = 64;
    const TAPS: usize = 25;
    let length = PHASES * (TAPS - 1) + 1;
    let centre = (length - 1) as f64 / 2.0;
    let beta = 12.0;
    let i0 = |x: f64| {
        let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
        while term > 1e-12 * sum {
            term *= (x / (2.0 * k)).powi(2);
            sum += term;
            k += 1.0;
        }
        sum
    };
    // Each phase's taps, newest sample last, normalised to a gain of one.
    let phase_taps = |p: usize| -> [f64; TAPS] {
        let mut taps = [0.0f64; TAPS];
        for (j, tap) in taps.iter_mut().enumerate() {
            let n = j * PHASES + p;
            if n >= length {
                continue;
            }
            let x = (n as f64 - centre) / PHASES as f64;
            let sinc = if x == 0.0 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            let r = (n as f64 - centre) / centre;
            *tap = sinc * i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0(beta);
        }
        let sum: f64 = taps.iter().sum();
        for tap in &mut taps {
            *tap /= sum;
        }
        taps
    };
    let phases: Vec<[f64; TAPS]> = (0..PHASES).map(phase_taps).collect();
    let mut most = 0.0f32;
    // The ends are left out: outside them the window reads zeros, and the
    // interpolation of a signal cut off like that overshoots.
    for k in TAPS..samples.len().saturating_sub(TAPS) {
        for phase in &phases {
            let mut value = 0.0f64;
            for (j, weight) in phase.iter().enumerate() {
                // Tap `j` is the oldest of the window, so it reads before `k`.
                let index = k as isize + j as isize - (TAPS as isize - 1);
                let x = if index < 0 || index as usize >= samples.len() {
                    0.0
                } else {
                    f64::from(samples[index as usize])
                };
                value += x * weight;
            }
            most = most.max(value.abs() as f32);
        }
    }
    most
}

/// True-peak mode measures the peaks between the samples: what leaves is under
/// the ceiling by a long reference interpolation (a twentieth of a decibel),
/// with the four-phase meter kept as a second, looser check.
#[test]
fn true_peak_mode_keeps_the_points_between_the_samples_under_the_ceiling() {
    let frames = 6_000;
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
            // Past the glide from the default ceiling to the one asked for.
            let settled = &left[2_400..];
            let right_settled = &right[2_400..];
            let ceiling = 10f32.powf(ceiling_db / 20.0);
            // The reference: the waveform itself, between the samples.
            let reference = reference_true_peak(settled).max(reference_true_peak(right_settled));
            let over = db_of(reference) - ceiling_db;
            let allowed = if detect >= 0.5 { 0.05 } else { 1.0 };
            assert!(
                over <= allowed,
                "{frequency} Hz, detect {detect}: {over} dB over the ceiling by the reference \
                 ({reference} against {ceiling})"
            );
            // And the samples themselves are always under it.
            assert!(peak(settled) <= ceiling + 1e-6);
            // The four-phase meter — the one the master's true-peak reading
            // uses — is the second, coarser check: it reads near the reference
            // (within half a decibel; it is a little short at the top of the
            // band and a little over on a hard-limited waveform).
            let mut meter = TruePeak::new();
            let read = meter.process(settled).max(meter.process(right_settled));
            let apart = 20.0 * (read / reference.max(1e-9)).log10();
            assert!(
                apart.abs() <= 0.5,
                "{frequency} Hz: the meter reads {read}, {apart} dB from the reference {reference}"
            );
        }
    }
}

/// Lowering the ceiling holds the burst that is already in the window: the
/// window keeps the *level* and the ceiling is divided in at the front, so a
/// peak measured when the ceiling was high does not leave above the new one
/// (review L2, N3).
///
/// The ceiling is moved by a lane — a step, which glides over a millisecond —
/// at a frame where the burst is still in the ten-millisecond line, and what is
/// measured is the burst's own output, not a quiet stretch after it.
#[test]
fn lowering_the_ceiling_holds_the_peak_already_in_the_window() {
    let frames = 6_000;
    let look_ms = 10.0f32;
    const BURST: usize = 1_000;
    let latency =
        limiter::latency(&[(limiter::LOOK, at(limiter::TABLE, limiter::LOOK, look_ms))]) as usize;
    // A loud burst of 100 frames, then a quiet stretch. It leaves the line at
    // BURST + latency, well after the ceiling has come down.
    let mut quiet = vec![0.02f32; frames];
    quiet[BURST..BURST + 100].fill(0.9);
    let new_ceiling_db = -20.0f32;
    let new_ceiling = 10f32.powf(new_ceiling_db / 20.0);
    // The lane drops the ceiling to −20 dB at frame 1,200 (a step: over in a
    // millisecond), while the burst is still inside the line.
    let lane: Vec<(u32, f32)> = vec![
        (0, at(limiter::TABLE, limiter::CEILING, -0.3)),
        (1_200, at(limiter::TABLE, limiter::CEILING, -0.3)),
        (1_201, at(limiter::TABLE, limiter::CEILING, new_ceiling_db)),
    ];
    let render = |ceiling_lane: Option<&[(u32, f32)]>| {
        let (mut left, mut right) = (quiet.clone(), quiet.clone());
        let mut device = Limiter::new();
        let lanes: Vec<(u32, Vec<(u32, f32)>)> = ceiling_lane
            .map(|points| vec![(limiter::CEILING, points.to_vec())])
            .unwrap_or_default();
        run(
            &mut device,
            &mut left,
            &mut right,
            limiter::TABLE,
            256,
            &[
                (limiter::CEILING, at(limiter::TABLE, limiter::CEILING, -0.3)),
                (limiter::LOOK, at(limiter::TABLE, limiter::LOOK, look_ms)),
                (
                    limiter::RELEASE,
                    at(limiter::TABLE, limiter::RELEASE, 400.0),
                ),
            ],
            &lanes,
            None,
        );
        (left, right)
    };
    // The burst's own output frames, from where it leaves the line.
    let leave = BURST + latency;
    let span = leave..leave + 100;
    let dropped = render(Some(&lane));
    let loudest = peak(&dropped.0[span.clone()]).max(peak(&dropped.1[span.clone()]));
    assert!(
        loudest <= new_ceiling + 1e-6,
        "the burst left at {loudest}, over the new ceiling of {new_ceiling}"
    );
    // And with the ceiling left alone the same burst is much louder: the
    // silence above is the drop's doing, not the limiter's.
    let held = render(None);
    let before = peak(&held.0[span.clone()]).max(peak(&held.1[span]));
    assert!(
        before > 4.0 * new_ceiling,
        "the burst is loud when the ceiling stays up: {before}"
    );
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
        bpm: 120.0,
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

// ------------------------------------------------------- behaviour (review L5)

/// Mix at 0, 50 and 100 %: none of the compressed signal, half of it, all of it.
#[test]
fn the_mix_is_plain_compressed_half_and_all() {
    let frames = 48_000;
    let amplitude = 1.0f32;
    let at_mix = |mix: f32| {
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
                    at(compressor::TABLE, compressor::RATIO, 4.0),
                ),
                (compressor::KNEE, 0.0),
                (compressor::MAKEUP, 0.0),
                (compressor::MIX, at(compressor::TABLE, compressor::MIX, mix)),
            ],
            &[],
            None,
        );
        left[frames - 1]
    };
    let dry = at_mix(0.0);
    let wet = at_mix(100.0);
    let half = at_mix(50.0);
    assert!((dry - amplitude).abs() < 1e-3, "dry: {dry}");
    // −18 dB at 4:1 takes 13.5 dB away at full scale.
    let want = amplitude * 10f32.powf(-13.5 / 20.0);
    assert!((wet - want).abs() < 2e-3, "wet: {wet} against {want}");
    let mid = 0.5 * (dry + wet);
    assert!((half - mid).abs() < 5e-3, "half: {half} against {mid}");
}

/// A step reaches 63 % of its way to the gain the computer wants in one attack
/// time, and back again in one release time: the times in the table mean what
/// they say.
#[test]
fn the_attack_and_release_take_their_time_to_get_there() {
    // The threshold is −30 dB and the ratio 20:1, so 0.5 (−6 dBFS) wants
    // (23.98 dB over the threshold) × (1 − 1/20) = 22.78 dB taken away.
    let set = |attack_ms: f32, release_ms: f32| {
        [
            (
                compressor::THRESHOLD,
                at(compressor::TABLE, compressor::THRESHOLD, -30.0),
            ),
            (
                compressor::RATIO,
                at(compressor::TABLE, compressor::RATIO, 20.0),
            ),
            (compressor::KNEE, 0.0),
            (compressor::MAKEUP, 0.0),
            (
                compressor::ATTACK,
                at(compressor::TABLE, compressor::ATTACK, attack_ms),
            ),
            (
                compressor::RELEASE,
                at(compressor::TABLE, compressor::RELEASE, release_ms),
            ),
        ]
    };
    let reduction = |input: f32, samples: &mut (Vec<f32>, Vec<f32>)| {
        let at = samples.0.len() - 1;
        let gain = samples.0[at] / input;
        -20.0 * gain.max(1e-9).log10()
    };
    // The attack: a step onto the input, measured one attack time later.
    let attack_ms = 20.0f32;
    let frames = 960 + 8; // one 20 ms time constant, and a block's worth
    let mut samples = level(frames, 0.5);
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut samples.0,
        &mut samples.1,
        compressor::TABLE,
        8,
        &set(attack_ms, 2_000.0),
        &[],
        None,
    );
    let early = reduction(0.5, &mut samples);
    // The settled reduction, far past the attack.
    let mut settled = level(48_000, 0.5);
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut settled.0,
        &mut settled.1,
        compressor::TABLE,
        256,
        &set(attack_ms, 2_000.0),
        &[],
        None,
    );
    let full = reduction(0.5, &mut settled);
    let want_full = (20.0 * 0.5f32.log10() + 30.0) * (1.0 - 1.0 / 20.0);
    assert!(
        (full - want_full).abs() < 0.2,
        "the gain computer's answer: {full} against {want_full}"
    );
    let want = 0.632 * full;
    assert!(
        (early - want).abs() < 0.15 * full,
        "attack {attack_ms} ms: {early} of {full} (63 % is {want})"
    );

    // The release: settle, then drop the input well under the threshold and
    // measure one release time later.
    let release_ms = 120.0f32;
    let mut settled = level(48_000, 0.5);
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut settled.0,
        &mut settled.1,
        compressor::TABLE,
        256,
        &set(0.05, release_ms),
        &[],
        None,
    );
    let before = reduction(0.5, &mut settled);
    let mut quiet = level(5_760 + 8, 0.02);
    run(
        &mut device,
        &mut quiet.0,
        &mut quiet.1,
        compressor::TABLE,
        8,
        &set(0.05, release_ms),
        &[],
        None,
    );
    let after = reduction(0.02, &mut quiet);
    let want = 0.368 * before;
    assert!(
        (after - want).abs() < 0.15 * before,
        "release {release_ms} ms: {after} of {before} (37 % is {want})"
    );
}

/// RMS detection reads the mean level, which is 3 dB under a sine's peak, so a
/// sine of the same amplitude is pulled down 3 dB less than peak detection
/// pulls it.
#[test]
fn rms_detection_reads_the_mean_and_not_the_peak() {
    let frames = 96_000;
    let amplitude = 0.5f32; // −6 dBFS peak, −9 dBFS RMS
    let settle = |detect: f32| {
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (0..frames)
            .map(|k| {
                let x =
                    amplitude * (std::f32::consts::TAU * 1_000.0 * k as f32 / RATE as f32).sin();
                (x, x)
            })
            .unzip();
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
                    at(compressor::TABLE, compressor::THRESHOLD, -20.0),
                ),
                (
                    compressor::RATIO,
                    at(compressor::TABLE, compressor::RATIO, 4.0),
                ),
                (compressor::KNEE, 0.0),
                (compressor::MAKEUP, 0.0),
                // A long release, so the gain settles instead of following the
                // wave.
                (
                    compressor::RELEASE,
                    at(compressor::TABLE, compressor::RELEASE, 2_000.0),
                ),
                (compressor::DETECT, detect),
            ],
            &[],
            None,
        );
        // The peak of the last 1,000 frames.
        peak(&left[frames - 1_000..])
    };
    let peak_detection = settle(0.0);
    let rms_detection = settle(1.0);
    // Peak detection reads the wave's peak, −6 dBFS; RMS reads its mean,
    // −9 dBFS, three decibels lower, so it takes that much less away.
    let want = |level_db: f32| {
        let gain_db = compressor::curve_db(level_db, -20.0, 4.0, 0.0) - level_db;
        amplitude * 10f32.powf(gain_db / 20.0)
    };
    let want_peak = want(20.0 * amplitude.log10());
    let want_rms = want(20.0 * (amplitude / std::f32::consts::SQRT_2).log10());
    assert!(
        (peak_detection - want_peak).abs() < 0.02,
        "peak detection: {peak_detection} against {want_peak}"
    );
    assert!(
        (rms_detection - want_rms).abs() < 0.02,
        "RMS detection: {rms_detection} against {want_rms}"
    );
    assert!(rms_detection > peak_detection);
}

/// Offline is real time: the same buffer, processed in one pass and in blocks of
/// various sizes, is bit for bit the same — which is what makes a bounce and an
/// export play what was heard.
#[test]
fn offline_rendering_is_bit_for_bit_what_playback_plays() {
    let frames = 4_800;
    let (mut source_l, mut source_r) = (vec![0.0f32; frames], vec![0.0f32; frames]);
    for k in 0..frames {
        let x = k as f32 * 0.019;
        source_l[k] = (x * 23.0).sin() * 0.7;
        source_r[k] = (x * 11.0).cos() * 0.6;
    }
    for device_kind in ["gravity", "horizon"] {
        let render = |block: usize| {
            let (mut left, mut right) = (source_l.clone(), source_r.clone());
            let (table, set): (&'static [lumen_devices::ParamInfo], Vec<(u32, f32)>) =
                if device_kind == "gravity" {
                    (
                        compressor::TABLE,
                        vec![
                            (
                                compressor::THRESHOLD,
                                at(compressor::TABLE, compressor::THRESHOLD, -24.0),
                            ),
                            (
                                compressor::RATIO,
                                at(compressor::TABLE, compressor::RATIO, 6.0),
                            ),
                        ],
                    )
                } else {
                    (
                        limiter::TABLE,
                        vec![
                            (limiter::CEILING, at(limiter::TABLE, limiter::CEILING, -6.0)),
                            (limiter::LOOK, at(limiter::TABLE, limiter::LOOK, 1.5)),
                        ],
                    )
                };
            let mut device: Box<dyn Effect> = if device_kind == "gravity" {
                Box::new(Compressor::new())
            } else {
                Box::new(Limiter::new())
            };
            run(
                device.as_mut(),
                &mut left,
                &mut right,
                table,
                block,
                &set,
                &[],
                None,
            );
            left.into_iter().zip(right).collect::<Vec<(f32, f32)>>()
        };
        let one_pass = render(256);
        for block in [64usize, 97, 128, 512, 1024] {
            let in_blocks = render(block);
            for (frame, (a, b)) in one_pass.iter().zip(&in_blocks).enumerate() {
                assert!(
                    a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(),
                    "{device_kind}, block {block}, frame {frame}: {a:?} against {b:?}"
                );
            }
        }
    }
}

/// Horizon's latency is the same number of frames at any rate (the description
/// has no rate to ask) and the device delays by exactly those frames at 44.1
/// and 96 kHz too.
#[test]
fn the_limiter_delays_by_its_frames_at_any_rate() {
    for rate in [44_100u32, 48_000, 96_000] {
        let frames = 1_024;
        let look = at(limiter::TABLE, limiter::LOOK, 1.5);
        let latency = limiter::latency(&[(limiter::LOOK, look)]) as usize;
        assert_eq!(latency, 72, "the description's frames");
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        left[0] = 0.5;
        right[0] = 0.5;
        let mut device = Limiter::new();
        let events = defaults(limiter::TABLE, 256, &[(limiter::LOOK, look)]).build();
        let mut done = 0;
        while done < frames {
            let n = 256.min(frames - done);
            let block = Block {
                rate,
                start: done as i64,
                bpm: 120.0,
                law: PanLaw::Balance,
                events: &events,
            };
            device.process(
                &block,
                Stereo {
                    left: &mut left[done..done + n],
                    right: &mut right[done..done + n],
                },
                None,
            );
            done += n;
        }
        assert_eq!(device.latency(), latency as u32, "the device at {rate} Hz");
        let first = left
            .iter()
            .position(|x| x.abs() > 1e-6)
            .expect("the impulse came out");
        assert_eq!(first, latency, "at {rate} Hz");
    }
}

/// A lookahead of none delays by nothing: the frame written now is the frame
/// played now (review H1: reading the line before writing it played a frame
/// from a whole line ago, 482 frames late, with the ceiling broken).
#[test]
fn no_lookahead_delays_by_nothing_and_still_holds_the_ceiling() {
    let frames = 1_024;
    let (mut left, mut right) = level(frames, 0.0);
    left[0] = 0.5;
    right[0] = 0.5;
    let mut device = Limiter::new();
    run(
        &mut device,
        &mut left,
        &mut right,
        limiter::TABLE,
        64,
        &[
            (limiter::LOOK, at(limiter::TABLE, limiter::LOOK, 0.0)),
            (limiter::CEILING, at(limiter::TABLE, limiter::CEILING, -6.0)),
            (limiter::RELEASE, at(limiter::TABLE, limiter::RELEASE, 10.0)),
        ],
        &[],
        None,
    );
    assert_eq!(device.latency(), 0);
    let first = left
        .iter()
        .position(|x| x.abs() > 1e-6)
        .expect("the impulse came out");
    assert_eq!(first, 0, "the impulse is at the frame it was played on");
    // −6 dB of a 0.5 impulse: just under the ceiling.
    let ceiling = 10f32.powf(-6.0 / 20.0);
    assert!(left[0] <= ceiling + 1e-6, "{}", left[0]);
    assert!(left[0] > 0.2, "it is not silent either: {}", left[0]);
}

/// A lane on the lookahead changes nothing: the device reads the value the
/// project holds (review M1), so its delay is the routing's number and the
/// audio is the same at any block size.
#[test]
fn a_lane_on_the_lookahead_is_ignored() {
    let frames = 4_800;
    // A lane that sweeps the lookahead from nothing to the top.
    let lane: Vec<(u32, f32)> = (0..40)
        .map(|k| ((k * 120) as u32, (k as f32 / 40.0).clamp(0.0, 1.0)))
        .collect();
    let held = at(limiter::TABLE, limiter::LOOK, 1.5);
    let input = |k: usize| {
        let x = k as f32 * 0.021;
        ((x * 23.0).sin() * 0.9, (x * 13.0).cos() * 0.9)
    };
    let render = |block: usize| {
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (0..frames).map(input).unzip();
        let mut device = Limiter::new();
        run(
            &mut device,
            &mut left,
            &mut right,
            limiter::TABLE,
            block,
            &[(limiter::LOOK, held)],
            &[(limiter::LOOK, lane.clone())],
            None,
        );
        assert_eq!(
            device.lookahead(),
            limiter::latency(&[(limiter::LOOK, held)]) as usize,
            "the held value, not the lane's"
        );
        assert_eq!(device.latency(), 72);
        left.into_iter().zip(right).collect::<Vec<(f32, f32)>>()
    };
    let whole = render(256);
    for block in [64usize, 128] {
        let parts = render(block);
        for (frame, (a, b)) in whole.iter().zip(&parts).enumerate() {
            assert!(
                a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(),
                "block {block} frame {frame}: {a:?} against {b:?}"
            );
        }
    }
}

/// Raising the lookahead while a burst is in the line lets go of what the line
/// held: a short gap, never a frame replayed from outside the window the gain
/// covers (review N6).
#[test]
fn raising_the_lookahead_lets_go_of_what_the_line_held() {
    let frames = 4_000;
    const BURST: usize = 400;
    const BURST_END: usize = 900;
    const SPLIT: usize = 1_000;
    let mut loud = vec![0.02f32; frames];
    loud[BURST..BURST_END].fill(1.5);
    let ceiling_db = -0.3f32;
    let ceiling = 10f32.powf(ceiling_db / 20.0);
    let mut left = loud.clone();
    let mut right = loud.clone();
    let mut device = Limiter::new();
    // Short lookahead over the burst.
    run(
        &mut device,
        &mut left[..SPLIT],
        &mut right[..SPLIT],
        limiter::TABLE,
        256,
        &[
            (
                limiter::CEILING,
                at(limiter::TABLE, limiter::CEILING, ceiling_db),
            ),
            (limiter::LOOK, at(limiter::TABLE, limiter::LOOK, 1.5)),
            (
                limiter::RELEASE,
                at(limiter::TABLE, limiter::RELEASE, 400.0),
            ),
        ],
        &[],
        None,
    );
    // Ten milliseconds of lookahead: the read jumps back into the line, where
    // the burst still is.
    let mut rest_l = left[SPLIT..].to_vec();
    let mut rest_r = right[SPLIT..].to_vec();
    run(
        &mut device,
        &mut rest_l,
        &mut rest_r,
        limiter::TABLE,
        256,
        &[
            (
                limiter::CEILING,
                at(limiter::TABLE, limiter::CEILING, ceiling_db),
            ),
            (limiter::LOOK, at(limiter::TABLE, limiter::LOOK, 10.0)),
            (
                limiter::RELEASE,
                at(limiter::TABLE, limiter::RELEASE, 400.0),
            ),
        ],
        &[],
        None,
    );
    // Nothing of the burst comes out above the ceiling...
    let loudest = peak(&rest_l).max(peak(&rest_r));
    assert!(
        loudest <= ceiling + 1e-6,
        "the burst was replayed at {loudest}, above {ceiling}"
    );
    // ...and the first frames are a gap, not the burst.
    let first = peak(&rest_l[..64]);
    assert!(first < 1e-4, "a short gap, not a replay: {first}");
}

/// A key shorter than the block keys the frames it has and no more: the
/// compressor reads its own input for the rest, and never reads past the end of
/// what it was handed (review L4, N7).
#[test]
fn a_key_shorter_than_the_block_is_read_only_as_far_as_it_goes() {
    let frames = 256;
    let short = 64;
    let key = vec![1.0f32; short];
    let mut keyed = (vec![0.05f32; frames], vec![0.05f32; frames]);
    let mut unkeyed = keyed.clone();
    let set = [
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
    ];
    let mut meter = None;
    for (samples, with_key) in [(&mut keyed, true), (&mut unkeyed, false)] {
        let mut effect = Compressor::new();
        if with_key {
            meter = Some(effect.meter());
        }
        let events = defaults(compressor::TABLE, frames, &set).build();
        let block = Block {
            rate: RATE,
            start: 0,
            bpm: 120.0,
            law: PanLaw::Balance,
            events: &events,
        };
        let key_side: Option<Key<'_>> = with_key.then_some(Key {
            left: &key,
            right: &key,
        });
        effect.process(
            &block,
            Stereo {
                left: &mut samples.0,
                right: &mut samples.1,
            },
            key_side,
        );
    }
    // The key was read for the frames it covered...
    let reading = meter.expect("a meter").take();
    assert!(reading.input_db > -1.0, "the key was read: {reading:?}");
    // ...and reading it no further than it goes is the point: past frame 64 the
    // device keys on its own input rather than reading past the end of the
    // slice it was handed (without the clamp this block would panic), and every
    // frame it produced is a number.
    assert!(
        keyed.0.iter().chain(&keyed.1).all(|x| x.is_finite()),
        "nothing broken past the key"
    );
    assert!(
        keyed.0[..short]
            .iter()
            .zip(&unkeyed.0[..short])
            .any(|(a, b)| (a - b).abs() > 1e-3),
        "the key moved the frames it covered"
    );
}

/// `reset()` is real-time safe: the audio thread calls it on a locate, so it
/// may not touch the heap (review N7).
#[test]
fn a_reset_never_touches_the_heap() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is installed"
    );
    let mut gravity = Compressor::new();
    let mut horizon = Limiter::new();
    let mut left = vec![0.1f32; 256];
    let mut right = vec![-0.1f32; 256];
    let events = defaults(compressor::TABLE, 256, &[]).build();
    let (_, violations) = lumen_rt::count_rt_violations(|| {
        gravity.reset();
        horizon.reset();
        // And a block after each, so the state is walked as a locate leaves it.
        let block = Block {
            rate: RATE,
            start: 0,
            bpm: 120.0,
            law: PanLaw::Balance,
            events: &events,
        };
        gravity.process(
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
        0.477_397_05,
        0.285_880_83,
        -0.752_835,
        0.627_081,
        -0.070_977_48,
        -0.507_191_2,
        0.702_264_25,
        -0.366_780_88,
        -0.245_726_72,
        0.672_628,
        -0.591_350_9,
        0.063_342_944,
        0.512_515_66,
        -0.701_235_9,
        0.360_125_66,
        0.252_825_86,
        -0.675_011_1,
        0.587_052_8,
        -0.055_787_597,
    ];
    for (k, want) in PINS.iter().enumerate() {
        close(f64::from(samples[k]), f64::from(*want));
    }
    close(level, 1.833_519_686_036_379_4e3);
    close(energy, 1.062_318_475_213_476_9e3);
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
