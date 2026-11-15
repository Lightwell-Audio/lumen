use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const SIZE: usize = 2048;
pub const BINS: usize = SIZE / 2;
pub const TRIES: usize = 4;

pub const POINTS: usize = 479;
pub const PER_OCTAVE: f32 = 48.0;
pub const SMOOTH: f32 = 1.0 / 6.0;
pub const RISE: f32 = 0.010;
pub const FALL: f32 = 0.300;
const ENBW: f32 = 1.5;
const LONGEST_STEP: f32 = 0.5;

const BAND_BELOW: f32 = 0.943_874_3;
const BAND_ABOVE: f32 = 1.059_463_1;

const MASK: u64 = SIZE as u64 - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    Window,
    Stale,
    Nothing,
}

struct Shared {
    ring: Box<[AtomicU32]>,
    written: AtomicU64,
    on: AtomicBool,
}

pub struct Tap {
    shared: Arc<Shared>,
}

impl Tap {
    pub fn is_on(&self) -> bool {
        self.shared.on.load(Ordering::Relaxed)
    }

    pub fn write(&mut self, left: &[f32], right: &[f32]) {
        if !self.is_on() {
            return;
        }
        let start = self.shared.written.load(Ordering::Relaxed);
        let frames = left.len().min(right.len());
        for (i, (&l, &r)) in left.iter().zip(right).enumerate() {
            let mono = 0.5 * (l + r);
            self.shared.ring[((start + i as u64) & MASK) as usize]
                .store(mono.to_bits(), Ordering::Relaxed);
        }
        self.shared
            .written
            .store(start + frames as u64, Ordering::Release);
    }
}

pub struct Reader {
    shared: Arc<Shared>,
    window: Vec<f32>,
    scratch: Vec<f32>,
    taper: Vec<f32>,
    spectrum: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    seen: u64,
    stale: bool,
    #[cfg(test)]
    pretend_laps: usize,
}

pub fn pair() -> (Tap, Reader) {
    let shared = Arc::new(Shared {
        ring: (0..SIZE).map(|_| AtomicU32::new(0)).collect(),
        written: AtomicU64::new(0),
        on: AtomicBool::new(false),
    });
    (
        Tap {
            shared: Arc::clone(&shared),
        },
        Reader {
            shared,
            window: vec![0.0; SIZE],
            scratch: vec![0.0; SIZE],
            taper: (0..SIZE)
                .map(|i| {
                    let phase = std::f32::consts::TAU * i as f32 / SIZE as f32;
                    0.5 - 0.5 * phase.cos()
                })
                .collect(),
            spectrum: vec![MIN_DB; BINS],
            re: vec![0.0; SIZE],
            im: vec![0.0; SIZE],
            seen: 0,
            stale: true,
            #[cfg(test)]
            pretend_laps: 0,
        },
    )
}

pub const MIN_DB: f32 = -90.0;

impl Reader {
    pub fn set_on(&self, on: bool) {
        self.shared.on.store(on, Ordering::Relaxed);
    }

    pub fn is_on(&self) -> bool {
        self.shared.on.load(Ordering::Relaxed)
    }

    pub fn read(&mut self) -> Read {
        let mut end = self.shared.written.load(Ordering::Acquire);
        if end < SIZE as u64 || end == self.seen {
            return Read::Nothing;
        }
        for _ in 0..TRIES {
            let start = end - SIZE as u64;
            for (i, sample) in self.scratch.iter_mut().enumerate() {
                let slot = ((start + i as u64) & MASK) as usize;
                *sample = f32::from_bits(self.shared.ring[slot].load(Ordering::Relaxed));
            }
            let after = self.written_after(end);
            if after - end < SIZE as u64 {
                self.window.copy_from_slice(&self.scratch);
                self.seen = end;
                self.stale = true;
                return Read::Window;
            }
            end = after;
        }
        Read::Stale
    }

    #[cfg(not(test))]
    fn written_after(&self, _end: u64) -> u64 {
        self.shared.written.load(Ordering::Acquire)
    }

    #[cfg(test)]
    fn written_after(&mut self, end: u64) -> u64 {
        if self.pretend_laps > 0 {
            self.pretend_laps -= 1;
            return end + SIZE as u64;
        }
        self.shared.written.load(Ordering::Acquire)
    }

    #[cfg(test)]
    fn pretend_laps(&mut self, laps: usize) {
        self.pretend_laps = laps;
    }

    pub fn window(&self) -> &[f32] {
        &self.window
    }

    pub fn spectrum(&mut self) -> &[f32] {
        if !self.stale {
            return &self.spectrum;
        }
        self.stale = false;
        for i in 0..SIZE {
            self.re[i] = self.window[i] * self.taper[i];
            self.im[i] = 0.0;
        }
        transform(&mut self.re, &mut self.im);
        let scale = 4.0 / SIZE as f32;
        for (bin, out) in self.spectrum.iter_mut().enumerate() {
            let (re, im) = (self.re[bin], self.im[bin]);
            let magnitude = (re * re + im * im).sqrt() * scale;
            *out = if magnitude > 0.0 {
                (20.0 * magnitude.log10()).max(MIN_DB)
            } else {
                MIN_DB
            };
        }
        &self.spectrum
    }
}

struct Picture<'a> {
    power: &'a [f32],
    rate: u32,
}

impl Picture<'_> {
    fn band(&self, centre: f32) -> f32 {
        let bins = self.power.len();
        if bins < 3 {
            return MIN_DB;
        }
        let bin_hz = self.rate as f32 / 2.0 / bins as f32;
        let low = centre * BAND_BELOW;
        let high = centre * BAND_ABOVE;
        let width = (high - low) / bin_hz;
        if low >= bin_hz * (bins - 1) as f32 {
            return level(
                f64::from(self.at(bins - 1)) * f64::from(width.max(1.0)) / f64::from(ENBW),
            );
        }
        if high <= bin_hz {
            return level(f64::from(self.at(1)) * f64::from(width.max(1.0)) / f64::from(ENBW));
        }
        let first = ((low / bin_hz).floor() as usize).clamp(1, bins - 2);
        let last = ((high / bin_hz).ceil() as usize).clamp(first, bins - 1);
        let mut integral = 0.0f64;
        let mut weighed = 0.0f64;
        for k in first..last {
            let start = k as f32 * bin_hz;
            let end = (k + 1) as f32 * bin_hz;
            let (from, to) = (start.max(low), end.min(high));
            if to <= from {
                continue;
            }
            let slope = (self.at(k + 1) - self.at(k)) / bin_hz;
            let (a, b) = (
                self.at(k) + slope * (from - start),
                self.at(k) + slope * (to - start),
            );
            integral += 0.5 * f64::from(a + b) * f64::from(to - from);
            weighed += f64::from(to - from);
        }
        let mean = if weighed > 0.0 {
            integral / weighed
        } else {
            f64::from(self.at(first))
        };
        level(mean * f64::from(width.max(1.0)) / f64::from(ENBW))
    }

    fn at(&self, bin: usize) -> f32 {
        self.power[bin.min(self.power.len() - 1)]
    }
}

fn level(power: f64) -> f32 {
    if power <= 0.0 {
        MIN_DB
    } else {
        ((10.0 * power.log10()) as f32).max(MIN_DB)
    }
}

pub fn follow(shown: f32, target: f32, dt: f32) -> f32 {
    let tau = if target > shown { RISE } else { FALL };
    let step = f64::from(dt.clamp(0.0, LONGEST_STEP)) / f64::from(tau);
    let share = 1.0 - (-step).exp();
    shown + (target - shown) * share as f32
}

pub struct Display {
    low: f32,
    high: f32,
    centres: Vec<f32>,
    power: Vec<f32>,
    target: Vec<f32>,
    shown: Vec<f32>,
}

impl Display {
    pub fn new((low, high): (f32, f32)) -> Self {
        let low = low.max(1.0);
        let high = high.max(low * 2.0);
        let count = (((high / low).log2() * PER_OCTAVE).round() as usize + 1).max(2);
        let centres = (0..count)
            .map(|i| low * (high / low).powf(i as f32 / (count - 1) as f32))
            .collect();
        Self {
            low,
            high,
            centres,
            power: vec![0.0; BINS],
            target: vec![MIN_DB; count],
            shown: vec![MIN_DB; count],
        }
    }

    pub fn bins(&self) -> &[f32] {
        &self.shown
    }

    pub fn len(&self) -> usize {
        self.shown.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    pub fn update(&mut self, spectrum: &[f32], rate: u32, dt: f32) {
        self.power.resize(spectrum.len(), 0.0);
        for (power, db) in self.power.iter_mut().zip(spectrum) {
            *power = if *db > MIN_DB {
                10f32.powf(*db * 0.1)
            } else {
                0.0
            };
        }
        let Self {
            power,
            target,
            centres,
            ..
        } = self;
        let picture = Picture {
            power,
            rate: rate.max(2),
        };
        for (target, &centre) in target.iter_mut().zip(centres.iter()) {
            *target = picture.band(centre);
        }
        self.advance(dt);
    }

    pub fn advance(&mut self, dt: f32) {
        for (shown, &target) in self.shown.iter_mut().zip(&self.target) {
            *shown = follow(*shown, target, dt);
        }
    }

    pub fn axis(&self) -> (f32, f32) {
        (self.low, self.high)
    }
}

fn transform(re: &mut [f32], im: &mut [f32]) {
    let n = re.len().min(im.len());
    if n < 2 || !n.is_power_of_two() {
        return;
    }
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let angle = -std::f64::consts::TAU / len as f64;
        let (wr, wi) = (angle.cos(), angle.sin());
        let mut base = 0;
        while base < n {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (cr32, ci32) = (cr as f32, ci as f32);
                let (ur, ui) = (re[base + k], im[base + k]);
                let (vr0, vi0) = (re[base + k + len / 2], im[base + k + len / 2]);
                let vr = vr0 * cr32 - vi0 * ci32;
                let vi = vr0 * ci32 + vi0 * cr32;
                re[base + k] = ur + vr;
                im[base + k] = ui + vi;
                re[base + k + len / 2] = ur - vr;
                im[base + k + len / 2] = ui - vi;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
            base += len;
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests;
