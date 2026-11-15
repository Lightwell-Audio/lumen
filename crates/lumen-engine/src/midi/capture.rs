use lumen_core::midi::Msg;
use lumen_core::time::Ticks;

use crate::source::RenderContext;

pub(super) const QUEUE: usize = 8_192;

pub const WRAP: u8 = u8::MAX;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    Recording,
    Playing,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Captured {
    pub track: u64,
    pub source: u8,
    pub at: i64,
    pub pass: u32,
    pub frame: u64,
    pub state: Heard,
    pub msg: Msg,
}

pub(super) fn heard(cx: &RenderContext<'_>, played: u64, wrapped: bool) -> (i64, bool) {
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
            deadline: None,
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
        let timeline = Timeline::default();
        let cx = context(&timeline, 10_480, 5_000, 5_480, None);
        assert_eq!(at(&cx, 5_000), 10_000);
    }

    #[test]
    fn a_note_played_before_a_loop_wrap_lands_at_the_loop_end() {
        let timeline = Timeline::default();
        let cx = context(&timeline, 8_000, 5_000, 5_000, Some((8_000, 16_000)));
        assert_eq!(at(&cx, 4_900), 15_900);
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
