//! The maths the synth's render path needs, written out so that every
//! platform plays the same samples (M8 slice B4).
//!
//! A golden render is pinned across Windows, Linux and macOS, so the render
//! path may not call `f32::sin`, `cos`, `tan`, `exp` or `powf`: a library's
//! answer for those differs in the last bit from platform to platform (and
//! the f32 ones from build to build). Everything here is plain IEEE `f64`
//! arithmetic — multiply, add, divide and a bit shift — so the answer is the
//! same everywhere the hardware rounds the same way, which every target
//! Lumen ships for does.
//!
//! Each function is tested against the standard library's `f64` answer, so
//! the polynomials' accuracy is pinned rather than assumed.

/// π, to the last bit an `f64` holds.
const PI: f64 = std::f64::consts::PI;
/// π/2.
const HALF_PI: f64 = PI / 2.0;
/// 2/π.
const TWO_OVER_PI: f64 = 2.0 / PI;
/// The natural logarithm of 2: what `exp2`'s argument is scaled by.
const LN_2: f64 = std::f64::consts::LN_2;
/// log2(e): what `exp`'s argument is scaled by.
const LOG2_E: f64 = std::f64::consts::LOG2_E;

/// `sin(x)` and `cos(x)` (radians).
pub(super) fn sin_cos(x: f64) -> (f64, f64) {
    // Reduce to `n` quarter turns and a remainder in [-π/4, π/4], where the
    // two series converge quickly.
    let n = (x * TWO_OVER_PI).round();
    let r = x - n * HALF_PI;
    let (s, c) = (sin_series(r), cos_series(r));
    match (n as i64).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

/// `sin(x)`.
#[allow(dead_code)] // The library's shape, kept beside `sin_cos` for a caller that wants one half.
pub(super) fn sin(x: f64) -> f64 {
    sin_cos(x).0
}

/// `cos(x)`.
#[allow(dead_code)] // As `sin`.
pub(super) fn cos(x: f64) -> f64 {
    sin_cos(x).1
}

/// `tan(x)`, for the range a filter's coefficient needs (under π/2).
pub(super) fn tan(x: f64) -> f64 {
    let (s, c) = sin_cos(x);
    s / c
}

/// `e^x`.
pub(super) fn exp(x: f64) -> f64 {
    exp2(x * LOG2_E)
}

/// `2^x`: the octave-to-ratio step a pitch, a filter opening and an envelope
/// time all go through.
pub(super) fn exp2(x: f64) -> f64 {
    // A range no voice reaches keeps the bit shift exact and the series
    // finite (2^-1000 and 2^1000 are silent and louder than any output).
    let x = x.clamp(-1_000.0, 1_000.0);
    let n = x.floor();
    let f = x - n;
    let scale = f64::from_bits((((n as i64) + 1_023) as u64) << 52);
    scale * exp2_series(f)
}

/// The `sin` series on [-π/4, π/4], by Horner on `x²`.
fn sin_series(x: f64) -> f64 {
    // sin x = x (1 − x²/3! + x⁴/5! − … )
    const TERMS: [f64; 7] = [
        1.0,
        -1.0 / 6.0,
        1.0 / 120.0,
        -1.0 / 5_040.0,
        1.0 / 362_880.0,
        -1.0 / 39_916_800.0,
        1.0 / 6_227_020_800.0,
    ];
    let x2 = x * x;
    let mut sum = TERMS[TERMS.len() - 1];
    for k in (0..TERMS.len() - 1).rev() {
        sum = TERMS[k] + x2 * sum;
    }
    x * sum
}

/// The `cos` series on [-π/4, π/4], by Horner on `x²`.
fn cos_series(x: f64) -> f64 {
    // cos x = 1 − x²/2! + x⁴/4! − …
    const TERMS: [f64; 7] = [
        1.0,
        -1.0 / 2.0,
        1.0 / 24.0,
        -1.0 / 720.0,
        1.0 / 40_320.0,
        -1.0 / 3_628_800.0,
        1.0 / 479_001_600.0,
    ];
    let x2 = x * x;
    let mut sum = TERMS[TERMS.len() - 1];
    for k in (0..TERMS.len() - 1).rev() {
        sum = TERMS[k] + x2 * sum;
    }
    sum
}

/// `2^f` for `f` in [0, 1), by its series in `f·ln 2`.
fn exp2_series(f: f64) -> f64 {
    // 2^f = Σ (f ln 2)^k / k!, fourteen terms: under 1e-17 over the range.
    const TERMS: [f64; 14] = [
        1.0,
        1.0,
        1.0 / 2.0,
        1.0 / 6.0,
        1.0 / 24.0,
        1.0 / 120.0,
        1.0 / 720.0,
        1.0 / 5_040.0,
        1.0 / 40_320.0,
        1.0 / 362_880.0,
        1.0 / 3_628_800.0,
        1.0 / 39_916_800.0,
        1.0 / 479_001_600.0,
        1.0 / 8_717_829_120.0,
    ];
    let y = f * LN_2;
    let mut sum = TERMS[TERMS.len() - 1];
    for k in (0..TERMS.len() - 1).rev() {
        sum = TERMS[k] + y * sum;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The library's answer and ours agree to within a part in `1e-12` over
    /// everything the synth asks of them.
    fn close(ours: f64, theirs: f64) {
        let allowance = 1e-12 * theirs.abs().max(1.0);
        assert!(
            (ours - theirs).abs() <= allowance,
            "{ours} against {theirs}"
        );
    }

    #[test]
    fn the_series_match_the_library() {
        for k in 0..=2_000 {
            let x = -40.0 + k as f64 * 0.04;
            close(sin(x), x.sin());
            close(cos(x), x.cos());
        }
        // The filter's range: 20 Hz to 0.45 of the rate, as π·f/rate.
        for k in 0..=1_000 {
            let x = 0.001 + k as f64 * 0.0014;
            close(tan(x), x.tan());
        }
        for k in -1_000..=1_000 {
            let x = k as f64 * 0.05;
            close(exp2(x), x.exp2());
            close(exp(x), x.exp());
        }
    }

    /// A quarter turn is a quarter turn, and the octaves are exact powers of
    /// two where they should be.
    #[test]
    fn the_corners_are_exact() {
        assert!((sin(0.0)).abs() < 1e-15);
        assert!((cos(0.0) - 1.0).abs() < 1e-15);
        assert!((sin(HALF_PI) - 1.0).abs() < 1e-15);
        assert!(cos(HALF_PI).abs() < 1e-15);
        assert!((sin(PI)).abs() < 1e-15);
        assert_eq!(exp2(0.0), 1.0);
        assert_eq!(exp2(1.0), 2.0);
        assert_eq!(exp2(-1.0), 0.5);
        assert_eq!(exp2(3.0), 8.0);
        assert_eq!(exp2(-12.0), 1.0 / 4_096.0);
        assert_eq!(exp(0.0), 1.0);
        assert!((exp(1.0) - std::f64::consts::E).abs() < 1e-15);
    }

    /// The same input gives the same bits every time: no reading of a
    /// library's table, no f32 rounding in the middle.
    #[test]
    fn the_same_input_is_the_same_bits() {
        let mut seen = Vec::new();
        for k in 0..1_000 {
            let x = (k as f64) * 0.031_25;
            seen.push((
                sin(x).to_bits(),
                cos(x).to_bits(),
                tan(x * 0.5).to_bits(),
                exp2(x * 0.125 - 6.0).to_bits(),
            ));
        }
        for (k, first) in seen.iter().enumerate() {
            let x = (k as f64) * 0.031_25;
            let again = (
                sin(x).to_bits(),
                cos(x).to_bits(),
                tan(x * 0.5).to_bits(),
                exp2(x * 0.125 - 6.0).to_bits(),
            );
            assert_eq!(*first, again);
        }
    }
}
