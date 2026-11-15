use lumen_core::time::Ticks;
use lumen_core::warp::{self, Marker};

use super::warp::{FRAME_SLACK, Refused, anchor, file_seconds};
use crate::model::{Clip, Project};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkerEdit {
    Add(Ticks),
    AddOn(f64),
    Move {
        index: usize,
        to: Ticks,
    },
    Slide {
        index: usize,
        source: f64,
    },
    Remove(usize),
    Clear,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edited {
    pub clip: Clip,
    pub index: Option<usize>,
    pub held: bool,
}

pub fn edit_marker(project: &Project, clip: &Clip, edit: MarkerEdit) -> Result<Edited, Refused> {
    let Some(before) = &clip.warp else {
        return Err(Refused::Unchanged);
    };
    let bpm = before.original_bpm;
    let old = &before.markers;
    let (markers, index, held) = match edit {
        MarkerEdit::Add(at) => {
            let (markers, index) =
                warp::with_marker(old, warp::marker_at(old, bpm, at)).ok_or(Refused::Unchanged)?;
            (markers, Some(index), false)
        }
        MarkerEdit::AddOn(source) => {
            let (markers, index) = warp::with_marker(old, warp::marker_on(old, bpm, source))
                .ok_or(Refused::Unchanged)?;
            (markers, Some(index), false)
        }
        MarkerEdit::Move { index, to } => {
            let marker = *old.get(index).ok_or(Refused::Setting)?;
            let (low, high) = warp::tick_limits(old, bpm, index);
            let at = to.clamp(low, high);
            let mut markers = old.clone();
            markers[index] = Marker::new(marker.source, at);
            (markers, Some(index), at != to)
        }
        MarkerEdit::Slide { index, source } => {
            let marker = *old.get(index).ok_or(Refused::Setting)?;
            let (low, high) = warp::source_limits(old, bpm, index);
            let on = source.clamp(low, high);
            let mut markers = old.clone();
            markers[index] = Marker::new(on, marker.at);
            (markers, Some(index), on != source)
        }
        MarkerEdit::Remove(index) => {
            let markers = warp::without_marker(old, index).ok_or(Refused::Setting)?;
            (markers, None, false)
        }
        MarkerEdit::Clear => {
            let markers = vec![warp::marker_at(old, bpm, Ticks::ZERO)];
            (markers, None, false)
        }
    };
    let mut changed = before.clone();
    changed.markers = markers;
    changed.check().map_err(|_| Refused::Setting)?;
    if changed == *before {
        return Err(Refused::Unchanged);
    }
    if let Some(total) = file_seconds(project, clip)
        && !(0.0..=total).contains(&changed.source_at(Ticks::ZERO))
    {
        return Err(Refused::Setting);
    }
    let mut out = clip.clone();
    out.warp = Some(changed);
    anchor(project, &mut out);
    Ok(Edited {
        clip: out,
        index,
        held,
    })
}

pub fn held_at(clip: &Clip, index: usize, to: Ticks) -> bool {
    clip.warp.as_ref().is_some_and(|w| {
        let (low, high) = warp::tick_limits(&w.markers, w.original_bpm, index);
        to < low || to > high
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct FixedHit {
    pub markers: Vec<Marker>,
    pub hit: Option<usize>,
}

fn standing_at(markers: &[Marker], bpm: f64, source: f64) -> Option<usize> {
    let at = warp::tick_at_source(markers, bpm, source);
    markers
        .iter()
        .position(|m| m.at == at || (m.source - source).abs() < FRAME_SLACK)
}

pub fn fix_hit(
    clip: &Clip,
    hit: f64,
    (before, after): (Option<f64>, Option<f64>),
) -> Result<FixedHit, Refused> {
    let Some(map) = &clip.warp else {
        return Err(Refused::Unchanged);
    };
    let bpm = map.original_bpm;
    let mut wanted = Vec::new();
    if standing_at(&map.markers, bpm, hit).is_none() {
        wanted.push(hit);
    }
    for source in [before, after].into_iter().flatten() {
        if standing_at(&map.markers, bpm, source).is_some() {
            continue;
        }
        let (low, high) = if source < hit {
            (source, hit)
        } else {
            (hit, source)
        };
        let between = map
            .markers
            .iter()
            .any(|m| m.source > low && m.source < high);
        if !between {
            wanted.push(source);
        }
    }
    if wanted.is_empty() {
        return Err(Refused::Unchanged);
    }
    wanted.sort_by(f64::total_cmp);
    let mut markers = map.markers.clone();
    for source in wanted {
        let Some((next, _)) =
            warp::with_marker(&markers, warp::marker_on(&map.markers, bpm, source))
        else {
            continue;
        };
        markers = next;
    }
    if markers == map.markers {
        return Err(Refused::Unchanged);
    }
    let hit = standing_at(&markers, bpm, hit);
    Ok(FixedHit { markers, hit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{q, song};
    use crate::edit::warp::enable;
    use crate::model::ClipContent;
    use lumen_core::time::PPQ;

    fn warped() -> (Project, Clip) {
        let project = song();
        let clip = (*project.tracks[0].clips[0]).clone();
        let clip = enable(&project, &clip, 120.0).unwrap();
        (project, clip)
    }

    fn markers(clip: &Clip) -> &[Marker] {
        &clip.warp.as_ref().unwrap().markers
    }

    fn heard(clip: &Clip, at: Ticks) -> f64 {
        clip.warp.as_ref().unwrap().source_at(at)
    }

    #[test]
    fn adding_a_marker_changes_nothing_heard_and_says_where_it_is() {
        let (project, clip) = warped();
        let added = edit_marker(&project, &clip, MarkerEdit::Add(q(2))).unwrap();
        assert_eq!(added.index, Some(1));
        assert_eq!(markers(&added.clip).len(), 2);
        for t in [0, PPQ, 2 * PPQ, 5 * PPQ + 3] {
            assert!((heard(&added.clip, Ticks(t)) - heard(&clip, Ticks(t))).abs() < 1e-9);
        }
        assert_eq!(added.clip.content, clip.content, "the offset stays");
        assert_eq!(
            edit_marker(&project, &added.clip, MarkerEdit::Add(q(2))),
            Err(Refused::Unchanged)
        );
        let second = heard(&clip, q(3));
        let on = edit_marker(&project, &clip, MarkerEdit::AddOn(second)).unwrap();
        assert_eq!(markers(&on.clip)[1], Marker::new(second, q(3)));
    }

    #[test]
    fn moving_a_marker_stretches_between_its_neighbours_only() {
        let (project, clip) = warped();
        let mut pinned = clip.clone();
        for at in [q(2), q(4)] {
            pinned = edit_marker(&project, &pinned, MarkerEdit::Add(at))
                .unwrap()
                .clip;
        }
        let late = q(2) + Ticks(PPQ / 4);
        let moved =
            edit_marker(&project, &pinned, MarkerEdit::Move { index: 1, to: late }).unwrap();
        assert!(!moved.held);
        assert_eq!(moved.index, Some(1));
        assert!((heard(&moved.clip, late) - heard(&clip, q(2))).abs() < 1e-9);
        for t in [q(0), q(4), q(6)] {
            assert!(
                (heard(&moved.clip, t) - heard(&clip, t)).abs() < 1e-9,
                "{t:?}"
            );
        }
        assert!(
            heard(&moved.clip, q(1)) < heard(&clip, q(1)),
            "slowed before"
        );
        assert!(
            heard(&moved.clip, q(3)) < heard(&clip, q(3)),
            "sped up after"
        );
    }

    #[test]
    fn past_the_last_marker_the_audio_moves_along() {
        let (project, clip) = warped();
        let one = edit_marker(&project, &clip, MarkerEdit::Add(q(2)))
            .unwrap()
            .clip;
        let moved = edit_marker(&project, &one, MarkerEdit::Move { index: 1, to: q(3) })
            .unwrap()
            .clip;
        assert!((heard(&moved, q(5)) - heard(&clip, q(4))).abs() < 1e-9);
    }

    #[test]
    fn a_move_stops_at_a_quarter_and_at_four_times_the_speed() {
        let (project, clip) = warped();
        let mut pinned = clip.clone();
        for at in [q(2), q(4)] {
            pinned = edit_marker(&project, &pinned, MarkerEdit::Add(at))
                .unwrap()
                .clip;
        }
        let far = edit_marker(&project, &pinned, MarkerEdit::Move { index: 1, to: q(4) }).unwrap();
        assert!(far.held);
        assert_eq!(markers(&far.clip)[1].at, q(4) - Ticks(PPQ / 2));
        assert!(held_at(&pinned, 1, q(4)));
        assert!(!held_at(&pinned, 1, q(3)));
        assert_eq!(
            edit_marker(&project, &far.clip, MarkerEdit::Move { index: 1, to: q(5) }),
            Err(Refused::Unchanged)
        );
        assert!(held_at(&far.clip, 1, q(5)));
    }

    #[test]
    fn sliding_moves_the_audio_and_leaves_the_marker() {
        let (project, clip) = warped();
        let mut pinned = clip.clone();
        for at in [q(2), q(4)] {
            pinned = edit_marker(&project, &pinned, MarkerEdit::Add(at))
                .unwrap()
                .clip;
        }
        let source = heard(&clip, q(2)) + 0.1;
        let slid = edit_marker(&project, &pinned, MarkerEdit::Slide { index: 1, source }).unwrap();
        assert!(!slid.held);
        assert_eq!(markers(&slid.clip)[1], Marker::new(source, q(2)));
        assert!((heard(&slid.clip, q(4)) - heard(&clip, q(4))).abs() < 1e-9);
        let held = edit_marker(
            &project,
            &pinned,
            MarkerEdit::Slide {
                index: 1,
                source: 9.0,
            },
        )
        .unwrap();
        assert!(held.held);
        assert!(markers(&held.clip)[1].source < heard(&clip, q(4)));
    }

    #[test]
    fn removing_and_clearing_keep_one_marker_and_the_clips_start() {
        let (project, clip) = warped();
        assert_eq!(
            edit_marker(&project, &clip, MarkerEdit::Remove(0)),
            Err(Refused::Setting),
            "the only marker stays"
        );
        let mut pinned = clip.clone();
        for at in [q(2), q(4)] {
            pinned = edit_marker(&project, &pinned, MarkerEdit::Add(at))
                .unwrap()
                .clip;
        }
        let moved = edit_marker(&project, &pinned, MarkerEdit::Move { index: 1, to: q(3) })
            .unwrap()
            .clip;
        let removed = edit_marker(&project, &moved, MarkerEdit::Remove(1)).unwrap();
        assert_eq!(removed.index, None);
        assert_eq!(removed.clip, {
            let mut plain = pinned.clone();
            plain.warp.as_mut().unwrap().markers.remove(1);
            plain
        });
        let cleared = edit_marker(&project, &moved, MarkerEdit::Clear)
            .unwrap()
            .clip;
        assert_eq!(markers(&cleared).len(), 1);
        for t in [q(0), q(3), q(7)] {
            assert!((heard(&cleared, t) - heard(&clip, t)).abs() < 1e-9, "{t:?}");
        }
        assert_eq!(
            edit_marker(&project, &clip, MarkerEdit::Clear),
            Err(Refused::Unchanged),
            "one marker at the start already"
        );
    }

    #[test]
    fn the_clips_start_stays_in_the_file_and_its_offset_follows_the_map() {
        let (project, clip) = warped();
        assert_eq!(
            edit_marker(&project, &clip, MarkerEdit::Move { index: 0, to: q(1) }),
            Err(Refused::Setting)
        );
        let earlier = edit_marker(
            &project,
            &clip,
            MarkerEdit::Move {
                index: 0,
                to: q(-2),
            },
        )
        .unwrap()
        .clip;
        assert!(matches!(
            earlier.content,
            ClipContent::Audio { offset: 49_000, .. }
        ));
        let plain = (*project.tracks[0].clips[0]).clone();
        assert_eq!(
            edit_marker(&project, &plain, MarkerEdit::Add(q(1))),
            Err(Refused::Unchanged)
        );
    }

    fn mapped(markers: &[(f64, i64)]) -> Clip {
        let (_, clip) = warped();
        let mut clip = clip;
        clip.warp.as_mut().unwrap().markers = markers
            .iter()
            .map(|(source, at)| Marker::new(*source, q(*at)))
            .collect();
        clip
    }

    fn map(clip: &Clip) -> &[Marker] {
        &clip.warp.as_ref().unwrap().markers
    }

    fn beats_in(markers: &[Marker], bpm: f64, sources: &[f64]) -> Vec<Ticks> {
        sources
            .iter()
            .map(|s| warp::tick_at_source(markers, bpm, *s))
            .collect()
    }

    #[test]
    fn fixing_a_late_hit_pins_the_hit_and_the_hits_either_side_without_moving_them() {
        let clip = mapped(&[(0.0, 0), (2.0, 4)]);
        let hit = 0.7;
        let sides = (Some(0.4), Some(1.2));
        let bpm = clip.warp.as_ref().unwrap().original_bpm;
        let before = beats_in(map(&clip), bpm, &[0.4, hit, 1.2]);
        let fixed = fix_hit(&clip, hit, sides).expect("three markers");
        assert_eq!(fixed.markers.len(), 5);
        assert_eq!(beats_in(&fixed.markers, bpm, &[0.4, hit, 1.2]), before);
        for (source, at) in [(0.4, before[0]), (hit, before[1]), (1.2, before[2])] {
            assert!(
                (warp::source_at(&fixed.markers, bpm, at) - source).abs() < 1e-9,
                "{source} s on {at:?}"
            );
        }
        assert_eq!(fixed.hit, Some(2), "the hit's own marker");
        assert_eq!(
            fixed.markers[2],
            Marker::new(hit, before[1]),
            "on the hit, where it plays"
        );
        assert_eq!(fixed.markers[1], Marker::new(0.4, before[0]));
        assert_eq!(fixed.markers[3], Marker::new(1.2, before[2]));
    }

    #[test]
    fn a_marker_between_the_hit_and_a_neighbour_holds_that_side() {
        let clip = mapped(&[(0.0, 0), (0.85, 1), (1.0, 2)]);
        let fixed = fix_hit(&clip, 0.7, (Some(0.4), Some(1.2))).expect("some markers");
        let sources: Vec<f64> = fixed.markers.iter().map(|m| m.source).collect();
        assert!(sources.contains(&0.4), "the side before is pinned");
        assert!(!sources.contains(&1.2), "the side after was already held");
        let clip = mapped(&[(0.0, 0), (0.55, 1), (1.3, 3)]);
        let fixed = fix_hit(&clip, 0.7, (Some(0.4), Some(1.2))).expect("some markers");
        let sources: Vec<f64> = fixed.markers.iter().map(|m| m.source).collect();
        assert!(!sources.contains(&0.4), "the side before was already held");
        assert!(sources.contains(&1.2), "the side after is pinned");
    }

    #[test]
    fn a_marker_on_the_hit_or_its_neighbour_adds_no_second_one() {
        let clip = mapped(&[(0.0, 0), (0.5, 1), (1.0, 2)]);
        let fixed = fix_hit(&clip, 0.5, (Some(0.25), Some(0.75))).expect("the sides");
        let sources: Vec<f64> = fixed.markers.iter().map(|m| m.source).collect();
        assert_eq!(
            sources.iter().filter(|s| (**s - 0.5).abs() < 1e-9).count(),
            1
        );
        let index = fixed.hit.expect("the marker already on it");
        assert_eq!(fixed.markers[index].source, 0.5);
        assert_eq!(fix_hit(&clip, 0.5, (None, None)), Err(Refused::Unchanged));
        let mut plain = clip;
        plain.warp = None;
        assert_eq!(
            fix_hit(&plain, 0.5, (Some(0.25), None)),
            Err(Refused::Unchanged)
        );
    }

    #[test]
    fn the_first_and_last_hits_have_only_one_side() {
        let clip = mapped(&[(0.0, 0), (1.0, 2)]);
        let first = fix_hit(&clip, 0.1, (None, Some(0.9))).expect("the hit and after it");
        assert_eq!(first.markers.len(), 4);
        let last = fix_hit(&clip, 3.0, (Some(2.5), None)).expect("the hit and before it");
        assert_eq!(last.markers.len(), 4);
        assert_eq!(
            fix_hit(&clip, 3.0, (None, None)).map(|f| f.markers.len()),
            Ok(3),
            "the hit alone"
        );
    }
}
