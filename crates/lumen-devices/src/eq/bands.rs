//! What an EQ band is as a filter: second-order sections, their coefficients
//! from the standard cookbook forms (M8 slice B1, ADR-0146), and the magnitude
//! response a face draws from them.
//!
//! Everything here is a pure function of a band's own units, so the same code
//! answers what the face draws, what the running filter does and what a test
//! measures: one place a coefficient is worked out, three readers.

use std::f32::consts::TAU;

/// The most sections one band takes: a 48 dB/octave cut is four of them.
pub const MOST_SECTIONS: usize = 4;

/// One second-order section, its coefficients over `a0`:
/// `(b0 + b1 z⁻¹ + b2 z⁻²) / (1 + a1 z⁻¹ + a2 z⁻²)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Section {
    /// Nothing at all: what a band that is off passes.
    pub const FLAT: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// The section's gain at `hz`, in decibels. Worked out in double, so a
    /// deep stopband (where both polynomials are tiny) still reads its own
    /// level rather than a rounding of it.
    pub fn db(&self, hz: f32, rate: u32) -> f32 {
        let w = std::f64::consts::TAU * f64::from(hz.max(0.0)) / f64::from(rate.max(1));
        let (sin1, cos1) = (w.sin(), w.cos());
        let (sin2, cos2) = ((2.0 * w).sin(), (2.0 * w).cos());
        let (b0, b1, b2) = (f64::from(self.b0), f64::from(self.b1), f64::from(self.b2));
        let (a1, a2) = (f64::from(self.a1), f64::from(self.a2));
        // z⁻¹ = cos − j sin, z⁻² = cos 2w − j sin 2w.
        let num = (b0 + b1 * cos1 + b2 * cos2, -(b1 * sin1 + b2 * sin2));
        let den = (1.0 + a1 * cos1 + a2 * cos2, -(a1 * sin1 + a2 * sin2));
        let magnitude = (num.0 * num.0 + num.1 * num.1).sqrt()
            / (den.0 * den.0 + den.1 * den.1).sqrt().max(f64::MIN_POSITIVE);
        if magnitude <= 0.0 {
            -200.0
        } else {
            (20.0 * magnitude.log10()) as f32
        }
    }

    /// Whether it passes everything (a band that is off).
    pub fn is_flat(&self) -> bool {
        (self.b0 - 1.0).abs() < 1e-9
            && self.b1 == 0.0
            && self.b2 == 0.0
            && self.a1 == 0.0
            && self.a2 == 0.0
    }

    /// The section one step further from `from` towards `to`, a `share` of the
    /// way: what a band's coefficients glide along (see [`crate::eq`]).
    pub fn between(from: Self, to: Self, share: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * share;
        Self {
            b0: mix(from.b0, to.b0),
            b1: mix(from.b1, to.b1),
            b2: mix(from.b2, to.b2),
            a1: mix(from.a1, to.a1),
            a2: mix(from.a2, to.a2),
        }
    }
}

/// The Q of each section of a maximally flat cascade: 12, 24, 36 and 48
/// dB/octave, one to four sections (a Butterworth alignment, so the sum has no
/// peak at the corner).
const BUTTERWORTH: [[f32; MOST_SECTIONS]; 4] = [
    [0.707_106_8, 0.0, 0.0, 0.0],
    [0.541_196_1, 1.306_563, 0.0, 0.0],
    [0.517_638_1, 0.707_106_8, 1.931_851_7, 0.0],
    [0.509_795_6, 0.601_344_7, 0.899_976_2, 2.562_915_3],
];

/// What a band does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A cut under the frequency, 12 to 48 dB an octave.
    LowCut,
    /// A peak or a dip at the frequency.
    Bell,
    LowShelf,
    HighShelf,
    /// A narrow dip at the frequency; its gain is nothing.
    Notch,
    /// A cut over the frequency, 12 to 48 dB an octave.
    HighCut,
}

impl Kind {
    /// The word this kind is saved and read as, index for index with the Type
    /// parameter's choices.
    pub const CHOICES: [&'static str; 4] = ["Bell", "Shelf low", "Shelf high", "Notch"];

    /// The kinds the six middle bands can be, chosen by the Type parameter.
    pub fn of_choice(index: usize) -> Self {
        match index {
            1 => Self::LowShelf,
            2 => Self::HighShelf,
            3 => Self::Notch,
            _ => Self::Bell,
        }
    }

    /// The word a person reads for this band ("bell", "low cut").
    pub fn word(self) -> &'static str {
        match self {
            Self::LowCut => "low cut",
            Self::Bell => "bell",
            Self::LowShelf => "low shelf",
            Self::HighShelf => "high shelf",
            Self::Notch => "notch",
            Self::HighCut => "high cut",
        }
    }

    /// Whether the band has a gain, and a Q.
    pub fn has_gain(self) -> bool {
        matches!(self, Self::Bell | Self::LowShelf | Self::HighShelf)
    }

    pub fn has_q(self) -> bool {
        matches!(self, Self::Bell | Self::Notch)
    }

    /// Whether its slope is what the slope parameter sets.
    pub fn has_slope(self) -> bool {
        matches!(self, Self::LowCut | Self::HighCut)
    }
}

/// A band's settings in its own units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setting {
    pub kind: Kind,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    /// 12, 24, 36 or 48 dB an octave, for a cut.
    pub slope: f32,
}

impl Setting {
    /// The sections this band is, and how many; `out` holds
    /// [`MOST_SECTIONS`].
    pub fn sections(&self, rate: u32, out: &mut [Section; MOST_SECTIONS]) -> usize {
        let rate = rate.max(1);
        let hz = self.freq.clamp(1.0, rate as f32 * 0.49);
        match self.kind {
            Kind::LowCut | Kind::HighCut => {
                let count = ((self.slope / 12.0).round().clamp(1.0, 4.0)) as usize;
                let qs = BUTTERWORTH[count - 1];
                for (i, section) in out.iter_mut().enumerate().take(count) {
                    *section = cascaded(self.kind, hz, qs[i], rate);
                }
                count
            }
            Kind::Bell => {
                out[0] = bell(hz, self.gain, self.q, rate);
                1
            }
            Kind::LowShelf => {
                out[0] = shelf(hz, self.gain, true, rate);
                1
            }
            Kind::HighShelf => {
                out[0] = shelf(hz, self.gain, false, rate);
                1
            }
            Kind::Notch => {
                out[0] = notch(hz, self.q, rate);
                1
            }
        }
    }
}

/// The analytic answer for a whole band at `hz`, from its own units: what the
/// face draws and what a test measures against.
pub fn band_db(setting: &Setting, rate: u32, hz: f32) -> f32 {
    let mut sections = [Section::FLAT; MOST_SECTIONS];
    let count = setting.sections(rate, &mut sections);
    sections[..count]
        .iter()
        .map(|section| section.db(hz, rate))
        .sum()
}

/// `w0` and `alpha` for a corner at `hz`.
fn corner(hz: f32, q: f32, rate: u32) -> (f32, f32, f32) {
    let w0 = TAU * hz / rate as f32;
    let alpha = w0.sin() / (2.0 * q.max(1e-4));
    (w0, w0.cos(), alpha)
}

/// A bell: a peak or a dip of `gain` decibels at `hz`.
fn bell(hz: f32, gain: f32, q: f32, rate: u32) -> Section {
    let (_, cos, alpha) = corner(hz, q, rate);
    let a = 10f32.powf(gain / 40.0);
    let a0 = 1.0 + alpha / a;
    Section {
        b0: (1.0 + alpha * a) / a0,
        b1: -2.0 * cos / a0,
        b2: (1.0 - alpha * a) / a0,
        a1: -2.0 * cos / a0,
        a2: (1.0 - alpha / a) / a0,
    }
}

/// A shelf of `gain` decibels over (or under) `hz`, its Q fixed to the
/// steepest slope that does not ripple.
fn shelf(hz: f32, gain: f32, low: bool, rate: u32) -> Section {
    let (_, cos, _) = corner(hz, 0.707_106_8, rate);
    let a = 10f32.powf(gain / 40.0);
    let alpha = (TAU * hz / rate as f32).sin() / 2.0 * std::f32::consts::SQRT_2;
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
    let (b0, b1, b2, a0, a1, a2) = if low {
        (
            a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha),
            (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha,
        )
    } else {
        (
            a * ((a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha),
            (a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha,
        )
    };
    Section {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

/// A notch at `hz`: the band around the frequency taken away.
fn notch(hz: f32, q: f32, rate: u32) -> Section {
    let (_, cos, alpha) = corner(hz, q, rate);
    let a0 = 1.0 + alpha;
    Section {
        b0: 1.0 / a0,
        b1: -2.0 * cos / a0,
        b2: 1.0 / a0,
        a1: -2.0 * cos / a0,
        a2: (1.0 - alpha) / a0,
    }
}

/// One section of a cut: a cutoff filter of the given Q.
fn cascaded(kind: Kind, hz: f32, q: f32, rate: u32) -> Section {
    let (_, cos, alpha) = corner(hz, q, rate);
    let a0 = 1.0 + alpha;
    let (b0, b1) = if kind == Kind::LowCut {
        ((1.0 + cos) / 2.0, -(1.0 + cos))
    } else {
        ((1.0 - cos) / 2.0, 1.0 - cos)
    };
    Section {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b0 / a0,
        a1: -2.0 * cos / a0,
        a2: (1.0 - alpha) / a0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// The sections of an RBJ peaking filter, written out again here from the
    /// cookbook, so the code under test is checked against the formulas rather
    /// than against itself.
    fn rbj_peaking(hz: f32, gain: f32, q: f32, rate: f32) -> [f64; 5] {
        let w0 = 2.0 * std::f64::consts::PI * f64::from(hz) / f64::from(rate);
        let a = 10f64.powf(f64::from(gain) / 40.0);
        let alpha = w0.sin() / (2.0 * f64::from(q));
        let a0 = 1.0 + alpha / a;
        [
            (1.0 + alpha * a) / a0,
            (-2.0 * w0.cos()) / a0,
            (1.0 - alpha * a) / a0,
            (-2.0 * w0.cos()) / a0,
            (1.0 - alpha / a) / a0,
        ]
    }

    fn magnitude(c: &[f64; 5], hz: f32, rate: f32) -> f64 {
        let w = 2.0 * std::f64::consts::PI * f64::from(hz) / f64::from(rate);
        let (s1, c1) = (w.sin(), w.cos());
        let (s2, c2) = ((2.0 * w).sin(), (2.0 * w).cos());
        let num = ((c[0] + c[1] * c1 + c[2] * c2).powi(2) + (c[1] * s1 + c[2] * s2).powi(2)).sqrt();
        let den = ((1.0 + c[3] * c1 + c[4] * c2).powi(2) + (c[3] * s1 + c[4] * s2).powi(2)).sqrt();
        num / den
    }

    #[test]
    fn a_bell_is_the_cookbook_s_peaking_filter() {
        for (hz, gain, q) in [(1_000.0, 6.0, 2.0), (80.0, -9.0, 0.5), (12_000.0, 12.0, 8.0)] {
            let setting = Setting {
                kind: Kind::Bell,
                freq: hz,
                gain,
                q,
                slope: 12.0,
            };
            let mut sections = [Section::FLAT; MOST_SECTIONS];
            assert_eq!(setting.sections(RATE, &mut sections), 1);
            let want = rbj_peaking(hz, gain, q, 48_000.0);
            for probe in [20.0, 100.0, 500.0, 1_000.0, 2_500.0, 10_000.0, 19_000.0] {
                let got = f64::from(sections[0].db(probe, RATE));
                let want = 20.0 * magnitude(&want, probe, 48_000.0).log10();
                // The coefficients are held as `f32` (the filter's form), so
                // a deep stopband reads a hundredth of a decibel off the
                // formula's own answer; the brief's tolerance is a tenth.
                assert!(
                    (got - want).abs() < 0.02,
                    "{hz} {gain} {q} at {probe}: {got} against {want}"
                );
            }
            // Its own frequency is the peak it was asked for.
            assert!((sections[0].db(hz, RATE) - gain).abs() < 0.01);
        }
    }

    /// A cut falls by its slope once an octave past the corner and is 3 dB
    /// down at it, whatever the slope.
    #[test]
    fn a_cut_falls_by_its_slope_an_octave_past_the_corner() {
        for (slope, sections) in [(12.0, 1), (24.0, 2), (36.0, 3), (48.0, 4)] {
            let setting = Setting {
                kind: Kind::LowCut,
                freq: 1_000.0,
                gain: 0.0,
                q: 0.7,
                slope,
            };
            let at_corner = band_db(&setting, RATE, 1_000.0);
            assert!((at_corner - -3.01).abs() < 0.2, "{slope}: {at_corner}");
            let below = band_db(&setting, RATE, 500.0);
            assert!((below + slope).abs() < 0.5, "{slope}: {below}");
            let mut out = [Section::FLAT; MOST_SECTIONS];
            assert_eq!(setting.sections(RATE, &mut out), sections);
        }
        // A high cut is the mirror: it takes the top away and leaves the rest.
        let high = Setting {
            kind: Kind::HighCut,
            freq: 1_000.0,
            gain: 0.0,
            q: 0.7,
            slope: 24.0,
        };
        assert!(band_db(&high, RATE, 100.0).abs() < 0.05);
        assert!((band_db(&high, RATE, 2_000.0) - -24.0).abs() < 0.5);
    }

    #[test]
    fn a_shelf_lifts_its_side_and_leaves_the_other_alone() {
        let low = Setting {
            kind: Kind::LowShelf,
            freq: 200.0,
            gain: 6.0,
            q: 0.7,
            slope: 12.0,
        };
        assert!((band_db(&low, RATE, 20.0) - 6.0).abs() < 0.1);
        assert!(band_db(&low, RATE, 8_000.0).abs() < 0.05);
        let high = Setting {
            kind: Kind::HighShelf,
            freq: 4_000.0,
            gain: -6.0,
            q: 0.7,
            slope: 12.0,
        };
        assert!((band_db(&high, RATE, 19_000.0) - -6.0).abs() < 0.1);
        assert!(band_db(&high, RATE, 100.0).abs() < 0.05);
    }

    #[test]
    fn a_notch_takes_its_own_frequency_away_and_nothing_else() {
        let notch = Setting {
            kind: Kind::Notch,
            freq: 1_000.0,
            gain: 0.0,
            q: 4.0,
            slope: 12.0,
        };
        assert!(band_db(&notch, RATE, 1_000.0) < -40.0);
        assert!(band_db(&notch, RATE, 100.0).abs() < 0.1);
        assert!(band_db(&notch, RATE, 10_000.0).abs() < 0.1);
    }

    /// A section glides along a straight line in its coefficients, which is
    /// what makes a move click-free: at the halfway point every coefficient is
    /// halfway.
    #[test]
    fn a_section_glides_straight_between_two_coefficients() {
        let from = Section::FLAT;
        let to = bell(1_000.0, 6.0, 1.0, RATE);
        let half = Section::between(from, to, 0.5);
        assert!((half.b0 - (from.b0 + to.b0) / 2.0).abs() < 1e-7);
        assert!((half.a2 - to.a2 / 2.0).abs() < 1e-7);
        assert_eq!(Section::between(from, to, 0.0), from);
        assert_eq!(Section::between(from, to, 1.0), to);
        assert!(Section::FLAT.is_flat());
        assert!(!to.is_flat());
        // The gain at half way is between the two ends, never beyond them.
        let db = half.db(1_000.0, RATE);
        assert!(db > 0.0 && db < 6.0, "{db}");
    }
}
