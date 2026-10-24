//! Waveforms for the GPU (M3 slice 4c, ADR-0027): which audio each clip
//! shows, from which frame and at how many frames per beat. Kept pure, so
//! the mapping from the project to what is drawn is tested, like the one
//! to what is heard (`lanes::engine_clips`).

use eframe::egui::Color32;
use lumen_core::time::{PPQ, TempoMap, Ticks};
use lumen_project::model::{Clip, ClipContent, Project, Track};
use lumen_render::{WaveInstance, beats, rgba};

use super::samples::Geometry;

/// The most repeats of one looping clip drawn as waves; past this, a very
/// short loop over a long clip shows its first repeats only.
const MAX_REPEATS: usize = 4_096;

/// What the waveform of a media file needs: its atlas slot, how many
/// channels to show, and the file's own sample rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    pub slot: u32,
    pub channels: u32,
    pub native_rate: u32,
}

/// A wave for the GPU, and its exact place for drawing sample by sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Wave {
    pub instance: WaveInstance,
    pub geometry: Geometry,
}

/// The waves for every audio clip whose peaks are loaded (`source` gives
/// them by media id), with frames at the device `rate` as the audio is
/// held. A clip spanning tempo changes gets one wave per constant tempo
/// (and one per quarter note over a ramp), since audio plays at its own
/// speed while beats stretch.
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
    for (row, track) in project.tracks.iter().enumerate() {
        for clip in &track.clips {
            let [r, g, b, a] = color(track, clip).to_srgba_unmultiplied();
            let ClipContent::Audio {
                media,
                offset,
                gain_db,
            } = &clip.content
            else {
                continue;
            };
            // A reversed clip draws the reversed twin of its file.
            let key = if clip.reversed {
                crate::media::reversed_key(media.0)
            } else {
                media.0
            };
            let Some(source) = source(key) else {
                continue;
            };
            // The file offset in the device-rate frames the peaks index.
            let base =
                (*offset as f64 * f64::from(rate) / f64::from(source.native_rate.max(1))) as i64;
            let origin = if clip.loop_length.is_some() {
                clip.start - clip.loop_phase
            } else {
                clip.start
            };
            let source_anchor = ((tempo.seconds_at(origin) - clip.audio_phase_seconds)
                * f64::from(rate))
            .round() as i64;
            let offset = (base + tempo.sample_at(origin, rate) - source_anchor) as f64;
            let (start, end) = (clip.start, clip.start + clip.length);
            let mut cuts: Vec<Ticks> = tempo
                .points()
                .iter()
                .map(|p| p.at)
                .filter(|&at| at > start && at < end)
                .collect();
            // Over a ramp the audio stretches a little more or less each beat:
            // one wave per quarter note there, exact where each begins.
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
            // A looping clip draws its content again from each repeat
            // (slice 6c), as the engine plays it.
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
            // Drawn at the clip's gain and through its fades (slice 6d),
            // as the engine plays it.
            // A polarity-inverted clip is drawn with a negative gain, upside down.
            let gain = 10f32.powf(gain_db / 20.0) * if clip.inverted { -1.0 } else { 1.0 };
            let (curve_in, curve_out) = (clip.fade_in.curve.into(), clip.fade_out.curve.into());
            let fade_start = start - clip.fade_in.offset;
            let fade_end = end + clip.fade_out.offset;
            let fade_in = [
                beats(fade_start.as_quarters()),
                beats((fade_start + clip.fade_in.span.max(clip.fade_in.length)).as_quarters()),
            ];
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
                        row: row as u32,
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
                        row: row as u32,
                        start: from.as_quarters(),
                        first_frame,
                        frames_per_beat: frames / length,
                        frames,
                    },
                });
                from = to;
            }
        }
    }
    waves
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
        // 120 BPM: 24,000 frames per beat at 48 kHz. The file is 44.1 kHz,
        // so an offset of 44,100 file frames is 48,000 device frames.
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
        // 120 BPM, then 60 BPM from beat 6: the second half's beats are
        // twice as long, and its audio starts where the first half's ended.
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
        // 120 BPM gliding to 60 over beats 0 to 8; the clip is beats 4 to 8.
        let ramp = TempoPoint {
            ramp: true,
            ..bpm(0, 120.0)
        };
        let p = project(vec![ramp, bpm(8, 60.0)], 0);
        let waves = waves(&p, 48_000);
        let starts: Vec<f32> = waves.iter().map(|w| w.start[0]).collect();
        assert_eq!(starts, [4.0, 5.0, 6.0, 7.0]);
        // Each beat is longer than the one before, and the audio of a wave
        // begins where the one before it ended.
        for pair in waves.windows(2) {
            assert!(pair[1].frames_per_beat > pair[0].frames_per_beat);
            let drawn = pair[0].frames_per_beat * (pair[0].end[0] - pair[0].start[0]);
            assert!((pair[0].first_frame + drawn - pair[1].first_frame).abs() < 1.0);
        }
    }

    #[test]
    fn clips_without_loaded_peaks_have_no_wave() {
        let project = project(vec![bpm(0, 120.0)], 0);
        let none = wave_instances(&project, 48_000, |_| None, |_, _| Color32::WHITE);
        assert!(none.is_empty());
    }
}
