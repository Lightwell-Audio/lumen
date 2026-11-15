use lumen_core::pan::PanLaw;

use crate::mixer::{AutoLane, CHUNK, LaneKind};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipEnvelope {
    pub gain: Option<AutoLane>,
    pub pan: Option<(AutoLane, PanLaw)>,
}

impl ClipEnvelope {
    pub fn is_empty(&self) -> bool {
        self.gain.is_none() && self.pan.is_none()
    }

    pub(crate) fn meets(&self, other: &Self, left_frames: i64) -> bool {
        fn translated(left: &AutoLane, right: &AutoLane, by: i64) -> bool {
            left.points().len() == right.points().len()
                && left.points().iter().zip(right.points()).all(|(a, b)| {
                    a.at.checked_sub(by) == Some(b.at) && a.value == b.value && a.shape == b.shape
                })
        }
        let gain = match (&self.gain, &other.gain) {
            (None, None) => true,
            (Some(a), Some(b)) => translated(a, b, left_frames),
            _ => false,
        };
        let pan = match (&self.pan, &other.pan) {
            (None, None) => true,
            (Some((a, law_a)), Some((b, law_b))) if law_a == law_b => translated(a, b, left_frames),
            _ => false,
        };
        gain && pan
    }
}

pub(crate) struct EnvelopeGains {
    sides: Box<[f32]>,
    lane: Box<[f32]>,
}

impl EnvelopeGains {
    pub fn new() -> Self {
        Self {
            sides: vec![1.0; 2 * CHUNK].into(),
            lane: vec![0.0; 2 * CHUNK].into(),
        }
    }

    pub fn side(&self, side: usize) -> &[f32] {
        &self.sides[side * CHUNK..(side + 1) * CHUNK]
    }

    pub fn fill(
        &mut self,
        env: &ClipEnvelope,
        (start, period): (i64, Option<i64>),
        (from, a, b): (i64, i64, i64),
        rate: u32,
    ) {
        let (left, right) = self.sides.split_at_mut(CHUNK);
        let (first, second) = self.lane.split_at_mut(CHUNK);
        let range = (a - from) as usize..(b - from) as usize;
        left[range.clone()].fill(1.0);
        right[range].fill(1.0);
        let mut t = a;
        while t < b {
            let pos = t - start;
            let (inner, run) = match period {
                Some(p) if p > 0 => {
                    let inner = pos.rem_euclid(p);
                    (inner, (p - inner).min(b - t))
                }
                _ => (pos, b - t),
            };
            let (at, n) = ((t - from) as usize, run as usize);
            if let Some(gain) = &env.gain {
                gain.fill(
                    inner,
                    n,
                    rate,
                    LaneKind::Db,
                    [&mut first[..n], &mut second[..n]],
                );
                for k in 0..n {
                    left[at + k] *= first[k];
                    right[at + k] *= first[k];
                }
            }
            if let Some((pan, law)) = &env.pan {
                pan.fill(
                    inner,
                    n,
                    rate,
                    LaneKind::Pan(*law),
                    [&mut first[..n], &mut second[..n]],
                );
                for k in 0..n {
                    left[at + k] *= first[k];
                    right[at + k] *= second[k];
                }
            }
            t += run;
        }
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::automation::Shape;

    use super::*;
    use crate::mixer::AutoPoint;

    fn lane(points: &[(i64, f32)]) -> AutoLane {
        AutoLane::new(
            points
                .iter()
                .map(|&(at, value)| AutoPoint {
                    at,
                    value,
                    shape: Shape::Linear,
                })
                .collect(),
        )
        .unwrap()
    }

    fn gains(env: &ClipEnvelope, start: i64, period: Option<i64>, a: i64, b: i64) -> Vec<f32> {
        let mut g = EnvelopeGains::new();
        g.fill(env, (start, period), (a, a, b), 48_000);
        g.side(0)[..(b - a) as usize].to_vec()
    }

    #[test]
    fn a_gain_envelope_is_a_straight_line_in_db_from_the_clips_start() {
        let env = ClipEnvelope {
            gain: Some(lane(&[(0, 0.0), (64, -20.0)])),
            pan: None,
        };
        let g = gains(&env, 1_000, None, 1_000, 1_064);
        assert!((g[0] - 1.0).abs() < 1e-6);
        assert!((g[32] - 10f32.powf(-10.0 / 20.0)).abs() < 1e-3);
        assert!((g[63] - 10f32.powf(-20.0 * 63.0 / 64.0 / 20.0)).abs() < 5e-3);
    }

    #[test]
    fn a_looping_clip_reads_the_envelope_again_at_every_repeat() {
        let env = ClipEnvelope {
            gain: Some(lane(&[(0, 0.0), (32, -60.0)])),
            pan: None,
        };
        let g = gains(&env, 0, Some(64), 0, 192);
        assert_eq!(g[0], g[64]);
        assert_eq!(g[0], g[128]);
        assert_eq!(g[40], g[40 + 64]);
        assert!(g[31] < g[0]);
    }

    #[test]
    fn the_same_frames_have_the_same_gains_at_any_block_split() {
        let env = ClipEnvelope {
            gain: Some(lane(&[(0, 0.0), (300, -30.0), (500, -6.0)])),
            pan: Some((lane(&[(0, -1.0), (400, 1.0)]), PanLaw::Balance)),
        };
        let whole = {
            let mut g = EnvelopeGains::new();
            g.fill(&env, (0, Some(500)), (0, 0, 250), 48_000);
            let a: Vec<f32> = g.side(0)[..250].to_vec();
            let b: Vec<f32> = g.side(1)[..250].to_vec();
            (a, b)
        };
        let mut parts = (Vec::new(), Vec::new());
        for (a, b) in [(0, 100), (100, 101), (101, 250)] {
            let mut g = EnvelopeGains::new();
            g.fill(&env, (0, Some(500)), (a, a, b), 48_000);
            parts.0.extend_from_slice(&g.side(0)[..(b - a) as usize]);
            parts.1.extend_from_slice(&g.side(1)[..(b - a) as usize]);
        }
        assert_eq!(whole, parts);
    }

    #[test]
    fn pan_turns_one_side_down_and_leaves_the_other() {
        let env = ClipEnvelope {
            gain: None,
            pan: Some((lane(&[(0, 1.0)]), PanLaw::Balance)),
        };
        let mut g = EnvelopeGains::new();
        g.fill(&env, (0, None), (0, 0, 8), 48_000);
        assert!(g.side(0)[..8].iter().all(|&x| x == 0.0));
        assert!(g.side(1)[..8].iter().all(|&x| x == 1.0));
    }
}
