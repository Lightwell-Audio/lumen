//! The loop edits of a warped clip (M9.5 S4a). The brace edits of
//! [`super::clip_loop`] for a clip whose file plays by a map: the loop is
//! counted in ticks, not seconds, since the map is.
//!
//! A looping clip plays map tick `(position − clock) mod period`, where the
//! clock is `start − loop_phase`. In the first turn, the one the clip starts
//! in, that is `position − origin` (`origin` is the clock plus whole turns).
//! Each edit picks the new clock `K′` and moves the map by `origin − K′`, so
//! every frame from the clip's start to the first repeat stays what it was.

use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;

use super::clip_loop::{LoopEdit, LoopLimits};
use super::{Edge, MIN_LENGTH, trim, warp};
use crate::model::{Clip, ClipContent, Project};

/// The start of the turn the clip starts in and its end, in ticks, if the
/// clip loops.
pub(super) fn turn(clip: &Clip) -> Option<(Ticks, Ticks)> {
    let period = clip.loop_length.filter(|p| *p > Ticks::ZERO)?;
    let origin = clip.start - Ticks(clip.loop_phase.0.rem_euclid(period.0));
    Some((origin, origin + period))
}

/// How far each edge of the brace may go for a warped clip (see
/// [`super::clip_loop::loop_limits`]).
pub(super) fn limits(project: &Project, clip: &Clip) -> Option<LoopLimits> {
    clip.warp.as_ref()?;
    let total = warp::file_seconds(project, clip)?;
    let (origin, end) = turn(clip)?;
    // The file's first frame sits where the map puts second 0.
    let earliest = (origin + warp::first_tick(clip)?)
        .max(Ticks::ZERO)
        .min(clip.start);
    let last = origin + warp::last_tick(clip, total)?;
    let least_end = (origin + MIN_LENGTH).max(clip.start + Ticks(1));
    let latest_start = clip.start.min(end - MIN_LENGTH).max(earliest);
    Some(LoopLimits {
        start: (earliest, latest_start),
        end: (least_end, last.max(least_end)),
    })
}

/// `clip` with its loop changed by `edit`.
pub(super) fn set(project: &Project, clip: &Clip, edit: LoopEdit) -> Option<Clip> {
    clip.warp.as_ref()?;
    let total = warp::file_seconds(project, clip)?;
    let looping = clip.loop_length.is_some_and(|p| p > Ticks::ZERO);
    let rate = super::audio_of(project, clip).map(|(rate, _, _)| f64::from(rate));
    let mut c = clip.clone();
    // The clock the map is measured from afterwards, and the turn's origin
    // before.
    let (clock, origin) = match edit {
        LoopEdit::On => {
            if looping {
                return None;
            }
            // What the file holds from the clip's start.
            let holds = warp::last_tick(clip, total)?;
            if holds < MIN_LENGTH {
                return None;
            }
            c.loop_length = Some(clip.length.min(holds));
            c.loop_phase = Ticks::ZERO;
            (clip.start, clip.start)
        }
        LoopEdit::End(to) => {
            let (origin, _) = turn(clip)?;
            let limits = limits(project, clip)?;
            c.loop_length = Some(to.clamp(limits.end.0, limits.end.1) - origin);
            c.loop_phase = clip.start - origin;
            (origin, origin)
        }
        LoopEdit::Start(to) => {
            let (origin, end) = turn(clip)?;
            let limits = limits(project, clip)?;
            let to = to.clamp(limits.start.0, limits.start.1);
            c.loop_length = Some(end - to);
            c.loop_phase = clip.start - to;
            (to, origin)
        }
        LoopEdit::Off => {
            if !looping {
                return None;
            }
            let (origin, _) = turn(clip)?;
            c.loop_length = None;
            c.loop_phase = Ticks::ZERO;
            (clip.start, origin)
        }
    };
    // The envelopes keep their ticks on the timeline: they are measured from
    // the clock, which moved.
    let old_clock = clock_of(clip);
    if old_clock != clock {
        for (kind, lane) in clip.envelopes.iter() {
            let points: Vec<Point> = lane
                .points()
                .iter()
                .map(|p| Point {
                    at: p.at + (old_clock - clock),
                    ..*p
                })
                .collect();
            if let Ok(lane) = Lane::new(points) {
                c.envelopes.set(kind, lane);
            }
        }
    }
    let by = origin - clock;
    if let Some(warp) = &mut c.warp {
        warp.shift(by);
    }
    warp::shift_passes(&mut c, by, rate);
    warp::anchor(project, &mut c);
    if matches!(edit, LoopEdit::Off) {
        // As much of the clip as the file holds from its first frame.
        let end = clip.start + clip.length;
        return Some(trim(
            project,
            &super::tempo_of(project),
            &c,
            Edge::End,
            end,
            false,
        ));
    }
    debug_assert!(matches!(c.content, ClipContent::Audio { .. }));
    Some(c)
}

/// The clock the engine reads `clip`'s envelopes from.
fn clock_of(clip: &Clip) -> Ticks {
    if clip.loop_length.is_some_and(|p| p > Ticks::ZERO) {
        clip.start - clip.loop_phase
    } else {
        clip.start
    }
}
