use lumen_core::time::{RAMP_STEP, TempoPoint, Ticks, bpm_at};
use lumen_core::warp::Marker;

use crate::model::{Clip, ClipContent, Pass, Project, Warp, WarpMode};

use super::{Edge, audio_of};

pub(super) const FRAME_SLACK: f64 = 1e-6;

pub(super) fn split_seconds(seconds: f64, rate: f64) -> (i64, f64) {
    let frames = seconds.max(0.0) * rate;
    let nearest = frames.round();
    if (frames - nearest).abs() < FRAME_SLACK {
        return (nearest as i64, 0.0);
    }
    let whole = frames.floor();
    (whole as i64, seconds - whole / rate)
}

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

fn reanchor_passes(project: &Project, clip: &mut Clip) {
    if let Some((rate, _, _)) = audio_of(project, clip) {
        anchor_passes(clip, f64::from(rate));
    }
}

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

pub(super) fn first_tick(clip: &Clip) -> Option<Ticks> {
    warps(clip).map(Warp::first_tick).max()
}

pub(super) fn last_tick(clip: &Clip, total: f64) -> Option<Ticks> {
    warps(clip).map(|w| w.last_tick(total)).min()
}

pub(super) fn file_seconds(project: &Project, clip: &Clip) -> Option<f64> {
    let (rate, frames, _) = audio_of(project, clip)?;
    Some(frames as f64 / f64::from(rate))
}

pub(super) fn slip_limit(project: &Project, clip: &Clip, by: Ticks) -> Ticks {
    let (Some(warp), Some(total)) = (&clip.warp, file_seconds(project, clip)) else {
        return by;
    };
    let latest = Ticks::ZERO - warp.first_tick();
    let earliest = window(clip) - warp.last_tick(total);
    by.clamp(earliest, latest.max(earliest))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    NotAudio,
    Unchanged,
    Setting,
}

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

pub fn fit_to_file(project: &Project, clip: &Clip) -> Clip {
    let loops = clip.loop_length.is_some_and(|p| p > Ticks::ZERO);
    if clip.warp.is_none() || loops {
        return clip.clone();
    }
    super::trim(
        project,
        &super::tempo_of(project),
        clip,
        Edge::End,
        clip.start + clip.length,
        false,
    )
}

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

const MOST_RECORDED_MARKERS: usize = 1_024;

pub fn as_recorded(project: &Project, clip: &Clip, mode: WarpMode) -> Result<Clip, Refused> {
    let tempo = super::tempo_of(project);
    let mut out = enable(project, clip, bpm_at(&project.tempo, clip.start))?;
    let ticks = tempo_ticks(&project.tempo, clip.start, clip.length);
    let varies = tempo_varies(&project.tempo, clip.start, clip.length);
    let origin = tempo.seconds_at(clip.start);
    let fill = |warp: &mut Warp| {
        warp.mode = mode;
        let base = warp.markers[0].source;
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

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Mode(WarpMode),
    OriginalBpm(f64),
    Transpose(i32),
}

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
    if matches!(change, Change::OriginalBpm(_)) {
        out = fit_to_file(project, &out);
    }
    if !warps(clip)
        .zip(warps(&out))
        .all(|(before, after)| may_become(project, &out, before, after))
    {
        return Err(Refused::Setting);
    }
    anchor(project, &mut out);
    reanchor_passes(project, &mut out);
    Ok(out)
}

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

fn may_become(project: &Project, clip: &Clip, before: &Warp, after: &Warp) -> bool {
    let Some(total) = file_seconds(project, clip) else {
        return true;
    };
    let span = window(clip);
    let inside = |second: f64| (0.0..=total).contains(&second);
    let (ends_before, ends_after) = (
        inside(before.source_at(span)),
        inside(after.source_at(span)),
    );
    inside(after.source_at(Ticks::ZERO)) && (ends_after || !ends_before)
}

pub fn origin(clip: &Clip) -> Ticks {
    if clip.loop_length.is_some_and(|p| p > Ticks::ZERO) {
        clip.start - clip.loop_phase
    } else {
        clip.start
    }
}

pub fn within(clip: &Clip, at: Ticks) -> Ticks {
    if clip.loop_length.is_some_and(|p| p > Ticks::ZERO) {
        Ticks((at - origin(clip)).0.rem_euclid(window(clip).0))
    } else {
        at - origin(clip)
    }
}

pub fn from_here(
    project: &Project,
    clip: &Clip,
    original_bpm: f64,
    (here, bar): (Ticks, Ticks),
    snap: &dyn Fn(f64) -> Option<f64>,
) -> Result<Clip, Refused> {
    let origin = origin(clip);
    let (here_in, bar_in) = (within(clip, here), within(clip, bar));
    let heard = match &clip.warp {
        Some(warp) => warp.source_at(here_in),
        None => {
            let (rate, _, offset) = audio_of(project, clip).ok_or(Refused::NotAudio)?;
            let tempo = super::tempo_of(project);
            offset as f64 / f64::from(rate)
                + clip.audio_phase_seconds
                + tempo.seconds_at(origin + here_in)
                - tempo.seconds_at(origin)
        }
    };
    let on = if clip.warp.is_some() {
        clip.clone()
    } else {
        enable(project, clip, original_bpm)?
    };
    let warp = on.warp.as_ref().ok_or(Refused::Unchanged)?;
    let source = snap(heard).unwrap_or(heard);
    let now = lumen_core::warp::tick_at_source(&warp.markers, warp.original_bpm, source);
    let mut markers = warp.markers.clone();
    lumen_core::warp::shift(&mut markers, bar_in - now);
    match set_markers(project, &on, markers) {
        Err(Refused::Unchanged) if on != *clip => Ok(on),
        other => other,
    }
}

pub(super) fn window(clip: &Clip) -> Ticks {
    clip.loop_length
        .filter(|p| *p > Ticks::ZERO)
        .unwrap_or(clip.length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{q, song};
    use lumen_core::time::PPQ;
    use std::sync::Arc;

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
    fn a_clip_warped_faster_than_its_file_is_cut_to_where_the_sound_ends() {
        let project = song();
        let clip = (*project.tracks[0].clips[0]).clone();
        let warped = enable(&project, &clip, 30.0).unwrap();
        let fitted = fit_to_file(&project, &warped);
        assert_eq!(fitted.start, clip.start);
        assert!(fitted.length < warped.length, "cut to the sound");
        assert!(fitted.length >= super::super::MIN_LENGTH);
        assert_eq!(fit_to_file(&project, &fitted), fitted);
    }

    #[test]
    fn fitting_never_lengthens_and_leaves_what_it_does_not_apply_to() {
        let project = song();
        let clip = (*project.tracks[0].clips[0]).clone();
        assert_eq!(fit_to_file(&project, &clip), clip);
        let slow = enable(&project, &clip, 240.0).unwrap();
        assert_eq!(fit_to_file(&project, &slow), slow);
        let mut looping = enable(&project, &clip, 30.0).unwrap();
        looping.loop_length = Some(looping.length);
        assert_eq!(fit_to_file(&project, &looping), looping);
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

    #[test]
    fn warp_from_here_puts_the_heard_audio_on_the_bar_and_keeps_the_stretch() {
        let project = song();
        let clip = (*project.tracks[0].clips[0]).clone();
        let (here, bar) = (q(6), q(4));
        let none = |_: f64| None;
        let moved = from_here(&project, &clip, 120.0, (here, bar), &none).unwrap();
        let warp = moved.warp.as_ref().expect("warp is on");
        let heard = 1_000.0 / 48_000.0 + 1.0;
        assert!((warp.source_at(bar - origin(&moved)) - heard).abs() < 1e-6);
        let span = warp.source_at(q(2)) - warp.source_at(Ticks::ZERO);
        assert!((span - 1.0).abs() < 1e-9);
        assert_eq!(moved.start, clip.start);
        assert!(matches!(
            moved.content,
            ClipContent::Audio { offset: 49_000, .. }
        ));
        let snap = |s: f64| ((s - heard).abs() < 1e-6).then_some(heard + 0.01);
        let snapped = from_here(&project, &clip, 120.0, (here, bar), &snap).unwrap();
        let warp = snapped.warp.as_ref().unwrap();
        assert!((warp.source_at(Ticks::ZERO) - (heard + 0.01)).abs() < 1e-6);
        assert_eq!(
            from_here(&project, &moved, 120.0, (bar, bar), &none),
            Err(Refused::Unchanged)
        );
    }

    #[test]
    fn warp_from_here_warps_a_clip_that_is_not_and_refuses_what_leaves_the_file() {
        let project = song();
        let clip = (*project.tracks[0].clips[0]).clone();
        let none = |_: f64| None;
        let on = from_here(&project, &clip, 100.0, (q(4), q(4)), &none).unwrap();
        assert_eq!(on, enable(&project, &clip, 100.0).unwrap());
        assert_eq!(
            from_here(&project, &clip, 120.0, (q(4), q(8)), &none),
            Err(Refused::Setting)
        );
        let mut midi = clip;
        midi.content = ClipContent::midi();
        assert_eq!(
            from_here(&project, &midi, 120.0, (q(4), q(4)), &none),
            Err(Refused::NotAudio)
        );
    }

    #[test]
    fn warp_from_here_aligns_the_sound_heard_when_the_file_tempo_differs() {
        let project = song();
        let clip = (*project.tracks[0].clips[0]).clone();
        let none = |_: f64| None;
        let base = 1_000.0 / 48_000.0;
        for (here, bar) in [(q(6), q(4)), (q(8), q(5)), (q(5), q(4))] {
            let moved = from_here(&project, &clip, 100.0, (here, bar), &none).unwrap();
            let warp = moved.warp.as_ref().expect("warp is on");
            let heard = base + (here - clip.start).0 as f64 / PPQ as f64 * 0.5;
            let landed = warp.source_at(bar - origin(&moved));
            assert!(
                (landed - heard).abs() < 1e-6,
                "{landed} on the bar, {heard} heard at {here:?}"
            );
        }
    }

    #[test]
    fn warp_from_here_reads_the_second_heard_through_a_tempo_change() {
        let mut project = song();
        project.tempo = vec![
            TempoPoint::step(Ticks::ZERO, 120.0),
            TempoPoint::step(q(5), 60.0),
        ];
        let clip = (*project.tracks[0].clips[0]).clone();
        let none = |_: f64| None;
        let (here, bar) = (q(7), q(4));
        let moved = from_here(&project, &clip, 120.0, (here, bar), &none).unwrap();
        let warp = moved.warp.as_ref().expect("warp is on");
        let heard = 1_000.0 / 48_000.0 + 2.5;
        assert!((warp.source_at(bar - origin(&moved)) - heard).abs() < 1e-6);
    }

    #[test]
    fn warp_from_here_in_a_later_repeat_aligns_the_sound_that_repeats() {
        let mut project = song();
        let mut clip = (*project.tracks[0].clips[0]).clone();
        clip.loop_length = Some(q(4));
        clip.loop_phase = Ticks::ZERO;
        project.tracks[0] = Arc::new(crate::model::Track {
            clips: vec![Arc::new(clip.clone())],
            ..(*project.tracks[0]).clone()
        });
        let none = |_: f64| None;
        let base = 1_000.0 / 48_000.0;
        let want = base + 0.5;
        let bar = clip.start;
        let mut seen = Vec::new();
        for turn in 0..2 {
            let here = clip.start + q(1) + q(4 * turn);
            let moved = from_here(&project, &clip, 100.0, (here, bar), &none).unwrap();
            let warp = moved.warp.as_ref().expect("warp is on");
            seen.push(warp.source_at(Ticks::ZERO));
        }
        assert!((seen[0] - want).abs() < 1e-6, "{seen:?}");
        assert!((seen[1] - want).abs() < 1e-6, "{seen:?}");
        let mut phased = clip.clone();
        phased.loop_phase = q(1);
        let (here, bar) = (phased.start + q(6), phased.start + q(1));
        let moved = from_here(&project, &phased, 100.0, (here, bar), &none).unwrap();
        let again = from_here(&project, &phased, 100.0, (here + q(4), bar), &none).unwrap();
        assert_eq!(moved.warp, again.warp, "a turn later is the same place");
    }

    #[test]
    fn a_looped_clips_origin_is_its_loop_clock() {
        let project = song();
        let mut clip = (*project.tracks[0].clips[0]).clone();
        assert_eq!(origin(&clip), clip.start);
        clip.loop_length = Some(q(2));
        clip.loop_phase = q(1);
        assert_eq!(origin(&clip), clip.start - q(1));
    }

    #[test]
    fn a_place_in_a_later_turn_of_a_loop_is_the_same_place_in_the_first() {
        let project = song();
        let mut clip = (*project.tracks[0].clips[0]).clone();
        assert_eq!(
            within(&clip, clip.start + q(3)),
            q(3),
            "no loop: from the start"
        );
        clip.loop_length = Some(q(4));
        clip.loop_phase = q(1);
        assert_eq!(within(&clip, clip.start + q(1)), q(2));
        assert_eq!(within(&clip, clip.start + q(7)), q(0));
        assert_eq!(within(&clip, clip.start + q(10)), q(3));
        assert_eq!(within(&clip, clip.start - q(2)), q(3), "and before it");
    }
}
