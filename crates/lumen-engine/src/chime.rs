use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};

pub const NOTES: usize = 3;
pub const NOTE_SECONDS: f64 = 0.5;
pub const LEVEL: f32 = 0.125_9;
const ATTACK_SECONDS: f32 = 0.005;
const DECAY_SECONDS: f32 = 0.12;
const OVERTONE_DECAY_SECONDS: f32 = 0.05;
const OVERTONE: f32 = 0.25;
const RELEASE_SECONDS: f32 = 0.02;
const PITCHES: [f32; NOTES] = [659.255, 830.609, 987.767];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sounding {
    Silent,
    Left,
    Right,
    Both,
}

impl Sounding {
    fn of_note(note: usize) -> Self {
        match note {
            0 => Self::Left,
            1 => Self::Right,
            2 => Self::Both,
            _ => Self::Silent,
        }
    }

    fn from_u8(n: u8) -> Self {
        match n {
            1 => Self::Left,
            2 => Self::Right,
            3 => Self::Both,
            _ => Self::Silent,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Self::Silent => 0,
            Self::Left => 1,
            Self::Right => 2,
            Self::Both => 3,
        }
    }

    pub fn left(self) -> bool {
        matches!(self, Self::Left | Self::Both)
    }

    pub fn right(self) -> bool {
        matches!(self, Self::Right | Self::Both)
    }
}

const STOP: u64 = 0;
const PLAY_PAIR: u64 = 1;
const PLAY_MONO: u64 = 2;
const ASK_BITS: u32 = 2;

#[derive(Debug, Default)]
struct Shared {
    asked: AtomicU64,
    sounding: AtomicU8,
    peaks: [AtomicU32; 2],
}

#[derive(Debug, Clone)]
pub struct ChimeHandle {
    shared: Arc<Shared>,
}

impl ChimeHandle {
    pub fn play(&self, pair: bool) {
        self.ask(if pair { PLAY_PAIR } else { PLAY_MONO });
    }

    pub fn stop(&self) {
        self.ask(STOP);
    }

    fn ask(&self, what: u64) {
        let count = (self.shared.asked.load(Ordering::Relaxed) >> ASK_BITS) + 1;
        self.shared
            .asked
            .store((count << ASK_BITS) | what, Ordering::Release);
    }

    pub fn sounding(&self) -> Sounding {
        Sounding::from_u8(self.shared.sounding.load(Ordering::Acquire))
    }

    pub fn peaks(&self) -> [f32; 2] {
        self.shared
            .peaks
            .each_ref()
            .map(|p| f32::from_bits(p.load(Ordering::Relaxed)))
    }
}

#[derive(Debug, Clone, Copy)]
struct Playing {
    rate: u32,
    pair: bool,
    at: usize,
}

#[derive(Debug)]
pub struct ChimePlayer {
    shared: Arc<Shared>,
    seen: u64,
    playing: Option<Playing>,
}

impl ChimePlayer {
    pub fn process(
        &mut self,
        rate: u32,
        outputs: &mut [f32],
        output_channels: usize,
        frames: usize,
    ) {
        let asked = self.shared.asked.load(Ordering::Acquire);
        if asked != self.seen {
            self.seen = asked;
            self.playing = match asked & ((1 << ASK_BITS) - 1) {
                PLAY_PAIR | PLAY_MONO if rate > 0 => Some(Playing {
                    rate,
                    pair: asked & PLAY_PAIR != 0,
                    at: 0,
                }),
                _ => None,
            };
            if self.playing.is_none() {
                self.publish(Sounding::Silent, [0.0; 2]);
            }
        }
        let Some(playing) = self.playing.as_mut() else {
            return;
        };
        if playing.rate != rate || output_channels == 0 || outputs.len() < output_channels * frames
        {
            self.playing = None;
            self.publish(Sounding::Silent, [0.0; 2]);
            return;
        }
        let pair = playing.pair && output_channels >= 2;
        let mut peaks = [0.0_f32; 2];
        for i in 0..frames {
            let (left, right) = sample(playing.at + i, rate);
            peaks[0] = peaks[0].max(left.abs());
            peaks[1] = peaks[1].max(right.abs());
            if pair {
                outputs[i] += left;
                outputs[frames + i] += right;
            } else {
                outputs[i] += if left != 0.0 { left } else { right };
            }
        }
        let at = playing.at;
        playing.at += frames;
        let sounding = Sounding::of_note(at / note_frames(rate));
        if at >= total_frames(rate) {
            self.playing = None;
            self.publish(Sounding::Silent, [0.0; 2]);
        } else {
            self.publish(sounding, peaks);
        }
    }

    fn publish(&self, sounding: Sounding, peaks: [f32; 2]) {
        self.shared
            .sounding
            .store(sounding.to_u8(), Ordering::Release);
        for (to, peak) in self.shared.peaks.iter().zip(peaks) {
            to.store(peak.to_bits(), Ordering::Relaxed);
        }
    }
}

pub fn note_frames(rate: u32) -> usize {
    (NOTE_SECONDS * f64::from(rate.max(1))).round() as usize
}

pub fn total_frames(rate: u32) -> usize {
    NOTES * note_frames(rate)
}

pub fn sample(n: usize, rate: u32) -> (f32, f32) {
    let length = note_frames(rate);
    let note = n / length;
    let Some(&pitch) = PITCHES.get(note) else {
        return (0.0, 0.0);
    };
    let rate = rate.max(1) as f32;
    let t = (n - note * length) as f32 / rate;
    let left_in_note = (length - (n - note * length)) as f32 / rate;
    let envelope = (t / ATTACK_SECONDS).min(1.0)
        * (-t / DECAY_SECONDS).exp()
        * (left_in_note / RELEASE_SECONDS).min(1.0);
    let tone = (TAU * pitch * t).sin()
        + OVERTONE * (-t / OVERTONE_DECAY_SECONDS).exp() * (TAU * 2.0 * pitch * t).sin();
    let value = LEVEL / (1.0 + OVERTONE) * envelope * tone;
    let sounding = Sounding::of_note(note);
    (
        if sounding.left() { value } else { 0.0 },
        if sounding.right() { value } else { 0.0 },
    )
}

pub fn chime() -> (ChimeHandle, ChimePlayer) {
    let shared = Arc::new(Shared::default());
    (
        ChimeHandle {
            shared: Arc::clone(&shared),
        },
        ChimePlayer {
            shared,
            seen: 0,
            playing: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn play(
        player: &mut ChimePlayer,
        handle: &ChimeHandle,
        blocks: usize,
        frames: usize,
    ) -> (Vec<f32>, Vec<f32>, Vec<Sounding>) {
        let (mut left, mut right, mut heard) = (Vec::new(), Vec::new(), Vec::new());
        let mut out = vec![0.0; 2 * frames];
        for _ in 0..blocks {
            out.fill(0.0);
            player.process(RATE, &mut out, 2, frames);
            left.extend_from_slice(&out[..frames]);
            right.extend_from_slice(&out[frames..]);
            heard.push(handle.sounding());
        }
        (left, right, heard)
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |p, s| p.max(s.abs()))
    }

    #[test]
    fn left_then_right_then_both_then_silence() {
        let (handle, mut player) = chime();
        handle.play(true);
        let note = note_frames(RATE);
        let (left, right, _) = play(&mut player, &handle, 2 * total_frames(RATE) / 64, 64);
        assert!(peak(&left[..note]) > 0.05);
        assert_eq!(peak(&right[..note]), 0.0);
        assert_eq!(peak(&left[note..2 * note]), 0.0);
        assert!(peak(&right[note..2 * note]) > 0.05);
        assert!(peak(&left[2 * note..3 * note]) > 0.05);
        assert_eq!(&left[2 * note..3 * note], &right[2 * note..3 * note]);
        assert_eq!(peak(&left[3 * note..]), 0.0);
        assert_eq!(peak(&right[3 * note..]), 0.0);
        assert_eq!(handle.sounding(), Sounding::Silent);
    }

    #[test]
    fn it_never_passes_minus_18_dbfs_and_starts_and_ends_each_note_at_silence() {
        let note = note_frames(RATE);
        let all: Vec<f32> = (0..total_frames(RATE)).map(|n| sample(n, RATE).0).collect();
        assert!(peak(&all) <= LEVEL, "peak {}", peak(&all));
        assert!(peak(&all) > LEVEL * 0.7, "peak {}", peak(&all));
        for k in 0..NOTES {
            assert_eq!(sample(k * note, RATE), (0.0, 0.0), "note {k} starts at 0");
            let last = sample((k + 1) * note - 1, RATE);
            assert!(
                last.0.abs() < 1e-4 && last.1.abs() < 1e-4,
                "note {k} ends near 0"
            );
        }
    }

    #[test]
    fn the_lights_follow_the_frames() {
        let (handle, mut player) = chime();
        handle.play(true);
        let note = note_frames(RATE);
        let frames = 480;
        assert_eq!(note % frames, 0);
        let (_, _, heard) = play(&mut player, &handle, 3 * note / frames + 2, frames);
        let per_note = note / frames;
        assert!(heard[..per_note].iter().all(|&s| s == Sounding::Left));
        assert!(
            heard[per_note..2 * per_note]
                .iter()
                .all(|&s| s == Sounding::Right)
        );
        assert!(
            heard[2 * per_note..3 * per_note]
                .iter()
                .all(|&s| s == Sounding::Both)
        );
        assert!(heard[3 * per_note..].iter().all(|&s| s == Sounding::Silent));
        assert_eq!(handle.peaks(), [0.0, 0.0]);
    }

    #[test]
    fn playing_again_restarts_and_stop_silences() {
        let (handle, mut player) = chime();
        handle.play(true);
        let note = note_frames(RATE);
        play(&mut player, &handle, (note + 4_800) / 64, 64);
        assert_eq!(handle.sounding(), Sounding::Right);
        handle.play(true);
        let (left, right, _) = play(&mut player, &handle, 10, 64);
        assert!(peak(&left) > 0.0, "restarted on the left");
        assert_eq!(peak(&right), 0.0);
        assert_eq!(handle.sounding(), Sounding::Left);
        handle.stop();
        let (left, right, _) = play(&mut player, &handle, 10, 64);
        assert_eq!(peak(&left) + peak(&right), 0.0);
        assert_eq!(handle.sounding(), Sounding::Silent);
    }

    #[test]
    fn a_rate_change_ends_it() {
        let (handle, mut player) = chime();
        handle.play(true);
        let mut out = vec![0.0; 128];
        player.process(RATE, &mut out, 2, 64);
        assert_eq!(handle.sounding(), Sounding::Left);
        out.fill(0.0);
        player.process(44_100, &mut out, 2, 64);
        assert_eq!(peak(&out), 0.0);
        assert_eq!(handle.sounding(), Sounding::Silent);
    }

    #[test]
    fn it_adds_to_what_the_engine_played_and_mono_hears_every_note() {
        let (handle, mut player) = chime();
        handle.play(false);
        let note = note_frames(RATE);
        let mut out = vec![0.25; total_frames(RATE)];
        player.process(RATE, &mut out, 1, total_frames(RATE));
        for k in 0..NOTES {
            let part = &out[k * note..(k + 1) * note];
            assert!(
                part.iter().any(|s| (s - 0.25).abs() > 0.05),
                "note {k} heard"
            );
        }
    }
}
