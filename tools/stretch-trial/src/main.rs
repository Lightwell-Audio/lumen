//! The M9.5 S0 time-stretch trial (docs/milestones/M9.5.md, TS1 and E).
//!
//! `stretch-trial <sources dir> <out dir>` reads every 48 kHz stereo WAV in
//! the sources folder, renders each candidate on each transform, writes the
//! blind listening pack (`<out>/pack/<source>/<transform>/<letter>.wav`) and
//! prints the measurements as JSON (`<out>/results.json`).
//!
//! Every candidate is driven the way Lumen would drive it: streaming, a block
//! of input at a time, with its latency compensated so the output lines up
//! with the input. The same routine renders the "prepared ahead" chunks of
//! the plan's variant E A: each chunk is rendered by a fresh instance from a
//! fixed pre-roll of source before it, so a jump and straight play give the
//! same bits by construction; the chunks are joined with a 3 ms crossfade.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Allocation counting: only while `COUNTING` is set (the steady state of a
// streaming render), so setup allocations are not counted.

struct Counting;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const RATE: u32 = 48_000;

/// Stereo audio, one Vec per channel.
#[derive(Clone)]
struct Stereo {
    l: Vec<f32>,
    r: Vec<f32>,
}

impl Stereo {
    fn len(&self) -> usize {
        self.l.len()
    }
    fn zeros(n: usize) -> Self {
        Stereo {
            l: vec![0.0; n],
            r: vec![0.0; n],
        }
    }
    fn slice(&self, a: usize, b: usize) -> Self {
        Stereo {
            l: self.l[a..b].to_vec(),
            r: self.r[a..b].to_vec(),
        }
    }
    fn interleaved(&self) -> Vec<f32> {
        self.l
            .iter()
            .zip(&self.r)
            .flat_map(|(l, r)| [*l, *r])
            .collect()
    }
    fn from_interleaved(v: &[f32]) -> Self {
        Stereo {
            l: v.iter().step_by(2).copied().collect(),
            r: v.iter().skip(1).step_by(2).copied().collect(),
        }
    }
    /// Pads or cuts to exactly `n` frames.
    fn fit(mut self, n: usize) -> Self {
        self.l.resize(n, 0.0);
        self.r.resize(n, 0.0);
        self
    }
}

/// A transform: playback speed (2.0 is twice as fast) and pitch in semitones.
#[derive(Clone, Copy)]
struct Transform {
    name: &'static str,
    speed: f64,
    semitones: f64,
    /// On the listening page (the others are measured only).
    listen: bool,
}

const TRANSFORMS: &[Transform] = &[
    Transform {
        name: "tempo-0.75",
        speed: 0.75,
        semitones: 0.0,
        listen: true,
    },
    Transform {
        name: "tempo-0.9",
        speed: 0.9,
        semitones: 0.0,
        listen: false,
    },
    Transform {
        name: "tempo-1.1",
        speed: 1.1,
        semitones: 0.0,
        listen: false,
    },
    Transform {
        name: "tempo-1.5",
        speed: 1.5,
        semitones: 0.0,
        listen: true,
    },
    Transform {
        name: "pitch-minus-5",
        speed: 1.0,
        semitones: -5.0,
        listen: true,
    },
    Transform {
        name: "pitch-plus-3",
        speed: 1.0,
        semitones: 3.0,
        listen: true,
    },
];

/// What the trial knows of a candidate.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Candidate {
    Signalsmith,
    RubberBandFiner,
    RubberBandFaster,
    Bungee,
    TimestretchRs,
}

const CANDIDATES: &[Candidate] = &[
    Candidate::Signalsmith,
    Candidate::RubberBandFiner,
    Candidate::RubberBandFaster,
    Candidate::Bungee,
    Candidate::TimestretchRs,
];

impl Candidate {
    fn name(self) -> &'static str {
        match self {
            Candidate::Signalsmith => "signalsmith-stretch (default preset)",
            Candidate::RubberBandFiner => "rubberband (R3, finer, real-time mode)",
            Candidate::RubberBandFaster => "rubberband (R2, faster, real-time mode)",
            Candidate::Bungee => "bungee-rs (stream)",
            Candidate::TimestretchRs => "timestretch (pure Rust, whole-buffer render)",
        }
    }
    /// Whether the block size is ours to choose (streaming APIs).
    fn streams(self) -> bool {
        !matches!(self, Candidate::TimestretchRs)
    }
}

/// Measured during one streaming render.
#[derive(Default, Clone, Copy)]
struct RenderStats {
    /// Allocations counted while processing, after the first 8 blocks.
    steady_allocs: u64,
    /// The candidate's own latency, in input frames.
    latency_frames: usize,
}

/// Renders `input` through `candidate` at `speed` and `semitones`, feeding
/// `block` input frames at a time. The output is aligned with the input (the
/// candidate's latency removed) and is exactly `len / speed` frames long.
fn render(
    candidate: Candidate,
    input: &Stereo,
    speed: f64,
    semitones: f64,
    block: usize,
) -> (Stereo, RenderStats) {
    let want = (input.len() as f64 / speed).round() as usize;
    let pitch = 2f64.powf(semitones / 12.0);
    match candidate {
        Candidate::Signalsmith => signalsmith(input, speed, semitones, block, want),
        Candidate::RubberBandFiner => rubberband(input, speed, pitch, block, want, true),
        Candidate::RubberBandFaster => rubberband(input, speed, pitch, block, want, false),
        Candidate::Bungee => bungee(input, speed, pitch, block, want),
        Candidate::TimestretchRs => timestretch_render(input, speed, pitch, want),
    }
}

fn signalsmith(
    input: &Stereo,
    speed: f64,
    semitones: f64,
    block: usize,
    want: usize,
) -> (Stereo, RenderStats) {
    use signalsmith_stretch::Stretch;
    let mut s = Stretch::preset_default(2, RATE);
    s.set_transpose_factor_semitones(semitones as f32, None);
    let in_lat = s.input_latency();
    let out_lat = s.output_latency();
    // As the library's own `exact()`: the first `in_lat` frames go to `seek`,
    // which centres the stream on input frame 0; the rest of the input is
    // processed, then `in_lat` frames of silence so the end comes out; the
    // first `out_lat` output frames are dropped.
    let n = input.len();
    let lead = in_lat.min(n);
    let mut inter = input.interleaved();
    inter.extend(std::iter::repeat_n(0.0f32, lead * 2));
    s.seek(&inter[..lead * 2], speed);
    let mut out = Vec::with_capacity((want + out_lat + block) * 2);
    let mut pos = lead;
    let end = n + lead;
    let mut made = 0.0f64;
    let mut blocks = 0usize;
    let mut buf = vec![0.0f32; (block as f64 / speed).ceil() as usize * 2 + 8];
    ALLOCS.store(0, Ordering::Relaxed);
    while pos < end {
        let take = block.min(end - pos);
        made += take as f64 / speed;
        let give = made.round() as usize - out.len() / 2;
        let o = &mut buf[..give * 2];
        COUNTING.store(blocks >= 8 && pos + take <= n, Ordering::Relaxed);
        s.process(&inter[pos * 2..(pos + take) * 2], &mut *o);
        COUNTING.store(false, Ordering::Relaxed);
        out.extend_from_slice(o);
        pos += take;
        blocks += 1;
    }
    let steady = ALLOCS.load(Ordering::Relaxed);
    let mut tail = vec![0.0f32; out_lat * 2];
    s.flush(&mut tail);
    out.extend_from_slice(&tail);
    let aligned = out[(out_lat * 2).min(out.len())..].to_vec();
    (
        Stereo::from_interleaved(&aligned).fit(want),
        RenderStats {
            steady_allocs: steady,
            latency_frames: in_lat + out_lat,
        },
    )
}

fn rubberband(
    input: &Stereo,
    speed: f64,
    pitch: f64,
    block: usize,
    want: usize,
    finer: bool,
) -> (Stereo, RenderStats) {
    use rubberband::{Options, Stretcher};
    let engine = if finer {
        Options::ENGINE_FINER
    } else {
        Options::ENGINE_FASTER
    };
    let opts = Options::PROCESS_REALTIME
        | engine
        | Options::THREADING_NEVER
        | Options::PITCH_HIGH_CONSISTENCY;
    let mut s = Stretcher::new(RATE, 2, opts, 1.0 / speed, pitch);
    s.set_max_process_size(block as u32);
    let pad = s.preferred_start_pad() as usize;
    let delay = s.start_delay() as usize;
    let n = input.len();
    let zeros = vec![0.0f32; pad.max(1)];
    if pad > 0 {
        s.process(&[&zeros[..pad], &zeros[..pad]], false);
    }
    let mut l = Vec::with_capacity(want + delay + block);
    let mut r = Vec::with_capacity(want + delay + block);
    let mut ol = vec![0.0f32; 1 << 16];
    let mut or = vec![0.0f32; 1 << 16];
    let mut pos = 0;
    let mut blocks = 0usize;
    ALLOCS.store(0, Ordering::Relaxed);
    let mut drain = |s: &mut Stretcher, l: &mut Vec<f32>, r: &mut Vec<f32>| {
        while let Some(av) = s.available() {
            if av == 0 {
                break;
            }
            let k = (av as usize).min(ol.len());
            let got = s.retrieve(&mut [&mut ol[..k], &mut or[..k]]) as usize;
            l.extend_from_slice(&ol[..got]);
            r.extend_from_slice(&or[..got]);
        }
    };
    while pos < n {
        let take = block.min(n - pos);
        let last = pos + take >= n;
        COUNTING.store(blocks >= 8, Ordering::Relaxed);
        s.process(
            &[&input.l[pos..pos + take], &input.r[pos..pos + take]],
            last,
        );
        COUNTING.store(false, Ordering::Relaxed);
        drain(&mut s, &mut l, &mut r);
        pos += take;
        blocks += 1;
    }
    // The `rubberband` wrapper collects the channel pointers into a Vec on
    // every `process` call: one allocation per counted call is the wrapper's,
    // not the library's, so it is taken off.
    let counted_calls = blocks.saturating_sub(8) as u64;
    let steady = ALLOCS.load(Ordering::Relaxed).saturating_sub(counted_calls);
    drain(&mut s, &mut l, &mut r);
    let skip = delay.min(l.len());
    let out = Stereo {
        l: l[skip..].to_vec(),
        r: r[skip..].to_vec(),
    };
    (
        out.fit(want),
        RenderStats {
            steady_allocs: steady,
            latency_frames: delay,
        },
    )
}

fn bungee(
    input: &Stereo,
    speed: f64,
    pitch: f64,
    block: usize,
    want: usize,
) -> (Stereo, RenderStats) {
    use bungee_rs::Stream;
    let n = input.len();
    let mut s = Stream::new(RATE as usize, 2, block).expect("bungee stream");
    // The stream's `latency()` reads its last output grain, so it is asked
    // for once a few blocks have been processed (it is undefined before).
    // Until then a generous bound feeds enough silence at the end.
    let mut latency_in = 0.0f64;
    let bound = RATE as usize / 2;
    let cap = (block as f64 / speed).ceil() as usize + 8;
    let mut ob = vec![vec![0.0f32; cap]; 2];
    let mut ib = vec![vec![0.0f32; block]; 2];
    let mut l = Vec::with_capacity(want + bound + cap);
    let mut r = Vec::with_capacity(want + bound + cap);
    let mut pos = 0;
    let mut blocks = 0usize;
    let mut made = 0.0f64;
    ALLOCS.store(0, Ordering::Relaxed);
    // Feed the input, then enough silence to push the latency out.
    let total = n + bound + block;
    while pos < total {
        let take = block.min(total - pos);
        for (c, src) in [&input.l, &input.r].into_iter().enumerate() {
            for i in 0..take {
                ib[c][i] = if pos + i < n { src[pos + i] } else { 0.0 };
            }
        }
        made += take as f64 / speed;
        let target = made - l.len() as f64;
        COUNTING.store(blocks >= 8 && pos + take <= n, Ordering::Relaxed);
        let got = s.process(Some(&ib[..]), &mut ob, take, target.max(1.0), pitch);
        COUNTING.store(false, Ordering::Relaxed);
        l.extend_from_slice(&ob[0][..got]);
        r.extend_from_slice(&ob[1][..got]);
        pos += take;
        blocks += 1;
        if blocks == 8 {
            latency_in = s.latency().max(0.0);
        }
    }
    let steady = ALLOCS.load(Ordering::Relaxed);
    let latency_out = (latency_in / speed).round() as usize;
    let skip = latency_out.min(l.len());
    let out = Stereo {
        l: l[skip..].to_vec(),
        r: r[skip..].to_vec(),
    };
    (
        out.fit(want),
        RenderStats {
            steady_allocs: steady,
            latency_frames: latency_in.round() as usize,
        },
    )
}

fn timestretch_render(
    input: &Stereo,
    speed: f64,
    pitch: f64,
    want: usize,
) -> (Stereo, RenderStats) {
    use timestretch::{Backend, Parameters, PitchRatio, SessionConfig, TempoRate};
    let params = Parameters {
        tempo: TempoRate::new(speed).expect("tempo"),
        pitch: PitchRatio::new(pitch).expect("pitch"),
        ..Parameters::default()
    };
    let config = SessionConfig {
        source_sample_rate: RATE,
        output_sample_rate: RATE,
        channels: 2,
        backend: Backend::Portable,
        parameters: params,
        ..SessionConfig::default()
    };
    let out = timestretch::render(&input.interleaved(), config).expect("timestretch render");
    (
        Stereo::from_interleaved(&out.samples).fit(want),
        RenderStats::default(),
    )
}

/// The plan's E A: output chunks of `chunk` frames, each rendered by a fresh
/// instance from `preroll` source frames before it (and `postroll` after, so
/// its end is complete), then joined with a 3 ms equal-power crossfade.
fn render_chunked(
    c: Candidate,
    input: &Stereo,
    t: Transform,
    chunk_out: usize,
    preroll: usize,
) -> Stereo {
    let n = input.len();
    let want = (n as f64 / t.speed).round() as usize;
    let fade = (RATE as f64 * 0.003) as usize;
    let chunk_in = (chunk_out as f64 * t.speed).round() as usize;
    let post = preroll / 2 + fade * 2;
    let mut out = Stereo::zeros(want);
    let mut start = 0usize;
    while start < n {
        let a = start.saturating_sub(preroll);
        let b = (start + chunk_in + post).min(n);
        let piece = input.slice(a, b);
        let (rendered, _) = render(c, &piece, t.speed, t.semitones, 1024);
        let head = ((start - a) as f64 / t.speed).round() as usize;
        let at = (start as f64 / t.speed).round() as usize;
        let len =
            ((chunk_in as f64 / t.speed).round() as usize + fade).min(want.saturating_sub(at));
        for i in 0..len {
            let src = head + i;
            if src >= rendered.len() {
                break;
            }
            // Fade in over the first 3 ms (except the first chunk), fade the
            // previous chunk's overhang out over the same frames.
            let g_in = if start == 0 || i >= fade {
                1.0
            } else {
                ((i as f32 + 0.5) / fade as f32 * std::f32::consts::FRAC_PI_2).sin()
            };
            let g_out = if start == 0 || i >= fade {
                0.0
            } else {
                ((i as f32 + 0.5) / fade as f32 * std::f32::consts::FRAC_PI_2).cos()
            };
            out.l[at + i] = out.l[at + i] * g_out + rendered.l[src] * g_in;
            out.r[at + i] = out.r[at + i] * g_out + rendered.r[src] * g_in;
        }
        start += chunk_in;
    }
    out
}

fn read_wav(path: &Path) -> Stereo {
    let mut r = hound::WavReader::open(path).expect("wav");
    let spec = r.spec();
    assert_eq!(spec.sample_rate, RATE, "{} must be 48 kHz", path.display());
    assert_eq!(spec.channels, 2, "{} must be stereo", path.display());
    let v: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.unwrap() as f32 * scale)
                .collect()
        }
    };
    Stereo::from_interleaved(&v)
}

fn write_wav(path: &Path, s: &Stereo) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for (l, r) in s.l.iter().zip(&s.r) {
        w.write_sample(*l).unwrap();
        w.write_sample(*r).unwrap();
    }
    w.finalize().unwrap();
}

fn same_bits(a: &Stereo, b: &Stereo) -> bool {
    a.l.iter()
        .zip(&b.l)
        .all(|(x, y)| x.to_bits() == y.to_bits())
        && a.r
            .iter()
            .zip(&b.r)
            .all(|(x, y)| x.to_bits() == y.to_bits())
        && a.len() == b.len()
}

fn max_diff(a: &Stereo, b: &Stereo) -> f32 {
    a.l.iter()
        .zip(&b.l)
        .chain(a.r.iter().zip(&b.r))
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// The listening letter of each candidate: fixed across the whole pack so a
/// favourite can be followed from row to row, and not in the order above.
fn letter(c: Candidate) -> char {
    match c {
        Candidate::Bungee => 'A',
        Candidate::RubberBandFaster => 'B',
        Candidate::Signalsmith => 'C',
        Candidate::TimestretchRs => 'D',
        Candidate::RubberBandFiner => 'E',
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (src_dir, out_dir) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let mut sources: Vec<PathBuf> = std::fs::read_dir(&src_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "wav"))
        .collect();
    sources.sort();
    let mut results = Vec::new();
    for src in &sources {
        let name = src.file_stem().unwrap().to_string_lossy().to_string();
        let input = read_wav(src);
        let seconds = input.len() as f64 / RATE as f64;
        eprintln!("{name}: {seconds:.2} s");
        write_wav(
            &out_dir.join("pack").join(&name).join("original.wav"),
            &input,
        );
        for t in TRANSFORMS {
            for &c in CANDIDATES {
                // CPU: the best of three renders at a 512-frame block.
                let mut best = f64::MAX;
                let mut first = None;
                let mut stats = RenderStats::default();
                for _ in 0..3 {
                    let t0 = Instant::now();
                    let (o, st) = render(c, &input, t.speed, t.semitones, 512);
                    best = best.min(t0.elapsed().as_secs_f64());
                    stats = st;
                    if first.is_none() {
                        first = Some(o);
                    }
                }
                let out = first.unwrap();
                let (again, _) = render(c, &input, t.speed, t.semitones, 512);
                let repeats = same_bits(&out, &again);
                let (block_info, block_same) = if c.streams() {
                    let (b64, _) = render(c, &input, t.speed, t.semitones, 64);
                    let (b4k, _) = render(c, &input, t.speed, t.semitones, 4096);
                    (
                        format!("{:.2e}", max_diff(&b64, &b4k)),
                        same_bits(&b64, &b4k),
                    )
                } else {
                    ("not measured: whole-buffer API".to_string(), false)
                };
                // The click-to-sound cost under E A: one 4,096-frame chunk from
                // a 16,384-frame pre-roll, on one core.
                let mid = input.len() / 2;
                let piece = input.slice(
                    mid - 16_384,
                    (mid + (4096.0 * t.speed) as usize + 4096).min(input.len()),
                );
                let t0 = Instant::now();
                let _ = render(c, &piece, t.speed, t.semitones, 1024);
                let chunk_ms = t0.elapsed().as_secs_f64() * 1000.0;
                let independent = if c.streams() {
                    block_same.to_string()
                } else {
                    "n/a".to_string()
                };
                eprintln!(
                    "  {:14} {:44} {:6.1} ms/s  first chunk {:6.1} ms  allocs {:5}  repeat {repeats}  block-independent {independent}",
                    t.name,
                    c.name(),
                    best / seconds * 1000.0,
                    chunk_ms,
                    stats.steady_allocs
                );
                if t.listen {
                    write_wav(
                        &out_dir
                            .join("pack")
                            .join(&name)
                            .join(t.name)
                            .join(format!("{}.wav", letter(c))),
                        &out,
                    );
                }
                results.push(format!(
                    "{{\"source\":\"{name}\",\"transform\":\"{}\",\"candidate\":\"{}\",\"letter\":\"{}\",\"ms_per_second\":{:.2},\"first_chunk_ms\":{:.2},\"latency_ms\":{:.1},\"steady_allocs\":{},\"repeats_bit_for_bit\":{repeats},\"block_size_independent\":{block_same},\"max_diff_block_64_vs_4096\":\"{block_info}\"}}",
                    t.name, c.name(), letter(c), best / seconds * 1000.0, chunk_ms, stats.latency_frames as f64 * 1000.0 / RATE as f64, stats.steady_allocs
                ));
            }
        }
        // The prepared-ahead chunks (E A), for listening: 0.9 speed, 4,096-frame chunks, 16,384-frame pre-roll.
        let t = TRANSFORMS[1];
        for &c in CANDIDATES {
            let chunked = render_chunked(c, &input, t, 4096, 16_384);
            write_wav(
                &out_dir
                    .join("pack")
                    .join(&name)
                    .join("chunked-tempo-0.9")
                    .join(format!("{}.wav", letter(c))),
                &chunked,
            );
            let (whole, _) = render(c, &input, t.speed, t.semitones, 1024);
            write_wav(
                &out_dir
                    .join("pack")
                    .join(&name)
                    .join("whole-tempo-0.9")
                    .join(format!("{}.wav", letter(c))),
                &whole,
            );
        }
    }
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(
        out_dir.join("results.json"),
        format!("[\n{}\n]\n", results.join(",\n")),
    )
    .unwrap();
}
