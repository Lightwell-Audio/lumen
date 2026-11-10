//! The worker's cost of warped playback (M9.5 S4c, ADR-0132): the disk thread
//! renders the frames of every warped clip ahead of the playhead, so what
//! matters is how long it takes to render a second of 64 stereo tracks, in
//! Repitch and in each of the stretcher's two settings. The audio callback
//! does not change: it reads pages as it does for any streamed clip
//! (`engine_streamed_64` in the performance gate).
//!
//! A track renders a second in `t`; 64 of them on one worker take `64 t`, so
//! the worker keeps up while that is under a second (`t` under 15.6 ms).

use divan::black_box;
use lumen_media::MediaError;
use lumen_media::repitch::{Renderer, Source};
use lumen_media::stretch::{CHUNK, Chunks, Preset, Stretcher};

fn main() {
    divan::main();
}

const RATE: u32 = 48_000;
const TRACKS: usize = 64;
/// A second: what a track is asked for per run.
const SECOND: usize = RATE as usize;

struct Mem {
    planes: Vec<Vec<f32>>,
}

impl Source for Mem {
    fn channels(&self) -> usize {
        self.planes.len()
    }

    fn frames(&self) -> u64 {
        self.planes[0].len() as u64
    }

    fn read(&mut self, from: u64, out: &mut [&mut [f32]]) -> Result<(), MediaError> {
        for (out, plane) in out.iter_mut().zip(&self.planes) {
            let from = (from as usize).min(plane.len());
            let n = out.len().min(plane.len() - from);
            out[..n].copy_from_slice(&plane[from..from + n]);
            out[n..].fill(0.0);
        }
        Ok(())
    }
}

/// Ten seconds of a noisy two-partial tone.
fn file() -> Mem {
    let mut seed = 1_u32;
    let mut plane = |phase: f64| -> Vec<f32> {
        (0..10 * SECOND)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f32 / (1 << 23) as f32 - 0.5;
                let t = i as f64 / f64::from(RATE);
                (0.3 * (std::f64::consts::TAU * 220.0 * t + phase).sin() as f32) + 0.05 * noise
            })
            .collect()
    };
    Mem {
        planes: vec![plane(0.0), plane(1.0)],
    }
}

/// A tenth faster than the file, as a 132 BPM song over a 120 BPM loop.
fn position(k: i64) -> f64 {
    k as f64 * 1.1 + 1_000.0
}

/// One track's second, in Repitch.
#[divan::bench]
fn repitch_one_track(bencher: divan::Bencher<'_, '_>) {
    let mut source = file();
    let mut renderer = Renderer::new();
    let (mut l, mut r) = (vec![0.0; SECOND], vec![0.0; SECOND]);
    bencher.bench_local(|| {
        renderer
            .render(&mut source, &position, 2 * SECOND as i64, &mut l, &mut r)
            .unwrap();
        black_box((&l, &r));
    });
}

/// One track's second, stretched; a fresh chunk every run so the cost of
/// starting one (the pre-roll and the seam's tail) is in the figure.
#[divan::bench(args = [Preset::Full, Preset::Light])]
fn stretch_one_track(bencher: divan::Bencher<'_, '_>, preset: Preset) {
    let mut source = file();
    let mut chunks = Chunks::new(Stretcher::new(2, RATE, preset).unwrap());
    let (mut l, mut r) = (vec![0.0; SECOND], vec![0.0; SECOND]);
    let mut at = 0_i64;
    bencher.bench_local(|| {
        // A second at a time through the file, ten seconds round.
        at = (at + SECOND as i64) % (3 * CHUNK);
        chunks
            .render(&mut source, (&position, None), at, &mut l, &mut r)
            .unwrap();
        black_box((&l, &r));
    });
}

/// The 64 tracks' second on one worker, stretched: the number the plan wants.
#[divan::bench(args = [Preset::Full, Preset::Light], sample_count = 5)]
fn warped_64(bencher: divan::Bencher<'_, '_>, preset: Preset) {
    // One file read by every track: the cost is the stretching, not the memory.
    let mut source = file();
    let mut tracks: Vec<Chunks> = (0..TRACKS)
        .map(|_| Chunks::new(Stretcher::new(2, RATE, preset).unwrap()))
        .collect();
    let (mut l, mut r) = (vec![0.0; SECOND], vec![0.0; SECOND]);
    let mut at = 0_i64;
    bencher.bench_local(|| {
        at = (at + SECOND as i64) % (3 * CHUNK);
        for chunks in &mut tracks {
            chunks
                .render(&mut source, (&position, None), at, &mut l, &mut r)
                .unwrap();
        }
        black_box((&l, &r));
    });
}
