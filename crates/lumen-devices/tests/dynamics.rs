use lumen_core::loudness::TruePeak;
use lumen_core::pan::PanLaw;
use lumen_devices::dynamics::compressor;
use lumen_devices::dynamics::limiter;
use lumen_devices::params;
use lumen_devices::{Block, Compressor, Effect, Key, Limiter, ParamBlock, Stereo};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;

fn at(table: &'static [lumen_devices::ParamInfo], id: u32, value: f32) -> f32 {
    params::find(table, id)
        .expect("a parameter of the table")
        .normalised(value)
}

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

    fn held(mut self, id: u32, value: f32) -> Self {
        self.points.push((id, vec![(0, f64::from(value))]));
        self
    }

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

fn level(frames: usize, amplitude: f32) -> (Vec<f32>, Vec<f32>) {
    (vec![amplitude; frames], vec![amplitude; frames])
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn db_of(amplitude: f32) -> f32 {
    20.0 * amplitude.max(1e-12).log10()
}

#[test]
fn at_its_defaults_every_parameter_names_its_bounds() {
    assert_eq!(compressor::NAME, "compressor");
    assert!(lumen_devices::effect_params(compressor::NAME).is_some());
    let first: Vec<&str> = compressor::TABLE.iter().take(4).map(|p| p.name).collect();
    assert_eq!(first, ["Pull", "Ratio", "Attack", "Release"]);
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
    let got = left[frames - 1];
    assert!((got - 1.0).abs() < 0.005, "{got}");
}

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
    assert_eq!(meter.take().reduction_db, 0.0);
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

#[test]
fn a_key_ducks_the_input_and_listen_plays_the_key() {
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
    let ducked = db_of(keyed.0[frames - 1]);
    assert!(ducked < -40.0, "the key ducks it: {ducked}");
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

#[test]
fn the_key_high_pass_takes_the_low_end_out_of_the_key() {
    let frames = 48_000;
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
    assert!(
        listen.0[frames - 1].abs() < 1e-3,
        "the low end is gone: {}",
        listen.0[frames - 1]
    );
}

#[test]
fn gravity_is_the_same_at_any_block_size() {
    let frames = 4_800;
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
    for ms in [0.0f32, 1.5, 4.0, 10.0] {
        let values = [(limiter::LOOK, at(limiter::TABLE, limiter::LOOK, ms))];
        assert_eq!(
            lumen_devices::effect_tail(limiter::NAME, &values),
            Some(latency_at(ms))
        );
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
    for k in TAPS..samples.len().saturating_sub(TAPS) {
        for phase in &phases {
            let mut value = 0.0f64;
            for (j, weight) in phase.iter().enumerate() {
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

#[test]
fn true_peak_mode_keeps_the_points_between_the_samples_under_the_ceiling() {
    let frames = 6_000;
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
            let settled = &left[2_400..];
            let right_settled = &right[2_400..];
            let ceiling = 10f32.powf(ceiling_db / 20.0);
            let reference = reference_true_peak(settled).max(reference_true_peak(right_settled));
            let over = db_of(reference) - ceiling_db;
            let allowed = if detect >= 0.5 { 0.05 } else { 1.0 };
            assert!(
                over <= allowed,
                "{frequency} Hz, detect {detect}: {over} dB over the ceiling by the reference \
                 ({reference} against {ceiling})"
            );
            assert!(peak(settled) <= ceiling + 1e-6);
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

#[test]
fn lowering_the_ceiling_holds_the_peak_already_in_the_window() {
    let frames = 6_000;
    let look_ms = 10.0f32;
    const BURST: usize = 1_000;
    let latency =
        limiter::latency(&[(limiter::LOOK, at(limiter::TABLE, limiter::LOOK, look_ms))]) as usize;
    let mut quiet = vec![0.02f32; frames];
    quiet[BURST..BURST + 100].fill(0.9);
    let new_ceiling_db = -20.0f32;
    let new_ceiling = 10f32.powf(new_ceiling_db / 20.0);
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
    let leave = BURST + latency;
    let span = leave..leave + 100;
    let dropped = render(Some(&lane));
    let loudest = peak(&dropped.0[span.clone()]).max(peak(&dropped.1[span.clone()]));
    assert!(
        loudest <= new_ceiling + 1e-6,
        "the burst left at {loudest}, over the new ceiling of {new_ceiling}"
    );
    let held = render(None);
    let before = peak(&held.0[span.clone()]).max(peak(&held.1[span]));
    assert!(
        before > 4.0 * new_ceiling,
        "the burst is loud when the ceiling stays up: {before}"
    );
}

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
    assert!(
        (reading.reduction_db - 12.0).abs() < 0.5,
        "{}",
        reading.reduction_db
    );
    assert!(reading.input_db > 5.0, "{}", reading.input_db);
    let mut left = vec![2.0f32; 4_800];
    let mut right = left.clone();
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
    let settled = meter.take();
    assert!(
        settled.output_db < -5.5 && settled.output_db > -6.5,
        "the output holds at the ceiling: {}",
        settled.output_db
    );
    assert!(
        settled.input_db > settled.output_db + 11.0,
        "the input is the loud signal, not the limited one: {settled:?}"
    );
    let after = meter.take();
    assert_eq!(after.reduction_db, 0.0);
    assert!(
        after.input_db < -100.0 && after.output_db < -100.0,
        "a read starts the levels afresh: {after:?}"
    );
}

#[test]
fn the_limiter_s_meter_keeps_the_loudest_block_between_two_reads() {
    let mut device = Limiter::new();
    let meter = device.meter();
    for level in [1.0f32, 0.01, 0.01] {
        let (mut left, mut right) = (vec![level; 64], vec![level; 64]);
        run(
            &mut device,
            &mut left,
            &mut right,
            limiter::TABLE,
            64,
            &[(limiter::LOOK, 0.0)],
            &[],
            None,
        );
    }
    let reading = meter.take();
    assert!(
        reading.input_db > -0.5,
        "the loud block is still the level read: {}",
        reading.input_db
    );
}

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

#[test]
fn a_new_lookahead_is_what_the_next_block_delays_by() {
    let frames = 2_048;
    let (mut left, mut right) = level(frames, 0.0);
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
    let want = amplitude * 10f32.powf(-13.5 / 20.0);
    assert!((wet - want).abs() < 2e-3, "wet: {wet} against {want}");
    let mid = 0.5 * (dry + wet);
    assert!((half - mid).abs() < 5e-3, "half: {half} against {mid}");
}

#[test]
fn the_attack_and_release_take_their_time_to_get_there() {
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
    let attack_ms = 20.0f32;
    let frames = 960 + 8;
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

#[test]
fn rms_detection_reads_the_mean_and_not_the_peak() {
    let frames = 96_000;
    let amplitude = 0.5f32;
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
                (
                    compressor::RELEASE,
                    at(compressor::TABLE, compressor::RELEASE, 2_000.0),
                ),
                (compressor::DETECT, detect),
            ],
            &[],
            None,
        );
        peak(&left[frames - 1_000..])
    };
    let peak_detection = settle(0.0);
    let rms_detection = settle(1.0);
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
    let ceiling = 10f32.powf(-6.0 / 20.0);
    assert!(left[0] <= ceiling + 1e-6, "{}", left[0]);
    assert!(left[0] > 0.2, "it is not silent either: {}", left[0]);
}

#[test]
fn a_lane_on_the_lookahead_is_ignored() {
    let frames = 4_800;
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
    let loudest = peak(&rest_l).max(peak(&rest_r));
    assert!(
        loudest <= ceiling + 1e-6,
        "the burst was replayed at {loudest}, above {ceiling}"
    );
    let first = peak(&rest_l[..64]);
    assert!(first < 1e-4, "a short gap, not a replay: {first}");
}

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
    let reading = meter.expect("a meter").take();
    assert!(reading.input_db > -1.0, "the key was read: {reading:?}");
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

fn pinned(left: &[f32]) -> ([f32; 20], f64, f64, f64) {
    let mut samples = [0.0f32; 20];
    for (k, sample) in samples.iter_mut().enumerate() {
        *sample = left[k * (left.len() / 20)];
    }
    let level: f64 = left.iter().map(|x| f64::from(x.abs())).sum();
    let energy: f64 = left.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    let peak = f64::from(left.iter().fold(0.0f32, |most, x| most.max(x.abs())));
    (samples, level, energy, peak)
}

#[track_caller]
fn close_sample(got: f64, want: f64, scale: f64) {
    assert!(
        (got - want).abs() <= 1e-6 * want.abs().max(scale),
        "{got:e} against the pinned {want:e}"
    );
}

#[track_caller]
fn close(got: f64, want: f64) {
    assert!(
        (got - want).abs() <= 1e-6 * want.abs().max(1e-6),
        "{got:e} against the pinned {want:e}"
    );
}

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
        let x = k as f64 * 0.031;
        left[k] = ((x * 29.0).sin() * 0.8) as f32;
        right[k] = ((x * 17.0).cos() * 0.7) as f32;
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
    let (samples, level, energy, peak) = pinned(&left);
    const PINS: [f32; 20] = [
        0.0,
        -1.028_203_6,
        1.768_817_8,
        -1.476_86,
        0.385_756_8,
        0.585_213_84,
        -0.772_766_65,
        0.603_440_6,
        -0.236_126_5,
        -0.128_319_77,
        0.341_472_2,
        -0.344_068_08,
        0.189_226_42,
        0.014_762_647,
        -0.180_116_53,
        0.247_255_09,
        -0.191_308_32,
        0.040_964_693,
        0.128_095_18,
        -0.231_132_5,
    ];
    for (k, want) in PINS.iter().enumerate() {
        close_sample(f64::from(samples[k]), f64::from(*want), peak);
    }
    close(level, 1.799_343_980_573_081_4e3);
    close(energy, 1.665_880_419_708_235_2e3);
}

#[test]
fn a_golden_render_of_horizon_is_pinned_to_its_samples() {
    let frames = 4_000;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    for k in 0..frames {
        let x = k as f64 * 0.043;
        left[k] = ((x * 37.0).sin() * 1.4) as f32;
        right[k] = ((x * 21.0).cos() * 1.2) as f32;
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
    let (samples, level, energy, peak) = pinned(&left);
    const PINS: [f32; 20] = [
        0.0,
        0.477_405_04,
        0.285_862_8,
        -0.752_826_2,
        0.627_100_05,
        -0.071_062_08,
        -0.507_156,
        0.702_255_37,
        -0.366_838_75,
        -0.245_695_55,
        0.672_627_7,
        -0.591_426_2,
        0.063_444_87,
        0.512_468_93,
        -0.701_240_6,
        0.360_273_3,
        0.252_859_47,
        -0.674_979_7,
        0.587_186_63,
        -0.055_820_62,
    ];
    for (k, want) in PINS.iter().enumerate() {
        close_sample(f64::from(samples[k]), f64::from(*want), peak);
    }
    close(level, 1.833_520_986_829_666e3);
    close(energy, 1.062_317_835_910_838_5e3);
}

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
    for table in [compressor::TABLE, limiter::TABLE] {
        let ids: std::collections::BTreeSet<u32> = table.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), table.len(), "an id is used twice");
        assert!(!ids.contains(&0), "0 is not a parameter");
        for p in table {
            assert!(!p.name.is_empty());
        }
    }
}

fn tone(frames: usize, frequency: f32, amplitude: f32) -> (Vec<f32>, Vec<f32>) {
    let sample =
        |k: usize| amplitude * (std::f32::consts::TAU * frequency * k as f32 / RATE as f32).sin();
    (
        (0..frames).map(sample).collect(),
        (0..frames).map(sample).collect(),
    )
}

fn loudest(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn curve_set(threshold: f32, ratio: f32, knee: f32) -> Vec<(u32, f32)> {
    vec![
        (
            compressor::THRESHOLD,
            at(compressor::TABLE, compressor::THRESHOLD, threshold),
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
    ]
}

#[test]
fn a_steady_tone_follows_the_curve_within_a_tenth_of_a_decibel() {
    let frames = 96_000;
    for input_db in [-30.0f32, -21.0, -18.0, -15.0, -12.0, -6.0, -1.0] {
        for (ratio, knee) in [(4.0f32, 6.0f32), (4.0, 0.0), (12.0, 6.0), (1.5, 12.0)] {
            let amplitude = 10f32.powf(input_db / 20.0);
            let (mut left, mut right) = tone(frames, 997.0, amplitude);
            let mut device = Compressor::new();
            let mut set = curve_set(-18.0, ratio, knee);
            set.push((
                compressor::DETECT,
                at(compressor::TABLE, compressor::DETECT, 1.0),
            ));
            run(
                &mut device,
                &mut left,
                &mut right,
                compressor::TABLE,
                256,
                &set,
                &[],
                None,
            );
            let rms = loudest(&left[frames - 2_000..]) / std::f32::consts::SQRT_2;
            let got_db = db_of(rms);
            let want_db = compressor::curve_db(input_db - 3.01, -18.0, ratio, knee);
            assert!(
                (got_db - want_db).abs() < 0.1,
                "{input_db} dB at {ratio}:1, {knee} dB knee: {got_db} against {want_db}"
            );
        }
    }
}

#[test]
fn a_peak_detector_reads_the_wave_and_that_is_its_curve() {
    let frames = 96_000;
    let mut worst = 0.0f32;
    let mut worst_at = 0.0f32;
    for input_db in [-30.0f32, -21.0, -18.0, -15.0, -12.0, -6.0, -1.0] {
        for (ratio, knee) in [(4.0f32, 6.0f32), (4.0, 0.0), (12.0, 6.0), (1.5, 12.0)] {
            let amplitude = 10f32.powf(input_db / 20.0);
            let (mut left, mut right) = tone(frames, 997.0, amplitude);
            let mut device = Compressor::new();
            run(
                &mut device,
                &mut left,
                &mut right,
                compressor::TABLE,
                256,
                &curve_set(-18.0, ratio, knee),
                &[],
                None,
            );
            let got_db = db_of(loudest(&left[frames - 2_000..]));
            let want_db = compressor::curve_db(input_db, -18.0, ratio, knee);
            if (got_db - want_db).abs() > worst {
                worst = (got_db - want_db).abs();
                worst_at = input_db;
            }
            if input_db < -18.0 - knee - 3.0 {
                assert!(
                    (got_db - want_db).abs() < 0.02,
                    "below the knee at {input_db} dB: {got_db} against {want_db}"
                );
            }
        }
    }
    assert!(
        worst < 1.2,
        "the peak detector is {worst} dB off the curve at {worst_at} dB"
    );
}

#[test]
fn sweeping_through_the_knee_does_not_step_the_level() {
    let frames = 96_000;
    let (mut left, mut right) = (vec![0.0f32; frames], vec![0.0f32; frames]);
    for k in 0..frames {
        let db = -30.0 + 24.0 * (k as f32 / frames as f32);
        let x =
            10f32.powf(db / 20.0) * (std::f32::consts::TAU * 997.0 * k as f32 / RATE as f32).sin();
        left[k] = x;
        right[k] = x;
    }
    let mut device = Compressor::new();
    run(
        &mut device,
        &mut left,
        &mut right,
        compressor::TABLE,
        256,
        &curve_set(-18.0, 4.0, 12.0),
        &[],
        None,
    );
    let step = RATE as usize / 1_000;
    let settled = 50 * step;
    let levels: Vec<f32> = left[settled..]
        .chunks(step)
        .map(|span| db_of(loudest(span)))
        .collect();
    let worst = levels
        .windows(2)
        .fold(0.0f32, |m, pair| m.max((pair[1] - pair[0]).abs()));
    assert!(
        worst < 0.1,
        "the level steps by {worst} dB in a millisecond"
    );
}

fn settle_settings(device: &mut Compressor, set: &[(u32, f32)]) {
    let (mut left, mut right) = level(RATE as usize / 40, 0.0);
    run(
        device,
        &mut left,
        &mut right,
        compressor::TABLE,
        256,
        set,
        &[],
        None,
    );
}

#[test]
fn the_attack_and_release_reach_63_percent_in_their_own_time() {
    let set = |attack_ms: f32, release_ms: f32| {
        let mut values = curve_set(-30.0, 20.0, 0.0);
        values.push((
            compressor::ATTACK,
            at(compressor::TABLE, compressor::ATTACK, attack_ms),
        ));
        values.push((
            compressor::RELEASE,
            at(compressor::TABLE, compressor::RELEASE, release_ms),
        ));
        values
    };
    let full = (20.0 * 0.5f32.log10() + 30.0) * (1.0 - 1.0 / 20.0);

    for attack_ms in [1.0f32, 5.0, 20.0, 100.0] {
        let tau = (attack_ms / 1000.0 * RATE as f32) as usize;
        let (mut left, mut right) = level(tau + 64, 0.5);
        let mut device = Compressor::new();
        settle_settings(&mut device, &set(attack_ms, 2_000.0));
        run(
            &mut device,
            &mut left,
            &mut right,
            compressor::TABLE,
            16,
            &set(attack_ms, 2_000.0),
            &[],
            None,
        );
        let reduction = -db_of(left[tau] / 0.5);
        let want = 0.632 * full;
        assert!(
            (reduction - want).abs() < 0.1 * full,
            "attack {attack_ms} ms: {reduction} dB of {full} (63 % is {want})"
        );
    }

    for release_ms in [20.0f32, 120.0, 500.0] {
        let mut settled = level(96_000, 0.5);
        let mut device = Compressor::new();
        settle_settings(&mut device, &set(0.05, release_ms));
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
        let before = -db_of(settled.0[settled.0.len() - 1] / 0.5);
        let tau = (release_ms / 1000.0 * RATE as f32) as usize;
        let mut quiet = level(tau + 64, 0.02);
        run(
            &mut device,
            &mut quiet.0,
            &mut quiet.1,
            compressor::TABLE,
            16,
            &set(0.05, release_ms),
            &[],
            None,
        );
        let after = -db_of(quiet.0[tau] / 0.02);
        let want = 0.368 * before;
        assert!(
            (after - want).abs() < 0.1 * before,
            "release {release_ms} ms: {after} dB of {before} (37 % is {want})"
        );
    }
}

#[test]
fn automatic_make_up_is_what_the_loud_end_lost() {
    for (threshold, ratio, want_db) in [
        (-18.0f32, 4.0f32, 0.0f32),
        (-18.0, 2.0, 0.0),
        (-6.0, 20.0, 0.0),
        (-30.0, 4.0, 0.0),
        (-60.0, 20.0, -33.0),
    ] {
        let frames = 48_000;
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
                    at(compressor::TABLE, compressor::THRESHOLD, threshold),
                ),
                (
                    compressor::RATIO,
                    at(compressor::TABLE, compressor::RATIO, ratio),
                ),
                (compressor::KNEE, 0.0),
            ],
            &[],
            None,
        );
        let got = db_of(left[frames - 1]);
        assert!(
            (got - want_db).abs() < 0.2,
            "{threshold} dB at {ratio}:1: a full-scale signal leaves at {got} dB, not {want_db}"
        );
    }
}

#[test]
fn the_sides_share_one_gain() {
    for detect in [0.0f32, 1.0] {
        let frames = 48_000;
        let (mut left, mut right) = (vec![0.0f32; frames], vec![0.0f32; frames]);
        for k in 0..frames {
            let phase = std::f32::consts::TAU * 997.0 * k as f32 / RATE as f32;
            left[k] = 0.5 * phase.sin();
            right[k] = 0.05 * phase.sin();
        }
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
                (compressor::DETECT, detect),
            ],
            &[],
            None,
        );
        let span = frames - 2_000..;
        let gain =
            |samples: &[f32], amplitude: f32| db_of(loudest(&samples[span.clone()]) / amplitude);
        let (left_gain, right_gain) = (gain(&left, 0.5), gain(&right, 0.05));
        assert!(
            (left_gain - right_gain).abs() < 0.05,
            "detect {detect}: {left_gain} dB on the left against {right_gain} on the right"
        );
        assert!(left_gain < -6.0, "and it is really pulling them down");
    }
}

#[test]
fn the_meter_says_the_gain_that_was_applied() {
    for input_db in [-24.0f32, -12.0, -3.0] {
        let frames = 48_000;
        let amplitude = 10f32.powf(input_db / 20.0);
        let (mut left, mut right) = level(frames, amplitude);
        let mut device = Compressor::new();
        let meter = device.meter();
        run(
            &mut device,
            &mut left,
            &mut right,
            compressor::TABLE,
            256,
            &curve_set(-18.0, 4.0, 6.0),
            &[],
            None,
        );
        let applied = -db_of(left[frames - 1] / amplitude);
        let shown = meter.take().reduction_db;
        assert!(
            (applied - shown).abs() < 0.05,
            "{input_db} dB: the samples lost {applied} dB, the face shows {shown}"
        );
    }
}

#[test]
fn a_long_silence_leaves_nothing_behind() {
    let quiet = 48_000 * 5;
    let frames = quiet + 48_000;
    let mut input = vec![0.0f32; frames];
    for (k, s) in input.iter_mut().enumerate().skip(quiet) {
        *s = 0.25 * (std::f32::consts::TAU * 997.0 * k as f32 / RATE as f32).sin();
    }
    let set = curve_set(-18.0, 4.0, 6.0);
    let render = |device: &mut Compressor| {
        let (mut left, mut right) = (input.clone(), input.clone());
        run(
            device,
            &mut left,
            &mut right,
            compressor::TABLE,
            256,
            &set,
            &[],
            None,
        );
        left
    };
    let after_silence = render(&mut Compressor::new());
    assert!(after_silence.iter().all(|s| s.is_finite()));
    let mut fresh = Compressor::new();
    let mut warm = vec![0.0f32; quiet];
    let mut warm_r = warm.clone();
    run(
        &mut fresh,
        &mut warm,
        &mut warm_r,
        compressor::TABLE,
        256,
        &set,
        &[],
        None,
    );
    let reachable = render(&mut fresh);
    let from = quiet + 24_000;
    let worst = (from..frames).fold(0.0f32, |m, k| {
        m.max((after_silence[k] - reachable[k]).abs())
    });
    assert!(worst < 1e-4, "the silence left {worst} behind");
}
