use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;

use lumen_engine::{ClipAudio, StreamAudio};
use lumen_render::{STRIP_CAPACITY, StripEntry};

use crate::media::streams::Readers;

pub(super) const SAMPLE_BELOW: f64 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Geometry {
    pub media: u64,
    pub row: u32,
    pub start: f64,
    pub first_frame: f64,
    pub frames_per_beat: f64,
    pub frames: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Pixels {
    pub left: f64,
    pub per_beat: f64,
    pub origin: f64,
    pub width: f64,
}

impl Geometry {
    fn frame_at(&self, px: Pixels, x: f64) -> f64 {
        self.first_frame
            + ((x - px.origin) / px.per_beat + px.left - self.start) * self.frames_per_beat
    }

    fn x_of(&self, px: Pixels, frame: f64) -> f64 {
        let beat = self.start + (frame - self.first_frame) / self.frames_per_beat;
        px.origin + (beat - px.left) * px.per_beat
    }

    fn visible(&self, px: Pixels, length: usize) -> Option<(i64, i64)> {
        let wave_end = self.first_frame + self.frames;
        let a = self.frame_at(px, 0.0).max(self.first_frame).floor() as i64 - 1;
        let b = self.frame_at(px, px.width).min(wave_end).ceil() as i64 + 1;
        let (a, b) = (a.max(0), b.min(length as i64 - 1));
        (a <= b).then_some((a, b))
    }
}

pub struct Update {
    pub table: Vec<StripEntry>,
    pub writes: Vec<(u32, Vec<[i16; 2]>)>,
}

#[derive(Debug, Default)]
pub(super) struct Strips {
    cached: Vec<Option<Cached>>,
    used: u32,
    reader: StripReader,
    active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Cached {
    first: i64,
    count: u32,
    at: u32,
}

impl Strips {
    pub fn reset(&mut self) {
        self.cached.clear();
        self.used = 0;
    }

    pub fn update(
        &mut self,
        waves: &[Geometry],
        visible: std::ops::Range<u32>,
        px: Pixels,
        ctx: &egui::Context,
        audio: impl Fn(u64) -> Option<Arc<ClipAudio>>,
    ) -> Option<Update> {
        self.cached.resize(waves.len(), None);
        let mut needs = Vec::new();
        let from = waves.partition_point(|w| w.row < visible.start);
        let to = waves.partition_point(|w| w.row < visible.end);
        for (i, wave) in waves.iter().enumerate().take(to).skip(from) {
            if wave.frames_per_beat / px.per_beat >= SAMPLE_BELOW {
                continue;
            }
            let Some(sound) = audio(wave.media) else {
                continue;
            };
            if let Some((a, b)) = wave.visible(px, sound.frames()) {
                let fits = self.cached[i]
                    .is_some_and(|c| c.first <= a && b < c.first + i64::from(c.count));
                needs.push((i, a, b, sound, fits));
            }
        }
        if needs.is_empty() && !self.active {
            return None;
        }
        let spare =
            |a: i64, b: i64, len: usize| ((a - (b - a)).max(0), (b + (b - a)).min(len as i64 - 1));
        let fresh: u64 = needs
            .iter()
            .filter(|n| !n.4)
            .map(|&(_, a, b, ref s, _)| {
                let (a, b) = spare(a, b, s.frames());
                (b - a + 1) as u64
            })
            .sum();
        if u64::from(self.used) + fresh > u64::from(STRIP_CAPACITY) {
            self.reset();
            self.cached.resize(waves.len(), None);
            for need in &mut needs {
                need.4 = false;
            }
        }
        let mut writes = Vec::new();
        let mut table = vec![[0_u32; 4]; waves.len()];
        for (i, a, b, sound, fits) in needs {
            if !fits {
                let (a, b) = spare(a, b, sound.frames());
                let count = (b - a + 1) as u32;
                if self.used + count > STRIP_CAPACITY {
                    self.cached[i] = None;
                    continue;
                }
                let Some(samples) = self.reader.samples(&sound, a, count, ctx) else {
                    self.cached[i] = None;
                    continue;
                };
                writes.push((self.used, samples));
                self.cached[i] = Some(Cached {
                    first: a,
                    count,
                    at: self.used,
                });
                self.used += count;
            }
            if let Some(c) = self.cached[i] {
                let wave = &waves[i];
                let per = px.per_beat / wave.frames_per_beat;
                table[i] = [
                    c.at,
                    c.count,
                    (wave.x_of(px, c.first as f64) as f32).to_bits(),
                    (per as f32).to_bits(),
                ];
            }
        }
        self.active = table.iter().any(|e| e[1] > 0);
        Some(Update { table, writes })
    }
}

fn to_strip(left: &[f32], right: &[f32]) -> Vec<[i16; 2]> {
    let at = |v: f32| (v.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
    left.iter()
        .zip(right)
        .map(|(l, r)| [at(*l), at(*r)])
        .collect()
}

fn in_memory(channels: &[Vec<f32>], first: i64, count: u32) -> Vec<[i16; 2]> {
    let left = channels.first().map_or(&[][..], Vec::as_slice);
    let right = channels.get(1).map_or(left, Vec::as_slice);
    let range = |side: &[f32]| {
        let mut out = vec![0.0; count as usize];
        let from = (first.max(0) as usize).min(side.len());
        let to = (from + count as usize).min(side.len());
        out[..to - from].copy_from_slice(&side[from..to]);
        out
    };
    to_strip(&range(left), &range(right))
}

type Want = (u64, i64, u32);

type Read = (Want, Vec<[i16; 2]>);

#[derive(Default)]
pub(crate) struct StripReader {
    worker: Option<Worker>,
}

impl std::fmt::Debug for StripReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StripReader").finish_non_exhaustive()
    }
}

struct Worker {
    requests: Sender<(StreamAudio, Want, egui::Context)>,
    done: Arc<Mutex<Option<Read>>>,
    asked: Option<Want>,
}

impl StripReader {
    pub(crate) fn waiting(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| {
            w.asked.is_some()
                && w.done
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_none()
        })
    }

    pub(crate) fn samples(
        &mut self,
        audio: &ClipAudio,
        first: i64,
        count: u32,
        ctx: &egui::Context,
    ) -> Option<Vec<[i16; 2]>> {
        let stream = match audio {
            ClipAudio::Memory { channels, .. } => return Some(in_memory(channels, first, count)),
            ClipAudio::Stream(stream) => stream,
        };
        let want = (stream.id(), first, count);
        let worker = self.worker.get_or_insert_with(Worker::spawn);
        let mut done = worker.done.lock().unwrap_or_else(PoisonError::into_inner);
        if done.as_ref().is_some_and(|(read, _)| *read == want) {
            worker.asked = None;
            return done.take().map(|(_, samples)| samples);
        }
        drop(done);
        if worker.asked != Some(want) {
            worker.asked = Some(want);
            let _ = worker.requests.send((stream.clone(), want, ctx.clone()));
        }
        None
    }
}

impl Worker {
    fn spawn() -> Self {
        let (requests, inbox) = channel::<(StreamAudio, Want, egui::Context)>();
        let done = Arc::new(Mutex::new(None));
        let results = Arc::clone(&done);
        let spawned = crate::spawn::spawn_named("lumen-strip-read", move || {
            let mut readers = Readers::default();
            while let Ok(mut request) = inbox.recv() {
                while let Ok(newer) = inbox.try_recv() {
                    request = newer;
                }
                let (stream, want @ (_, first, count), ctx) = request;
                let (mut left, mut right) = (vec![0.0; count as usize], vec![0.0; count as usize]);
                let from = first.max(0) as u64;
                if readers.read(&stream, from, &mut left, &mut right).is_err() {
                    continue;
                }
                if stream.channels() == 1 {
                    right.copy_from_slice(&left);
                }
                readers.keep(std::slice::from_ref(&stream));
                *results.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some((want, to_strip(&left, &right)));
                ctx.request_repaint();
            }
        });
        if let Err(error) = spawned {
            crate::trace::log(|| {
                format!(
                    "samples: {}",
                    crate::spawn::start_failed("the sample reader", &error)
                )
            });
        }
        Self {
            requests,
            done,
            asked: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio() -> Arc<ClipAudio> {
        let left: Vec<f32> = (0..48_000).map(|i| i as f32 / 48_000.0).collect();
        let right = left.iter().map(|v| -v).collect();
        Arc::new(ClipAudio::Memory {
            sample_rate: 48_000,
            channels: vec![left, right],
        })
    }

    const WAVE: Geometry = Geometry {
        media: 7,
        row: 0,
        start: 100.0,
        first_frame: 0.0,
        frames_per_beat: 24_000.0,
        frames: 48_000.0,
    };

    const DEEP: Pixels = Pixels {
        left: 100.25,
        per_beat: 192_000.0,
        origin: 0.3,
        width: 1_000.0,
    };

    #[test]
    fn the_visible_frames_have_a_sample_to_spare() {
        assert_eq!(WAVE.visible(DEEP, 48_000), Some((5_998, 6_126)));
        let late = Pixels {
            left: 101.999,
            ..DEEP
        };
        assert_eq!(WAVE.visible(late, 48_000).unwrap().1, 47_999);
        let after = Pixels {
            left: 103.0,
            ..DEEP
        };
        assert_eq!(WAVE.visible(after, 48_000), None);
    }

    #[test]
    fn strips_are_placed_exactly_and_reused_while_scrolling() {
        let mut strips = Strips::default();
        let ctx = egui::Context::default();
        let sound = audio();
        let update = strips
            .update(&[WAVE], 0..1, DEEP, &ctx, |_| Some(Arc::clone(&sound)))
            .unwrap();
        assert_eq!(update.writes.len(), 1);
        let [at, count, x, per] = update.table[0];
        assert_eq!((at, count), (0, 385));
        let first = 5_998 - 128;
        let level = ((first as f32 / 48_000.0) * 32_767.0).round() as i16;
        assert_eq!(update.writes[0].1[0], [level, -level]);
        assert!((f64::from(f32::from_bits(x)) - (0.3 - 130.0 * 8.0)).abs() < 1e-3);
        assert_eq!(f32::from_bits(per), 8.0);
        let moved = Pixels {
            left: 100.2505,
            ..DEEP
        };
        let update = strips
            .update(&[WAVE], 0..1, moved, &ctx, |_| Some(Arc::clone(&sound)))
            .unwrap();
        assert!(update.writes.is_empty());
        assert_eq!(update.table[0][..2], [0, 385]);
    }

    #[test]
    fn coarser_zooms_and_hidden_rows_draw_from_peaks() {
        let mut strips = Strips::default();
        let ctx = egui::Context::default();
        let sound = audio();
        let coarse = Pixels {
            per_beat: 1_200.0,
            ..DEEP
        };
        assert!(
            strips
                .update(&[WAVE], 0..1, coarse, &ctx, |_| Some(Arc::clone(&sound)))
                .is_none()
        );
        assert!(
            strips
                .update(&[WAVE], 0..0, DEEP, &ctx, |_| Some(Arc::clone(&sound)))
                .is_none()
        );
        strips.update(&[WAVE], 0..1, DEEP, &ctx, |_| Some(Arc::clone(&sound)));
        let clear = strips
            .update(&[WAVE], 0..1, coarse, &ctx, |_| Some(Arc::clone(&sound)))
            .unwrap();
        assert_eq!(clear.table[0][1], 0);
        assert!(
            strips
                .update(&[WAVE], 0..1, coarse, &ctx, |_| Some(Arc::clone(&sound)))
                .is_none()
        );
    }

    #[test]
    fn a_streamed_file_gets_its_strip_from_the_worker_with_the_same_samples() {
        use lumen_media::wav::{Sample, Spec, Writer};

        let sound = audio();
        let channels = sound.memory().unwrap();
        let path = std::env::temp_dir().join(format!("lumen-strips-{}.wav", std::process::id()));
        let mut writer = Writer::create(
            &path,
            Spec {
                sample_rate: 48_000,
                channels: 2,
                sample: Sample::F32,
            },
        )
        .unwrap();
        writer.write(&[&channels[0], &channels[1]]).unwrap();
        writer.finish().unwrap();
        let id = crate::media::streams::register(path.clone());
        let pool = lumen_engine::PagePool::new(1);
        let streamed = Arc::new(ClipAudio::Stream(StreamAudio::new(
            &pool, id, 48_000, 2, 48_000,
        )));

        let ctx = egui::Context::default();
        let mut memory = Strips::default();
        let want = memory
            .update(&[WAVE], 0..1, DEEP, &ctx, |_| Some(Arc::clone(&sound)))
            .unwrap();
        let mut strips = Strips::default();
        let first = strips
            .update(&[WAVE], 0..1, DEEP, &ctx, |_| Some(Arc::clone(&streamed)))
            .unwrap();
        assert!(first.writes.is_empty() && first.table[0][1] == 0);
        let started = std::time::Instant::now();
        let got = loop {
            let update = strips
                .update(&[WAVE], 0..1, DEEP, &ctx, |_| Some(Arc::clone(&streamed)))
                .unwrap();
            if !update.writes.is_empty() {
                break update;
            }
            assert!(started.elapsed().as_secs() < 10, "the strip never came");
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!(got.writes, want.writes);
        assert_eq!(got.table, want.table);
        crate::media::streams::forget(id);
        let _ = std::fs::remove_file(path);
    }
}
