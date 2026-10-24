//! Crossfades between audio clips that touch (M7b E2, ADR-0094). A crossfade
//! is two overlapping stretches of sound: the outgoing clip fades out while
//! the incoming one fades in. What is drawn, what is edited and what is heard
//! all come from the same place, [`played`].
//!
//! **A crossfade made by hand** belongs to the incoming clip: `crossfade` is
//! set and `fade_in.length` is how long it lasts. The clips meet at the end
//! of the crossfade (the incoming clip *starts* there, as the owner asked),
//! and the crossfade runs over the last stretch of the outgoing clip, against
//! the audio the incoming clip keeps hidden before its start. Dragging the
//! outgoing clip's fade-out handle over the next clip makes one: the outgoing
//! clip is extended by that much and the incoming one trimmed by it
//! (`Document::crossfade_across`).
//!
//! **An automatic crossfade** (a project setting) blends 5 ms across a join of
//! unrelated audio, so it never clicks. It has no hidden head to use, so it
//! runs the other way: over the outgoing clip's hidden tail past the join.
//! It is played but not drawn.
//!
//! Joins where the two clips continue the same audio (a split) are *seamless*:
//! a crossfade there is shown, but blending identical sound would only make
//! it louder, so the sound is left alone.
//!
//! This runs off the audio thread; the engine bounds the lengths again
//! against the audio it holds, which may have been resampled.

use std::collections::HashMap;

use lumen_core::fade::Fade;
use lumen_core::time::{TempoMap, Ticks};
use lumen_project::model::{Clip, ClipContent, Id, Media, Project};

/// Where a crossfade lies against the incoming clip's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Zone {
    /// Before it, over the end of the outgoing clip (a crossfade by hand).
    Before,
    /// After it, over the outgoing clip's hidden tail (an automatic one).
    After,
}

/// A crossfade into an incoming clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Crossing {
    /// The outgoing clip.
    pub left: Id,
    /// How long it lasts, and its curve.
    pub fade: Fade,
    pub zone: Zone,
    /// The two clips continue the same audio: shown, never played.
    pub seamless: bool,
}

/// A clip as it is heard, and which of its fades are halves of a crossfade
/// (the in and the out).
#[derive(Debug, Clone)]
pub(crate) struct Played {
    pub clip: Clip,
    pub blend: [bool; 2],
}

/// The file an audio clip plays, its offset into it and its rate.
fn audio(clip: &Clip) -> Option<Id> {
    match clip.content {
        ClipContent::Audio { media, .. } => Some(media),
        ClipContent::Midi { .. } => None,
    }
}

/// The clip that ends where `clip` starts on `track`, if any.
fn before<'a>(clips: &'a [std::sync::Arc<Clip>], clip: &Clip) -> Option<&'a Clip> {
    clips
        .iter()
        .map(|c| &**c)
        .find(|c| c.id != clip.id && c.start + c.length == clip.start && c.length > Ticks::ZERO)
}

/// The crossfade into `clip`, if it makes one with the clip before it on
/// `track`.
pub(crate) fn crossing(project: &Project, track: Id, clip: &Clip) -> Option<Crossing> {
    if !clip.crossfade && !project.auto_crossfade {
        return None;
    }
    audio(clip)?;
    let track = project.tracks.iter().find(|t| t.id == track)?;
    let left = before(&track.clips, clip)?;
    let tempo = TempoMap::new(project.tempo.clone()).ok()?;
    let sources = sources(project, left, clip);
    resolve(project.auto_crossfade, &sources, &tempo, left, clip)
}

fn sources<'a>(project: &'a Project, a: &Clip, b: &Clip) -> HashMap<Id, &'a Media> {
    project
        .media
        .iter()
        .filter(|m| Some(m.id) == audio(a) || Some(m.id) == audio(b))
        .map(|m| (m.id, m))
        .collect()
}

/// The crossfade that starts at the end of `left`, as `left`'s fade-out
/// handle shows it: the incoming clip and its crossfade, when one is made by
/// hand.
pub(crate) fn outgoing(project: &Project, track: Id, left: &Clip) -> Option<(Id, Crossing)> {
    let track = project.tracks.iter().find(|t| t.id == track)?;
    let end = left.start + left.length;
    let next = track
        .clips
        .iter()
        .find(|c| c.id != left.id && c.start == end && c.crossfade)?;
    let crossing = crossing(project, track.id, next)?;
    (crossing.left == left.id && crossing.zone == Zone::Before).then_some((next.id, crossing))
}

/// The longest crossfade into `clip` its two clips have audio for: not more
/// than the outgoing clip's length nor the audio `clip` keeps before its
/// start. Zero when it has no clip to cross from.
pub(crate) fn room(project: &Project, track: Id, clip: &Clip) -> Ticks {
    let Some(track) = project.tracks.iter().find(|t| t.id == track) else {
        return Ticks::ZERO;
    };
    let (Some(left), Ok(tempo)) = (
        before(&track.clips, clip),
        TempoMap::new(project.tempo.clone()),
    ) else {
        return Ticks::ZERO;
    };
    let Some(source) = audio(clip).and_then(|m| project.media.iter().find(|f| f.id == m)) else {
        return Ticks::ZERO;
    };
    left.length.min(hidden(&tempo, source, clip))
}

/// How much of `clip`'s audio lies before its start.
fn hidden(tempo: &TempoMap, source: &Media, clip: &Clip) -> Ticks {
    let ClipContent::Audio { offset, .. } = clip.content else {
        return Ticks::ZERO;
    };
    let origin = offset as f64 / f64::from(source.sample_rate.max(1)) + clip.audio_phase_seconds;
    clip.start - tempo.ticks_at_seconds((tempo.seconds_at(clip.start) - origin).max(0.0))
}

/// A temporary clip for handles and hit testing, with what a crossfade
/// replaces taken out and put where it is shown: the incoming clip has no
/// fade-in of its own, and the outgoing clip's fade-out is the crossfade
/// (`crossfade` marks it). Never persisted.
pub(crate) fn preview(project: &Project, track: Id, clip: &Clip) -> Clip {
    let mut shown = clip.clone();
    shown.crossfade = false;
    if clip.crossfade {
        shown.fade_in = Fade::default();
    }
    if let Some((_, crossing)) = outgoing(project, track, clip) {
        shown.crossfade = true;
        shown.fade_out = crossing.fade;
    }
    shown
}

/// The audio clip that starts where `left` ends on the same track: the one a
/// crossfade dragged from `left`'s fade-out handle is made with. Both clips
/// must be unlocked audio, and the next one must not loop (its hidden start
/// is what the crossfade uses).
pub(crate) fn incoming(project: &Project, left: Id) -> Option<Id> {
    let track = project
        .tracks
        .iter()
        .find(|t| t.clips.iter().any(|c| c.id == left))?;
    let from = track.clips.iter().find(|c| c.id == left)?;
    if from.locked || audio(from).is_none() {
        return None;
    }
    let end = from.start + from.length;
    track
        .clips
        .iter()
        .find(|c| {
            c.id != left
                && c.start == end
                && !c.locked
                && c.length > Ticks::ZERO
                && c.loop_length.is_none()
                && audio(c).is_some()
        })
        .map(|c| c.id)
}

/// Every clip the crossfades change, as it is heard (by clip id): the
/// outgoing clip with its fade-out, and the incoming one with its fade-in
/// (and, for a crossfade by hand, its hidden start brought in). Seamless
/// joins change nothing. The arrangement draws these clips' waves and the
/// engine plays them; the project's clips stay where they are shown.
pub(crate) fn played(project: &Project) -> HashMap<Id, Played> {
    let mut out: HashMap<Id, Played> = HashMap::new();
    let Ok(tempo) = TempoMap::new(project.tempo.clone()) else {
        return out;
    };
    let sources = project.media.iter().map(|m| (m.id, m)).collect();
    for track in &project.tracks {
        for clip in &track.clips {
            if !clip.crossfade && !project.auto_crossfade {
                continue;
            }
            let Some(left) = before(&track.clips, clip) else {
                continue;
            };
            let Some(crossing) = resolve(project.auto_crossfade, &sources, &tempo, left, clip)
            else {
                continue;
            };
            if crossing.seamless {
                continue;
            }
            let d = crossing.fade.length;
            let fade = Fade {
                length: d,
                curve: crossing.fade.curve,
                ..Fade::default()
            };
            let outgoing = out.entry(left.id).or_insert_with(|| Played {
                clip: left.clone(),
                blend: [false; 2],
            });
            outgoing.clip.fade_out = fade;
            outgoing.blend[1] = true;
            if crossing.zone == Zone::After {
                outgoing.clip.length = outgoing.clip.length + d;
            }
            let incoming = out.entry(clip.id).or_insert_with(|| Played {
                clip: (**clip).clone(),
                blend: [false; 2],
            });
            incoming.clip.fade_in = fade;
            incoming.blend[0] = true;
            if crossing.zone == Zone::Before {
                let c = &mut incoming.clip;
                let begins = tempo.seconds_at(c.start);
                c.start = c.start - d;
                c.length = c.length + d;
                c.audio_phase_seconds -= begins - tempo.seconds_at(c.start);
                c.envelopes.shift(d);
            }
        }
    }
    out
}

fn resolve(
    auto: bool,
    sources: &HashMap<Id, &Media>,
    tempo: &TempoMap,
    left: &Clip,
    clip: &Clip,
) -> Option<Crossing> {
    let explicit = clip.crossfade;
    // An explicit handle dragged to zero overrides the automatic default.
    if explicit && clip.fade_in.length <= Ticks::ZERO {
        return None;
    }
    let ClipContent::Audio { media, offset, .. } = clip.content else {
        return None;
    };
    let ClipContent::Audio {
        media: left_media,
        offset: left_offset,
        ..
    } = left.content
    else {
        return None;
    };
    let source = sources.get(&left_media)?;
    let incoming = sources.get(&media)?;
    let duration = tempo.seconds_at(clip.start) - tempo.seconds_at(left.start);
    let left_origin =
        left_offset as f64 / f64::from(source.sample_rate.max(1)) + left.audio_phase_seconds;
    let right_origin =
        offset as f64 / f64::from(incoming.sample_rate.max(1)) + clip.audio_phase_seconds;
    let contiguous = left.loop_length.is_none()
        && clip.loop_length.is_none()
        && left_media == media
        && left.reversed == clip.reversed
        && (left_origin + duration - right_origin).abs() < 1e-7;
    if explicit {
        // The crossfade runs over the audio the incoming clip keeps before
        // its start, so it needs some, and a clip that does not loop.
        if clip.loop_length.is_some() {
            return None;
        }
        let length = clip
            .fade_in
            .length
            .min(left.length)
            .min(hidden(tempo, incoming, clip));
        return (length > Ticks::ZERO).then_some(Crossing {
            left: left.id,
            fade: Fade {
                length,
                curve: clip.fade_in.curve,
                ..Fade::default()
            },
            zone: Zone::Before,
            seamless: contiguous,
        });
    }
    // Automatic: only where the two are not the same sound, nothing of the
    // user's own fades is overridden, and the outgoing clip has a tail.
    if !auto
        || contiguous
        || clip.fade_in.length > Ticks::ZERO
        || left.fade_out.length > Ticks::ZERO
        || left.loop_length.is_some()
        || clip.loop_length.is_some()
    {
        return None;
    }
    let available =
        source.frames as f64 / f64::from(source.sample_rate.max(1)) - (left_origin + duration);
    if available <= 0.0 {
        return None;
    }
    let begins = tempo.seconds_at(clip.start);
    let wanted = tempo.ticks_at_seconds(begins + 0.005) - clip.start;
    let tail = tempo.ticks_at_seconds(begins + available) - clip.start;
    let length = wanted.min(tail).min(clip.length);
    (length > Ticks::ZERO).then_some(Crossing {
        left: left.id,
        fade: Fade {
            length,
            ..Fade::default()
        },
        zone: Zone::After,
        seamless: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Two audio clips on one track, `left` from beat 0 to 2 and `right`
    /// from beat 2 to 4, of an eight-beat file at 120 BPM (48,000 frames a
    /// beat pair). `right` starts a beat into the file, so it keeps a beat
    /// of audio before its start. Not a split: the left clip's sound at its
    /// end is the file's start again.
    fn pair() -> Project {
        let mut project = lumen_project::testing::small_project(1);
        project.auto_crossfade = false;
        project.tempo = vec![lumen_core::time::TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
            ramp: false,
        }];
        let mut track = project
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| audio(c).is_some()))
            .unwrap()
            .as_ref()
            .clone();
        let mut left = track
            .clips
            .iter()
            .find(|c| audio(c).is_some())
            .unwrap()
            .as_ref()
            .clone();
        let media = audio(&left).unwrap();
        let source = project.media.iter_mut().find(|m| m.id == media).unwrap();
        source.sample_rate = 48_000;
        source.frames = 48_000 * 8;
        left.start = Ticks::ZERO;
        left.length = Ticks::quarters(2);
        left.content = ClipContent::Audio {
            media,
            offset: 0,
            gain_db: 0.0,
        };
        left.audio_phase_seconds = 0.0;
        left.loop_length = None;
        left.loop_phase = Ticks::ZERO;
        left.reversed = false;
        left.crossfade = false;
        left.fade_in = Fade::default();
        left.fade_out = Fade::default();
        let mut right = left.clone();
        right.id = Id(100_000);
        right.start = Ticks::quarters(2);
        // A beat (0.5 s) into the file, which is not where the left clip's
        // sound ends (1 s in the file): a join of unrelated sound.
        right.content = ClipContent::Audio {
            media,
            offset: 12_000,
            gain_db: 0.0,
        };
        track.clips = vec![Arc::new(left), Arc::new(right)];
        project.tracks = vec![Arc::new(track)];
        project
    }

    fn right_mut(p: &mut Project) -> &mut Clip {
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1])
    }

    fn by_hand(p: &mut Project, length: Ticks) {
        let right = right_mut(p);
        right.crossfade = true;
        right.fade_in.length = length;
    }

    #[test]
    fn a_crossfade_by_hand_runs_over_the_end_of_the_left_clip() {
        let mut p = pair();
        by_hand(&mut p, Ticks(Ticks::quarters(1).0 / 4));
        let id = p.tracks[0].id;
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).unwrap();
        assert_eq!(found.zone, Zone::Before);
        assert_eq!(found.fade.length, Ticks(Ticks::quarters(1).0 / 4));
        assert!(!found.seamless);

        // Heard: both clips overlap for the crossfade, which ends where the
        // right clip starts.
        let played = played(&p);
        let (out, inc) = (&played[&p.tracks[0].clips[0].id], &played[&right.id]);
        assert_eq!(out.clip.start + out.clip.length, right.start);
        assert_eq!(out.clip.fade_out.length, found.fade.length);
        assert_eq!(inc.clip.start, right.start - found.fade.length);
        assert_eq!(inc.clip.length, right.length + found.fade.length);
        assert_eq!(inc.clip.fade_in.length, found.fade.length);
        assert_eq!((out.blend, inc.blend), ([false, true], [true, false]));
        // The incoming clip's sound is unmoved: its hidden start is brought in.
        let tempo = TempoMap::new(p.tempo.clone()).unwrap();
        let shift = tempo.seconds_at(right.start) - tempo.seconds_at(inc.clip.start);
        assert!((inc.clip.audio_phase_seconds - (right.audio_phase_seconds - shift)).abs() < 1e-9);
    }

    #[test]
    fn it_is_limited_by_the_hidden_start_and_the_left_clip() {
        let mut p = pair();
        let id = p.tracks[0].id;
        by_hand(&mut p, Ticks::quarters(4));
        // The right clip keeps 12,000 frames (a quarter second, half a beat).
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).unwrap();
        assert_eq!(found.fade.length, Ticks(Ticks::quarters(1).0 / 2));
        // With none hidden there is nothing to blend, and nothing is made.
        let right = right_mut(&mut p);
        right.content = ClipContent::Audio {
            media: audio(right).unwrap(),
            offset: 0,
            gain_db: 0.0,
        };
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
        assert!(played(&p).is_empty());
        // The stale request is not shown either.
        assert_eq!(preview(&p, id, &right).fade_in.length, Ticks::ZERO);
        // Zero turns it off.
        let mut p = pair();
        by_hand(&mut p, Ticks::ZERO);
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
    }

    #[test]
    fn a_gap_or_a_loop_makes_no_crossfade() {
        let mut p = pair();
        let id = p.tracks[0].id;
        by_hand(&mut p, Ticks(Ticks::quarters(1).0 / 4));
        right_mut(&mut p).start = Ticks::quarters(3);
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
        right_mut(&mut p).start = Ticks::quarters(2);
        right_mut(&mut p).loop_length = Some(Ticks::quarters(1));
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
    }

    #[test]
    fn a_crossfade_across_a_split_is_shown_but_changes_nothing_heard() {
        let mut p = pair();
        let id = p.tracks[0].id;
        // The right half continues the left's sound (a plain split).
        let right = right_mut(&mut p);
        right.content = ClipContent::Audio {
            media: audio(right).unwrap(),
            offset: 48_000,
            gain_db: 0.0,
        };
        by_hand(&mut p, Ticks(Ticks::quarters(1).0 / 4));
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).expect("shown");
        assert!(found.seamless);
        assert!(played(&p).is_empty(), "no blend is played");
        let left = p.tracks[0].clips[0].clone();
        assert!(preview(&p, id, &left).crossfade, "but the handle is there");
    }

    #[test]
    fn the_handles_show_the_crossfade_on_the_left_clip() {
        let mut p = pair();
        let id = p.tracks[0].id;
        by_hand(&mut p, Ticks(Ticks::quarters(1).0 / 4));
        let right = (*p.tracks[0].clips[1]).clone();
        let mut left = (*p.tracks[0].clips[0]).clone();
        left.fade_out.length = Ticks::quarters(1);
        let shown = preview(&p, id, &left);
        assert!(shown.crossfade);
        assert_eq!(
            shown.fade_out.length,
            Ticks(Ticks::quarters(1).0 / 4),
            "its own fade is replaced"
        );
        let shown = preview(&p, id, &right);
        assert!(!shown.crossfade);
        assert_eq!(
            shown.fade_in.length,
            Ticks::ZERO,
            "the incoming clip has no fade-in handle"
        );
        let (next, found) = outgoing(&p, id, &left).unwrap();
        assert_eq!((next, found.left), (right.id, left.id));
    }

    #[test]
    fn an_automatic_crossfade_blends_five_ms_over_the_left_clips_tail() {
        let mut p = pair();
        p.auto_crossfade = true;
        let id = p.tracks[0].id;
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).unwrap();
        assert_eq!(found.zone, Zone::After);
        let tempo = TempoMap::new(p.tempo.clone()).unwrap();
        assert_eq!(
            tempo.sample_at(right.start + found.fade.length, 48_000)
                - tempo.sample_at(right.start, 48_000),
            240
        );
        let played = played(&p);
        let out = &played[&p.tracks[0].clips[0].id].clip;
        assert_eq!(out.length, p.tracks[0].clips[0].length + found.fade.length);
        assert_eq!(
            played[&right.id].clip.start, right.start,
            "the right clip stays"
        );
        assert_eq!(played[&right.id].clip.fade_in.length, found.fade.length);
        // Its own fades, no tail, a split, and the setting all win.
        right_mut(&mut p).fade_in.length = Ticks(10);
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
        right_mut(&mut p).fade_in.length = Ticks::ZERO;
        p.auto_crossfade = false;
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
        p.auto_crossfade = true;
        let ClipContent::Audio { media, .. } = p.tracks[0].clips[0].content else {
            unreachable!()
        };
        p.media.iter_mut().find(|m| m.id == media).unwrap().frames = 48_000;
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none(), "no audio past the end");
    }
}
