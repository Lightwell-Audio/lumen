//! A spectrum analyser's shared window (M8 slice B1, ADR-0146), and the
//! picture a face draws from it (the owner's review OR-7).
//!
//! The audio thread writes the last [`SIZE`] frames it hears into a ring of
//! atomics and publishes how many it has written; the interface copies the
//! newest window out and transforms it. Nothing here allocates, locks or
//! blocks after its constructor, so a device writes from the audio thread with
//! the same rules as the rest of its DSP, and the interface reads when it
//! draws.
//!
//! Nothing here knows about a device: Iris writes its input or its output,
//! and the analysis devices of X2 reuse the same ring (M8 §3 X2). The window
//! is mono — the sum of the two sides — because a spectrum read off a stereo
//! pair is one picture, not two.
//!
//! A reader can be lapped by the writer, and a window copied across a lap is
//! torn; the copy is then thrown away and tried again, up to [`TRIES`] times.
//! An analyser may show an older window, never a wrong one that a person could
//! read as a peak that is not there, so when every try is lapped the reader
//! keeps the whole window it held and says [`Read::Stale`].
//!
//! # The picture ([`Display`])
//!
//! A transform's bins are evenly spaced in hertz and a face's axis is even in
//! octaves, so the two do not line up: at 20 Hz a whole octave is under one
//! bin, and at 10 kHz one bin is a fortieth of a pixel. A picture made of "the
//! loudest bin under this point" therefore stair-steps at the bottom and
//! flickers at the top, which is what the owner saw. [`Display`] turns the
//! spectrum into a fixed set of log-spaced bins instead: each bin is the mean
//! power over a sixth of an octave, integrated across the transform's own bins
//! with the power read straight between them, so the bottom is interpolated
//! rather than stepped and the top is averaged rather than sampled. Each bin
//! then follows its target with a time-based rise and fall, so the picture
//! moves at the same speed whatever the interface's frame rate is.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// The frames in one window: 2048 at 48 kHz is 42.7 ms, 23 Hz a bin.
pub const SIZE: usize = 2048;
/// The bins a window's transform has, at or under the Nyquist frequency.
pub const BINS: usize = SIZE / 2;
/// How often a copy across a lap is tried before the reader keeps what it has
/// and says the window is old.
pub const TRIES: usize = 4;

/// The bins of a picture across the axis a face draws (20 Hz to 20 kHz):
/// [`PER_OCTAVE`] of them to an octave.
pub const POINTS: usize = 479;
/// The picture's bins to an octave: a forty-eighth, three times finer than the
/// sixth of an octave each one is smoothed over.
pub const PER_OCTAVE: f32 = 48.0;
/// How wide a bin's own band is, in octaves.
pub const SMOOTH: f32 = 1.0 / 6.0;
/// How long a bin takes to reach a level above it, and one below it: a rise of
/// ten milliseconds and a fall of three hundred, the ballistic an analyser is
/// read at. Both are time constants, so a frame that took twice as long moves
/// twice as far and the picture keeps its speed at any frame rate.
pub const RISE: f32 = 0.010;
pub const FALL: f32 = 0.300;
/// How much wider than one bin a Hann window's own response is: a tone's power
/// spread over its neighbours, which a band's level is measured against.
const ENBW: f32 = 1.5;
/// The longest frame the ballistics will take in one step: a window that
/// stalled for longer than this jumps rather than crawling.
const LONGEST_STEP: f32 = 0.5;

/// `2^-SMOOTH/2` and `2^SMOOTH/2`: a bin's band's ends, from its centre.
const BELOW: f32 = 0.943_874_3;
const ABOVE: f32 = 1.059_463_1;

const MASK: u64 = SIZE as u64 - 1;

/// What a [`Reader::read`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// A whole new window was copied: what the reader holds is the newest
    /// sound.
    Window,
    /// The writer came round during every try, so nothing was copied: what
    /// the reader holds is the window before, which is whole but old, and the
    /// face may show it rather than a copy that is two windows joined.
    Stale,
    /// The writer has not filled a window yet, or has not moved since the
    /// last read.
    Nothing,
}

/// What the audio thread writes and the interface reads.
struct Shared {
    /// The last [`SIZE`] mono frames, by position modulo [`SIZE`], as `f32`
    /// bits.
    ring: Box<[AtomicU32]>,
    /// Frames written since the ring was made, which is where the newest
    /// window ends.
    written: AtomicU64,
    /// Whether anything is listening: false while the face is not drawn, so a
    /// device pays one load a block and writes nothing.
    on: AtomicBool,
}

/// The audio thread's end of the window: cheap to hold, and it writes nothing
/// while nothing reads.
pub struct Tap {
    shared: Arc<Shared>,
}

impl Tap {
    /// Whether the interface is reading now.
    pub fn is_on(&self) -> bool {
        self.shared.on.load(Ordering::Relaxed)
    }

    /// Adds `left` and `right` (the same length) to the window as one mono
    /// frame each. Real-time safe: an atomic store a frame, and no branch
    /// that allocates.
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

/// The interface's end: it copies the newest window out and transforms it.
/// Allocates in [`Reader::new`]: make it on the interface side.
pub struct Reader {
    shared: Arc<Shared>,
    /// The newest whole window of mono frames.
    window: Vec<f32>,
    /// Where a copy is made before it is known to be whole: a copy the writer
    /// lapped is thrown away rather than shown.
    scratch: Vec<f32>,
    /// The window's Hann taper, worked out once.
    taper: Vec<f32>,
    /// The window's transform, in dB a bin: a full-scale sine at a bin's
    /// centre reads 0 dB.
    spectrum: Vec<f32>,
    /// The work buffers of the transform.
    re: Vec<f32>,
    im: Vec<f32>,
    /// How many frames had been written when the window was last copied.
    seen: u64,
    /// The newest window has not been transformed yet.
    stale: bool,
    /// A lap the test asks for: the next copies look as if the writer came
    /// round during each of them (M8 slice B1, review L1).
    #[cfg(test)]
    pretend_laps: usize,
}

/// A tap and its reader: the device holds the one, the interface the other.
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

/// What a bin reads when there is nothing in it.
pub const MIN_DB: f32 = -90.0;

impl Reader {
    /// Whether the analyser is listened to: set by the interface while its
    /// face is drawn, so a device that is not looked at writes nothing.
    pub fn set_on(&self, on: bool) {
        self.shared.on.store(on, Ordering::Relaxed);
    }

    pub fn is_on(&self) -> bool {
        self.shared.on.load(Ordering::Relaxed)
    }

    /// Copies the newest whole window out of the ring. A copy the writer came
    /// round during is thrown away and tried again; when every try was lapped
    /// the reader keeps the window before it held and says it is stale, rather
    /// than showing a copy that is two windows joined.
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
            // The writer came round while the copy was made: the frames
            // copied are two windows joined. Try again at the newer end.
            end = after;
        }
        // Every try was lapped. What the reader holds is the whole window
        // before this call, and it says so.
        Read::Stale
    }

    /// How many frames the writer had written after a copy made at `end`.
    #[cfg(not(test))]
    fn written_after(&self, _end: u64) -> u64 {
        self.shared.written.load(Ordering::Acquire)
    }

    /// [`Reader::written_after`], with the lap a test asks for.
    #[cfg(test)]
    fn written_after(&mut self, end: u64) -> u64 {
        if self.pretend_laps > 0 {
            self.pretend_laps -= 1;
            return end + SIZE as u64;
        }
        self.shared.written.load(Ordering::Acquire)
    }

    /// Makes the next `laps` copies look as if the writer came round during
    /// each of them, so what a reader does when it never sees a whole window
    /// is a test and not a race (M8 slice B1, review L1).
    #[cfg(test)]
    fn pretend_laps(&mut self, laps: usize) {
        self.pretend_laps = laps;
    }

    /// The copied window, oldest first.
    pub fn window(&self) -> &[f32] {
        &self.window
    }

    /// The window's spectrum in dB a bin, `BINS` of them from 0 Hz up to the
    /// Nyquist frequency less one bin. Worked out only when the window is new
    /// since the last call.
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
        // A Hann-windowed sine of amplitude 1 reads `SIZE / 4` in its bin.
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

/// One window's spectrum as power a bin, read as a fixed set of log-spaced
/// bands: what [`Display`] folds its target from.
struct Picture<'a> {
    power: &'a [f32],
    rate: u32,
}

impl Picture<'_> {
    /// The mean power over a [`SMOOTH`]-octave band centred at `centre`, in
    /// decibels: the transform's bins added up with the power read straight
    /// between them, so a band narrower than a bin is interpolated rather than
    /// stepped and a band of many bins is averaged rather than sampled.
    ///
    /// The level is the band's energy over a Hann window's own width, which
    /// makes a tone read its own amplitude whatever the band's width is.
    fn band(&self, centre: f32) -> f32 {
        let bins = self.power.len();
        if bins < 3 {
            return MIN_DB;
        }
        let bin_hz = self.rate as f32 / 2.0 / bins as f32;
        let low = centre * BELOW;
        let high = centre * ABOVE;
        let width = (high - low) / bin_hz;
        if low >= bin_hz * (bins - 1) as f32 {
            return level(
                f64::from(self.at(bins - 1)) * f64::from(width.max(1.0)) / f64::from(ENBW),
            );
        }
        if high <= bin_hz {
            // Under the transform's own first bin: the lowest bin's level, so
            // the deepest octave is a straight line rather than a stair.
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

    /// A bin's power.
    fn at(&self, bin: usize) -> f32 {
        self.power[bin.min(self.power.len() - 1)]
    }
}

/// Power as decibels, floored at [`MIN_DB`].
fn level(power: f64) -> f32 {
    if power <= 0.0 {
        MIN_DB
    } else {
        ((10.0 * power.log10()) as f32).max(MIN_DB)
    }
}

/// One shown level following its target: a step of `dt` seconds, rising in
/// [`RISE`] and falling in [`FALL`]. The coefficient is worked out from the
/// time the frame took, so the same wall-clock time covers the same share of
/// the distance at any frame rate — 60 fps, 120, or a frame that stalled.
pub fn follow(shown: f32, target: f32, dt: f32) -> f32 {
    let tau = if target > shown { RISE } else { FALL };
    let step = f64::from(dt.clamp(0.0, LONGEST_STEP)) / f64::from(tau);
    let share = 1.0 - (-step).exp();
    shown + (target - shown) * share as f32
}

/// The picture a face draws: [`POINTS`] log-spaced bins across the axis it is
/// made for, smoothed across frequency and in time (see the module's own
/// note). Allocates in [`Display::new`]: make it on the interface side.
pub struct Display {
    low: f32,
    high: f32,
    /// Each bin's centre frequency, low to high.
    centres: Vec<f32>,
    /// The last window's transform, as power a bin: the box the bands are
    /// folded out of, so a decibel is turned into a power once.
    power: Vec<f32>,
    /// The level a bin is heading for this window, in decibels.
    target: Vec<f32>,
    /// The level a bin is at now: what the face draws.
    shown: Vec<f32>,
}

impl Display {
    /// A picture across `low` to `high` hertz, one bin every [`PER_OCTAVE`]th
    /// of an octave: [`POINTS`] of them on the face's own axis.
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

    /// The bins, low to high, in decibels: what a face draws.
    pub fn bins(&self) -> &[f32] {
        &self.shown
    }

    /// How many bins there are.
    pub fn len(&self) -> usize {
        self.shown.len()
    }

    /// Whether nothing has been heard yet.
    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// Takes a window's spectrum and moves the picture `dt` seconds on.
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

    /// Moves the picture `dt` seconds towards the levels it last heard: what a
    /// stalled analyser does, so a picture falls back rather than freezing.
    pub fn advance(&mut self, dt: f32) {
        for (shown, &target) in self.shown.iter_mut().zip(&self.target) {
            *shown = follow(*shown, target, dt);
        }
    }

    /// The axis the picture is drawn across.
    pub fn axis(&self) -> (f32, f32) {
        (self.low, self.high)
    }
}

/// The transform of `re`/`im` in place: an iterative radix-2 Cooley-Tukey,
/// the length a power of two. Written here rather than pulled in (ADR-0146):
/// it is forty lines, it allocates nothing, and the crates that would do it
/// bring a planner this never asks for.
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
