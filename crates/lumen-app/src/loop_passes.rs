//! Cutting a loop recording into passes (M9 slice R3).
//!
//! With Loop on, the take worker writes one file across the wraps and notes
//! where each pass begins (`take_worker::Wrap`). This turns that into the
//! clip's passes: the clip sits over the loop (one pass long, from the loop's
//! start) and plays the last whole pass; every other pass is the same file
//! seen from another frame. Stopping part-way through a pass leaves a short
//! last pass: it is offered as a take, and plays up to where it stopped, if
//! it is long enough to be worth choosing.
//!
//! What a take recorded before the loop's start is placed as a clip of its
//! own, where it was played.
//!
//! A clip cannot begin before its file, so a first pass that began part-way
//! into the loop is not offered as a take; its audio stays in the file. When
//! fewer than two passes are left the recording is one clip, as without Loop.

use crate::take_worker::Wrap;

/// How far from the loop's start the first pass may begin and still be a
/// whole pass, as a fraction of a second.
const START_SLACK: u32 = 50;

/// The shortest last pass worth offering as a take, as a fraction of a second.
const SHORTEST_TAIL: u32 = 4;

/// The passes of a take, ready to place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cut {
    /// The placed sample where the clip begins: the loop's start, not before
    /// the song.
    pub start: i64,
    /// The frames the clip lasts: one pass, or less when the only pass there
    /// is stopped short.
    pub frames: u64,
    /// For each offered pass, in order, the file frame at the clip's start.
    pub passes: Vec<i64>,
    /// The pass the clip plays: the last one that lasts the whole loop.
    pub active: usize,
    /// What a take that began before the loop recorded before it: the placed
    /// sample it begins at and its frames, to be placed as a clip of its own.
    pub lead: Option<(i64, u64)>,
}

/// Cuts a take that began at placed sample `start`, holds `frames` and saw
/// `wraps`, in a loop of `loop_frames`. `None` when the take never wrapped.
pub(crate) fn cut(
    start: i64,
    frames: u64,
    wraps: &[Wrap],
    rate: u32,
    loop_frames: u64,
) -> Option<Cut> {
    let last = wraps.last()?;
    // A pass lasts as long as the loop: the file says so once it has wrapped
    // twice, and the loop's own length until then.
    let length = match wraps {
        [.., before, after] => after.frame.checked_sub(before.frame).filter(|&f| f > 0)?,
        _ => loop_frames.max(1),
    };
    let slack = i64::from(rate / START_SLACK);
    let mut begins: Vec<(u64, i64)> = Vec::with_capacity(wraps.len() + 1);
    // The first pass has the whole loop in it if it began at or before the
    // loop's start; what it recorded before the loop is a clip of its own.
    let first_whole = start <= last.at + slack;
    if first_whole {
        begins.push((0, start));
    }
    begins.extend(wraps.iter().map(|w| (w.frame, w.at)));
    let clip_start = last.at.max(0);
    let mut passes: Vec<i64> = begins
        .iter()
        .map(|&(frame, at)| (frame as i64 + (clip_start - at)).max(0))
        .collect();
    let left = |pass: i64| frames.saturating_sub(pass as u64);
    // The last pass is a take only if it lasted long enough to choose.
    if passes.len() > 1 && left(*passes.last()?) < length.min(u64::from(rate / SHORTEST_TAIL)) {
        passes.pop();
    }
    let active = passes
        .iter()
        .rposition(|&p| left(p) >= length)
        .unwrap_or(passes.len().checked_sub(1)?);
    let played = left(passes[active]).min(length);
    // A take began inside the loop and was stopped before a second pass: the
    // file is one stretch of the timeline from where it began.
    if !first_whole && passes.len() == 1 && played < length.min(u64::from(rate / SHORTEST_TAIL)) {
        return Some(Cut {
            start: start.max(0),
            frames: last.frame,
            passes: vec![0],
            active: 0,
            lead: None,
        });
    }
    let lead = (clip_start - start).max(0) as u64;
    let lead = (first_whole && lead > slack as u64).then_some((start.max(0), lead));
    Some(Cut {
        start: clip_start,
        frames: if passes.len() > 1 { length } else { played },
        passes,
        active,
        lead,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    /// A loop of one second.
    const LOOP: u64 = 48_000;

    fn wrap(frame: u64, at: i64) -> Wrap {
        Wrap { frame, at }
    }

    fn cut(start: i64, frames: u64, wraps: &[Wrap]) -> Option<Cut> {
        super::cut(start, frames, wraps, RATE, LOOP)
    }

    #[test]
    fn a_recording_that_never_wrapped_is_not_cut() {
        assert_eq!(cut(0, 96_000, &[]), None);
    }

    #[test]
    fn passes_from_the_loop_start_are_cut_where_the_file_wrapped() {
        // A 1-second loop at sample 10_000; recorded for two and a half passes.
        let wraps = [wrap(48_000, 10_000), wrap(96_000, 10_000)];
        let cut = cut(10_000, 120_000, &wraps).unwrap();
        assert_eq!(cut.start, 10_000);
        assert_eq!(cut.frames, 48_000, "the clip is one pass long");
        assert_eq!(cut.passes, [0, 48_000, 96_000], "the half pass is offered");
        assert_eq!(cut.active, 1, "the last whole pass plays, not the half");
    }

    #[test]
    fn stopping_at_the_end_of_a_pass_plays_the_last_pass() {
        let wraps = [wrap(48_000, 10_000), wrap(96_000, 10_000)];
        let cut = cut(10_000, 144_000, &wraps).unwrap();
        assert_eq!(cut.passes, [0, 48_000, 96_000]);
        assert_eq!(cut.active, 2);
    }

    #[test]
    fn a_sliver_after_the_last_wrap_is_not_a_take_and_never_the_clip() {
        // Stopped 0.1 s into the third pass (the owner's recording).
        let wraps = [wrap(48_000, 10_000), wrap(96_000, 10_000)];
        let cut = cut(10_000, 96_000 + 4_800, &wraps).unwrap();
        assert_eq!(cut.frames, 48_000);
        assert_eq!(cut.passes, [0, 48_000], "0.1 s is too short to choose");
        assert_eq!(cut.active, 1);
    }

    #[test]
    fn a_loop_at_the_start_of_the_song_skips_the_latency_in_every_pass() {
        // The loop starts at sample 0 and the take is shifted 300 samples
        // earlier, so each pass begins at -300 and its first 300 frames are
        // before the song. The first file frame is at placed 0.
        let wraps = [wrap(47_700, -300), wrap(95_700, -300)];
        let cut = cut(0, 143_700, &wraps).unwrap();
        assert_eq!(cut.start, 0);
        // Pass 0 (frame 0 is at sample 0), pass 1 (47_700 is at -300: skip
        // 300), pass 2 likewise.
        assert_eq!(cut.passes, [0, 48_000, 96_000]);
        assert_eq!(cut.frames, 48_000, "a pass is one loop");
        assert_eq!(cut.active, 1, "the last pass is cut short by the stop");
    }

    #[test]
    fn a_first_pass_that_began_inside_the_loop_is_not_offered() {
        let wraps = [wrap(30_000, 10_000), wrap(78_000, 10_000)];
        // It began 18_000 samples into the loop.
        let cut = cut(28_000, 120_000, &wraps).unwrap();
        assert_eq!(cut.passes, [30_000, 78_000]);
        assert_eq!(cut.start, 10_000);
        assert_eq!(cut.frames, 48_000);
        assert_eq!(cut.active, 0, "the pass after it is only partly there");
    }

    #[test]
    fn a_take_that_began_before_the_loop_keeps_the_loop_of_its_first_pass() {
        // The loop is 10_000 to 58_000; recording began at 0, so the file
        // holds 10_000 frames before the loop, then the first pass.
        let wraps = [wrap(58_000, 10_000), wrap(106_000, 10_000)];
        let cut = cut(0, 120_000, &wraps).unwrap();
        assert_eq!(cut.start, 10_000, "the clip is the loop, not the lead-in");
        assert_eq!(cut.passes, [10_000, 58_000, 106_000]);
        assert_eq!((cut.frames, cut.active), (48_000, 1));
        assert_eq!(
            cut.lead,
            Some((0, 10_000)),
            "the lead-in is a clip of its own"
        );
    }

    #[test]
    fn a_take_that_began_at_the_loop_has_no_lead_in() {
        let wraps = [wrap(48_000, 10_000), wrap(96_000, 10_000)];
        assert_eq!(cut(10_000, 120_000, &wraps).unwrap().lead, None);
    }

    #[test]
    fn one_wrap_from_before_the_loop_makes_one_plain_clip_over_the_loop() {
        let wraps = [wrap(58_000, 10_000)];
        // Stopped a hair into the second pass, which is dropped.
        let cut = cut(0, 59_000, &wraps).unwrap();
        assert_eq!(cut.start, 10_000);
        assert_eq!(cut.passes, [10_000], "one pass is no take");
        assert_eq!((cut.frames, cut.active), (48_000, 0));
    }

    #[test]
    fn one_wrap_from_inside_the_loop_places_what_there_is() {
        let wraps = [wrap(30_000, 10_000)];
        // Began 18_000 in; the pass after the wrap lasted 30_000 frames.
        let cut = cut(28_000, 60_000, &wraps).unwrap();
        assert_eq!(cut.passes, [30_000]);
        assert_eq!((cut.start, cut.frames), (10_000, 30_000));
        // Stopped almost at once after the wrap: the first stretch is kept
        // where it was played.
        let cut = cut_at(28_000, 32_000, &wraps);
        assert_eq!((cut.start, cut.frames), (28_000, 30_000));
        assert_eq!(cut.passes, [0]);
    }

    fn cut_at(start: i64, frames: u64, wraps: &[Wrap]) -> Cut {
        cut(start, frames, wraps).unwrap()
    }

    #[test]
    fn two_passes_are_enough_and_a_file_that_ends_at_the_wrap_is_one_plain_clip() {
        let wraps = [wrap(48_000, 0)];
        let cut = cut(0, 60_000, &wraps).unwrap();
        assert_eq!(cut.passes, [0, 48_000]);
        assert_eq!(cut.active, 0);
        let cut = super::cut(0, 48_000, &wraps, RATE, LOOP).unwrap();
        assert_eq!(cut.passes, [0]);
        assert_eq!((cut.start, cut.frames), (0, 48_000));
    }
}
