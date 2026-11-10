//! A stretched stream rendered in chunks (M9.5 S4c, ADR-0132).
//!
//! A stretcher has state: what it plays now depends on what it heard before.
//! A stream must not: the same frames come out whether they are asked for in
//! pages of 4,096 from the start, from a jump into the middle, by playback or
//! by an export. So the stream is cut at a fixed grid of [`CHUNK`] frames, and
//! each chunk is rendered from nothing: a stretcher reset to its start, fed the
//! file from a *pre-roll* ahead of the chunk (long enough that the part of the
//! chunk kept no longer depends on where the pre-roll started), the pre-roll's
//! output thrown away. A chunk's frames depend on the chunk, so any way of
//! asking for them gives the same bits.
//!
//! **The seams.** Two stretchers started apart do not agree on the phase of a
//! steady tone, so chunks laid end to end would click at every seam. The end
//! of each chunk is therefore crossfaded, over [`FADE`] frames, into the next
//! chunk's render of the same frames (which the next chunk has anyway: it
//! keeps its run from [`FADE`] before its own start). A span at the file's own
//! speed is a plain copy of the file, not stretched (D3), wherever it is: a
//! chunk wholly at the file's speed is never stretched, and in a chunk that
//! also holds a change of speed, the frames well inside the unchanged span are
//! the file bit for bit, [`FADE`] frames of each edge of the span blending
//! into the stretch. A copy and a stretch are crossfaded at a seam in the same
//! way.

use super::Stretcher;
use crate::MediaError;
use crate::repitch::{GRID, OfFrame, Source};

/// Frames in a chunk: 24 pages of 4,096, a little over two seconds at 48 kHz.
pub const CHUNK: i64 = 98_304;

/// Frames over which the end of a chunk fades into the start of the next.
pub const FADE: i64 = 2_048;

/// The slowest and fastest a span of a clip plays against the file: a quarter
/// of the speed to four times it (the limits of a warp span).
const SLOWEST: f64 = 0.25;
const FASTEST: f64 = 4.0;

/// Within this of a whole frame, at unity speed, is a copy of the file.
const SNAP: f64 = 1.0e-7;

/// Frames of the file read at a time.
const WINDOW: i64 = 32_768;

/// Renders the frames of a stretched stream. Holds the stretchers and the
/// chunk it last rendered, so make one per worker and read it forwards.
pub struct Chunks {
    main: Stretcher,
    /// The stretcher the end of a chunk is faded into, made when first needed.
    aux: Option<Stretcher>,
    window: Window,
    aux_window: Window,
    /// The file as read for the copied spans (apart from the stretcher's).
    copy_window: Window,
    session: Option<Session>,
    /// The hop being rendered.
    hop_out: [Vec<f32>; 2],
}

/// The chunk being rendered, as far as it has got.
struct Session {
    chunk: i64,
    /// A plain copy of the file at this whole-frame offset, when the chunk
    /// plays at the file's own speed throughout.
    copy: Option<i64>,
    /// The spans of the stream (around the chunk) that play the file as it
    /// was made.
    unity: Unity,
    /// The stretched render of the chunk, when it is not a copy.
    run: Option<Run>,
    /// What the next chunk plays over the last [`FADE`] frames of this one.
    tail: Option<[Vec<f32>; 2]>,
}

/// A stretcher run from a reset over a span of the stream.
struct Run {
    /// The stream frame `kept` starts at.
    first: i64,
    /// The stream frame the first hop starts at: `first` less the pre-roll.
    start: i64,
    /// The hop to render next.
    hop: i64,
    /// The file frame the next hop starts reading at: where the hop before it
    /// stopped.
    read_at: i64,
    /// The most frames kept.
    limit: usize,
    kept: [Vec<f32>; 2],
}

impl Chunks {
    pub fn new(stretcher: Stretcher) -> Self {
        let hop = stretcher.interval();
        Self {
            main: stretcher,
            aux: None,
            window: Window::default(),
            aux_window: Window::default(),
            copy_window: Window::default(),
            session: None,
            hop_out: [vec![0.0; hop], vec![0.0; hop]],
        }
    }

    /// Renders stream frames `first..first + left.len()` into `left` and, for
    /// a file of two channels, `right` (both one length; a mono file leaves
    /// `right` alone). `position(k)` is the frame of the file, fractional,
    /// that plays at stream frame `k`; it must not depend on anything but `k`.
    /// `pitch`, when there is one, gives the shift of the pitch in cents at
    /// stream frame `k` (also a function of `k` alone): a stream with one is
    /// shifted at every hop, and copies no part of the file but the frames
    /// where the shift is nothing.
    pub fn render(
        &mut self,
        source: &mut dyn Source,
        (position, pitch): (OfFrame<'_>, Option<OfFrame<'_>>),
        first: i64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), MediaError> {
        let n = left.len();
        let stereo = self.main.channels() >= 2;
        left.fill(0.0);
        if stereo {
            right[..n].fill(0.0);
        }
        // The stream starts at frame 0: nothing before it is asked for.
        let mut k = first.max(0);
        let end = first + n as i64;
        while k < end {
            let chunk = k.div_euclid(CHUNK);
            let stop = end.min((chunk + 1) * CHUNK);
            let (a, b) = ((k - first) as usize, (stop - first) as usize);
            let r = if stereo { Some(&mut right[a..b]) } else { None };
            self.fill(source, (position, pitch), chunk, k, &mut left[a..b], r)?;
            k = stop;
        }
        Ok(())
    }

    /// Frames `from..from + left.len()`, all in `chunk`.
    fn fill(
        &mut self,
        source: &mut dyn Source,
        (position, pitch): (OfFrame<'_>, Option<OfFrame<'_>>),
        chunk: i64,
        from: i64,
        left: &mut [f32],
        mut right: Option<&mut [f32]>,
    ) -> Result<(), MediaError> {
        let hop = self.main.interval() as i64;
        let at = Positions::new(position, pitch, hop);
        if self.session.as_ref().map(|s| s.chunk) != Some(chunk) {
            self.begin(&at, chunk)?;
        }
        let len = left.len();
        let session = self.session.as_mut().expect("a chunk is begun");
        // The chunk's own version of the frames.
        if let Some(offset) = session.copy {
            let planes = self
                .window
                .get(source, from + offset, from + offset + len as i64)?;
            left.copy_from_slice(planes[0]);
            if let Some(right) = right.as_deref_mut() {
                right.copy_from_slice(planes[1]);
            }
        } else if let Some(run) = session.run.as_mut() {
            let wanted = (from - run.first) as usize + len;
            run.extend(
                &mut self.main,
                &mut self.window,
                source,
                &at,
                &mut self.hop_out,
                wanted,
            )?;
            let a = (from - run.first) as usize;
            left.copy_from_slice(&run.kept[0][a..a + len]);
            if let Some(right) = right.as_deref_mut() {
                right.copy_from_slice(&run.kept[1][a..a + len]);
            }
            // The spans at the file's own speed are the file.
            unity_over(
                &session.unity,
                &mut self.copy_window,
                source,
                from,
                left,
                right.as_deref_mut(),
            )?;
        }
        // The end of the chunk fades into the next one's.
        let zone = (chunk + 1) * CHUNK - FADE;
        if from + len as i64 > zone {
            if session.tail.is_none() {
                let tail = tail(
                    &mut self.aux,
                    &mut self.aux_window,
                    &mut self.copy_window,
                    &self.main,
                    source,
                    &at,
                    chunk + 1,
                )?;
                session.tail = Some(tail);
            }
            let tail = session.tail.as_ref().expect("made above");
            for i in 0..len {
                let frame = from + i as i64;
                if frame < zone {
                    continue;
                }
                let t = (frame - zone) as f32 + 0.5;
                let into = 0.5 - 0.5 * (std::f32::consts::PI * t / FADE as f32).cos();
                let j = (frame - zone) as usize;
                left[i] += (tail[0][j] - left[i]) * into;
                if let Some(right) = right.as_deref_mut() {
                    right[i] += (tail[1][j] - right[i]) * into;
                }
            }
        }
        Ok(())
    }

    /// Starts `chunk`: a plain copy when the whole of it plays at the file's
    /// own speed, otherwise a fresh run.
    fn begin(&mut self, at: &Positions<'_>, chunk: i64) -> Result<(), MediaError> {
        let first = chunk * CHUNK;
        let unity = Unity::scan(at, first - FADE, first + CHUNK);
        let (copy, run) = match unity.whole(first - FADE, first + CHUNK) {
            Some(offset) => (Some(offset), None),
            None => (
                None,
                Some(Run::new(
                    &mut self.main,
                    at,
                    first - FADE,
                    (CHUNK + FADE) as usize,
                )?),
            ),
        };
        self.session = Some(Session {
            chunk,
            copy,
            unity,
            run,
            tail: None,
        });
        Ok(())
    }
}

/// What `chunk` plays over its first [`FADE`] frames: the other end of the
/// seam before it.
fn tail(
    aux: &mut Option<Stretcher>,
    window: &mut Window,
    copy_window: &mut Window,
    like: &Stretcher,
    source: &mut dyn Source,
    at: &Positions<'_>,
    chunk: i64,
) -> Result<[Vec<f32>; 2], MediaError> {
    let first = chunk * CHUNK;
    let stereo = like.channels() >= 2;
    // The same spans the chunk will find for itself when it is begun.
    let unity = Unity::scan(at, first - FADE, first + CHUNK);
    if let Some(offset) = unity.whole(first - FADE, first + CHUNK) {
        // The chunk is a plain copy: so are the frames before it that it
        // would have played, the ones this seam is crossfaded into.
        let from = first - FADE + offset;
        let planes = window.get(source, from, from + FADE)?;
        return Ok([
            planes[0].to_vec(),
            if stereo {
                planes[1].to_vec()
            } else {
                Vec::new()
            },
        ]);
    }
    if aux.is_none() {
        *aux = Some(Stretcher::new(like.channels(), like.rate(), like.preset())?);
    }
    let stretcher = aux.as_mut().expect("made above");
    let mut run = Run::new(stretcher, at, first - FADE, FADE as usize)?;
    let mut out = [vec![0.0; like.interval()], vec![0.0; like.interval()]];
    run.extend(stretcher, window, source, at, &mut out, FADE as usize)?;
    let [left, right] = &mut run.kept;
    unity_over(
        &unity,
        copy_window,
        source,
        first - FADE,
        left,
        stereo.then_some(&mut right[..]),
    )?;
    Ok(run.kept)
}

impl Run {
    /// A run over `first..first + limit` of the stream, `stretcher` reset to
    /// start it a pre-roll ahead.
    fn new(
        stretcher: &mut Stretcher,
        at: &Positions<'_>,
        first: i64,
        limit: usize,
    ) -> Result<Self, MediaError> {
        let hop = stretcher.interval() as i64;
        // How far back the pre-roll goes: the block, the hops the stretcher
        // needs to settle, its latency, and the file's history a slow span
        // needs for the same output (a block of output reads more of the file
        // when the file runs fast, less when slow, so use the speed here).
        let speed = ((at.get(first + hop) - at.get(first)) / hop as f64).clamp(SLOWEST, FASTEST);
        let before = stretcher.block() as f64 * (1.0 + 0.5 / speed) + (2 * hop) as f64;
        let before = before as i64 + stretcher.output_latency() as i64;
        let hops = (before + hop - 1) / hop;
        let start = first - hops * hop;
        let read_at = round(at.get(start + stretcher.output_latency() as i64))
            + stretcher.input_latency() as i64;
        stretcher.reset()?;
        Ok(Self {
            first,
            start,
            hop: 0,
            read_at,
            limit,
            kept: [Vec::new(), Vec::new()],
        })
    }

    /// Renders hops until `wanted` frames (at most the limit) are kept.
    fn extend(
        &mut self,
        stretcher: &mut Stretcher,
        window: &mut Window,
        source: &mut dyn Source,
        at: &Positions<'_>,
        out: &mut [Vec<f32>; 2],
        wanted: usize,
    ) -> Result<(), MediaError> {
        let channels = stretcher.channels();
        let hop = stretcher.interval() as i64;
        let lead = stretcher.input_latency() as i64;
        let latency = stretcher.output_latency() as i64;
        let wanted = wanted.min(self.limit);
        while self.kept[0].len() < wanted {
            let a = self.start + self.hop * hop;
            // The file frames this hop is fed: from where the last one
            // stopped to where the file is one hop on, ahead by the
            // stretcher's input latency (and its output latency's worth of
            // the stream).
            let next = round(at.get(a + hop + latency)) + lead;
            let n = (next - self.read_at).max(0);
            // A pitched stream tells the stretcher its shift for every hop,
            // the pre-roll's too, so what it plays never depends on what it
            // was told before (a chunk starts from a reset, not a new one).
            if let Some(cents) = at.cents(a + hop / 2) {
                stretcher.set_transpose(cents)?;
            }
            let planes = window.get(source, self.read_at, self.read_at + n)?;
            let input = [planes[0], planes[1]];
            let [o0, o1] = out;
            let mut output = [&mut o0[..], &mut o1[..]];
            stretcher.process(&input[..channels], &mut output[..channels])?;
            self.read_at = self.read_at.max(next);
            self.hop += 1;
            // Keep the part of the hop that is in the span.
            for (heard, kept) in out.iter().zip(&mut self.kept).take(channels) {
                for (i, &v) in heard.iter().enumerate() {
                    let frame = a + i as i64;
                    if frame >= self.first && kept.len() < self.limit {
                        kept.push(v);
                    }
                }
            }
        }
        Ok(())
    }
}

fn round(position: f64) -> i64 {
    if position.is_finite() {
        position.round() as i64
    } else {
        0
    }
}

/// The position function, extended below frame 0 along the line the stream
/// starts on, so the pre-roll of the first chunk reads the file as if the
/// stream ran on backwards.
struct Positions<'p> {
    position: &'p dyn Fn(i64) -> f64,
    pitch: Option<&'p dyn Fn(i64) -> f64>,
    hop: i64,
}

/// The most the pitch is shifted either way, in cents: four octaves.
const MOST_CENTS: f64 = 4_800.0;

/// A shift this small, in cents, is none: the frame is the file's own pitch.
const NO_SHIFT: f64 = 0.005;

impl<'p> Positions<'p> {
    fn new(
        position: &'p dyn Fn(i64) -> f64,
        pitch: Option<&'p dyn Fn(i64) -> f64>,
        hop: i64,
    ) -> Self {
        Self {
            position,
            pitch,
            hop,
        }
    }

    /// The shift in cents at stream frame `k`, held from frame 0 below it, and
    /// `None` for a stream with no pitch at all.
    fn cents(&self, k: i64) -> Option<f64> {
        let cents = (self.pitch?)(k.max(0));
        Some(if cents.is_finite() {
            cents.clamp(-MOST_CENTS, MOST_CENTS)
        } else {
            0.0
        })
    }

    /// Whether the file plays at its own pitch at frame `k`.
    fn unshifted(&self, k: i64) -> bool {
        self.cents(k).is_none_or(|c| c.abs() < NO_SHIFT)
    }

    fn get(&self, k: i64) -> f64 {
        if k >= 0 {
            return (self.position)(k);
        }
        let (p0, p1) = ((self.position)(0), (self.position)(self.hop));
        p0 + k as f64 * (p1 - p0) / self.hop as f64
    }
}

/// A stretch of the stream that plays the file as it was made.
#[derive(Clone, Copy)]
struct Span {
    /// The whole number of frames from a stream frame to its file frame.
    offset: i64,
    /// The stream frames `from..to`, the whole of the unchanged run this
    /// frame is in.
    from: i64,
    to: i64,
}

/// Which frames around a chunk play the file as it was made: a whole number
/// of frames ahead, at unity speed, at every grid frame between. A function of
/// the position function alone, so every chunk that looks at a frame decides
/// the same for it (D3).
struct Unity {
    /// The first frame described, on the grid.
    lo: i64,
    /// One entry per [`GRID`] frames from `lo`.
    spans: Vec<Option<Span>>,
}

impl Unity {
    /// The spans over `first..last` and a margin beyond them, wide enough
    /// that a frame of the range is never within [`FADE`] of a cut made by the
    /// scan's own ends.
    fn scan(at: &Positions<'_>, first: i64, last: i64) -> Self {
        let margin = 2 * FADE + GRID;
        let lo = (first - margin).div_euclid(GRID) * GRID;
        let hi = (last + margin + GRID - 1).div_euclid(GRID) * GRID;
        let count = ((hi - lo) / GRID) as usize;
        let mut spans: Vec<Option<Span>> = Vec::with_capacity(count);
        let mut p = at.get(lo);
        for i in 0..count {
            let k = lo + i as i64 * GRID;
            let q = at.get(k + GRID);
            let offset = (p - k as f64).round();
            // (Within a range frame arithmetic cannot overflow.)
            let unity = offset.is_finite()
                && offset.abs() < 1.0e15
                && (p - k as f64 - offset).abs() < SNAP
                && ((q - p) / GRID as f64 - 1.0).abs() < SNAP
                && at.unshifted(k)
                && at.unshifted(k + GRID);
            spans.push(unity.then(|| Span {
                offset: offset as i64,
                from: k,
                to: k + GRID,
            }));
            p = q;
        }
        // Join the neighbours that share an offset into one run.
        let mut i = 0;
        while i < count {
            let Some(here) = spans[i] else {
                i += 1;
                continue;
            };
            let mut j = i + 1;
            while j < count && spans[j].is_some_and(|s| s.offset == here.offset) {
                j += 1;
            }
            let run = Span {
                offset: here.offset,
                from: lo + i as i64 * GRID,
                to: lo + j as i64 * GRID,
            };
            spans[i..j].fill(Some(run));
            i = j;
        }
        Self { lo, spans }
    }

    fn span(&self, k: i64) -> Option<Span> {
        let i = usize::try_from((k - self.lo).div_euclid(GRID)).ok()?;
        self.spans.get(i).copied().flatten()
    }

    /// How much of the file frame `k` plays: all of it well inside a run, less
    /// toward either end of it.
    fn weight(span: Span, k: i64) -> f32 {
        let edge = (k - span.from).min(span.to - 1 - k);
        if edge >= FADE {
            1.0
        } else {
            0.5 - 0.5 * (std::f32::consts::PI * (edge as f32 + 0.5) / FADE as f32).cos()
        }
    }

    /// The offset when every frame of `a..b` is the file, with nothing of the
    /// stretch in it.
    fn whole(&self, a: i64, b: i64) -> Option<i64> {
        let (first, last) = (self.span(a)?, self.span(b - 1)?);
        let same = first.from == last.from && first.to == last.to && first.offset == last.offset;
        (same && a - first.from >= FADE && first.to - b >= FADE).then_some(first.offset)
    }
}

/// Puts the file over the frames of `left` (and `right`, for a stereo file)
/// that are in a run at the file's own speed: all of the file well inside the
/// run, blended into what is there toward its ends. `first` is the stream
/// frame of `left[0]`.
fn unity_over(
    unity: &Unity,
    window: &mut Window,
    source: &mut dyn Source,
    first: i64,
    left: &mut [f32],
    mut right: Option<&mut [f32]>,
) -> Result<(), MediaError> {
    let end = first + left.len() as i64;
    let mut k = first;
    while k < end {
        // One grid step at a time: a run is whole grid steps.
        let stop = (unity.lo + ((k - unity.lo).div_euclid(GRID) + 1) * GRID).min(end);
        if let Some(span) = unity.span(k) {
            let from = k + span.offset;
            let planes = window.get(source, from, from + (stop - k))?;
            let a = (k - first) as usize;
            for (j, v) in left[a..a + (stop - k) as usize].iter_mut().enumerate() {
                blend(v, planes[0][j], Unity::weight(span, k + j as i64));
            }
            if let Some(right) = right.as_deref_mut() {
                for (j, v) in right[a..a + (stop - k) as usize].iter_mut().enumerate() {
                    blend(v, planes[1][j], Unity::weight(span, k + j as i64));
                }
            }
        }
        k = stop;
    }
    Ok(())
}

/// `v` moved toward `file` by `weight`; the file itself when it is 1.
fn blend(v: &mut f32, file: f32, weight: f32) {
    *v = if weight >= 1.0 {
        file
    } else {
        *v + (file - *v) * weight
    };
}

/// A run of the file, with silence outside it.
#[derive(Default)]
struct Window {
    from: i64,
    data: [Vec<f32>; 2],
}

impl Window {
    /// The file's frames `a..b` (silence where the file is not), one slice
    /// per channel; a mono file's second slice is empty.
    fn get(&mut self, source: &mut dyn Source, a: i64, b: i64) -> Result<[&[f32]; 2], MediaError> {
        let len = (b - a).max(0) as usize;
        let have = self.data[0].len() as i64;
        if a < self.from || b > self.from + have {
            self.fill(source, a, (b - a).max(WINDOW))?;
        }
        let at = (a - self.from) as usize;
        let stereo = source.channels() >= 2;
        let second: &[f32] = if stereo {
            &self.data[1][at..at + len]
        } else {
            &[]
        };
        Ok([&self.data[0][at..at + len], second])
    }

    fn fill(&mut self, source: &mut dyn Source, from: i64, len: i64) -> Result<(), MediaError> {
        let stereo = source.channels() >= 2;
        let len = len as usize;
        self.from = from;
        for (c, plane) in self.data.iter_mut().enumerate() {
            plane.clear();
            plane.resize(if c == 0 || stereo { len } else { 0 }, 0.0);
        }
        let frames = source.frames() as i64;
        let (start, stop) = (from.max(0), (from + len as i64).min(frames));
        if stop <= start {
            return Ok(());
        }
        let (head, tail) = ((start - from) as usize, (stop - from) as usize);
        let [a, b] = &mut self.data;
        if stereo {
            source.read(start as u64, &mut [&mut a[head..tail], &mut b[head..tail]])
        } else {
            source.read(start as u64, &mut [&mut a[head..tail]])
        }
    }
}
