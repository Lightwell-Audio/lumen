//! Waveforms sample by sample (M3 slice 4d part 4; owner: zoom to single
//! samples). Past the peaks' detail (fewer than [`SAMPLE_BELOW`] frames per
//! pixel) the app hands the GPU the visible samples of each wave, with a
//! screen's width to spare on each side so scrolling rarely needs more.
//! Where they go on screen is worked out here in f64 and sent as viewport
//! pixels, so it stays exact deep into a long file.

use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;

use lumen_engine::{ClipAudio, StreamAudio};
use lumen_render::{STRIP_CAPACITY, StripEntry};

use crate::media::streams::Readers;

/// Below this many frames per pixel a wave is drawn from its samples (the
/// finest peaks hold 16 frames a bucket).
pub(super) const SAMPLE_BELOW: f64 = 16.0;

/// A wave's place in time, exactly: the beat it starts at, its first frame
/// at the device rate, frames per beat (its tempo), and how many frames it
/// covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Geometry {
    pub media: u64,
    pub row: u32,
    pub start: f64,
    pub first_frame: f64,
    pub frames_per_beat: f64,
    pub frames: f64,
}

/// The view in the GPU's physical pixels: the beat at the left edge, pixels
/// per beat, where the left edge falls in the viewport (under a pixel), and
/// the width.
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

    /// The frames on screen, within the wave and the file (`length`):
    /// first and last, with a sample to spare at each end for the line.
    fn visible(&self, px: Pixels, length: usize) -> Option<(i64, i64)> {
        let wave_end = self.first_frame + self.frames;
        let a = self.frame_at(px, 0.0).max(self.first_frame).floor() as i64 - 1;
        let b = self.frame_at(px, px.width).min(wave_end).ceil() as i64 + 1;
        let (a, b) = (a.max(0), b.min(length as i64 - 1));
        (a <= b).then_some((a, b))
    }
}

/// Strips sent this frame: an entry per wave, and new samples to place.
pub struct Update {
    pub table: Vec<StripEntry>,
    pub writes: Vec<(u32, Vec<[i16; 2]>)>,
}

/// Which samples each wave has on the GPU.
#[derive(Debug, Default)]
pub(super) struct Strips {
    cached: Vec<Option<Cached>>,
    /// Space handed out in the sample atlas.
    used: u32,
    /// Reads the samples of files played from disk.
    reader: StripReader,
    /// Strips were in use at the last update (so a clear is sent once).
    active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Cached {
    first: i64,
    count: u32,
    at: u32,
}

impl Strips {
    /// The waves were rebuilt: strips start over.
    pub fn reset(&mut self) {
        self.cached.clear();
        self.used = 0;
    }

    /// Strips for the waves in sample mode on the `visible` rows; `None`
    /// when nothing changes on the GPU. `waves` are in row order, so only
    /// the visible rows' waves are looked at.
    pub fn update(
        &mut self,
        waves: &[Geometry],
        visible: std::ops::Range<u32>,
        px: Pixels,
        ctx: &egui::Context,
        audio: impl Fn(u64) -> Option<Arc<ClipAudio>>,
    ) -> Option<Update> {
        self.cached.resize(waves.len(), None);
        // The frames each wave in sample mode needs, and whether its strip
        // already holds them.
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
        // New strips, each a screen's width to spare on either side; if they
        // will not fit, start the atlas over and place every strip afresh.
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
                    // Too much on screen at once: these draw from peaks.
                    self.cached[i] = None;
                    continue;
                }
                let Some(samples) = self.reader.samples(&sound, a, count, ctx) else {
                    // A file played from disk is read on a worker: until
                    // then this wave draws from its peaks.
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

/// Samples as the strips hold them: left and right (mono repeats), as 16-bit
/// fractions of full scale.
fn to_strip(left: &[f32], right: &[f32]) -> Vec<[i16; 2]> {
    let at = |v: f32| (v.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
    left.iter()
        .zip(right)
        .map(|(l, r)| [at(*l), at(*r)])
        .collect()
}

/// `count` samples of `audio` in memory from frame `first`; frames past the
/// end are silent.
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

/// Which samples were asked for: a file, its first frame and the count.
type Want = (u64, i64, u32);

/// What the worker read: the request and its samples.
type Read = (Want, Vec<[i16; 2]>);

/// Reads strips of files played from disk on a worker thread (M9 slice R6),
/// so the UI never waits for the disk. One request at a time is kept: a
/// newer one replaces what was asked before.
#[derive(Default)]
struct StripReader {
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
    /// `count` samples of `audio` from frame `first`: at once for audio in
    /// memory; for a file played from disk once the worker has read them
    /// (`None` until then, and `ctx` is woken when they are there).
    fn samples(
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
            // The worker only ends when this reader drops.
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
        std::thread::Builder::new()
            .name("lumen-strip-read".into())
            .spawn(move || {
                let mut readers = Readers::default();
                while let Ok(mut request) = inbox.recv() {
                    // Only the newest request is wanted.
                    while let Ok(newer) = inbox.try_recv() {
                        request = newer;
                    }
                    let (stream, want @ (_, first, count), ctx) = request;
                    let (mut left, mut right) =
                        (vec![0.0; count as usize], vec![0.0; count as usize]);
                    let from = first.max(0) as u64;
                    if readers.read(&stream, from, &mut left, &mut right).is_err() {
                        // A file that cannot be read draws from its peaks.
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
            })
            .expect("spawn the strip reader");
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

    /// A one-second stereo file at 48 kHz: left ramps up, right down.
    fn audio() -> Arc<ClipAudio> {
        let left: Vec<f32> = (0..48_000).map(|i| i as f32 / 48_000.0).collect();
        let right = left.iter().map(|v| -v).collect();
        Arc::new(ClipAudio::Memory {
            sample_rate: 48_000,
            channels: vec![left, right],
        })
    }

    /// A wave of the whole file from beat 100, at 120 BPM.
    const WAVE: Geometry = Geometry {
        media: 7,
        row: 0,
        start: 100.0,
        first_frame: 0.0,
        frames_per_beat: 24_000.0,
        frames: 48_000.0,
    };

    /// 8 pixels per sample, the view starting a quarter beat into the wave.
    const DEEP: Pixels = Pixels {
        left: 100.25,
        per_beat: 192_000.0,
        origin: 0.3,
        width: 1_000.0,
    };

    #[test]
    fn the_visible_frames_have_a_sample_to_spare() {
        // A quarter beat in is frame 6,000 (the left edge sits 0.3 pixels
        // in, a fraction of a sample earlier); 1,000 pixels are 125 samples.
        assert_eq!(WAVE.visible(DEEP, 48_000), Some((5_998, 6_126)));
        // Clamped to the file.
        let late = Pixels {
            left: 101.999,
            ..DEEP
        };
        assert_eq!(WAVE.visible(late, 48_000).unwrap().1, 47_999);
        // Nothing of it in view.
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
        // One strip, a screen's width to spare either side.
        assert_eq!(update.writes.len(), 1);
        let [at, count, x, per] = update.table[0];
        assert_eq!((at, count), (0, 385));
        let first = 5_998 - 128;
        let level = ((first as f32 / 48_000.0) * 32_767.0).round() as i16;
        assert_eq!(update.writes[0].1[0], [level, -level]);
        // The first sample's x: 130 samples (8 pixels each) before frame
        // 6,000, which sits at the edge.
        assert!((f64::from(f32::from_bits(x)) - (0.3 - 130.0 * 8.0)).abs() < 1e-3);
        assert_eq!(f32::from_bits(per), 8.0);
        // Scrolling a little: no new samples, only the place moves.
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
        // 20 frames per pixel: peaks.
        let coarse = Pixels {
            per_beat: 1_200.0,
            ..DEEP
        };
        assert!(
            strips
                .update(&[WAVE], 0..1, coarse, &ctx, |_| Some(Arc::clone(&sound)))
                .is_none()
        );
        // Deep, but the row is out of view.
        assert!(
            strips
                .update(&[WAVE], 0..0, DEEP, &ctx, |_| Some(Arc::clone(&sound)))
                .is_none()
        );
        // After strips were in use, leaving sample level clears them once.
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
        // Asked for: nothing to draw yet but the peaks.
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
