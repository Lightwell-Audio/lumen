use lumen_core::time::{TempoMap, Ticks};

use crate::edit::tempo_of;
use crate::model::{Clip, ClipContent, Id, Project};

pub const SILENT: f32 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub from: f64,
    pub span: f64,
}

pub fn window(clip: &Clip, rate: f64, tempo: &TempoMap) -> Option<Window> {
    let ClipContent::Audio { offset, .. } = &clip.content else {
        return None;
    };
    let (begin, end) = match clip.loop_length {
        Some(period) => {
            let origin = clip.start - clip.loop_phase;
            (origin, origin + period)
        }
        None => (clip.start, clip.start + clip.length),
    };
    let from = *offset as f64 / rate + clip.audio_phase_seconds;
    let span = match &clip.warp {
        Some(warp) => warp.source_at(end - begin) - warp.source_at(Ticks::ZERO),
        None => tempo.seconds_at(end) - tempo.seconds_at(begin),
    };
    Some(Window { from, span })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    pub media: Id,
    pub reversed: bool,
    pub window: Window,
}

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

pub fn is_current(project: &Project, id: Id, scanned: &Source) -> bool {
    project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id)
        .and_then(|c| source_of(project, c))
        .is_some_and(|now| now == *scanned)
}

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
        assert!((w.from - 48_000.0 / rate).abs() < 1e-9);
        assert!((w.span - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_warped_clip_is_measured_over_the_stretch_its_map_plays() {
        let mut project = song();
        let t = Arc::make_mut(&mut project.tracks[0]);
        let c = Arc::make_mut(&mut t.clips[0]);
        c.length = q(4);
        let mut warp = crate::model::Warp::new(120.0, 0.0);
        warp.markers = vec![
            lumen_core::warp::Marker::new(0.0, Ticks::ZERO),
            lumen_core::warp::Marker::new(1.0, q(4)),
        ];
        c.warp = Some(warp);
        let rate = f64::from(project.media[0].sample_rate);
        let w = window(&clip(&project), rate, &tempo_of(&project)).unwrap();
        assert!((w.span - 1.0).abs() < 1e-9, "{}", w.span);
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
