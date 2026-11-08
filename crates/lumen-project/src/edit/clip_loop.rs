//! An audio clip's loop, set from the audio editor's brace (M9.5 S2b, D21):
//! turned on, its end or its start moved, or turned off. The clip stays where
//! it is on the arrangement and the file does not move: only what repeats
//! changes.
//!
//! The loop's origin `O` is the start of the turn the clip starts in, worked
//! out as the engine plays it (`(position + phase) mod period` from the
//! stored clock `C = start − loop_phase`). Every edit keeps the frame the
//! clip plays at each beat from its start up to its first repeat:
//!
//! | Edit | `loop_length` | `loop_phase` | `audio_phase_seconds` | `length` |
//! |---|---|---|---|---|
//! | On | `length`, capped to the file | 0 | same | same |
//! | End to E | E − O | start − O | same | same |
//! | Start to O′ | old end − O′ | start − O′ | + s(O′) − s(O) | same |
//! | Off | none | 0 | + s(start) − s(O) | capped to the file |
//!
//! Envelope points are ticks from the stored clock (`lanes.rs` bakes them
//! from `start − loop_phase` when the clip loops, from `start` when not). A
//! new clock re-times them so each stays on the audio it was drawn over:
//! `t′ = ticks_at(s(C′) + s(C + t) − s(C) − d) − C′`, where `d` is how many
//! seconds the file moved under the new clock. Points outside the loop are
//! kept, unheard. Takes' passes move by the same seconds (`takes::advance`).

use lumen_core::automation::{Lane, Point};
use lumen_core::time::{TempoMap, Ticks};

use super::{Edge, MIN_LENGTH, audio_of, takes, trim};
use crate::model::{Clip, ClipContent, Project};

/// What the brace asks of a clip's loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopEdit {
    /// Loop the clip as it is: one turn is its length, or what the file holds.
    On,
    /// The loop ends at this time (its origin stays).
    End(Ticks),
    /// The loop begins at this time (its end stays).
    Start(Ticks),
    /// Stop looping: the file carries on from where the clip starts.
    Off,
}

/// The turn of a looping clip that its start falls in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    /// Where the turn begins: at or before the clip's start (to the tick).
    pub origin: Ticks,
    /// The same place in seconds, exactly, as the engine's frames have it.
    pub origin_seconds: f64,
    /// Where the turn ends, to the tick.
    pub end: Ticks,
    /// One turn, in seconds.
    pub seconds: f64,
}

/// The turn `clip`'s start is in, if it loops.
pub fn turn(tempo: &TempoMap, clip: &Clip) -> Option<Turn> {
    let period = clip.loop_length.filter(|p| *p > Ticks::ZERO)?;
    let clock = clip.start - clip.loop_phase;
    let seconds = tempo.seconds_at(clock + period) - tempo.seconds_at(clock);
    if seconds <= 0.0 {
        return None;
    }
    let start = tempo.seconds_at(clip.start);
    let phase = (start - tempo.seconds_at(clock)).rem_euclid(seconds);
    let origin_seconds = start - phase;
    Some(Turn {
        origin: tempo.ticks_at_seconds(origin_seconds).min(clip.start),
        origin_seconds,
        end: tempo.ticks_at_seconds(origin_seconds + seconds),
        seconds,
    })
}

/// How far each edge of the brace may go, as [`set_clip_loop`] holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopLimits {
    /// The loop's start: from the earliest (the song's start or the file's
    /// first frame) to the clip's start.
    pub start: (Ticks, Ticks),
    /// The loop's end: after the clip's start and a least loop past the
    /// origin, up to the file's end.
    pub end: (Ticks, Ticks),
}

/// The file's length and where the clip's clock reads it, in seconds: the
/// file is `frames / rate` long, and at the turn's origin (or the clip's
/// start, for a clip that does not loop) the clip plays second `at`.
struct FileTime {
    length: f64,
    at: f64,
}

fn file_time(project: &Project, clip: &Clip) -> Option<FileTime> {
    let (rate, frames, offset) = audio_of(project, clip)?;
    let rate = f64::from(rate.max(1));
    Some(FileTime {
        length: frames as f64 / rate,
        at: offset as f64 / rate + clip.audio_phase_seconds,
    })
}

/// The limits of `clip`'s brace, if it is an audio clip that loops.
pub fn loop_limits(project: &Project, tempo: &TempoMap, clip: &Clip) -> Option<LoopLimits> {
    let turn = turn(tempo, clip)?;
    let file = file_time(project, clip)?;
    // The file's first frame sits `file.at` seconds before the origin.
    let first = tempo
        .ticks_at_seconds(turn.origin_seconds - file.at)
        .max(Ticks::ZERO);
    let earliest = first.max(Ticks::ZERO).min(clip.start);
    // Its last frame, `length − at` seconds after.
    let last = tempo.ticks_at_seconds(turn.origin_seconds + file.length - file.at);
    let least_end = (turn.origin + MIN_LENGTH).max(clip.start + Ticks(1));
    // A start that leaves the loop shorter than the least.
    let latest_start = clip.start.min(turn.end - MIN_LENGTH).max(earliest);
    Some(LoopLimits {
        start: (earliest, latest_start),
        end: (least_end, last.max(least_end)),
    })
}

/// `clip` with its loop changed by `edit`, within [`loop_limits`]. `None`
/// when the clip is not audio, its file is unknown, or the edit does not
/// apply (an edge moved on a clip that does not loop, On on one that does).
pub fn set_clip_loop(
    project: &Project,
    tempo: &TempoMap,
    clip: &Clip,
    edit: LoopEdit,
) -> Option<Clip> {
    if !matches!(clip.content, ClipContent::Audio { .. }) {
        return None;
    }
    let file = file_time(project, clip)?;
    let rate = audio_of(project, clip).map(|(rate, _, _)| f64::from(rate.max(1)));
    let looping = clip.loop_length.is_some_and(|p| p > Ticks::ZERO);
    let start_s = tempo.seconds_at(clip.start);
    let mut c = clip.clone();
    match edit {
        LoopEdit::On => {
            if looping {
                return None;
            }
            // What the file holds from the clip's start.
            let left = file.length - file.at;
            let holds = tempo.ticks_at_seconds(start_s + left) - clip.start;
            if holds < MIN_LENGTH {
                return None;
            }
            c.loop_length = Some(clip.length.min(holds));
            c.loop_phase = Ticks::ZERO;
            // The clock stays the clip's start: nothing moves.
            Some(c)
        }
        LoopEdit::End(to) => {
            let turn = turn(tempo, clip)?;
            let limits = loop_limits(project, tempo, clip)?;
            let to = to.clamp(limits.end.0, limits.end.1);
            let d = tempo.seconds_at(turn.origin) - turn.origin_seconds;
            c.loop_length = Some(to - turn.origin);
            c.loop_phase = clip.start - turn.origin;
            c.audio_phase_seconds = clip.audio_phase_seconds + d;
            retime(tempo, clip, &mut c, turn.origin, d);
            takes::advance(&mut c, Ticks::ZERO, d, rate, &|_| false);
            Some(c)
        }
        LoopEdit::Start(to) => {
            let turn = turn(tempo, clip)?;
            let limits = loop_limits(project, tempo, clip)?;
            let to = to.clamp(limits.start.0, limits.start.1);
            let d = tempo.seconds_at(to) - turn.origin_seconds;
            c.loop_length = Some(turn.end - to);
            c.loop_phase = clip.start - to;
            c.audio_phase_seconds = clip.audio_phase_seconds + d;
            retime(tempo, clip, &mut c, to, d);
            takes::advance(&mut c, Ticks::ZERO, d, rate, &|_| false);
            Some(c)
        }
        LoopEdit::Off => {
            if !looping {
                return None;
            }
            let turn = turn(tempo, clip)?;
            let d = start_s - turn.origin_seconds;
            c.loop_length = None;
            c.loop_phase = Ticks::ZERO;
            c.audio_phase_seconds = clip.audio_phase_seconds + d;
            retime(tempo, clip, &mut c, clip.start, d);
            takes::advance(&mut c, Ticks::ZERO, d, rate, &|_| false);
            // As much of the clip as the file holds from its first frame: a
            // trim of the end to where it is caps it there, fades and all.
            let end = clip.start + clip.length;
            Some(trim(project, tempo, &c, Edge::End, end, false))
        }
    }
}

/// The clock the engine reads `clip`'s envelopes from.
fn clock_of(clip: &Clip) -> Ticks {
    if clip.loop_length.is_some_and(|p| p > Ticks::ZERO) {
        clip.start - clip.loop_phase
    } else {
        clip.start
    }
}

/// Re-times `to`'s envelope points from `from`'s clock to `clock`, the file
/// having moved `d` seconds under it, so each stays on the same audio.
fn retime(tempo: &TempoMap, from: &Clip, to: &mut Clip, clock: Ticks, d: f64) {
    let old = clock_of(from);
    if old == clock && d == 0.0 {
        return;
    }
    let (old_s, new_s) = (tempo.seconds_at(old), tempo.seconds_at(clock));
    for (kind, lane) in from.envelopes.iter() {
        let points: Vec<Point> = lane
            .points()
            .iter()
            .map(|p| {
                let heard = tempo.seconds_at(old + p.at) - old_s;
                Point {
                    at: tempo.ticks_at_seconds(new_s + heard - d) - clock,
                    ..*p
                }
            })
            .collect();
        // Points one tick apart can land on the same tick: keep the first.
        let mut kept: Vec<Point> = Vec::with_capacity(points.len());
        for point in points {
            if kept.last().is_none_or(|last| point.at > last.at) {
                kept.push(point);
            }
        }
        if let Ok(lane) = Lane::new(kept) {
            to.envelopes.set(kind, lane);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_core::time::{PPQ, TempoPoint};

    use super::*;
    use crate::edit::tests::{q, song};
    use crate::envelope::Kind;

    fn tempo(project: &Project) -> TempoMap {
        TempoMap::new(project.tempo.clone()).expect("a tempo map")
    }

    /// The clip from 4 to 12 of `song()`, changed by `edit`.
    /// The clip from 4 to 12, playing its file from its first frame.
    fn the_clip(project: &Project) -> Clip {
        played_from(project, 0)
    }

    /// The same clip playing its file from frame `offset`.
    fn played_from(project: &Project, offset: i64) -> Clip {
        let mut clip = project.tracks[0].clips[0].as_ref().clone();
        if let ClipContent::Audio { offset: o, .. } = &mut clip.content {
            *o = offset;
        }
        clip
    }

    /// The file second the clip plays at `t` (inside it), as the engine reads
    /// it: from the clock, modulo a turn when it loops.
    fn heard(project: &Project, tempo: &TempoMap, clip: &Clip, t: Ticks) -> f64 {
        let file = file_time(project, clip).expect("audio");
        let clock = clock_of(clip);
        let from_clock = tempo.seconds_at(t) - tempo.seconds_at(clock);
        match clip.loop_length {
            Some(p) => {
                let turn = tempo.seconds_at(clock + p) - tempo.seconds_at(clock);
                file.at + from_clock.rem_euclid(turn)
            }
            None => file.at + from_clock,
        }
    }

    fn assert_same_until(project: &Project, a: &Clip, b: &Clip, until: Ticks) {
        let tempo = tempo(project);
        let mut t = a.start;
        while t < until {
            let (x, y) = (heard(project, &tempo, a, t), heard(project, &tempo, b, t));
            assert!((x - y).abs() < 1e-6, "at {t:?}: {x} != {y}");
            t = t + Ticks(PPQ / 8);
        }
    }

    fn with_tempo_change(mut project: Project) -> Project {
        project.tempo = vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
                ramp: false,
            },
            TempoPoint {
                at: q(6),
                bpm: 75.0,
                ramp: true,
            },
            TempoPoint {
                at: q(10),
                bpm: 90.0,
                ramp: false,
            },
        ];
        project
    }

    #[test]
    fn loop_on_keeps_every_frame_and_loops_the_clips_length() {
        let project = song();
        let clip = the_clip(&project);
        let on = set_clip_loop(&project, &tempo(&project), &clip, LoopEdit::On).expect("on");
        assert_eq!(on.loop_length, Some(clip.length));
        assert_eq!(on.loop_phase, Ticks::ZERO);
        assert_eq!(on.audio_phase_seconds, clip.audio_phase_seconds);
        assert_same_until(&project, &clip, &on, clip.start + clip.length);
        // On a clip that loops already it does nothing.
        assert!(set_clip_loop(&project, &tempo(&project), &on, LoopEdit::On).is_none());
    }

    #[test]
    fn loop_on_is_capped_to_what_the_file_holds() {
        let mut project = song();
        // A file of two seconds: four quarters at 120.
        project.media[0].frames = 96_000;
        let clip = the_clip(&project);
        let on = set_clip_loop(&project, &tempo(&project), &clip, LoopEdit::On).expect("on");
        assert_eq!(on.loop_length, Some(q(4)));
        assert_eq!(on.length, clip.length, "the clip stays as long");
    }

    #[test]
    fn moving_the_end_keeps_the_frames_up_to_the_earlier_repeat() {
        for project in [song(), with_tempo_change(song())] {
            let tempo = tempo(&project);
            let mut clip = the_clip(&project);
            clip.loop_length = Some(q(4));
            clip.loop_phase = q(1);
            for end in [q(5), q(6), q(9)] {
                let moved = set_clip_loop(&project, &tempo, &clip, LoopEdit::End(end)).unwrap();
                let old_end = turn(&tempo, &clip).unwrap().end;
                assert_same_until(&project, &clip, &moved, old_end.min(end));
                assert_eq!(turn(&tempo, &moved).unwrap().end, end, "{end:?}");
            }
        }
    }

    #[test]
    fn moving_the_start_keeps_the_frames_and_the_end() {
        for project in [song(), with_tempo_change(song())] {
            let tempo = tempo(&project);
            // Four seconds of file before the clip's first frame.
            let mut clip = played_from(&project, 192_000);
            clip.loop_length = Some(q(4));
            clip.loop_phase = q(1);
            let old = turn(&tempo, &clip).unwrap();
            for start in [q(2), q(3) + Ticks(PPQ / 3), q(4)] {
                let moved = set_clip_loop(&project, &tempo, &clip, LoopEdit::Start(start)).unwrap();
                let new = turn(&tempo, &moved).unwrap();
                assert_eq!(new.origin, start);
                assert_eq!(new.end, old.end, "the end stays put");
                assert_same_until(&project, &clip, &moved, old.end);
            }
        }
    }

    #[test]
    fn the_start_stops_at_the_clips_start_and_the_files_first_frame() {
        let project = song();
        let tempo = tempo(&project);
        let mut clip = the_clip(&project);
        clip.loop_length = Some(q(4));
        // Past the clip's start: it stops there.
        let late = set_clip_loop(&project, &tempo, &clip, LoopEdit::Start(q(7))).unwrap();
        assert_eq!(turn(&tempo, &late).unwrap().origin, clip.start);
        // The file starts at the clip's start (offset 0): no earlier.
        let early = set_clip_loop(&project, &tempo, &clip, LoopEdit::Start(q(1))).unwrap();
        assert_eq!(turn(&tempo, &early).unwrap().origin, clip.start);
        let limits = loop_limits(&project, &tempo, &clip).unwrap();
        assert_eq!(limits.start, (clip.start, clip.start));
    }

    #[test]
    fn the_end_stays_after_the_clips_start_and_inside_the_file() {
        let project = song();
        let tempo = tempo(&project);
        let mut clip = the_clip(&project);
        clip.loop_length = Some(q(4));
        let limits = loop_limits(&project, &tempo, &clip).unwrap();
        assert!(limits.end.0 > clip.start);
        // Ten seconds of file from beat 4: it ends at beat 24.
        assert_eq!(limits.end.1, q(24));
        let long = set_clip_loop(&project, &tempo, &clip, LoopEdit::End(q(40))).unwrap();
        assert_eq!(long.loop_length, Some(q(20)));
        let short = set_clip_loop(&project, &tempo, &clip, LoopEdit::End(q(1))).unwrap();
        assert_eq!(short.loop_length, Some(MIN_LENGTH.max(Ticks(1))));
    }

    #[test]
    fn loop_off_keeps_the_frames_to_the_first_repeat() {
        for project in [song(), with_tempo_change(song())] {
            let tempo = tempo(&project);
            let mut clip = the_clip(&project);
            clip.loop_length = Some(q(3));
            clip.loop_phase = q(2);
            let first_repeat = turn(&tempo, &clip).unwrap().end;
            let off = set_clip_loop(&project, &tempo, &clip, LoopEdit::Off).unwrap();
            assert_eq!(off.loop_length, None);
            assert_same_until(&project, &clip, &off, first_repeat);
        }
    }

    #[test]
    fn loop_on_then_off_gives_the_clip_back() {
        for project in [song(), with_tempo_change(song())] {
            let tempo = tempo(&project);
            let clip = the_clip(&project);
            let on = set_clip_loop(&project, &tempo, &clip, LoopEdit::On).unwrap();
            let off = set_clip_loop(&project, &tempo, &on, LoopEdit::Off).unwrap();
            assert_eq!(off, clip);
        }
    }

    #[test]
    fn loop_off_caps_a_clip_longer_than_its_file() {
        let mut project = song();
        project.media[0].frames = 96_000;
        let mut clip = the_clip(&project);
        clip.loop_length = Some(q(4));
        let off = set_clip_loop(&project, &tempo(&project), &clip, LoopEdit::Off).unwrap();
        assert_eq!(off.length, q(4));
    }

    #[test]
    fn envelope_points_stay_on_their_audio() {
        let project = with_tempo_change(song());
        let tempo = tempo(&project);
        let mut clip = played_from(&project, 192_000);
        clip.loop_length = Some(q(4));
        clip.loop_phase = q(5);
        let lane = Lane::new(vec![
            Point::new(q(0), -6.0),
            Point::new(q(1), 0.0),
            Point::new(q(3), -3.0),
        ])
        .unwrap();
        clip.envelopes.set(Kind::Gain, lane.clone());
        // The file second each point is heard over.
        let at_audio = |c: &Clip, p: &Point| {
            let clock = clock_of(c);
            file_time(&project, c).unwrap().at + tempo.seconds_at(clock + p.at)
                - tempo.seconds_at(clock)
        };
        let before: Vec<f64> = lane.points().iter().map(|p| at_audio(&clip, p)).collect();
        for edit in [LoopEdit::Start(q(2)), LoopEdit::End(q(9)), LoopEdit::Off] {
            let edited = set_clip_loop(&project, &tempo, &clip, edit).unwrap();
            let moved = edited.envelopes.get(Kind::Gain).unwrap();
            assert_eq!(moved.len(), 3, "{edit:?}");
            for (p, want) in moved.points().iter().zip(&before) {
                let got = at_audio(&edited, p);
                // To the tick: a tick is under a millisecond here.
                assert!((got - want).abs() < 1e-3, "{edit:?}: {got} != {want}");
            }
        }
    }

    #[test]
    fn a_midi_clip_and_an_edge_without_a_loop_do_nothing() {
        let mut project = song();
        let tempo = tempo(&project);
        let clip = the_clip(&project);
        assert!(set_clip_loop(&project, &tempo, &clip, LoopEdit::End(q(9))).is_none());
        assert!(set_clip_loop(&project, &tempo, &clip, LoopEdit::Off).is_none());
        let track = Arc::make_mut(&mut project.tracks[0]);
        Arc::make_mut(&mut track.clips[0]).content = ClipContent::midi();
        let midi = the_clip(&project);
        assert!(set_clip_loop(&project, &tempo, &midi, LoopEdit::On).is_none());
    }
}
