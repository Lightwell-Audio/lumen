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
//!
//! Levels glide. Each clip's level (gain and pan) is carried from one list
//! to the next by clip id and moves toward its new value at a fixed rate,
//! so volume and pan drags, mute and solo never step or click. A clip that
//! appears partway through (its audio finished loading during playback)
//! fades in. The M4 mixer takes this over with per-track smoothing.
//!
//! A clip can fade in and out in any [`FadeShape`] and repeat its content
//! (loop-extend, M3 slice 6). Each repeat gets the 2 ms edge fades, so the
//! seams never click. Clips with neither take the plain path.

use std::sync::Arc;

use lumen_core::fade::FadeShape;
use lumen_core::time::Ticks;

use crate::source::{Jump, Output, RenderContext, Source};
use crate::timeline::Timeline;

/// Clip edge fades: 2 ms.
const FADE_PER_SECOND: u32 = 500;
/// How fast a level glides: full scale in 20 ms (owner: pan and volume
/// drags crackled while the level stepped once per screen frame).
const GLIDE_PER_SECOND: f32 = 50.0;
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
    /// Identity across clip lists (the project's clip id), so a clip's
    /// level glides from list to list.
    pub id: u64,
    /// Timeline position of the clip's first sample.
    pub start: Ticks,
    /// First frame of the audio played at the clip start.
    pub offset: i64,
    /// Frames played, repeats included.
    pub length: i64,
    /// Linear gain; 0 for a clip that is muted or cannot be heard, which
    /// stays in the list so it fades out and back in.
    pub gain: f32,
    /// Stereo balance, -1 (left) to +1 (right); 0 leaves both sides at
    /// unity. The track pan of M3; pan laws arrive with the mixer (M4).
    pub pan: f32,
    /// Plays only while the device runs at `audio.sample_rate`; otherwise
    /// the clip is silent until the app converts it again.
    pub audio: Arc<ClipAudio>,
    /// Fades and repeats.
    pub shape: ClipShape,
}

/// A clip's fades and repeats, in frames at the device rate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipShape {
    pub fade_in: FadeFrames,
    pub fade_out: FadeFrames,
    /// The content repeats every `period` frames from the clip's start.
    pub period: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FadeFrames {
    pub frames: i64,
    pub shape: FadeShape,
}

impl ClipShape {
    fn plain(&self) -> bool {
        self.fade_in.frames <= 0 && self.fade_out.frames <= 0 && self.period.is_none()
    }

    /// The gain of the fades on frame `pos` of a clip `length` frames long.
    #[inline]
    fn fade_gain(&self, pos: i64, length: i64) -> f32 {
        let mut gain = 1.0;
        let fade_in = self.fade_in.frames;
        if pos < fade_in {
            gain *= self.fade_in.shape.gain((pos as f32 + 0.5) / fade_in as f32);
        }
        let fade_out = self.fade_out.frames;
        let left = length - pos;
        if left <= fade_out {
            gain *= self
                .fade_out
                .shape
                .gain((left as f32 - 0.5) / fade_out as f32);
        }
        gain
    }
}

/// A clip list. Start samples are worked out every block from the current
/// tempo and rate (a binary search per clip): always right after tempo or
/// rate changes, and cheap at clip counts like these.
struct ClipSet {
    clips: Vec<Clip>,
    /// The level each clip's left and right sides play at now. NaN until
    /// the clip is first heard: then it starts at its target, or from
    /// silence if it appears partway through.
    levels: Vec<[f32; 2]>,
    /// `(id, index)` sorted by id, to carry levels into the next list.
    by_id: Vec<(u64, u32)>,
}

impl ClipSet {
    /// Allocates; called on the UI side.
    fn new(clips: Vec<Clip>) -> Self {
        let mut by_id: Vec<(u64, u32)> = clips
            .iter()
            .enumerate()
            .map(|(i, clip)| (clip.id, i as u32))
            .collect();
        by_id.sort_unstable();
        Self {
            levels: vec![[f32::NAN; 2]; clips.len()],
            clips,
            by_id,
        }
    }

    /// Carries the levels of clips that are in both lists from `old`.
    /// Real-time safe: one merge over two sorted lists.
    fn carry_levels(&mut self, old: &ClipSet) {
        let (mut i, mut j) = (0, 0);
        while let (Some(&(new_id, new_index)), Some(&(old_id, old_index))) =
            (self.by_id.get(i), old.by_id.get(j))
        {
            match new_id.cmp(&old_id) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    self.levels[new_index as usize] = old.levels[old_index as usize];
                    i += 1;
                    j += 1;
                }
            }
        }
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

/// Balance gain for `output` of `outputs`: unity at the centre, the far
/// side fading to silence as the pan moves away from it.
fn balance(pan: f32, output: usize, outputs: usize) -> f32 {
    if outputs < 2 {
        return 1.0;
    }
    let pan = pan.clamp(-1.0, 1.0);
    if output == 0 {
        (1.0 - pan).min(1.0)
    } else {
        (1.0 + pan).min(1.0)
    }
}

/// Where a clip sits in the block being rendered.
#[derive(Clone, Copy)]
struct Placement {
    /// Timeline sample of the clip's first frame.
    start: i64,
    /// Timeline sample of the block's first frame.
    from: i64,
    /// Edge fade length, in frames.
    edge: i64,
}

/// Adds `clip`'s audio at `gain` for the timeline samples `frames` into
/// `target`: the plain path, for clips without fades or repeats.
#[inline(always)]
fn mix(
    target: &mut [f32],
    samples: &[f32],
    clip: &Clip,
    at: Placement,
    frames: std::ops::Range<i64>,
    gain: f32,
) {
    for t in frames {
        let pos = t - at.start;
        let Some(&sample) = usize::try_from(clip.offset + pos)
            .ok()
            .and_then(|i| samples.get(i))
        else {
            continue;
        };
        target[(t - at.from) as usize] += sample * gain * edge_gain(pos, clip.length, at.edge);
    }
}

/// [`mix`] for a clip with fades or repeats: each repeat plays the content
/// from the offset again, with its own edge fades.
#[inline(always)]
fn mix_shaped(
    target: &mut [f32],
    samples: &[f32],
    clip: &Clip,
    at: Placement,
    frames: std::ops::Range<i64>,
    gain: f32,
) {
    let shape = &clip.shape;
    for t in frames {
        let pos = t - at.start;
        let (inner, span) = match shape.period {
            Some(period) if period > 0 => {
                let repeat = pos.div_euclid(period) * period;
                (pos - repeat, period.min(clip.length - repeat))
            }
            _ => (pos, clip.length),
        };
        let Some(&sample) = usize::try_from(clip.offset + inner)
            .ok()
            .and_then(|i| samples.get(i))
        else {
            continue;
        };
        let shaped = edge_gain(inner, span, at.edge) * shape.fade_gain(pos, clip.length);
        target[(t - at.from) as usize] += sample * gain * shaped;
    }
}

/// The level a clip's left and right sides should reach.
fn targets(clip: &Clip, outputs: usize) -> [f32; 2] {
    [0, 1].map(|output| clip.gain * balance(clip.pan, output, outputs))
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
        if let Ok(mut new) = self.incoming.pop() {
            new.carry_levels(&self.current);
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
        let glide = GLIDE_PER_SECOND / rate as f32;
        let set = &mut *self.current;
        for (clip, kept) in set.clips.iter().zip(set.levels.iter_mut()) {
            if clip.audio.sample_rate != rate || clip.length <= 0 {
                continue;
            }
            // A tail (rendered ahead of a jump) reads the levels but does
            // not move them.
            let mut scratch = *kept;
            let level = if cx.tail { &mut scratch } else { kept };
            let goals = targets(clip, outputs);
            let start = cx.timeline.sample_at(clip.start, rate);
            let end = start + clip.length;
            let (a, b) = (from.max(start), to.min(end));
            if a >= b {
                // Silent in this block: nothing to glide from.
                *level = goals;
                continue;
            }
            if level[0].is_nan() {
                // First heard. At its start the edge fade covers it;
                // partway through, it fades in.
                *level = if a == start { goals } else { [0.0; 2] };
            }
            if goals == [0.0; 2] && *level == [0.0; 2] {
                continue;
            }
            let sources = clip.audio.channels.len();
            let at = Placement { start, from, edge };
            let mix = if clip.shape.plain() { mix } else { mix_shaped };
            for output in 0..outputs {
                // Mono plays on both sides; extra channels are ignored.
                let samples = &clip.audio.channels[output.min(sources - 1)];
                let target = out.channel(output);
                let goal = goals[output];
                let mut gain = level[output];
                let mut t = a;
                // Gliding: a new gain every frame until it arrives.
                while t < b && gain != goal {
                    gain = if (goal - gain).abs() <= glide {
                        goal
                    } else {
                        gain + glide.copysign(goal - gain)
                    };
                    mix(target, samples, clip, at, t..t + 1, gain);
                    t += 1;
                }
                // Steady: the common case, kept as tight as before.
                mix(target, samples, clip, at, t..b, gain);
                level[output] = gain;
            }
        }
    }

    fn locate(&mut self, _: &Timeline, _: u32, _: i64, _: Jump) {
        self.take_new();
        // A jump is crossfaded by the engine; levels start there at their
        // targets rather than gliding.
        let set = &mut *self.current;
        for (clip, level) in set.clips.iter().zip(set.levels.iter_mut()) {
            *level = targets(clip, 2);
        }
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

    #[test]
    fn balance_is_unity_at_the_centre_and_silences_the_far_side() {
        assert_eq!((balance(0.0, 0, 2), balance(0.0, 1, 2)), (1.0, 1.0));
        assert_eq!((balance(-1.0, 0, 2), balance(-1.0, 1, 2)), (1.0, 0.0));
        assert_eq!((balance(0.5, 0, 2), balance(0.5, 1, 2)), (0.5, 1.0));
        assert_eq!(balance(1.0, 0, 1), 1.0, "mono output ignores the pan");
    }
}
