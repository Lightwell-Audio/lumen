//! Synthetic session at quality-bar scale, plus a waveform peak pyramid.

use eframe::egui::Color32;

pub const TRACKS: usize = 200;
pub const CLIPS_PER_TRACK: usize = 100;
pub const NOTES: usize = 100_000;
pub const NOTE_SPAN_BEATS: f32 = 4_000.0;
pub const SAMPLE_RATE: f32 = 48_000.0;
pub const SECONDS_PER_BEAT: f32 = 0.5; // 120 BPM

pub struct Clip {
    pub start: f32,
    pub len: f32,
    pub name: String,
    pub wave_offset: usize,
}

pub struct Track {
    pub name: String,
    pub color: Color32,
    pub audio: bool,
    pub mute: bool,
    pub solo: bool,
    pub clips: Vec<Clip>,
}

#[derive(Clone, Copy)]
pub struct Note {
    pub start: f32,
    pub len: f32,
    pub pitch: u8,
    pub velocity: u8,
}

pub const MAX_NOTE_LEN: f32 = 2.0;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1_u64 << 24) as f32
    }
}

const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0xE0, 0x6C, 0x5F),
    Color32::from_rgb(0xE8, 0xA0, 0x4C),
    Color32::from_rgb(0xD6, 0xC8, 0x5A),
    Color32::from_rgb(0x7F, 0xC0, 0x6A),
    Color32::from_rgb(0x4F, 0xB8, 0xB0),
    Color32::from_rgb(0x5B, 0x9B, 0xE6),
    Color32::from_rgb(0x95, 0x7D, 0xE0),
    Color32::from_rgb(0xD4, 0x75, 0xB8),
];

pub fn tracks() -> Vec<Track> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    (0..TRACKS)
        .map(|t| Track {
            name: format!("Track {}", t + 1),
            color: PALETTE[t % PALETTE.len()],
            audio: t % 2 == 0,
            mute: false,
            solo: false,
            clips: (0..CLIPS_PER_TRACK)
                .map(|k| Clip {
                    start: k as f32 * 4.0 + (rng.unit() * 4.0).floor() * 0.25,
                    len: 1.0 + (rng.unit() * 8.0).floor() * 0.25,
                    name: format!("Clip {}", k + 1),
                    wave_offset: (rng.next() % 1_000_000) as usize,
                })
                .collect(),
        })
        .collect()
}

pub fn notes() -> Vec<Note> {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut notes: Vec<Note> = (0..NOTES)
        .map(|_| Note {
            start: (rng.unit() * NOTE_SPAN_BEATS * 4.0).floor() / 4.0,
            len: 0.25 * (1.0 + (rng.unit() * 7.0).floor()),
            pitch: 24 + (rng.next() % 84) as u8,
            velocity: 1 + (rng.next() % 127) as u8,
        })
        .collect();
    notes.sort_by(|a, b| a.start.total_cmp(&b.start));
    notes
}

/// Min/max peaks of a synthetic drum-like signal at several resolutions,
/// so a column of any width needs only a few buckets.
pub struct Peaks {
    levels: Vec<(usize, Vec<(f32, f32)>)>,
    len: usize,
}

impl Peaks {
    pub fn synthetic() -> Self {
        let len = 1 << 21;
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let mut envelope = 0.0_f32;
        let signal: Vec<f32> = (0..len)
            .map(|i| {
                if i % 12_000 == 0 {
                    envelope = 0.5 + rng.unit() * 0.5;
                }
                envelope *= 0.9997;
                let noise = rng.unit() * 2.0 - 1.0;
                envelope * (0.6 * noise + 0.4 * (i as f32 * 0.03).sin())
            })
            .collect();
        let mut levels = Vec::new();
        let mut bucket = 16;
        while bucket <= 1 << 18 {
            let peaks = signal
                .chunks(bucket)
                .map(|c| {
                    c.iter()
                        .fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)))
                })
                .collect();
            levels.push((bucket, peaks));
            bucket *= 4;
        }
        Self { levels, len }
    }

    /// (min, max) over `count` samples starting at `start` (wrapping).
    pub fn range(&self, start: usize, count: usize) -> (f32, f32) {
        let (bucket, peaks) = self
            .levels
            .iter()
            .rev()
            .find(|(bucket, _)| *bucket <= count.max(16))
            .unwrap_or(&self.levels[0]);
        let first = (start % self.len) / bucket;
        let buckets = (count / bucket).max(1);
        (0..buckets).fold((f32::MAX, f32::MIN), |(lo, hi), i| {
            let (a, b) = peaks[(first + i) % peaks.len()];
            (lo.min(a), hi.max(b))
        })
    }
}
