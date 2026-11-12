//! The pages a streamed clip reads: the source frames a block needs, which
//! of them are asked of the pool, and the samples the player reads from a
//! streamed side.

use crate::stream::{PAGE, StreamSide};

use super::Clip;

/// Where the clip player reads one channel's samples from, passed by value
/// so the loops read a slice as directly as before streaming.
pub(super) trait Samples: Copy {
    fn at(&self, frame: usize) -> Option<f32>;
}

impl Samples for &[f32] {
    #[inline(always)]
    fn at(&self, frame: usize) -> Option<f32> {
        self.get(frame).copied()
    }
}

impl Samples for &StreamSide<'_> {
    #[inline(always)]
    fn at(&self, frame: usize) -> Option<f32> {
        StreamSide::at(self, frame)
    }
}

/// The frames of `clip`'s source it plays from timeline sample `from` to
/// `to`, with the clip starting at timeline sample `start`: one span, or two
/// where its repeats wrap. Spans are clipped to the clip.
fn source_spans(clip: &Clip, start: i64, from: i64, to: i64) -> [Option<(i64, i64)>; 2] {
    let (a, b) = (from.max(start), to.min(start + clip.length));
    if a >= b {
        return [None, None];
    }
    match clip.shape.period.filter(|p| *p > 0) {
        Some(period) => {
            let inner = (a - start + clip.shape.phase).rem_euclid(period);
            let len = b - a;
            if len >= period {
                [Some((clip.offset, clip.offset + period)), None]
            } else if inner + len <= period {
                [Some((clip.offset + inner, clip.offset + inner + len)), None]
            } else {
                [
                    Some((clip.offset + inner, clip.offset + period)),
                    Some((clip.offset, clip.offset + inner + len - period)),
                ]
            }
        }
        None => [
            Some((clip.offset + a - start, clip.offset + b - start)),
            None,
        ],
    }
}

/// Calls `f` with each page of a streamed source `frames` long that `clip`
/// reads from timeline sample `span.start` to `span.end`, the clip starting
/// at timeline sample `start`, until `f` returns false; returns whether it
/// never did. Pages past the end of the source are not read and not visited.
/// Allocates nothing: the audio thread checks pages with it, and the disk
/// thread plans with it.
pub fn for_each_page(
    clip: &Clip,
    start: i64,
    span: std::ops::Range<i64>,
    frames: u64,
    f: &mut dyn FnMut(usize) -> bool,
) -> bool {
    let frames = i64::try_from(frames).unwrap_or(i64::MAX);
    let page = PAGE as i64;
    for (lo, hi) in source_spans(clip, start, span.start, span.end)
        .into_iter()
        .flatten()
    {
        let (lo, hi) = (lo.max(0), hi.min(frames));
        if lo >= hi {
            continue;
        }
        for p in lo / page..=(hi - 1) / page {
            if !f(p as usize) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_core::time::Ticks;

    use super::*;
    use crate::clips::{ClipAudio, ClipShape};

    fn clip(offset: i64, length: i64, period: Option<i64>, phase: i64) -> Clip {
        Clip {
            id: 1,
            track: 0,
            start: Ticks::ZERO,
            offset,
            length,
            gain: 1.0,
            audio: Arc::new(ClipAudio::Memory {
                sample_rate: 48_000,
                channels: vec![],
            }),
            shape: ClipShape {
                period,
                phase,
                ..Default::default()
            },
            envelope: Default::default(),
        }
    }

    #[test]
    fn source_spans_follow_the_clip_and_wrap_with_its_repeats() {
        let plain = clip(1_000, 10_000, None, 0);
        // Clipped to the clip, offset into the source.
        assert_eq!(
            source_spans(&plain, 500, 0, 2_000),
            [Some((1_000, 2_500)), None]
        );
        assert_eq!(source_spans(&plain, 500, 20_000, 30_000), [None, None]);
        // Repeating every 3,000 frames: a span across a repeat reads the
        // end of the loop, then its start.
        let looped = clip(1_000, 10_000, Some(3_000), 0);
        assert_eq!(
            source_spans(&looped, 0, 2_500, 3_500),
            [Some((3_500, 4_000)), Some((1_000, 1_500))]
        );
        assert_eq!(
            source_spans(&looped, 0, 3_100, 3_200),
            [Some((1_100, 1_200)), None]
        );
        // A span longer than the loop reads all of it once.
        assert_eq!(
            source_spans(&looped, 0, 0, 7_000),
            [Some((1_000, 4_000)), None]
        );
        // The phase moves where the clip begins in the loop.
        let phased = clip(1_000, 10_000, Some(3_000), 2_000);
        assert_eq!(
            source_spans(&phased, 0, 0, 100),
            [Some((3_000, 3_100)), None]
        );
    }

    #[test]
    fn the_pages_a_clip_reads_are_each_visited_once_within_the_file() {
        let pages = |clip: &Clip, span: std::ops::Range<i64>, frames: u64| {
            let mut seen = Vec::new();
            assert!(for_each_page(clip, 0, span, frames, &mut |p| {
                seen.push(p);
                true
            }));
            seen
        };
        let page = PAGE as i64;
        let plain = clip(page - 1, 10 * page, None, 0);
        // Frames page-1 to page+1: pages 0 and 1.
        assert_eq!(pages(&plain, 0..2, 100 * PAGE as u64), [0, 1]);
        // Past the end of the file nothing is visited.
        assert_eq!(pages(&plain, 0..3 * page, 2 * PAGE as u64), [0, 1]);
        assert!(pages(&plain, 0..10, (page - 1) as u64).is_empty());
        // A wrapping loop visits the loop's end, then its start.
        let looped = clip(0, 10 * page, Some(3 * page), 0);
        assert_eq!(
            pages(&looped, 3 * page - 1..3 * page + 1, 100 * PAGE as u64),
            [2, 0]
        );
        // Stops at the first page `f` refuses.
        let mut asked = 0;
        assert!(!for_each_page(
            &plain,
            0,
            0..5 * page,
            100 * PAGE as u64,
            &mut |p| {
                asked += 1;
                p < 2
            }
        ));
        assert_eq!(asked, 3);
    }
}
