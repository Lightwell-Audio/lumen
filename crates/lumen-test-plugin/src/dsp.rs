//! The test plugins' sound: exact and repeatable, so a golden render can be
//! compared sample for sample. Nothing here allocates after construction.

use crate::params::MAX_LATENCY;

pub const VOICES: usize = 16;
/// The most output buses the instrument plays on.
pub const MAX_BUSES: usize = 9;

#[derive(Clone, Copy, Default)]
struct Voice {
    on: bool,
    pitch: i16,
    channel: i16,
    note_id: i32,
    amp: f32,
    phase: f64,
    step: f64,
    rate: f64,
    /// Released, and fading by this much a sample (0 while held).
    fade: f32,
}

/// A square or saw voice bank. A note starts at phase 0 on exactly the
/// sample of its event and stops at once on its note-off (no release), so
/// where a note sounds is a plain function of its events. With a release
/// (the `release` fault), a note fades out in a straight line over it instead.
pub struct Voices {
    voices: [Voice; VOICES],
    next: usize,
    /// Seconds a released note takes to fade out; 0 stops it at once.
    release: f64,
}

impl Voices {
    pub fn new() -> Self {
        Self {
            voices: [Voice::default(); VOICES],
            next: 0,
            release: 0.0,
        }
    }

    pub fn set_release(&mut self, seconds: f64) {
        self.release = seconds.max(0.0);
    }

    pub fn silence(&mut self) {
        self.voices = [Voice::default(); VOICES];
    }

    pub fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.on).count()
    }

    /// Starts a note. When all voices are busy the oldest is taken.
    pub fn note_on(&mut self, channel: i16, pitch: i16, velocity: f32, note_id: i32, rate: f64) {
        let free = self.voices.iter().position(|v| !v.on).unwrap_or_else(|| {
            let taken = self.next;
            self.next = (self.next + 1) % VOICES;
            taken
        });
        let hz = 440.0 * 2_f64.powf((f64::from(pitch) - 69.0) / 12.0);
        self.voices[free] = Voice {
            on: true,
            pitch,
            channel,
            note_id,
            amp: velocity.clamp(0.0, 1.0),
            phase: 0.0,
            step: hz / rate,
            rate,
            fade: 0.0,
        };
    }

    /// Stops the note with this id, or (id -1) the first on this pitch and
    /// channel.
    pub fn note_off(&mut self, channel: i16, pitch: i16, note_id: i32) {
        let found = self.voices.iter_mut().find(|v| {
            v.on && v.fade == 0.0
                && if note_id >= 0 {
                    v.note_id == note_id
                } else {
                    v.pitch == pitch && v.channel == channel
                }
        });
        if let Some(v) = found {
            let samples = self.release * v.rate;
            if samples >= 1.0 && v.amp > 0.0 {
                v.fade = (f64::from(v.amp) / samples) as f32;
            } else {
                v.on = false;
            }
        }
    }

    /// The next sample of each bus's voices, scaled by `level`: a voice
    /// plays on the bus its pitch picks (`pitch % buses`) when `on` has that
    /// bus's bit, and on bus 0 when it has not.
    pub fn next_routed(
        &mut self,
        level: f32,
        saw: bool,
        buses: usize,
        on: u32,
    ) -> [f32; MAX_BUSES] {
        let mut sums = [0.0; MAX_BUSES];
        for v in self.voices.iter_mut().filter(|v| v.on) {
            let wave = if saw {
                (2.0 * v.phase - 1.0) as f32
            } else if v.phase < 0.5 {
                1.0
            } else {
                -1.0
            };
            let picked = v.pitch.rem_euclid(buses.max(1) as i16) as usize;
            let bus = if on & (1 << picked) != 0 { picked } else { 0 };
            sums[bus] += wave * v.amp;
            v.phase += v.step;
            if v.phase >= 1.0 {
                v.phase -= 1.0;
            }
            if v.fade > 0.0 {
                v.amp -= v.fade;
                if v.amp <= v.fade * 0.5 {
                    v.on = false;
                }
            }
        }
        sums.map(|s| s * level)
    }

    /// The next sample of the sum, scaled by `level`.
    pub fn next_sample(&mut self, level: f32, saw: bool) -> f32 {
        let mut sum = 0.0;
        for v in self.voices.iter_mut().filter(|v| v.on) {
            let wave = if saw {
                (2.0 * v.phase - 1.0) as f32
            } else if v.phase < 0.5 {
                1.0
            } else {
                -1.0
            };
            sum += wave * v.amp;
            v.phase += v.step;
            if v.phase >= 1.0 {
                v.phase -= 1.0;
            }
            if v.fade > 0.0 {
                v.amp -= v.fade;
                // Half a step's room for rounding in the sum of the steps.
                if v.amp <= v.fade * 0.5 {
                    v.on = false;
                }
            }
        }
        sum * level
    }
}

/// A stereo delay of 0 to `MAX_LATENCY` samples.
pub struct Delay {
    ring: [Vec<f32>; 2],
    at: usize,
}

impl Delay {
    pub fn new() -> Self {
        let len = MAX_LATENCY as usize + 1;
        Self {
            ring: [vec![0.0; len], vec![0.0; len]],
            at: 0,
        }
    }

    pub fn clear(&mut self) {
        for r in &mut self.ring {
            r.fill(0.0);
        }
    }

    /// Takes one stereo frame in and gives the frame from `delay` frames ago.
    pub fn step(&mut self, input: [f32; 2], delay: usize) -> [f32; 2] {
        let len = self.ring[0].len();
        let delay = delay.min(len - 1);
        let read = (self.at + len - delay) % len;
        let mut out = [0.0; 2];
        for (c, x) in input.into_iter().enumerate() {
            self.ring[c][self.at] = x;
            out[c] = self.ring[c][read];
        }
        self.at = (self.at + 1) % len;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_note_has_the_velocity_as_its_height() {
        let mut v = Voices::new();
        v.note_on(0, 69, 0.5, -1, 48_000.0);
        let first = v.next_sample(1.0, false);
        assert_eq!(first, 0.5);
        assert_eq!(v.next_sample(0.5, false), 0.25);
        v.note_off(0, 69, -1);
        assert_eq!(v.next_sample(1.0, false), 0.0);
        assert_eq!(v.sounding(), 0);
    }

    #[test]
    fn a_release_fades_a_note_out_in_a_straight_line() {
        let mut v = Voices::new();
        v.set_release(0.001);
        // A release of 48 samples at 48 kHz.
        v.note_on(0, 69, 0.5, -1, 48_000.0);
        assert_eq!(v.next_sample(1.0, false), 0.5);
        v.note_off(0, 69, -1);
        let tail: Vec<f32> = (0..48).map(|_| v.next_sample(1.0, false)).collect();
        assert_eq!(tail[0], 0.5, "the first sample after the release is whole");
        assert!((tail[24] - 0.25).abs() < 1e-3, "half way: {}", tail[24]);
        assert!(tail.windows(2).all(|w| w[1].abs() <= w[0].abs()));
        assert_eq!(v.sounding(), 0);
        assert_eq!(v.next_sample(1.0, false), 0.0);
    }

    #[test]
    fn note_off_by_id_ignores_pitch() {
        let mut v = Voices::new();
        v.note_on(0, 60, 1.0, 7, 48_000.0);
        v.note_on(0, 60, 1.0, 8, 48_000.0);
        v.note_off(0, 99, 8);
        assert_eq!(v.sounding(), 1);
        v.note_off(0, 60, 7);
        assert_eq!(v.sounding(), 0);
    }

    #[test]
    fn the_oldest_voice_is_taken_when_all_are_busy() {
        let mut v = Voices::new();
        for p in 0..VOICES as i16 + 1 {
            v.note_on(0, p, 1.0, -1, 48_000.0);
        }
        assert_eq!(v.sounding(), VOICES);
    }

    #[test]
    fn square_flips_at_half_a_period() {
        let mut v = Voices::new();
        // 440 Hz at 48 kHz: a period of about 109 samples.
        v.note_on(0, 69, 1.0, -1, 48_000.0);
        let wave: Vec<f32> = (0..109).map(|_| v.next_sample(1.0, false)).collect();
        assert_eq!(wave[0], 1.0);
        assert_eq!(wave[54], 1.0);
        assert_eq!(wave[55], -1.0);
        assert_eq!(wave[108], -1.0);
    }

    #[test]
    fn delay_returns_the_frame_from_n_ago() {
        let mut d = Delay::new();
        let out: Vec<f32> = (1..=6).map(|i| d.step([i as f32; 2], 3)[0]).collect();
        assert_eq!(out, [0.0, 0.0, 0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn zero_delay_is_a_pass_through() {
        let mut d = Delay::new();
        assert_eq!(d.step([0.25, -0.5], 0), [0.25, -0.5]);
    }

    #[test]
    fn the_longest_delay_works_across_the_ring_wrap() {
        let mut d = Delay::new();
        let n = MAX_LATENCY as usize;
        let mut first = None;
        for i in 0..n * 2 + 5 {
            let out = d.step([if i == 0 { 1.0 } else { 0.0 }; 2], n)[0];
            if out != 0.0 {
                first = Some(i);
                break;
            }
        }
        assert_eq!(first, Some(n));
    }
}
