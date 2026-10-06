//! Audio clips on the timeline (M2 slice 6). The audio lives in memory
//! (owner decision 2026-10-06); disk streaming replaces that source in M9
//! behind the same [`Source`] interface.
//!
//! A clip starts at a musical position and plays its audio at the audio's
//! own speed (no warping yet). Clips fade in and out over 2 ms at their
//! edges; the engine crossfades every transport jump.
//!
//! The clip list is replaced whole: the UI builds a new one and sends it
//! through a lock-free queue, and the old one goes back to be freed on the
//! UI side. The audio thread never allocates or frees.

use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::source::{Jump, Output, RenderContext, Source};
use crate::timeline::Timeline;

/// Clip edge fades: 2 ms.
const FADE_PER_SECOND: u32 = 500;
const QUEUE: usize = 4;

/// Audio for clips, channel after channel, at `sample_rate`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipAudio {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

impl ClipAudio {
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
}

#[derive(Debug, Clone)]
pub struct Clip {
    /// Timeline position of the clip's first sample.
    pub start: Ticks,
    /// First frame of the audio played at the clip start.
    pub offset: i64,
    /// Frames played.
    pub length: i64,
    /// Linear gain.
    pub gain: f32,
    /// Plays only while the device runs at `audio.sample_rate`; otherwise
    /// the clip is silent until the app converts it again.
    pub audio: Arc<ClipAudio>,
}

/// A clip list. Start samples are worked out every block from the current
/// tempo and rate (a binary search per clip): always right after tempo or
/// rate changes, and cheap at clip counts like these.
struct ClipSet {
    clips: Vec<Clip>,
}

impl ClipSet {
    fn new(clips: Vec<Clip>) -> Self {
        Self { clips }
    }
}

/// UI side: replaces the clip list and frees old ones. May allocate.
pub struct ClipsHandle {
    incoming: rtrb::Producer<Box<ClipSet>>,
    retired: rtrb::Consumer<Box<ClipSet>>,
}

impl ClipsHandle {
    /// Sends a new clip list. Returns it if the queue is full (call again
    /// next frame).
    pub fn set(&mut self, clips: Vec<Clip>) -> Result<(), Vec<Clip>> {
        self.incoming
            .push(Box::new(ClipSet::new(clips)))
            .map_err(|rtrb::PushError::Full(set)| set.clips)
    }

    /// Frees clip lists the audio side has replaced. Call regularly.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

/// Audio side: plays the clips. A [`Source`] for the engine.
pub struct ClipPlayer {
    incoming: rtrb::Consumer<Box<ClipSet>>,
    retired: rtrb::Producer<Box<ClipSet>>,
    /// A replaced list that did not fit in the retired queue yet.
    stash: Option<Box<ClipSet>>,
    current: Box<ClipSet>,
}

/// Creates the clip pair, starting with no clips. Allocates, so call it off
/// the audio thread.
pub fn clip_player() -> (ClipsHandle, ClipPlayer) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    (
        ClipsHandle {
            incoming: incoming_tx,
            retired: retired_rx,
        },
        ClipPlayer {
            incoming: incoming_rx,
            retired: retired_tx,
            stash: None,
            current: Box::new(ClipSet::new(Vec::new())),
        },
    )
}

fn fade_length(rate: u32) -> u32 {
    (rate / FADE_PER_SECOND).max(1)
}

/// The edge fade for frame `pos` of a clip `length` frames long.
fn edge_gain(pos: i64, length: i64, edge: i64) -> f32 {
    let edge = edge.min(length / 2).max(1);
    let from_start = (pos + 1).min(edge);
    let to_end = (length - pos).min(edge);
    (from_start.min(to_end) as f32 / edge as f32).min(1.0)
}

impl ClipPlayer {
    /// Takes a new clip list if one arrived. Real-time safe.
    fn take_new(&mut self) {
        if let Some(old) = self.stash.take()
            && let Err(rtrb::PushError::Full(old)) = self.retired.push(old)
        {
            self.stash = Some(old);
            return;
        }
        if let Ok(new) = self.incoming.pop() {
            let old = std::mem::replace(&mut self.current, new);
            if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
                self.stash = Some(old);
            }
        }
    }
}

impl Source for ClipPlayer {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
        self.take_new();
        let rate = cx.sample_rate;
        let frames = out.frames() as i64;
        let (from, to) = (cx.start, cx.start + frames);
        let edge = i64::from(fade_length(rate));
        let outputs = out.channels().min(2);
        for clip in &self.current.clips {
            if clip.audio.sample_rate != rate || clip.length <= 0 {
                continue;
            }
            let start = cx.timeline.sample_at(clip.start, rate);
            let end = start + clip.length;
            let (a, b) = (from.max(start), to.min(end));
            if a >= b {
                continue;
            }
            let sources = clip.audio.channels.len();
            for output in 0..outputs {
                // Mono plays on both sides; extra channels are ignored.
                let samples = &clip.audio.channels[output.min(sources - 1)];
                let target = out.channel(output);
                for t in a..b {
                    let pos = t - start;
                    let Some(&sample) = usize::try_from(clip.offset + pos)
                        .ok()
                        .and_then(|i| samples.get(i))
                    else {
                        continue;
                    };
                    target[(t - from) as usize] +=
                        sample * clip.gain * edge_gain(pos, clip.length, edge);
                }
            }
        }
    }

    fn locate(&mut self, _: &Timeline, _: u32, _: i64, _: Jump) {
        self.take_new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_fade_over_the_edge_length() {
        assert_eq!(edge_gain(0, 1_000, 10), 0.1);
        assert_eq!(edge_gain(9, 1_000, 10), 1.0);
        assert_eq!(edge_gain(500, 1_000, 10), 1.0);
        assert_eq!(edge_gain(999, 1_000, 10), 0.1);
        // Short clips use half their length.
        assert_eq!(edge_gain(1, 4, 10), 1.0);
        assert_eq!(edge_gain(0, 4, 10), 0.5);
    }
}
