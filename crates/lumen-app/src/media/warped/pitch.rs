use std::hash::{Hash, Hasher};

use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_project::envelope;
use lumen_project::model::{Id, Warp, WarpMode};

pub const MOST_CENTS: f64 = lumen_project::model::MAX_TRANSPOSE_CENTS as f64;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pitch {
    pub cents: i32,
    pub lane: Option<Lane>,
}

impl Pitch {
    pub fn of(warp: &Warp, lane: Option<&Lane>) -> Self {
        if !warp.mode.transposes() {
            return Self::default();
        }
        Self {
            cents: warp.transpose_cents,
            lane: lane.cloned(),
        }
    }

    pub fn is_set(&self) -> bool {
        self.cents != 0 || self.lane.is_some()
    }

    pub fn cents_at(&self, at: f64) -> f64 {
        let envelope = self.lane.as_ref().map_or(0.0, |lane| {
            let ticks = Ticks(at.round().clamp(-9.0e18, 9.0e18) as i64);
            f64::from(lane.value_at(ticks).unwrap_or(0.0)) * 100.0
        });
        (f64::from(self.cents) + envelope).clamp(-MOST_CENTS, MOST_CENTS)
    }

    pub fn hash_into(&self, hasher: &mut impl Hasher) {
        self.cents.hash(hasher);
        let Some(lane) = &self.lane else {
            return;
        };
        for point in lane.points() {
            point.at.0.hash(hasher);
            point.value.to_bits().hash(hasher);
            match point.shape {
                Shape::Step => 0_u8.hash(hasher),
                Shape::Linear => 1_u8.hash(hasher),
                Shape::Curve(curve) => {
                    2_u8.hash(hasher);
                    for v in [curve.p1[0], curve.p1[1], curve.p2[0], curve.p2[1]] {
                        v.to_bits().hash(hasher);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Identity(u64);

impl Identity {
    pub fn of(warp: &Warp, media: Id, origin: Ticks) -> Self {
        let first = warp.markers.first().map_or(Ticks::ZERO, |m| m.at);
        let mut hasher = std::hash::DefaultHasher::new();
        media.0.hash(&mut hasher);
        (origin + first).0.hash(&mut hasher);
        warp.original_bpm.to_bits().hash(&mut hasher);
        for marker in &warp.markers {
            marker.source.to_bits().hash(&mut hasher);
            (marker.at - first).0.hash(&mut hasher);
        }
        warp.mode.hash(&mut hasher);
        warp.transpose_cents.hash(&mut hasher);
        Self(hasher.finish())
    }
}

impl Identity {
    pub fn alone(self, clip: Id) -> Self {
        let mut hasher = std::hash::DefaultHasher::new();
        self.0.hash(&mut hasher);
        clip.0.hash(&mut hasher);
        Self(hasher.finish())
    }
}

pub struct Part<'a> {
    pub start: Ticks,
    pub length: Ticks,
    pub lane: Option<&'a Lane>,
    pub window: Ticks,
}

impl<'a> Part<'a> {
    pub fn of(clip: &'a lumen_project::model::Clip) -> Self {
        Self {
            start: clip.start,
            length: clip.length,
            lane: clip.envelopes.pitch.as_ref(),
            window: envelope::window(clip),
        }
    }
}

pub fn merge(parts: &[Part<'_>]) -> Option<Lane> {
    if parts.iter().all(|p| p.lane.is_none()) {
        return None;
    }
    let mut order: Vec<&Part<'_>> = parts.iter().filter(|p| p.length > Ticks::ZERO).collect();
    order.sort_by_key(|p| p.start);
    let mut points: std::collections::BTreeMap<i64, Point> = std::collections::BTreeMap::new();
    let mut put = |at: Ticks, value: f32, shape: Shape| {
        points.insert(at.0, Point { at, value, shape });
    };
    for part in order {
        let end = part.start + part.length;
        let Some(lane) = part.lane else {
            put(part.start, 0.0, Shape::Step);
            put(end - Ticks(1), 0.0, Shape::Step);
            continue;
        };
        let Some(played) = envelope::windowed(lane, part.window.min(part.length)) else {
            continue;
        };
        let first = played.points()[0];
        if first.at > Ticks::ZERO {
            put(part.start, first.value, Shape::Step);
        }
        for point in played.points() {
            put(point.at + part.start, point.value, point.shape);
        }
        let last = played.points()[played.len() - 1];
        if last.at + part.start < end - Ticks(1) {
            put(end - Ticks(1), last.value, Shape::Step);
        }
    }
    Lane::new(points.into_values().collect()).ok()
}

pub fn needs_full_preset(mode: WarpMode, pitch: &Pitch) -> bool {
    mode == WarpMode::Melodic && pitch.is_set()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn lane(points: &[(i64, f32)]) -> Lane {
        Lane::new(points.iter().map(|&(t, v)| Point::new(q(t), v)).collect()).unwrap()
    }

    fn part(start: i64, length: i64, lane: Option<&Lane>) -> Part<'_> {
        Part {
            start: q(start),
            length: q(length),
            lane,
            window: q(length),
        }
    }

    fn at(lane: &Lane, ticks: i64) -> f32 {
        lane.value_at(Ticks(ticks)).unwrap()
    }

    #[test]
    fn a_clip_without_an_envelope_adds_none() {
        assert!(merge(&[part(0, 4, None), part(4, 4, None)]).is_none());
    }

    #[test]
    fn one_clips_envelope_is_moved_to_the_song_and_held_either_side() {
        let l = lane(&[(1, 2.0), (3, 6.0)]);
        let merged = merge(&[part(8, 4, Some(&l))]).unwrap();
        assert_eq!(at(&merged, q(8).0), 2.0);
        assert_eq!(at(&merged, q(9).0), 2.0);
        assert_eq!(at(&merged, q(10).0), 4.0);
        assert_eq!(at(&merged, q(11).0), 6.0);
        assert_eq!(at(&merged, q(11).0 + 100), 6.0);
    }

    #[test]
    fn the_pieces_of_a_split_make_the_curve_of_the_clip_they_came_from() {
        let whole = lane(&[(0, 0.0), (8, 8.0)]);
        let one = merge(&[part(0, 8, Some(&whole))]).unwrap();
        let first = lane(&[(0, 0.0), (3, 3.0)]);
        let second = lane(&[(0, 3.0), (5, 8.0)]);
        let split = merge(&[part(0, 3, Some(&first)), part(3, 5, Some(&second))]).unwrap();
        for tick in (0..q(8).0).step_by(37) {
            let (a, b) = (at(&one, tick), at(&split, tick));
            assert!((a - b).abs() < 0.01, "tick {tick}: {a} against {b}");
        }
    }

    #[test]
    fn a_clip_with_no_envelope_between_two_with_one_holds_nothing() {
        let a = lane(&[(0, 5.0)]);
        let c = lane(&[(0, 7.0)]);
        let merged =
            merge(&[part(0, 2, Some(&a)), part(2, 2, None), part(4, 2, Some(&c))]).unwrap();
        assert_eq!(at(&merged, q(1).0), 5.0);
        assert_eq!(at(&merged, q(3).0), 0.0, "the middle clip is not shifted");
        assert_eq!(at(&merged, q(5).0), 7.0);
    }

    #[test]
    fn the_shift_adds_the_warps_transpose_to_the_envelope_in_cents() {
        let l = lane(&[(0, 2.0)]);
        let pitch = Pitch {
            cents: 150,
            lane: Some(l),
        };
        assert_eq!(pitch.cents_at(0.0), 350.0);
        assert_eq!(Pitch::default().cents_at(5.0), 0.0);
        assert!(!Pitch::default().is_set());
        let far = Pitch {
            cents: 4_800,
            lane: Some(lane(&[(0, 40.0)])),
        };
        assert_eq!(far.cents_at(0.0), 4_800.0, "never past four octaves");
    }

    #[test]
    fn a_mode_that_does_not_transpose_has_no_pitch() {
        let mut warp = Warp::new(120.0, 0.0);
        warp.transpose_cents = 100;
        let l = lane(&[(0, 2.0)]);
        warp.mode = WarpMode::Repitch;
        assert!(!Pitch::of(&warp, Some(&l)).is_set());
        warp.mode = WarpMode::Rhythmic;
        assert!(Pitch::of(&warp, Some(&l)).is_set());
    }

    #[test]
    fn clips_of_one_line_share_an_identity_and_others_do_not() {
        let warp = Warp::new(120.0, 0.0);
        let a = Identity::of(&warp, Id(1), q(4));
        assert_eq!(a, Identity::of(&warp, Id(1), q(4)));
        assert_ne!(a, Identity::of(&warp, Id(2), q(4)), "another file");
        assert_ne!(a, Identity::of(&warp, Id(1), q(5)), "another place");
        let moved = warp.shifted(q(2));
        assert_eq!(a, Identity::of(&moved, Id(1), q(2)));
    }
}
