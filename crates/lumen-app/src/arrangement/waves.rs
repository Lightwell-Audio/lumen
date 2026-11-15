use eframe::egui::Color32;
use lumen_core::time::{PPQ, TempoMap, Ticks};
use lumen_project::model::{Clip, ClipContent, Project, Track};
use lumen_render::{WaveInstance, beats, rgba};

use super::samples::Geometry;

const MAX_REPEATS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    pub slot: u32,
    pub channels: u32,
    pub native_rate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Wave {
    pub instance: WaveInstance,
    pub geometry: Geometry,
}

pub(super) fn wave_instances(
    project: &Project,
    rate: u32,
    source: impl Fn(u64) -> Option<Source>,
    color: impl Fn(&Track, &Clip) -> Color32,
) -> Vec<Wave> {
    let Ok(tempo) = TempoMap::new(project.tempo.clone()) else {
        return Vec::new();
    };
    let mut waves = Vec::new();
    let played = crate::crossfades::played(project);
    for (row, track) in project.tracks.iter().enumerate() {
        for model in &track.clips {
            let (clip, blend) = played
                .get(&model.id)
                .map_or((&**model, false), |p| (&p.clip, p.blend[0]));
            let shown = Shown {
                model,
                clip,
                blend,
                row: row as u32,
                color: color(track, model),
            };
            clip_waves(&mut waves, &tempo, rate, &shown, &source);
        }
    }
    waves
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Span {
    pub media: u64,
    pub start: f64,
    pub end: f64,
    pub first_frame: f64,
    pub frames_per_beat: f64,
    pub gain: f32,
}

pub(crate) fn spans_of(
    clip: &Clip,
    tempo: &TempoMap,
    rate: u32,
    source: impl Fn(u64) -> Option<Source>,
) -> Vec<Span> {
    let mut waves = Vec::new();
    let shown = Shown {
        model: clip,
        clip,
        blend: false,
        row: 0,
        color: Color32::WHITE,
    };
    clip_waves(&mut waves, tempo, rate, &shown, &source);
    waves
        .into_iter()
        .map(|w| Span {
            media: w.geometry.media,
            start: w.geometry.start,
            end: f64::from(w.instance.end[0]) + f64::from(w.instance.end[1]),
            first_frame: w.geometry.first_frame,
            frames_per_beat: w.geometry.frames_per_beat,
            gain: w.instance.gain,
        })
        .collect()
}

struct Shown<'a> {
    model: &'a Clip,
    clip: &'a Clip,
    blend: bool,
    row: u32,
    color: Color32,
}

fn clip_waves(
    waves: &mut Vec<Wave>,
    tempo: &TempoMap,
    rate: u32,
    shown: &Shown<'_>,
    source: &impl Fn(u64) -> Option<Source>,
) {
    let Shown {
        model,
        clip,
        blend,
        row,
        color,
    } = *shown;
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let ClipContent::Audio {
        media,
        offset,
        gain_db,
    } = &clip.content
    else {
        return;
    };
    let key = if clip.reversed {
        crate::media::reversed_key(media.0)
    } else {
        media.0
    };
    let Some(source) = source(key) else {
        return;
    };
    if clip.warp.is_some() {
        warped_waves(waves, rate, shown, key, &source);
        return;
    }
    let base = (*offset as f64 * f64::from(rate) / f64::from(source.native_rate.max(1))) as i64;
    let origin = if clip.loop_length.is_some() {
        clip.start - clip.loop_phase
    } else {
        clip.start
    };
    let source_anchor =
        ((tempo.seconds_at(origin) - clip.audio_phase_seconds) * f64::from(rate)).round() as i64;
    let offset = (base + tempo.sample_at(origin, rate) - source_anchor) as f64;
    let (start, end) = (clip.start, clip.start + clip.length);
    let mut cuts: Vec<Ticks> = tempo
        .points()
        .iter()
        .map(|p| p.at)
        .filter(|&at| at > start && at < end)
        .collect();
    for pair in tempo.points().windows(2) {
        if !pair[0].ramp || pair[0].bpm == pair[1].bpm {
            continue;
        }
        let (from, to) = (pair[0].at.max(start), pair[1].at.min(end));
        let mut at = Ticks((from.0 / PPQ + 1) * PPQ);
        while at < to && cuts.len() < MAX_REPEATS {
            cuts.push(at);
            at = at + Ticks(PPQ);
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    let period = clip.loop_length.filter(|p| *p > Ticks::ZERO);
    let cycle = period.and_then(|period| {
        let origin = start - clip.loop_phase;
        let origin_frame = tempo.sample_at(origin, rate);
        let frames = tempo.sample_at(origin + period, rate) - origin_frame;
        (frames > 0).then_some((origin_frame, frames))
    });
    if let Some((origin, period)) = cycle {
        let start_frame = tempo.sample_at(start, rate);
        let phase = (start_frame - origin).rem_euclid(period);
        let end_frame = tempo.sample_at(end, rate);
        let mut frame = start_frame + period - phase;
        while frame < end_frame && cuts.len() < MAX_REPEATS {
            cuts.push(tempo.ticks_at_sample(frame, rate));
            frame += period;
        }
        cuts.sort_unstable();
        cuts.dedup();
    }
    cuts.push(end);
    let gain = 10f32.powf(gain_db / 20.0) * if clip.inverted { -1.0 } else { 1.0 };
    let (curve_in, curve_out) = (clip.fade_in.curve.into(), clip.fade_out.curve.into());
    let fade_start = start - clip.fade_in.offset;
    let fade_end = end + clip.fade_out.offset;
    let (curve_in, fade_in) = if model.crossfade && !blend {
        (curve_in, [beats(start.as_quarters()); 2])
    } else {
        (
            curve_in,
            [
                beats(fade_start.as_quarters()),
                beats((fade_start + clip.fade_in.span.max(clip.fade_in.length)).as_quarters()),
            ],
        )
    };
    let fade_out = [
        beats((fade_end - clip.fade_out.span.max(clip.fade_out.length)).as_quarters()),
        beats(fade_end.as_quarters()),
    ];
    let mut from = start;
    for to in cuts {
        let length = (to - from).as_quarters();
        if length <= 0.0 {
            continue;
        }
        let frames = (tempo.sample_at(to, rate) - tempo.sample_at(from, rate)) as f64;
        let from_frame = tempo.sample_at(from, rate);
        let source_frame = cycle.map_or(
            from_frame - tempo.sample_at(start, rate),
            |(origin, period)| (from_frame - origin).rem_euclid(period),
        );
        let first_frame = offset + source_frame as f64;
        waves.push(Wave {
            instance: WaveInstance {
                start: beats(from.as_quarters()),
                end: beats(to.as_quarters()),
                row,
                slot: source.slot,
                first_frame: first_frame as f32,
                frames_per_beat: (frames / length) as f32,
                color: rgba(r, g, b, a),
                channels: source.channels,
                gain,
                curve_in,
                curve_out,
                fade_in,
                fade_out,
            },
            geometry: Geometry {
                media: key,
                row,
                start: from.as_quarters(),
                first_frame,
                frames_per_beat: frames / length,
                frames,
            },
        });
        from = to;
    }
}

fn warped_waves(waves: &mut Vec<Wave>, rate: u32, shown: &Shown<'_>, key: u64, source: &Source) {
    let Shown {
        model,
        clip,
        blend,
        row,
        color,
    } = *shown;
    let Some(warp) = &clip.warp else {
        return;
    };
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let ClipContent::Audio { gain_db, .. } = &clip.content else {
        return;
    };
    let period = clip.loop_length.filter(|p| *p > Ticks::ZERO);
    let origin = if period.is_some() {
        clip.start - clip.loop_phase
    } else {
        clip.start
    };
    let (start, end) = (clip.start, clip.start + clip.length);
    let mut cuts: Vec<Ticks> = Vec::new();
    let turns = |cuts: &mut Vec<Ticks>, within: Ticks| {
        let mut at = origin + within;
        match period {
            Some(period) => {
                if at <= start {
                    at = at + Ticks((start - at).0 / period.0 * period.0 + period.0);
                }
                while at < end && cuts.len() < MAX_REPEATS {
                    if at > start {
                        cuts.push(at);
                    }
                    at = at + period;
                }
            }
            None if at > start && at < end => cuts.push(at),
            None => {}
        }
    };
    for marker in warp.markers.iter().take(MAX_REPEATS) {
        let within = match period {
            Some(period) => Ticks(marker.at.0.rem_euclid(period.0)),
            None => marker.at,
        };
        turns(&mut cuts, within);
    }
    if period.is_some() {
        turns(&mut cuts, Ticks::ZERO);
    }
    cuts.push(end);
    cuts.sort_unstable();
    cuts.dedup();
    let gain = 10f32.powf(gain_db / 20.0) * if clip.inverted { -1.0 } else { 1.0 };
    let (curve_in, curve_out) = (clip.fade_in.curve.into(), clip.fade_out.curve.into());
    let fade_start = start - clip.fade_in.offset;
    let fade_end = end + clip.fade_out.offset;
    let (curve_in, fade_in) = if model.crossfade && !blend {
        (curve_in, [beats(start.as_quarters()); 2])
    } else {
        (
            curve_in,
            [
                beats(fade_start.as_quarters()),
                beats((fade_start + clip.fade_in.span.max(clip.fade_in.length)).as_quarters()),
            ],
        )
    };
    let fade_out = [
        beats((fade_end - clip.fade_out.span.max(clip.fade_out.length)).as_quarters()),
        beats(fade_end.as_quarters()),
    ];
    let device_rate = f64::from(rate);
    let mut from = start;
    for to in cuts {
        let length = (to - from).as_quarters();
        if length <= 0.0 {
            continue;
        }
        let local = |at: Ticks| match period {
            Some(period) => (at - origin).0.rem_euclid(period.0) as f64,
            None => (at - origin).0 as f64,
        };
        let begins = local(from);
        let ends = begins + (to - from).0 as f64;
        let first = lumen_core::warp::source_at_f64(&warp.markers, warp.original_bpm, begins);
        let last = lumen_core::warp::source_at_f64(&warp.markers, warp.original_bpm, ends);
        let first_frame = first * device_rate;
        let frames = (last - first) * device_rate;
        waves.push(Wave {
            instance: WaveInstance {
                start: beats(from.as_quarters()),
                end: beats(to.as_quarters()),
                row,
                slot: source.slot,
                first_frame: first_frame as f32,
                frames_per_beat: (frames / length) as f32,
                color: rgba(r, g, b, a),
                channels: source.channels,
                gain,
                curve_in,
                curve_out,
                fade_in,
                fade_out,
            },
            geometry: Geometry {
                media: key,
                row,
                start: from.as_quarters(),
                first_frame,
                frames_per_beat: frames / length,
                frames,
            },
        });
        from = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::TempoPoint;
    use lumen_project::model::{Clip, Color, Id, Track, TrackKind};
    use std::sync::Arc;

    fn project(tempo: Vec<TempoPoint>, offset: i64) -> Project {
        let mut project = crate::empty_project();
        project.tempo = tempo;
        project.tracks.push(Arc::new(Track {
            clips: vec![Arc::new(Clip {
                id: Id(2),
                name: "C".into(),
                start: Ticks::quarters(4),
                length: Ticks::quarters(4),
                muted: false,
                color: None,
                locked: false,
                crossfade: false,
                fade_in: Default::default(),
                fade_out: Default::default(),
                loop_length: None,
                loop_phase: Default::default(),
                envelopes: Default::default(),
                audio_phase_seconds: 0.0,
                reversed: false,
                inverted: false,
                takes: None,
                warp: None,
                content: ClipContent::Audio {
                    media: Id(3),
                    offset,
                    gain_db: 0.0,
                },
            })],
            ..Track::new(Id(1), "A", TrackKind::Audio, Color(0x4C7A9E))
        }));
        project
    }

    fn waves(project: &Project, native_rate: u32) -> Vec<WaveInstance> {
        let waves = wave_instances(
            project,
            48_000,
            |media| {
                (media == 3).then_some(Source {
                    slot: 5,
                    channels: 2,
                    native_rate,
                })
            },
            |_, _| Color32::WHITE,
        );
        waves.into_iter().map(|w| w.instance).collect()
    }

    fn bpm(at: i64, bpm: f64) -> TempoPoint {
        TempoPoint {
            at: Ticks::quarters(at),
            bpm,
            ramp: false,
        }
    }

    #[test]
    fn a_clip_at_one_tempo_is_one_wave_from_its_offset() {
        let waves = waves(&project(vec![bpm(0, 120.0)], 44_100), 44_100);
        assert_eq!(waves.len(), 1);
        let w = waves[0];
        assert_eq!(
            (w.start, w.end, w.row, w.slot, w.channels),
            ([4.0, 0.0], [8.0, 0.0], 0, 5, 2)
        );
        assert_eq!(w.first_frame, 48_000.0);
        assert_eq!(w.frames_per_beat, 24_000.0);
    }

    fn warped(mut project: Project, warp: lumen_project::model::Warp) -> Project {
        let track = Arc::make_mut(&mut project.tracks[0]);
        Arc::make_mut(&mut track.clips[0]).warp = Some(warp);
        project
    }

    #[test]
    fn a_warped_clip_draws_the_file_at_the_speed_the_map_plays_it() {
        let tempo = vec![bpm(0, 120.0), bpm(6, 60.0)];
        let p = warped(
            project(tempo, 0),
            lumen_project::model::Warp::new(90.0, 0.0),
        );
        let waves = waves(&p, 48_000);
        assert_eq!(waves.len(), 1);
        assert_eq!((waves[0].start, waves[0].end), ([4.0, 0.0], [8.0, 0.0]));
        assert_eq!(waves[0].first_frame, 0.0);
        assert_eq!(waves[0].frames_per_beat, 32_000.0);
    }

    #[test]
    fn a_warped_clip_is_cut_at_its_markers_and_each_piece_has_its_own_speed() {
        use lumen_core::warp::Marker;
        let mut warp = lumen_project::model::Warp::new(60.0, 0.0);
        warp.markers.push(Marker::new(1.0, Ticks::quarters(2)));
        let p = warped(project(vec![bpm(0, 120.0)], 0), warp);
        let waves = waves(&p, 48_000);
        let seen: Vec<_> = waves
            .iter()
            .map(|w| (w.start[0], w.first_frame, w.frames_per_beat))
            .collect();
        assert_eq!(seen, [(4.0, 0.0, 24_000.0), (6.0, 48_000.0, 48_000.0)]);
    }

    #[test]
    fn a_warped_loop_far_into_its_turns_is_drawn_without_walking_them() {
        use lumen_core::warp::Marker;
        let mut warp = lumen_project::model::Warp::new(120.0, 0.0);
        warp.markers.push(Marker::new(0.25, Ticks::quarters(1)));
        let mut p = warped(project(vec![bpm(0, 120.0)], 0), warp);
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
        clip.loop_length = Some(Ticks::quarters(2));
        clip.loop_phase = Ticks::quarters(4_000_000_000);
        let began = std::time::Instant::now();
        let waves = waves(&p, 48_000);
        assert!(began.elapsed().as_secs() < 2, "it walked the turns");
        assert!(!waves.is_empty() && waves.len() <= 64);
        assert_eq!(waves[0].start[0], 4.0);
    }

    #[test]
    fn a_looping_clip_draws_its_content_again_from_each_repeat() {
        let mut p = project(vec![bpm(0, 120.0)], 1_000);
        let track = Arc::make_mut(&mut p.tracks[0]);
        Arc::make_mut(&mut track.clips[0]).loop_length = Some(Ticks::quarters(1));
        let waves = waves(&p, 48_000);
        let spans: Vec<_> = waves.iter().map(|w| (w.start[0], w.first_frame)).collect();
        assert_eq!(
            spans,
            [
                (4.0, 1_000.0),
                (5.0, 1_000.0),
                (6.0, 1_000.0),
                (7.0, 1_000.0)
            ]
        );
    }

    #[test]
    fn a_tempo_change_inside_a_clip_splits_it_and_the_audio_runs_on() {
        let waves = waves(&project(vec![bpm(0, 120.0), bpm(6, 60.0)], 0), 48_000);
        assert_eq!(waves.len(), 2);
        assert_eq!((waves[0].start, waves[0].end), ([4.0, 0.0], [6.0, 0.0]));
        assert_eq!((waves[1].start, waves[1].end), ([6.0, 0.0], [8.0, 0.0]));
        assert_eq!(waves[0].first_frame, 0.0);
        assert_eq!(waves[0].frames_per_beat, 24_000.0);
        assert_eq!(waves[1].first_frame, 48_000.0);
        assert_eq!(waves[1].frames_per_beat, 48_000.0);
    }

    #[test]
    fn a_cut_inside_a_repeat_draws_the_remaining_source_before_starting_the_next_cycle() {
        let mut p = project(vec![bpm(0, 120.0)], 1_000);
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
        clip.loop_length = Some(Ticks::quarters(2));
        clip.loop_phase = Ticks::quarters(1);
        let w = waves(&p, 48_000);
        let spans: Vec<_> = w
            .iter()
            .map(|w| (w.start[0], w.end[0], w.first_frame))
            .collect();
        assert_eq!(
            spans,
            [
                (4.0, 5.0, 25_000.0),
                (5.0, 7.0, 1_000.0),
                (7.0, 8.0, 1_000.0)
            ]
        );
    }

    #[test]
    fn a_cut_fade_keeps_the_original_waveform_curve_coordinates() {
        let mut p = project(vec![bpm(0, 120.0)], 0);
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
        clip.fade_in = lumen_core::fade::Fade {
            length: Ticks::quarters(1),
            offset: Ticks::quarters(1),
            span: Ticks::quarters(2),
            ..Default::default()
        };
        let w = waves(&p, 48_000);
        assert_eq!(w[0].fade_in, [[3.0, 0.0], [5.0, 0.0]]);
    }

    #[test]
    fn resampled_split_waveforms_keep_the_original_absolute_sample_clock() {
        let mut p = project(vec![bpm(0, 123.0)], 37);
        let tempo = TempoMap::new(p.tempo.clone()).unwrap();
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
        clip.audio_phase_seconds =
            tempo.seconds_at(Ticks::quarters(5)) - tempo.seconds_at(clip.start);
        clip.start = Ticks::quarters(5);
        let w = waves(&p, 44_100);
        let expected = 40 + tempo.sample_at(Ticks::quarters(5), 48_000)
            - tempo.sample_at(Ticks::quarters(4), 48_000);
        assert_eq!(w[0].first_frame, expected as f32);
    }

    #[test]
    fn a_clip_over_a_tempo_ramp_has_a_wave_a_beat_and_the_audio_runs_on() {
        let ramp = TempoPoint {
            ramp: true,
            ..bpm(0, 120.0)
        };
        let p = project(vec![ramp, bpm(8, 60.0)], 0);
        let waves = waves(&p, 48_000);
        let starts: Vec<f32> = waves.iter().map(|w| w.start[0]).collect();
        assert_eq!(starts, [4.0, 5.0, 6.0, 7.0]);
        for pair in waves.windows(2) {
            assert!(pair[1].frames_per_beat > pair[0].frames_per_beat);
            let drawn = pair[0].frames_per_beat * (pair[0].end[0] - pair[0].start[0]);
            assert!((pair[0].first_frame + drawn - pair[1].first_frame).abs() < 1.0);
        }
    }

    #[test]
    fn a_crossfade_draws_the_incoming_clip_from_the_start_of_the_crossing() {
        let mut p = project(vec![bpm(0, 120.0)], 24_000);
        p.media.push(lumen_project::model::Media {
            id: Id(3),
            path: "a.wav".into(),
            original_path: None,
            frames: 48_000 * 8,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 0,
        });
        let track = Arc::make_mut(&mut p.tracks[0]);
        let mut left = (*track.clips[0]).clone();
        left.id = Id(1);
        left.start = Ticks::ZERO;
        left.content = ClipContent::Audio {
            media: Id(3),
            offset: 0,
            gain_db: 0.0,
        };
        track.clips.insert(0, Arc::new(left));
        let right = Arc::make_mut(&mut track.clips[1]);
        right.crossfade = true;
        right.fade_in.length = Ticks(Ticks::quarters(1).0 / 2);
        let waves = waves(&p, 48_000);
        assert_eq!(waves.len(), 2);
        let (out, inc) = (waves[0], waves[1]);
        assert_eq!(inc.start, [3.0, 0.5]);
        assert_eq!(inc.end, [8.0, 0.0]);
        assert_eq!(inc.fade_in, [[3.0, 0.5], [4.0, 0.0]]);
        assert_eq!(inc.first_frame, 12_000.0);
        assert_eq!(out.fade_out, [[3.0, 0.5], [4.0, 0.0]]);
    }

    #[test]
    fn clips_without_loaded_peaks_have_no_wave() {
        let project = project(vec![bpm(0, 120.0)], 0);
        let none = wave_instances(&project, 48_000, |_| None, |_, _| Color32::WHITE);
        assert!(none.is_empty());
    }
}
