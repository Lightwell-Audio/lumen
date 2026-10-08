//! Curves from (0, 0) to (1, 1) (M3 slice 6d, the owner's choice C3): the
//! shape of a fade now, and of automation segments later (M7), so every
//! envelope in Lumen bends the same way.
//!
//! A curve is a cubic Bézier with two inner control points, as CSS easing
//! curves are. With every control point inside the unit square and the
//! first left of the second, the level only ever rises with x, so a curve
//! is a function of x. The four preset shapes are exact cases of it: a
//! straight line, x², 1 − (1 − x)² and smoothstep.
//!
//! It is edited through one free handle (C3): the curve passes through the
//! handle's dot, which can go anywhere in the curve's middle three
//! quarters, and holding Alt adds S-shape (flat at both ends) while the
//! curve still passes through the dot.

use serde::{Deserialize, Serialize};

/// A cubic Bézier from (0, 0) to (1, 1) through control points `p1` and
/// `p2`, each `[x, y]` in the unit square. Saved as `[x1, y1, x2, y2]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f32; 4]", into = "[f32; 4]")]
pub struct Curve {
    pub p1: [f32; 2],
    pub p2: [f32; 2],
}

impl From<[f32; 4]> for Curve {
    fn from([x1, y1, x2, y2]: [f32; 4]) -> Self {
        Self {
            p1: [x1, y1],
            p2: [x2, y2],
        }
    }
}

impl From<Curve> for [f32; 4] {
    fn from(c: Curve) -> Self {
        [c.p1[0], c.p1[1], c.p2[0], c.p2[1]]
    }
}

impl Default for Curve {
    fn default() -> Self {
        Self::LINEAR
    }
}

/// The handle a curve is edited by (C3): the curve's middle point, and how
/// S-shaped it is, 0 (none) to 1 (flat at both ends).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handle {
    pub dot: [f32; 2],
    pub s: f32,
}

/// How far the handle's dot can go: the curve's middle three quarters.
pub const DOT_MIN: f32 = 1.0 / 8.0;
pub const DOT_MAX: f32 = 7.0 / 8.0;

/// Control points sit a sixth either side of the curve's centre, as in all
/// four presets.
const SPREAD: f32 = 1.0 / 6.0;

/// Within this, two curves count as the same (for presets and saving).
const SAME: f32 = 1e-5;

impl Curve {
    /// Straight.
    pub const LINEAR: Self = Self {
        p1: [1.0 / 3.0, 1.0 / 3.0],
        p2: [2.0 / 3.0, 2.0 / 3.0],
    };
    /// x²: slow at first, then quick.
    pub const SLOW: Self = Self {
        p1: [1.0 / 3.0, 0.0],
        p2: [2.0 / 3.0, 1.0 / 3.0],
    };
    /// 1 − (1 − x)²: quick at first, then slow.
    pub const FAST: Self = Self {
        p1: [1.0 / 3.0, 2.0 / 3.0],
        p2: [2.0 / 3.0, 1.0],
    };
    /// Smoothstep, 3x² − 2x³: slow at both ends.
    pub const S_CURVE: Self = Self {
        p1: [1.0 / 3.0, 0.0],
        p2: [2.0 / 3.0, 1.0],
    };

    /// Every control point finite, inside the unit square, the first left
    /// of the second: a curve that rises with x and stays in 0 to 1.
    pub fn is_valid(&self) -> bool {
        let inside = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        [self.p1[0], self.p1[1], self.p2[0], self.p2[1]]
            .into_iter()
            .all(inside)
            && self.p1[0] <= self.p2[0]
            && self.p1[1] <= self.p2[1]
    }

    /// Whether `other` is the same curve, to within rounding.
    pub fn same(&self, other: &Curve) -> bool {
        let a: [f32; 4] = (*self).into();
        let b: [f32; 4] = (*other).into();
        a.iter().zip(b).all(|(a, b)| (a - b).abs() <= SAME)
    }

    pub fn is_linear(&self) -> bool {
        self.same(&Self::LINEAR)
    }

    /// The point at parameter `t` (0 to 1) along the curve.
    pub fn point(&self, t: f32) -> [f32; 2] {
        let u = 1.0 - t;
        let (a, b, c) = (3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        [
            a * self.p1[0] + b * self.p2[0] + c,
            a * self.p1[1] + b * self.p2[1] + c,
        ]
    }

    /// The curve's level at `x` (0 to 1): the y where it crosses x.
    pub fn at(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        // x rises with t, so halve the interval: 24 steps is f32 precision.
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if self.point(mid)[0] < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        self.point(0.5 * (lo + hi))[1].clamp(0.0, 1.0)
    }

    /// The curve's level at `n + 1` evenly spaced x from 0 to 1, for the
    /// audio thread to read from (it does no solving of its own).
    pub fn table(&self, n: usize) -> Vec<f32> {
        (0..=n).map(|i| self.at(i as f32 / n as f32)).collect()
    }

    /// The curve edited by `handle` (C3): through the handle's dot, with
    /// its S-shape.
    pub fn from_handle(handle: Handle) -> Self {
        let dot = handle.dot.map(|v| v.clamp(DOT_MIN, DOT_MAX));
        // The middle of a cubic Bézier is (P0 + 3·P1 + 3·P2 + P3) / 8; with
        // P1 and P2 either side of a centre, that centre is (8·dot − 1) / 6.
        let [cx, cy] = dot.map(|v| ((8.0 * v - 1.0) / 6.0).clamp(0.0, 1.0));
        let h = SPREAD.min(cx).min(1.0 - cx);
        let room = cy.min(1.0 - cy);
        let rest = SPREAD.min(room);
        let a = rest + handle.s.clamp(0.0, 1.0) * (room - rest);
        Self {
            p1: [cx - h, cy - a],
            p2: [cx + h, cy + a],
        }
    }

    /// The handle that edits this curve; [`Curve::from_handle`] of it is
    /// the same curve (for the presets and every curve the handle made).
    pub fn handle(&self) -> Handle {
        let [x, y] = self.point(0.5);
        let cy = 0.5 * (self.p1[1] + self.p2[1]);
        let room = cy.min(1.0 - cy);
        let rest = SPREAD.min(room);
        let a = 0.5 * (self.p2[1] - self.p1[1]);
        let s = if room - rest > SAME {
            ((a - rest) / (room - rest)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Handle {
            dot: [x.clamp(DOT_MIN, DOT_MAX), y.clamp(DOT_MIN, DOT_MAX)],
            s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn the_presets_are_exactly_the_shapes_they_are_named_for() {
        for k in 0..=100 {
            let x = k as f32 / 100.0;
            assert!(close(Curve::LINEAR.at(x), x), "linear at {x}");
            assert!(close(Curve::SLOW.at(x), x * x), "slow at {x}");
            assert!(
                close(Curve::FAST.at(x), 1.0 - (1.0 - x) * (1.0 - x)),
                "fast at {x}"
            );
            let smooth = x * x * (3.0 - 2.0 * x);
            assert!(close(Curve::S_CURVE.at(x), smooth), "s-curve at {x}");
        }
    }

    #[test]
    fn every_curve_the_handle_makes_rises_from_silence_to_full() {
        for dx in 0..=10 {
            for dy in 0..=10 {
                for s in [0.0, 0.5, 1.0] {
                    let dot = [dx as f32 / 10.0, dy as f32 / 10.0];
                    let c = Curve::from_handle(Handle { dot, s });
                    assert!(c.is_valid(), "{c:?} from {dot:?}, {s}");
                    assert_eq!(c.at(0.0), 0.0);
                    assert!(close(c.at(1.0), 1.0));
                    let mut last = 0.0;
                    for k in 0..=200 {
                        let level = c.at(k as f32 / 200.0);
                        assert!(level >= last - 1e-6, "{c:?} falls at {k}");
                        last = level;
                    }
                }
            }
        }
    }

    #[test]
    fn the_curve_passes_through_the_handles_dot() {
        for (dot, s) in [([0.5, 0.5], 0.0), ([0.3, 0.8], 0.0), ([0.7, 0.2], 0.6)] {
            let c = Curve::from_handle(Handle { dot, s });
            let [x, y] = c.point(0.5);
            assert!(close(x, dot[0]) && close(y, dot[1]), "{dot:?}: {x}, {y}");
        }
        // Past the middle three quarters, the dot stops at the edge.
        let edge = Curve::from_handle(Handle {
            dot: [0.0, 1.0],
            s: 0.0,
        });
        assert!(close(edge.point(0.5)[0], DOT_MIN));
        assert!(close(edge.point(0.5)[1], DOT_MAX));
    }

    #[test]
    fn the_presets_and_handle_curves_come_back_from_their_handles() {
        let made = Curve::from_handle(Handle {
            dot: [0.35, 0.7],
            s: 0.4,
        });
        for c in [
            Curve::LINEAR,
            Curve::SLOW,
            Curve::FAST,
            Curve::S_CURVE,
            made,
        ] {
            let back = Curve::from_handle(c.handle());
            assert!(back.same(&c), "{c:?} came back as {back:?}");
        }
        assert_eq!(Curve::LINEAR.handle().s, 0.0);
        assert_eq!(Curve::S_CURVE.handle().s, 1.0);
        // S-shape keeps the dot where it is.
        let flat = Curve::from_handle(Handle {
            dot: [0.5, 0.5],
            s: 0.0,
        });
        assert!(flat.same(&Curve::LINEAR), "the middle, no S: straight");
    }

    #[test]
    fn it_saves_as_four_numbers_and_reads_back_exactly() {
        let c = Curve::from_handle(Handle {
            dot: [0.31, 0.77],
            s: 0.25,
        });
        let text = serde_json::to_string(&c).unwrap();
        assert!(
            text.starts_with('[') && text.matches(',').count() == 3,
            "{text}"
        );
        assert_eq!(serde_json::from_str::<Curve>(&text).unwrap(), c);
        assert!(!Curve::from([0.5, -0.1, 0.6, 1.0]).is_valid());
        assert!(!Curve::from([0.8, 0.1, 0.2, 1.0]).is_valid(), "x must rise");
        let table = Curve::SLOW.table(4);
        assert_eq!(table.len(), 5);
        assert!(close(table[2], 0.25));
    }
}
