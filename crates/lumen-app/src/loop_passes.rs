//! Cutting a loop recording into passes (M9 slice R3).
//!
//! With Loop on, the take worker writes one file across the wraps and notes
//! where each pass begins (`take_worker::Wrap`). This turns that into the
//! clip's passes: the clip sits over the loop's start and plays the last
//! pass; every earlier pass is the same file seen from another frame.
//!
//! A clip cannot begin before its file, so a first pass that began part-way
//! into the loop is not offered as a take; its audio stays in the file. When
//! fewer than two passes are left the recording is one clip, as without Loop.

use crate::take_worker::Wrap;

/// How far from the loop's start the first pass may begin and still be a
/// whole pass, as a fraction of a second.
const START_SLACK: u32 = 50;

/// The passes of a take, ready to place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cut {
    /// The placed sample where the clip begins: the loop's start, not before
    /// the song.
    pub start: i64,
    /// The frames the last pass lasts: how long the clip is.
    pub frames: u64,
    /// For each offered pass, in order, the file frame at the clip's start.
    pub passes: Vec<i64>,
}

/// Cuts a take that began at placed sample `start`, holds `frames` and saw
/// `wraps`. `None` when there are not two passes to offer.
pub(crate) fn cut(start: i64, frames: u64, wraps: &[Wrap], rate: u32) -> Option<Cut> {
    let last = wraps.last()?;
    let mut begins: Vec<(u64, i64)> = Vec::with_capacity(wraps.len() + 1);
    if (start - last.at).unsigned_abs() <= u64::from(rate / START_SLACK) {
        begins.push((0, start));
    }
    begins.extend(wraps.iter().map(|w| (w.frame, w.at)));
    let clip_start = last.at.max(0);
    let passes: Vec<i64> = begins
        .iter()
        .map(|&(frame, at)| (frame as i64 + (clip_start - at)).max(0))
        .collect();
    let played = frames
        .checked_sub(*passes.last()? as u64)
        .filter(|&f| f > 0)?;
    (begins.len() >= 2).then_some(Cut {
        start: clip_start,
        frames: played,
        passes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn wrap(frame: u64, at: i64) -> Wrap {
        Wrap { frame, at }
    }

    #[test]
    fn a_recording_that_never_wrapped_is_one_clip() {
        assert_eq!(cut(0, 96_000, &[], RATE), None);
    }

    #[test]
    fn passes_from_the_loop_start_are_cut_where_the_file_wrapped() {
        // A 1-second loop at sample 10_000; recorded for two and a half passes.
        let wraps = [wrap(48_000, 10_000), wrap(96_000, 10_000)];
        let cut = cut(10_000, 120_000, &wraps, RATE).unwrap();
        assert_eq!(cut.start, 10_000);
        assert_eq!(cut.frames, 24_000, "the half pass is the last");
        assert_eq!(cut.passes, [0, 48_000, 96_000]);
    }

    #[test]
    fn a_loop_at_the_start_of_the_song_skips_the_latency_in_every_pass() {
        // The loop starts at sample 0 and the take is shifted 300 samples
        // earlier, so each pass begins at -300 and its first 300 frames are
        // before the song. The first file frame is at placed 0.
        let wraps = [wrap(47_700, -300), wrap(95_700, -300)];
        let cut = cut(0, 143_700, &wraps, RATE).unwrap();
        assert_eq!(cut.start, 0);
        // Pass 0 (frame 0 is at sample 0), pass 1 (47_700 is at -300: skip
        // 300), pass 2 likewise.
        assert_eq!(cut.passes, [0, 48_000, 96_000]);
        assert_eq!(
            cut.frames, 47_700,
            "the last pass loses what was before the song"
        );
    }

    #[test]
    fn a_first_pass_that_began_inside_the_loop_is_not_offered() {
        let wraps = [wrap(30_000, 10_000), wrap(78_000, 10_000)];
        // It began 18_000 samples into the loop.
        let cut = cut(28_000, 100_000, &wraps, RATE).unwrap();
        assert_eq!(cut.passes, [30_000, 78_000]);
        assert_eq!(cut.start, 10_000);
        assert_eq!(cut.frames, 22_000);
    }

    #[test]
    fn one_whole_pass_is_not_a_set() {
        // Began inside the loop and wrapped once: only one pass is whole.
        let wraps = [wrap(30_000, 10_000)];
        assert_eq!(cut(28_000, 60_000, &wraps, RATE), None);
    }

    #[test]
    fn two_passes_are_enough_and_a_file_that_ends_at_the_wrap_is_not() {
        let wraps = [wrap(48_000, 0)];
        let cut = cut(0, 60_000, &wraps, RATE).unwrap();
        assert_eq!(cut.passes, [0, 48_000]);
        assert_eq!(super::cut(0, 48_000, &wraps, RATE), None);
    }
}
