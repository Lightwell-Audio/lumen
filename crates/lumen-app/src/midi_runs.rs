//! A MIDI track's clips as the engine plays them: in **runs** (ADR-0058).
//!
//! Clips that meet at a seam (one begins where the one before it stops,
//! neither loops: `edit::seams`) play as one engine clip, a run. A note
//! that runs past its clip's end is cut there unless the next clip of the
//! run carries it on, holding the same note; then it sounds on until it
//! ends or the run stops carrying it. Two things follow:
//!
//! - a note held across a clip cut in two, or across the start of an Overdub begun
//!   inside a clip, sustains instead of stopping at the seam;
//! - the engine knows a sounding note by where it began on the timeline and
//!   its key (`hold.rs`, ADR-0061), so while a recording moves the seams
//!   after it once a commit (about once a second), or a clip is resized,
//!   the notes playing are neither released nor struck again;
//! - each note still belongs to the clip it begins in: the run tells the
//!   engine where its clips meet (`MidiClip::seams`), so playing from a
//!   clip's start, or from inside it, does not strike a note carried on into
//!   it from the clip before; only playing through the seam sounds it.
//!
//! Lengths are worked out here, so each note reaches the engine already cut
//! where it stops (a looping clip, always a run of its own, keeps its notes
//! whole: the engine cuts them at each repeat). Only what is in a clip's
//! window plays (ADR-0056); muted notes play nothing.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::midi::Msg;
use lumen_core::time::Ticks;
use lumen_engine::{MidiClip, MidiControl, MidiNote, control_list, note_list};
use lumen_project::edit::seams;
use lumen_project::model::{self, ClipContent, EventKind, Id};

/// A clip in a run, and how much of it plays (less than its length when a
/// Replace take is recording over its end).
#[derive(Clone, Copy)]
pub(crate) struct Member<'a> {
    pub clip: &'a Arc<model::Clip>,
    pub length: Ticks,
}

/// Arrangement interval hidden while a bounded Replace take is recorded.
/// The take's clip stays audible so loop passes can hear their own notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Suppression {
    pub from: Ticks,
    pub until: Ticks,
    pub exempt: Option<Id>,
}

impl Suppression {
    fn hides(self, clip: &model::Clip, at: Ticks) -> bool {
        self.from < self.until && self.exempt != Some(clip.id) && self.from <= at && at < self.until
    }
}

/// Groups `members` (in start order) into runs: each next member that
/// meets the one before at a seam joins its run.
pub(crate) fn runs<'a>(members: impl IntoIterator<Item = Member<'a>>) -> Vec<Vec<Member<'a>>> {
    let mut out: Vec<Vec<Member<'a>>> = Vec::new();
    for m in members {
        match out.last_mut() {
            Some(run)
                if run.last().is_some_and(|p| {
                    seams::is_seam(p.clip, p.length, m.clip) || loop_seam(p, &m)
                }) =>
            {
                run.push(m);
            }
            _ => out.push(vec![m]),
        }
    }
    out
}

/// Adjacent pieces cut from one loop share a pattern, but their source
/// phase must advance by the amount of timeline each piece covers.
fn loop_seam(prev: &Member<'_>, next: &Member<'_>) -> bool {
    let Some(period) = prev.clip.loop_length.filter(|p| p.0 > 0) else {
        return false;
    };
    prev.clip.start + prev.length == next.clip.start
        && next.clip.loop_length == Some(period)
        && prev.clip.content == next.clip.content
        && (prev.clip.loop_phase.0 + prev.length.0).rem_euclid(period.0)
            == next.clip.loop_phase.0.rem_euclid(period.0)
}

pub(crate) fn loop_period(run: &[Member<'_>]) -> Option<Ticks> {
    let period = run.first()?.clip.loop_length.filter(|p| p.0 > 0)?;
    run.iter()
        .all(|m| m.clip.loop_length == Some(period))
        .then_some(period)
}

fn content(clip: &model::Clip) -> (&[model::Note], &[model::ClipEvent]) {
    match &clip.content {
        ClipContent::Midi { notes, events } => (notes, events),
        ClipContent::Audio { .. } => (&[], &[]),
    }
}

/// Whether clip time `at` is in the window of a member playing `length`.
fn shows(m: &Member<'_>, at: Ticks) -> bool {
    lumen_project::edit::in_window(m.clip, at) && at < m.length
}

/// Whether the member's clip loops (it is then a run of its own).
fn loops(m: &Member<'_>) -> bool {
    m.clip.loop_length.is_some_and(|l| l.0 > 0)
}

/// The notes a run plays, in ticks from its first clip's start, each cut
/// where it stops (see the module documentation). Unsorted.
pub(crate) fn run_notes(run: &[Member<'_>]) -> Vec<MidiNote> {
    run_notes_masked(run, None)
}

fn run_notes_masked(run: &[Member<'_>], suppression: Option<Suppression>) -> Vec<MidiNote> {
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
            let at = m.clip.start + n.start();
            if suppression.is_some_and(|s| s.hides(m.clip, at)) {
                continue;
            }
            if let Some(&(_, _, e)) = reaching.iter().find(|r| (r.0, r.1) == (at, n.pitch())) {
                let mut end = n.end().min(m.length);
                if let Some(s) = suppression.filter(|s| {
                    s.exempt != Some(m.clip.id)
                        && s.from < s.until
                        && at < s.from
                        && m.clip.start + end > s.from
                }) {
                    end = end.min(s.from - m.clip.start);
                }
                out[e].length = offset + end - out[e].start;
                if n.end() >= m.length && end >= m.length {
                    next.push((at, n.pitch(), e));
                }
            }
        }
        for n in notes.iter().filter(|n| !n.muted() && shows(m, n.start())) {
            let arrangement_start = m.clip.start + n.start();
            if suppression.is_some_and(|s| s.hides(m.clip, arrangement_start)) {
                continue;
            }
            let mut length = if loops(m) {
                n.length()
            } else {
                n.length().min(m.length - n.start())
            };
            if !loops(m)
                && let Some(s) = suppression.filter(|s| {
                    s.exempt != Some(m.clip.id)
                        && s.from < s.until
                        && arrangement_start < s.from
                        && arrangement_start + length > s.from
                })
            {
                length = length.min(s.from - arrangement_start);
            }
            if !loops(m)
                && n.end() >= m.length
                && arrangement_start + length >= m.clip.start + m.length
            {
                next.push((m.clip.start + n.start(), n.pitch(), out.len()));
            }
            out.push(MidiNote {
                start: offset + n.start(),
                length,
                pitch: n.pitch(),
                velocity: n.velocity(),
            });
        }
        reaching = next;
    }
    out
}

/// The controllers a run plays, in ticks from its first clip's start.
fn run_controls(run: &[Member<'_>], suppression: Option<Suppression>) -> Vec<MidiControl> {
    let Some(head) = run.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for m in run {
        let offset = m.clip.start - head.clip.start;
        let (_, events) = content(m.clip);
        for e in events.iter().filter(|e| {
            shows(m, e.at) && !suppression.is_some_and(|s| s.hides(m.clip, m.clip.start + e.at))
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
) -> (Vec<MidiNote>, Vec<MidiControl>) {
    let ClipContent::Midi { notes, events } = &clip.content else {
        return (Vec::new(), Vec::new());
    };
    let phase = Ticks((clip.loop_phase.0 + from.0).rem_euclid(period.0));
    let mut played_notes = Vec::new();
    for note in notes
        .iter()
        .filter(|n| !n.muted() && n.start() >= Ticks::ZERO && n.start() < period)
    {
        let source_end = (note.start() + note.length()).min(period);
        if note.start() < phase && source_end > phase && length > Ticks::ZERO {
            played_notes.push(MidiNote {
                start: Ticks::ZERO,
                length: (source_end - phase).min(length),
                pitch: note.pitch(),
                velocity: note.velocity(),
            });
        }
        if note.start() < Ticks::ZERO || note.start() >= period {
            continue;
        }
        let at = Ticks((note.start().0 - phase.0).rem_euclid(period.0));
        if at >= length {
            continue;
        }
        let note_length = note.length().min(period - note.start()).min(length - at);
        played_notes.push(MidiNote {
            start: at,
            length: note_length,
            pitch: note.pitch(),
            velocity: note.velocity(),
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
fn loop_pattern(clip: &model::Clip, period: Ticks) -> (Arc<[MidiNote]>, Arc<[MidiControl]>) {
    let ClipContent::Midi { notes, events } = &clip.content else {
        return (note_list(Vec::new()), control_list(Vec::new()));
    };
    let notes = notes
        .iter()
        .filter(|n| !n.muted() && n.start() >= Ticks::ZERO && n.start() < period)
        .map(|n| MidiNote {
            start: n.start(),
            length: n.length(),
            pitch: n.pitch(),
            velocity: n.velocity(),
        })
        .collect();
    let controls = events
        .iter()
        .filter(|e| e.at >= Ticks::ZERO && e.at < period)
        .flat_map(control)
        .collect();
    (note_list(notes), control_list(controls))
}

/// Where a run lies: its first clip's start, how long it plays, and the
/// loop of a looping clip.
fn extent(run: &[Member<'_>]) -> (Ticks, Ticks, Option<Ticks>) {
    let head = &run[0];
    let last = &run[run.len() - 1];
    let period = loop_period(run);
    let start = head.clip.start;
    (start, last.clip.start + last.length - start, period)
}

/// The run's original periodic pattern is shared by prefix and suffix pieces,
/// even when the bounded Replace window moves.
struct Pattern {
    /// The clips they were built from, and how much of each played (also
    /// keeps their addresses from being reused while the entry lives).
    members: Vec<(Arc<model::Clip>, Ticks)>,
    notes: Arc<[MidiNote]>,
    controls: Arc<[MidiControl]>,
}

impl Pattern {
    fn matches(&self, run: &[Member<'_>]) -> bool {
        self.members.len() == run.len()
            && self.members.iter().zip(run).all(|((clip, length), m)| {
                *length == m.length && (Arc::ptr_eq(clip, m.clip) || **clip == **m.clip)
            })
    }
}

/// A translated run, kept while its clips and suppression window are stable.
struct Cached {
    members: Vec<(Arc<model::Clip>, Ticks)>,
    suppression: Option<Suppression>,
    clips: Vec<MidiClip>,
}

impl Cached {
    fn matches(&self, run: &[Member<'_>], suppression: Option<Suppression>) -> bool {
        self.suppression == suppression
            && self.members.len() == run.len()
            && self.members.iter().zip(run).all(|((clip, length), m)| {
                *length == m.length && (Arc::ptr_eq(clip, m.clip) || **clip == **m.clip)
            })
    }
}

/// Translated runs by their first clip's id, so an edit elsewhere does not
/// copy a 100,000-note clip again and the engine's comparison is a pointer
/// check.
#[derive(Default)]
pub struct Cache {
    runs: HashMap<Id, Cached>,
    patterns: HashMap<Id, Pattern>,
}

impl Cache {
    /// The engine clip `run` plays as, optionally masking a bounded Replace
    /// interval. Cached note and control lists are rebuilt only when either
    /// the run or its suppression window changes.
    pub(crate) fn clip_with_suppression(
        &mut self,
        run: &[Member<'_>],
        suppression: Option<Suppression>,
    ) -> &[MidiClip] {
        let head = run[0].clip;
        let cached_matches = self
            .runs
            .get(&head.id)
            .is_some_and(|c| c.matches(run, suppression));

        if !cached_matches {
            let period = loop_period(run);
            let pattern = match self.patterns.get(&head.id) {
                Some(cached) if cached.matches(run) => {
                    (Arc::clone(&cached.notes), Arc::clone(&cached.controls))
                }
                _ => {
                    let source = Member {
                        clip: head,
                        length: head.length,
                    };
                    let source_run = if period.is_some() {
                        std::slice::from_ref(&source)
                    } else {
                        run
                    };
                    let (notes, controls) = if let Some(period) = period {
                        loop_pattern(head, period)
                    } else {
                        (
                            note_list(run_notes(source_run)),
                            control_list(run_controls(source_run, None)),
                        )
                    };
                    self.patterns.insert(
                        head.id,
                        Pattern {
                            members: run.iter().map(|m| (Arc::clone(m.clip), m.length)).collect(),
                            notes: Arc::clone(&notes),
                            controls: Arc::clone(&controls),
                        },
                    );
                    (notes, controls)
                }
            };

            let (start, total_length, _) = extent(run);
            let split_loop = period.is_some()
                && suppression.is_some_and(|s| {
                    s.exempt != Some(head.id)
                        && s.from < s.until
                        && s.from < start + total_length
                        && start < s.until
                });
            let suppression_applies = suppression.is_some_and(|s| {
                run.iter().any(|m| {
                    s.exempt != Some(m.clip.id)
                        && s.from < m.clip.start + m.clip.length
                        && m.clip.start < s.until
                })
            });
            let seams: Vec<Ticks> = run[1..].iter().map(|m| m.clip.start - head.start).collect();
            let mut clips = Vec::new();
            if split_loop {
                let suppression = suppression.expect("split loop has a bounded suppression window");
                let period = period.expect("split loop has a positive period");
                let clip_end = start + total_length;
                let mask_from = suppression.from.max(start);
                let mask_until = suppression.until.min(clip_end);
                clips.extend(loop_segments(
                    head,
                    LoopSpan {
                        start,
                        elapsed: Ticks::ZERO,
                        length: mask_from - start,
                        period,
                    },
                    &pattern.0,
                    &pattern.1,
                    &seams,
                ));
                let suffix_elapsed = mask_until - start;
                clips.extend(loop_segments(
                    head,
                    LoopSpan {
                        start: mask_until,
                        elapsed: suffix_elapsed,
                        length: clip_end - mask_until,
                        period,
                    },
                    &pattern.0,
                    &pattern.1,
                    &seams,
                ));
            } else if let Some(period) = period.filter(|p| head.loop_phase.0.rem_euclid(p.0) != 0) {
                let (start, length, _) = extent(run);
                clips.extend(loop_segments(
                    head,
                    LoopSpan {
                        start,
                        elapsed: Ticks::ZERO,
                        length,
                        period,
                    },
                    &pattern.0,
                    &pattern.1,
                    &seams,
                ));
            } else {
                let (start, length, period) = extent(run);
                let (notes, controls) = if !suppression_applies {
                    pattern
                } else {
                    (
                        note_list(run_notes_masked(run, suppression)),
                        control_list(run_controls(run, suppression)),
                    )
                };
                clips.push(engine_clip(
                    head.id,
                    start,
                    length,
                    period,
                    notes,
                    controls,
                    run[1..].iter().map(|m| m.clip.start - head.start).collect(),
                ));
            }
            self.runs.insert(
                head.id,
                Cached {
                    members: run.iter().map(|m| (Arc::clone(m.clip), m.length)).collect(),
                    suppression,
                    clips: clips.clone(),
                },
            );
        }
        self.runs
            .get(&head.id)
            .map(|cached| cached.clips.as_slice())
            .expect("the MIDI run cache was filled above")
    }

    /// Forgets runs whose first clip is not in `project` any more.
    pub(crate) fn retain(&mut self, project: &model::Project) {
        let live: std::collections::HashSet<Id> = project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        if self.runs.len() > live.len() {
            self.runs.retain(|id, _| live.contains(id));
        }
        if self.patterns.len() > live.len() {
            self.patterns.retain(|id, _| live.contains(id));
        }
    }
}

fn engine_clip(
    id: Id,
    start: Ticks,
    length: Ticks,
    period: Option<Ticks>,
    notes: Arc<[MidiNote]>,
    controls: Arc<[MidiControl]>,
    seams: Arc<[Ticks]>,
) -> MidiClip {
    MidiClip {
        id: id.0,
        start,
        length,
        period,
        notes,
        controls,
        seams,
    }
}

/// Builds the phase-rotated first source fragment, then ordinary source-loop
/// repeats. `elapsed` is the clip's timeline offset of this segment.
struct LoopSpan {
    start: Ticks,
    elapsed: Ticks,
    length: Ticks,
    period: Ticks,
}

fn loop_segments(
    clip: &model::Clip,
    span: LoopSpan,
    notes: &Arc<[MidiNote]>,
    controls: &Arc<[MidiControl]>,
    seams: &[Ticks],
) -> Vec<MidiClip> {
    let LoopSpan {
        start,
        elapsed,
        length,
        period,
    } = span;
    if length <= Ticks::ZERO {
        return Vec::new();
    }
    let phase = Ticks((clip.loop_phase.0 + elapsed.0).rem_euclid(period.0));
    if phase == Ticks::ZERO {
        return vec![engine_clip(
            clip.id,
            start,
            length,
            Some(period),
            Arc::clone(notes),
            Arc::clone(controls),
            segment_seams(elapsed, length, seams),
        )];
    }
    let first = (period - phase).min(length);
    let (fragment_notes, fragment_controls) = loop_fragment(clip, elapsed, first, period);
    let mut segments = vec![engine_clip(
        clip.id,
        start,
        first,
        None,
        note_list(fragment_notes),
        control_list(fragment_controls),
        segment_seams(elapsed, first, seams),
    )];
    if first < length {
        segments.push(engine_clip(
            clip.id,
            start + first,
            length - first,
            Some(period),
            Arc::clone(notes),
            Arc::clone(controls),
            segment_seams(elapsed + first, length - first, seams),
        ));
    }
    segments
}

fn segment_seams(elapsed: Ticks, length: Ticks, seams: &[Ticks]) -> Arc<[Ticks]> {
    let end = elapsed + length;
    seams
        .iter()
        .filter(|&&seam| elapsed < seam && seam < end)
        .map(|&seam| seam - elapsed)
        .collect::<Vec<_>>()
        .into()
}

/// The keys `track`'s clips hold at arrangement time `at`, as the engine
/// plays them (`hold.rs`): a note sounds from its start up to where it
/// stops, within its repeat of a looping clip's content. A zero-length note
/// sounds for one tick. Each key once.
pub(crate) fn sounding_keys(track: &model::Track, at: Ticks) -> Vec<u8> {
    let members = track
        .clips
        .iter()
        .filter(|c| !c.muted && c.length.0 > 0 && matches!(c.content, ClipContent::Midi { .. }))
        .map(|clip| Member {
            clip,
            length: clip.length,
        });
    let mut keys = Vec::new();
    for run in runs(members) {
        let (start, length, period) = extent(&run);
        let local = (at - start).0;
        if local < 0 || local >= length.0 {
            continue;
        }
        let (notes, local) = if let Some(period) = period {
            (
                loop_pattern(run[0].clip, period).0.to_vec(),
                (run[0].clip.loop_phase.0 + local).rem_euclid(period.0),
            )
        } else {
            (run_notes(&run), local)
        };
        for n in notes {
            let (from, to) = (n.start.0, n.start.0 + n.length.0.max(1));
            // A loop's notes stop at the end of their repeat.
            let to = period.map_or(to, |p| to.min(p.0));
            if (from..to.max(from + 1)).contains(&local) && !keys.contains(&n.pitch) {
                keys.push(n.pitch);
            }
        }
    }
    keys
}

#[cfg(test)]
mod tests;
