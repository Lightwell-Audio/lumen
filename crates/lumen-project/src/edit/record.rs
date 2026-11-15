use std::sync::Arc;

use lumen_core::time::Ticks;

use super::notes::MIN_NOTE;
use super::{in_window, overwrite, repeats, shown, takes, to_arrangement};
use crate::model::{
    Clip, ClipContent, ClipEvent, EventKind, Id, Note, Pass, Project, Track, TrackKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Replace,
    Overdub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeNote {
    pub start: Ticks,
    pub length: Ticks,
    pub pitch: u8,
    pub velocity: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeEvent {
    pub at: Ticks,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Take {
    pub start: Ticks,
    pub end: Ticks,
    pub notes: Vec<TakeNote>,
    pub events: Vec<TakeEvent>,
    pub passes: Vec<PassTake>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PassTake {
    pub notes: Vec<TakeNote>,
    pub events: Vec<TakeEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub mode: Mode,
    pub punch: Option<(Ticks, Ticks)>,
    pub take_clip: Id,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blocked;

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
        takes: None,
        warp: None,
        content: ClipContent::Midi { notes, events },
    }
}

type Content = (Vec<Note>, Vec<ClipEvent>);

fn replace(
    project: &mut Project,
    track: Id,
    (a, b): (Ticks, Ticks),
    take: &Take,
    take_clip: Id,
    fresh: &mut impl FnMut() -> Id,
) -> Vec<Id> {
    let content = |take: &Take| {
        let (active_notes, hidden_notes, events) = capture_parts(take, a, b - a);
        let mut notes = settle(Vec::new(), active_notes);
        notes.extend(hidden_notes);
        notes.sort_by_key(|n| (n.start(), n.pitch()));
        (notes, events)
    };
    let mut clip;
    if take.passes.len() >= 2 {
        let passes: Vec<Pass> = take
            .passes
            .iter()
            .map(|pass| {
                let (notes, events) = content(&Take {
                    start: take.start,
                    end: take.end,
                    notes: pass.notes.clone(),
                    events: pass.events.clone(),
                    passes: Vec::new(),
                });
                Pass::Midi { notes, events }
            })
            .collect();
        let last = passes.len() - 1;
        clip = new_clip(take_clip, a, b - a, (Vec::new(), Vec::new()));
        takes::with_passes(&mut clip, passes, last);
    } else {
        clip = new_clip(take_clip, a, b - a, content(take));
    }
    add_clips(project, track, vec![clip]);
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

fn begun_in(track: &Track, t: Ticks) -> Option<&Clip> {
    track
        .clips
        .iter()
        .map(|c| &**c)
        .find(|c| is_midi(c) && c.start <= t && t < c.start + c.length)
}

fn heard_in(track: &Track, (a, b): (Ticks, Ticks)) -> Take {
    let mut heard = Take {
        start: a,
        end: b,
        notes: Vec::new(),
        events: Vec::new(),
        passes: Vec::new(),
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

fn fill(take: &Take, (from, to): (Ticks, Ticks), start: Ticks, period: Ticks) -> Content {
    let period = period.max(Ticks(1));
    let notes: Vec<Note> = take
        .notes
        .iter()
        .filter(|n| (from..to).contains(&n.start))
        .map(|n| {
            let at = relative(n.start, start, period);
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

fn relative(t: Ticks, start: Ticks, period: Ticks) -> Ticks {
    Ticks((t - start).0.rem_euclid(period.0))
}

#[cfg(test)]
mod tests;
