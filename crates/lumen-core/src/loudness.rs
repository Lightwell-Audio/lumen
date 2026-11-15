pub const SILENT: f64 = f64::NEG_INFINITY;

#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighting {
    pub fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate.max(1));
        let pi = std::f64::consts::PI;

        let f0 = 1_681.974_450_955_533;
        let gain_db = 3.999_843_853_973_347;
        let q = 0.707_175_236_955_419_6;
        let k = (pi * f0 / rate).tan();
        let vh = 10f64.powf(gain_db / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad {
            b: [
                (vh + vb * k / q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / q + k * k) / a0,
            ],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            ..Biquad::default()
        };

        let f0 = 38.135_470_876_024_44;
        let q = 0.500_327_037_323_877_3;
        let k = (pi * f0 / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        let high_pass = Biquad {
            b: [1.0, -2.0, 1.0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            ..Biquad::default()
        };
        Self { shelf, high_pass }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f64 {
        self.high_pass.process(self.shelf.process(f64::from(x)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Blocks {
    filters: [KWeighting; 2],
    rate: u32,
    length: u32,
    frames: u32,
    sum: f64,
}

impl Blocks {
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(10);
        Self {
            filters: [KWeighting::new(rate); 2],
            rate,
            length: rate / 10,
            frames: 0,
            sum: 0.0,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub fn add(&mut self, left: &[f32], right: &[f32], mut done: impl FnMut(f32)) {
        let [l, r] = &mut self.filters;
        for (&a, &b) in left.iter().zip(right) {
            let (a, b) = (l.process(a), r.process(b));
            self.sum += a * a + b * b;
            self.frames += 1;
            if self.frames == self.length {
                done((self.sum / f64::from(self.length)) as f32);
                self.sum = 0.0;
                self.frames = 0;
            }
        }
    }
}

pub fn lufs(mean_square: f64) -> f64 {
    if mean_square > 0.0 {
        -0.691 + 10.0 * mean_square.log10()
    } else {
        SILENT
    }
}

const MOMENTARY: usize = 4;
const SHORT_TERM: usize = 30;
const ABSOLUTE_GATE: f64 = -70.0;
const RELATIVE_GATE: f64 = -10.0;

#[derive(Debug, Clone, Default)]
pub struct Loudness {
    recent: std::collections::VecDeque<f64>,
    gating: Vec<f64>,
    fresh: usize,
    integrated: f64,
}

impl Loudness {
    pub fn new() -> Self {
        Self {
            integrated: SILENT,
            ..Self::default()
        }
    }

    pub fn reset(&mut self) {
        self.gating.clear();
        self.fresh = 0;
        self.integrated = SILENT;
    }

    pub fn push(&mut self, mean_square: f32) {
        if self.recent.len() == SHORT_TERM {
            self.recent.pop_front();
        }
        self.recent.push_back(f64::from(mean_square.max(0.0)));
        self.fresh += 1;
        if self.fresh >= MOMENTARY {
            let block = self.window(MOMENTARY);
            if lufs(block) > ABSOLUTE_GATE {
                self.gating.push(block);
                self.integrated = integrate(&self.gating);
            }
        }
    }

    fn window(&self, blocks: usize) -> f64 {
        if self.recent.len() < blocks {
            return 0.0;
        }
        self.recent.iter().rev().take(blocks).sum::<f64>() / blocks as f64
    }

    pub fn momentary(&self) -> f64 {
        lufs(self.window(MOMENTARY))
    }

    pub fn short_term(&self) -> f64 {
        lufs(self.window(SHORT_TERM))
    }

    pub fn integrated(&self) -> f64 {
        self.integrated
    }
}

fn integrate(blocks: &[f64]) -> f64 {
    if blocks.is_empty() {
        return SILENT;
    }
    let mean = blocks.iter().sum::<f64>() / blocks.len() as f64;
    let gate = lufs(mean) + RELATIVE_GATE;
    let (sum, count) = blocks
        .iter()
        .filter(|&&z| lufs(z) > gate)
        .fold((0.0, 0usize), |(s, n), &z| (s + z, n + 1));
    if count == 0 {
        SILENT
    } else {
        lufs(sum / count as f64)
    }
}

const PHASES: usize = 4;
const TAPS: usize = 13;

fn interpolator() -> [[f32; TAPS]; PHASES] {
    let length = PHASES * (TAPS - 1) + 1;
    let centre = (length - 1) as f64 / 2.0;
    let beta = 8.0;
    let i0 = |x: f64| {
        let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
        while term > 1e-12 * sum {
            term *= (x / (2.0 * k)).powi(2);
            sum += term;
            k += 1.0;
        }
        sum
    };
    let mut phases = [[0.0f32; TAPS]; PHASES];
    for (p, phase) in phases.iter_mut().enumerate() {
        let mut taps = [0.0f64; TAPS];
        for (j, tap) in taps.iter_mut().enumerate() {
            let n = j * PHASES + p;
            if n >= length {
                continue;
            }
            let x = (n as f64 - centre) / PHASES as f64;
            let sinc = if x == 0.0 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            let r = (n as f64 - centre) / centre;
            *tap = sinc * i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0(beta);
        }
        let sum: f64 = taps.iter().sum();
        for (out, tap) in phase.iter_mut().zip(taps) {
            *out = (tap / sum) as f32;
        }
    }
    phases
}

#[derive(Debug, Clone, Copy)]
pub struct TruePeak {
    phases: [[f32; TAPS]; PHASES],
    history: [f32; 2 * TAPS],
    at: usize,
}

impl Default for TruePeak {
    fn default() -> Self {
        Self::new()
    }
}

impl TruePeak {
    pub fn new() -> Self {
        let mut phases = interpolator();
        for phase in &mut phases {
            phase.reverse();
        }
        Self {
            phases,
            history: [0.0; 2 * TAPS],
            at: 0,
        }
    }

    pub fn process(&mut self, samples: &[f32]) -> f32 {
        let mut peak = 0.0f32;
        for &x in samples {
            self.at = if self.at + 1 == TAPS { 0 } else { self.at + 1 };
            self.history[self.at] = x;
            self.history[self.at + TAPS] = x;
            let start = self.at + 1;
            let window: &[f32; TAPS] = self
                .history
                .get(start..start + TAPS)
                .and_then(|w| w.try_into().ok())
                .unwrap_or(&[0.0; TAPS]);
            for phase in &self.phases {
                let y: f32 = phase.iter().zip(window).map(|(h, s)| h * s).sum();
                peak = peak.max(y.abs());
            }
        }
        peak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(frequency: f64, dbfs: f64, seconds: f64, phase: f64) -> Vec<f32> {
        let amplitude = 10f64.powf(dbfs / 20.0);
        (0..(seconds * f64::from(RATE)) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                (amplitude * (2.0 * std::f64::consts::PI * frequency * t + phase).sin()) as f32
            })
            .collect()
    }

    fn measure(parts: &[(f64, f64)]) -> Loudness {
        let mut blocks = Blocks::new(RATE);
        let mut loudness = Loudness::new();
        for &(dbfs, seconds) in parts {
            let tone = sine(1_000.0, dbfs, seconds, 0.0);
            blocks.add(&tone, &tone, |z| loudness.push(z));
        }
        loudness
    }

    fn near(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn a_tone_at_minus_23_dbfs_reads_minus_23_lufs() {
        let l = measure(&[(-23.0, 20.0)]);
        assert!(near(l.momentary(), -23.0, 0.1), "{}", l.momentary());
        assert!(near(l.short_term(), -23.0, 0.1), "{}", l.short_term());
        assert!(near(l.integrated(), -23.0, 0.1), "{}", l.integrated());
    }

    #[test]
    fn a_tone_at_minus_33_dbfs_reads_minus_33_lufs() {
        let l = measure(&[(-33.0, 20.0)]);
        assert!(near(l.integrated(), -33.0, 0.1), "{}", l.integrated());
    }

    #[test]
    fn the_relative_gate_ignores_quiet_passages() {
        let l = measure(&[(-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0)]);
        assert!(near(l.integrated(), -23.0, 0.1), "{}", l.integrated());
    }

    #[test]
    fn the_absolute_gate_ignores_near_silence() {
        let l = measure(&[
            (-72.0, 10.0),
            (-36.0, 10.0),
            (-23.0, 60.0),
            (-36.0, 10.0),
            (-72.0, 10.0),
        ]);
        assert!(near(l.integrated(), -23.0, 0.1), "{}", l.integrated());
    }

    #[test]
    fn louder_passages_raise_the_integrated_loudness() {
        let l = measure(&[(-26.0, 20.0), (-20.0, 20.1), (-26.0, 20.0)]);
        assert!(near(l.integrated(), -23.0, 0.1), "{}", l.integrated());
    }

    #[test]
    fn the_k_weighting_matches_the_standard_at_48_khz() {
        let k = KWeighting::new(48_000);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-8;
        assert!(close(k.shelf.b[0], 1.535_124_859_586_97));
        assert!(close(k.shelf.b[1], -2.691_696_189_406_38));
        assert!(close(k.shelf.b[2], 1.198_392_810_852_85));
        assert!(close(k.shelf.a[0], -1.690_659_293_182_41));
        assert!(close(k.shelf.a[1], 0.732_480_774_215_85));
        assert!(close(k.high_pass.a[0], -1.990_047_454_833_98));
        assert!(close(k.high_pass.a[1], 0.990_072_250_366_21));
    }

    #[test]
    fn other_rates_measure_the_same() {
        for rate in [44_100, 96_000] {
            let mut blocks = Blocks::new(rate);
            let mut loudness = Loudness::new();
            let tone: Vec<f32> = (0..rate as usize * 10)
                .map(|i| {
                    let t = i as f64 / f64::from(rate);
                    (10f64.powf(-23.0 / 20.0) * (2.0 * std::f64::consts::PI * 1_000.0 * t).sin())
                        as f32
                })
                .collect();
            blocks.add(&tone, &tone, |z| loudness.push(z));
            assert!(
                near(loudness.integrated(), -23.0, 0.1),
                "{rate}: {}",
                loudness.integrated()
            );
        }
    }

    #[test]
    fn silence_and_reset() {
        let mut l = measure(&[(-23.0, 5.0)]);
        l.reset();
        assert_eq!(l.integrated(), SILENT);
        assert!(near(l.momentary(), -23.0, 0.1));
        for _ in 0..3 {
            l.push(1e-3);
        }
        assert_eq!(l.integrated(), SILENT);
        l.push(1e-3);
        assert!(near(l.integrated(), lufs(1e-3), 0.01));
        let quiet = measure(&[(-90.0, 5.0)]);
        assert_eq!(quiet.integrated(), SILENT);
    }

    #[test]
    fn true_peak_finds_the_peak_between_samples() {
        let tone = sine(f64::from(RATE) / 4.0, 0.0, 0.1, std::f64::consts::FRAC_PI_4);
        let sample_peak = tone.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((20.0 * sample_peak.log10() + 3.01).abs() < 0.05);
        let mut tp = TruePeak::new();
        let peak = 20.0 * tp.process(&tone).log10();
        assert!((-0.4..=0.2).contains(&peak), "{peak}");
    }

    #[test]
    fn true_peak_of_a_full_scale_tone_is_zero_dbtp() {
        for frequency in [100.0, 997.0, 5_000.0, 10_000.0] {
            let tone = sine(frequency, 0.0, 0.2, 0.3);
            let peak = 20.0 * TruePeak::new().process(&tone).log10();
            assert!((-0.4..=0.2).contains(&peak), "{frequency} Hz: {peak}");
        }
    }
}
