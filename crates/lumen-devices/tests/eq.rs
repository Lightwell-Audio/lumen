//! Iris, the parametric equaliser (M8 slice B1): what it does to the audio,
//! that its response is the analytic filter's, that it does the same thing at
//! every block size, that a swept band never steps the sound, and that it
//! allocates nothing while it runs.

use lumen_core::pan::PanLaw;
use lumen_devices::eq::{
    ALL_SECTIONS, BANDS, OUTPUT, TABLE, freq_of, gain_of, on_of, q_of, type_of,
};
use lumen_devices::params;
use lumen_devices::{Block, Effect, Eq, ParamBlock, Stereo};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;

/// The normalised value of `id` at `value` in its own units.
fn at(id: u32, value: f32) -> f32 {
    params::find(TABLE, id)
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

/// Everything at rest, with `set`'s values (normalised) and the others'
/// defaults.
fn defaults(frames: usize, set: &[(u32, f32)]) -> BlockBuilder {
    let mut block = BlockBuilder::new(frames);
    for info in TABLE {
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

/// Runs `device` over `left` and `right` a block at a time, `block` frames
/// each (at most the 256 a device is given), giving each block
/// `lanes(done, frames)` for its points.
fn run(
    device: &mut Eq,
    left: &mut [f32],
    right: &mut [f32],
    block: usize,
    lanes: &dyn Fn(usize, usize) -> BlockBuilder,
) {
    let block = block.clamp(1, 256);
    let mut done = 0;
    while done < left.len() {
        let n = block.min(left.len() - done);
        let events = lanes(done, n).build();
        let block = Block {
            rate: RATE,
            start: done as i64,
            law: PanLaw::Balance,
            events: &events,
            bpm: 120.0,
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
}

/// Iris run with every parameter at rest, for `frames` frames.
fn plain(set: &[(u32, f32)], left: &mut [f32], right: &mut [f32]) {
    let mut device = Eq::new();
    let set = set.to_vec();
    run(&mut device, left, right, 256, &|_, n| defaults(n, &set));
}

/// An RBJ peaking filter's coefficients, written out again here from the
/// cookbook, so the code under test is held to the formulas rather than to
/// itself.
fn rbj_peaking(hz: f32, gain: f32, q: f32, rate: f32) -> [f64; 5] {
    let w0 = 2.0 * std::f64::consts::PI * f64::from(hz) / f64::from(rate);
    let a = 10f64.powf(f64::from(gain) / 40.0);
    let alpha = w0.sin() / (2.0 * f64::from(q));
    let a0 = 1.0 + alpha / a;
    [
        (1.0 + alpha * a) / a0,
        (-2.0 * w0.cos()) / a0,
        (1.0 - alpha * a) / a0,
        (-2.0 * w0.cos()) / a0,
        (1.0 - alpha / a) / a0,
    ]
}

/// The gain of those coefficients at `hz`, in decibels.
fn magnitude_db(c: &[f64; 5], hz: f32, rate: f32) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f64::from(hz) / f64::from(rate);
    let (s1, c1) = (w.sin(), w.cos());
    let (s2, c2) = ((2.0 * w).sin(), (2.0 * w).cos());
    let num = ((c[0] + c[1] * c1 + c[2] * c2).powi(2) + (c[1] * s1 + c[2] * s2).powi(2)).sqrt();
    let den = ((1.0 + c[3] * c1 + c[4] * c2).powi(2) + (c[3] * s1 + c[4] * s2).powi(2)).sqrt();
    20.0 * (num / den).log10()
}

/// The gain a rendered tone gets, in decibels: the root mean square of the last
/// 4800 frames (a whole number of cycles at every probe) against the tone that
/// went in.
fn measured_db(set: &[(u32, f32)], hz: f32) -> f64 {
    let frames = 48_000;
    let amplitude = 0.25;
    let tone: Vec<f32> = (0..frames)
        .map(|k| amplitude * (std::f32::consts::TAU * hz * k as f32 / RATE as f32).sin())
        .collect();
    let mut left = tone.clone();
    let mut right = tone;
    plain(set, &mut left, &mut right);
    let tail = &left[frames - 4800..];
    let mean = tail
        .iter()
        .map(|x| f64::from(*x) * f64::from(*x))
        .sum::<f64>()
        / 4800.0;
    let rms = mean.sqrt();
    let measured = rms * std::f64::consts::SQRT_2;
    20.0 * (measured / f64::from(amplitude)).log10()
}

/// The acceptance line: what Iris does to a tone is what the analytic biquad
/// says, within a tenth of a decibel, at every kind of band.
#[test]
fn the_response_is_the_analytic_filter_within_a_tenth_of_a_decibel() {
    // A bell, against the cookbook formulas written out in this file.
    for (hz, gain, q) in [(1_000.0, 9.0, 2.0), (120.0, -9.0, 0.7), (6_000.0, 6.0, 4.0)] {
        let set = [
            (on_of(3), 1.0),
            (freq_of(3), at(freq_of(3), hz)),
            (gain_of(3), at(gain_of(3), gain)),
            (q_of(3), at(q_of(3), q)),
        ];
        let want = rbj_peaking(hz, gain, q, RATE as f32);
        for probe in [50.0, 200.0, 700.0, 1_000.0, 2_000.0, 8_000.0] {
            let got = measured_db(&set, probe);
            let want = magnitude_db(&want, probe, RATE as f32);
            assert!(
                (got - want).abs() < 0.1,
                "bell {hz} {gain} {q} at {probe}: {got:.3} against {want:.3}"
            );
        }
    }
    // A cut at the steepest slope, and a shelf, against the same maths the
    // face draws with.
    let cut = [
        (on_of(1), 1.0),
        (freq_of(1), at(freq_of(1), 1_000.0)),
        (gain_of(1), at(gain_of(1), 24.0)),
    ];
    // Two octaves below the corner the cut falls 24 dB an octave, so the
    // second octave is 24 dB under the first.
    let (half, quarter) = (measured_db(&cut, 500.0), measured_db(&cut, 250.0));
    assert!(
        (half - quarter - 24.0).abs() < 3.0,
        "the 24 dB cut fell {half:.1} then {quarter:.1}"
    );
    assert!(half < -18.0, "{half}");
    assert!((measured_db(&cut, 8_000.0)).abs() < 0.1);
    let shelf = [
        (on_of(4), 1.0),
        (freq_of(4), at(freq_of(4), 200.0)),
        (gain_of(4), at(gain_of(4), 6.0)),
        (type_of(4), at(type_of(4), 1.0)),
    ];
    assert!((measured_db(&shelf, 20.0) - 6.0).abs() < 0.15);
    assert!(measured_db(&shelf, 10_000.0).abs() < 0.1);
    // And the face's own answer agrees with the rendered one (the drawn curve
    // is the filter, not a picture of a different one).
    let values: Vec<(u32, f32)> = TABLE
        .iter()
        .map(|info| {
            let value = cut
                .iter()
                .find(|(id, _)| *id == info.id)
                .map_or(info.normalised(info.default), |(_, v)| *v);
            (info.id, value)
        })
        .collect();
    let mut out = [0.0f32; 4];
    lumen_devices::eq::response(&values, RATE, &mut out, (250.0, 250.0));
    assert!(
        (measured_db(&cut, 250.0) - f64::from(out[0])).abs() < 0.1,
        "drawn {} against heard {}",
        out[0],
        measured_db(&cut, 250.0)
    );
}

/// Nothing set: the audio comes out as it went in, to the last bit — a device
/// that does nothing costs nothing.
#[test]
fn at_its_defaults_it_passes_the_audio_through_untouched() {
    let mut left: Vec<f32> = (0..512).map(|k| (k as f32 * 0.01).sin() * 0.3).collect();
    let mut right = left.clone();
    let want = left.clone();
    plain(&[], &mut left, &mut right);
    assert_eq!(left, want);
}

/// The output gain is a gain, and a band that is off changes nothing.
#[test]
fn the_output_gain_is_heard_and_an_off_band_is_silent() {
    let mut left = vec![0.25; 4096];
    let mut right = vec![-0.25; 4096];
    plain(&[(OUTPUT, at(OUTPUT, -6.0))], &mut left, &mut right);
    let want = 0.25 * 10f32.powf(-6.0 / 20.0);
    assert!((left[4095] - want).abs() < 1e-6, "{}", left[4095]);
    assert!((right[4095] + want).abs() < 1e-6);
    // A +24 dB bell that is off leaves the tone where it was.
    let mut left = vec![0.25; 4096];
    let mut right = vec![-0.25; 4096];
    plain(
        &[
            (on_of(5), 0.0),
            (gain_of(5), at(gain_of(5), 24.0)),
            (freq_of(5), at(freq_of(5), 1_600.0)),
        ],
        &mut left,
        &mut right,
    );
    assert!((left[4095] - 0.25).abs() < 1e-6, "{}", left[4095]);
}

/// A band's frequency sweeping under a lane never makes the sound step: the
/// largest step from one sample to the next stays within a quarter of what the
/// same bell makes standing still.
#[test]
fn a_swept_band_never_makes_the_sound_step() {
    let frames = 48_000;
    let tone: Vec<f32> = (0..frames)
        .map(|k| 0.5 * (std::f32::consts::TAU * 300.0 * k as f32 / RATE as f32).sin())
        .collect();
    // The frequency rises from 200 Hz to 4 kHz over a second, in a straight
    // line on the axis the knob turns on.
    let low = at(freq_of(3), 200.0);
    let high = at(freq_of(3), 4_000.0);
    let sweep: Vec<(u32, f32)> = (0..=50)
        .map(|k| {
            (
                (k * (frames as u32 - 1) / 50),
                low + (high - low) * k as f32 / 50.0,
            )
        })
        .collect();
    let setting = [
        (on_of(3), 1.0),
        (gain_of(3), at(gain_of(3), 12.0)),
        (q_of(3), at(q_of(3), 3.0)),
    ];
    let render = |lane: Option<&[(u32, f32)]>| {
        let mut left = tone.clone();
        let mut right = tone.clone();
        let set = setting.to_vec();
        let lane = lane.map(<[(u32, f32)]>::to_vec);
        let mut device = Eq::new();
        run(&mut device, &mut left, &mut right, 128, &|done, n| {
            let mut block = defaults(n, &set);
            let points = match &lane {
                Some(points) => window(points, done, n),
                None => vec![(0, low)],
            };
            block = block.lane(freq_of(3), &points);
            block
        });
        left
    };
    let worst_step = |samples: &[f32]| {
        samples
            .windows(2)
            .fold(0.0f32, |most, pair| most.max((pair[1] - pair[0]).abs()))
    };
    let swept = render(Some(&sweep));
    // Standing still at each of the places the sweep passes, the loudest bell
    // makes the biggest step a smooth tone can make; the sweep may not better
    // it by more than a quarter.
    let still = [200.0f32, 300.0, 1_000.0, 4_000.0]
        .iter()
        .map(|hz| {
            let mut left = tone.clone();
            let mut right = tone.clone();
            let set = setting
                .iter()
                .copied()
                .chain([(freq_of(3), at(freq_of(3), *hz))])
                .collect::<Vec<_>>();
            let mut device = Eq::new();
            run(&mut device, &mut left, &mut right, 128, &|_, n| {
                defaults(n, &set)
            });
            worst_step(&left)
        })
        .fold(0.0f32, f32::max);
    let swept = worst_step(&swept);
    assert!(
        swept <= still * 1.25 + 1e-3,
        "the sweep stepped the sound: {swept} against {still} standing still"
    );
    assert!(swept < 0.25, "{swept}");
}

/// The same audio at every block size a device is given.
#[test]
fn the_same_audio_at_any_block_size() {
    let frames = 4800;
    let lane = |scale: f32| {
        (0..40)
            .map(|k| {
                (
                    (k * 120) as u32,
                    (0.2 + 0.6 * ((k as f32 * scale).sin() * 0.5 + 0.5)).clamp(0.0, 1.0),
                )
            })
            .collect::<Vec<(u32, f32)>>()
    };
    let freq = lane(0.7);
    let gain = lane(1.1);
    let q = lane(1.7);
    let on = lane(0.4);
    let output = lane(2.3);
    let input = |k: usize| {
        let x = k as f32 * 0.017;
        ((x * 31.0).sin() * 0.4, (x * 17.0).cos() * 0.3)
    };
    let render = |block: usize| {
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (0..frames).map(input).unzip();
        let mut device = Eq::new();
        run(&mut device, &mut left, &mut right, block, &|done, n| {
            let mut block = defaults(n, &[(on_of(2), 1.0), (on_of(5), 1.0)]);
            for (id, points) in [
                (freq_of(2), &freq),
                (gain_of(2), &gain),
                (q_of(2), &q),
                (on_of(5), &on),
                (OUTPUT, &output),
            ] {
                block = block.lane(id, &window(points, done, n));
            }
            block
        });
        left.into_iter().zip(right).collect::<Vec<(f32, f32)>>()
    };
    let whole = render(256);
    for block in [64usize, 97, 128, 256] {
        let parts = render(block);
        for (k, ((a, b), (c, d))) in whole.iter().zip(&parts).enumerate() {
            assert!(
                (a - c).abs() < 1e-4 && (b - d).abs() < 1e-4,
                "block {block} frame {k}: ({a}, {b}) against ({c}, {d})"
            );
        }
    }
}

/// The audio thread's rules: a block with automation allocates nothing.
#[test]
fn a_block_with_automation_allocates_nothing() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is installed"
    );
    let mut device = Eq::new();
    let mut left = vec![0.1; 256];
    let mut right = vec![-0.1; 256];
    let events = defaults(256, &[(on_of(2), 1.0), (on_of(6), 1.0), (on_of(7), 1.0)])
        .lane(freq_of(2), &[(0, 0.1), (40, 0.9), (41, 0.2), (200, 0.6)])
        .lane(gain_of(2), &[(0, 0.1), (128, 0.9)])
        .lane(freq_of(7), &[(0, 1.0), (255, 0.5)])
        .lane(on_of(6), &[(0, 0.0), (120, 1.0)])
        .build();
    let block = Block {
        rate: RATE,
        start: 0,
        law: PanLaw::ConstantPower,
        events: &events,
        bpm: 120.0,
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

/// The analyser reads the side its own parameter names, and a tap nothing
/// listens to costs nothing.
#[test]
fn the_analyser_reads_the_side_it_is_told_to() {
    let frames = 8192;
    let tone: Vec<f32> = (0..frames)
        .map(|k| 0.5 * (std::f32::consts::TAU * 1_000.0 * k as f32 / RATE as f32).sin())
        .collect();
    let peak_of = |window: &[f32]| window.iter().fold(0.0f32, |most, x| most.max(x.abs()));
    // The output side: a +12 dB bell at 1 kHz makes the window louder.
    for (side, want_louder) in [(1.0f32, true), (0.0, false)] {
        let (tap, mut reader) = lumen_devices::analyzer::pair();
        reader.set_on(true);
        let mut device = Eq::with_tap(Some(tap));
        let mut left = tone.clone();
        let mut right = tone.clone();
        let set = [
            (on_of(3), 1.0),
            (freq_of(3), at(freq_of(3), 1_000.0)),
            (gain_of(3), at(gain_of(3), 12.0)),
            (
                lumen_devices::eq::ANALYZER,
                at(lumen_devices::eq::ANALYZER, side),
            ),
        ];
        run(&mut device, &mut left, &mut right, 256, &|_, n| {
            defaults(n, &set)
        });
        assert_eq!(
            reader.read(),
            lumen_devices::Read::Window,
            "a window was written"
        );
        let peak = peak_of(reader.window());
        if want_louder {
            assert!(peak > 1.5, "the output side hears the boost: {peak}");
        } else {
            assert!(
                (peak - 0.5).abs() < 1e-3,
                "the input side hears the input: {peak}"
            );
        }
    }
}

#[test]
fn the_table_names_the_device_it_is_for() {
    assert_eq!(lumen_devices::eq::NAME, "eq");
    assert!(lumen_devices::effect_params(lumen_devices::eq::NAME).is_some());
    // Every band's sections fit the array the filter runs.
    assert_eq!(ALL_SECTIONS, 4 + (BANDS - 2) + 4);
}
