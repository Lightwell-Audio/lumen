//! A spectrum analyser's shared window (M8 slice B1, ADR-0146).
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
//! Later tries always win: an analyser may show an older window, never a
//! wrong one that a person could read as a peak that is not there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// The frames in one window: 2048 at 48 kHz is 42.7 ms, 23 Hz a bin.
pub const SIZE: usize = 2048;
/// The bins a window's transform has, at or under the Nyquist frequency.
pub const BINS: usize = SIZE / 2;
/// How often a copy across a lap is tried before the last one is accepted:
/// a display is worth a torn window sooner than a stalled interface.
pub const TRIES: usize = 4;

const MASK: u64 = SIZE as u64 - 1;

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

    /// Copies the newest window out of the ring; true when the writer has
    /// moved since the last copy, so a still writer costs a transform once,
    /// not a frame.
    pub fn read(&mut self) -> bool {
        let mut end = self.shared.written.load(Ordering::Acquire);
        if end < SIZE as u64 || end == self.seen {
            return false;
        }
        for _ in 0..TRIES {
            let start = end - SIZE as u64;
            for (i, sample) in self.window.iter_mut().enumerate() {
                let slot = ((start + i as u64) & MASK) as usize;
                *sample = f32::from_bits(self.shared.ring[slot].load(Ordering::Relaxed));
            }
            let after = self.shared.written.load(Ordering::Acquire);
            if after - end < SIZE as u64 {
                self.seen = end;
                self.stale = true;
                return true;
            }
            // The writer came round while the copy was made: the frames
            // copied are two windows joined. Try again at the newer end.
            end = after;
        }
        // Still racing after every try: the display takes the last window
        // rather than stopping.
        self.seen = end;
        self.stale = true;
        true
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

    /// The spectrum gathered into `out` points spread evenly on a log
    /// frequency axis from `low` to `high` Hz at `rate`: each point is the
    /// loudest bin under it, so a narrow peak is never lost between points.
    pub fn points(&mut self, rate: u32, (low, high): (f32, f32), out: &mut [f32]) {
        let bins = self.spectrum();
        let nyquist = rate as f32 / 2.0;
        let count = bins.len().max(1) as f32;
        let points = out.len().max(1);
        for (i, point) in out.iter_mut().enumerate() {
            let at = |share: f32| low * (high / low).powf(share);
            let from = (at(i as f32 / points as f32) / nyquist * count).floor() as usize;
            let to = ((at((i + 1) as f32 / points as f32) / nyquist * count).ceil() as usize)
                .clamp(from + 1, bins.len());
            let mut loudest = MIN_DB;
            if let Some(slice) = bins.get(from.min(bins.len())..to) {
                for db in slice {
                    if *db > loudest {
                        loudest = *db;
                    }
                }
            }
            *point = loudest;
        }
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
mod tests {
    use super::*;

    fn sine(hz: f32, rate: f32, frames: usize, amplitude: f32) -> Vec<f32> {
        (0..frames)
            .map(|i| amplitude * (std::f32::consts::TAU * hz * i as f32 / rate).sin())
            .collect()
    }

    /// A sine on a bin's own frequency reads its amplitude there, and
    /// nothing anywhere else (the transform and its scale are right).
    #[test]
    fn a_sine_on_a_bin_reads_its_amplitude_in_that_bin() {
        let rate = 48_000.0;
        // Bin 40 of 1024 over 24 kHz is 937.5 Hz.
        let bin = 40;
        let hz = bin as f32 * rate / 2.0 / BINS as f32;
        let samples = sine(hz, rate, SIZE, 0.5);
        let (mut tap, mut reader) = pair();
        reader.set_on(true);
        tap.write(&samples, &samples);
        assert!(reader.read());
        let spectrum = reader.spectrum();
        // 0.5 of full scale is −6 dB.
        assert!(
            (spectrum[bin] - (-6.02)).abs() < 0.2,
            "{} dB at its bin",
            spectrum[bin]
        );
        // A Hann window's main lobe is three bins wide, so its neighbours sit
        // 6 dB down and 31 dB down; nothing further away is anything at all.
        assert!(
            (spectrum[bin - 1] - (-12.0)).abs() < 1.0,
            "{}",
            spectrum[bin - 1]
        );
        for (other, db) in spectrum.iter().enumerate() {
            if other.abs_diff(bin) > 2 {
                assert!(*db < -25.0, "{other} reads {db}");
            }
        }
    }

    /// The transform is the one a direct sum gives: every bin to 1e-3.
    #[test]
    fn the_transform_is_the_one_a_direct_sum_gives() {
        let n = 64;
        let signal: Vec<f32> = (0..n)
            .map(|i| (i as f32 * 0.31).sin() + 0.4 * (i as f32 * 1.7).cos())
            .collect();
        let mut re = signal.clone();
        let mut im = vec![0.0; n];
        transform(&mut re, &mut im);
        for k in 0..n {
            let (mut want_re, mut want_im) = (0.0f32, 0.0f32);
            for (i, sample) in signal.iter().enumerate() {
                let angle = -std::f32::consts::TAU * (k * i) as f32 / n as f32;
                want_re += sample * angle.cos();
                want_im += sample * angle.sin();
            }
            assert!((re[k] - want_re).abs() < 1e-3, "bin {k} re: {}", re[k]);
            assert!((im[k] - want_im).abs() < 1e-3, "bin {k} im: {}", im[k]);
        }
    }

    /// A tap that nothing listens to writes nothing at all.
    #[test]
    fn a_tap_nothing_reads_writes_nothing() {
        let (mut tap, mut reader) = pair();
        assert!(!tap.is_on());
        tap.write(&[1.0; 64], &[1.0; 64]);
        assert!(!reader.read(), "nothing was written");
        reader.set_on(true);
        assert!(tap.is_on());
        tap.write(&[1.0; SIZE], &[1.0; SIZE]);
        assert!(reader.read());
        assert!(reader.window().iter().all(|x| *x == 1.0));
        // A second read of a still writer finds nothing new.
        assert!(!reader.read(), "the window has not moved");
    }

    /// The reader gets the newest window, not the first one.
    #[test]
    fn the_reader_gets_the_newest_window() {
        let (mut tap, mut reader) = pair();
        reader.set_on(true);
        tap.write(&vec![1.0; SIZE], &vec![1.0; SIZE]);
        tap.write(&vec![2.0; SIZE], &vec![2.0; SIZE]);
        assert!(reader.read());
        assert!(reader.window().iter().all(|x| *x == 2.0));
    }

    /// The spectrum of silence is the floor, and its points are its floor.
    #[test]
    fn silence_reads_the_floor_and_the_points_follow_a_peak() {
        let rate = 48_000;
        let (mut tap, mut reader) = pair();
        reader.set_on(true);
        tap.write(&vec![0.0; SIZE], &vec![0.0; SIZE]);
        assert!(reader.read());
        assert!(reader.spectrum().iter().all(|db| *db == MIN_DB));

        let samples = sine(1_000.0, rate as f32, SIZE, 0.5);
        tap.write(&samples, &samples);
        assert!(reader.read());
        let mut points = [MIN_DB; 40];
        reader.points(rate, (20.0, 20_000.0), &mut points);
        let loudest = points
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(at, _)| at)
            .unwrap();
        // 1 kHz of 20 Hz to 20 kHz over 40 points is the 22nd or 23rd.
        assert!((22..=23).contains(&loudest), "{loudest}: {points:?}");
        assert!(points[loudest] > -8.0, "{}", points[loudest]);
    }
}
