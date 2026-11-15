use super::*;
use crate::MediaError;
use crate::repitch::Source;

const RATE: u32 = 48_000;

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

fn burst(frames: usize, centre: f64, sigma: f64) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            let t = (i as f64 - centre) / sigma;
            let envelope = (-0.5 * t * t).exp();
            (envelope * (std::f64::consts::TAU * 1000.0 * i as f64 / f64::from(RATE)).sin()) as f32
        })
        .collect()
}

fn tone(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            let t = i as f64 / f64::from(RATE);
            let tau = std::f64::consts::TAU;
            (0.4 * (tau * 220.0 * t).sin()
                + 0.25 * (tau * 660.0 * t).sin()
                + 0.1 * (tau * 1870.0 * t).sin()) as f32
        })
        .collect()
}

fn chunks(preset: Preset, channels: usize) -> Chunks {
    Chunks::new(Stretcher::new(channels, RATE, preset).expect("the stretcher is made"))
}

fn render(
    chunks: &mut Chunks,
    source: &mut Mem,
    position: &dyn Fn(i64) -> f64,
    first: i64,
    frames: usize,
) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    chunks
        .render(source, (position, None), first, &mut l, &mut r)
        .expect("renders");
    (l, r)
}

fn centroid(x: &[f32]) -> f64 {
    let (mut sum, mut weight) = (0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let e = f64::from(*v) * f64::from(*v);
        sum += e * i as f64;
        weight += e;
    }
    sum / weight
}

fn amplitude(x: &[f32], hz: f64) -> f64 {
    let n = x.len() as f64;
    let (mut re, mut im, mut window) = (0.0, 0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n).cos();
        let phase = std::f64::consts::TAU * hz * i as f64 / f64::from(RATE);
        re += w * f64::from(*v) * phase.cos();
        im += w * f64::from(*v) * phase.sin();
        window += w;
    }
    2.0 * re.hypot(im) / window
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

#[test]
fn the_stretcher_is_made_with_latencies() {
    for preset in [Preset::Full, Preset::Light] {
        let s = Stretcher::new(2, RATE, preset).unwrap();
        assert!(s.block() > s.interval());
        assert!(s.interval() > 100);
        assert!(
            s.input_latency() > 0 && s.output_latency() > 0,
            "{preset:?}"
        );
        eprintln!(
            "{preset:?}: block {} interval {} input latency {} output latency {}",
            s.block(),
            s.interval(),
            s.input_latency(),
            s.output_latency()
        );
    }
}

#[test]
fn a_burst_lands_where_the_map_puts_it() {
    for preset in [Preset::Full, Preset::Light] {
        for speed in [0.5_f64, 0.8, 1.01, 1.25, 2.0] {
            let want = 30_000.0;
            let mut source = Mem {
                planes: vec![
                    burst(400_000, want * speed, 1_500.0 * speed),
                    burst(400_000, want * speed, 1_500.0 * speed),
                ],
            };
            let mut c = chunks(preset, 2);
            let (l, _) = render(&mut c, &mut source, &|k| k as f64 * speed, 0, 80_000);
            let got = centroid(&l);
            eprintln!("{preset:?} speed {speed}: centre {got:.1}, want {want}");
            assert!((got - want).abs() < 64.0, "{preset:?} {speed}: {got}");
        }
    }
}

#[test]
fn a_steady_tone_keeps_its_level_and_pitch_at_every_speed() {
    let mut source = Mem {
        planes: vec![tone(1_000_000)],
    };
    for speed in [0.5_f64, 0.9, 0.998, 1.001, 1.002, 1.05, 2.0] {
        let mut c = chunks(Preset::Full, 1);
        let (l, _) = render(
            &mut c,
            &mut source,
            &|k| 5_000.0 + k as f64 * speed,
            20_000,
            40_000,
        );
        let level = rms(&l);
        let want = rms(&source.planes[0][100_000..140_000]);
        let (a220, a660) = (amplitude(&l, 220.0), amplitude(&l, 660.0));
        eprintln!(
            "speed {speed}: level {level:.4} want {want:.4}, 220 Hz {a220:.3}, 660 Hz {a660:.3}"
        );
        assert!(
            (level / want - 1.0).abs() < 0.15,
            "{speed}: {level} vs {want}"
        );
        assert!((a220 / 0.4 - 1.0).abs() < 0.15, "{speed}: 220 Hz is {a220}");
        assert!(
            (a660 / 0.25 - 1.0).abs() < 0.15,
            "{speed}: 660 Hz is {a660}"
        );
        if (speed - 1.0).abs() > 0.2 {
            let moved = amplitude(&l, 220.0 * speed);
            assert!(moved < 0.05, "{speed}: {moved} at the transposed pitch");
        }
    }
}

#[test]
fn high_partials_keep_their_level_and_pitch_just_off_unity() {
    let high = |frames: usize| -> Vec<f32> {
        (0..frames)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                let tau = std::f64::consts::TAU;
                (0.3 * (tau * 4_000.0 * t).sin()
                    + 0.2 * (tau * 9_000.0 * t).sin()
                    + 0.1 * (tau * 15_000.0 * t).sin()) as f32
            })
            .collect()
    };
    let mut source = Mem {
        planes: vec![high(1_000_000)],
    };
    for preset in [Preset::Full, Preset::Light] {
        for speed in [0.97_f64, 0.998, 0.9999, 1.0001, 1.002, 1.03] {
            let mut c = chunks(preset, 1);
            let (l, _) = render(
                &mut c,
                &mut source,
                &|k| 5_000.0 + k as f64 * speed,
                20_000,
                48_000,
            );
            for (hz, want) in [(4_000.0, 0.3), (9_000.0, 0.2), (15_000.0, 0.1)] {
                let got = amplitude(&l, hz);
                eprintln!("{preset:?} speed {speed}: {hz} Hz {got:.4} (want {want})");
                assert!(
                    (got / want - 1.0).abs() < 0.1,
                    "{preset:?} {speed}: {hz} Hz is {got}, not {want}"
                );
            }
        }
    }
}

fn ramp(k: i64) -> f64 {
    let t = k as f64;
    t * 1.05
        + 0.25 * 144_000.0 / std::f64::consts::TAU * (std::f64::consts::TAU * t / 144_000.0).sin()
        + 7_000.0
}

fn music() -> Mem {
    Mem {
        planes: vec![tone(900_000), tone(900_000).iter().rev().copied().collect()],
    }
}

#[test]
fn any_way_of_asking_gives_the_same_bits() {
    let total = 3 * CHUNK as usize + 5_000;
    let mut whole = chunks(Preset::Full, 2);
    let (wl, wr) = render(&mut whole, &mut music(), &ramp, 0, total);
    assert!(rms(&wl) > 0.1, "something was rendered");

    let mut pages = chunks(Preset::Full, 2);
    let mut source = music();
    let (mut pl, mut pr) = (Vec::new(), Vec::new());
    for first in (0..total).step_by(4096) {
        let n = 4096.min(total - first);
        let (l, r) = render(&mut pages, &mut source, &ramp, first as i64, n);
        pl.extend(l);
        pr.extend(r);
    }
    assert!(pl == wl && pr == wr, "pages differ from the whole");

    let mut jumpy = chunks(Preset::Full, 2);
    for (first, n) in [
        (250_000, 777),
        (10, 100_000),
        (200_000, 50_000),
        (95_000, 4_001),
        (3 * CHUNK as usize, 5_000),
        (1, 1),
        (CHUNK as usize - 3, 9),
    ] {
        let (l, r) = render(&mut jumpy, &mut source, &ramp, first as i64, n);
        assert!(
            l == wl[first..first + n] && r == wr[first..first + n],
            "{first}+{n}"
        );
    }
}

#[test]
fn a_copy_of_the_file_is_the_file() {
    let mut source = music();
    let mut c = chunks(Preset::Full, 2);
    let (l, r) = render(&mut c, &mut source, &|k| k as f64 + 1_000.0, 0, 20_000);
    assert!(l == source.planes[0][1_000..21_000] && r == source.planes[1][1_000..21_000]);
    let (l, _) = render(&mut c, &mut source, &|k| k as f64 * 1.001, 0, 20_000);
    assert!(l != source.planes[0][..20_000]);
}

#[test]
fn a_copy_of_the_file_is_the_file_across_every_seam() {
    let mut source = music();
    let total = 3 * CHUNK as usize + 5_000;
    let mut c = chunks(Preset::Full, 2);
    let (l, r) = render(&mut c, &mut source, &|k| k as f64 + 1_000.0, 0, total);
    assert!(l == source.planes[0][1_000..1_000 + total], "left");
    assert!(r == source.planes[1][1_000..1_000 + total], "right");
    let mut source = Mem {
        planes: vec![tone(900_000)],
    };
    let mut c = chunks(Preset::Light, 1);
    for first in (0..total).step_by(7_000) {
        let n = 7_000.min(total - first);
        let (l, _) = render(&mut c, &mut source, &|k| k as f64 - 500.0, first as i64, n);
        for (i, v) in l.iter().enumerate() {
            let want = (first + i)
                .checked_sub(500)
                .map_or(0.0, |f| source.planes[0][f]);
            assert!(*v == want, "frame {}", first + i);
        }
    }
}

#[test]
fn an_unchanged_span_beside_a_change_in_one_chunk_is_the_file() {
    let change = CHUNK + 40_000;
    let map = move |k: i64| {
        if k < change {
            k as f64 + 200.0
        } else {
            (change + 200) as f64 + (k - change) as f64 * 0.8
        }
    };
    let mut source = music();
    let total = 2 * CHUNK as usize;
    let mut whole = chunks(Preset::Full, 2);
    let (l, r) = render(&mut whole, &mut source, &map, 0, total);
    let exact = (change - FADE) as usize;
    assert!(
        l[..exact] == source.planes[0][200..200 + exact]
            && r[..exact] == source.planes[1][200..200 + exact],
        "the unchanged span is the file"
    );
    let after = change as usize + 20_000;
    assert!(l[after..after + 5_000] != source.planes[0][200 + after..200 + after + 5_000]);
    assert!(rms(&l[after..after + 5_000]) > 0.1, "and it sounds");
    let step = |a: usize, b: usize| {
        l[a..b]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max)
    };
    let (join, elsewhere) = (
        step(change as usize - 3_000, change as usize + 3_000),
        step(change as usize - 20_000, change as usize - 4_000),
    );
    assert!(join < elsewhere * 1.5 + 0.01, "{join} against {elsewhere}");
    let mut jumpy = chunks(Preset::Full, 2);
    for (first, n) in [
        (change as usize - 100, 5_000),
        (10, 4_096),
        (CHUNK as usize - 50, 100),
        (change as usize + 30_000, 9_999),
        (0, total),
    ] {
        let (pl, pr) = render(&mut jumpy, &mut source, &map, first as i64, n);
        assert!(
            pl == l[first..first + n] && pr == r[first..first + n],
            "{first}+{n}"
        );
    }
}

#[test]
fn the_seam_between_chunks_is_no_click() {
    let mut source = Mem {
        planes: vec![tone(900_000)],
    };
    let mut c = chunks(Preset::Full, 1);
    let at = CHUNK as usize;
    let (l, _) = render(
        &mut c,
        &mut source,
        &|k| k as f64 * 0.9,
        at as i64 - 2_000,
        4_000,
    );
    let step = |a: usize, b: usize| {
        l[a..b]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max)
    };
    let (seam, elsewhere) = (step(1_990, 2_010), step(100, 1_900));
    eprintln!("largest step at the seam {seam:.4}, elsewhere {elsewhere:.4}");
    assert!(seam < elsewhere * 1.5 + 0.01, "{seam} against {elsewhere}");
}

#[test]
fn silence_stays_silent_and_the_end_of_the_file_is_safe() {
    let mut quiet = Mem {
        planes: vec![vec![0.0; 100_000]],
    };
    let mut c = chunks(Preset::Light, 1);
    let (l, _) = render(&mut c, &mut quiet, &|k| k as f64 * 0.7, 0, 60_000);
    assert!(l.iter().all(|v| *v == 0.0));
    let mut short = Mem {
        planes: vec![tone(20_000)],
    };
    let (l, _) = render(
        &mut c,
        &mut short,
        &|k| k as f64 * 1.3 - 5_000.0,
        0,
        200_000,
    );
    assert!(l.iter().all(|v| v.is_finite()));
    assert!(rms(&l[100_000..]) < 1.0e-4, "the tail ends");
    let (l, _) = render(&mut c, &mut short, &|k| k as f64 * 2.0 + 1.0e9, 0, 10_000);
    assert!(l.iter().all(|v| *v == 0.0));
}

#[test]
fn a_copy_and_a_stretch_meet_without_a_click() {
    let mut source = Mem {
        planes: vec![tone(900_000)],
    };
    let seam = CHUNK;
    let map = move |k: i64| {
        if k < seam {
            k as f64
        } else {
            seam as f64 + (k - seam) as f64 * 0.9
        }
    };
    let mut c = chunks(Preset::Full, 1);
    let (l, _) = render(&mut c, &mut source, &map, seam - 3_000, 6_000);
    let step = |a: usize, b: usize| {
        l[a..b]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max)
    };
    let (at, elsewhere) = (step(2_000, 4_000), step(100, 1_800));
    eprintln!("largest step across the join {at:.4}, before it {elsewhere:.4}");
    assert!(at < elsewhere * 1.5 + 0.01, "{at} against {elsewhere}");
}

fn render_pitched(
    chunks: &mut Chunks,
    source: &mut Mem,
    (position, pitch): (&dyn Fn(i64) -> f64, &dyn Fn(i64) -> f64),
    first: i64,
    frames: usize,
) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    chunks
        .render(source, (position, Some(pitch)), first, &mut l, &mut r)
        .expect("renders");
    (l, r)
}

#[test]
fn a_pitched_tone_moves_its_partials_at_every_speed() {
    let fifth = 2.0_f64.powf(7.0 / 12.0);
    for speed in [1.0_f64, 0.9, 1.1] {
        let mut source = Mem {
            planes: vec![tone(1_000_000)],
        };
        let mut c = chunks(Preset::Full, 1);
        let (l, _) = render_pitched(
            &mut c,
            &mut source,
            (&|k| 5_000.0 + k as f64 * speed, &|_| 700.0),
            20_000,
            40_000,
        );
        let (up, high) = (amplitude(&l, 220.0 * fifth), amplitude(&l, 660.0 * fifth));
        let old = amplitude(&l, 220.0);
        eprintln!("speed {speed}: 329.6 Hz {up:.3}, 988.9 Hz {high:.3}, 220 Hz {old:.3}");
        assert!((up / 0.4 - 1.0).abs() < 0.15, "{speed}: {up}");
        assert!((high / 0.25 - 1.0).abs() < 0.15, "{speed}: {high}");
        assert!(old < 0.05, "{speed}: {old} left at the old pitch");
    }
}

#[test]
fn a_moving_pitch_gives_the_same_bits_however_it_is_asked() {
    let total = 2 * CHUNK as usize + 5_000;
    let pitch = |k: i64| 400.0 * (k as f64 / 40_000.0).sin() - 100.0;
    let (wl, wr) = render_pitched(
        &mut chunks(Preset::Full, 2),
        &mut music(),
        (&ramp, &pitch),
        0,
        total,
    );
    assert!(rms(&wl) > 0.1, "something was rendered");

    let mut pages = chunks(Preset::Full, 2);
    let mut source = music();
    let (mut pl, mut pr) = (Vec::new(), Vec::new());
    for first in (0..total).step_by(4096) {
        let n = 4096.min(total - first);
        let (l, r) = render_pitched(&mut pages, &mut source, (&ramp, &pitch), first as i64, n);
        pl.extend(l);
        pr.extend(r);
    }
    assert!(pl == wl && pr == wr, "pages differ from the whole");

    let mut used = chunks(Preset::Full, 2);
    render_pitched(&mut used, &mut source, (&ramp, &pitch), 150_000, 9_000);
    for (first, n) in [(120_000, 777), (10, 60_000), (CHUNK as usize - 3, 9)] {
        let (l, r) = render_pitched(&mut used, &mut source, (&ramp, &pitch), first as i64, n);
        assert!(
            l == wl[first..first + n] && r == wr[first..first + n],
            "{first}+{n}"
        );
    }
}

#[test]
fn a_pitched_stream_copies_the_file_only_where_it_is_not_shifted() {
    let mut source = music();
    let mut c = chunks(Preset::Full, 2);
    let pitch = |k: i64| if k < 30_000 { 0.0 } else { 600.0 };
    let (l, _) = render_pitched(
        &mut c,
        &mut source,
        (&|k| k as f64 + 1_000.0, &pitch),
        0,
        60_000,
    );
    assert!(
        l[2_000..27_000] == source.planes[0][3_000..28_000],
        "before the shift it is the file"
    );
    assert!(
        l[34_000..58_000] != source.planes[0][35_000..59_000],
        "after it is not"
    );
    let (l, _) = render_pitched(
        &mut chunks(Preset::Full, 2),
        &mut source,
        (&|k| k as f64 + 1_000.0, &|_| 100.0),
        0,
        20_000,
    );
    assert!(l != source.planes[0][1_000..21_000]);
}

#[test]
fn a_pitch_of_nothing_keeps_the_level_and_the_pitch() {
    for speed in [0.9_f64, 1.2] {
        let mut source = Mem {
            planes: vec![tone(1_000_000)],
        };
        let (l, _) = render_pitched(
            &mut chunks(Preset::Full, 1),
            &mut source,
            (&|k| 5_000.0 + k as f64 * speed, &|_| 0.0),
            20_000,
            40_000,
        );
        let (a220, a660) = (amplitude(&l, 220.0), amplitude(&l, 660.0));
        assert!((a220 / 0.4 - 1.0).abs() < 0.15, "{speed}: {a220}");
        assert!((a660 / 0.25 - 1.0).abs() < 0.15, "{speed}: {a660}");
    }
}

#[test]
fn an_octave_keeps_the_level_and_the_full_preset_keeps_more_of_the_tone() {
    let pure = |frames: usize| -> Vec<f32> {
        (0..frames)
            .map(|i| {
                (std::f64::consts::TAU * 960.0 * i as f64 / f64::from(RATE)).sin() as f32 * 0.5
            })
            .collect()
    };
    let share = |preset: Preset, cents: f64| {
        let mut source = Mem {
            planes: vec![pure(1_000_000)],
        };
        let (l, _) = render_pitched(
            &mut chunks(preset, 1),
            &mut source,
            (&|k| 5_000.0 + k as f64, &|_| cents),
            20_000,
            40_000,
        );
        let target = amplitude(&l, 960.0 * 2f64.powf(cents / 1_200.0)) / 2f64.sqrt();
        let level = rms(&l);
        assert!(
            (level / 0.3536 - 1.0).abs() < 0.12,
            "{preset:?} {cents}: level {level}"
        );
        target * target / (level * level)
    };
    for cents in [1_200.0, -1_200.0] {
        assert!(share(Preset::Full, cents) > 0.4, "Full at {cents}");
    }
    assert!(share(Preset::Full, -1_200.0) > share(Preset::Light, -1_200.0) * 3.0);
}
