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
//! go into clips relative to each clip's start. The clip's visible window is
//! the intersection of the take range and punch range, while all captured
//! notes and events stay in its content, including those hidden before or
//! after that window. Their original lengths are kept for later trim recovery.
//! Notes shorter than [`MIN_NOTE`] are extended to that minimum.
//!
//! # Modes
//!
//! - [`Mode::Replace`]: the take is a new clip over the range. What is under
//!   it is cut away, as every clip put down is (`super::overwrite`).
//! - [`Mode::Overdub`]: also a new clip over the range, wherever it begins
//!   (inside a clip, at a clip's start, before one or over empty lane).
//!   What is under it is cut away, so
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
//!   A note played over an old one of the same pitch wins it:
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
/// The visible clip is cut to the placement's `punch` when given, while all
/// captured notes and events remain in its content for later trim recovery.
/// When capture is nonempty but its intersection with the punch is empty, a
/// one-tick clip is anchored at the nearest punch edge so the captured data
/// still has a home. An empty capture, a track that is not MIDI and a track
/// `base` does not have place nothing, and the track still becomes `base`'s.
/// A locked clip overlapping the range returns [`Blocked`], with the track
/// as `base` has it.
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
    // Only the track's clips go back to what they were when recording began,
    // for the take to be placed on them. Everything else on the track is the
    // user's to change while it records (a fader, an automation lane, a
    // plugin's knob) and stays as it is now.
    if let Some(slot) = project.tracks.iter_mut().find(|t| t.id == track)
        && slot.clips != source.clips
    {
        Arc::make_mut(slot).clips = source.clips.clone();
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

/// The visible range: the take from zero, cut to the punch. Because clips
/// require a positive length, nonempty capture outside the intersection gets
/// a one-tick hidden-content anchor at the nearest punch edge. For an
/// inverted loop take (the loop head has not reached its start), the anchor
/// is the loop start. This preserves the recorded data without making it
/// audible outside its active window.
fn range_of(take: &Take, punch: Option<(Ticks, Ticks)>) -> Option<(Ticks, Ticks)> {
    let (mut a, mut b) = (take.start.max(Ticks::ZERO), take.end);
    if let Some((from, to)) = punch {
        a = a.max(from);
        b = b.min(to);
    }
    if a < b {
        return Some((a, b));
    }
    if take.notes.is_empty() && take.events.is_empty() {
        return None;
    }
    let anchor = match punch {
        Some((from, _to)) if take.end <= from => from.max(Ticks::ZERO),
        Some((_, to)) if take.start >= to => (to - Ticks(1)).max(Ticks::ZERO),
        Some((from, _)) => from.max(Ticks::ZERO),
        None => take.start.max(Ticks::ZERO),
    };
    Some((anchor, anchor + Ticks(1)))
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
        crossfade: false,
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
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
    let (active_notes, hidden_notes, events) = capture_parts(take, a, b - a);
    let mut notes = settle(Vec::new(), active_notes);
    notes.extend(hidden_notes);
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    let content = (notes, events);
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
    let (active_notes, hidden_notes, mut new_events) = capture_parts(take, a, b - a);
    let mut notes = settle(old_notes, active_notes);
    notes.extend(hidden_notes);
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    events.append(&mut new_events);
    events.sort_by_key(|e| e.at);
    let mut clip = new_clip(take_clip, a, b - a, (notes, events));
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

/// `new` notes laid over `old` ones: a note played wins
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

/// Captured notes partitioned by start into the visible window and hidden
/// content, plus every event. The notes keep their captured lengths; callers
/// settle active notes using the rules of Replace or Overdub and retain hidden
/// notes untouched for trim recovery.
fn capture_parts(
    take: &Take,
    clip_start: Ticks,
    visible_length: Ticks,
) -> (Vec<Note>, Vec<Note>, Vec<ClipEvent>) {
    let notes: Vec<_> = take
        .notes
        .iter()
        .map(|n| {
            Note::new(
                n.start - clip_start,
                n.length.max(MIN_NOTE),
                n.pitch,
                n.velocity,
            )
        })
        .collect();
    let (mut active, mut hidden): (Vec<_>, Vec<_>) = notes
        .into_iter()
        .partition(|note| (Ticks::ZERO..visible_length).contains(&note.start()));
    for note in &mut hidden {
        *note = note.with_carries_on(false);
    }
    active.sort_by_key(|n| (n.start(), n.pitch()));
    hidden.sort_by_key(|n| (n.start(), n.pitch()));
    let mut events: Vec<_> = take
        .events
        .iter()
        .map(|e| ClipEvent {
            at: e.at - clip_start,
            kind: e.kind,
        })
        .collect();
    events.sort_by_key(|e| e.at);
    (active, hidden, events)
}

/// Where time `t` falls in a clip that starts at `start` and repeats every
/// `period`.
fn relative(t: Ticks, start: Ticks, period: Ticks) -> Ticks {
    Ticks((t - start).0.rem_euclid(period.0))
}

#[cfg(test)]
mod tests;
