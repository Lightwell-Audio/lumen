//! Cutting an audio recording into the clip it places (M9 slice R3,
//! ADR-0104).
//!
//! The take worker writes one file across the loop's wraps and notes where
//! each pass begins (`take_worker::Wrap`). A take begun inside the loop
//! begins with silence back to the loop's start, so every pass is the same
//! shape: one loop of the file, seen from its own frame. One rule then makes
//! the clip:
//!
//! - **A pass** is the loop's length of the file from the frame that sits at
//!   the loop's start. The latency puts that frame a little after the wrap,
//!   so a pass's last moments are the head of the next one: what was played
//!   while the loop's end was heard. What a pass holds is what was played in
//!   it: one begun late holds silence before, one stopped early ends there.
//! - **Passes worth choosing** hold at least a quarter of a second (or the
//!   whole loop, if shorter). The rest stay in the file.
//! - **Two or more** make one clip exactly over the loop with a take for
//!   each; it plays the last whole pass, or with none whole the one that
//!   holds most.
//! - **One** is a plain clip over just what it holds, where it was played.
//! - **What was recorded before the loop** (a take begun before it with punch
//!   in off) is a clip of its own, where it was played (`Cut::lead`).
//!
//! Without Loop, or before the first wrap, the take is one stretch of the
//! timeline from where it began. The loop is the one recording began with,
//! carried by the recording, so moving it afterwards changes nothing.

use crate::take_worker::Wrap;

/// How far from the loop's start the file may begin and still begin there,
/// as a fraction of a second.
const START_SLACK: u32 = 50;

/// The shortest pass worth offering as a take, as a fraction of a second.
const SHORTEST: u32 = 4;

/// A finished recording, as the worker wrote it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Recorded<'a> {
    /// The placed sample of the file's first frame.
    pub start: i64,
    /// Frames of silence the file begins with, lining it up with the loop.
    pub silence: u64,
    /// Frames in the file, the silence included.
    pub frames: u64,
    pub wraps: &'a [Wrap],
    /// The loop it was recorded in, as timeline samples, with Loop on.
    pub looped: Option<(i64, i64)>,
}

/// The clip a recording places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cut {
    /// The placed sample where the clip begins.
    pub start: i64,
    /// The frames the clip lasts.
    pub frames: u64,
    /// For each pass offered, in order, the file frame at the clip's start.
    /// One is a plain clip that plays from that frame.
    pub passes: Vec<i64>,
    /// The pass the clip plays.
    pub active: usize,
    /// What was recorded before the loop: the placed sample it begins at and
    /// its frames, to be placed as a clip of its own from the file's start.
    pub lead: Option<(i64, u64)>,
}

/// One pass: the file frame at the loop's start, and the file frames it
/// holds sound for.
#[derive(Debug, Clone, Copy)]
struct Pass {
    at_start: i64,
    sound: (i64, i64),
}

impl Pass {
    fn held(self) -> u64 {
        (self.sound.1 - self.sound.0).max(0) as u64
    }
}

/// Cuts `take`, recorded at `rate`.
pub(crate) fn cut(take: Recorded<'_>, rate: u32) -> Cut {
    let silence = take.silence.min(take.frames) as i64;
    let frames = take.frames as i64;
    let (Some(last), Some((loop_start, loop_end))) = (take.wraps.last(), take.looped) else {
        return stretch(take.start, silence, frames);
    };
    let length = (loop_end - loop_start).max(1);
    let slack = i64::from(rate / START_SLACK);
    let pass = |at_start: i64| {
        let at_start = at_start.max(0);
        Pass {
            at_start,
            sound: (at_start.max(silence), (at_start + length).min(frames)),
        }
    };
    // How long before the loop's start, as the wraps place it, the file
    // began: about nothing for a take begun at it (or lined up with it).
    let before = last.at - take.start;
    let mut passes: Vec<Pass> = Vec::with_capacity(take.wraps.len() + 1);
    // The file's own first pass, unless it began inside the loop with no
    // silence to line it up (a take from before ADR-0104's fifth run).
    if before >= -slack {
        passes.push(pass(loop_start - take.start));
    }
    passes.extend(
        take.wraps
            .iter()
            .map(|w| pass(w.frame as i64 + loop_start - w.at)),
    );
    let lead = (before > slack).then(|| (take.start, (loop_start - take.start).max(0) as u64));
    let worth = (length as u64).min(u64::from(rate / SHORTEST));
    let mut offered: Vec<Pass> = passes
        .iter()
        .copied()
        .filter(|p| p.held() >= worth)
        .collect();
    if offered.is_empty() {
        // Nothing long enough: keep what holds the most, so nothing played
        // in the loop is left out.
        offered.extend(passes.iter().copied().max_by_key(|p| p.held()));
    }
    let Some(&only) = offered.first() else {
        return stretch(take.start, silence, frames);
    };
    if offered.len() == 1 {
        // A plain clip over what the pass holds, where it was played.
        return Cut {
            start: loop_start + (only.sound.0 - only.at_start),
            frames: only.held(),
            passes: vec![only.sound.0],
            active: 0,
            lead,
        };
    }
    // Whole: begun at the loop's start and played to its end (the last
    // pass may lack the latency's few frames after the stop).
    let whole = |p: &Pass| p.sound.0 == p.at_start && p.held() as i64 + slack >= length;
    let active = offered.iter().rposition(whole).unwrap_or_else(|| {
        // None is whole: the one that holds most, the later on a tie.
        let most = offered.iter().map(|p| p.held()).max().unwrap_or(0);
        offered.iter().rposition(|p| p.held() == most).unwrap_or(0)
    });
    Cut {
        start: loop_start,
        frames: length as u64,
        passes: offered.iter().map(|p| p.at_start).collect(),
        active,
        lead,
    }
}

/// One stretch of the timeline from where the sound began.
fn stretch(start: i64, silence: i64, frames: i64) -> Cut {
    Cut {
        start: start + silence,
        frames: (frames - silence).max(0) as u64,
        passes: vec![silence],
        active: 0,
        lead: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    /// A loop of one second from sample 10_000.
    const LOOP: (i64, i64) = (10_000, 58_000);
    /// The latency: a wrap is placed this much before the loop's start.
    const SHIFT: i64 = 300;
    /// Recording began at the loop's start: the file's first frame is placed
    /// where the wraps are.
    const AT_LOOP: i64 = LOOP.0 - SHIFT;

    /// A wrap at file `frame`, placed where the loop's start is heard.
    fn wrap(frame: u64) -> Wrap {
        Wrap { frame, at: AT_LOOP }
    }

    fn cut(start: i64, silence: u64, frames: u64, wraps: &[Wrap]) -> Cut {
        cut_in(LOOP, start, silence, frames, wraps)
    }

    fn cut_in(looped: (i64, i64), start: i64, silence: u64, frames: u64, wraps: &[Wrap]) -> Cut {
        super::cut(
            Recorded {
                start,
                silence,
                frames,
                wraps,
                looped: Some(looped),
            },
            RATE,
        )
    }

    #[test]
    fn a_recording_that_never_wrapped_is_one_stretch_from_where_it_began() {
        let c = cut(500, 0, 96_000, &[]);
        assert_eq!(
            (c.start, c.frames, c.passes, c.lead),
            (500, 96_000, vec![0], None)
        );
        // Begun inside the loop and stopped before the wrap: the silence that
        // lines the file up is not part of the clip.
        let c = cut(AT_LOOP, 18_000, 30_000, &[]);
        assert_eq!((c.start, c.frames), (AT_LOOP + 18_000, 12_000));
        assert_eq!(c.passes, [18_000]);
    }

    #[test]
    fn passes_sit_exactly_on_the_loop_and_end_in_the_head_of_the_next() {
        // Two passes and a half.
        let wraps = [wrap(48_000), wrap(96_000)];
        let c = cut(AT_LOOP, 0, 120_000, &wraps);
        assert_eq!((c.start, c.frames), (LOOP.0, 48_000), "exactly the loop");
        // Each pass's frame at the loop's start is the latency after its wrap.
        assert_eq!(c.passes, [SHIFT, 48_000 + SHIFT, 96_000 + SHIFT]);
        assert_eq!(c.active, 1, "the last whole pass plays, not the half");
        assert_eq!(c.lead, None, "the latency is no lead-in");
    }

    #[test]
    fn stopping_at_the_end_of_a_pass_plays_the_last_pass() {
        let wraps = [wrap(48_000), wrap(96_000)];
        let c = cut(AT_LOOP, 0, 144_000, &wraps);
        assert_eq!(c.passes.len(), 3);
        assert_eq!(c.active, 2, "only the latency's frames are missing from it");
    }

    #[test]
    fn a_sliver_after_the_last_wrap_is_not_a_take_and_never_the_clip() {
        // Stopped 0.1 s into the third pass (the owner's first run).
        let wraps = [wrap(48_000), wrap(96_000)];
        let c = cut(AT_LOOP, 0, 96_000 + 4_800, &wraps);
        assert_eq!(c.frames, 48_000);
        assert_eq!(
            c.passes,
            [SHIFT, 48_000 + SHIFT],
            "0.1 s is too short to choose"
        );
        assert_eq!(c.active, 1);
    }

    #[test]
    fn a_loop_at_the_start_of_the_song_is_the_whole_loop_from_sample_zero() {
        // The file cannot begin before the song: it begins at 0, the latency
        // after where the wraps place the loop's start.
        let wraps = [
            Wrap {
                frame: 47_700,
                at: -SHIFT,
            },
            Wrap {
                frame: 95_700,
                at: -SHIFT,
            },
        ];
        let c = cut_in((0, 48_000), 0, 0, 143_700, &wraps);
        assert_eq!((c.start, c.frames), (0, 48_000));
        assert_eq!(c.passes, [0, 48_000, 96_000]);
        assert_eq!(c.active, 2);
        assert_eq!(c.lead, None);
    }

    #[test]
    fn a_first_pass_begun_inside_the_loop_is_a_take_with_silence_before() {
        // The owner's fifth run: recording began 18_000 into the loop, so the
        // file begins with 18_000 frames of silence and wraps at 48_000 like
        // any pass.
        let wraps = [wrap(48_000), wrap(96_000)];
        let c = cut(AT_LOOP, 18_000, 120_000, &wraps);
        assert_eq!((c.start, c.frames), (LOOP.0, 48_000));
        assert_eq!(c.passes, [SHIFT, 48_000 + SHIFT, 96_000 + SHIFT]);
        assert_eq!(c.active, 1, "the last whole pass plays, not the late one");
        assert_eq!(c.lead, None);
    }

    #[test]
    fn with_no_whole_pass_the_one_that_holds_most_plays() {
        // Began 18_000 in (about 30_000 played), wrapped, stopped 20_000 into
        // the second pass.
        let wraps = [wrap(48_000)];
        let c = cut(AT_LOOP, 18_000, 68_000, &wraps);
        assert_eq!(c.passes.len(), 2);
        assert_eq!(c.active, 0, "30_000 frames beat 20_000");
    }

    #[test]
    fn begun_inside_the_loop_and_stopped_just_after_the_wrap_is_a_plain_clip() {
        // 30_000 frames before the wrap, 2_000 after: the sliver is dropped
        // and the first pass is placed over what it holds.
        let wraps = [wrap(48_000)];
        let c = cut(AT_LOOP, 18_000, 50_000, &wraps);
        assert_eq!(c.passes, [18_000]);
        // It runs to the loop's end, the latency's frames after the wrap in.
        assert_eq!(
            (c.start, c.frames),
            (LOOP.0 + 18_000 - SHIFT, 30_000 + SHIFT as u64)
        );
    }

    #[test]
    fn begun_at_the_very_end_of_the_loop_a_short_first_pass_is_left_in_the_file() {
        // Began 47_000 into the loop: 1_000 frames, then two whole passes.
        let wraps = [wrap(48_000), wrap(96_000)];
        let c = cut(AT_LOOP, 47_000, 144_000, &wraps);
        assert_eq!(c.passes, [48_000 + SHIFT, 96_000 + SHIFT]);
        assert_eq!(c.active, 1);
    }

    #[test]
    fn a_take_that_began_before_the_loop_keeps_what_it_played_before_as_a_lead_in() {
        // Recording began at sample 0, 10_000 before the loop's start.
        let wraps = [wrap(58_000 - SHIFT as u64), wrap(106_000 - SHIFT as u64)];
        let c = cut(0, 0, 120_000, &wraps);
        assert_eq!(
            (c.start, c.frames),
            (LOOP.0, 48_000),
            "the loop, not the lead-in"
        );
        assert_eq!(c.passes, [10_000, 58_000, 106_000]);
        assert_eq!(c.active, 1);
        assert_eq!(
            c.lead,
            Some((0, 10_000)),
            "up to the loop's start, where it abuts"
        );
    }

    #[test]
    fn one_wrap_from_before_the_loop_makes_one_plain_clip_over_the_loop() {
        let wraps = [wrap(58_000 - SHIFT as u64)];
        // Stopped a hair into the second pass, which is dropped.
        let c = cut(0, 0, 59_000, &wraps);
        assert_eq!((c.start, c.frames), (LOOP.0, 48_000));
        assert_eq!(c.passes, [10_000], "one pass is no take");
        assert_eq!(c.lead, Some((0, 10_000)));
    }

    #[test]
    fn a_file_from_before_the_silence_that_began_inside_the_loop_keeps_its_later_passes() {
        // No silence: the first pass cannot be lined up and stays in the file.
        let wraps = [wrap(30_000), wrap(78_000)];
        let c = cut(AT_LOOP + 18_000, 0, 120_000, &wraps);
        assert_eq!(c.passes, [30_000 + SHIFT, 78_000 + SHIFT]);
        assert_eq!((c.start, c.frames, c.active), (LOOP.0, 48_000, 0));
    }

    #[test]
    fn without_the_loop_a_wrapped_file_is_one_stretch() {
        let wraps = [wrap(48_000)];
        let c = super::cut(
            Recorded {
                start: 0,
                silence: 0,
                frames: 60_000,
                wraps: &wraps,
                looped: None,
            },
            RATE,
        );
        assert_eq!((c.start, c.frames, c.passes), (0, 60_000, vec![0]));
    }
}
