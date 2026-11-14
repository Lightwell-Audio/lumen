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
//!
//! **The files.** `voice` reads a note through the track's drum map and for
//! the instrument it is heard on (M8 D4); `notes` turns a run into the notes
//! and controllers the engine plays, and reads a loop's pattern; `cache` keeps
//! a translated run while nothing it was made from changed; `sounding` says
//! which notes a track holds at a position, for the lights. This file is the
//! shape of a run: which clips are one, how long it plays and where its seams
//! are.

mod cache;
mod notes;
mod sounding;
mod voice;

pub use cache::Cache;
pub(crate) use notes::{loop_fragment, run_notes};
pub(crate) use sounding::{sounding_inputs, sounding_keys};
pub(crate) use voice::{Dest, Member};

use lumen_core::time::Ticks;
use lumen_project::edit::seams;
use lumen_project::model::{self, ClipContent};

/// Arrangement interval hidden while a bounded Replace take is recorded,
/// the take's own clip included: in Replace a pass replaces the one before
/// it, so it is not heard under the new one (`record_span`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Suppression {
    pub from: Ticks,
    pub until: Ticks,
}

impl Suppression {
    fn hides(self, at: Ticks) -> bool {
        self.from < self.until && self.from <= at && at < self.until
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

pub(super) fn extent(run: &[Member<'_>]) -> (Ticks, Ticks, Option<Ticks>) {
    let head = &run[0];
    let last = &run[run.len() - 1];
    let period = loop_period(run);
    let start = head.clip.start;
    (start, last.clip.start + last.length - start, period)
}

#[cfg(test)]
mod tests;
