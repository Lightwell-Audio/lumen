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

fn shows(m: &Member<'_>, at: Ticks) -> bool {
    lumen_project::edit::in_window(m.clip, at) && at < m.length
}

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
