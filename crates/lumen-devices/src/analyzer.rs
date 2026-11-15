use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const SIZE: usize = 2048;
pub const BINS: usize = SIZE / 2;
pub const TRIES: usize = 4;

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

    #[test]
    fn a_sine_on_a_bin_reads_its_amplitude_in_that_bin() {
        let rate = 48_000.0;
        let bin = 40;
        let hz = bin as f32 * rate / 2.0 / BINS as f32;
        let samples = sine(hz, rate, SIZE, 0.5);
        let (mut tap, mut reader) = pair();
        reader.set_on(true);
        tap.write(&samples, &samples);
        assert_eq!(reader.read(), Read::Window);
        let spectrum = reader.spectrum();
        assert!(
            (spectrum[bin] - (-6.02)).abs() < 0.2,
            "{} dB at its bin",
            spectrum[bin]
        );
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

    #[test]
    fn a_tap_nothing_reads_writes_nothing() {
        let (mut tap, mut reader) = pair();
        assert!(!tap.is_on());
        tap.write(&[1.0; 64], &[1.0; 64]);
        assert_eq!(reader.read(), Read::Nothing, "nothing was written");
        reader.set_on(true);
        assert!(tap.is_on());
        tap.write(&[1.0; SIZE], &[1.0; SIZE]);
        assert_eq!(reader.read(), Read::Window);
        assert!(reader.window().iter().all(|x| *x == 1.0));
        assert_eq!(reader.read(), Read::Nothing, "the window has not moved");
    }

    #[test]
    fn the_reader_gets_the_newest_window() {
        let (mut tap, mut reader) = pair();
        reader.set_on(true);
        tap.write(&vec![1.0; SIZE], &vec![1.0; SIZE]);
        tap.write(&vec![2.0; SIZE], &vec![2.0; SIZE]);
        assert_eq!(reader.read(), Read::Window);
        assert!(reader.window().iter().all(|x| *x == 2.0));
    }

    #[test]
    fn a_reader_lapped_on_every_try_keeps_the_window_before() {
        let (mut tap, mut reader) = pair();
        reader.set_on(true);
        tap.write(&vec![1.0; SIZE], &vec![1.0; SIZE]);
        assert_eq!(reader.read(), Read::Window);
        assert!(reader.window().iter().all(|x| *x == 1.0));
        let spectrum_before = reader.spectrum().to_vec();

        tap.write(&vec![2.0; SIZE], &vec![2.0; SIZE]);
        reader.pretend_laps(TRIES);
        assert_eq!(reader.read(), Read::Stale);
        assert!(
            reader.window().iter().all(|x| *x == 1.0),
            "the window before is kept, not a torn one"
        );
        assert_eq!(
            reader.spectrum(),
            spectrum_before,
            "and its spectrum is not worked out again"
        );

        assert_eq!(reader.read(), Read::Window);
        assert!(reader.window().iter().all(|x| *x == 2.0));
    }

    #[test]
    fn silence_reads_the_floor_and_the_points_follow_a_peak() {
        let rate = 48_000;
        let (mut tap, mut reader) = pair();
        reader.set_on(true);
        tap.write(&vec![0.0; SIZE], &vec![0.0; SIZE]);
        assert_eq!(reader.read(), Read::Window);
        assert!(reader.spectrum().iter().all(|db| *db == MIN_DB));

        let samples = sine(1_000.0, rate as f32, SIZE, 0.5);
        tap.write(&samples, &samples);
        assert_eq!(reader.read(), Read::Window);
        let mut points = [MIN_DB; 40];
        reader.points(rate, (20.0, 20_000.0), &mut points);
        let loudest = points
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(at, _)| at)
            .unwrap();
        assert!((22..=23).contains(&loudest), "{loudest}: {points:?}");
        assert!(points[loudest] > -8.0, "{}", points[loudest]);
    }
}
