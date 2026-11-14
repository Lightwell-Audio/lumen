//! A run as the notes and controllers the engine plays: cut where a note
//! stops, carried across a seam, and a looping clip's pattern read against its
//! own period.

use std::sync::Arc;

use lumen_core::midi::Msg;
use lumen_core::time::Ticks;
use lumen_engine::{MidiControl, MidiNote, control_list, note_list};
use lumen_project::edit::seams;
use lumen_project::model::{self, ClipContent, DrumMap, EventKind};

use super::voice::Voice;
use super::{Dest, Member, Suppression, content, loops, shows};

/// The notes a run plays, in ticks from its first clip's start, each cut
/// where it stops (see the module documentation). Unsorted.
pub(crate) fn run_notes(run: &[Member<'_>]) -> Vec<MidiNote> {
    run_notes_masked(run, None)
}

pub(super) fn run_notes_masked(
    run: &[Member<'_>],
    suppression: Option<Suppression>,
) -> Vec<MidiNote> {
    let Some(head) = run.first() else {
        return Vec::new();
    };
    let mut out: Vec<MidiNote> = Vec::new();
    // Notes sounding up to the end of the member before: arrangement start,
    // pitch, and where they are in `out`.
    let mut reaching: Vec<(Ticks, u8, usize)> = Vec::new();
    let mut carried: Vec<usize> = Vec::new();
    for (k, m) in run.iter().enumerate() {
        let offset = m.clip.start - head.clip.start;
        let (notes, _) = content(m.clip);
        let voice = Voice::new(m.map, m.dest);
        let mut next: Vec<(Ticks, u8, usize)> = Vec::new();
        carried = match k {
            0 => Vec::new(),
            _ => {
                let p = &run[k - 1];
                seams::carried_on(p.clip, p.length, &carried, m.clip)
            }
        };
        for &i in &carried {
            let n = notes[i];
            let Some((key, _channel, length)) = voice.played(n) else {
                continue;
            };
            let at = m.clip.start + n.start();
            if suppression.is_some_and(|s| s.hides(at)) {
                continue;
            }
            if let Some(&(_, _, e)) = reaching.iter().find(|r| (r.0, r.1) == (at, key)) {
                let mut end = (n.start() + length).min(m.length);
                if let Some(s) = suppression
                    .filter(|s| s.from < s.until && at < s.from && m.clip.start + end > s.from)
                {
                    end = end.min(s.from - m.clip.start);
                }
                out[e].length = offset + end - out[e].start;
                if length >= m.length - n.start() && end >= m.length {
                    next.push((at, key, e));
                }
            }
        }
        for n in notes.iter().filter(|n| !n.muted() && shows(m, n.start())) {
            let Some((key, channel, note_length)) = voice.played(*n) else {
                continue;
            };
            let arrangement_start = m.clip.start + n.start();
            if suppression.is_some_and(|s| s.hides(arrangement_start)) {
                continue;
            }
            let mut length = if loops(m) {
                note_length
            } else {
                note_length.min(m.length - n.start())
            };
            if !loops(m)
                && let Some(s) = suppression.filter(|s| {
                    s.from < s.until
                        && arrangement_start < s.from
                        && arrangement_start + length > s.from
                })
            {
                length = length.min(s.from - arrangement_start);
            }
            if !loops(m)
                && n.start() + note_length >= m.length
                && arrangement_start + length >= m.clip.start + m.length
            {
                next.push((m.clip.start + n.start(), key, out.len()));
            }
            out.push(MidiNote {
                start: offset + n.start(),
                length,
                pitch: key,
                velocity: n.velocity(),
                channel,
            });
        }
        reaching = next;
    }
    out
}

/// The controllers a run plays, in ticks from its first clip's start.
pub(super) fn run_controls(
    run: &[Member<'_>],
    suppression: Option<Suppression>,
) -> Vec<MidiControl> {
    let Some(head) = run.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for m in run {
        let offset = m.clip.start - head.clip.start;
        let (_, events) = content(m.clip);
        for e in events.iter().filter(|e| {
            shows(m, e.at) && !suppression.is_some_and(|s| s.hides(m.clip.start + e.at))
        }) {
            out.extend(control(e).into_iter().map(|mut c| {
                c.at = offset + c.at;
                c
            }));
        }
    }
    out
}

/// The first part of a looping clip after punch-out, ending at the next
/// original cycle boundary. Notes and controls are rotated to the clip's
/// phase at `from`; later cycles can keep the original periodic pattern.
pub(crate) fn loop_fragment(
    clip: &model::Clip,
    from: Ticks,
    length: Ticks,
    period: Ticks,
    map: Option<&DrumMap>,
    dest: Dest,
) -> (Vec<MidiNote>, Vec<MidiControl>) {
    let ClipContent::Midi { notes, events } = &clip.content else {
        return (Vec::new(), Vec::new());
    };
    let voice = Voice::new(map, dest);
    let phase = Ticks((clip.loop_phase.0 + from.0).rem_euclid(period.0));
    let mut played_notes = Vec::new();
    for note in notes
        .iter()
        .filter(|n| !n.muted() && n.start() >= Ticks::ZERO && n.start() < period)
    {
        let Some((key, channel, note_length)) = voice.played(*note) else {
            continue;
        };
        let source_end = (note.start() + note_length).min(period);
        if note.start() < phase && source_end > phase && length > Ticks::ZERO {
            played_notes.push(MidiNote {
                start: Ticks::ZERO,
                length: (source_end - phase).min(length),
                pitch: key,
                velocity: note.velocity(),
                channel,
            });
        }
        if note.start() < Ticks::ZERO || note.start() >= period {
            continue;
        }
        let at = Ticks((note.start().0 - phase.0).rem_euclid(period.0));
        if at >= length {
            continue;
        }
        let note_length = note_length.min(period - note.start()).min(length - at);
        played_notes.push(MidiNote {
            start: at,
            length: note_length,
            pitch: key,
            velocity: note.velocity(),
            channel,
        });
    }
    let mut played_controls = Vec::new();
    for event in events
        .iter()
        .filter(|e| e.at >= Ticks::ZERO && e.at < period)
    {
        if event.at < Ticks::ZERO || event.at >= period {
            continue;
        }
        let at = Ticks((event.at.0 - phase.0).rem_euclid(period.0));
        if at < length {
            played_controls.extend(control(event).into_iter().map(|mut c| {
                c.at = at;
                c
            }));
        }
    }
    (played_notes, played_controls)
}

/// A clip event as the messages that carry it (a program change with a
/// bank sends the bank first).
fn control(event: &model::ClipEvent) -> Vec<MidiControl> {
    let at = event.at;
    let msg = |m: Msg| MidiControl { at, msg: m };
    match event.kind {
        EventKind::Controller { number, value } => vec![msg(Msg::controller(0, number, value))],
        EventKind::PitchBend { value } => vec![msg(Msg::pitch_bend(0, value))],
        EventKind::Pressure { value } => vec![msg(Msg::pressure(0, value))],
        EventKind::Program { program, bank } => {
            let mut out = Vec::new();
            if let Some(bank) = bank {
                out.push(msg(Msg::controller(
                    0,
                    lumen_core::midi::cc::BANK_MSB,
                    (bank >> 7) as u8,
                )));
                out.push(msg(Msg::controller(
                    0,
                    lumen_core::midi::cc::BANK_LSB,
                    (bank & 127) as u8,
                )));
            }
            out.push(msg(Msg::program(0, program)));
            out
        }
    }
}

/// A split piece may be shorter than the source loop. Read the source pattern
/// against its original period, not that piece's timeline window.
pub(super) fn loop_pattern(
    clip: &model::Clip,
    period: Ticks,
    map: Option<&DrumMap>,
    dest: Dest,
) -> (Arc<[MidiNote]>, Arc<[MidiControl]>) {
    let ClipContent::Midi { notes, events } = &clip.content else {
        return (note_list(Vec::new()), control_list(Vec::new()));
    };
    let voice = Voice::new(map, dest);
    let notes = notes
        .iter()
        .filter(|n| !n.muted() && n.start() >= Ticks::ZERO && n.start() < period)
        .filter_map(|n| {
            let (pitch, channel, length) = voice.played(*n)?;
            Some(MidiNote {
                start: n.start(),
                length,
                pitch,
                velocity: n.velocity(),
                channel,
            })
        })
        .collect();
    let controls = events
        .iter()
        .filter(|e| e.at >= Ticks::ZERO && e.at < period)
        .flat_map(control)
        .collect();
    (note_list(notes), control_list(controls))
}
