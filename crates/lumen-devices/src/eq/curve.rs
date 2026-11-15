//! The curve a face draws: the whole EQ's response on a log frequency axis,
//! dense enough that a line between two points has no visible corner, with
//! extra points where a band's own shape turns (M8 slice B1, the owner's
//! review OR-7).
//!
//! The response is the filter's own — every band's sections, evaluated at
//! `z = e^{jw}` of the coefficients the sound runs — worked out as two
//! quadratics in `cos w` a section (`bands::Ratio`), so a point is one cosine
//! and a handful of products whatever the bands are.
//!
//! # Why it is adaptive
//!
//! A bell of Q 8 at 8 kHz is a needle 1.1 % of an octave wide: on a 700-pixel
//! screen that is under a quarter of a pixel of the axis, and a fixed grid
//! either misses it or draws it as one corner. [`Curve::path`] therefore takes
//! a base grid half a device pixel apart and subdivides it eight times inside
//! each band's own turn (two octaves for a shelf or a cut, about `1.5/Q`
//! octaves for a bell), which is where the curvature is. Between bands the
//! curve is nearly straight and needs nothing more.
//!
//! The path is the same arithmetic as [`super::response`]: a test holds it to a
//! hundredth of a decibel of the sections added up directly.

use super::bands::{Kind, MOST_SECTIONS, Section, Setting};
use super::{BANDS, HIGH_HZ, LOW_HZ};

/// The closest two points of a path come, in device pixels.
pub const STEP_PX: f32 = 0.5;
/// How many times closer the points are inside a band's own turn.
pub const REFINE: usize = 8;
/// The most points a path takes, whatever the screen's width: a screen wider
/// than this gets a half-pixel grid's worth and no more.
pub const MOST: usize = 2048;

/// How far either side of its centre a shelf or a cut turns, in octaves.
const TURN_OCTAVES: f32 = 2.0;
/// A bell's or a notch's turn, in octaves, is this over its Q.
const BELL_TURN_OCTAVES: f32 = 2.0;

/// One point of a path: how far across the frequency axis (0 at [`LOW_HZ`], 1
/// at [`HIGH_HZ`]) and the response there in decibels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub share: f32,
    pub db: f32,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            share: 0.0,
            db: 0.0,
        }
    }
}

/// One band as the curve reads it: its sections, worked out once, and the
/// stretch of the axis its own shape turns on (empty when it is off).
#[derive(Debug, Clone, Copy)]
struct Band {
    sections: [Section; MOST_SECTIONS],
    count: usize,
    turn: Option<(f32, f32)>,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            sections: [Section::FLAT; MOST_SECTIONS],
            count: 0,
            turn: None,
        }
    }
}

/// Where a frequency is on the axis: 0 at the low end, 1 at the high.
fn share_of(hz: f32) -> f32 {
    super::axis(hz)
}

/// The whole EQ's response on the axis the face draws, from settings worked
/// out once. Made on the interface side, once a frame.
#[derive(Debug, Clone)]
pub struct Curve {
    bands: [Band; BANDS],
    output: f32,
    rate: u32,
    low: f32,
    /// `ln(high / low)`: the axis is `low · e^(share · span)`.
    span: f64,
}

impl Curve {
    /// The curve of `settings` (a band and whether it is on), with `output`
    /// decibels added, at `rate`.
    pub fn new(settings: [(Setting, bool); BANDS], output: f32, rate: u32) -> Self {
        let rate = rate.max(1);
        let mut bands = [Band::default(); BANDS];
        for (band, (setting, on)) in bands.iter_mut().zip(settings) {
            if !on {
                continue;
            }
            band.count = setting.sections(rate, &mut band.sections);
            // How far either side of its centre the band's own shape turns:
            // a shelf and a cut over octaves, a bell and a notch by its width.
            let octaves = match setting.kind {
                Kind::Bell | Kind::Notch => BELL_TURN_OCTAVES / setting.q.max(1e-3),
                _ => TURN_OCTAVES,
            }
            .clamp(0.01, 4.0);
            let half = octaves * std::f32::consts::LOG10_2 / (HIGH_HZ / LOW_HZ).log10();
            let centre = share_of(setting.freq);
            band.turn = Some((centre - half, centre + half));
        }
        Self {
            bands,
            output,
            rate,
            low: LOW_HZ,
            span: f64::from((HIGH_HZ / LOW_HZ).ln()),
        }
    }

    /// The response at `share` of the way across the axis, in decibels.
    pub fn db_at(&self, share: f32) -> f32 {
        self.db_at_hz(self.hz_at(share))
    }

    /// The frequency at `share` of the way across the axis.
    pub fn hz_at(&self, share: f32) -> f32 {
        (f64::from(self.low) * (self.span * f64::from(share)).exp()) as f32
    }

    /// The response at `hz`, in decibels: one cosine, then every section.
    pub fn db_at_hz(&self, hz: f32) -> f32 {
        let w = std::f64::consts::TAU * f64::from(hz.max(0.0)) / f64::from(self.rate);
        let cos = w.cos();
        let mut power = 1.0f64;
        for band in &self.bands {
            for section in &band.sections[..band.count] {
                power *= section.ratio().power(cos);
            }
        }
        let db = 10.0 * power.max(f64::MIN_POSITIVE).log10();
        db as f32 + self.output
    }

    /// Fills `out` with the response at points spread evenly across the axis:
    /// `out.len()` of them from the low end to the high.
    pub fn uniform(&self, out: &mut [f32]) {
        let last = out.len().saturating_sub(1).max(1) as f32;
        for (i, point) in out.iter_mut().enumerate() {
            *point = self.db_at(i as f32 / last);
        }
    }

    /// Fills `out` with the path a face draws across `width_px` device pixels,
    /// and answers how many points it wrote: a point every half pixel, and
    /// eight times as many across each band's own turn, up to [`MOST`].
    pub fn path(&self, width_px: f32, out: &mut [Sample]) -> usize {
        let cap = out.len().min(MOST);
        if cap == 0 {
            return 0;
        }
        if cap == 1 {
            out[0] = self.sample(0.0);
            return 1;
        }
        let width = width_px.max(1.0);
        // The base grid: as fine as a half pixel, coarser only when that plus
        // the turns' extra points would take more than a path holds.
        let coverage = self.coverage();
        let room = (cap as f32 / (1.0 + (REFINE as f32 - 1.0) * coverage)).floor() as usize;
        let room = room.clamp(2, cap);
        let step = (width / (room - 1) as f32).max(STEP_PX);
        let base = ((width / step).ceil() as usize + 1).clamp(2, room);
        let last = (base - 1) as f32;
        let mut count = 0;
        for i in 0..base {
            if count >= cap {
                break;
            }
            let share = i as f32 / last;
            out[count] = self.sample(share);
            count += 1;
            if i + 1 == base || count + REFINE > cap {
                continue;
            }
            let next = (i + 1) as f32 / last;
            // An extra point or three between the grid's own: the stretch is
            // inside a band's turn, where the curve bends.
            if self.turns_between(share, next) {
                for k in 1..REFINE {
                    let at = share + (next - share) * k as f32 / REFINE as f32;
                    out[count] = self.sample(at);
                    count += 1;
                }
            }
        }
        count
    }

    /// Whether any band's own turn overlaps the stretch of the axis from `from`
    /// to `to`: where a straight line between two points would show a corner.
    fn turns_between(&self, from: f32, to: f32) -> bool {
        self.bands
            .iter()
            .filter_map(|band| band.turn)
            .any(|(a, b)| {
                let (a, b) = (a.min(b), a.max(b));
                a < to && b > from
            })
    }

    /// How much of the axis is inside some band's turn, 0 to 1: how much of a
    /// path may need the finer step.
    fn coverage(&self) -> f32 {
        let mut total = 0.0;
        for (a, b) in self.bands.iter().filter_map(|band| band.turn) {
            let low = a.min(b).max(0.0);
            let high = a.max(b).min(1.0);
            if high > low {
                total += high - low;
            }
        }
        total.min(1.0)
    }

    fn sample(&self, share: f32) -> Sample {
        Sample {
            share,
            db: self.db_at(share),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::bands::band_db;
    use super::*;

    /// A patch of bands: a low cut, two bells and a high shelf, in their own
    /// units, all on.
    fn patch() -> ([(Setting, bool); BANDS], f32) {
        let flat = Setting {
            kind: Kind::Bell,
            freq: 1_000.0,
            gain: 0.0,
            q: 1.0,
            slope: 12.0,
        };
        let mut settings = [(flat, false); BANDS];
        settings[0] = (
            Setting {
                kind: Kind::LowCut,
                freq: 30.0,
                gain: 0.0,
                q: 0.7,
                slope: 24.0,
            },
            true,
        );
        settings[1] = (
            Setting {
                kind: Kind::Bell,
                freq: 90.0,
                gain: 4.0,
                q: 1.2,
                slope: 12.0,
            },
            true,
        );
        settings[2] = (
            Setting {
                kind: Kind::Bell,
                freq: 1_000.0,
                gain: -6.0,
                q: 4.0,
                slope: 12.0,
            },
            true,
        );
        settings[5] = (
            Setting {
                kind: Kind::HighShelf,
                freq: 6_000.0,
                gain: 3.0,
                q: 0.7,
                slope: 12.0,
            },
            true,
        );
        (settings, 0.0)
    }

    const RATE: u32 = 48_000;
    /// The face's own scale: 5.5 points a decibel.
    const PT_PER_DB: f32 = 5.5;

    /// The curve is the bands added up: a hundredth of a decibel, everywhere.
    #[test]
    fn the_curve_is_the_sections_added_up() {
        let (settings, output) = patch();
        let curve = Curve::new(settings, output, RATE);
        for i in 0..400 {
            let hz = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(i as f32 / 399.0);
            let mut want = output;
            for (setting, on) in settings {
                if on {
                    want += band_db(&setting, RATE, hz);
                }
            }
            let got = curve.db_at_hz(hz);
            assert!(
                (got - want).abs() < 0.01,
                "{hz:.1} Hz: {got} against {want}"
            );
        }
    }

    /// The path is dense where a band turns and no denser than it needs to be
    /// away from them: a half-pixel base, subdivided four times at a peak.
    #[test]
    fn the_path_is_denser_where_a_band_turns() {
        let side = Setting {
            kind: Kind::Bell,
            freq: 1_000.0,
            gain: 12.0,
            q: 8.0,
            slope: 12.0,
        };
        let flat = Setting {
            kind: Kind::Bell,
            freq: 1_000.0,
            gain: 0.0,
            q: 1.0,
            slope: 12.0,
        };
        let mut settings = [(flat, false); BANDS];
        settings[2] = (side, true);
        let curve = Curve::new(settings, 0.0, RATE);
        let mut path = [Sample::default(); MOST];
        let width = 352.0;
        let count = curve.path(width, &mut path);
        assert!(count >= (width / STEP_PX) as usize, "{count} points");
        assert!(count <= MOST);
        let path = &path[..count];
        assert_eq!(path[0].share, 0.0);
        assert_eq!(path[count - 1].share, 1.0);
        // No point twice, and every one in order.
        for pair in path.windows(2) {
            assert!(pair[1].share > pair[0].share, "{:?}", pair);
        }
        let spacing = |share: f32| {
            let mut gaps: Vec<f32> = path
                .iter()
                .map(|s| s.share)
                .filter(|s| (s - share).abs() < 0.01)
                .collect::<Vec<_>>()
                .windows(2)
                .map(|w| w[1] - w[0])
                .collect();
            gaps.sort_by(f32::total_cmp);
            gaps.get(gaps.len() / 2).copied().unwrap_or(0.0)
        };
        let at_the_peak = spacing(share_of(1_000.0));
        let far_away = spacing(share_of(30.0));
        assert!(
            at_the_peak * 3.0 < far_away,
            "{at_the_peak} at the peak against {far_away} at 30 Hz"
        );
    }

    /// A line between two points of the path is within a tenth of a point of
    /// the curve it stands for: there is no corner to see, even on a screen
    /// twice as sharp as a 350-point card at 200 %. The same picture on the
    /// ninety-six points the face drew before misses by points, at the peaks.
    #[test]
    fn the_path_has_no_corner_a_pen_could_show() {
        let width = 704.0;
        for (q, gain) in [(4.0, 12.0), (8.0, 24.0), (2.0, -9.0)] {
            let side = Setting {
                kind: Kind::Bell,
                freq: 1_000.0,
                gain,
                q,
                slope: 12.0,
            };
            let flat = Setting {
                kind: Kind::Bell,
                freq: 1_000.0,
                gain: 0.0,
                q: 1.0,
                slope: 12.0,
            };
            let mut settings = [(flat, false); BANDS];
            settings[2] = (side, true);
            let curve = Curve::new(settings, 0.0, RATE);
            let mut path = [Sample::default(); MOST];
            let count = curve.path(width, &mut path);
            let sag = |from: f32, to: f32| {
                let middle = (from + to) / 2.0;
                let straight = (curve.db_at(from) + curve.db_at(to)) / 2.0;
                (curve.db_at(middle) - straight).abs() * PT_PER_DB
            };
            let mut worst: f32 = 0.0;
            for pair in path[..count].windows(2) {
                worst = worst.max(sag(pair[0].share, pair[1].share));
            }
            assert!(
                worst < 0.25,
                "Q {q} {gain} dB: {worst} points off the curve"
            );

            // The same bell on a fixed ninety-six points, as B1 drew it.
            let mut old = [0.0f32; 96];
            curve.uniform(&mut old);
            let last = (old.len() - 1) as f32;
            let mut worst_old: f32 = 0.0;
            for i in 0..old.len() - 1 {
                let (from, to) = (i as f32 / last, (i + 1) as f32 / last);
                let straight = (old[i] + old[i + 1]) / 2.0;
                worst_old =
                    worst_old.max((curve.db_at((from + to) / 2.0) - straight).abs() * PT_PER_DB);
            }
            assert!(
                worst_old > 10.0 * worst,
                "Q {q} {gain} dB: {worst_old} points before, {worst} now"
            );
            assert!(
                worst_old > 1.0,
                "Q {q} {gain} dB: the ninety-six-point picture was already smooth at {worst_old}"
            );
        }
    }

    /// Near a peak the sampled curve bends by no more than a twentieth of a
    /// decibel from one point to the next: the second difference along the log
    /// axis is bounded. At ninety-six points a Q 4 bell bent by decibels.
    #[test]
    fn the_second_difference_near_a_peak_is_bounded() {
        let side = Setting {
            kind: Kind::Bell,
            freq: 1_000.0,
            gain: 12.0,
            q: 4.0,
            slope: 12.0,
        };
        let flat = Setting {
            kind: Kind::Bell,
            freq: 1_000.0,
            gain: 0.0,
            q: 1.0,
            slope: 12.0,
        };
        let mut settings = [(flat, false); BANDS];
        settings[2] = (side, true);
        let curve = Curve::new(settings, 0.0, RATE);
        let bent = |points: &[f32]| {
            points
                .windows(3)
                .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
                .fold(0.0f32, f32::max)
        };
        let mut dense = [0.0f32; 2048];
        curve.uniform(&mut dense);
        assert!(bent(&dense) < 0.05, "{} dB", bent(&dense));
        let mut old = [0.0f32; 96];
        curve.uniform(&mut old);
        assert!(bent(&old) > 1.0, "{} dB", bent(&old));
    }

    /// A curve of silence is flat, and a band that is off is not in it.
    #[test]
    fn a_flat_patch_draws_a_straight_line() {
        let flat = Setting {
            kind: Kind::Bell,
            freq: 1_000.0,
            gain: 0.0,
            q: 1.0,
            slope: 12.0,
        };
        let curve = Curve::new([(flat, true); BANDS], 1.5, RATE);
        let mut path = [Sample::default(); MOST];
        let count = curve.path(704.0, &mut path);
        for sample in &path[..count] {
            assert!((sample.db - 1.5).abs() < 1e-4, "{:?}", sample);
        }
    }

    /// A path that may not hold a half-pixel grid still covers the axis and
    /// fits the buffer: a narrow card and a small buffer both draw.
    #[test]
    fn a_small_buffer_or_a_narrow_screen_still_covers_the_axis() {
        let (settings, output) = patch();
        let curve = Curve::new(settings, output, RATE);
        for (width, room) in [(8.0, MOST), (3_000.0, MOST), (704.0, 64), (704.0, 2)] {
            let mut path = [Sample::default(); MOST];
            let count = curve.path(width, &mut path[..room]);
            assert!(count >= 1 && count <= room, "{width} px in {room}: {count}");
            assert_eq!(path[0].share, 0.0);
            if count > 1 {
                assert_eq!(path[count - 1].share, 1.0);
            }
        }
    }

    /// A measurement, not a test: what a path costs a frame at the widest
    /// screen a card is drawn on and at a third of it, with four bands on.
    #[test]
    #[ignore = "a measurement: prints times, asserts only a generous bound"]
    fn the_cost_of_a_path() {
        let (settings, output) = patch();
        let curve = Curve::new(settings, output, RATE);
        let mut path = [Sample::default(); MOST];
        for width in [176.0, 352.0, 704.0] {
            // Warm the caches, then measure a hundred frames.
            let mut count = curve.path(width, &mut path);
            let mut times = Vec::new();
            for _ in 0..100 {
                let started = std::time::Instant::now();
                count = curve.path(width, &mut path);
                times.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            println!(
                "iris curve at {width} px: {count} points ({:.2} px a point), {:.3} ms median, {:.3} ms worst",
                width / count as f32,
                times[50],
                times[99]
            );
            assert!(times[50] < 10.0, "{} ms", times[50]);
        }
    }
}
