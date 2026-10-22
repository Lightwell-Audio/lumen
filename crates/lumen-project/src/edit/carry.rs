//! Automation that follows clips (M7 slice A7, the owner's E3): when a clip is
//! moved, copied or deleted and the song does not lock envelopes, the points
//! of the track's lanes under the clip go with it, are copied with it, or go
//! away with it. Pure edits over the model; the app wraps each in the same
//! undo step as the clip edit.
//!
//! A clip's footprint is `[start, start + length)`. Moved points land in the
//! destination footprint, which they first clear (as a clip put down cuts away
//! what it covers). A lane the points leave empty is taken away. Across tracks
//! only the values every track has go (volume, pan, trim, mute) and the sends
//! the destination makes; a plugin's parameters go only with a clip that stays
//! on its track.

use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;

use crate::automation::{AutomationLane, MAX_LANES, Target};
use crate::model::{Id, Project, Track};

/// One clip's journey: from `span` on `from`, to the same length `by` later on
/// `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Carry {
    pub from: Id,
    pub span: (Ticks, Ticks),
    pub to: Id,
    pub by: Ticks,
}

/// Whether the lane for `target` can go from track `from` to track `to`.
fn goes(target: Target, from: &Track, to: &Track) -> bool {
    from.id == to.id || goes_between(target, to)
}

/// Whether the lane for `target` of some other track can land on `to`: the
/// values every track has, and the sends `to` makes.
pub(crate) fn goes_between(target: Target, to: &Track) -> bool {
    match target {
        Target::Volume | Target::Pan | Target::Trim | Target::Mute => true,
        Target::Send { .. } => to.has_target(target),
        Target::Device { .. } | Target::ClipGain { .. } | Target::ClipPan { .. } => false,
    }
}

/// `lane` without the points in `[a, b)`, and those points.
pub fn lift(lane: &Lane, a: Ticks, b: Ticks) -> (Lane, Vec<Point>) {
    let (inside, outside): (Vec<Point>, Vec<Point>) = lane
        .points()
        .iter()
        .copied()
        .partition(|p| p.at >= a && p.at < b);
    (Lane::new(outside).unwrap_or_default(), inside)
}

/// `lane` with the points in `[a, b)` cleared and `points` put in. `None` when
/// the result is not a lane (points out of order, or too many).
pub fn lay(lane: &Lane, (a, b): (Ticks, Ticks), points: &[Point]) -> Option<Lane> {
    let mut all: Vec<Point> = lane
        .points()
        .iter()
        .copied()
        .filter(|p| p.at < a || p.at >= b)
        .collect();
    all.extend_from_slice(points);
    all.sort_by_key(|p| p.at);
    Lane::new(all).ok()
}

fn track_mut(project: &mut Project, id: Id) -> Option<&mut Track> {
    project
        .tracks
        .iter_mut()
        .find(|t| t.id == id)
        .map(std::sync::Arc::make_mut)
}

/// What a carry takes: for each lane of `from` that can go, its points in the
/// span.
type Taken = Vec<(Target, Vec<Point>)>;

fn gather(project: &Project, c: &Carry) -> Taken {
    let (Some(from), Some(to)) = (
        project.tracks.iter().find(|t| t.id == c.from),
        project.tracks.iter().find(|t| t.id == c.to),
    ) else {
        return Vec::new();
    };
    from.automation
        .iter()
        .filter(|l| goes(l.target, from, to))
        .map(|l| {
            let (_, taken) = lift(&l.lane, c.span.0, c.span.1);
            (l.target, taken)
        })
        .filter(|(_, taken)| !taken.is_empty())
        .collect()
}

/// Moves (`keep` false) or copies (`keep` true) the points under each carry's
/// span to where the clip goes. Every carry reads the lanes as they were, so
/// moving clips past one another loses nothing.
pub fn carry(project: &mut Project, carries: &[Carry], keep: bool) {
    let taken: Vec<(Carry, Taken)> = carries.iter().map(|c| (*c, gather(project, c))).collect();
    if !keep {
        for (c, lanes) in &taken {
            let Some(track) = track_mut(project, c.from) else {
                continue;
            };
            for (target, _) in lanes {
                if let Some(entry) = track.automation.iter_mut().find(|l| l.target == *target) {
                    entry.lane = lift(&entry.lane, c.span.0, c.span.1).0;
                }
            }
            track.automation.retain(|l| !l.lane.is_empty());
        }
    }
    for (c, lanes) in taken {
        let landing = (c.span.0 + c.by, c.span.1 + c.by);
        let Some(track) = track_mut(project, c.to) else {
            continue;
        };
        for (target, points) in lanes {
            let moved: Vec<Point> = points
                .into_iter()
                .map(|p| Point {
                    at: p.at + c.by,
                    ..p
                })
                .collect();
            let room = track.automation.len() < MAX_LANES;
            match track.automation.iter_mut().find(|l| l.target == target) {
                Some(entry) => {
                    if let Some(lane) = lay(&entry.lane, landing, &moved) {
                        entry.lane = lane;
                    }
                }
                None if room => {
                    if let Ok(lane) = Lane::new(moved) {
                        track.automation.push(AutomationLane::new(target, lane));
                    }
                }
                None => {}
            }
        }
    }
}

/// Takes away the points under the spans of deleted clips.
pub fn remove(project: &mut Project, spans: &[(Id, Ticks, Ticks)]) {
    for &(id, a, b) in spans {
        let Some(track) = track_mut(project, id) else {
            continue;
        };
        for entry in &mut track.automation {
            entry.lane = lift(&entry.lane, a, b).0;
        }
        track.automation.retain(|l| !l.lane.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_core::automation::Shape;

    use super::*;
    use crate::model::{Color, TrackKind};

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn lane(points: &[(i64, f32)]) -> Lane {
        Lane::new(points.iter().map(|&(t, v)| Point::new(q(t), v)).collect()).unwrap()
    }

    fn two_tracks() -> Project {
        let mut a = Track::new(Id(1), "A", TrackKind::Audio, Color(0));
        a.automation = vec![
            AutomationLane::new(Target::Volume, lane(&[(0, -6.0), (5, -12.0), (9, 0.0)])),
            AutomationLane::new(
                Target::Device {
                    device: Id(50),
                    param: 3,
                },
                lane(&[(5, 0.25)]),
            ),
        ];
        let b = Track::new(Id(2), "B", TrackKind::Audio, Color(0));
        crate::testing::project_of("carry", vec![a, b])
    }

    fn points_of(p: &Project, track: Id, target: Target) -> Vec<(i64, f32)> {
        p.tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.lane(target))
            .map(|l| {
                l.lane
                    .points()
                    .iter()
                    .map(|x| (x.at.0 / q(1).0, x.value))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn lifting_and_laying_are_inverses_of_each_other() {
        let l = lane(&[(0, 1.0), (3, 2.0), (6, 3.0)]);
        let (rest, taken) = lift(&l, q(2), q(5));
        assert_eq!(taken.len(), 1);
        assert_eq!(rest.len(), 2);
        let back = lay(&rest, (q(2), q(5)), &taken).unwrap();
        assert_eq!(back, l);
        // Laying clears what is in the way.
        let over = lay(&l, (q(2), q(5)), &[Point::new(q(4), 9.0)]).unwrap();
        assert_eq!(over.len(), 3);
        assert_eq!(over.points()[1].value, 9.0);
    }

    #[test]
    fn a_moved_clip_takes_the_points_under_it_and_leaves_the_rest() {
        let mut p = two_tracks();
        // The clip spans quarters 4 to 8 and moves 10 later on its own track.
        carry(
            &mut p,
            &[Carry {
                from: Id(1),
                span: (q(4), q(8)),
                to: Id(1),
                by: q(10),
            }],
            false,
        );
        assert_eq!(
            points_of(&p, Id(1), Target::Volume),
            vec![(0, -6.0), (9, 0.0), (15, -12.0)]
        );
        // The plugin parameter went with it too.
        let device = Target::Device {
            device: Id(50),
            param: 3,
        };
        assert_eq!(points_of(&p, Id(1), device), vec![(15, 0.25)]);
    }

    #[test]
    fn across_tracks_only_the_values_every_track_has_go() {
        let mut p = two_tracks();
        carry(
            &mut p,
            &[Carry {
                from: Id(1),
                span: (q(4), q(8)),
                to: Id(2),
                by: q(0),
            }],
            false,
        );
        assert_eq!(points_of(&p, Id(2), Target::Volume), vec![(5, -12.0)]);
        assert_eq!(
            points_of(&p, Id(1), Target::Volume),
            vec![(0, -6.0), (9, 0.0)]
        );
        // The plugin's parameter stays where its plugin is.
        let device = Target::Device {
            device: Id(50),
            param: 3,
        };
        assert_eq!(points_of(&p, Id(1), device), vec![(5, 0.25)]);
        assert!(points_of(&p, Id(2), device).is_empty());
    }

    #[test]
    fn a_copy_leaves_the_source_and_clears_where_it_lands() {
        let mut p = two_tracks();
        carry(
            &mut p,
            &[Carry {
                from: Id(1),
                span: (q(4), q(8)),
                to: Id(1),
                by: q(4),
            }],
            true,
        );
        // The copy lands on 8 to 12 and replaces the point at 9.
        assert_eq!(
            points_of(&p, Id(1), Target::Volume),
            vec![(0, -6.0), (5, -12.0), (9, -12.0)]
        );
    }

    #[test]
    fn a_lane_the_points_leave_is_taken_away_and_deleting_does_the_same() {
        let mut p = two_tracks();
        remove(&mut p, &[(Id(1), q(4), q(8))]);
        let t = &p.tracks[0];
        assert!(t.lane(Target::Volume).is_some_and(|l| l.lane.len() == 2));
        assert!(
            t.lane(Target::Device {
                device: Id(50),
                param: 3
            })
            .is_none(),
            "no empty lane is kept"
        );
    }

    #[test]
    fn clips_moving_past_one_another_lose_nothing() {
        let mut p = two_tracks();
        Arc::make_mut(&mut p.tracks[0]).automation = vec![AutomationLane::new(
            Target::Volume,
            Lane::new(vec![
                Point::new(q(1), -1.0).with_shape(Shape::Step),
                Point::new(q(5), -5.0),
            ])
            .unwrap(),
        )];
        // Two clips, quarters 0 to 4 and 4 to 8, swap places.
        carry(
            &mut p,
            &[
                Carry {
                    from: Id(1),
                    span: (q(0), q(4)),
                    to: Id(1),
                    by: q(4),
                },
                Carry {
                    from: Id(1),
                    span: (q(4), q(8)),
                    to: Id(1),
                    by: q(-4),
                },
            ],
            false,
        );
        assert_eq!(
            points_of(&p, Id(1), Target::Volume),
            vec![(1, -5.0), (5, -1.0)]
        );
    }
}
