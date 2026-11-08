//! Repitch: playing a file at a speed that follows the song (M9.5 S4b, D1).
//!
//! The speed of the file against the song changes from frame to frame (a
//! tempo ramp, a warp marker), so the output is not a resampling at one ratio
//! but the file read at a *position* that is a function of the output frame.
//! [`Renderer::render`] takes that function and a run of output frames and
//! returns them, band-limited: a windowed-sinc interpolation, widened when the
//! file runs faster than the song so that nothing folds back (a quarter of the
//! speed to four times it, the limits of a warp span).
//!
//! **Every output frame depends only on its own index.** There is no filter
//! state between frames, so a run rendered in one piece, in pieces of any
//! length, or starting from a jump holds the same bits as the same frames cut
//! from a render of the whole clip. That is what lets playback and export
//! agree (M9.5 §2 E, "same bits") however the audio is prepared ahead, and why
//! the first piece after a jump needs no different recipe from the others.
//!
//! The position is read at a fixed grid of frames (every [`GRID`] frames,
//! counted from frame 0 of the output) and interpolated between, so the cost
//! of the position function (a tempo map lookup) is paid once per [`GRID`]
//! frames and the grid is the same whatever run is asked for.
//!
//! Slow work for a background thread: it reads the file and allocates its
//! scratch once.

use crate::MediaError;

/// Half the support of the interpolation kernel, in frames of the file at
/// unity speed: 16 on each side of the position.
pub const HALF: usize = 16;

/// Entries in the kernel table per frame of distance.
const PHASES: usize = 512;

/// The position function is read every this many output frames.
pub const GRID: i64 = 16;

/// An output frame whose position is within this of a whole frame, at a
/// speed within this of unity, is a copy of the file's frame: a span that
/// plays as it was made is not changed (D3).
const SNAP: f64 = 1.0e-7;

/// Where [`Renderer`] reads the file. Frames outside it are silence.
pub trait Source {
    /// Channels the file has: one or two.
    fn channels(&self) -> usize;

    /// Frames in the file.
    fn frames(&self) -> u64;

    /// Reads `out[c].len()` frames from frame `from` (at least 0) of channel
    /// `c`, one slice per channel; whatever lies past the end of the file is
    /// left as silence by the caller, so a short read is not an error.
    fn read(&mut self, from: u64, out: &mut [&mut [f32]]) -> Result<(), MediaError>;
}

/// The interpolation kernel, a Kaiser-windowed sinc, tabulated once.
pub struct Kernel {
    table: Box<[f32]>,
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

impl Kernel {
    pub fn new() -> Self {
        // One extra entry, so the last interval has a right neighbour.
        let n = HALF * PHASES + 2;
        let beta = 9.0_f64;
        let norm = bessel_i0(beta);
        let table = (0..n)
            .map(|i| {
                let x = i as f64 / PHASES as f64;
                if x >= HALF as f64 {
                    return 0.0;
                }
                let sinc = if x == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                };
                let r = x / HALF as f64;
                let window = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / norm;
                (sinc * window) as f32
            })
            .collect();
        Self { table }
    }

    /// The kernel at distance `x` (in kernel units, 0 to [`HALF`]).
    #[inline]
    fn at(&self, x: f64) -> f32 {
        let p = x * PHASES as f64;
        let i = p as usize;
        match (self.table.get(i), self.table.get(i + 1)) {
            (Some(&a), Some(&b)) => a + (b - a) * (p - i as f64) as f32,
            _ => 0.0,
        }
    }
}

/// The zeroth-order modified Bessel function, by its series.
fn bessel_i0(x: f64) -> f64 {
    let q = x * x / 4.0;
    let (mut term, mut sum) = (1.0, 1.0);
    for k in 1..60 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1.0e-16 * sum {
            break;
        }
    }
    sum
}

/// Renders runs of output frames. Holds the scratch a run needs, so make one
/// per worker.
pub struct Renderer {
    kernel: Kernel,
    /// The file's frames a run reads, per channel.
    window: [Vec<f32>; 2],
    /// The position at each grid frame a run touches.
    grid: Vec<f64>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            kernel: Kernel::new(),
            window: [Vec::new(), Vec::new()],
            grid: Vec::new(),
        }
    }

    /// Renders output frames `first..first + left.len()` into `left` and, for
    /// a file of two channels, `right` (both one length; a mono file leaves
    /// `right` alone). `position(k)` is the frame of the file, fractional,
    /// that plays at output frame `k`; it is called for the grid frames only,
    /// and must not depend on anything but `k`.
    pub fn render(
        &mut self,
        source: &mut dyn Source,
        position: &dyn Fn(i64) -> f64,
        first: i64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), MediaError> {
        let n = left.len();
        let stereo = source.channels() >= 2;
        left.fill(0.0);
        if stereo {
            right[..n].fill(0.0);
        }
        if n == 0 {
            return Ok(());
        }
        // The positions at the grid frames the run touches.
        let g0 = first.div_euclid(GRID);
        let g1 = (first + n as i64 - 1).div_euclid(GRID) + 1;
        self.grid.clear();
        self.grid.extend((g0..=g1).map(|g| position(g * GRID)));
        if self.grid.iter().any(|p| !p.is_finite()) {
            return Ok(());
        }
        // The frames of the file the run reads: from the lowest position
        // less the kernel to the highest plus it, the kernel widened by the
        // fastest speed in the run.
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        let mut fastest = 1.0_f64;
        for pair in self.grid.windows(2) {
            lo = lo.min(pair[0]).min(pair[1]);
            hi = hi.max(pair[0]).max(pair[1]);
            fastest = fastest.max(((pair[1] - pair[0]) / GRID as f64).abs());
        }
        let reach = (HALF as f64 * fastest).ceil() + 2.0;
        let from = (lo - reach).floor() as i64;
        let to = (hi + reach).ceil() as i64;
        let frames = source.frames() as i64;
        if to <= 0 || from >= frames {
            return Ok(());
        }
        let len = (to - from) as usize;
        for w in &mut self.window {
            w.clear();
            w.resize(len, 0.0);
        }
        // The part of the window the file covers.
        let (start, stop) = (from.max(0), to.min(frames));
        let (head, tail) = ((start - from) as usize, (stop - from) as usize);
        {
            let [a, b] = &mut self.window;
            let (a, b) = (&mut a[head..tail], &mut b[head..tail]);
            if stereo {
                source.read(start as u64, &mut [a, b])?;
            } else {
                source.read(start as u64, &mut [a])?;
            }
        }
        for (i, frame) in (first..first + n as i64).enumerate() {
            let g = (frame.div_euclid(GRID) - g0) as usize;
            let along = frame.rem_euclid(GRID) as f64 / GRID as f64;
            let (a, b) = (self.grid[g], self.grid[g + 1]);
            let p = a + (b - a) * along;
            let speed = ((b - a) / GRID as f64).abs();
            let whole = p.round();
            if (p - whole).abs() < SNAP && (speed - 1.0).abs() < SNAP {
                // Played as it was made: the file's own frame.
                let at = whole as i64 - from;
                if (0..len as i64).contains(&at) {
                    left[i] = self.window[0][at as usize];
                    if stereo {
                        right[i] = self.window[1][at as usize];
                    }
                }
                continue;
            }
            // Wider than the file's own band when the file runs fast.
            let cut = (1.0 / speed.max(1.0)).min(1.0);
            let half = HALF as f64 / cut;
            let first_tap = (p - half).ceil() as i64;
            let last_tap = (p + half).floor() as i64;
            let (mut sl, mut sr) = (0.0_f32, 0.0_f32);
            for tap in first_tap..=last_tap {
                let at = tap - from;
                if at < 0 || at >= len as i64 {
                    continue;
                }
                let w = self.kernel.at(((tap as f64 - p) * cut).abs()) * cut as f32;
                sl += self.window[0][at as usize] * w;
                if stereo {
                    sr += self.window[1][at as usize] * w;
                }
            }
            left[i] = sl;
            if stereo {
                right[i] = sr;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
