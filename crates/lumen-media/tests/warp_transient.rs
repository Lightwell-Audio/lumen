//! Investigation F6 (MT-PITCH-1): a transient that seems to play twice when a
//! stretched clip has a non-zero transpose (the owner's `Phrase 120 BPM.wav`,
//! Melodic or Full mix, +2 st, near the start of measure 3).
//!
//! These tests measure; they do not assert an artefact. They are ignored so
//! that the suite does not depend on the owner's asset; run them with
//!
//! ```text
//! $env:LUMEN_PHRASE_WAV = "C:\lt\m95-owner-20261110\assets\Phrase 120 BPM.wav"
//! cargo test -p lumen-media --test warp_transient -- --ignored --nocapture
//! ```
//!
//! Without `LUMEN_PHRASE_WAV` a synthetic stand-in is used: a sustained tone
//! with a sharp onset every 0.5 s, the shape the phrase has at 120 BPM.

use lumen_media::MediaError;
use lumen_media::decode;
use lumen_media::detect::{Analysis, Detector};
use lumen_media::repitch::Source;
use lumen_media::stretch::{CHUNK, Chunks, FADE, Preset, Stretcher};

const RATE: u32 = 48_000;

/// A file in memory.
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

/// The owner's phrase when `LUMEN_PHRASE_WAV` names it, else a stand-in with
/// the same shape: a tone that starts sharply every 0.5 s (one beat at
/// 120 BPM), eight seconds long.
fn phrase() -> (Mem, &'static str) {
    if let Ok(path) = std::env::var("LUMEN_PHRASE_WAV") {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let audio = decode(std::path::Path::new(&path), &cancel, &mut |_| {}).unwrap();
        let rate = audio.sample_rate;
        let frames = audio.frames();
        let channels = audio.into_channels();
        eprintln!(
            "file: {path} ({rate} Hz, {} ch, {:.3} s)",
            channels.len(),
            frames as f64 / f64::from(rate)
        );
        assert_eq!(rate, RATE, "a stream plays at the device rate");
        return (Mem { planes: channels }, "phrase");
    }
    let frames = 8 * RATE as usize;
    let mut seed = 7_u32;
    let mut plane = |detune: f64| -> Vec<f32> {
        (0..frames)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                // A beat every 0.5 s: a short attack into a held vowel with the
                // phase restarted at each onset, so the attack is sharp.
                let phase = t - (t / 0.5).floor() * 0.5;
                let attack = (phase / 0.004).min(1.0);
                let decay = (-phase / 0.25).exp();
                let body = 0.45 * (std::f64::consts::TAU * 261.63 * detune * phase).sin()
                    + 0.2 * (std::f64::consts::TAU * 523.25 * detune * phase).sin()
                    + 0.1 * (std::f64::consts::TAU * 1_046.5 * detune * phase).sin();
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let breath = 0.004 * ((seed >> 9) as f32 / (1 << 23) as f32 - 0.5);
                (attack * decay * body) as f32 + breath
            })
            .collect()
    };
    (
        Mem {
            planes: vec![plane(1.0), plane(1.003)],
        },
        "synthetic",
    )
}

/// What `Chunks::render` asks for: the position, and the pitch when the
/// stream is shifted at all.
type Ask<'a> = (&'a dyn Fn(i64) -> f64, Option<&'a dyn Fn(i64) -> f64>);

/// Renders stream frames `first..first + frames` as the engine does for a clip
/// at its file's tempo in a song of that tempo: `position(k) = k`, and a pitch
/// function only when `cents` is given (a stream that is shifted at all).
fn render(
    chunks: &mut Chunks,
    source: &mut Mem,
    cents: Option<f64>,
    first: i64,
    frames: usize,
) -> Vec<f32> {
    let position = |k: i64| k as f64;
    let pitch = move |_: i64| cents.unwrap_or(0.0);
    let asked: Ask<'_> = match cents {
        Some(_) => (&position, Some(&pitch)),
        None => (&position, None),
    };
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    chunks
        .render(source, asked, first, &mut l, &mut r)
        .expect("renders");
    l
}

/// The stretcher driven over the whole file in one run, no chunk grid: what
/// the library alone makes of it.
fn direct(source: &mut Mem, cents: f64, total: usize, preset: Preset) -> Vec<f32> {
    let mut s = Stretcher::new(source.channels(), RATE, preset).expect("a stretcher");
    let channels = s.channels();
    let hop = s.interval() as i64;
    let lead = s.input_latency() as i64;
    let latency = s.output_latency() as i64;
    let mut read_at = latency + lead;
    let mut out: Vec<f32> = Vec::with_capacity(total + hop as usize);
    let mut a = 0_i64;
    let (mut input, mut spare) = (Vec::new(), Vec::new());
    let (mut hop_l, mut hop_r) = (vec![0.0_f32; hop as usize], vec![0.0_f32; hop as usize]);
    while out.len() < total {
        let next = a + hop + latency + lead;
        let n = (next - read_at).max(0) as usize;
        input.resize(n, 0.0);
        spare.resize(n, 0.0);
        let from = read_at.max(0) as u64;
        if channels >= 2 {
            source
                .read(from, &mut [&mut input[..], &mut spare[..]])
                .expect("reads");
        } else {
            source.read(from, &mut [&mut input[..]]).expect("reads");
        }
        s.set_transpose(cents).expect("told its shift");
        if channels >= 2 {
            let mut outputs: [&mut [f32]; 2] = [&mut hop_l, &mut hop_r];
            s.process(&[&input[..], &spare[..]], &mut outputs[..channels])
                .expect("processes");
        } else {
            let mut outputs: [&mut [f32]; 1] = [&mut hop_l];
            s.process(&[&input[..]], &mut outputs[..])
                .expect("processes");
        }
        out.extend_from_slice(&hop_l);
        read_at = read_at.max(next);
        a += hop;
    }
    out.truncate(total);
    out
}

/// The short-term level in dB, one value per `hop` frames.
fn envelope(x: &[f32], hop: usize) -> Vec<f32> {
    x.chunks(hop)
        .map(|h| {
            let energy: f64 = h.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let rms = (energy / h.len() as f64).sqrt();
            20.0 * (rms.max(1.0e-9)).log10() as f32
        })
        .collect()
}

/// Attacks in `x` between `from` and `to` seconds: the places the level rises
/// by more than `rise` dB over the 30 ms before them, as (seconds, dB).
fn attacks(x: &[f32], from: f64, to: f64, rise: f32) -> Vec<(f64, f32)> {
    let hop = RATE as usize / 200; // 5 ms
    let env = envelope(x, hop);
    let before = 6; // 30 ms
    let rate = RATE as f64 / hop as f64;
    let mut found: Vec<(f64, f32)> = Vec::new();
    let first = (from * rate) as usize;
    let last = ((to * rate) as usize).min(env.len());
    for i in first.max(before)..last {
        let rise_here = env[i] - env[i - before];
        if rise_here > rise && env[i] > -60.0 {
            // Keep the best of a rising run.
            if found
                .last()
                .is_none_or(|&(t, _)| (i as f64 / rate) - t > 0.02)
            {
                found.push((i as f64 / rate, rise_here));
            } else if let Some(last) = found.last_mut()
                && rise_here > last.1
            {
                *last = (i as f64 / rate, rise_here);
            }
        }
    }
    found
}

/// The transients detection holds, to the millisecond, in time order.
fn times(analysis: &Analysis) -> Vec<f64> {
    let mut at: Vec<f64> = analysis
        .transients_at(0)
        .map(|t| (t.seconds * 1000.0).round() / 1000.0)
        .collect();
    at.sort_by(f64::total_cmp);
    at
}

fn analyse(x: &[f32]) -> Analysis {
    let mut d = Detector::new(RATE);
    d.add(x, None);
    d.finish()
}

fn print_onsets(label: &str, source: &[f32], output: &[f32]) {
    let (w, g) = (times(&analyse(source)), times(&analyse(output)));
    eprintln!("{label}: source onsets {w:?}");
    eprintln!("{label}: output onsets {g:?}");
    let extra: Vec<f64> = g
        .iter()
        .copied()
        .filter(|t| !w.iter().any(|u| (t - u).abs() < 0.03))
        .collect();
    let lost: Vec<f64> = w
        .iter()
        .copied()
        .filter(|t| !g.iter().any(|u| (t - u).abs() < 0.03))
        .collect();
    eprintln!("{label}: extra {extra:?}, lost {lost:?}");
}

#[test]
#[ignore = "investigation: prints measurements for MT-PITCH-1, needs no pass/fail"]
fn onsets_at_a_transpose() {
    let (mut source, which) = phrase();
    let total = source.frames() as usize;
    eprintln!(
        "{which}: {} frames ({:.3} s), chunk {CHUNK} frames ({:.3} s), fade {FADE} frames ({:.3} s)",
        total,
        total as f64 / f64::from(RATE),
        CHUNK as f64 / f64::from(RATE),
        FADE as f64 / f64::from(RATE),
    );
    let seams: Vec<f64> = (1..)
        .map(|i| i * CHUNK)
        .take_while(|&k| k < total as i64)
        .map(|k| (k as f64 / f64::from(RATE) * 1000.0).round() / 1000.0)
        .collect();
    eprintln!("chunk seams at {seams:?}");
    let planes = source.planes[0].clone();
    for preset in [Preset::Full, Preset::Light] {
        for cents in [None, Some(200.0)] {
            let label = format!("{preset:?} {cents:?}");
            let mut c = Chunks::new(Stretcher::new(source.channels(), RATE, preset).unwrap());
            let out = render(&mut c, &mut source, cents, 0, total);
            print_onsets(&label, &planes, &out);
            eprintln!("{label}: attacks {:?}", attacks(&out, 0.0, 8.0, 1.5));
        }
    }
}

/// The seam at 4.096 s, frame by frame in level, against the file and against
/// the library's single run: the owner's spot.
/// The best repeat of the sound at `at` within `max_lag` frames, ignoring the
/// phrase's own 0.5 s repetition: (lag in ms, r).
fn best_repeat(x: &[f32], at: usize, len: usize, max_lag: usize) -> Vec<(f64, f64)> {
    let Some(window) = x.get(at..at + len) else {
        return Vec::new();
    };
    let energy: f64 = window.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
    if energy <= 0.0 {
        return Vec::new();
    }
    let mut found: Vec<(usize, f64)> = Vec::new();
    for lag in 240..=max_lag {
        let b = at + lag;
        if b + len > x.len() {
            break;
        }
        // Not the phrase's own beat.
        if (lag as f64 / f64::from(RATE) - (lag as f64 / 24_000.0).round() * 0.5).abs() < 0.025 {
            continue;
        }
        let mut dot = 0.0_f64;
        let mut other = 0.0_f64;
        for (i, &v) in window.iter().enumerate() {
            let w = f64::from(x[b + i]);
            dot += f64::from(v) * w;
            other += w * w;
        }
        if other > 0.0 {
            let r = dot / (energy * other).sqrt();
            if r > 0.5 {
                found.push((lag, r));
            }
        }
    }
    found.sort_by(|a, b| b.1.total_cmp(&a.1));
    found
        .into_iter()
        .take(4)
        .map(|(lag, r)| (lag as f64 * 1000.0 / f64::from(RATE), r))
        .collect()
}

/// The 50 ms level, in dB, one value each 10 ms from `from`.
fn levels(x: &[f32], from: f64, to: f64) -> Vec<f32> {
    let hop = RATE as usize / 100;
    let env = envelope(x, hop);
    let mut out = Vec::new();
    let mut i = (from * 100.0) as usize;
    let last = ((to * 100.0) as usize).min(env.len() - 5);
    while i < last {
        let set: f64 = env[i..i + 5]
            .iter()
            .map(|v| 10f64.powf(f64::from(*v) / 10.0))
            .sum();
        out.push((10.0 * (set / 5.0).log10()) as f32);
        i += 1;
    }
    out
}

/// A run as `chunks.rs` `Run` makes one: a stretcher reset at `start` (a hop
/// boundary), hopping forward, keeping the frames from `keep_from`.
fn run_from(
    source: &mut Mem,
    cents: f64,
    start: i64,
    keep_from: i64,
    limit: usize,
    preset: Preset,
) -> Vec<f32> {
    let mut s = Stretcher::new(source.channels(), RATE, preset).expect("a stretcher");
    let channels = s.channels();
    let hop = s.interval() as i64;
    let lead = s.input_latency() as i64;
    let latency = s.output_latency() as i64;
    let at = |k: i64| k as f64;
    let mut read_at = at(start + latency).round() as i64 + lead;
    let mut kept: Vec<f32> = Vec::with_capacity(limit);
    let (mut input, mut spare) = (Vec::new(), Vec::new());
    let (mut hop_l, mut hop_r) = (vec![0.0_f32; hop as usize], vec![0.0_f32; hop as usize]);
    let mut a = start;
    while kept.len() < limit {
        let next = at(a + hop + latency).round() as i64 + lead;
        let n = (next - read_at).max(0) as usize;
        input.resize(n, 0.0);
        spare.resize(n, 0.0);
        let from = read_at.max(0) as u64;
        if channels >= 2 {
            source
                .read(from, &mut [&mut input[..], &mut spare[..]])
                .expect("reads");
        } else {
            source.read(from, &mut [&mut input[..]]).expect("reads");
        }
        s.set_transpose(cents).expect("told its shift");
        if channels >= 2 {
            let mut outputs: [&mut [f32]; 2] = [&mut hop_l, &mut hop_r];
            s.process(&[&input[..], &spare[..]], &mut outputs[..channels])
                .expect("processes");
        } else {
            let mut outputs: [&mut [f32]; 1] = [&mut hop_l];
            s.process(&[&input[..]], &mut outputs[..])
                .expect("processes");
        }
        for (i, &v) in hop_l.iter().enumerate() {
            let frame = a + i as i64;
            if frame >= keep_from && kept.len() < limit {
                kept.push(v);
            }
        }
        read_at = read_at.max(next);
        a += hop;
    }
    kept
}

/// Where `chunks.rs` starts a chunk's run: the pre-roll it is given.
fn run_start(first: i64, preset: Preset) -> i64 {
    let s = Stretcher::new(1, RATE, preset).expect("a stretcher");
    let hop = s.interval() as i64;
    let before = s.block() as f64 * 1.5 + (2 * hop) as f64;
    let before = before as i64 + s.output_latency() as i64;
    let hops = (before + hop - 1) / hop;
    first - hops * hop
}

/// Whether the two renders the seam blends are the same sound in time: the
/// chunk's own last `FADE` frames against the fresh run the seam fades into.
#[test]
#[ignore = "investigation: prints measurements for MT-PITCH-1, needs no pass/fail"]
fn the_two_renders_a_seam_blends() {
    let (mut source, which) = phrase();
    let total = source.frames() as usize;
    let position = |k: i64| k as f64;
    let pitch = |_: i64| 200.0;
    for preset in [Preset::Full, Preset::Light] {
        for chunk in 1..=(total as i64 / CHUNK) {
            let seam = chunk * CHUNK;
            let zone = seam - FADE;
            // The chunk's own render of the faded frames, before the blend:
            // asked for only up to the zone, so no tail is computed.
            let mut c = Chunks::new(Stretcher::new(source.channels(), RATE, preset).unwrap());
            let n = 4_096;
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            c.render(
                &mut source,
                (&position, Some(&pitch)),
                zone - n as i64,
                &mut l,
                &mut r,
            )
            .unwrap();
            let own = &l[n - FADE as usize..];
            // The fresh run the seam fades into (what `tail` renders).
            let start = run_start(zone, preset);
            let into = run_from(&mut source, 200.0, start, zone, FADE as usize, preset);
            let energy =
                |x: &[f32]| -> f64 { x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum() };
            let dot = |a: &[f32], b: &[f32], lag: i64| -> f64 {
                let mut sum = 0.0;
                for (i, &v) in a.iter().enumerate() {
                    let j = i as i64 + lag;
                    if j >= 0 && (j as usize) < b.len() {
                        sum += f64::from(v) * f64::from(b[j as usize]);
                    }
                }
                sum
            };
            let norm = (energy(own) * energy(&into)).sqrt();
            let at_zero = dot(own, &into, 0) / norm;
            let mut best = (0_i64, f64::MIN);
            for lag in -2_400_i64..=2_400 {
                let d = dot(own, &into, lag) / norm;
                if d > best.1 {
                    best = (lag, d);
                }
            }
            eprintln!(
                "{which} {preset:?}: seam {:.3} s (chunk {chunk}): r at 0 lag {at_zero:.3}, best {:.3} at {:.1} ms ({} frames)",
                seam as f64 / f64::from(RATE),
                best.1,
                best.0 as f64 * 1000.0 / f64::from(RATE),
                best.0
            );
        }
    }
}

/// The syllable at the start of measure 3, 5 ms at a time, in the chunked
/// render and in the library's single run.
#[test]
#[ignore = "investigation: prints measurements for MT-PITCH-1, needs no pass/fail"]
fn the_syllable_at_measure_three() {
    let (mut source, which) = phrase();
    let total = source.frames() as usize;
    let planes = source.planes[0].clone();
    let mut mono = Mem {
        planes: vec![planes.clone()],
    };
    let hop = RATE as usize / 200; // 5 ms
    let file = envelope(&planes, hop);
    for preset in [Preset::Full, Preset::Light] {
        let mut c = Chunks::new(Stretcher::new(source.channels(), RATE, preset).unwrap());
        let chunked = envelope(&render(&mut c, &mut source, Some(200.0), 0, total), hop);
        let alone = envelope(&direct(&mut mono, 200.0, total, preset), hop);
        eprintln!("{which} {preset:?}");
        for i in (795..=840).step_by(1) {
            eprintln!(
                "  {:.3} s: file {:7.1} | chunked {:7.1} | single run {:7.1}",
                i as f64 * 0.005,
                file[i],
                chunked[i],
                alone[i]
            );
        }
    }
}

#[test]
#[ignore = "investigation: prints measurements for MT-PITCH-1, needs no pass/fail"]
fn the_seams_at_a_transpose() {
    let (mut source, which) = phrase();
    let total = source.frames() as usize;
    let planes = source.planes[0].clone();
    let mut mono = Mem {
        planes: vec![planes.clone()],
    };
    for preset in [Preset::Full, Preset::Light] {
        let mut c = Chunks::new(Stretcher::new(source.channels(), RATE, preset).unwrap());
        let chunked = render(&mut c, &mut source, Some(200.0), 0, total);
        let alone = direct(&mut mono, 200.0, total, preset);
        eprintln!(
            "{which} {preset:?}: attacks in the chunked render {:?}",
            attacks(&chunked, 0.0, 8.0, 1.5)
        );
        eprintln!(
            "{which} {preset:?}: attacks in the single run     {:?}",
            attacks(&alone, 0.0, 8.0, 1.5)
        );
        for seam in (1..)
            .map(|i| i * CHUNK)
            .take_while(|&k| k < total as i64 - CHUNK / 2)
        {
            let seconds = seam as f64 / f64::from(RATE);
            eprintln!("{which} {preset:?}: seam at {seconds:.3} s");
            // 50 ms levels, each 10 ms, so a cycle of the vowel averages out.
            let (a, b, f) = (
                levels(&chunked, seconds - 0.06, seconds + 0.06),
                levels(&alone, seconds - 0.06, seconds + 0.06),
                levels(&planes, seconds - 0.06, seconds + 0.06),
            );
            for i in 0..a.len() {
                eprintln!(
                    "  {:.3} s: file {:6.1} | chunked {:6.1} | single run {:6.1}",
                    seconds - 0.05 + i as f64 * 0.01,
                    f[i],
                    a[i],
                    b[i]
                );
            }
        }
        // Is anything near the seam played twice? Lags of 20 to 400 ms, the
        // phrase's own 0.5 s beat left out.
        for at in [3.95_f64, 4.0, 4.05, 4.06, 4.07, 4.08, 4.09, 4.1, 4.11, 4.15] {
            let frame = (at * f64::from(RATE)) as usize;
            let window = RATE as usize / 33; // 30 ms
            eprintln!(
                "{which} {preset:?}: repeats at {at:.2} s: chunked {:?} single {:?}",
                best_repeat(&chunked, frame, window, RATE as usize / 2),
                best_repeat(&alone, frame, window, RATE as usize / 2),
            );
        }
    }
}

/// The library alone, for the record: no chunk grid at all.
#[test]
#[ignore = "investigation: prints measurements for MT-PITCH-1, needs no pass/fail"]
fn the_library_alone_at_a_transpose() {
    let (source, which) = phrase();
    let total = source.frames() as usize;
    let planes = source.planes[0].clone();
    for preset in [Preset::Full, Preset::Light] {
        for cents in [0.0, 200.0] {
            let label = format!("direct {which} {preset:?} {cents}");
            let mut mono = Mem {
                planes: vec![planes.clone()],
            };
            let out = direct(&mut mono, cents, total, preset);
            print_onsets(&label, &planes, &out);
            eprintln!("{label}: attacks {:?}", attacks(&out, 0.0, 8.0, 1.5));
        }
    }
}
