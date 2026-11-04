//! Where an audio clip's whole file sits on the timeline (M9.5 S1): the
//! audio editor draws the file from its first frame to its last, at the
//! place and speed the clip plays it, with the part the clip plays bright.
//! Pure, so the mapping is tested like the arrangement's waves
//! (`arrangement::waves`), which it reuses: the file is the waves of a copy
//! of the clip stretched to the file's ends.

use lumen_core::fade::Fade;
use lumen_core::time::{PPQ, TempoMap, Ticks};
use lumen_project::model::Clip;

use crate::arrangement::waves::{self, Source, Span};

/// The file of an audio clip, laid on the timeline.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WholeFile {
    /// The file from its first frame to its last (or from the song's start,
    /// for a file that would begin before it), one span per tempo, in time
    /// order; frames at the device rate.
    pub spans: Vec<Span>,
    /// The beats the file covers: `spans`' first start and last end.
    pub from: f64,
    pub to: f64,
    /// The beats the clip plays: its start and end; a looping clip's first
    /// repeat (its loop brace), from the loop's origin.
    pub window: (f64, f64),
}

impl WholeFile {
    /// The span under beat `q`, if any.
    pub fn span_at(&self, q: f64) -> Option<&Span> {
        let i = self.spans.partition_point(|s| s.end <= q);
        self.spans.get(i).filter(|s| s.start <= q)
    }

    /// The device-rate frame of the file at beat `q` (clamped to the span
    /// it falls in), if the file is there.
    pub fn frame_at(&self, q: f64) -> Option<f64> {
        let span = self.span_at(q)?;
        Some(span.first_frame + (q - span.start) * span.frames_per_beat)
    }
}

/// The whole file of `clip` on the timeline: its file is `frames` long at
/// the device `rate`, and `source` says where its peaks are. `None` when the
/// clip is not audio, its peaks are not loaded or the tempo map is broken.
pub(crate) fn whole_file(
    clip: &Clip,
    tempo: &TempoMap,
    rate: u32,
    frames: u64,
    source: impl Fn(u64) -> Option<Source> + Copy,
) -> Option<WholeFile> {
    // The clip's content starts at its origin: its start, or for a looping
    // clip the start of the loop it is part way through.
    let origin = match clip.loop_length {
        Some(period) if period > Ticks::ZERO => clip.start - clip.loop_phase,
        _ => clip.start,
    };
    let window_end = match clip.loop_length {
        Some(period) if period > Ticks::ZERO => origin + period,
        _ => clip.start + clip.length,
    };
    // The frame the clip plays at its origin: the first wave of a copy that
    // starts there and does not loop.
    let mut probe = plain(clip);
    probe.start = origin;
    probe.length = Ticks(PPQ);
    let at_origin = waves::spans_of(&probe, tempo, rate, source)
        .first()?
        .first_frame;
    // Frame 0 of the file is that many frames before the origin.
    let origin_sample = tempo.sample_at(origin, rate);
    let first_sample = (origin_sample as f64 - at_origin).round() as i64;
    let last_sample = first_sample + frames as i64;
    let from = tempo.ticks_at_sample(first_sample.max(0), rate);
    let to = tempo.ticks_at_sample(last_sample.max(0), rate);
    if to <= from {
        return None;
    }
    // A copy of the clip from the file's first frame to its last, playing
    // the same frames at the same times: its start moves earlier, so its
    // phase moves by the seconds between.
    let mut whole = plain(clip);
    whole.audio_phase_seconds =
        clip.audio_phase_seconds - (tempo.seconds_at(origin) - tempo.seconds_at(from));
    whole.start = from;
    whole.length = to - from;
    let spans = waves::spans_of(&whole, tempo, rate, source);
    Some(WholeFile {
        from: spans.first()?.start,
        to: spans.last()?.end,
        spans,
        window: (origin.as_quarters(), window_end.as_quarters()),
    })
}

/// `clip` with nothing that changes how its file maps to time: no loop, no
/// fades, no crossfade.
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
    /// One second is two beats at 120 BPM.
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
    fn the_file_starts_before_the_clip_by_its_offset() {
        // The clip starts at beat 8 and plays from frame 48,000 (two beats
        // in): the file begins at beat 6 and its ten seconds end at beat 26.
        let clip = clip(8, 4, 48_000);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert!((file.from - 6.0).abs() < 1e-6, "{}", file.from);
        assert!((file.to - 26.0).abs() < 1e-6, "{}", file.to);
        assert_eq!(file.window, (8.0, 12.0));
        // The clip's own start shows the frame it plays there.
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
        // 120 BPM to beat 10, then 60: the file plays at its own speed, so
        // past beat 10 a beat holds twice the frames.
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
        // Five seconds from beat 8: one second (two beats) to beat 10 at
        // 120, then four seconds (four beats) at 60.
        assert!((file.to - 14.0).abs() < 1e-6, "{}", file.to);
    }

    #[test]
    fn a_looping_clip_shows_its_loop_from_the_loop_origin() {
        let mut clip = clip(8, 16, 0);
        clip.loop_length = Some(Ticks::quarters(4));
        clip.loop_phase = Ticks::quarters(1);
        let file = whole_file(&clip, &tempo(120.0), RATE, 480_000, source).expect("the file");
        assert_eq!(file.window, (7.0, 11.0));
        assert!((file.frame_at(7.0).expect("origin") - 0.0).abs() < 1.0);
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
