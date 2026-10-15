//! Recording MIDI: what armed tracks are sent while the transport records
//! (M5 slice 6, ADR-0055).
//!
//! The player keeps every live message a recording track takes, with the
//! timeline sample the player *heard* when playing it, and queues it for the
//! app, which turns the samples into ticks and commits them to the project.
//! The sample is worked out here, where the position of each part is known:
//! it is the time the message was played (the device frame the feed read
//! from the driver's stamp, before the feed's one block of delay), taken
//! back to the timeline the part renders, less the read ahead, so a note
//! lands where the playhead was when its key went down. The output
//! latency, which the app knows, is taken off by the app.

use lumen_core::midi::Msg;
use lumen_core::time::Ticks;

use crate::source::RenderContext;

/// Messages waiting for the app. A player holds a few hundred at most, so
/// this is room for seconds of a dense take if the UI stalls.
pub(super) const QUEUE: usize = 8_192;

/// The `source` of a marker the player queues at each loop wrap, for every
/// recording track, instead of a message: it carries the new pass and no
/// message. A note held across a wrap is told from one played again by it.
pub const WRAP: u8 = u8::MAX;

/// Applies latency compensation to a captured position, keeping the first
/// pass at the Record boundary and carrying later compensated positions
/// across the loop seam when needed. Returns the adjusted position and its
/// owning pass.
pub fn record_position(
    heard: Ticks,
    compensated: Ticks,
    pass: u32,
    first_pass: u32,
    start: Ticks,
    looped: Option<(Ticks, Ticks)>,
) -> (Ticks, u32) {
    let mut at = compensated;
    let mut pass = pass;
    if pass != first_pass
        && let Some((from, to)) = looped
        && at < from
        && (from..to).contains(&heard)
    {
        at = Ticks(at.0 + (to - from).0);
        pass = pass.wrapping_sub(1);
    }
    if pass == first_pass {
        at = at.max(start);
    }
    (at, pass)
}

/// A live message kept for the take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Captured {
    /// The track recording it.
    pub track: u64,
    /// The input's slot, or the keyboard; [`WRAP`] for a wrap marker.
    pub source: u8,
    /// The timeline sample heard when it was played: never before 0, and
    /// inside the loop when it was played just before a wrap.
    pub at: i64,
    /// The pass through the loop it was played in: it counts up at every
    /// wrap and never goes back, so it is the same for the messages of one
    /// pass (and one less than the playing pass for a message played just
    /// before a wrap). Only differences mean anything.
    pub pass: u32,
    pub msg: Msg,
}

/// The timeline sample heard at device frame `played`, for a part `cx`, and
/// whether it is of the pass before this part's (played just before a wrap).
/// `wrapped` is true only after a loop wrap since the last discontinuity.
pub(super) fn heard(cx: &RenderContext<'_>, played: u64, wrapped: bool) -> (i64, bool) {
    // The render position leads what is heard by the read ahead.
    let ahead = cx.stream.saturating_sub(cx.device) as i64;
    let heard_now = cx.start - ahead;
    let mut at = heard_now + (played as i64 - cx.device as i64);
    let mut back = false;
    if let Some((start, end)) = cx.looping
        && at < start
        && heard_now >= start
        && wrapped
    {
        at += end - start;
        back = true;
    }
    (at.max(0), back)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Mode;
    use crate::timeline::Timeline;

    fn at(cx: &RenderContext<'_>, played: u64) -> i64 {
        heard(cx, played, true).0
    }

    fn context<'a>(
        timeline: &'a Timeline,
        start: i64,
        device: u64,
        stream: u64,
        looping: Option<(i64, i64)>,
    ) -> RenderContext<'a> {
        RenderContext {
            timeline,
            sample_rate: 48_000,
            start,
            tail: false,
            stopping: false,
            mode: Mode::Play,
            stream,
            device,
            recording: true,
            looping,
        }
    }

    #[test]
    fn a_message_played_in_this_part_lands_at_its_frame() {
        let timeline = Timeline::default();
        let cx = context(&timeline, 10_000, 5_000, 5_000, None);
        assert_eq!(at(&cx, 5_000), 10_000);
        assert_eq!(at(&cx, 5_100), 10_100);
    }

    #[test]
    fn a_message_played_a_block_ago_lands_a_block_back() {
        let timeline = Timeline::default();
        let cx = context(&timeline, 10_000, 5_000, 5_000, None);
        assert_eq!(at(&cx, 4_872), 9_872);
    }

    #[test]
    fn the_read_ahead_is_taken_off() {
        // The part renders 480 frames ahead of what is heard.
        let timeline = Timeline::default();
        let cx = context(&timeline, 10_480, 5_000, 5_480, None);
        assert_eq!(at(&cx, 5_000), 10_000);
    }

    #[test]
    fn a_note_played_before_a_loop_wrap_lands_at_the_loop_end() {
        // The loop is 8_000 to 16_000; the part begins at its start, and
        // the note was played 100 frames earlier.
        let timeline = Timeline::default();
        let cx = context(&timeline, 8_000, 5_000, 5_000, Some((8_000, 16_000)));
        assert_eq!(at(&cx, 4_900), 15_900);
        // Not wrapped when the part is not at the loop start.
        let cx = context(&timeline, 9_000, 5_000, 5_000, Some((8_000, 16_000)));
        assert_eq!(at(&cx, 4_900), 8_900);
    }

    #[test]
    fn only_a_note_played_before_a_wrap_belongs_to_the_pass_before() {
        let timeline = Timeline::default();
        let cx = context(&timeline, 8_000, 5_000, 5_000, Some((8_000, 16_000)));
        assert_eq!(heard(&cx, 4_900, true), (15_900, true));
        assert_eq!(heard(&cx, 5_100, true), (8_100, false));
    }

    #[test]
    fn a_message_before_loop_start_stays_before_it_until_a_wrap_happens() {
        let timeline = Timeline::default();
        let cx = context(&timeline, 8_000, 5_000, 5_000, Some((8_000, 16_000)));
        assert_eq!(heard(&cx, 4_900, false), (7_900, false));
        assert_eq!(heard(&cx, 4_900, true), (15_900, true));
    }

    #[test]
    fn never_before_the_start_of_the_song() {
        let timeline = Timeline::default();
        let cx = context(&timeline, 50, 5_000, 5_000, None);
        assert_eq!(at(&cx, 4_000), 0);
    }
}
