use std::ops::Range;

use crate::{Clip, ClipAudio, PAGE, StreamAudio, Timeline, for_each_page};

#[derive(Default)]
pub struct Scene {
    clips: Vec<Placed>,
    streams: Vec<StreamAudio>,
    first: Vec<usize>,
    pages: usize,
    looped: Option<(i64, i64)>,
    points: Vec<i64>,
    rate: u32,
    timeline: Timeline,
}

struct Placed {
    clip: Clip,
    start: i64,
    stream: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Horizon {
    pub near: i64,
    pub far: i64,
}

impl Horizon {
    pub fn at(rate: u32) -> Self {
        let rate = i64::from(rate);
        Self {
            near: rate / 2,
            far: 2 * rate,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Position {
    pub sample: i64,
    pub playing: bool,
    pub cue: Option<i64>,
    pub marker: Option<i64>,
}

struct Seen {
    bits: Vec<u64>,
}

impl Seen {
    fn new(pages: usize) -> Self {
        Self {
            bits: vec![0; pages.div_ceil(64)],
        }
    }

    fn insert(&mut self, i: usize) -> bool {
        let (word, bit) = (i / 64, 1u64 << (i % 64));
        let clear = self.bits[word] & bit == 0;
        self.bits[word] |= bit;
        clear
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Want {
    pub stream: usize,
    pub page: usize,
}

impl Scene {
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

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn sample_at(&self, at: lumen_core::time::Ticks) -> i64 {
        self.timeline.sample_at(at, self.rate)
    }

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

    fn ahead(&self, from: i64, range: Range<i64>, playing: bool) -> Vec<Range<i64>> {
        let (a, b) = (from + range.start, from + range.end);
        match self.looped {
            Some((start, end)) if playing && (start..end).contains(&from) => {
                let length = end - start;
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
