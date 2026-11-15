use lumen_core::fade::Fade;
use lumen_core::time::{PPQ, TempoMap, Ticks};
use lumen_project::model::Clip;

use crate::arrangement::waves::{self, Source, Span};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WholeFile {
    pub spans: Vec<Span>,
    pub from: f64,
    pub to: f64,
    pub loop_window: (f64, f64),
    pub played: Played,
    pub heard: (f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Played {
    pub first: (f64, f64),
    pub second: Option<(f64, f64)>,
}

impl Played {
    fn one(part: (f64, f64)) -> Self {
        Self {
            first: part,
            second: None,
        }
    }

    pub fn parts(self) -> impl Iterator<Item = (f64, f64)> {
        std::iter::once(self.first).chain(self.second)
    }

    pub fn contains(self, q: f64) -> bool {
        self.parts().any(|(a, b)| q >= a && q < b)
    }

    pub fn extent(self) -> (f64, f64) {
        (self.first.0, self.second.unwrap_or(self.first).1)
    }
}

impl WholeFile {
    pub fn span_at(&self, q: f64) -> Option<&Span> {
        let i = self.spans.partition_point(|s| s.end <= q);
        self.spans.get(i).filter(|s| s.start <= q)
    }

    pub fn beat_of_frame(&self, frame: f64) -> Option<f64> {
        self.spans.iter().find_map(|s| {
            let frames = (s.end - s.start) * s.frames_per_beat;
            (frame >= s.first_frame && frame < s.first_frame + frames)
                .then(|| s.start + (frame - s.first_frame) / s.frames_per_beat)
        })
    }

    pub fn frame_at(&self, q: f64) -> Option<f64> {
        let span = self.span_at(q)?;
        Some(span.first_frame + (q - span.start) * span.frames_per_beat)
    }
}

struct Turn {
    first: i64,
    frames: i64,
    phase: i64,
}

fn turn_of(clip: &Clip, tempo: &TempoMap, rate: u32) -> Option<Turn> {
    let period = clip.loop_length.filter(|p| *p > Ticks::ZERO)?;
    let clock = clip.start - clip.loop_phase;
    let frames = tempo.sample_at(clock + period, rate) - tempo.sample_at(clock, rate);
    if frames <= 0 {
        return None;
    }
    let start = tempo.sample_at(clip.start, rate);
    let phase = (start - tempo.sample_at(clock, rate)).rem_euclid(frames);
    Some(Turn {
        first: start - phase,
        frames,
        phase,
    })
}

pub(crate) fn whole_file(
    clip: &Clip,
    tempo: &TempoMap,
    rate: u32,
    frames: u64,
    source: impl Fn(u64) -> Option<Source> + Copy,
) -> Option<WholeFile> {
    let beat = |sample: i64| tempo.ticks_at_sample(sample, rate);
    let end = clip.start + clip.length;
    let turn = turn_of(clip, tempo, rate);
    let origin = turn.as_ref().map_or(clip.start, |t| beat(t.first));
    let (loop_window, played, heard) = match &turn {
        None => {
            let own = (clip.start.as_quarters(), end.as_quarters());
            (own, Played::one(own), own)
        }
        Some(turn) => {
            let turn_end = beat(turn.first + turn.frames);
            let window = (origin.as_quarters(), turn_end.as_quarters());
            let length = tempo.sample_at(end, rate) - tempo.sample_at(clip.start, rate);
            let played = if length >= turn.frames {
                Played::one(window)
            } else if turn.phase + length <= turn.frames {
                Played::one((clip.start.as_quarters(), end.as_quarters()))
            } else {
                let wrapped = beat(turn.first + turn.phase + length - turn.frames);
                Played {
                    first: (origin.as_quarters(), wrapped.as_quarters()),
                    second: Some((clip.start.as_quarters(), turn_end.as_quarters())),
                }
            };
            let heard = (clip.start.as_quarters(), end.min(turn_end).as_quarters());
            (window, played, heard)
        }
    };
    let (from, to, mut whole) = match &clip.warp {
        Some(warp) => {
            let seconds = frames as f64 / f64::from(rate.max(1));
            let at = |second: f64| {
                origin + lumen_core::warp::tick_at_source(&warp.markers, warp.original_bpm, second)
            };
            let (from, to) = (at(0.0).max(Ticks::ZERO), at(seconds).max(Ticks::ZERO));
            if to <= from {
                return None;
            }
            let mut whole = plain(clip);
            if let Some(map) = whole.warp.as_mut() {
                lumen_core::warp::shift(&mut map.markers, origin - from);
            }
            (from, to, whole)
        }
        None => {
            let mut probe = plain(clip);
            probe.start = origin;
            probe.length = Ticks(PPQ);
            let at_origin = waves::spans_of(&probe, tempo, rate, source)
                .first()?
                .first_frame;
            let origin_sample = tempo.sample_at(origin, rate);
            let first_sample = (origin_sample as f64 - at_origin).round() as i64;
            let last_sample = first_sample + frames as i64;
            let from = tempo.ticks_at_sample(first_sample.max(0), rate);
            let to = tempo.ticks_at_sample(last_sample.max(0), rate);
            if to <= from {
                return None;
            }
            let mut whole = plain(clip);
            whole.audio_phase_seconds =
                clip.audio_phase_seconds - (tempo.seconds_at(origin) - tempo.seconds_at(from));
            (from, to, whole)
        }
    };
    whole.start = from;
    whole.length = to - from;
    let spans = waves::spans_of(&whole, tempo, rate, source);
    Some(WholeFile {
        from: spans.first()?.start,
        to: spans.last()?.end,
        spans,
        loop_window,
        played,
        heard,
    })
}

fn plain(clip: &Clip) -> Clip {
    let mut copy = clip.clone();
    copy.loop_length = None;
    copy.loop_phase = Ticks::ZERO;
    copy.fade_in = Fade::default();
    copy.fade_out = Fade::default();
    copy.crossfade = false;
    copy
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::TempoPoint;
    use lumen_project::model::{ClipContent, Id};

    const RATE: u32 = 48_000;
    const FRAMES_PER_BEAT: f64 = 24_000.0;

    fn tempo(bpm: f64) -> TempoMap {
        TempoMap::new(vec![TempoPoint {
            at: Ticks::ZERO,
            bpm,
            ramp: false,
        }])
        .expect("a tempo map")
    }

    fn clip(start: i64, length: i64, offset: i64) -> Clip {
        Clip {
            id: Id(1),
            name: "Loop".into(),
            start: Ticks::quarters(start),
            length: Ticks::quarters(length),
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
                media: Id(9),
                offset,
                gain_db: 0.0,
            },
        }
    }

    fn source(_: u64) -> Option<Source> {
        Some(Source {
            slot: 0,
            channels: 2,
            native_rate: RATE,
        })
    }

    #[test]
    fn a_warped_clip_shows_the_file_at_the_speed_the_map_plays_it() {
        let mut clip = clip(4, 8, 0);
        clip.warp = Some(lumen_project::model::Warp::new(90.0, 0.0));
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        let a = file.frame_at(4.0).expect("at the start");
        let b = file.frame_at(5.0).expect("a beat on");
        assert!(a.abs() < 1.0, "{a}");
        assert!((b - a - 32_000.0).abs() < 1.0, "{}", b - a);
        assert!((file.to - 4.0 - 15.0).abs() < 1e-6, "{}", file.to);
    }

    #[test]
    fn a_trimmed_warped_clip_shows_its_file_around_the_part_it_plays() {
        let map = TempoMap::new(vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(10),
                bpm: 60.0,
                ramp: false,
            },
        ])
        .expect("a tempo map");
        let mut clip = clip(8, 4, 144_000);
        clip.warp = Some(lumen_project::model::Warp::new(90.0, 3.0));
        let file = whole_file(&clip, &map, RATE, 480_000, source).expect("the file");
        assert!((file.from - 3.5).abs() < 1e-3, "{}", file.from);
        assert!((file.to - 18.5).abs() < 1e-3, "{}", file.to);
        let start = file.frame_at(8.0).expect("the clip's start");
        assert!((start - 144_000.0).abs() < 1.0, "{start}");
        let beat = file.frame_at(9.0).expect("a beat on") - start;
        assert!((beat - 32_000.0).abs() < 1.0, "{beat}");
        let after = file.frame_at(13.0).expect("after the tempo change") - start;
        assert!((after - 5.0 * 32_000.0).abs() < 1.0, "{after}");
    }

    #[test]
    fn a_marker_changes_the_speed_the_editor_draws_after_it() {
        use lumen_core::warp::Marker;
        let mut warp = lumen_project::model::Warp::new(60.0, 0.0);
        warp.markers.push(Marker::new(1.0, Ticks::quarters(2)));
        let mut clip = clip(4, 8, 0);
        clip.warp = Some(warp);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        let at = |beat: f64| file.frame_at(beat).expect("in the file");
        assert!((at(5.0) - 24_000.0).abs() < 1.0);
        assert!((at(6.0) - 48_000.0).abs() < 1.0);
        assert!((at(7.0) - 96_000.0).abs() < 1.0);
        assert!((file.to - (6.0 + 9.0)).abs() < 1e-3, "{}", file.to);
    }

    #[test]
    fn a_frame_of_the_file_is_drawn_where_the_beat_for_it_is() {
        let clip = clip(8, 4, 48_000);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert!((file.beat_of_frame(48_000.0).unwrap() - 8.0).abs() < 1e-9);
        assert!((file.beat_of_frame(0.0).unwrap() - 6.0).abs() < 1e-9);
        assert!((file.beat_of_frame(60_000.0).unwrap() - 8.5).abs() < 1e-9);
        for frame in [0.0, 123_456.0, 479_999.0] {
            let beat = file.beat_of_frame(frame).unwrap();
            assert!((file.frame_at(beat).unwrap() - frame).abs() < 1e-6);
        }
        assert_eq!(file.beat_of_frame(-1.0), None);
        assert_eq!(file.beat_of_frame(480_000.0), None, "past the last frame");
    }

    #[test]
    fn the_file_starts_before_the_clip_by_its_offset() {
        let clip = clip(8, 4, 48_000);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert!((file.from - 6.0).abs() < 1e-6, "{}", file.from);
        assert!((file.to - 26.0).abs() < 1e-6, "{}", file.to);
        assert_eq!(file.loop_window, (8.0, 12.0));
        assert_eq!(file.played, Played::one((8.0, 12.0)));
        assert_eq!(file.heard, (8.0, 12.0));
        let at_start = file.frame_at(8.0).expect("in the file");
        assert!((at_start - 48_000.0).abs() < 1.0, "{at_start}");
        assert!((file.frame_at(6.0).expect("frame 0") - 0.0).abs() < 1.0);
    }

    #[test]
    fn a_file_that_would_start_before_the_song_is_cut_at_the_song_start() {
        let clip = clip(1, 4, 96_000);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert_eq!(file.from, 0.0);
        let at_zero = file.frame_at(0.0).expect("in the file");
        assert!((at_zero - 72_000.0).abs() < 1.0, "{at_zero}");
    }

    #[test]
    fn a_tempo_change_is_followed_frame_for_frame() {
        let map = TempoMap::new(vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(10),
                bpm: 60.0,
                ramp: false,
            },
        ])
        .expect("a tempo map");
        let clip = clip(8, 4, 0);
        let file = whole_file(&clip, &map, RATE, 240_000, source).expect("the file");
        let a = file.frame_at(10.0).expect("at 10");
        let b = file.frame_at(11.0).expect("at 11");
        assert!((a - 2.0 * FRAMES_PER_BEAT).abs() < 1.0, "{a}");
        assert!((b - a - 2.0 * FRAMES_PER_BEAT).abs() < 1.0, "{}", b - a);
        assert!((file.to - 14.0).abs() < 1e-6, "{}", file.to);
    }

    #[test]
    fn a_looping_clip_shows_its_loop_from_the_loop_origin() {
        let mut clip = clip(8, 16, 0);
        clip.loop_length = Some(Ticks::quarters(4));
        clip.loop_phase = Ticks::quarters(1);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert_eq!(file.loop_window, (7.0, 11.0));
        assert_eq!(file.played, Played::one((7.0, 11.0)));
        assert_eq!(file.heard, (8.0, 11.0));
        assert!((file.frame_at(7.0).expect("origin") - 0.0).abs() < 1.0);
    }

    #[test]
    fn a_clip_with_a_phase_keeps_its_frames_at_their_times() {
        let mut clip = clip(8, 4, 0);
        clip.audio_phase_seconds = 0.25;
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        let at_start = file.frame_at(8.0).expect("in the file");
        assert!((at_start - 12_000.0).abs() < 1.0, "{at_start}");
        assert!((file.from - 7.5).abs() < 1e-6, "{}", file.from);
    }

    #[test]
    fn a_reversed_clip_reads_its_reversed_twin() {
        let mut clip = clip(0, 4, 0);
        clip.reversed = true;
        let twin = crate::media::reversed_key(9);
        let asked = std::cell::Cell::new(None);
        let file = whole_file(&clip, &tempo(120.0), RATE, 48_000, |key| {
            asked.set(Some(key));
            source(key)
        })
        .expect("the file");
        assert_eq!(asked.get(), Some(twin));
        assert!(file.spans.iter().all(|s| s.media == twin));
    }

    #[test]
    fn a_looping_clip_shorter_than_its_loop_shows_only_what_it_plays() {
        let mut clip = clip(8, 2, 0);
        clip.loop_length = Some(Ticks::quarters(4));
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert_eq!(file.loop_window, (8.0, 12.0));
        assert_eq!(file.played, Played::one((8.0, 10.0)));
        assert_eq!(file.heard, (8.0, 10.0));
    }

    #[test]
    fn a_phase_at_a_whole_turn_is_the_loop_from_the_clips_start() {
        let mut clip = clip(8, 16, 0);
        clip.loop_length = Some(Ticks::quarters(4));
        clip.loop_phase = Ticks::quarters(4);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert_eq!(file.loop_window, (8.0, 12.0));
        assert_eq!(file.heard, (8.0, 12.0));
        let at_start = file.frame_at(8.0).expect("in the file");
        assert!(at_start.abs() < 1.0, "the turn's first frame: {at_start}");
    }

    #[test]
    fn a_clip_split_past_its_first_repeat_shows_the_frame_it_plays() {
        let mut clip = clip(13, 6, 0);
        clip.loop_length = Some(Ticks::quarters(2));
        clip.loop_phase = Ticks::quarters(5);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert_eq!(file.loop_window, (12.0, 14.0));
        assert_eq!(file.played, Played::one((12.0, 14.0)));
        assert_eq!(file.heard, (13.0, 14.0));
        let at_start = file.frame_at(13.0).expect("in the file");
        assert!((at_start - FRAMES_PER_BEAT).abs() < 1.0, "{at_start}");
    }

    #[test]
    fn the_origin_is_worked_out_in_frames_across_a_tempo_change() {
        let map = TempoMap::new(vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(4),
                bpm: 60.0,
                ramp: false,
            },
        ])
        .expect("a tempo map");
        let mut clip = clip(8, 4, 0);
        clip.loop_length = Some(Ticks::quarters(4));
        clip.loop_phase = Ticks::quarters(8);
        let file = whole_file(&clip, &map, RATE, 480_000, source).expect("the file");
        assert_eq!(file.loop_window, (8.0, 10.0));
        assert_eq!(file.heard, (8.0, 10.0));
        assert!(file.frame_at(8.0).expect("in the file").abs() < 1.0);
    }

    #[test]
    fn a_clip_shorter_than_a_turn_that_wraps_plays_two_parts() {
        let mut clip = clip(8, 3, 0);
        clip.loop_length = Some(Ticks::quarters(4));
        clip.loop_phase = Ticks(PPQ * 5 / 2);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert_eq!(file.loop_window, (5.5, 9.5));
        assert_eq!(
            file.played,
            Played {
                first: (5.5, 7.0),
                second: Some((8.0, 9.5)),
            }
        );
        assert_eq!(file.played.extent(), (5.5, 9.5));
        assert!(file.played.contains(6.0) && file.played.contains(9.0));
        assert!(!file.played.contains(7.5));
        assert_eq!(file.heard, (8.0, 9.5));
    }

    #[test]
    fn spans_are_found_by_beat() {
        let clip = clip(0, 4, 0);
        let file = whole_file(&clip, &tempo(120.0), RATE, 48_000, source).expect("the file");
        assert!(file.span_at(1.0).is_some());
        assert!(file.span_at(2.5).is_none(), "past the file's one second");
        assert!(file.frame_at(-1.0).is_none());
    }

    #[test]
    fn a_midi_clip_has_no_file() {
        let mut clip = clip(0, 4, 0);
        clip.content = ClipContent::midi();
        assert!(whole_file(&clip, &tempo(120.0), RATE, 48_000, source).is_none());
    }
}
