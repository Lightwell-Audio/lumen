use super::*;

/// A file in memory.
struct Memory(Vec<Vec<f32>>);

impl Source for Memory {
    fn channels(&self) -> usize {
        self.0.len()
    }

    fn frames(&self) -> u64 {
        self.0[0].len() as u64
    }

    fn read(&mut self, from: u64, out: &mut [&mut [f32]]) -> Result<(), MediaError> {
        for (c, out) in out.iter_mut().enumerate() {
            let data = &self.0[c];
            let from = from as usize;
            let n = out.len().min(data.len().saturating_sub(from));
            out[..n].copy_from_slice(&data[from..from + n]);
        }
        Ok(())
    }
}

fn sine(frames: usize, cycles_per_frame: f64) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f64::consts::TAU * cycles_per_frame * i as f64).sin() as f32)
        .collect()
}

fn run(
    source: &mut Memory,
    position: &dyn Fn(i64) -> f64,
    first: i64,
    n: usize,
) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![9.0; n], vec![9.0; n]);
    Renderer::new()
        .render(source, position, first, &mut l, &mut r)
        .unwrap();
    (l, r)
}

#[test]
fn a_file_played_as_it_was_made_is_copied() {
    let data = sine(5_000, 0.013);
    let mut source = Memory(vec![data.clone(), data.iter().map(|v| -v).collect()]);
    let (l, r) = run(&mut source, &|k| k as f64 + 100.0, 0, 3_000);
    assert_eq!(l, data[100..3_100]);
    assert!(l.iter().zip(&r).all(|(l, r)| *l == -*r));
}

#[test]
fn a_new_speed_moves_the_pitch_by_the_ratio() {
    let tone = 0.02;
    let data = sine(60_000, tone);
    for speed in [0.5, 0.8, 1.0 + 1.0e-3, 1.37, 2.0, 3.0] {
        let mut source = Memory(vec![data.clone()]);
        let (l, _) = run(&mut source, &|k| 2_000.0 + k as f64 * speed, 0, 8_000);
        // The file's sine seen at the new speed: the tone times the speed.
        let worst = (0..8_000)
            .map(|k| {
                let want = (std::f64::consts::TAU * tone * (2_000.0 + k as f64 * speed)).sin();
                (f64::from(l[k]) - want).abs()
            })
            .fold(0.0, f64::max);
        assert!(worst < 2.0e-3, "speed {speed}: off by {worst}");
    }
}

#[test]
fn a_run_holds_the_same_bits_whatever_its_pieces() {
    // A position that bends: a ramp and a kink, like a marker.
    let position = |k: i64| {
        let k = k as f64;
        if k < 20_000.0 {
            500.0 + k * 0.83
        } else {
            500.0 + 20_000.0 * 0.83 + (k - 20_000.0) * 1.9
        }
    };
    let data = sine(120_000, 0.031);
    let mut whole = Memory(vec![data.clone(), data.clone()]);
    let (wl, wr) = run(&mut whole, &position, 0, 40_000);
    // Pieces of odd lengths, starting off the grid, across the kink.
    let mut renderer = Renderer::new();
    let mut at = 0_usize;
    for len in [
        1, 7, 4_095, 4_096, 4_097, 15, 16, 17, 9_001, 3_000, 5, 20_000,
    ] {
        let len = len.min(40_000 - at);
        if len == 0 {
            break;
        }
        let (mut l, mut r) = (vec![0.0; len], vec![0.0; len]);
        renderer
            .render(&mut whole, &position, at as i64, &mut l, &mut r)
            .unwrap();
        assert_eq!(l, wl[at..at + len], "left from {at}");
        assert_eq!(r, wr[at..at + len], "right from {at}");
        at += len;
    }
}

#[test]
fn a_file_that_runs_fast_is_filtered_before_it_folds_back() {
    // 0.4 of the file's rate is far above where twice the speed can keep it:
    // played at 2x it would fold to 0.2 if it were not filtered out.
    let data = sine(40_000, 0.4);
    let mut source = Memory(vec![data]);
    let (l, _) = run(&mut source, &|k| 1_000.0 + k as f64 * 2.0, 0, 8_000);
    let loudest = l[200..7_800].iter().fold(0.0_f32, |m, v| m.max(v.abs()));
    assert!(loudest < 0.01, "folded back at {loudest}");
}

#[test]
fn outside_the_file_is_silent_and_the_edges_do_not_ring() {
    let mut source = Memory(vec![vec![0.5; 1_000]]);
    // Before the start, across it, across the end and past it.
    let (l, _) = run(&mut source, &|k| k as f64 - 3_000.0 + 0.25, 0, 6_000);
    assert!(l[..2_000].iter().all(|v| *v == 0.0));
    assert!(l[5_000..].iter().all(|v| *v == 0.0));
    assert!(l[3_100..3_900].iter().all(|v| (v - 0.5).abs() < 1.0e-3));
    // A step of 0.5 is bounded: the kernel overshoots by well under a tenth.
    assert!(l.iter().all(|v| v.abs() < 0.6), "ringing past the edge");
}

#[test]
fn a_mono_file_leaves_the_right_alone() {
    let mut source = Memory(vec![sine(2_000, 0.01)]);
    let (l, r) = run(&mut source, &|k| k as f64 * 1.1, 0, 1_000);
    assert!(l.iter().any(|v| *v != 0.0));
    assert!(r.iter().all(|v| *v == 9.0), "the right was touched");
}

#[test]
fn the_kernel_passes_a_whole_frame_through_untouched() {
    let kernel = Kernel::new();
    assert_eq!(kernel.at(0.0), 1.0);
    for n in 1..HALF {
        assert!(kernel.at(n as f64).abs() < 1.0e-6, "tap {n}");
    }
    assert_eq!(kernel.at(HALF as f64), 0.0);
    assert_eq!(kernel.at(HALF as f64 + 5.0), 0.0);
}
