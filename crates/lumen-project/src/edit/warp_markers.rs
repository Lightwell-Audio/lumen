//! Warp markers edited one at a time (M9.5 slice S5b): the warp strip's
//! gestures and keys. A marker is added where the audio already plays, so
//! adding one changes nothing heard; moving it stretches the audio between it
//! and its neighbours, and the audio past the outermost marker moves along at
//! the file's own tempo (D1). Sliding moves the audio under a marker that
//! stays where it is.
//!
//! A span plays between a quarter and four times the file's speed
//! ([`lumen_core::warp::MIN_RATIO`], [`lumen_core::warp::MAX_RATIO`]): a move
//! or a slide past that stops at the limit and says so ([`Edited::held`]).
//! Markers are counted in the playing pass; a take's other passes keep theirs.

use lumen_core::time::Ticks;
use lumen_core::warp::{self, Marker};

use super::warp::{Refused, anchor, file_seconds};
use crate::model::{Clip, Project};

/// One change to a warped clip's markers. Ticks count from the clip's content
/// origin ([`super::warp::origin`]); an index is the marker's place in time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkerEdit {
    /// A marker at this tick, on the audio that plays there.
    Add(Ticks),
    /// A marker on this second of the file (a transient), where it plays.
    AddOn(f64),
    /// The marker moves to this tick, with the audio it pins.
    Move {
        index: usize,
        to: Ticks,
    },
    /// The marker stays and pins this second of the file instead.
    Slide {
        index: usize,
        source: f64,
    },
    Remove(usize),
    /// Every marker goes but one, which keeps the clip's start where it is:
    /// the clip plays at the file's tempo throughout.
    Clear,
}

/// What a marker edit made.
#[derive(Debug, Clone, PartialEq)]
pub struct Edited {
    pub clip: Clip,
    /// The marker added, moved or slid, by its place now.
    pub index: Option<usize>,
    /// The move or slide stopped at a speed limit.
    pub held: bool,
}

/// `clip` with `edit` made to its markers. Refused when the clip is not
/// warped ([`Refused::Unchanged`], also for an edit that changes nothing) or
/// when the result is not a map or would start the clip outside its file
/// ([`Refused::Setting`]).
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
        // A move held where the marker already stands is still a held move:
        // the caller shows the limit.
        return Err(Refused::Unchanged);
    }
    // The clip must still start inside its file. Its end may come to read
    // past the file's last second: that is silence, as for any clip longer
    // than what is left of its file.
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

/// Whether moving marker `index` of `clip` to `to` would stop at a speed
/// limit: what the strip shows while a drag holds there, including when the
/// marker is already at the limit and the edit changes nothing.
pub fn held_at(clip: &Clip, index: usize, to: Ticks) -> bool {
    clip.warp.as_ref().is_some_and(|w| {
        let (low, high) = warp::tick_limits(&w.markers, w.original_bpm, index);
        to < low || to > high
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{q, song};
    use crate::edit::warp::enable;
    use crate::model::ClipContent;
    use lumen_core::time::PPQ;

    /// The song's first clip (beat 4, eight beats, from frame 1,000 of a ten
    /// second file), warped as made at the song's 120 BPM.
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
        // The same tick again adds nothing.
        assert_eq!(
            edit_marker(&project, &added.clip, MarkerEdit::Add(q(2))),
            Err(Refused::Unchanged)
        );
        // On a second of the file: the tick it plays at.
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
        // The audio that was at beat 2 is at the new tick; beat 0 and beat 4
        // onwards play what they did.
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
        // A beat later, at the file's own speed.
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
        // Two beats of audio after it fit half a beat at 400%.
        assert_eq!(markers(&far.clip)[1].at, q(4) - Ticks(PPQ / 2));
        assert!(held_at(&pinned, 1, q(4)));
        assert!(!held_at(&pinned, 1, q(3)));
        // Held where it already stands: nothing changes, and it is still held.
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
        // Far past the next marker's audio: held at the limit.
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
        // The one marker dragged later: the clip's start then reads before
        // the file's first frame (it began 1,000 frames in).
        assert_eq!(
            edit_marker(&project, &clip, MarkerEdit::Move { index: 0, to: q(1) }),
            Err(Refused::Setting)
        );
        // Dragged earlier: the start plays later in the file, and the media
        // offset holds that frame.
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
        // A clip that is not warped has no markers to edit.
        let plain = (*project.tracks[0].clips[0]).clone();
        assert_eq!(
            edit_marker(&project, &plain, MarkerEdit::Add(q(1))),
            Err(Refused::Unchanged)
        );
    }
}
