//! Recording MIDI (M5 slice 6b): where a take goes in a project.
//!
//! A recording commits its take about once a second, so [`place`] is
//! idempotent. Each call starts again from `base`, the project as it was when
//! the recording began, so a growing take placed again gives the same result
//! as one call with the final take. The app wraps every call in one undo step
//! (`Document::record_take`), so a whole recording undoes at once.
//!
//! # Times
//!
//! A take holds absolute project ticks, and so do its notes and events. They
//! go into clips relative to each clip's start. Notes and events outside the
//! range are dropped; a note that starts before the range is dropped, not
//! shortened. A note that runs past the range's end, or past the end of the
//! clip it goes into, is cut there and keeps at least one tick. Otherwise a
//! note is at least [`MIN_NOTE`] long.
//!
//! # Modes
//!
//! - [`Mode::Replace`]: the take is a new clip over the range. What is under
//!   it is cut away, as every clip put down is (`super::overwrite`).
//! - [`Mode::Overdub`]: also a new clip over the range, wherever it begins
//!   (inside a clip, at a clip's start, before one or over empty lane), as
//!   Ableton records in the arrangement: what is under it is cut away, so
//!   the clips recorded over stay clips of their own beside it, holding
//!   what they covered hidden (format v15). The new clip carries the notes
//!   and events the clips under it played there, so nothing heard is lost.
//!   Carried notes stay whole: one already sounding where the take begins
//!   keeps its start before the new clip (hidden), and one running past the
//!   take's end is not cut, so both carry on across the seams with the
//!   clips around the new one (`super::seams`) and sustain as they did.
//!   Begun inside a clip, the new clip takes its name and colour. The clip
//!   is there from the moment recording begins, empty until something is
//!   played, as a Replace take's is.
//!
//!   A note played over an old one of the same pitch wins it, as in Ableton:
//!   the old note is cut where the new one starts, or dropped when it starts
//!   under it (`settle`). With Loop on, every pass layers into the same clip.

use std::sync::Arc;

use lumen_core::time::Ticks;

use super::notes::MIN_NOTE;
use super::{in_window, overwrite, repeats, shown, to_arrangement};
use crate::model::{Clip, ClipContent, ClipEvent, EventKind, Id, Note, Project, Track, TrackKind};

/// How a take lands on the clips under its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A new clip over the range; what is under it is cut away.
    Replace,
    /// A new clip over the range that also carries what the clips under it
    /// played there; what is under it is cut away.
    Overdub,
}

/// A note of a take, at absolute project ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeNote {
    pub start: Ticks,
    pub length: Ticks,
    pub pitch: u8,
    pub velocity: u8,
}

/// A controller, bend, pressure or program change of a take, at an absolute
/// project time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeEvent {
    pub at: Ticks,
    pub kind: EventKind,
}

/// What a recording has played so far: its notes and events, and the range
/// `[start, end)` they cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Take {
    pub start: Ticks,
    pub end: Ticks,
    pub notes: Vec<TakeNote>,
    pub events: Vec<TakeEvent>,
}

/// How a take is placed: the mode, the punch range that cuts it, and the id
/// of the clip the take is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub mode: Mode,
    pub punch: Option<(Ticks, Ticks)>,
    pub take_clip: Id,
}

/// A locked clip is in the way of the take, so nothing is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blocked;

/// Places `take` on the MIDI track `track` of `project`, starting from the
/// track as `base` has it. Returns the ids of the clips it made or changed.
///
/// The take is cut to the placement's `punch` when given. An empty range, a track that is not
/// MIDI and a track `base` does not have place nothing, and the track still
/// becomes `base`'s. A locked clip overlapping the range returns [`Blocked`],
/// with the track as `base` has it.
///
/// `fresh` gives ids for the parts of clips the take cuts in two. Calling it
/// again with the same `base` and `take_clip` and a longer take gives what
/// one call with the final take would.
pub fn place(
    project: &mut Project,
    base: &Project,
    track: Id,
    take: &Take,
    placement: Placement,
    fresh: &mut impl FnMut() -> Id,
) -> Result<Vec<Id>, Blocked> {
    let Placement {
        mode,
        punch,
        take_clip,
    } = placement;
    let Some(source) = base.tracks.iter().find(|t| t.id == track) else {
        return Ok(Vec::new());
    };
    if let Some(slot) = project.tracks.iter_mut().find(|t| t.id == track) {
        *slot = Arc::clone(source);
    }
    let Some(range) = range_of(take, punch) else {
        return Ok(Vec::new());
    };
    if source.kind != TrackKind::Midi {
        return Ok(Vec::new());
    }
    if source.clips.iter().any(|c| c.locked && overlaps(c, range)) {
        return Err(Blocked);
    }
    Ok(match mode {
        Mode::Replace => replace(project, track, range, take, take_clip, fresh),
        Mode::Overdub => overdub(project, track, source, range, take, take_clip, fresh),
    })
}

/// The range the take lands in: its own, from zero, cut to the punch.
fn range_of(take: &Take, punch: Option<(Ticks, Ticks)>) -> Option<(Ticks, Ticks)> {
    let (mut a, mut b) = (take.start.max(Ticks::ZERO), take.end);
    if let Some((from, to)) = punch {
        a = a.max(from);
        b = b.min(to);
    }
    (a < b).then_some((a, b))
}

fn overlaps(clip: &Clip, (a, b): (Ticks, Ticks)) -> bool {
    clip.start < b && clip.start + clip.length > a
}

fn is_midi(clip: &Clip) -> bool {
    matches!(clip.content, ClipContent::Midi { .. })
}

/// Track `id` of `project`, ready to change.
fn track_mut(project: &mut Project, id: Id) -> Option<&mut Track> {
    project
        .tracks
        .iter_mut()
        .find(|t| t.id == id)
        .map(Arc::make_mut)
}

fn add_clips(project: &mut Project, track: Id, clips: Vec<Clip>) {
    if clips.is_empty() {
        return;
    }
    if let Some(t) = track_mut(project, track) {
        t.clips.extend(clips.into_iter().map(Arc::new));
        t.clips.sort_by_key(|c| c.start);
    }
}

fn new_clip(id: Id, start: Ticks, length: Ticks, content: Content) -> Clip {
    let (notes, events) = content;
    Clip {
        id,
        name: "MIDI clip".into(),
        start,
        length,
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: None,
        content: ClipContent::Midi { notes, events },
    }
}

/// A clip's notes and events.
type Content = (Vec<Note>, Vec<ClipEvent>);

fn replace(
    project: &mut Project,
    track: Id,
    (a, b): (Ticks, Ticks),
    take: &Take,
    take_clip: Id,
    fresh: &mut impl FnMut() -> Id,
) -> Vec<Id> {
    let content = fill(take, (a, b), a, b - a);
    add_clips(project, track, vec![new_clip(take_clip, a, b - a, content)]);
    overwrite(project, &[take_clip], fresh);
    vec![take_clip]
}

fn overdub(
    project: &mut Project,
    track: Id,
    source: &Track,
    (a, b): (Ticks, Ticks),
    take: &Take,
    take_clip: Id,
    fresh: &mut impl FnMut() -> Id,
) -> Vec<Id> {
    let heard = heard_in(source, (a, b));
    let old_notes = carry(&heard, (a, b));
    let (_, mut events) = fill(&heard, (a, b), a, b - a);
    let (new_notes, new_events) = fill(take, (a, b), a, b - a);
    events.extend(new_events);
    events.sort_by_key(|e| e.at);
    let mut clip = new_clip(take_clip, a, b - a, (settle(old_notes, new_notes), events));
    if let Some(begun_in) = begun_in(source, a) {
        clip.name.clone_from(&begun_in.name);
        clip.color = begun_in.color;
    }
    add_clips(project, track, vec![clip]);
    overwrite(project, &[take_clip], fresh);
    vec![take_clip]
}

/// The MIDI clip that plays at `t` (from its start, before its end), if
/// any: an Overdub begun there takes its name and colour.
fn begun_in(track: &Track, t: Ticks) -> Option<&Clip> {
    track
        .clips
        .iter()
        .map(|c| &**c)
        .find(|c| is_midi(c) && c.start <= t && t < c.start + c.length)
}

/// What the MIDI clips of `track` that are not muted play in `[a, b)`: the
/// notes and events in their windows, at project time, every repeat of a
/// looping clip, each note as long as it plays. A note already sounding at
/// `a` counts, with its own start. Muted notes play nothing, so they are
/// not heard.
fn heard_in(track: &Track, (a, b): (Ticks, Ticks)) -> Take {
    let mut heard = Take {
        start: a,
        end: b,
        notes: Vec::new(),
        events: Vec::new(),
    };
    for clip in track
        .clips
        .iter()
        .filter(|c| !c.muted && overlaps(c, (a, b)))
    {
        let ClipContent::Midi { notes, events } = &clip.content else {
            continue;
        };
        for repeat in 0..repeats(clip) {
            let room = shown(clip, repeat);
            let at = |local: Ticks| to_arrangement(clip, local, repeat);
            for n in notes
                .iter()
                .filter(|n| !n.muted() && in_window(clip, n.start()) && n.start() < room)
            {
                let start = at(n.start());
                let length = n.length().min(room - n.start());
                if start < b && start + length > a {
                    heard.notes.push(TakeNote {
                        start,
                        length,
                        pitch: n.pitch(),
                        velocity: n.velocity(),
                    });
                }
            }
            for e in events
                .iter()
                .filter(|e| in_window(clip, e.at) && e.at < room)
            {
                let at = at(e.at);
                if (a..b).contains(&at) {
                    heard.events.push(TakeEvent { at, kind: e.kind });
                }
            }
        }
    }
    heard
}

/// The notes `heard` holds as the content of a clip beginning at `a`: each
/// whole and as long as it was heard. One sounding at `a` keeps its start
/// before the clip, hidden, so it carries on across the seam from the
/// clip before (`super::seams`) instead of stopping there; one running past
/// `b` is not cut, so it carries on into what follows. A take placed again
/// with a later end leaves these notes as they were, so the note playing
/// is the same one from commit to commit and is not struck again.
fn carry(heard: &Take, (a, b): (Ticks, Ticks)) -> Vec<Note> {
    let mut notes: Vec<Note> = heard
        .notes
        .iter()
        .filter(|n| n.start < b && n.start + n.length > a)
        .map(|n| Note::new(n.start - a, n.length, n.pitch, n.velocity))
        .collect();
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    notes
}

/// `new` notes laid over `old` ones, as Ableton overdubs: a note played wins
/// its pitch. An earlier note of the same pitch still sounding where it
/// starts is cut there, and one that starts under it is dropped, so no two
/// notes of a pitch overlap (they would also fight over one key when
/// played). New notes settle among themselves the same way, the later one
/// winning: a loop pass over a note of the last one. Notes of other pitches
/// are untouched. Returns the notes sorted by start, then pitch.
fn settle(old: Vec<Note>, mut new: Vec<Note>) -> Vec<Note> {
    let mut by_pitch: Vec<Vec<Note>> = vec![Vec::new(); 128];
    for n in old {
        by_pitch[usize::from(n.pitch().min(127))].push(n);
    }
    new.sort_by_key(|n| n.start());
    for n in new {
        let lane = &mut by_pitch[usize::from(n.pitch().min(127))];
        lane.retain_mut(|r| {
            if r.start() < n.start() {
                if r.end() > n.start() {
                    r.1 = n.start() - r.start();
                }
                true
            } else {
                r.start() >= n.end()
            }
        });
        lane.push(n);
    }
    let mut all: Vec<Note> = by_pitch.into_iter().flatten().collect();
    all.sort_by_key(|n| (n.start(), n.pitch()));
    all
}

/// The take's notes and events starting in `[from, to)`, as the content of a
/// clip that starts at `start` and repeats every `period` ticks (its length,
/// or its loop length). A note is cut where the content ends.
fn fill(take: &Take, (from, to): (Ticks, Ticks), start: Ticks, period: Ticks) -> Content {
    let period = period.max(Ticks(1));
    let notes: Vec<Note> = take
        .notes
        .iter()
        .filter(|n| (from..to).contains(&n.start))
        .map(|n| {
            let at = relative(n.start, start, period);
            // Cut where the content ends, and where the range does.
            let room = (period - at).min(to - n.start);
            Note::new(at, n.length.max(MIN_NOTE).min(room), n.pitch, n.velocity)
        })
        .collect();
    let notes = settle(Vec::new(), notes);
    let mut events: Vec<ClipEvent> = take
        .events
        .iter()
        .filter(|e| (from..to).contains(&e.at))
        .map(|e| ClipEvent {
            at: relative(e.at, start, period),
            kind: e.kind,
        })
        .collect();
    events.sort_by_key(|e| e.at);
    (notes, events)
}

/// Where time `t` falls in a clip that starts at `start` and repeats every
/// `period`.
fn relative(t: Ticks, start: Ticks, period: Ticks) -> Ticks {
    Ticks((t - start).0.rem_euclid(period.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::shown_notes;
    use crate::model::Color;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn note(start: i64, length: i64, pitch: u8) -> Note {
        Note::new(q(start), q(length), pitch, 100)
    }

    fn tn(start: i64, length: i64, pitch: u8) -> TakeNote {
        TakeNote {
            start: q(start),
            length: q(length),
            pitch,
            velocity: 100,
        }
    }

    fn ev(at: i64, value: u8) -> TakeEvent {
        TakeEvent {
            at: q(at),
            kind: EventKind::Pressure { value },
        }
    }

    fn ev_at(at: i64, value: u8) -> ClipEvent {
        ClipEvent {
            at: q(at),
            kind: EventKind::Pressure { value },
        }
    }

    fn take(start: i64, end: i64, notes: Vec<TakeNote>) -> Take {
        Take {
            start: q(start),
            end: q(end),
            notes,
            events: Vec::new(),
        }
    }

    fn clip(id: u64, start: i64, length: i64, notes: Vec<Note>) -> Clip {
        Clip {
            id: Id(id),
            name: format!("c{id}"),
            start: q(start),
            length: q(length),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            content: midi(notes),
        }
    }

    fn midi(notes: Vec<Note>) -> ClipContent {
        ClipContent::Midi {
            notes,
            events: Vec::new(),
        }
    }

    /// Track 1 (MIDI, `clips`) and track 2 (MIDI, one clip of its own, so a
    /// test can see that it is left alone). The master is replaced so its id
    /// cannot collide with the test ids.
    fn base(clips: Vec<Clip>) -> Project {
        let mut p = crate::testing::small_project(1);
        p.markers.clear();
        p.media.clear();
        p.master = Arc::new(Track::master(Id(999_999)));
        p.tracks = vec![
            Arc::new(Track {
                clips: clips.into_iter().map(Arc::new).collect(),
                ..Track::new(Id(1), "Keys", TrackKind::Midi, Color(0))
            }),
            Arc::new(Track {
                clips: vec![Arc::new(clip(20, 0, 4, vec![note(0, 1, 36)]))],
                ..Track::new(Id(2), "Bass", TrackKind::Midi, Color(0))
            }),
        ];
        p
    }

    /// Fresh ids counting from 900, as one call's `fresh` would.
    fn ids() -> impl FnMut() -> Id {
        let mut next = 900;
        move || {
            let id = Id(next);
            next += 1;
            id
        }
    }

    #[test]
    fn a_note_played_again_wins_its_pitch() {
        // An old 60 from 0 to 4; the take plays 60 again at 2: the old one
        // is cut at 2, not stacked under the new one.
        assert_eq!(
            settle(vec![note(0, 4, 60)], vec![note(2, 1, 60)]),
            vec![note(0, 2, 60), note(2, 1, 60)]
        );
        // An old note that starts under the new one goes; one that starts
        // after it ends stays; other pitches are left alone.
        assert_eq!(
            settle(
                vec![note(3, 1, 60), note(6, 1, 60), note(3, 2, 64)],
                vec![note(2, 3, 60)],
            ),
            vec![note(2, 3, 60), note(3, 2, 64), note(6, 1, 60)]
        );
        // Played on the same start: the new note replaces the old.
        assert_eq!(
            settle(vec![note(1, 2, 60)], vec![Note::new(q(1), q(1), 60, 30)]),
            vec![Note::new(q(1), q(1), 60, 30)]
        );
    }

    #[test]
    fn new_notes_settle_among_themselves_the_later_winning() {
        // A loop pass over a note of the pass before.
        assert_eq!(
            settle(Vec::new(), vec![note(3, 1, 60), note(1, 4, 60)]),
            vec![note(1, 2, 60), note(3, 1, 60)]
        );
    }

    #[test]
    fn an_overdub_over_the_same_pitch_cuts_the_old_note() {
        let base_project = base(vec![clip(10, 0, 8, vec![note(0, 4, 60), note(0, 4, 67)])]);
        let mut p = base_project.clone();
        put(
            &mut p,
            &base_project,
            &take(0, 8, vec![tn(2, 1, 60)]),
            Mode::Overdub,
        )
        .expect("placed");
        let ClipContent::Midi { notes, .. } = &p.tracks[0].clips[0].content else {
            panic!("a MIDI clip");
        };
        assert_eq!(notes, &vec![note(0, 2, 60), note(0, 4, 67), note(2, 1, 60)]);
    }

    /// Places `take` on track 1 as a Replace or an Overdub with the take clip
    /// id 50.
    fn put(p: &mut Project, base: &Project, take: &Take, mode: Mode) -> Result<Vec<Id>, Blocked> {
        let placement = Placement {
            mode,
            punch: None,
            take_clip: Id(50),
        };
        place(p, base, Id(1), take, placement, &mut ids())
    }

    /// `(id, start, length)` of track 1's clips.
    fn spans(p: &Project) -> Vec<(Id, Ticks, Ticks)> {
        p.tracks[0]
            .clips
            .iter()
            .map(|c| (c.id, c.start, c.length))
            .collect()
    }

    fn clip_of(p: &Project, id: u64) -> &Clip {
        p.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == Id(id))
            .map(|c| c.as_ref())
            .expect("the clip is in the project")
    }

    #[test]
    fn replace_puts_one_clip_over_the_range_with_its_notes_relative_to_it() {
        let base = base(vec![]);
        let mut p = base.clone();
        let mut t = take(
            4,
            8,
            vec![tn(5, 1, 60), tn(4, 2, 62), tn(4, 1, 50), tn(9, 1, 70)],
        );
        t.events = vec![ev(6, 10), ev(4, 20), ev(9, 30)];
        assert_eq!(put(&mut p, &base, &t, Mode::Replace), Ok(vec![Id(50)]));
        assert_eq!(spans(&p), [(Id(50), q(4), q(4))]);
        assert_eq!(clip_of(&p, 50).name, "MIDI clip");
        assert_eq!(
            clip_of(&p, 50).content,
            ClipContent::Midi {
                notes: vec![note(0, 1, 50), note(0, 2, 62), note(1, 1, 60)],
                events: vec![ev_at(0, 20), ev_at(2, 10)],
            }
        );
        assert_eq!(p.tracks[1], base.tracks[1], "the other track is untouched");
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn replace_cuts_the_clip_it_covers_and_keeps_both_outside_parts() {
        let base = base(vec![clip(10, 0, 8, vec![note(1, 1, 60), note(5, 1, 64)])]);
        let mut p = base.clone();
        put(
            &mut p,
            &base,
            &take(2, 4, vec![tn(3, 1, 62)]),
            Mode::Replace,
        )
        .unwrap();
        assert_eq!(
            spans(&p),
            [
                (Id(10), q(0), q(2)),
                (Id(50), q(2), q(2)),
                (Id(900), q(4), q(4)),
            ]
        );
        // The pieces of the old clip are windows onto all its notes (format
        // v15): each shows its own, and keeps the other's hidden.
        assert_eq!(
            clip_of(&p, 10).content,
            midi(vec![note(1, 1, 60), note(5, 1, 64)])
        );
        assert_eq!(shown_notes(clip_of(&p, 10)), vec![0]);
        assert_eq!(clip_of(&p, 50).content, midi(vec![note(1, 1, 62)]));
        assert_eq!(
            clip_of(&p, 900).content,
            midi(vec![note(-3, 1, 60), note(1, 1, 64)])
        );
        assert_eq!(shown_notes(clip_of(&p, 900)), vec![1]);
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn overdub_begun_at_a_clips_start_is_a_new_clip_and_the_rest_stays_its_own() {
        // The owner's case: begun exactly at a clip's start, the take is a
        // new clip carrying what the old one played there, and the rest of
        // the old clip stays a clip of its own after it.
        let base = base(vec![clip(10, 0, 8, vec![note(0, 1, 48), note(5, 1, 50)])]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(0, 4, vec![tn(3, 1, 64)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(50)]));
        assert_eq!(spans(&p), [(Id(50), q(0), q(4)), (Id(10), q(4), q(4))]);
        let new = clip_of(&p, 50);
        assert_eq!(new.content, midi(vec![note(0, 1, 48), note(3, 1, 64)]));
        assert_eq!(new.name, "c10", "named after the clip it began in");
        assert_eq!(shown_notes(clip_of(&p, 10)), vec![1]);
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn overdub_begun_inside_a_clip_is_a_new_clip_from_there_to_the_stop() {
        let old = Clip {
            color: Some(Color(3)),
            ..clip(
                10,
                0,
                8,
                vec![note(0, 1, 48), note(2, 1, 50), note(5, 1, 52)],
            )
        };
        let base = base(vec![old]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(2, 4, vec![tn(3, 1, 64)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(50)]));
        assert_eq!(
            spans(&p),
            [
                (Id(10), q(0), q(2)),
                (Id(50), q(2), q(2)),
                (Id(900), q(4), q(4)),
            ]
        );
        // The new clip carries what the old one played there, and the take.
        let new = clip_of(&p, 50);
        assert_eq!(new.content, midi(vec![note(0, 1, 50), note(1, 1, 64)]));
        assert_eq!((new.name.as_str(), new.color), ("c10", Some(Color(3))));
        // What it covers is hidden in the pieces, not lost.
        assert_eq!(shown_notes(clip_of(&p, 10)), vec![0]);
        assert_eq!(shown_notes(clip_of(&p, 900)), vec![2]);
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn overdub_begun_inside_cuts_a_note_held_past_the_stop() {
        // A held note runs past the end of the take; the clip goes on.
        let base = base(vec![clip(10, 0, 16, vec![])]);
        let mut p = base.clone();
        put(
            &mut p,
            &base,
            &take(4, 8, vec![tn(7, 3, 60)]),
            Mode::Overdub,
        )
        .unwrap();
        assert_eq!(clip_of(&p, 50).content, midi(vec![note(3, 1, 60)]));
        assert_eq!(spans(&p)[1], (Id(50), q(4), q(4)));
    }

    #[test]
    fn overdub_begun_over_a_sounding_note_carries_it_on_whole() {
        // A note from 2 to 10 sounds when the take begins at 4 (the owner's
        // boundary case): the new clip keeps it whole, starting before it,
        // so it carries on through both seams instead of stopping at 4.
        let base = base(vec![clip(10, 0, 12, vec![note(2, 8, 60)])]);
        let mut p = base.clone();
        put(&mut p, &base, &take(4, 6, vec![]), Mode::Overdub).unwrap();
        assert_eq!(
            spans(&p),
            [
                (Id(10), q(0), q(4)),
                (Id(50), q(4), q(2)),
                (Id(900), q(6), q(6)),
            ]
        );
        assert_eq!(clip_of(&p, 50).content, midi(vec![note(-2, 8, 60)]));
        assert_eq!(
            crate::edit::seams::carried(&p.tracks[0]),
            vec![vec![], vec![0], vec![0]],
            "the note sounds on through the new clip and the rest"
        );
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn overdub_begun_inside_keeps_carried_notes_the_same_as_the_take_grows() {
        // A note that runs past the take's end is not cut at it, so each
        // commit leaves it as it was and the engine does not strike it again.
        let base = base(vec![clip(10, 0, 16, vec![note(2, 8, 60), note(5, 6, 62)])]);
        let carried = |end: i64| {
            let mut p = base.clone();
            put(&mut p, &base, &take(4, end, vec![]), Mode::Overdub).unwrap();
            clip_of(&p, 50).content.clone()
        };
        let first = carried(6);
        assert_eq!(first, midi(vec![note(-2, 8, 60), note(1, 6, 62)]));
        assert_eq!(carried(9), first);
        assert_eq!(carried(12), first);
    }

    #[test]
    fn a_note_played_over_a_carried_one_ends_it_there() {
        let base = base(vec![clip(10, 0, 12, vec![note(2, 8, 60)])]);
        let mut p = base.clone();
        put(
            &mut p,
            &base,
            &take(4, 6, vec![tn(5, 1, 60)]),
            Mode::Overdub,
        )
        .unwrap();
        assert_eq!(
            clip_of(&p, 50).content,
            midi(vec![note(-2, 3, 60), note(1, 1, 60)]),
            "the played note wins its pitch from 5"
        );
        // Cut short, the old note no longer reaches the rest of the clip.
        assert_eq!(
            crate::edit::seams::carried(&p.tracks[0]),
            vec![vec![], vec![0], vec![]]
        );
    }

    #[test]
    fn an_empty_overdub_over_empty_lane_shows_its_clip_at_once() {
        // Nothing played yet: the clip is there from the record start.
        let base = base(vec![clip(10, 0, 2, vec![])]);
        for end in [5, 8] {
            let mut p = base.clone();
            let placed = put(&mut p, &base, &take(4, end, vec![]), Mode::Overdub);
            assert_eq!(placed, Ok(vec![Id(50)]));
            assert_eq!(spans(&p)[1], (Id(50), q(4), q(end - 4)));
            assert_eq!(clip_of(&p, 50).content, midi(vec![]));
        }
    }

    #[test]
    fn overdub_inside_a_new_clip_skips_what_a_muted_clip_holds() {
        let muted = Clip {
            muted: true,
            ..clip(10, 0, 8, vec![note(3, 1, 48)])
        };
        let base = base(vec![muted]);
        let mut p = base.clone();
        put(
            &mut p,
            &base,
            &take(2, 4, vec![tn(2, 1, 64)]),
            Mode::Overdub,
        )
        .unwrap();
        assert_eq!(clip_of(&p, 50).content, midi(vec![note(0, 1, 64)]));
    }

    #[test]
    fn overdub_begun_before_a_clip_and_reaching_it_is_a_new_clip_beside_it() {
        // The owner's case: begun before a clip and running into it, the
        // take is a new clip from the record start, carrying the old notes
        // it reaches; the old clip keeps the rest as a clip of its own and
        // does not move or grow.
        let base = base(vec![clip(10, 4, 4, vec![note(0, 1, 48), note(3, 1, 50)])]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(2, 6, vec![tn(2, 1, 60), tn(5, 1, 64)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(50)]));
        assert_eq!(spans(&p), [(Id(50), q(2), q(4)), (Id(10), q(6), q(2))]);
        let new = clip_of(&p, 50);
        assert_eq!(
            new.content,
            midi(vec![note(0, 1, 60), note(2, 1, 48), note(3, 1, 64)]),
            "the old note still sounds at 4"
        );
        assert_eq!(new.name, "MIDI clip");
        assert_eq!(shown_notes(clip_of(&p, 10)), vec![1]);
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn overdub_stopped_before_a_clip_leaves_it_alone() {
        let base = base(vec![clip(10, 4, 4, vec![note(0, 1, 48)])]);
        for end in [3, 4] {
            let mut p = base.clone();
            let t = take(1, end, vec![tn(2, 1, 60)]);
            assert_eq!(put(&mut p, &base, &t, Mode::Overdub), Ok(vec![Id(50)]));
            assert_eq!(
                spans(&p),
                [(Id(50), q(1), q(end - 1)), (Id(10), q(4), q(4))]
            );
            assert_eq!(clip_of(&p, 10), &*base.tracks[0].clips[0]);
        }
    }

    #[test]
    fn overdub_begun_at_a_clips_end_is_a_new_clip_after_it() {
        let base = base(vec![clip(10, 0, 2, vec![note(0, 1, 48)])]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(2, 4, vec![tn(3, 1, 60)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(50)]));
        assert_eq!(spans(&p), [(Id(10), q(0), q(2)), (Id(50), q(2), q(2))]);
        assert_eq!(clip_of(&p, 10), &*base.tracks[0].clips[0]);
        assert_eq!(clip_of(&p, 50).content, midi(vec![note(1, 1, 60)]));
    }

    #[test]
    fn overdub_over_whole_clips_carries_what_they_played_into_one_clip() {
        // Clips the take covers end to end are recorded over, as in Replace,
        // but what they played is in the new clip.
        let base = base(vec![
            clip(10, 0, 2, vec![note(1, 1, 48)]),
            clip(11, 4, 2, vec![note(0, 1, 50)]),
        ]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(0, 8, vec![tn(3, 1, 60), tn(7, 1, 62)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(50)]));
        assert_eq!(spans(&p), [(Id(50), q(0), q(8))]);
        assert_eq!(
            clip_of(&p, 50).content,
            midi(vec![
                note(1, 1, 48),
                note(3, 1, 60),
                note(4, 1, 50),
                note(7, 1, 62)
            ])
        );
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn overdub_over_empty_lane_creates_a_clip_and_fills_only_what_is_covered() {
        // The take begins after the clip has ended: nothing borders it.
        let base = base(vec![clip(10, 0, 2, vec![])]);
        let mut p = base.clone();
        let mut t = take(3, 6, vec![tn(3, 1, 60), tn(7, 1, 62)]);
        t.events = vec![ev(4, 7)];
        let placed = put(&mut p, &base, &t, Mode::Overdub);
        assert_eq!(placed, Ok(vec![Id(50)]));
        assert_eq!(spans(&p), [(Id(10), q(0), q(2)), (Id(50), q(3), q(3))]);
        assert_eq!(
            clip_of(&p, 50).content,
            ClipContent::Midi {
                notes: vec![note(0, 1, 60)],
                events: vec![ev_at(1, 7)],
            }
        );
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn overdub_before_a_looping_clip_leaves_its_loop_alone() {
        let looped = Clip {
            loop_length: Some(q(2)),
            ..clip(10, 4, 4, vec![note(0, 1, 48)])
        };
        let base = base(vec![looped]);
        let mut p = base.clone();
        put(
            &mut p,
            &base,
            &take(2, 4, vec![tn(2, 1, 60)]),
            Mode::Overdub,
        )
        .unwrap();
        assert_eq!(spans(&p), [(Id(50), q(2), q(2)), (Id(10), q(4), q(4))]);
        assert_eq!(clip_of(&p, 10), &*base.tracks[0].clips[0]);
    }

    #[test]
    fn overdub_in_a_looping_clip_is_a_new_clip_carrying_its_repeats() {
        let looped = Clip {
            loop_length: Some(q(2)),
            ..clip(10, 0, 8, vec![note(1, 1, 48)])
        };
        let base = base(vec![looped]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(4, 6, vec![tn(4, 3, 60), tn(5, 1, 67)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(50)]));
        let at_50 = spans(&p).iter().position(|s| s.0 == Id(50)).unwrap();
        assert_eq!(spans(&p)[at_50], (Id(50), q(4), q(2)));
        assert_eq!(spans(&p).len(), 3, "the loop before and after it stays");
        // The repeat at 4 played its note at 5; the long note is cut at the
        // take's end.
        assert_eq!(
            clip_of(&p, 50).content,
            midi(vec![note(0, 2, 60), note(1, 1, 48), note(1, 1, 67)])
        );
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn punch_clips_the_take_and_drops_what_starts_outside_it() {
        let base = base(vec![]);
        let mut p = base.clone();
        let t = take(0, 8, vec![tn(1, 1, 60), tn(2, 4, 62), tn(5, 1, 64)]);
        let placed = place(
            &mut p,
            &base,
            Id(1),
            &t,
            Placement {
                mode: Mode::Replace,
                punch: Some((q(2), q(4))),
                take_clip: Id(50),
            },
            &mut ids(),
        );
        assert_eq!(placed, Ok(vec![Id(50)]));
        assert_eq!(spans(&p), [(Id(50), q(2), q(2))]);
        // The note that started in the punch is cut at its end.
        assert_eq!(clip_of(&p, 50).content, midi(vec![note(0, 2, 62)]));

        let mut away = base.clone();
        let none = place(
            &mut away,
            &base,
            Id(1),
            &t,
            Placement {
                mode: Mode::Replace,
                punch: Some((q(10), q(12))),
                take_clip: Id(50),
            },
            &mut ids(),
        );
        assert_eq!(none, Ok(vec![]));
        assert_eq!(away.tracks[0], base.tracks[0]);
    }

    #[test]
    fn lengths_are_at_least_the_shortest_note_unless_the_range_cuts_them() {
        let base = base(vec![]);
        let mut p = base.clone();
        let short = TakeNote {
            start: q(1),
            length: Ticks(1),
            pitch: 60,
            velocity: 100,
        };
        let at_end = TakeNote {
            start: q(4) - Ticks(1),
            length: q(1),
            pitch: 61,
            velocity: 100,
        };
        put(
            &mut p,
            &base,
            &take(0, 4, vec![short, at_end]),
            Mode::Replace,
        )
        .unwrap();
        assert_eq!(
            clip_of(&p, 50).content,
            midi(vec![
                Note::new(q(1), MIN_NOTE, 60, 100),
                Note::new(q(4) - Ticks(1), Ticks(1), 61, 100),
            ])
        );
    }

    #[test]
    fn calls_with_a_growing_take_match_one_call_with_the_final_take() {
        // Begun inside a clip, and (Overdub) before one.
        let cases = [(Mode::Replace, 0), (Mode::Overdub, 0), (Mode::Overdub, 4)];
        for (mode, at) in cases {
            let base = base(vec![clip(10, at, 8, vec![note(0, 1, 48)])]);
            let takes = [
                take(2, 3, vec![tn(2, 1, 60)]),
                take(2, 5, vec![tn(2, 1, 60), tn(3, 2, 62)]),
                take(2, 6, vec![tn(2, 1, 60), tn(3, 2, 62), tn(5, 1, 64)]),
            ];
            let mut p = base.clone();
            for t in &takes {
                put(&mut p, &base, t, mode).unwrap();
            }
            let mut once = base.clone();
            put(&mut once, &base, &takes[2], mode).unwrap();
            assert_eq!(p, once, "{mode:?} over a clip at {at}");
        }
    }

    #[test]
    fn a_locked_clip_in_the_range_blocks_the_take_and_the_track_is_as_base() {
        let locked = Clip {
            locked: true,
            ..clip(10, 0, 2, vec![note(0, 1, 48)])
        };
        let base = base(vec![locked, clip(11, 4, 2, vec![])]);
        for mode in [Mode::Replace, Mode::Overdub] {
            let mut p = base.clone();
            assert_eq!(
                put(&mut p, &base, &take(1, 3, vec![tn(1, 1, 60)]), mode),
                Err(Blocked)
            );
            assert_eq!(p, base, "{mode:?}");
        }
        // A take that placed something first is put back to base when blocked.
        let mut p = base.clone();
        assert_eq!(
            put(
                &mut p,
                &base,
                &take(2, 4, vec![tn(2, 1, 60)]),
                Mode::Replace
            ),
            Ok(vec![Id(50)])
        );
        assert_eq!(
            put(
                &mut p,
                &base,
                &take(1, 3, vec![tn(1, 1, 60)]),
                Mode::Replace
            ),
            Err(Blocked)
        );
        assert_eq!(p.tracks[0], base.tracks[0]);
    }

    #[test]
    fn empty_takes_and_ranges_change_only_what_they_must() {
        let base = base(vec![clip(10, 0, 8, vec![note(1, 1, 48)])]);
        // An empty take from the clip's start is a new clip there, holding
        // what the old one played.
        let mut p = base.clone();
        assert_eq!(
            put(&mut p, &base, &take(0, 4, vec![]), Mode::Overdub),
            Ok(vec![Id(50)])
        );
        assert_eq!(clip_of(&p, 50).content, midi(vec![note(1, 1, 48)]));
        // Begun inside the clip too.
        let mut p = base.clone();
        assert_eq!(
            put(&mut p, &base, &take(1, 4, vec![]), Mode::Overdub),
            Ok(vec![Id(50)])
        );
        assert_eq!(clip_of(&p, 50).content, midi(vec![note(0, 1, 48)]));
        // An empty take still replaces what is under a non-empty range.
        let mut p = base.clone();
        assert_eq!(
            put(&mut p, &base, &take(2, 4, vec![]), Mode::Replace),
            Ok(vec![Id(50)])
        );
        assert_eq!(clip_of(&p, 50).content, midi(vec![]));
        assert_eq!(clip_of(&p, 50).length, q(2));

        // An empty range puts the track back as base has it.
        let mut dirty = base.clone();
        dirty.tracks[0] = Arc::new(Track::new(Id(1), "x", TrackKind::Midi, Color(0)));
        let t = take(3, 3, vec![tn(3, 1, 60)]);
        assert_eq!(put(&mut dirty, &base, &t, Mode::Replace), Ok(vec![]));
        assert_eq!(dirty.tracks[0], base.tracks[0]);

        // An audio track takes nothing and is put back as base has it.
        let mut audio_base = base.clone();
        Arc::make_mut(&mut audio_base.tracks[0]).kind = TrackKind::Audio;
        let mut p = audio_base.clone();
        p.tracks[0] = Arc::new(Track::new(Id(1), "x", TrackKind::Audio, Color(0)));
        let t = take(0, 4, vec![tn(0, 1, 60)]);
        assert_eq!(put(&mut p, &audio_base, &t, Mode::Replace), Ok(vec![]));
        assert_eq!(p.tracks[0], audio_base.tracks[0]);

        // A track base does not have changes nothing.
        let mut p = base.clone();
        let placed = place(
            &mut p,
            &base,
            Id(77),
            &t,
            Placement {
                mode: Mode::Replace,
                punch: None,
                take_clip: Id(50),
            },
            &mut ids(),
        );
        assert_eq!(placed, Ok(vec![]));
        assert_eq!(p, base);
    }
}
