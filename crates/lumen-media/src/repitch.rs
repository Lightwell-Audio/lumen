use crate::MediaError;

pub const HALF: usize = 16;

const PHASES: usize = 512;

pub const GRID: i64 = 16;

const SNAP: f64 = 1.0e-7;

pub type OfFrame<'a> = &'a dyn Fn(i64) -> f64;

pub trait Source {
    fn channels(&self) -> usize;

    fn frames(&self) -> u64;

    fn read(&mut self, from: u64, out: &mut [&mut [f32]]) -> Result<(), MediaError>;
}

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

pub struct Renderer {
    kernel: Kernel,
    window: [Vec<f32>; 2],
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
        let g0 = first.div_euclid(GRID);
        let g1 = (first + n as i64 - 1).div_euclid(GRID) + 1;
        self.grid.clear();
        self.grid.extend((g0..=g1).map(|g| position(g * GRID)));
        if self.grid.iter().any(|p| !p.is_finite()) {
            return Ok(());
        }
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
                let at = whole as i64 - from;
                if (0..len as i64).contains(&at) {
                    left[i] = self.window[0][at as usize];
                    if stereo {
                        right[i] = self.window[1][at as usize];
                    }
                }
                continue;
            }
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
