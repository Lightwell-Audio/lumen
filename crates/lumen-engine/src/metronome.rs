use std::f32::consts::TAU;

use lumen_core::time::Ticks;

use crate::timeline::Timeline;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickSound {
    #[default]
    Beep,
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
    pub volume: f32,
    pub sound: ClickSound,
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

#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    age: u32,
    length: u32,
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

const CLICK: f32 = 0.050;
const TAIL: f32 = 0.015;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CountIn {
    pub beat: i64,
    pub beats: u32,
    pub per_bar: u32,
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

#[derive(Debug, Default)]
pub(crate) struct TestTone {
    pub on: bool,
    phase: f32,
    gain: f32,
}

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
        for sound in ClickSound::ALL {
            for accent in [false, true] {
                let out = click(sound, accent);
                let peak = out.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
                let last = out.last().copied().unwrap_or(0.0).abs();
                assert!(last < peak * 1e-4, "{sound:?}: ends at {last}, peak {peak}");
                let tail = out[out.len() - 240..]
                    .iter()
                    .fold(0.0_f32, |m, v| m.max(v.abs()));
                assert!(tail < peak * 1e-2, "{sound:?}: tail {tail}, peak {peak}");
            }
        }
    }
}
