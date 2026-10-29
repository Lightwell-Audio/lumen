//! What the disk thread reads next (M9 slice R6, ADR-0109): from the clip
//! list the engine plays, the playhead and the places playback can jump to,
//! the pages of streamed files wanted in memory, nearest first.
//!
//! Not real-time code: the thread that writes a pool plans with it, off the
//! audio thread (it allocates).

use std::collections::HashSet;
use std::hash::{BuildHasherDefault, DefaultHasher};
use std::ops::Range;

use crate::{Clip, ClipAudio, PAGE, StreamAudio, Timeline, for_each_page};

/// The streamed clips of one clip list, with their starts in samples.
#[derive(Default)]
pub struct Scene {
    clips: Vec<Placed>,
    /// Each file once, by the order clips first name it.
    streams: Vec<StreamAudio>,
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

/// The pages a plan has listed. Hashed with fixed keys, so a plan takes the
/// same work every time (the performance gate counts it).
type Seen = HashSet<Want, BuildHasherDefault<DefaultHasher>>;

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
    /// Plans pages in priority order and returns how many belong to the cue
    /// horizon before playhead, marker, and point pages are added.
    pub fn plan(&self, at: Position, horizon: Horizon, out: &mut Vec<Want>) -> usize {
        out.clear();
        let mut seen = Seen::default();
        let near = horizon.near.max(1);
        let from = |point: &i64| *point..point + near;
        if let Some(cue) = at.cue {
            // A cue is for a start or jump. Plan what playback will read,
            // including the loop wrap, even while the transport is stopped.
            self.walk(&self.ahead(cue, 0..near, true), &mut seen, out);
        }
        let cue_prefix = out.len();
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
        cue_prefix
    }

    /// Whether every page the clips play from `sample` for `span` frames is
    /// in memory: what the app waits for before it starts or jumps (L A).
    /// A start plays through an enabled loop, so the ready window wraps too.
    pub fn ready(&self, sample: i64, span: i64) -> bool {
        let windows = self.ahead(sample, 0..span.max(1), true);
        windows.iter().all(|window| {
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
        })
    }

    /// The timeline spans `from` plays over `range` frames ahead of it: one,
    /// or two where a playing loop wraps.
    fn ahead(&self, from: i64, range: Range<i64>, playing: bool) -> Vec<Range<i64>> {
        let (a, b) = (from + range.start, from + range.end);
        match self.looped {
            Some((start, end)) if playing && from < end => {
                let length = end - start;
                let mut spans = Vec::new();
                let (mut t, mut left) = if from < start && a < end {
                    let before_wrap = b.min(end);
                    if a < before_wrap {
                        spans.push(a..before_wrap);
                    }
                    if b <= end {
                        return spans;
                    }
                    (start, (b - end).min(length))
                } else {
                    let loop_from = if from < start { end } else { start };
                    let offset = (a - loop_from).rem_euclid(length);
                    (start + offset, (b - a).min(length))
                };
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

    /// Adds the pages of `spans` in the order they first occur in time across
    /// all clips.
    fn walk(&self, spans: &[Range<i64>], seen: &mut Seen, out: &mut Vec<Want>) {
        let mut first: Vec<(i64, Want)> = Vec::new();
        for span in spans {
            first.clear();
            for p in self
                .clips
                .iter()
                .filter(|p| p.start < span.end && p.start + p.clip.length > span.start)
            {
                let stream = p.stream;
                let frames = self.streams[stream].frames();
                for_each_page(&p.clip, p.start, span.clone(), frames, &mut |page| {
                    if let Some(at) =
                        first_page_sample(&p.clip, p.start, span.clone(), frames, page)
                    {
                        first.push((at, Want { stream, page }));
                    }
                    true
                });
            }
            // Stable sorting keeps clip order for pages first needed at the
            // same timeline sample, while putting every page needed earlier
            // ahead of a later source page from another clip.
            first.sort_by_key(|(at, _)| *at);
            for (_, want) in first.drain(..) {
                if seen.insert(want) {
                    out.push(want);
                }
            }
        }
    }
}

/// The first timeline sample at which `clip` reads a candidate `page` in
/// `span`. Callers enumerate candidates through `for_each_page`, so this only
/// calculates first use for pages touched by the requested span. The mapping
/// mirrors `source_spans`: linear clips advance with timeline time; repeated
/// clips wrap through their phase and period.
fn first_page_sample(
    clip: &Clip,
    start: i64,
    span: Range<i64>,
    frames: u64,
    page: usize,
) -> Option<i64> {
    let (from, to) = (span.start.max(start), span.end.min(start + clip.length));
    if from >= to {
        return None;
    }
    let page = i64::try_from(page).unwrap_or(i64::MAX);
    let page_start = page * PAGE as i64;
    let page_end = (page + 1) * PAGE as i64;
    let frames = i64::try_from(frames).unwrap_or(i64::MAX);
    match clip.shape.period.filter(|period| *period > 0) {
        Some(period) => {
            let logical_from = (page_start - clip.offset).max(0);
            let logical_to = (page_end - clip.offset)
                .min(period)
                .min(frames - clip.offset);
            if logical_from >= logical_to {
                return None;
            }
            let logical_at_from = (from - start + clip.shape.phase).rem_euclid(period);
            let ahead = if logical_at_from < logical_from {
                logical_from - logical_at_from
            } else if logical_at_from < logical_to {
                0
            } else {
                period - logical_at_from + logical_from
            };
            let first = from + ahead;
            (first < to).then_some(first)
        }
        None => {
            let source_from = clip.offset + from - start;
            let source_to = clip.offset + to - start;
            if source_from.max(0) >= source_to.min(frames) {
                return None;
            }
            let first = from + (page_start - source_from).max(0);
            (first < to).then_some(first)
        }
    }
}
