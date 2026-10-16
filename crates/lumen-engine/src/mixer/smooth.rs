//! Smoothed gains: every mixer level glides to its new value in a straight
//! line over [`GLIDE_SECONDS`], frame by frame, so fader and pan drags,
//! mutes and polarity flips never step or click. The glide is counted in
//! frames, so it sounds the same at every block size.

/// How long a change takes: 20 ms, as clip levels glide (ADR-0025).
pub const GLIDE_SECONDS: f32 = 0.02;

#[derive(Debug, Clone, Copy)]
pub struct Smoothed {
    value: f32,
    target: f32,
    /// How far the value moves per frame; NaN until the next render works
    /// it out from the device rate.
    step: f32,
}

impl Smoothed {
    /// Settled at `value`.
    pub fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            step: 0.0,
        }
    }

    #[cfg(test)]
    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Glides from `from`'s value to this one's target.
    pub fn carry(&mut self, from: &Smoothed) {
        self.value = from.value;
        self.step = f32::NAN;
    }

    /// Glides to `target` from the value it has now.
    pub fn set(&mut self, target: f32) {
        if target != self.target {
            self.target = target;
            self.step = f32::NAN;
        }
    }

    /// Jumps to the target (after a transport jump, which is crossfaded).
    pub fn settle(&mut self) {
        self.value = self.target;
    }

    pub fn settled(&self) -> bool {
        self.value == self.target
    }

    /// Fills `out` with the gain for each of its frames at `rate`, and
    /// moves on by that many frames when `advance` (a tail rendered ahead
    /// of a jump reads the gains without moving them).
    #[inline]
    pub fn fill(&mut self, out: &mut [f32], rate: u32, advance: bool) {
        if self.settled() {
            out.fill(self.value);
            return;
        }
        if self.step.is_nan() {
            let frames = (rate as f32 * GLIDE_SECONDS).max(1.0);
            self.step = (self.target - self.value).abs() / frames;
        }
        let (target, step) = (self.target, self.step);
        let mut value = self.value;
        for gain in out.iter_mut() {
            value = if (target - value).abs() <= step {
                target
            } else {
                value + step.copysign(target - value)
            };
            *gain = value;
        }
        if advance {
            self.value = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_glides_in_a_straight_line_over_20_ms() {
        let mut gain = Smoothed::new(1.0);
        let mut next = Smoothed::new(0.0);
        next.carry(&gain);
        gain = next;
        // 20 ms at 1 kHz: 20 frames.
        let mut out = [0.0; 25];
        gain.fill(&mut out, 1_000, true);
        assert!((out[0] - 0.95).abs() < 1e-6);
        assert!((out[9] - 0.5).abs() < 1e-5);
        assert_eq!(out[19], 0.0);
        assert_eq!(out[24], 0.0);
        assert!(gain.settled());
    }

    #[test]
    fn the_glide_is_the_same_in_any_block_size() {
        let start = || {
            let mut gain = Smoothed::new(0.25);
            gain.carry(&Smoothed::new(1.0));
            gain
        };
        let mut whole = [0.0; 1_000];
        start().fill(&mut whole, 48_000, true);
        let mut parts = Vec::new();
        let mut gain = start();
        for n in [7, 128, 1, 300, 564] {
            let mut out = vec![0.0; n];
            gain.fill(&mut out, 48_000, true);
            parts.extend(out);
        }
        assert_eq!(parts, whole);
    }

    #[test]
    fn a_tail_reads_without_moving() {
        let mut gain = Smoothed::new(0.0);
        gain.carry(&Smoothed::new(1.0));
        let mut out = [0.0; 4];
        gain.fill(&mut out, 1_000, false);
        assert_eq!(gain.value(), 1.0);
        gain.fill(&mut out, 1_000, true);
        assert!(gain.value() < 1.0);
    }
}
