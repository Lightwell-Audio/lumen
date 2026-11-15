use super::Stretcher;
use crate::MediaError;
use crate::repitch::{GRID, OfFrame, Source};

pub const CHUNK: i64 = 98_304;

pub const FADE: i64 = 2_048;

const SLOWEST: f64 = 0.25;
const FASTEST: f64 = 4.0;

const SNAP: f64 = 1.0e-7;

const WINDOW: i64 = 32_768;

pub struct Chunks {
    main: Stretcher,
    aux: Option<Stretcher>,
    window: Window,
    aux_window: Window,
    copy_window: Window,
    session: Option<Session>,
    hop_out: [Vec<f32>; 2],
}

struct Session {
    chunk: i64,
    copy: Option<i64>,
    unity: Unity,
    run: Option<Run>,
    tail: Option<[Vec<f32>; 2]>,
}

struct Run {
    first: i64,
    start: i64,
    hop: i64,
    read_at: i64,
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
            unity_over(
                &session.unity,
                &mut self.copy_window,
                source,
                from,
                left,
                right.as_deref_mut(),
            )?;
        }
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
    let unity = Unity::scan(at, first - FADE, first + CHUNK);
    if let Some(offset) = unity.whole(first - FADE, first + CHUNK) {
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
    fn new(
        stretcher: &mut Stretcher,
        at: &Positions<'_>,
        first: i64,
        limit: usize,
    ) -> Result<Self, MediaError> {
        let hop = stretcher.interval() as i64;
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
            let next = round(at.get(a + hop + latency)) + lead;
            let n = (next - self.read_at).max(0);
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

struct Positions<'p> {
    position: &'p dyn Fn(i64) -> f64,
    pitch: Option<&'p dyn Fn(i64) -> f64>,
    hop: i64,
}

const MOST_CENTS: f64 = 4_800.0;

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

    fn cents(&self, k: i64) -> Option<f64> {
        let cents = (self.pitch?)(k.max(0));
        Some(if cents.is_finite() {
            cents.clamp(-MOST_CENTS, MOST_CENTS)
        } else {
            0.0
        })
    }

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

#[derive(Clone, Copy)]
struct Span {
    offset: i64,
    from: i64,
    to: i64,
}

struct Unity {
    lo: i64,
    spans: Vec<Option<Span>>,
}

impl Unity {
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

    fn weight(span: Span, k: i64) -> f32 {
        let edge = (k - span.from).min(span.to - 1 - k);
        if edge >= FADE {
            1.0
        } else {
            0.5 - 0.5 * (std::f32::consts::PI * (edge as f32 + 0.5) / FADE as f32).cos()
        }
    }

    fn whole(&self, a: i64, b: i64) -> Option<i64> {
        let (first, last) = (self.span(a)?, self.span(b - 1)?);
        let same = first.from == last.from && first.to == last.to && first.offset == last.offset;
        (same && a - first.from >= FADE && first.to - b >= FADE).then_some(first.offset)
    }
}

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

fn blend(v: &mut f32, file: f32, weight: f32) {
    *v = if weight >= 1.0 {
        file
    } else {
        *v + (file - *v) * weight
    };
}

#[derive(Default)]
struct Window {
    from: i64,
    data: [Vec<f32>; 2],
}

impl Window {
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
