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
//! Each clip plays into its track's bus; the mixer does the rest (M4,
//! ADR-0035).
//!
//! Levels glide. Each clip's gain is carried from one list to the next by
//! clip id and moves toward its new value at a fixed rate, so clip gain
//! drags and clip mutes never step or click. A clip that appears partway
//! through (its audio finished loading during playback, or its track
//! joined the routing late) fades in.
//!
//! A clip can fade in and out along any [`Curve`] and repeat its content
//! (loop-extend, M3 slice 6). Each repeat gets the 2 ms edge fades, so the
//! seams never click. Clips with neither take the plain path. The audio
//! thread reads fades from tables built on the UI side, one per distinct
//! curve, shared between clips and lists.
//!
//! A clip that moves, is trimmed or slipped, or goes away while playing
//! crossfades over 5 ms: its old placement fades out while the new one
//! fades in, so a clip dragged under the playhead never clicks (owner,
//! after M3 slice 6c: dragging a playing clip crackled). Clips whose
//! placement did not change play straight through. New lists wait while a
//! crossfade runs, so at most two lists play at once.

use std::sync::Arc;

use std::collections::HashMap;

use lumen_core::curve::Curve;
use lumen_core::time::Ticks;

use crate::mixer::{BUS_CHANNELS, Buses, TrackSource};
use crate::source::{Jump, Mode, RenderContext};
use crate::timeline::Timeline;

/// Clip edge fades: 2 ms.
const FADE_PER_SECOND: u32 = 500;
/// How fast a level glides: full scale in 20 ms (owner: pan and volume
/// drags crackled while the level stepped once per screen frame).
const GLIDE_PER_SECOND: f32 = 50.0;
/// A clip list change that moves clips crossfades over 5 ms.
const SWAP_PER_SECOND: u32 = 200;
const QUEUE: usize = 4;
/// Fade curves are read from tables of this many steps: within about
/// 10⁻⁶ of the curve (far below hearing) for all but its steepest
/// first step.
const TABLE_STEPS: usize = 1_024;
/// The table cache starts over past this many curves.
const MAX_TABLES: usize = 256;

/// A fade curve's levels at `TABLE_STEPS + 1` evenly spaced points.
type Table = Arc<[f32]>;

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
    /// The track it plays on: the id of its node in the routing.
    pub track: u64,
    /// Timeline position of the clip's first sample.
    pub start: Ticks,
    /// First frame of the audio played at the clip start.
    pub offset: i64,
    /// Frames played, repeats included.
    pub length: i64,
    /// Linear gain; 0 for a muted clip, which stays in the list so it
    /// fades out and back in.
    pub gain: f32,
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
    /// From silence to full; a fade out plays it backwards.
    pub curve: Curve,
}

/// A clip's fade in and fade out tables, when it has fades.
#[derive(Clone, Default)]
struct Fades([Option<Table>; 2]);

/// The level `x` (0 to 1) of the way through `table`.
#[inline]
fn read(table: &[f32], x: f32) -> f32 {
    let p = x.clamp(0.0, 1.0) * TABLE_STEPS as f32;
    let i = (p as usize).min(TABLE_STEPS - 1);
    match (table.get(i), table.get(i + 1)) {
        (Some(&a), Some(&b)) => a + (b - a) * (p - i as f32),
        _ => 1.0,
    }
}

impl ClipShape {
    fn plain(&self) -> bool {
        self.fade_in.frames <= 0 && self.fade_out.frames <= 0 && self.period.is_none()
    }

    /// The gain of the fades on frame `pos` of a clip `length` frames long,
    /// read from `fades`.
    #[inline]
    fn fade_gain(&self, fades: &Fades, pos: i64, length: i64) -> f32 {
        let mut gain = 1.0;
        let fade_in = self.fade_in.frames;
        if pos < fade_in
            && let Some(table) = &fades.0[0]
        {
            gain *= read(table, (pos as f32 + 0.5) / fade_in as f32);
        }
        let fade_out = self.fade_out.frames;
        let left = length - pos;
        if left <= fade_out
            && let Some(table) = &fades.0[1]
        {
            gain *= read(table, (left as f32 - 0.5) / fade_out as f32);
        }
        gain
    }
}

/// A clip list. Start samples are worked out every block from the current
/// tempo and rate (a binary search per clip): always right after tempo or
/// rate changes, and cheap at clip counts like these.
struct ClipSet {
    clips: Vec<Clip>,
    /// The level each clip plays at now. NaN until the clip is first
    /// heard: then it starts at its target, or from silence if it appears
    /// partway through.
    levels: Vec<f32>,
    /// `(id, index)` sorted by id, to carry levels into the next list.
    by_id: Vec<(u64, u32)>,
    /// Per clip, set when the list arrives: in the new list, the clip moved
    /// and fades in; in the old one, it moved or went away and fades out.
    swapped: Vec<bool>,
    /// Per clip, its fade tables.
    fades: Vec<Fades>,
}

impl ClipSet {
    /// Allocates; called on the UI side. Fade tables come from `tables`,
    /// built there the first time a curve is seen.
    fn new(clips: Vec<Clip>, tables: &mut HashMap<[u32; 4], Table>) -> Self {
        let mut by_id: Vec<(u64, u32)> = clips
            .iter()
            .enumerate()
            .map(|(i, clip)| (clip.id, i as u32))
            .collect();
        by_id.sort_unstable();
        if tables.len() > MAX_TABLES {
            tables.clear();
        }
        let mut table = |fade: &FadeFrames| {
            (fade.frames > 0).then(|| {
                let key = <[f32; 4]>::from(fade.curve).map(f32::to_bits);
                Arc::clone(
                    tables
                        .entry(key)
                        .or_insert_with(|| fade.curve.table(TABLE_STEPS).into()),
                )
            })
        };
        let fades = clips
            .iter()
            .map(|c| Fades([table(&c.shape.fade_in), table(&c.shape.fade_out)]))
            .collect();
        Self {
            levels: vec![f32::NAN; clips.len()],
            swapped: vec![false; clips.len()],
            fades,
            clips,
            by_id,
        }
    }

    /// Carries the levels of clips that are in both lists from `old`, and
    /// marks the clips that must crossfade: those that moved (in both
    /// lists) and those that went away (in `old`). Returns whether any did.
    /// Real-time safe: one merge over two sorted lists.
    fn carry_levels(&mut self, old: &mut ClipSet) -> bool {
        self.swapped.fill(false);
        old.swapped.fill(true);
        let (mut i, mut j) = (0, 0);
        while let (Some(&(new_id, new_index)), Some(&(old_id, old_index))) =
            (self.by_id.get(i), old.by_id.get(j))
        {
            match new_id.cmp(&old_id) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    let (new_index, old_index) = (new_index as usize, old_index as usize);
                    self.levels[new_index] = old.levels[old_index];
                    let moved = !self.clips[new_index].placed_as(&old.clips[old_index]);
                    self.swapped[new_index] = moved;
                    old.swapped[old_index] = moved;
                    i += 1;
                    j += 1;
                }
            }
        }
        old.swapped.contains(&true)
    }
}

impl Clip {
    /// Whether `other` plays the same audio on the same samples (its level
    /// may differ: levels glide instead).
    fn placed_as(&self, other: &Clip) -> bool {
        self.start == other.start
            && self.offset == other.offset
            && self.length == other.length
            && self.shape == other.shape
            && Arc::ptr_eq(&self.audio, &other.audio)
    }
}

/// UI side: replaces the clip list and frees old ones. May allocate.
pub struct ClipsHandle {
    incoming: rtrb::Producer<Box<ClipSet>>,
    retired: rtrb::Consumer<Box<ClipSet>>,
    /// Fade tables by curve, shared between clips and lists.
    tables: HashMap<[u32; 4], Table>,
}

impl ClipsHandle {
    /// Sends a new clip list. Returns it if the queue is full (call again
    /// next frame).
    pub fn set(&mut self, clips: Vec<Clip>) -> Result<(), Vec<Clip>> {
        self.incoming
            .push(Box::new(ClipSet::new(clips, &mut self.tables)))
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
    /// The list `current` replaced, while its moved clips fade out.
    swap: Option<Swap>,
}

/// A crossfade from one clip list to the next.
struct Swap {
    old: Box<ClipSet>,
    /// Frames played of `length` (set on the first block, from its rate).
    done: i64,
    length: i64,
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
            tables: HashMap::new(),
        },
        ClipPlayer {
            incoming: incoming_rx,
            retired: retired_tx,
            stash: None,
            current: Box::new(ClipSet::new(Vec::new(), &mut HashMap::new())),
            swap: None,
        },
    )
}

fn fade_length(rate: u32) -> u32 {
    (rate / FADE_PER_SECOND).max(1)
}

/// The block being rendered.
struct Block {
    /// Timeline samples of the first frame and one past the last.
    from: i64,
    to: i64,
    /// Edge fade length, in frames.
    edge: i64,
    /// How far a level moves in one frame.
    glide: f32,
}

/// How a clip fades across a clip list change, on top of its level.
#[derive(Clone, Copy)]
enum Ramp {
    Steady,
    /// Its new placement fades in.
    In(Crossfade),
    /// Its old placement fades out, and is silent after.
    Out(Crossfade),
}

/// Where a crossfade stands in this block.
#[derive(Clone, Copy)]
struct Crossfade {
    /// Timeline sample at which it ends. Counted from the block, so it
    /// holds while scrubbing too, where blocks do not follow each other.
    end: i64,
    length: i64,
}

impl Ramp {
    fn active(self, t: i64) -> bool {
        match self {
            Ramp::Steady => false,
            Ramp::In(x) | Ramp::Out(x) => t < x.end,
        }
    }

    /// The gain at timeline sample `t`. In and out always add up to one.
    #[inline]
    fn gain(self, t: i64) -> f32 {
        let progress =
            |x: Crossfade| (((t - (x.end - x.length)) as f32 + 0.5) / x.length as f32).min(1.0);
        match self {
            Ramp::Steady => 1.0,
            Ramp::In(x) => progress(x),
            Ramp::Out(x) => 1.0 - progress(x),
        }
    }
}

/// Plays `clip` at its kept level into its track's bus in `buses`, faded
/// by `ramp`. A tail (rendered ahead of a jump) reads the level but does
/// not move it.
fn play(
    clip: &Clip,
    fades: &Fades,
    kept: &mut f32,
    cx: &RenderContext<'_>,
    block: &Block,
    ramp: Ramp,
    buses: &mut Buses<'_>,
) {
    let rate = cx.sample_rate;
    if clip.audio.sample_rate != rate || clip.length <= 0 {
        return;
    }
    let mut scratch = *kept;
    let level = if cx.tail { &mut scratch } else { kept };
    let Some(mut bus) = buses.track(clip.track) else {
        // Its track is not in the routing: not heard, so it fades in if
        // the track joins while it plays.
        *level = f32::NAN;
        return;
    };
    let goal = clip.gain;
    let start = cx.timeline.sample_at(clip.start, rate);
    let end = start + clip.length;
    let (a, mut b) = (block.from.max(start), block.to.min(end));
    if let Ramp::Out(x) = ramp {
        b = b.min(x.end);
    }
    if a >= b {
        // Silent in this block: nothing to glide from.
        *level = goal;
        return;
    }
    if level.is_nan() {
        // First heard. At its start the edge fade covers it; partway
        // through, it fades in.
        *level = if a == start { goal } else { 0.0 };
    }
    if goal == 0.0 && *level == 0.0 {
        return;
    }
    let sources = clip.audio.channels.len();
    if sources == 0 {
        // A clip with no channels has nothing to play (and no side to pick).
        return;
    }
    let at = Placement {
        start,
        from: block.from,
        edge: block.edge,
        fades,
    };
    let mix = if clip.shape.plain() { mix } else { mix_shaped };
    let mut gain = *level;
    for side in 0..BUS_CHANNELS {
        // Mono plays on both sides; extra channels are ignored.
        let samples = &clip.audio.channels[side.min(sources - 1)];
        let target = bus.channel(side);
        gain = *level;
        let mut t = a;
        // Gliding or crossfading: a new gain every frame until both settle.
        while t < b && (gain != goal || ramp.active(t)) {
            gain = if (goal - gain).abs() <= block.glide {
                goal
            } else {
                gain + block.glide.copysign(goal - gain)
            };
            mix(target, samples, clip, at, t..t + 1, gain * ramp.gain(t));
            t += 1;
        }
        // Steady: the common case, kept as tight as before.
        mix(target, samples, clip, at, t..b, gain);
    }
    *level = gain;
}

/// Where a clip sits in the block being rendered.
#[derive(Clone, Copy)]
struct Placement<'a> {
    /// Timeline sample of the clip's first frame.
    start: i64,
    /// Timeline sample of the block's first frame.
    from: i64,
    /// Edge fade length, in frames.
    edge: i64,
    /// The clip's fade tables.
    fades: &'a Fades,
}

/// Adds `clip`'s audio at `gain` for the timeline samples `frames` into
/// `target`: the plain path, for clips without fades or repeats.
#[inline(always)]
fn mix(
    target: &mut [f32],
    samples: &[f32],
    clip: &Clip,
    at: Placement<'_>,
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
    at: Placement<'_>,
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
        let shaped = edge_gain(inner, span, at.edge) * shape.fade_gain(at.fades, pos, clip.length);
        target[(t - at.from) as usize] += sample * gain * shaped;
    }
}

/// The edge fade for frame `pos` of a clip `length` frames long.
fn edge_gain(pos: i64, length: i64, edge: i64) -> f32 {
    let edge = edge.min(length / 2).max(1);
    let from_start = (pos + 1).min(edge);
    let to_end = (length - pos).min(edge);
    (from_start.min(to_end) as f32 / edge as f32).min(1.0)
}

impl ClipPlayer {
    /// Takes a new clip list if one arrived and no crossfade is running.
    /// With `fade`, clips that moved crossfade into it. Real-time safe.
    fn take_new(&mut self, fade: bool) {
        if let Some(old) = self.stash.take() {
            self.retire(old);
            if self.stash.is_some() {
                return;
            }
        }
        if self.swap.is_some() {
            return;
        }
        let Ok(mut new) = self.incoming.pop() else {
            return;
        };
        // Only the newest list matters: lists that queued up (while
        // stopped, or during a crossfade) go straight back unplayed, so a
        // drag's old positions never play late.
        // Skipping a list needs room for it and for the one it replaces
        // in the end, or the stash would be overwritten and a list freed
        // here; newer lists then wait a block.
        while self.retired.slots() >= 2
            && let Ok(newer) = self.incoming.pop()
        {
            self.retire(std::mem::replace(&mut new, newer));
        }
        let moved = new.carry_levels(&mut self.current);
        let old = std::mem::replace(&mut self.current, new);
        if fade && moved {
            self.swap = Some(Swap {
                old,
                done: 0,
                length: 0,
            });
        } else {
            self.retire(old);
        }
    }

    /// Hands `old` back to be freed, keeping it if the queue is full. Only
    /// called while the stash is empty.
    fn retire(&mut self, old: Box<ClipSet>) {
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
    }

    /// Ends the crossfade once it is done, or at once with `now`.
    fn finish_swap(&mut self, now: bool) {
        if self.stash.is_some() {
            return;
        }
        if let Some(swap) = self.swap.take_if(|s| now || s.done >= s.length) {
            self.retire(swap.old);
        }
    }
}

impl TrackSource for ClipPlayer {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Buses<'_>) {
        if cx.mode == Mode::Ring {
            // Stopped, and only instrument and effect tails ring.
            return;
        }
        if !cx.tail {
            self.take_new(true);
        }
        let rate = cx.sample_rate;
        let frames = out.frames() as i64;
        let block = Block {
            from: cx.start,
            to: cx.start + frames,
            edge: i64::from(fade_length(rate)),
            glide: GLIDE_PER_SECOND / rate as f32,
        };
        let crossfade = self.swap.as_mut().map(|swap| {
            if swap.length == 0 {
                swap.length = i64::from((rate / SWAP_PER_SECOND).max(1));
            }
            Crossfade {
                end: block.from + (swap.length - swap.done),
                length: swap.length,
            }
        });
        let set = &mut *self.current;
        for i in 0..set.clips.len() {
            let ramp = match crossfade {
                Some(x) if set.swapped[i] => Ramp::In(x),
                _ => Ramp::Steady,
            };
            let (clip, fades) = (&set.clips[i], &set.fades[i]);
            play(clip, fades, &mut set.levels[i], cx, &block, ramp, out);
        }
        if let (Some(swap), Some(x)) = (self.swap.as_mut(), crossfade) {
            let old = &mut *swap.old;
            for i in (0..old.clips.len()).filter(|&i| old.swapped[i]) {
                let (clip, fades) = (&old.clips[i], &old.fades[i]);
                play(
                    clip,
                    fades,
                    &mut old.levels[i],
                    cx,
                    &block,
                    Ramp::Out(x),
                    out,
                );
            }
            if !cx.tail {
                swap.done += frames;
            }
        }
        self.finish_swap(false);
    }

    fn idle(&mut self) {
        self.finish_swap(true);
        self.take_new(false);
    }

    fn locate(&mut self, _: &Timeline, _: u32, _: i64, _: Jump) {
        // A jump is crossfaded by the engine: a crossfade between lists ends
        // here, and levels start at their targets rather than gliding.
        self.finish_swap(true);
        self.take_new(false);
        let set = &mut *self.current;
        for (clip, level) in set.clips.iter().zip(set.levels.iter_mut()) {
            *level = clip.gain;
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
}
