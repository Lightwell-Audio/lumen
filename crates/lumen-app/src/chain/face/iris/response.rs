use eframe::egui::Pos2;
use lumen_devices::eq::bands::{MOST_SECTIONS, Ratio, Section, Setting};
use lumen_devices::eq::{self, Curve, Sample};

use super::bands::{Band, band};
use crate::chain::face::Values;

pub(super) const THIN: f32 = 0.03;
pub(super) const FILL_STEP: f32 = 1.5;

const SOLVE_STEPS: usize = 28;

fn flat() -> Setting {
    Setting {
        kind: eq::Kind::Bell,
        freq: 1_000.0,
        gain: 0.0,
        q: 1.0,
        slope: 12.0,
    }
}

pub(super) struct Response {
    settings: [(Setting, bool); eq::BANDS],
    output: f32,
    rate: u32,
    curve: Curve,
}

pub(super) fn response(values: &Values<'_>) -> Response {
    let mut settings = [(flat(), false); eq::BANDS];
    for (index, slot) in settings.iter_mut().enumerate() {
        let band = band(values, index);
        *slot = (band.setting(), band.on);
    }
    let output = values.plain(eq::OUTPUT);
    let rate = values.rate.max(1);
    Response {
        settings,
        output,
        rate,
        curve: Curve::new(settings, output, rate),
    }
}

impl Response {
    pub(super) fn db_at_hz(&self, hz: f32) -> f32 {
        self.curve.db_at_hz(hz)
    }

    pub(super) fn path(&self, width_px: f32, out: &mut [Sample]) -> usize {
        self.curve.path(width_px, out)
    }

    pub(super) fn without(&self, index: usize) -> Curve {
        let mut settings = self.settings;
        settings[index].1 = false;
        Curve::new(settings, self.output, self.rate)
    }

    pub(super) fn gain_for(&self, index: usize, band: &Band, freq: f32, target_db: f32) -> f32 {
        let need = target_db - self.without(index).db_at_hz(freq);
        let own = |gain: f32| {
            eq::band_db(
                &Setting {
                    freq,
                    gain,
                    ..band.setting()
                },
                self.rate,
                freq,
            )
        };
        let (mut low, mut high) = (-eq::MOST_DB, eq::MOST_DB);
        if need <= own(low) {
            return low;
        }
        if need >= own(high) {
            return high;
        }
        for _ in 0..SOLVE_STEPS {
            let middle = 0.5 * (low + high);
            if own(middle) < need {
                low = middle;
            } else {
                high = middle;
            }
        }
        0.5 * (low + high)
    }
}

pub(super) struct Shape {
    ratios: [Ratio; MOST_SECTIONS],
    count: usize,
    rate: u32,
}

impl Shape {
    pub(super) fn of(setting: &Setting, rate: u32) -> Self {
        let rate = rate.max(1);
        let mut sections = [Section::FLAT; MOST_SECTIONS];
        let count = setting.sections(rate, &mut sections);
        let mut ratios = [Section::FLAT.ratio(); MOST_SECTIONS];
        for (ratio, section) in ratios.iter_mut().zip(&sections[..count]) {
            *ratio = section.ratio();
        }
        Self {
            ratios,
            count,
            rate,
        }
    }

    pub(super) fn db_at_hz(&self, hz: f32) -> f32 {
        let w = std::f64::consts::TAU * f64::from(hz.max(0.0)) / f64::from(self.rate);
        let cos = w.cos();
        let power: f64 = self.ratios[..self.count]
            .iter()
            .map(|ratio| ratio.power(cos))
            .product();
        (10.0 * power.max(f64::MIN_POSITIVE).log10()) as f32
    }
}

pub(super) fn thin(points: &[Pos2], tolerance: f32) -> Vec<Pos2> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut ranges = vec![(0, n - 1)];
    while let Some((from, to)) = ranges.pop() {
        if to <= from + 1 {
            continue;
        }
        let (a, b) = (points[from], points[to]);
        let along = b - a;
        let length = along.length();
        let mut worst = (0.0_f32, from);
        for (offset, point) in points[from + 1..to].iter().enumerate() {
            let off = if length > 1e-6 {
                ((*point - a).x * along.y - (*point - a).y * along.x).abs() / length
            } else {
                (*point - a).length()
            };
            if off > worst.0 {
                worst = (off, from + 1 + offset);
            }
        }
        if worst.0 > tolerance {
            keep[worst.1] = true;
            ranges.push((from, worst.1));
            ranges.push((worst.1, to));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(point, kept)| kept.then_some(*point))
        .collect()
}
