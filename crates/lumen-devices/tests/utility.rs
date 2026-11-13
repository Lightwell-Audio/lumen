//! Prism, the utility device (M8 slice B0): what it does to the audio, that it
//! does the same thing at every block size, and that it allocates nothing
//! while it runs.

use lumen_core::pan::PanLaw;
use lumen_devices::params;
use lumen_devices::utility::{
    self, BASS_MONO, DC_BLOCK, GAIN, PAN, POLARITY_L, POLARITY_R, SWAP, TABLE, WIDTH,
};
use lumen_devices::{Block, Effect, ParamBlock, Stereo, Utility};

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

    /// Parameter `id` at rest at `value` (normalised), or its default.
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

/// The points a lane gives one block, as the engine fills them: where it is at
/// the first frame, each of its points inside the block, and where it has got
/// to at the last.
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
/// `lanes(frame, frames)` for its points.
fn run(
    device: &mut Utility,
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

/// A device run with every parameter at rest.
fn plain(set: &[(u32, f32)], left: &mut [f32], right: &mut [f32]) {
    let mut device = Utility::new();
    let set = set.to_vec();
    run(&mut device, left, right, left.len(), &|_, n| {
        defaults(n, &set)
    });
}

#[test]
fn at_its_defaults_it_passes_the_audio_through_untouched() {
    let mut left: Vec<f32> = (0..512).map(|k| (k as f32 * 0.01).sin() * 0.3).collect();
    let mut right = left.clone();
    let want = left.clone();
    plain(&[], &mut left, &mut right);
    for k in 0..left.len() {
        assert!((left[k] - want[k]).abs() < 1e-7, "frame {k}: {}", left[k]);
        assert_eq!(left[k], right[k]);
    }
}

#[test]
fn the_gain_reads_in_decibels() {
    let mut left = vec![0.25; 256];
    let mut right = vec![-0.25; 256];
    let mut device = Utility::new();
    // Past the glide.
    run(&mut device, &mut left, &mut right, 256, &|_, n| {
        defaults(n, &[(GAIN, at(GAIN, -6.0))])
    });
    let mut left = vec![0.25; 4096];
    let mut right = vec![-0.25; 4096];
    run(&mut device, &mut left, &mut right, 128, &|_, n| {
        defaults(n, &[(GAIN, at(GAIN, -6.0))])
    });
    let want = 0.25 * 10f32.powf(-6.0 / 20.0);
    assert!((left[4095] - want).abs() < 1e-6, "{}", left[4095]);
    assert!((right[4095] + want).abs() < 1e-6);
}

#[test]
fn pan_follows_the_law_the_track_has() {
    for (law, want) in [
        (PanLaw::Balance, [1.0, 0.0]),
        (PanLaw::Linear, [1.0, 0.0]),
        (PanLaw::ConstantPower, [1.0, 0.0]),
    ] {
        let mut left = vec![0.5; 4096];
        let mut right = vec![0.5; 4096];
        let mut device = Utility::new();
        let mut done = 0;
        while done < 4096 {
            let events = defaults(256, &[(PAN, at(PAN, -100.0))]).build();
            let block = Block {
                rate: RATE,
                start: done as i64,
                law,
                events: &events,
            };
            device.process(
                &block,
                Stereo {
                    left: &mut left[done..done + 256],
                    right: &mut right[done..done + 256],
                },
                None,
            );
            done += 256;
        }
        assert!(
            (left[4095] - 0.5 * want[0]).abs() < 1e-6,
            "{law:?}: {}",
            left[4095]
        );
        assert!(
            (right[4095] - 0.5 * want[1]).abs() < 1e-6,
            "{law:?}: {}",
            right[4095]
        );
    }
    // A quarter to the right, by the balance law: the left side is turned
    // down and the right stays at unity.
    let mut left = vec![0.5; 4096];
    let mut right = vec![0.5; 4096];
    plain(&[(PAN, at(PAN, 25.0))], &mut left, &mut right);
    assert!((left[4095] - 0.375).abs() < 1e-6, "{}", left[4095]);
    assert!((right[4095] - 0.5).abs() < 1e-6);
}

#[test]
fn the_width_spreads_or_folds_the_sides() {
    // 0 % is mono: both sides are the middle.
    let mut left = vec![0.5; 4096];
    let mut right = vec![-0.25; 4096];
    plain(&[(WIDTH, at(WIDTH, 0.0))], &mut left, &mut right);
    assert!((left[4095] - 0.125).abs() < 1e-6, "{}", left[4095]);
    assert_eq!(left[4095], right[4095]);
    // 200 % doubles what the sides differ by, so the middle stays put.
    let mut left = vec![0.5; 4096];
    let mut right = vec![-0.25; 4096];
    plain(&[(WIDTH, at(WIDTH, 200.0))], &mut left, &mut right);
    assert!((left[4095] - 0.875).abs() < 1e-6, "{}", left[4095]);
    assert!((right[4095] + 0.625).abs() < 1e-6, "{}", right[4095]);
}

#[test]
fn each_side_has_its_own_polarity_and_the_sides_can_swap() {
    let mut left = vec![0.5; 4096];
    let mut right = vec![0.25; 4096];
    plain(&[(POLARITY_L, 1.0)], &mut left, &mut right);
    assert!((left[4095] + 0.5).abs() < 1e-6);
    assert!((right[4095] - 0.25).abs() < 1e-6);
    let mut left = vec![0.5; 4096];
    let mut right = vec![0.25; 4096];
    plain(&[(POLARITY_R, 1.0)], &mut left, &mut right);
    assert!(left[4095] == 0.5 && right[4095] == -0.25);
    let mut left = vec![0.5; 4096];
    let mut right = vec![0.25; 4096];
    plain(&[(SWAP, 1.0)], &mut left, &mut right);
    assert!((left[4095] - 0.25).abs() < 1e-6 && (right[4095] - 0.5).abs() < 1e-6);
}

#[test]
fn bass_mono_centres_the_low_end_and_leaves_the_high_end_alone() {
    // The side signal holds a constant (the lowest frequency there is) and an
    // alternating one (the highest): under 200 Hz, only the constant goes.
    let mut left = vec![0.0; 4096];
    let mut right = vec![0.0; 4096];
    for k in 0..4096 {
        let low = 0.2;
        let high = if k % 2 == 0 { 0.1 } else { -0.1 };
        left[k] = 0.5 + low + high;
        right[k] = 0.5 - low - high;
    }
    plain(&[(BASS_MONO, at(BASS_MONO, 200.0))], &mut left, &mut right);
    // Late in the block the constant is gone from the sides: the two sides
    // differ only by the fast part, in anti-phase.
    let (l, r) = (left[4000], right[4000]);
    assert!(
        (l + r - 1.0).abs() < 1e-3,
        "the middle stays put: {}",
        l + r
    );
    assert!(
        (l - r - 0.2).abs() < 5e-3,
        "the fast part stays apart: {}",
        l - r
    );
    // With it off, the low part is still there.
    let mut left2 = vec![0.0; 4096];
    let mut right2 = vec![0.0; 4096];
    for k in 0..4096 {
        left2[k] = 0.5 + 0.2 + if k % 2 == 0 { 0.1 } else { -0.1 };
        right2[k] = 0.5 - 0.2 - if k % 2 == 0 { 0.1 } else { -0.1 };
    }
    plain(&[], &mut left2, &mut right2);
    assert!((left2[4000] - right2[4000] - 0.6).abs() < 1e-6);
}

#[test]
fn the_dc_block_takes_a_constant_offset_out() {
    let mut left = vec![0.1; 48_000];
    let mut right = vec![0.1; 48_000];
    plain(&[(DC_BLOCK, 1.0)], &mut left, &mut right);
    assert!(left[0] > 0.05, "it starts where the audio was: {}", left[0]);
    assert!(
        left[47_999].abs() < 1e-3,
        "and settles to nothing: {}",
        left[47_999]
    );
    assert!(right[47_999].abs() < 1e-3);
}

/// Every parameter moving at once (a lane for each) plays the same audio at
/// every block size.
#[test]
fn the_same_audio_at_any_block_size() {
    let frames = 4800;
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
    let gain = lane(0.7);
    let pan = lane(1.1);
    let width = lane(1.7);
    let bass = lane(0.4);
    let swap = lane(2.3);
    let input = |k: usize| {
        let x = k as f32 * 0.017;
        ((x * 31.0).sin() * 0.4, (x * 17.0).cos() * 0.3)
    };
    let render = |block: usize| {
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (0..frames).map(input).unzip();
        let mut device = Utility::new();
        run(&mut device, &mut left, &mut right, block, &|done, n| {
            let mut block = BlockBuilder::new(n);
            for info in TABLE {
                block = block.held(info.id, info.normalised(info.default));
            }
            for (id, points) in [
                (GAIN, &gain),
                (PAN, &pan),
                (WIDTH, &width),
                (BASS_MONO, &bass),
                (SWAP, &swap),
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
                (a - c).abs() < 2e-6 && (b - d).abs() < 2e-6,
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
    let mut device = Utility::new();
    let mut left = vec![0.1; 256];
    let mut right = vec![-0.1; 256];
    let events = defaults(256, &[(GAIN, 0.7), (PAN, 0.2), (BASS_MONO, 0.5)])
        .lane(GAIN, &[(0, 0.1), (40, 0.9), (41, 0.2), (200, 0.6)])
        .lane(PAN, &[(0, 0.1), (128, 0.9)])
        .lane(WIDTH, &[(0, 1.0), (255, 0.5)])
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
            None,
        );
    });
    assert_eq!(violations, 0, "the audio thread allocated");
}

#[test]
fn a_reset_forgets_the_filters_and_jumps_the_values() {
    let mut left = vec![0.1; 512];
    let mut right = vec![0.1; 512];
    plain(
        &[(DC_BLOCK, 1.0), (GAIN, at(GAIN, -12.0))],
        &mut left,
        &mut right,
    );
    let mut device = Utility::new();
    device.reset();
    let mut left = vec![0.25; 64];
    let mut right = vec![0.25; 64];
    run(&mut device, &mut left, &mut right, 64, &|_, n| {
        defaults(n, &[(GAIN, at(GAIN, 0.0))])
    });
    // No glide from anywhere: unity at once.
    assert!((left[0] - 0.25).abs() < 1e-6, "{}", left[0]);
}

#[test]
fn the_table_names_the_device_it_is_for() {
    assert_eq!(utility::NAME, "utility");
    assert!(lumen_devices::effect_params(utility::NAME).is_some());
}

/// The table, pinned (M8 slice B0, review L1): each parameter's id, the name a
/// person reads, and the normalised default. An id may not be reused or
/// renumbered and a name may not change without moving this list on purpose: a
/// project saves values by id, so a changed table is a changed sound.
#[test]
fn the_parameter_table_is_pinned_by_id_name_and_default() {
    let pinned: Vec<(u32, &str, f32)> = vec![
        (1, "Gain", 60.0 / 84.0),
        (2, "Pan", 0.5),
        (3, "Width", 0.5),
        (4, "Mono below", 0.0),
        (5, "Polarity L", 0.0),
        (6, "Polarity R", 0.0),
        (7, "Swap L/R", 0.0),
        (8, "DC block", 0.0),
    ];
    let table: Vec<(u32, &str, f32)> = TABLE
        .iter()
        .map(|p| (p.id, p.name, p.normalised(p.default)))
        .collect();
    assert_eq!(table, pinned);
    // The constants a project and a lane name are those ids.
    assert_eq!(
        [GAIN, PAN, WIDTH, BASS_MONO, POLARITY_L, POLARITY_R, SWAP],
        [1, 2, 3, 4, 5, 6, 7]
    );
    assert_eq!(DC_BLOCK, 8);
    // No id is reused and none is zero.
    let ids: std::collections::BTreeSet<u32> = TABLE.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), TABLE.len(), "an id is used twice");
    assert!(!ids.contains(&0), "0 is not a parameter");
}
