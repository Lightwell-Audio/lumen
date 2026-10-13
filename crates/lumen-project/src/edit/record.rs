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
//! - [`Mode::Overdub`]: what happens depends on where the range begins.
//!   - **Inside a MIDI clip** (after its start, before its end; a clip that
//!     does not loop): the take is a new clip from there to where it ends,
//!     as in Replace, but the new clip carries the notes and events the
//!     clips under it played there, so nothing heard is lost. It takes the
//!     name and colour of the clip it began in.
//!   - **Otherwise** (at or before a clip's start, or over empty lane): the
//!     take merges into the MIDI clips under the range, and those clips grow
//!     to hold it instead of leaving pieces beside them. The part of the
//!     range after a MIDI clip extends that clip's end; the part before the
//!     first clip extends its start back (a clip that does not loop). A part
//!     no MIDI clip borders (empty lane, or beside an audio clip) gets a new
//!     clip when it holds a note or event. Audio clips count as covered, so
//!     a take over one is dropped there.
//!
//!   A note played over an old one of the same pitch wins it, as in Ableton:
//!   the old note is cut where the new one starts, or dropped when it starts
//!   under it (`settle`).
//!
//! A looping clip takes the take at its place in the loop, so every pass
//! layers into the same loop (beginning inside one merges too).

use std::sync::Arc;

use lumen_core::time::Ticks;

use super::notes::MIN_NOTE;
use super::{
    content_length, in_window, overwrite, repeats, shift_events, shift_notes, shown, to_arrangement,
};
use crate::model::{Clip, ClipContent, ClipEvent, EventKind, Id, Note, Project, Track, TrackKind};

/// How a take lands on the clips under its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A new clip over the range; what is under it is cut away.
    Replace,
    /// Merged into the MIDI clips under the range, and a new clip where no
    /// clip is.
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
/// of the clip a Replace takes (or the first gap clip of an Overdub).
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
/// `fresh` gives ids for the parts of clips a Replace cuts in two, and for
/// further gap clips. Calling it again with the same `base` and `take_clip`
/// and a longer take gives what one call with the final take would.
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
    range: (Ticks, Ticks),
    take: &Take,
    take_clip: Id,
    fresh: &mut impl FnMut() -> Id,
) -> Vec<Id> {
    if let Some(begun_in) = begun_inside(source, range.0) {
        let dub = Dub {
            track,
            source,
            range,
            take,
            take_clip,
        };
        return overdub_from_inside(project, &dub, begun_in, fresh);
    }
    let (source, mut ids) = extended(source, range);
    if let Some(t) = track_mut(project, track) {
        t.clips.clone_from(&source.clips);
    }
    for id in merge_into_clips(project, track, &source, range, take) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut first = Some(take_clip);
    let mut created = Vec::new();
    for (from, to) in gaps(&source, range) {
        let content = fill(take, (from, to), from, to - from);
        if content.0.is_empty() && content.1.is_empty() {
            continue;
        }
        let id = match first.take() {
            Some(id) => id,
            None => fresh(),
        };
        ids.push(id);
        created.push(new_clip(id, from, to - from, content));
    }
    add_clips(project, track, created);
    ids
}

/// Where a take goes: its track (as `base` has it), the range and the id
/// of the clip it makes.
#[derive(Clone, Copy)]
struct Dub<'a> {
    track: Id,
    source: &'a Track,
    range: (Ticks, Ticks),
    take: &'a Take,
    take_clip: Id,
}

/// The MIDI clip that does not loop and has `t` strictly inside it, if any:
/// an Overdub begun there is a new clip.
fn begun_inside(track: &Track, t: Ticks) -> Option<&Clip> {
    track
        .clips
        .iter()
        .map(|c| &**c)
        .find(|c| is_midi(c) && c.loop_length.is_none() && c.start < t && t < c.start + c.length)
}

/// An Overdub begun inside `begun_in`: a new clip over the range holding
/// what the clips under it played there and the take, the new notes winning
/// their pitches. What it covers is cut away, as a Replace's clip does (the
/// covered notes stay hidden in the pieces, format v15).
fn overdub_from_inside(
    project: &mut Project,
    dub: &Dub<'_>,
    begun_in: &Clip,
    fresh: &mut impl FnMut() -> Id,
) -> Vec<Id> {
    let Dub {
        track,
        source,
        range: (a, b),
        take,
        take_clip,
    } = *dub;
    let heard = heard_in(source, (a, b));
    let (old_notes, mut events) = fill(&heard, (a, b), a, b - a);
    let (new_notes, new_events) = fill(take, (a, b), a, b - a);
    events.extend(new_events);
    events.sort_by_key(|e| e.at);
    let mut clip = new_clip(take_clip, a, b - a, (settle(old_notes, new_notes), events));
    clip.name.clone_from(&begun_in.name);
    clip.color = begun_in.color;
    add_clips(project, track, vec![clip]);
    overwrite(project, &[take_clip], fresh);
    vec![take_clip]
}

/// What the MIDI clips of `track` that are not muted play in `[a, b)`: the
/// notes and events in their windows, at project time, every repeat of a
/// looping clip, each note as long as it plays.
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
                .filter(|n| in_window(clip, n.start()) && n.start() < room)
            {
                let start = at(n.start());
                if (a..b).contains(&start) {
                    heard.notes.push(TakeNote {
                        start,
                        length: n.length().min(room - n.start()),
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

/// `track` with its MIDI clips grown over the parts of `range` beside them
/// that no clip covers (Overdub begun at or before a clip): a part after a
/// clip extends its end, a part before a clip that does not loop moves its
/// start back, its notes staying where they sound. Returns the track and
/// the ids of the clips grown.
fn extended(track: &Track, range: (Ticks, Ticks)) -> (Track, Vec<Id>) {
    let mut out = track.clone();
    let mut ids = Vec::new();
    for (from, to) in gaps(track, range) {
        let before = out
            .clips
            .iter_mut()
            .find(|c| is_midi(c) && c.start + c.length == from && overlaps(c, range));
        if let Some(clip) = before {
            let clip = Arc::make_mut(clip);
            clip.length = to - clip.start;
            ids.push(clip.id);
            continue;
        }
        let after = out
            .clips
            .iter_mut()
            .find(|c| is_midi(c) && c.loop_length.is_none() && c.start == to && overlaps(c, range));
        if let Some(clip) = after {
            let clip = Arc::make_mut(clip);
            let by = clip.start - from;
            clip.start = from;
            clip.length = clip.length + by;
            if let ClipContent::Midi { notes, events } = &mut clip.content {
                // Later in clip time by as much as the start moved back.
                shift_notes(notes, Ticks::ZERO - by);
                shift_events(events, Ticks::ZERO - by);
            }
            ids.push(clip.id);
        }
    }
    out.clips.sort_by_key(|c| c.start);
    (out, ids)
}

/// Merges the take into each MIDI clip under the range that it has notes or
/// events for. Returns the ids of the clips changed.
fn merge_into_clips(
    project: &mut Project,
    track: Id,
    source: &Track,
    range: (Ticks, Ticks),
    take: &Take,
) -> Vec<Id> {
    let edits: Vec<(Id, Content)> = source
        .clips
        .iter()
        .filter(|c| is_midi(c) && overlaps(c, range))
        .filter_map(|c| {
            let within = (c.start.max(range.0), (c.start + c.length).min(range.1));
            let content = fill(take, within, c.start, content_length(c));
            (!content.0.is_empty() || !content.1.is_empty()).then_some((c.id, content))
        })
        .collect();
    let Some(t) = track_mut(project, track) else {
        return Vec::new();
    };
    edits
        .into_iter()
        .filter_map(|(id, content)| {
            let clip = Arc::make_mut(t.clips.iter_mut().find(|c| c.id == id)?);
            merge_content(clip, content);
            Some(id)
        })
        .collect()
}

/// Adds `(notes, events)` to a MIDI clip's content, keeping both in order.
fn merge_content(clip: &mut Clip, (notes, events): Content) {
    if let ClipContent::Midi {
        notes: all_notes,
        events: all_events,
    } = &mut clip.content
    {
        *all_notes = settle(std::mem::take(all_notes), notes);
        all_events.extend(events);
        all_events.sort_by_key(|e| e.at);
    }
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

/// The parts of `range` that no clip of `track` covers, in time order.
pub fn gaps(track: &Track, range: (Ticks, Ticks)) -> Vec<(Ticks, Ticks)> {
    let (a, b) = range;
    let mut covered: Vec<(Ticks, Ticks)> = track
        .clips
        .iter()
        .filter(|c| overlaps(c, range))
        .map(|c| (c.start.max(a), (c.start + c.length).min(b)))
        .collect();
    covered.sort_by_key(|&(from, _)| from);
    let mut gaps = Vec::new();
    let mut cursor = a;
    for (from, to) in covered {
        if from > cursor {
            gaps.push((cursor, from));
        }
        cursor = cursor.max(to);
    }
    if cursor < b {
        gaps.push((cursor, b));
    }
    gaps
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
    fn overdub_begun_at_a_clips_start_merges_into_it_and_keeps_its_notes() {
        let base = base(vec![clip(10, 0, 8, vec![note(0, 1, 48)])]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(0, 4, vec![tn(3, 1, 64)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(10)]));
        assert_eq!(spans(&p), [(Id(10), q(0), q(8))]);
        assert_eq!(
            clip_of(&p, 10).content,
            midi(vec![note(0, 1, 48), note(3, 1, 64)])
        );
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
    fn overdub_begun_before_a_clip_moves_its_start_back_and_its_notes_stay() {
        let base = base(vec![clip(10, 4, 4, vec![note(0, 1, 48)])]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(2, 6, vec![tn(2, 1, 60), tn(5, 1, 64)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(10)]));
        assert_eq!(spans(&p), [(Id(10), q(2), q(6))], "one clip, grown");
        assert_eq!(
            clip_of(&p, 10).content,
            midi(vec![note(0, 1, 60), note(2, 1, 48), note(3, 1, 64)]),
            "the old note still sounds at 4"
        );
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn overdub_past_a_clips_end_extends_it_and_a_gap_between_clips_joins_the_first() {
        let base = base(vec![clip(10, 0, 2, vec![]), clip(11, 4, 2, vec![])]);
        let mut p = base.clone();
        let placed = put(
            &mut p,
            &base,
            &take(0, 8, vec![tn(3, 1, 60), tn(7, 1, 62)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(10), Id(11)]));
        assert_eq!(spans(&p), [(Id(10), q(0), q(4)), (Id(11), q(4), q(4))]);
        assert_eq!(clip_of(&p, 10).content, midi(vec![note(3, 1, 60)]));
        assert_eq!(clip_of(&p, 11).content, midi(vec![note(3, 1, 62)]));
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
    fn overdub_before_a_looping_clip_leaves_its_start_alone() {
        let looped = Clip {
            loop_length: Some(q(2)),
            ..clip(10, 4, 4, vec![])
        };
        let base = base(vec![looped]);
        let mut p = base.clone();
        put(
            &mut p,
            &base,
            &take(2, 5, vec![tn(2, 1, 60)]),
            Mode::Overdub,
        )
        .unwrap();
        assert_eq!(spans(&p), [(Id(50), q(2), q(2)), (Id(10), q(4), q(4))]);
    }

    #[test]
    fn overdub_in_a_looping_clip_wraps_each_pass_into_the_loop() {
        let looped = Clip {
            loop_length: Some(q(2)),
            ..clip(10, 0, 8, vec![])
        };
        let base = base(vec![looped]);
        let mut p = base.clone();
        // Four and five quarters are the loop's first and second beat.
        let placed = put(
            &mut p,
            &base,
            &take(4, 6, vec![tn(4, 3, 60), tn(5, 1, 67)]),
            Mode::Overdub,
        );
        assert_eq!(placed, Ok(vec![Id(10)]));
        assert_eq!(spans(&p), [(Id(10), q(0), q(8))]);
        assert_eq!(clip_of(&p, 10).loop_length, Some(q(2)));
        // The long note is cut at the loop's end, the second one wraps.
        assert_eq!(
            clip_of(&p, 10).content,
            midi(vec![note(0, 2, 60), note(1, 1, 67)])
        );
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
        // Begun inside a clip, and (Overdub) before one, which then grows.
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
        // An empty take from the clip's start merges nothing.
        let mut p = base.clone();
        assert_eq!(
            put(&mut p, &base, &take(0, 4, vec![]), Mode::Overdub),
            Ok(vec![])
        );
        assert_eq!(p, base);
        // Begun inside the clip it is still a new clip from there, holding
        // what the old one played.
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
