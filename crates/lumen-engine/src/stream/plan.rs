//! What the disk thread reads next (M9 slice R6, ADR-0109): from the clip
//! list the engine plays, the playhead and the places playback can jump to,
//! the pages of streamed files wanted in memory, nearest first.
//!
//! Not real-time code: the thread that writes a pool plans with it, off the
//! audio thread (it allocates).

use std::ops::Range;

use crate::{Clip, ClipAudio, PAGE, StreamAudio, Timeline, for_each_page};

/// The streamed clips of one clip list, with their starts in samples.
#[derive(Default)]
pub struct Scene {
    clips: Vec<Placed>,
    /// Each file once, by the order clips first name it.
    streams: Vec<StreamAudio>,
    /// Where each file's pages begin in a plan's [`Seen`] bits, and the
    /// pages of all files.
    first: Vec<usize>,
    pages: usize,
    /// The loop region, in timeline samples, while looping is on.
    looped: Option<(i64, i64)>,
    /// Places playback can jump to (loop start, markers, the song's start):
    /// the audio after each is kept in memory.
    points: Vec<i64>,
    /// The rate the clips play at, and the timeline that places them.
    rate: u32,
    timeline: Timeline,
}

struct Placed {
    clip: Clip,
    start: i64,
    stream: usize,
}

/// How far ahead the disk thread reads, in frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Horizon {
    /// Read first: what the next moments play, at the playhead and at each
    /// point.
    pub near: i64,
    /// Read after: the rest of the read-ahead at the playhead.
    pub far: i64,
}

impl Horizon {
    /// Half a second near, two seconds in all.
    pub fn at(rate: u32) -> Self {
        let rate = i64::from(rate);
        Self {
            near: rate / 2,
            far: 2 * rate,
        }
    }
}

/// Where the transport is, as the disk thread plans.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Position {
    pub sample: i64,
    pub playing: bool,
    /// Where the app is about to start or jump to (L A): read before
    /// anything else.
    pub cue: Option<i64>,
    /// The play marker, where Play starts from: read after the playhead.
    pub marker: Option<i64>,
}

/// The pages a plan has listed: a bit for each page of each file of the
/// scene. A plan lists thousands of pages every few milliseconds, and a hash
/// set took most of the disk thread's time to remember them (in a debug build,
/// more than it spent reading).
struct Seen {
    bits: Vec<u64>,
}

impl Seen {
    fn new(pages: usize) -> Self {
        Self {
            bits: vec![0; pages.div_ceil(64)],
        }
    }

    /// Marks bit `i`; returns whether it was clear.
    fn insert(&mut self, i: usize) -> bool {
        let (word, bit) = (i / 64, 1u64 << (i % 64));
        let clear = self.bits[word] & bit == 0;
        self.bits[word] |= bit;
        clear
    }
}

/// A page wanted in memory: a file of the scene and a page of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Want {
    pub stream: usize,
    pub page: usize,
}

impl Scene {
    /// The streamed clips of `clips` on `timeline` at `rate`.
    pub fn new(
        clips: &[Clip],
        timeline: &Timeline,
        rate: u32,
        looped: Option<(i64, i64)>,
        points: Vec<i64>,
    ) -> Self {
        let mut scene = Self {
            looped: looped.filter(|(a, b)| a < b),
            points,
            rate,
            timeline: timeline.clone(),
            ..Self::default()
        };
        for clip in clips {
            let ClipAudio::Stream(audio) = &*clip.audio else {
                continue;
            };
            if audio.sample_rate() != rate || clip.length <= 0 {
                continue;
            }
            let stream = match scene.streams.iter().position(|s| s == audio) {
                Some(i) => i,
                None => {
                    scene.first.push(scene.pages);
                    scene.pages += audio.pages();
                    scene.streams.push(audio.clone());
                    scene.streams.len() - 1
                }
            };
            scene.clips.push(Placed {
                clip: clip.clone(),
                start: timeline.sample_at(clip.start, rate),
                stream,
            });
        }
        scene
    }

    pub fn streams(&self) -> &[StreamAudio] {
        &self.streams
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// The rate the scene's clips play at (0 for an empty scene).
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The timeline sample of `at`.
    pub fn sample_at(&self, at: lumen_core::time::Ticks) -> i64 {
        self.timeline.sample_at(at, self.rate)
    }

    /// The pages wanted from `at`, in the order to read them, into `out`:
    /// the cue, then the next moments at the playhead and at the play
    /// marker, then the rest of the read-ahead, then the next moments after
    /// each point. Each page once. A pool too small for the plan keeps its
    /// beginning, so what plays on is read before the places playback might
    /// jump to (a jump that finds its audio missing waits, L A).
    pub fn plan(&self, at: Position, horizon: Horizon, out: &mut Vec<Want>) {
        out.clear();
        let mut seen = Seen::new(self.pages);
        let near = horizon.near.max(1);
        let from = |point: &i64| *point..point + near;
        if let Some(cue) = at.cue {
            self.walk(&self.ahead(cue, 0..near, at.playing), &mut seen, out);
        }
        self.walk(&self.ahead(at.sample, 0..near, at.playing), &mut seen, out);
        if let Some(marker) = &at.marker {
            self.walk(&[from(marker)], &mut seen, out);
        }
        if horizon.far > near {
            let rest = self.ahead(at.sample, near..horizon.far, at.playing);
            self.walk(&rest, &mut seen, out);
        }
        for point in &self.points {
            self.walk(&[from(point)], &mut seen, out);
        }
    }

    /// Whether every page the clips play from `sample` for `span` frames is
    /// in memory: what the app waits for before it starts or jumps (L A).
    pub fn ready(&self, sample: i64, span: i64) -> bool {
        let window = sample..sample + span.max(1);
        self.clips.iter().all(|p| {
            let audio = &self.streams[p.stream];
            for_each_page(
                &p.clip,
                p.start,
                window.clone(),
                audio.frames(),
                &mut |page| audio.resident(page),
            )
        })
    }

    /// The timeline spans `from` plays over `range` frames ahead of it: one,
    /// or two where a playing loop wraps.
    fn ahead(&self, from: i64, range: Range<i64>, playing: bool) -> Vec<Range<i64>> {
        let (a, b) = (from + range.start, from + range.end);
        match self.looped {
            Some((start, end)) if playing && (start..end).contains(&from) => {
                let length = end - start;
                // Frames ahead of the loop's start, wrapped into the loop.
                let wrap = |t: i64| start + (t - start).rem_euclid(length);
                let mut spans = Vec::new();
                let (mut t, mut left) = (wrap(a), (b - a).min(length));
                while left > 0 {
                    let to = (t + left).min(end);
                    spans.push(t..to);
                    left -= to - t;
                    t = start;
                }
                spans
            }
            _ => std::iter::once(a..b).collect(),
        }
    }

    /// Adds the pages of `spans`, slice by slice so pages nearer in time come
    /// first across all clips.
    fn walk(&self, spans: &[Range<i64>], seen: &mut Seen, out: &mut Vec<Want>) {
        let page = PAGE as i64;
        for span in spans {
            let under: Vec<&Placed> = self
                .clips
                .iter()
                .filter(|p| p.start < span.end && p.start + p.clip.length > span.start)
                .collect();
            let mut t = span.start;
            while t < span.end {
                let slice = t..(t + page).min(span.end);
                for p in &under {
                    let stream = p.stream;
                    let audio = &self.streams[stream];
                    let (first, pages) = (self.first[stream], audio.pages());
                    for_each_page(
                        &p.clip,
                        p.start,
                        slice.clone(),
                        audio.frames(),
                        &mut |page| {
                            // for_each_page visits only pages of the source.
                            if page < pages && seen.insert(first + page) {
                                out.push(Want { stream, page });
                            }
                            true
                        },
                    );
                }
                t = slice.end;
            }
        }
    }
}
