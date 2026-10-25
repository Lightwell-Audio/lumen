//! Normalize (M7b slice E1): the pure parts. A clip is normalized by setting
//! its gain so that the loudest sample of the stretch of its file that it
//! plays reaches 0 dBFS. The scan of the audio is the app's (it runs on a
//! worker); this module says which stretch that is and what gain a peak
//! asks for.

use lumen_core::time::TempoMap;

use crate::edit::tempo_of;
use crate::model::{Clip, ClipContent, Id, Project};

/// A peak at or below this (-100 dBFS) is silence: nothing is normalized to
/// it.
pub const SILENT: f32 = 1e-5;

/// The stretch of a clip's file it plays, in seconds of that file (of its
/// reversed twin, when the clip is reversed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// Where the stretch begins.
    pub from: f64,
    /// How long it is: the clip, or one repeat of its loop.
    pub span: f64,
}

/// The stretch of the file an audio clip plays, or `None` for a MIDI clip.
/// `rate` is the file's sample rate (the media offset counts its frames).
/// The span ignores where the file ends; a reader clamps it.
pub fn window(clip: &Clip, rate: f64, tempo: &TempoMap) -> Option<Window> {
    let ClipContent::Audio { offset, .. } = &clip.content else {
        return None;
    };
    // The material begins at the clip's start, or for a loop at the origin
    // its repeats are counted from.
    let (begin, end) = match clip.loop_length {
        Some(period) => {
            let origin = clip.start - clip.loop_phase;
            (origin, origin + period)
        }
        None => (clip.start, clip.start + clip.length),
    };
    Some(Window {
        from: *offset as f64 / rate + clip.audio_phase_seconds,
        span: tempo.seconds_at(end) - tempo.seconds_at(begin),
    })
}

/// What a scan reads for a clip: which file (its reversed twin when
/// `reversed`) and which stretch of it. If any of these differ when the scan
/// ends, the clip no longer plays what was scanned, and its peak is not
/// applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    pub media: Id,
    pub reversed: bool,
    pub window: Window,
}

/// The [`Source`] of an audio clip in `project`, or `None` for a MIDI clip or
/// a clip whose media is not in the project.
pub fn source_of(project: &Project, clip: &Clip) -> Option<Source> {
    let ClipContent::Audio { media, .. } = &clip.content else {
        return None;
    };
    let rate = project.media.iter().find(|m| m.id == *media)?.sample_rate;
    Some(Source {
        media: *media,
        reversed: clip.reversed,
        window: window(clip, f64::from(rate.max(1)), &tempo_of(project))?,
    })
}

/// Whether the clip `id` is still on a track of `project` and still plays
/// exactly what `scanned` read.
pub fn is_current(project: &Project, id: Id, scanned: &Source) -> bool {
    project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id)
        .and_then(|c| source_of(project, c))
        .is_some_and(|now| now == *scanned)
}

/// The clip gain in decibels that brings a peak of `peak` (a linear sample
/// value) to full scale, or `None` when the stretch is silent.
pub fn gain_for_peak(peak: f32) -> Option<f32> {
    (peak.is_finite() && peak > SILENT).then(|| -20.0 * peak.log10())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tempo_of;
    use crate::edit::tests::{q, song};
    use lumen_core::time::Ticks;
    use std::sync::Arc;

    fn clip(project: &crate::model::Project) -> Clip {
        (*project.tracks[0].clips[0]).clone()
    }

    #[test]
    fn a_scan_is_current_only_while_the_clip_plays_what_it_read() {
        let project = song();
        let id = project.tracks[0].clips[0].id;
        let scanned = source_of(&project, &clip(&project)).unwrap();
        assert!(is_current(&project, id, &scanned));
        // Trimmed, slipped, reversed, retimed or removed while it scanned.
        let edit = |f: &dyn Fn(&mut Clip)| {
            let mut p = project.clone();
            f(Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]));
            p
        };
        assert!(!is_current(&edit(&|c| c.length = q(1)), id, &scanned));
        assert!(!is_current(&edit(&|c| c.reversed = true), id, &scanned));
        assert!(!is_current(
            &edit(&|c| c.audio_phase_seconds = 0.5),
            id,
            &scanned
        ));
        let mut slipped = project.clone();
        if let ClipContent::Audio { offset, .. } =
            &mut Arc::make_mut(&mut Arc::make_mut(&mut slipped.tracks[0]).clips[0]).content
        {
            *offset += 100;
        }
        assert!(!is_current(&slipped, id, &scanned));
        let mut gone = project.clone();
        Arc::make_mut(&mut gone.tracks[0]).clips.remove(0);
        assert!(!is_current(&gone, id, &scanned));
        // A gain change does not matter: the gain is what the scan sets.
        assert!(is_current(
            &edit(
                &|c| if let ClipContent::Audio { gain_db, .. } = &mut c.content {
                    *gain_db = -6.0;
                }
            ),
            id,
            &scanned
        ));
    }

    #[test]
    fn a_clip_plays_from_its_offset_for_its_length() {
        let mut project = song();
        let t = Arc::make_mut(&mut project.tracks[0]);
        let c = Arc::make_mut(&mut t.clips[0]);
        c.length = q(4);
        if let ClipContent::Audio { offset, .. } = &mut c.content {
            *offset = 48_000;
        }
        let rate = f64::from(project.media[0].sample_rate);
        let w = window(&clip(&project), rate, &tempo_of(&project)).unwrap();
        // 120 BPM: four quarters are two seconds.
        assert!((w.from - 48_000.0 / rate).abs() < 1e-9);
        assert!((w.span - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_loop_is_one_repeat_of_the_loop_and_a_phase_moves_nothing() {
        let mut project = song();
        let t = Arc::make_mut(&mut project.tracks[0]);
        let c = Arc::make_mut(&mut t.clips[0]);
        c.loop_length = Some(q(2));
        c.loop_phase = q(1);
        c.length = q(9);
        let rate = f64::from(project.media[0].sample_rate);
        let w = window(&clip(&project), rate, &tempo_of(&project)).unwrap();
        assert!((w.span - 1.0).abs() < 1e-9, "two quarters at 120 BPM");
    }

    #[test]
    fn a_cut_clip_begins_where_the_cut_left_it() {
        let mut project = song();
        let t = Arc::make_mut(&mut project.tracks[0]);
        let c = Arc::make_mut(&mut t.clips[0]);
        c.audio_phase_seconds = 0.25;
        c.length = Ticks(q(1).0);
        if let ClipContent::Audio { offset, .. } = &mut c.content {
            *offset = 0;
        }
        let rate = f64::from(project.media[0].sample_rate);
        let w = window(&clip(&project), rate, &tempo_of(&project)).unwrap();
        assert!((w.from - 0.25).abs() < 1e-9);
    }

    #[test]
    fn the_gain_that_brings_a_peak_to_full_scale() {
        assert!((gain_for_peak(0.5).unwrap() - 6.0206).abs() < 1e-3);
        assert!(gain_for_peak(1.0).unwrap().abs() < 1e-6);
        assert!(
            gain_for_peak(2.0).unwrap() < 0.0,
            "over full scale turns down"
        );
        assert_eq!(gain_for_peak(0.0), None);
        assert_eq!(gain_for_peak(SILENT), None);
        assert_eq!(gain_for_peak(f32::NAN), None);
        assert_eq!(gain_for_peak(f32::INFINITY), None);
    }
}
