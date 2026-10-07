//! Waveforms for the GPU (M3 slice 4c, ADR-0027): which audio each clip
//! shows, from which frame and at how many frames per beat. Kept pure, so
//! the mapping from the project to what is drawn is tested, like the one
//! to what is heard (`lanes::engine_clips`).

use eframe::egui::Color32;
use lumen_core::time::{TempoMap, Ticks};
use lumen_project::model::{ClipContent, Project};
use lumen_render::{WaveInstance, rgba};

/// What the waveform of a media file needs: its atlas slot, how many
/// channels to show, and the file's own sample rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    pub slot: u32,
    pub channels: u32,
    pub native_rate: u32,
}

/// The waves for every audio clip whose peaks are loaded (`source` gives
/// them by media id), with frames at the device `rate` as the audio is
/// held. A clip spanning tempo changes gets one wave per constant tempo,
/// since audio plays at its own speed while beats stretch.
pub fn wave_instances(
    project: &Project,
    rate: u32,
    source: impl Fn(u64) -> Option<Source>,
    color: impl Fn(usize) -> Color32,
) -> Vec<WaveInstance> {
    let Ok(tempo) = TempoMap::new(project.tempo.clone()) else {
        return Vec::new();
    };
    let mut waves = Vec::new();
    for (row, track) in project.tracks.iter().enumerate() {
        let [r, g, b, a] = color(row).to_srgba_unmultiplied();
        for clip in &track.clips {
            let ClipContent::Audio { media, offset, .. } = &clip.content else {
                continue;
            };
            let Some(source) = source(media.0) else {
                continue;
            };
            // The file offset in the device-rate frames the peaks index.
            let offset = *offset as f64 * f64::from(rate) / f64::from(source.native_rate.max(1));
            let (start, end) = (clip.start, clip.start + clip.length);
            let at_start = tempo.sample_at(start, rate);
            let mut cuts: Vec<Ticks> = tempo
                .points()
                .iter()
                .map(|p| p.at)
                .filter(|&at| at > start && at < end)
                .collect();
            cuts.push(end);
            let mut from = start;
            for to in cuts {
                let beats = (to - from).as_quarters();
                if beats <= 0.0 {
                    continue;
                }
                let frames = (tempo.sample_at(to, rate) - tempo.sample_at(from, rate)) as f64;
                waves.push(WaveInstance {
                    start: from.as_quarters() as f32,
                    end: to.as_quarters() as f32,
                    row: row as u32,
                    slot: source.slot,
                    first_frame: (offset + (tempo.sample_at(from, rate) - at_start) as f64) as f32,
                    frames_per_beat: (frames / beats) as f32,
                    color: rgba(r, g, b, a),
                    channels: source.channels,
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
            id: Id(1),
            name: "A".into(),
            kind: TrackKind::Audio,
            color: Color(0x4C7A9E),
            parent: None,
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            devices: Vec::new(),
            clips: vec![Arc::new(Clip {
                id: Id(2),
                name: "C".into(),
                start: Ticks::quarters(4),
                length: Ticks::quarters(4),
                muted: false,
                content: ClipContent::Audio {
                    media: Id(3),
                    offset,
                    gain_db: 0.0,
                },
            })],
        }));
        project
    }

    fn waves(project: &Project, native_rate: u32) -> Vec<WaveInstance> {
        wave_instances(
            project,
            48_000,
            |media| {
                (media == 3).then_some(Source {
                    slot: 5,
                    channels: 2,
                    native_rate,
                })
            },
            |_| Color32::WHITE,
        )
    }

    fn bpm(at: i64, bpm: f64) -> TempoPoint {
        TempoPoint {
            at: Ticks::quarters(at),
            bpm,
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
            (4.0, 8.0, 0, 5, 2)
        );
        assert_eq!(w.first_frame, 48_000.0);
        assert_eq!(w.frames_per_beat, 24_000.0);
    }

    #[test]
    fn a_tempo_change_inside_a_clip_splits_it_and_the_audio_runs_on() {
        // 120 BPM, then 60 BPM from beat 6: the second half's beats are
        // twice as long, and its audio starts where the first half's ended.
        let waves = waves(&project(vec![bpm(0, 120.0), bpm(6, 60.0)], 0), 48_000);
        assert_eq!(waves.len(), 2);
        assert_eq!((waves[0].start, waves[0].end), (4.0, 6.0));
        assert_eq!((waves[1].start, waves[1].end), (6.0, 8.0));
        assert_eq!(waves[0].first_frame, 0.0);
        assert_eq!(waves[0].frames_per_beat, 24_000.0);
        assert_eq!(waves[1].first_frame, 48_000.0);
        assert_eq!(waves[1].frames_per_beat, 48_000.0);
    }

    #[test]
    fn clips_without_loaded_peaks_have_no_wave() {
        let project = project(vec![bpm(0, 120.0)], 0);
        let none = wave_instances(&project, 48_000, |_| None, |_| Color32::WHITE);
        assert!(none.is_empty());
    }
}
