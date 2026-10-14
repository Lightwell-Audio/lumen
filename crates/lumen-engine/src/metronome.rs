//! The metronome and the test tone (M2 slice 7). Both are synthesized, so
//! there are no sample files or licences to manage.
//!
//! Clicks start on the exact sample of each beat, with the downbeat
//! accented (higher and louder). A click that is still ringing carries on
//! across loop wraps, locates and stops: only new beats start clicks, so the
//! metronome never cuts itself off.
//!
//! The count-in clicks (M5 slice 6) sound whether or not the metronome is
//! on; "only while recording" is the app's, which switches the metronome.

use std::f32::consts::TAU;

use lumen_core::time::Ticks;

use crate::timeline::Timeline;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickSound {
    /// A short sine blip (1 kHz, 1.5 kHz on the downbeat).
    #[default]
    Beep,
    /// A woody tick: two fast-decaying partials and a breath of noise.
    Wood,
}

impl ClickSound {
    pub const ALL: [ClickSound; 2] = [ClickSound::Beep, ClickSound::Wood];

    pub fn name(self) -> &'static str {
        match self {
            ClickSound::Beep => "Beep",
            ClickSound::Wood => "Wood",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetronomeSettings {
    pub enabled: bool,
    /// Linear gain, 0.0–1.0.
    pub volume: f32,
    pub sound: ClickSound,
    /// The first device channel it plays on (M4 slice 4): 0 for the main
    /// pair, the cue pair's first channel for the cue. A pair the device
    /// does not have falls back to the main one.
    pub channel: u16,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: 0.5,
            sound: ClickSound::Beep,
            channel: 0,
        }
    }
}

/// One click in progress.
#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    /// Frames played so far.
    age: u32,
    /// Frames the click lasts.
    length: u32,
    /// Frames at the end that fade to zero.
    fade: u32,
    accent: bool,
    sound: ClickSound,
    gain: f32,
    noise: u32,
}

impl Voice {
    fn active(&self) -> bool {
        self.age < self.length
    }

    fn next(&mut self, rate: f32) -> f32 {
        let t = self.age as f32 / rate;
        self.age += 1;
        // The last `TAIL` fades to exactly zero (smoothstep, cheap enough
        // per sample), so a click never stops short with sound in it.
        let left = self.length - self.age;
        let tail = if left < self.fade {
            let x = left as f32 / self.fade as f32;
            x * x * (3.0 - 2.0 * x)
        } else {
            1.0
        };
        let value = match self.sound {
            ClickSound::Beep => {
                let hz = if self.accent { 1_500.0 } else { 1_000.0 };
                let attack = (t / 0.001).min(1.0);
                (TAU * hz * t).sin() * attack * (-t / 0.012).exp()
            }
            ClickSound::Wood => {
                let (a, b) = if self.accent {
                    (2_400.0, 3_600.0)
                } else {
                    (1_800.0, 2_700.0)
                };
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 17;
                self.noise ^= self.noise << 5;
                let noise = (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
                (TAU * a * t).sin() * (-t / 0.008).exp() * 0.7
                    + (TAU * b * t).sin() * (-t / 0.004).exp() * 0.4
                    + noise * (-t / 0.0015).exp() * 0.3
            }
        };
        value * self.gain * tail
    }
}

/// How long a click lasts.
const CLICK: f32 = 0.050;
/// The fade at the end of a click; the beep is already 26 dB down where it
/// starts, so the sound is unchanged.
const TAIL: f32 = 0.015;

/// Where the metronome writes in a channel-major buffer: `count` frames from
/// `offset`, for `channels` channels that are `stride` frames apart.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Window {
    pub stride: usize,
    pub channels: usize,
    pub offset: usize,
    pub count: usize,
}

#[derive(Debug, Default)]
pub(crate) struct Metronome {
    pub settings: MetronomeSettings,
    voice: Voice,
}

/// The beats before recording starts: `beats` clicks `beat` frames apart,
/// the first of every bar (`per_bar` beats) accented. The song waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CountIn {
    pub beat: i64,
    pub beats: u32,
    pub per_bar: u32,
    /// Frames counted so far.
    pub done: i64,
}

impl CountIn {
    pub fn total(&self) -> i64 {
        self.beat * i64::from(self.beats)
    }

    pub fn left(&self) -> i64 {
        self.total() - self.done
    }
}

/// Downbeats are at full level; other beats a little quieter.
const ACCENT_GAIN: f32 = 1.0;
const BEAT_GAIN: f32 = 0.6;

impl Metronome {
    fn start(&mut self, accent: bool, rate: u32) {
        self.voice = Voice {
            age: 0,
            length: (rate as f32 * CLICK) as u32,
            fade: (rate as f32 * TAIL) as u32,
            accent,
            sound: self.settings.sound,
            gain: self.settings.volume * if accent { ACCENT_GAIN } else { BEAT_GAIN },
            noise: 0x9E37_79B9,
        };
    }

    /// Adds the ringing click to `count` frames from `offset` of channel-major
    /// `data`.
    fn ring(
        &mut self,
        data: &mut [f32],
        stride: usize,
        channels: usize,
        offset: usize,
        count: usize,
        rate: f32,
    ) {
        let first = usize::from(self.settings.channel);
        let first = if first + 2 <= channels { first } else { 0 };
        for frame in offset..offset + count {
            if !self.voice.active() {
                return;
            }
            let value = self.voice.next(rate);
            for channel in first..(first + 2).min(channels) {
                data[channel * stride + frame] += value;
            }
        }
    }

    /// Clicks for the timeline samples `start .. start + window.count`,
    /// written to frames `window.offset ..` of `data`. Only new beats start
    /// clicks; a ringing one carries on.
    pub fn render(
        &mut self,
        timeline: &Timeline,
        rate: u32,
        start: i64,
        data: &mut [f32],
        window: Window,
    ) {
        let Window {
            stride,
            channels,
            offset,
            count,
        } = window;
        let rate_f = rate as f32;
        if !self.settings.enabled {
            self.ring(data, stride, channels, offset, count, rate_f);
            return;
        }
        let end = start + count as i64;
        let mut done = 0;
        // Start a quarter early so a beat that rounds onto `start` is kept.
        let mut beat = timeline
            .meter
            .next_beat(timeline.ticks_at(start, rate) - Ticks::quarters(1));
        loop {
            let at = timeline.sample_at(beat, rate);
            if at >= end {
                break;
            }
            if at >= start {
                let k = (at - start) as usize;
                self.ring(data, stride, channels, offset + done, k - done, rate_f);
                done = k;
                self.start(timeline.meter.bbt_at(beat).beat == 0, rate);
            }
            beat = timeline.meter.next_beat(beat + Ticks(1));
        }
        self.ring(data, stride, channels, offset + done, count - done, rate_f);
    }

    /// Clicks for the next `window.count` frames of a count-in (or the frames
    /// it has left, if fewer), and moves it on. Returns the frames used.
    pub fn count(
        &mut self,
        count_in: &mut CountIn,
        rate: u32,
        data: &mut [f32],
        window: Window,
    ) -> usize {
        let Window {
            stride,
            channels,
            offset,
            count,
        } = window;
        let n = (count as i64).min(count_in.left()).max(0) as usize;
        let beat = count_in.beat.max(1);
        let mut at = 0;
        while at < n {
            let into = count_in.done + at as i64;
            let to_next = beat - into % beat;
            if into % beat == 0 {
                let index = (into / beat) as u32;
                self.start(index.is_multiple_of(count_in.per_bar.max(1)), rate);
            }
            let run = (n - at).min(to_next as usize);
            self.ring(data, stride, channels, offset + at, run, rate as f32);
            at += run;
        }
        count_in.done += n as i64;
        n
    }

    /// Lets a ringing click finish while stopped.
    pub fn ring_out(
        &mut self,
        data: &mut [f32],
        stride: usize,
        channels: usize,
        frames: usize,
        rate: u32,
    ) {
        self.ring(data, stride, channels, 0, frames, rate as f32);
    }

    pub fn silence(&mut self) {
        self.voice = Voice::default();
    }
}

/// The test tone: 1 kHz at −20 dBFS on every output, playing or not, with
/// 5 ms ramps on and off.
#[derive(Debug, Default)]
pub(crate) struct TestTone {
    pub on: bool,
    phase: f32,
    gain: f32,
}

/// −20 dBFS.
const TONE_LEVEL: f32 = 0.1;

impl TestTone {
    pub fn render(
        &mut self,
        data: &mut [f32],
        stride: usize,
        channels: usize,
        frames: usize,
        rate: u32,
    ) {
        let target = if self.on { 1.0 } else { 0.0 };
        if target == 0.0 && self.gain == 0.0 {
            return;
        }
        let step = 1_000.0 / rate as f32;
        let ramp = 1.0 / (rate as f32 * 0.005);
        for frame in 0..frames {
            self.gain = if target > self.gain {
                (self.gain + ramp).min(1.0)
            } else {
                (self.gain - ramp).max(0.0)
            };
            let value = (TAU * self.phase).sin() * TONE_LEVEL * self.gain;
            self.phase = (self.phase + step).fract();
            for channel in 0..channels {
                data[channel * stride + frame] += value;
            }
        }
    }

    pub fn silence(&mut self) {
        self.on = false;
        self.gain = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One whole click, sample by sample.
    fn click(sound: ClickSound, accent: bool) -> Vec<f32> {
        let mut metronome = Metronome::default();
        metronome.settings.sound = sound;
        metronome.settings.volume = 1.0;
        metronome.start(accent, 48_000);
        let mut out = Vec::new();
        while metronome.voice.active() {
            out.push(metronome.voice.next(48_000.0));
        }
        out
    }

    #[test]
    fn every_click_fades_to_silence_instead_of_stopping_short() {
        // Owner: a little static on the metronome. Cut off at a fixed
        // length, the beep still had 1.5% of its level and stepped to
        // silence, ticking after every click.
        for sound in ClickSound::ALL {
            for accent in [false, true] {
                let out = click(sound, accent);
                let peak = out.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
                let last = out.last().copied().unwrap_or(0.0).abs();
                assert!(last < peak * 1e-4, "{sound:?}: ends at {last}, peak {peak}");
                // The final 5 ms are at least 40 dB down, fading out.
                let tail = out[out.len() - 240..]
                    .iter()
                    .fold(0.0_f32, |m, v| m.max(v.abs()));
                assert!(tail < peak * 1e-2, "{sound:?}: tail {tail}, peak {peak}");
            }
        }
    }
}
