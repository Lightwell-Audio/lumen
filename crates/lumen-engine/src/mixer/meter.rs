use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use lumen_core::loudness::{Blocks, TruePeak};

pub(super) const SIDE_KEY: u64 = 1 << 63;

pub const RMS_SECONDS: f32 = 0.3;
const LOUDNESS_QUEUE: usize = 512;

#[derive(Debug, Default)]
struct Slot {
    peak: [AtomicU32; 2],
    power: [AtomicU32; 2],
    clipped: AtomicBool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Level {
    pub peak: [f32; 2],
    pub rms: [f32; 2],
    pub clipped: bool,
}

#[derive(Debug)]
pub struct MeterBank {
    slots: Box<[Slot]>,
}

impl MeterBank {
    pub(super) fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            slots: (0..capacity).map(|_| Slot::default()).collect(),
        })
    }

    pub(super) fn capacity(&self) -> usize {
        self.slots.len()
    }

    #[inline]
    pub(super) fn record(&self, slot: u32, sides: [&[f32]; 2], keep: f32) {
        let Some(slot) = self.slots.get(slot as usize) else {
            return;
        };
        let mut over = false;
        for (side, samples) in sides.into_iter().enumerate() {
            let (peak, sum) = peak_and_sum(samples);
            over |= peak > 1.0;
            if peak > 0.0 {
                slot.peak[side].fetch_max(peak.to_bits(), Ordering::Relaxed);
            }
            let mean = if samples.is_empty() {
                0.0
            } else {
                sum / samples.len() as f32
            };
            let mean = if mean.is_finite() { mean } else { 0.0 };
            smooth(&slot.power[side], mean, keep);
        }
        if over {
            slot.clipped.store(true, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(super) fn silence(&self, slot: u32, keep: f32) {
        if let Some(slot) = self.slots.get(slot as usize) {
            for power in &slot.power {
                smooth(power, 0.0, keep);
            }
        }
    }

    pub(super) fn take(&self, slot: u32) -> Level {
        let Some(slot) = self.slots.get(slot as usize) else {
            return Level::default();
        };
        let peak = |i: usize| f32::from_bits(slot.peak[i].swap(0, Ordering::Relaxed));
        let rms = |i: usize| f32::from_bits(slot.power[i].load(Ordering::Relaxed)).sqrt();
        Level {
            peak: [peak(0), peak(1)],
            rms: [rms(0), rms(1)],
            clipped: slot.clipped.swap(false, Ordering::Relaxed),
        }
    }
}

#[inline]
fn peak_and_sum(samples: &[f32]) -> (f32, f32) {
    const LANES: usize = 8;
    let (mut peak, mut sum) = ([0.0f32; LANES], [0.0f32; LANES]);
    let (chunks, rest) = samples.as_chunks::<LANES>();
    for chunk in chunks {
        for lane in 0..LANES {
            peak[lane] = peak[lane].max(chunk[lane].abs());
            sum[lane] += chunk[lane] * chunk[lane];
        }
    }
    let (mut p, mut s) = (
        peak.iter().fold(0.0f32, |m, &v| m.max(v)),
        sum.iter().sum::<f32>(),
    );
    for &x in rest {
        p = p.max(x.abs());
        s += x * x;
    }
    (p, s)
}

#[inline]
fn smooth(power: &AtomicU32, mean: f32, keep: f32) {
    let was = f32::from_bits(power.load(Ordering::Relaxed));
    let now = was * keep + mean * (1.0 - keep);
    let now = if now < 1e-15 { 0.0 } else { now };
    power.store(now.to_bits(), Ordering::Relaxed);
}

#[inline]
pub(super) fn keep(frames: usize, rate: u32) -> f32 {
    (-(frames as f32) / (RMS_SECONDS * rate.max(1) as f32)).exp()
}

#[derive(Debug, Default)]
pub(super) struct Shared {
    true_peak: AtomicU32,
}

pub(super) struct LoudnessTap {
    blocks: Blocks,
    true_peak: [TruePeak; 2],
    out: rtrb::Producer<f32>,
    shared: Arc<Shared>,
}

pub(super) struct LoudnessReader {
    blocks: rtrb::Consumer<f32>,
    shared: Arc<Shared>,
}

pub(super) fn loudness() -> (LoudnessTap, LoudnessReader) {
    let (out, blocks) = rtrb::RingBuffer::new(LOUDNESS_QUEUE);
    let shared = Arc::new(Shared::default());
    (
        LoudnessTap {
            blocks: Blocks::new(48_000),
            true_peak: [TruePeak::new(), TruePeak::new()],
            out,
            shared: Arc::clone(&shared),
        },
        LoudnessReader { blocks, shared },
    )
}

impl LoudnessTap {
    pub(super) fn record(&mut self, left: &[f32], right: &[f32], rate: u32) {
        if self.blocks.sample_rate() != rate {
            self.blocks = Blocks::new(rate);
        }
        let out = &mut self.out;
        self.blocks.add(left, right, |z| {
            let _ = out.push(z);
        });
        let peak = self.true_peak[0]
            .process(left)
            .max(self.true_peak[1].process(right));
        if peak > 0.0 {
            self.shared
                .true_peak
                .fetch_max(peak.to_bits(), Ordering::Relaxed);
        }
    }
}

impl LoudnessReader {
    pub(super) fn drain(&mut self, mut block: impl FnMut(f32)) {
        while let Ok(z) = self.blocks.pop() {
            block(z);
        }
    }

    pub(super) fn take_true_peak(&self) -> f32 {
        f32::from_bits(self.shared.true_peak.swap(0, Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_hold_until_read_and_clips_latch() {
        let bank = MeterBank::new(2);
        bank.record(1, [&[0.5, -0.9], &[0.1, 0.2]], 0.0);
        bank.record(1, [&[0.2, 0.1], &[1.5, 0.0]], 0.0);
        let level = bank.take(1);
        assert_eq!(level.peak, [0.9, 1.5]);
        assert!(level.clipped);
        let again = bank.take(1);
        assert_eq!(again.peak, [0.0, 0.0]);
        assert!(!again.clipped);
        assert!(again.rms[1] > 0.0);
        bank.record(9, [&[1.0], &[1.0]], 0.0);
        assert_eq!(bank.take(9), Level::default());
    }

    #[test]
    fn one_non_finite_sample_does_not_leave_the_average_broken() {
        let bank = MeterBank::new(1);
        let k = keep(128, 48_000);
        let sine: Vec<f32> = (0..128).map(|i| (i as f32 * 0.2).sin()).collect();
        for _ in 0..50 {
            bank.record(0, [&sine, &sine], k);
        }
        let mut bad = sine.clone();
        bad[40] = f32::NAN;
        bad[41] = f32::INFINITY;
        bank.record(0, [&bad, &sine], k);
        for _ in 0..50 {
            bank.record(0, [&sine, &sine], k);
        }
        let level = bank.take(0);
        assert!(
            level.rms.iter().all(|r| r.is_finite() && *r > 0.0),
            "{:?}",
            level.rms
        );
    }

    #[test]
    fn rms_settles_on_a_sines_level_and_falls_in_silence() {
        let bank = MeterBank::new(1);
        let rate = 48_000;
        let sine: Vec<f32> = (0..128)
            .map(|i| (2.0 * std::f32::consts::PI * i as f32 / 32.0).sin())
            .collect();
        let k = keep(128, rate);
        for _ in 0..(rate as usize * 2 / 128) {
            bank.record(0, [&sine, &sine], k);
        }
        let rms = bank.take(0).rms[0];
        assert!((20.0 * rms.log10() + 3.01).abs() < 0.05, "{rms}");
        for _ in 0..(rate as usize * 2 / 128) {
            bank.silence(0, k);
        }
        let fallen = 20.0 * (bank.take(0).rms[0] / rms).log10();
        assert!((-30.0..-28.0).contains(&fallen), "{fallen}");
    }
}
