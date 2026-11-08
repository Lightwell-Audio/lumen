//! Warp of audio clips (M9.5 slice S4a): turning it on and off, setting its
//! parts, and the helpers the other edits use to keep a map true.
//!
//! The map's ticks are measured from the clip's content origin, the clock
//! `loop_phase` counts from (`start − loop_phase` for a looping clip, `start`
//! for one that does not loop). So an edit that moves that clock moves the
//! markers by the opposite amount, as it does a clip's envelopes, and an edit
//! that moves the audio under the clip (a slip) moves them with it. What each
//! edit does is next to that edit; this module holds what they share.
//!
//! While a clip is warped, `offset` and `audio_phase_seconds` follow the map
//! ([`anchor`]): they hold the second of the file the map puts at the clock,
//! so code that does not read the map yet still finds the clip starting in
//! the right place, and turning warp off leaves the clip playing from there.

use lumen_core::time::{RAMP_STEP, TempoPoint, Ticks, bpm_at};
use lumen_core::warp::Marker;

use crate::model::{Clip, ClipContent, Pass, Project, Warp, WarpMode};

use super::audio_of;

/// Source frames two positions may differ by and still be one frame.
const FRAME_SLACK: f64 = 1e-6;

/// Splits source seconds into the whole frame the media offset holds and the
/// remainder `audio_phase_seconds` holds, with a position on a frame kept
/// exactly on it. A position before the file's first frame is the first frame
/// (the clip's window never starts there; the phase would be invalid).
pub(super) fn split_seconds(seconds: f64, rate: f64) -> (i64, f64) {
    let frames = seconds.max(0.0) * rate;
    let nearest = frames.round();
    if (frames - nearest).abs() < FRAME_SLACK {
        return (nearest as i64, 0.0);
    }
    let whole = frames.floor();
    (whole as i64, seconds - whole / rate)
}

/// Makes `offset` and `audio_phase_seconds` hold the second the map puts at
/// the clock. A clip without a warp, or whose file is unknown, is left alone.
pub fn anchor(project: &Project, clip: &mut Clip) {
    let Some(warp) = &clip.warp else {
        return;
    };
    let Some((rate, _, _)) = audio_of(project, clip) else {
        return;
    };
    let (frame, phase) = split_seconds(warp.source_at(Ticks::ZERO), f64::from(rate));
    if let ClipContent::Audio { offset, .. } = &mut clip.content {
        *offset = frame;
    }
    clip.audio_phase_seconds = phase;
}

/// Moves the map of every pass `clip` is not playing `by` ticks later, and
/// keeps those passes' own offsets in step. `rate` is the file's.
pub(super) fn shift_passes(clip: &mut Clip, by: Ticks, rate: Option<f64>) {
    let Some(takes) = &mut clip.takes else {
        return;
    };
    for other in &mut takes.others {
        if let Pass::Audio {
            warp: Some(warp), ..
        } = &mut other.pass
        {
            warp.shift(by);
        }
    }
    if let Some(rate) = rate {
        anchor_passes(clip, rate);
    }
}

/// Makes each stored pass's offset and phase hold the second its own map
/// puts at the clock (the file's `rate`).
fn anchor_passes(clip: &mut Clip, rate: f64) {
    let Some(takes) = &mut clip.takes else {
        return;
    };
    for other in &mut takes.others {
        if let Pass::Audio {
            offset,
            audio_phase_seconds,
            warp: Some(warp),
        } = &mut other.pass
        {
            (*offset, *audio_phase_seconds) = split_seconds(warp.source_at(Ticks::ZERO), rate);
        }
    }
}

/// [`anchor_passes`] for the file `clip` plays.
fn reanchor_passes(project: &Project, clip: &mut Clip) {
    if let Some((rate, _, _)) = audio_of(project, clip) {
        anchor_passes(clip, f64::from(rate));
    }
}

/// Every warp a clip keeps: the playing one first, then each stored pass's.
/// They share the clip's window and its file.
pub(super) fn warps(clip: &Clip) -> impl Iterator<Item = &Warp> {
    clip.warp.iter().chain(
        clip.takes
            .iter()
            .flat_map(|t| t.others.iter())
            .filter_map(|o| match &o.pass {
                Pass::Audio { warp, .. } => warp.as_ref(),
                Pass::Midi { .. } => None,
            }),
    )
}

/// The first tick at which every pass of `clip` reads inside its file: the
/// latest of theirs. A change that moves the clip's window moves every pass's
/// map with it, so the tightest pass is the limit.
pub(super) fn first_tick(clip: &Clip) -> Option<Ticks> {
    warps(clip).map(Warp::first_tick).max()
}

/// The last tick at which every pass of `clip` still reads inside a file of
/// `total` seconds: the earliest of theirs.
pub(super) fn last_tick(clip: &Clip, total: f64) -> Option<Ticks> {
    warps(clip).map(|w| w.last_tick(total)).min()
}

/// The length of the clip's file in seconds, or `None` for a clip without a
/// known audio file.
pub(super) fn file_seconds(project: &Project, clip: &Clip) -> Option<f64> {
    let (rate, frames, _) = audio_of(project, clip)?;
    Some(frames as f64 / f64::from(rate))
}

/// `by` ticks of slip, held to what the file allows: the clip's window may
/// not start before the file or (for a clip that does not loop) end after it.
pub(super) fn slip_limit(project: &Project, clip: &Clip, by: Ticks) -> Ticks {
    let (Some(warp), Some(total)) = (&clip.warp, file_seconds(project, clip)) else {
        return by;
    };
    // A slip of `b` reads the old map at `t − b` for `t` in the window, so
    // the window's start needs `−b ≥ first` and its end `span − b ≤ last`.
    let latest = Ticks::ZERO - warp.first_tick();
    let earliest = window(clip) - warp.last_tick(total);
    by.clamp(earliest, latest.max(earliest))
}

/// Why a warp edit did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The clip is MIDI, or its file is not in the project.
    NotAudio,
    /// Warp is already on (or, for the other edits, not on).
    Unchanged,
    /// The setting is not allowed: out of range, or transpose in Repitch.
    Setting,
}

/// `clip` with warp on, for a file made at `original_bpm`. Nothing is
/// stretched yet: the clip's one marker puts the second it plays at its clock
/// at tick 0, so at that tempo it plays as before. Its other takes get a warp
/// of their own.
pub fn enable(project: &Project, clip: &Clip, original_bpm: f64) -> Result<Clip, Refused> {
    let (rate, _, offset) = audio_of(project, clip).ok_or(Refused::NotAudio)?;
    if clip.warp.is_some() {
        return Err(Refused::Unchanged);
    }
    let rate = f64::from(rate);
    let warp = Warp::new(
        original_bpm,
        offset as f64 / rate + clip.audio_phase_seconds,
    );
    warp.check().map_err(|_| Refused::Setting)?;
    let mut out = clip.clone();
    if let Some(takes) = &mut out.takes {
        for other in &mut takes.others {
            if let Pass::Audio {
                offset,
                audio_phase_seconds,
                warp: slot,
            } = &mut other.pass
            {
                *slot = Some(Warp::new(
                    original_bpm,
                    *offset as f64 / rate + *audio_phase_seconds,
                ));
            }
        }
    }
    out.warp = Some(warp);
    Ok(out)
}

/// The ticks, from the start of a clip of `length`, at which the song's tempo
/// changes inside it, with a tick every `step` along a ramp (a ramp is a
/// curve, and a marker only draws a straight line between two points). In
/// time order, never tick 0, at most [`MOST_RECORDED_MARKERS`].
fn tempo_ticks(points: &[TempoPoint], start: Ticks, length: Ticks) -> Vec<Ticks> {
    let end = start + length;
    let step = RAMP_STEP.max(Ticks(length.0 / 512));
    let mut found = Vec::new();
    for (i, point) in points.iter().enumerate() {
        if point.at > start && point.at < end {
            found.push(point.at - start);
        }
        let Some(next) = points.get(i + 1).filter(|_| point.ramp) else {
            continue;
        };
        let mut at = point.at + step;
        while at < next.at && at < end {
            if at > start {
                found.push(at - start);
            }
            at = at + step;
        }
    }
    found.sort();
    found.dedup();
    if found.len() > MOST_RECORDED_MARKERS {
        let every = found.len().div_ceil(MOST_RECORDED_MARKERS);
        found = found.into_iter().step_by(every).collect();
    }
    found
}

/// Whether the song's tempo is not one number over `start..start + length`:
/// a point inside it, or a ramp that crosses any of it. A take wholly inside
/// one step of the ramp's sampling has no point of its own and still varies.
fn tempo_varies(points: &[TempoPoint], start: Ticks, length: Ticks) -> bool {
    let end = start + length;
    points.iter().enumerate().any(|(i, point)| {
        if point.at > start && point.at < end {
            return true;
        }
        points.get(i + 1).is_some_and(|next| {
            point.ramp && point.at < end && next.at > start && point.bpm != next.bpm
        })
    })
}

/// The most markers a take gets for the song's tempo changes: well under
/// [`lumen_core::warp::MAX_MARKERS`], which a person may still add to.
const MOST_RECORDED_MARKERS: usize = 1_024;

/// `clip`, just recorded, with warp on so that it keeps playing as it was
/// played (M9.5 S4d, D8): the file's tempo is the song's where the clip
/// starts, and a tempo change inside the clip (a step, or a ramp) gets a marker
/// that pins the second of the file recorded then to the tick it was recorded
/// at, and so does the clip's end. At a steady tempo that is the one marker
/// [`enable`] makes. Editing the
/// song's tempo later makes the clip follow it. Every pass of a loop
/// recording gets the same.
pub fn as_recorded(project: &Project, clip: &Clip, mode: WarpMode) -> Result<Clip, Refused> {
    let tempo = super::tempo_of(project);
    let mut out = enable(project, clip, bpm_at(&project.tempo, clip.start))?;
    let ticks = tempo_ticks(&project.tempo, clip.start, clip.length);
    let varies = tempo_varies(&project.tempo, clip.start, clip.length);
    let origin = tempo.seconds_at(clip.start);
    let fill = |warp: &mut Warp| {
        warp.mode = mode;
        let base = warp.markers[0].source;
        // Past the last marker a clip plays at its original tempo, so a clip
        // that changes tempo is also pinned at its end: the last stretch of
        // it is then followed too (review round 1).
        let end = varies.then_some(clip.length);
        warp.markers.extend(
            ticks
                .iter()
                .copied()
                .chain(end)
                .map(|at| Marker::new(base + tempo.seconds_at(clip.start + at) - origin, at)),
        );
    };
    let passes = out.takes.iter_mut().flat_map(|t| t.others.iter_mut());
    let others = passes.filter_map(|o| match &mut o.pass {
        Pass::Audio { warp, .. } => warp.as_mut(),
        Pass::Midi { .. } => None,
    });
    for warp in out.warp.iter_mut().chain(others) {
        fill(warp);
        warp.check().map_err(|_| Refused::Setting)?;
    }
    Ok(out)
}

/// `clip` with warp off. The map is dropped; the clip plays from where it
/// stands at the file's own speed (the module's rule keeps that where the
/// map put it).
pub fn disable(clip: &Clip) -> Result<Clip, Refused> {
    if clip.warp.is_none() {
        return Err(Refused::Unchanged);
    }
    let mut out = clip.clone();
    out.warp = None;
    if let Some(takes) = &mut out.takes {
        for other in &mut takes.others {
            if let Pass::Audio { warp, .. } = &mut other.pass {
                *warp = None;
            }
        }
    }
    Ok(out)
}

/// The change to a warped clip's settings that [`set`] makes.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Mode(WarpMode),
    /// The tempo the file was made at.
    OriginalBpm(f64),
    /// A transpose in cents.
    Transpose(i32),
}

/// `clip` with one of its warp settings changed, on every pass it keeps.
pub fn set(project: &Project, clip: &Clip, change: &Change) -> Result<Clip, Refused> {
    let Some(warp) = &clip.warp else {
        return Err(Refused::Unchanged);
    };
    let mut out = clip.clone();
    let apply = |warp: &mut Warp| match change {
        Change::Mode(mode) => warp.mode = *mode,
        Change::OriginalBpm(bpm) => warp.original_bpm = *bpm,
        Change::Transpose(cents) => warp.transpose_cents = *cents,
    };
    let mut changed = warp.clone();
    apply(&mut changed);
    // A mode that cannot transpose takes the transpose away with it.
    if !changed.mode.transposes() {
        changed.transpose_cents = 0;
    }
    if matches!(change, Change::Transpose(c) if *c != 0) && !changed.mode.transposes() {
        return Err(Refused::Setting);
    }
    changed.check().map_err(|_| Refused::Setting)?;
    if changed == *warp {
        return Err(Refused::Unchanged);
    }
    let clear_transpose = !changed.mode.transposes();
    for warp in std::iter::once(&mut out.warp)
        .chain(
            out.takes
                .iter_mut()
                .flat_map(|t| t.others.iter_mut())
                .filter_map(|o| match &mut o.pass {
                    Pass::Audio { warp, .. } => Some(warp),
                    Pass::Midi { .. } => None,
                }),
        )
        .flatten()
    {
        apply(warp);
        if clear_transpose {
            warp.transpose_cents = 0;
        }
    }
    // The original tempo decides where a map reads outside its markers, so it
    // can move what the window plays: not out of the file, in any pass.
    if !warps(clip)
        .zip(warps(&out))
        .all(|(before, after)| may_become(project, clip, before, after))
    {
        return Err(Refused::Setting);
    }
    // A new original tempo moves the second the map puts at the clock.
    anchor(project, &mut out);
    reanchor_passes(project, &mut out);
    Ok(out)
}

/// `clip` with its map replaced by `markers` (the warp editor's drag and
/// delete, from S5). The audio under the clip's window moves with them.
pub fn set_markers(project: &Project, clip: &Clip, markers: Vec<Marker>) -> Result<Clip, Refused> {
    let Some(warp) = &clip.warp else {
        return Err(Refused::Unchanged);
    };
    let mut out = clip.clone();
    let mut changed = warp.clone();
    changed.markers = markers;
    changed.check().map_err(|_| Refused::Setting)?;
    if changed == *warp {
        return Err(Refused::Unchanged);
    }
    if !may_become(project, clip, warp, &changed) {
        return Err(Refused::Setting);
    }
    out.warp = Some(changed);
    anchor(project, &mut out);
    Ok(out)
}

/// Whether the clip's window, played by `after` instead of `before`, stays in
/// its file: it must start inside it, and end inside it unless it already
/// ran past the end (a clip made that way is not made worse). A clip whose
/// file is not in the project is not held to this.
fn may_become(project: &Project, clip: &Clip, before: &Warp, after: &Warp) -> bool {
    let Some(total) = file_seconds(project, clip) else {
        return true;
    };
    let span = window(clip);
    // Strict, as the limits of [`Warp::first_tick`] and [`Warp::last_tick`] are.
    let inside = |second: f64| (0.0..=total).contains(&second);
    let (ends_before, ends_after) = (
        inside(before.source_at(span)),
        inside(after.source_at(span)),
    );
    inside(after.source_at(Ticks::ZERO)) && (ends_after || !ends_before)
}

/// The stretch of the map a clip plays: one turn of a loop, or the clip.
pub(super) fn window(clip: &Clip) -> Ticks {
    clip.loop_length
        .filter(|p| *p > Ticks::ZERO)
        .unwrap_or(clip.length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{q, song};

    #[test]
    fn splitting_seconds_keeps_a_frame_exactly() {
        assert_eq!(split_seconds(1.0, 48_000.0), (48_000, 0.0));
        let (frame, phase) = split_seconds(1.000_005, 48_000.0);
        assert_eq!(frame, 48_000);
        assert!((phase - 5e-6).abs() < 1e-12);
        assert_eq!(split_seconds(-1.0, 48_000.0), (0, 0.0));
    }

    #[test]
    fn warp_on_then_off_returns_the_clip() {
        let project = song();
        let clip = (*project.tracks[0].clips[0]).clone();
        let warped = enable(&project, &clip, 120.0).unwrap();
        assert!(warped.warp.is_some());
        assert_eq!(disable(&warped).unwrap(), clip);
        assert_eq!(enable(&project, &warped, 120.0), Err(Refused::Unchanged));
        assert_eq!(disable(&clip), Err(Refused::Unchanged));
    }

    #[test]
    fn a_midi_clip_or_a_clip_without_its_file_is_not_warped() {
        let mut project = song();
        let clip = (*project.tracks[0].clips[0]).clone();
        project.media.clear();
        assert_eq!(enable(&project, &clip, 120.0), Err(Refused::NotAudio));
        let mut midi = clip;
        midi.content = ClipContent::midi();
        assert_eq!(enable(&song(), &midi, 120.0), Err(Refused::NotAudio));
    }

    #[test]
    fn the_first_marker_puts_the_clips_second_at_its_clock() {
        let project = song();
        let mut clip = (*project.tracks[0].clips[0]).clone();
        clip.audio_phase_seconds = 0.25;
        let warped = enable(&project, &clip, 120.0).unwrap();
        let warp = warped.warp.unwrap();
        assert_eq!(warp.markers.len(), 1);
        assert!((warp.source_at(Ticks::ZERO) - (1_000.0 / 48_000.0 + 0.25)).abs() < 1e-9);
        // Half a second a beat at 120 BPM.
        assert!((warp.source_at(q(2)) - warp.source_at(Ticks::ZERO) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn repitch_takes_no_transpose() {
        let project = song();
        let clip = enable(&project, &(*project.tracks[0].clips[0]).clone(), 120.0).unwrap();
        assert_eq!(
            set(&project, &clip, &Change::Transpose(200)),
            Err(Refused::Setting),
            "tape-style transpose would change the speed"
        );
        let melodic = set(&project, &clip, &Change::Mode(WarpMode::Melodic)).unwrap();
        let moved = set(&project, &melodic, &Change::Transpose(200)).unwrap();
        assert_eq!(moved.warp.as_ref().unwrap().transpose_cents, 200);
        let back = set(&project, &moved, &Change::Mode(WarpMode::Repitch)).unwrap();
        assert_eq!(
            back.warp.unwrap().transpose_cents,
            0,
            "Repitch drops the transpose"
        );
        assert_eq!(
            set(&project, &melodic, &Change::Transpose(99_999)),
            Err(Refused::Setting)
        );
        assert_eq!(
            set(&project, &melodic, &Change::OriginalBpm(5.0)),
            Err(Refused::Setting)
        );
        assert_eq!(
            set(&project, &melodic, &Change::Mode(WarpMode::Melodic)),
            Err(Refused::Unchanged)
        );
    }

    #[test]
    fn markers_are_replaced_whole_and_the_clip_follows_them() {
        let project = song();
        let clip = enable(&project, &(*project.tracks[0].clips[0]).clone(), 120.0).unwrap();
        let markers = vec![Marker::new(0.5, Ticks::ZERO), Marker::new(2.0, q(2))];
        let moved = set_markers(&project, &clip, markers.clone()).unwrap();
        assert_eq!(moved.warp.as_ref().unwrap().markers, markers);
        // The offset follows the map: second 0.5 is frame 24 000.
        assert!(matches!(
            moved.content,
            ClipContent::Audio { offset: 24_000, .. }
        ));
        assert_eq!(moved.audio_phase_seconds, 0.0);
        let bad = vec![Marker::new(2.0, Ticks::ZERO), Marker::new(1.0, q(2))];
        assert_eq!(set_markers(&project, &clip, bad), Err(Refused::Setting));
        assert_eq!(set_markers(&project, &clip, vec![]), Err(Refused::Setting));
    }

    #[test]
    fn every_pass_of_a_take_set_gets_a_warp_of_its_own() {
        use crate::model::{OtherTake, Takes};
        let project = song();
        let mut clip = (*project.tracks[0].clips[0]).clone();
        clip.takes = Some(Takes::new(
            0,
            2,
            vec![OtherTake {
                take: 1,
                pass: Pass::Audio {
                    offset: 96_000,
                    audio_phase_seconds: 0.0,
                    warp: None,
                },
            }],
        ));
        let warped = enable(&project, &clip, 100.0).unwrap();
        let Some(Pass::Audio {
            warp: Some(other), ..
        }) = warped.takes.as_ref().and_then(|t| t.other(1)).cloned()
        else {
            panic!("the other pass has a warp")
        };
        assert_eq!(other.original_bpm, 100.0);
        assert!((other.markers[0].source - 2.0).abs() < 1e-9);
        let off = disable(&warped).unwrap();
        assert_eq!(off, clip);
    }
}
