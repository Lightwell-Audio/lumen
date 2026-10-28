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
//! Clips on a track may overlap; each plays on and the bus sums them. A
//! crossfade is two overlapping clips whose fades are marked `blend`: the
//! outgoing one fades out and the incoming one in along an equal-power law.
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

mod seams;
mod set;
use seams::resolve as set_seams;
use set::ClipSet;

use crate::envelope::{ClipEnvelope, EnvelopeGains};
use crate::mixer::{BUS_CHANNELS, Buses, TrackSource};
use crate::source::{Jump, Mode, RenderContext};
use crate::stream::{PAGE, PagePool, StreamAudio, StreamSide};
use crate::timeline::Timeline;

/// Clip edge fades: 2 ms.
const FADE_PER_SECOND: u32 = 500;
/// How fast a level glides: full scale in 20 ms (owner: pan and volume
/// drags crackled while the level stepped once per screen frame).
const GLIDE_PER_SECOND: f32 = 50.0;
/// A clip list change that moves clips crossfades over 5 ms.
const SWAP_PER_SECOND: u32 = 200;
/// A streamed clip needs its pages 40 ms ahead (twice a full glide).
const LOOKAHEAD_PER_SECOND: u32 = 25;
const QUEUE: usize = 4;
/// Fade curves are read from tables of this many steps: within about
/// 10⁻⁶ of the curve (far below hearing) for all but its steepest
/// first step.
const TABLE_STEPS: usize = 1_024;
/// The table cache starts over past this many curves.
const MAX_TABLES: usize = 256;

/// A fade curve's levels at `TABLE_STEPS + 1` evenly spaced points.
type Table = Arc<[f32]>;

/// Audio for clips.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipAudio {
    /// Held in memory, channel after channel, at `sample_rate`.
    Memory {
        sample_rate: u32,
        channels: Vec<Vec<f32>>,
    },
    /// Read from disk as it plays (M9 slice R6): only the pages a disk
    /// thread has put in memory can be heard.
    Stream(StreamAudio),
}

impl ClipAudio {
    pub fn sample_rate(&self) -> u32 {
        match self {
            ClipAudio::Memory { sample_rate, .. } => *sample_rate,
            ClipAudio::Stream(stream) => stream.sample_rate(),
        }
    }

    pub fn frames(&self) -> usize {
        match self {
            ClipAudio::Memory { channels, .. } => channels.first().map_or(0, Vec::len),
            ClipAudio::Stream(stream) => stream.frames() as usize,
        }
    }

    /// Channels played: all of a file in memory, one or two streamed.
    pub fn channels(&self) -> usize {
        match self {
            ClipAudio::Memory { channels, .. } => channels.len(),
            ClipAudio::Stream(stream) => stream.channels(),
        }
    }

    /// The samples in memory, channel after channel; `None` when streamed.
    pub fn memory(&self) -> Option<&[Vec<f32>]> {
        match self {
            ClipAudio::Memory { channels, .. } => Some(channels),
            ClipAudio::Stream(_) => None,
        }
    }

    pub fn stream(&self) -> Option<&StreamAudio> {
        match self {
            ClipAudio::Memory { .. } => None,
            ClipAudio::Stream(stream) => Some(stream),
        }
    }
}

/// Where the clip player reads one channel's samples from.
trait Samples {
    fn at(&self, frame: usize) -> Option<f32>;
}

impl Samples for &[f32] {
    #[inline(always)]
    fn at(&self, frame: usize) -> Option<f32> {
        self.get(frame).copied()
    }
}

impl Samples for StreamSide<'_> {
    #[inline(always)]
    fn at(&self, frame: usize) -> Option<f32> {
        StreamSide::at(self, frame)
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
    /// Gain and pan envelopes, on the clip's own clock (M7 slice A7).
    pub envelope: ClipEnvelope,
}

/// A clip's fades and repeats, in frames at the device rate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipShape {
    pub fade_in: FadeFrames,
    pub fade_out: FadeFrames,
    /// The content repeats every `period` frames from the clip's start.
    pub period: Option<i64>,
    /// Position within the repeated source when this clip begins.
    pub phase: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FadeFrames {
    pub frames: i64,
    /// Position within the fade's original curve window.
    pub offset: i64,
    /// Original fade curve window in frames; zero keeps legacy `frames` behavior.
    pub span: i64,
    /// From silence to full; a fade out plays it backwards.
    pub curve: Curve,
    /// This fade is one half of a crossfade with an overlapping clip: it
    /// follows an equal-power law (the other half is its mirror, so the two
    /// keep a steady level) and the clip's 2 ms edge fade is left out at this
    /// end, as the fade already covers it.
    pub blend: bool,
}

/// A clip's fade in and fade out tables, when it has fades.
#[derive(Clone, Default)]
struct Fades([Option<Table>; 2]);

/// Precomputed edge state for one clip in a contiguous split chain.
#[derive(Clone, Copy, Default)]
struct Seam {
    edges: [bool; 2],
    lead: i64,
    total: i64,
}

#[derive(Clone, Copy)]
struct ClipRender<'a> {
    fades: &'a Fades,
    seam: Seam,
}

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

    #[inline]
    fn fade_in_gain(&self, fades: &Fades, pos: i64) -> f32 {
        let fade_in = self.fade_in.frames;
        if pos < fade_in
            && let Some(table) = &fades.0[0]
        {
            let span = self.fade_in.span.max(fade_in);
            let level = read(
                table,
                (pos as f32 + self.fade_in.offset as f32 + 0.5) / span as f32,
            );
            return if self.fade_in.blend {
                (level * std::f32::consts::FRAC_PI_2).sin()
            } else {
                level
            };
        }
        1.0
    }

    #[inline]
    fn fade_out_gain(&self, fades: &Fades, pos: i64, length: i64) -> f32 {
        let fade_out = self.fade_out.frames;
        let left = length - pos;
        if left <= fade_out
            && let Some(table) = &fades.0[1]
        {
            let span = self.fade_out.span.max(fade_out);
            let x = (left as f32 + self.fade_out.offset as f32 - 0.5) / span as f32;
            return if self.fade_out.blend {
                // The mirror of the fade-in: its curve read forwards.
                (read(table, 1.0 - x) * std::f32::consts::FRAC_PI_2).cos()
            } else {
                read(table, x)
            };
        }
        1.0
    }
}

/// UI side: replaces the clip list and frees old ones. May allocate.
pub struct ClipsHandle {
    incoming: rtrb::Producer<Box<ClipSet>>,
    retired: rtrb::Consumer<Box<ClipSet>>,
    /// Fade tables by curve, shared between clips and lists.
    tables: HashMap<[u32; 4], Table>,
    /// The lists sent leave the outermost edge of each track unfaded
    /// ([`ClipsHandle::bare_ends`]).
    bare: bool,
}

impl ClipsHandle {
    /// For lists sent from now on, the start of the first clip and the end of
    /// the last clip of each track get no 2 ms edge fade, as inside a seamless
    /// join. Rendering a stretch of clips into a file uses it: the file's
    /// outer edges are faded when the new clip plays, as any clip is, and the
    /// edges inside it keep the fades they had (M7b slice E3).
    pub fn bare_ends(&mut self, on: bool) {
        self.bare = on;
    }

    /// Sends a new clip list. Returns it if the queue is full (call again
    /// next frame).
    pub fn set(&mut self, clips: Vec<Clip>) -> Result<(), Vec<Clip>> {
        self.incoming
            .push(Box::new(ClipSet::new(clips, &mut self.tables, self.bare)))
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
    /// The envelope gains of the clip being played.
    gains: EnvelopeGains,
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
            bare: false,
        },
        ClipPlayer {
            incoming: incoming_rx,
            retired: retired_tx,
            stash: None,
            current: Box::new(ClipSet::new(Vec::new(), &mut HashMap::new(), false)),
            swap: None,
            gains: EnvelopeGains::new(),
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
    /// How far past the block a streamed clip's pages must be in memory: more
    /// than a glide, so a clip that will run out fades while it still plays.
    ahead: i64,
}

/// The block being rendered and the room to work out its envelope gains.
struct Work<'a> {
    block: Block,
    gains: &'a mut EnvelopeGains,
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
    kept: Kept<'_>,
    cx: &RenderContext<'_>,
    work: &mut Work<'_>,
    ramp: Ramp,
    render: ClipRender<'_>,
    buses: &mut Buses<'_>,
) {
    let ClipRender { fades, seam } = render;
    let Work { block, gains } = work;
    let rate = cx.sample_rate;
    if clip.audio.sample_rate() != rate || clip.length <= 0 {
        return;
    }
    let mut scratch = *kept.level;
    let level = if cx.tail { &mut scratch } else { kept.level };
    let Some(mut bus) = buses.track(clip.track) else {
        // Its track is not in the routing: not heard, so it fades in if
        // the track joins while it plays.
        *level = f32::NAN;
        return;
    };
    let mut goal = clip.gain;
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
    if let ClipAudio::Stream(stream) = &*clip.audio {
        // Read from disk: while any page it reads in this block or a little
        // after is missing, it glides out (on audio it still has) and back in
        // once its pages are there. Each time it has to is one underrun.
        let ready = for_each_page(clip, start, a..b + block.ahead, stream.frames(), &mut |p| {
            stream.ensure(p)
        });
        if !ready {
            goal = 0.0;
        }
        if !cx.tail {
            if !ready && !*kept.starved {
                stream.pool().underrun();
            }
            *kept.starved = !ready;
        }
    }
    if level.is_nan() {
        // First heard. At its start the edge fade covers it; partway
        // through, it fades in.
        *level = if a == start { goal } else { 0.0 };
    }
    if goal == 0.0 && *level == 0.0 {
        return;
    }
    let sources = clip.audio.channels();
    if sources == 0 {
        // A clip with no channels has nothing to play (and no side to pick).
        return;
    }
    let enveloped = !clip.envelope.is_empty();
    if enveloped {
        gains.fill(
            &clip.envelope,
            (
                start - clip.shape.phase,
                clip.shape.period.filter(|p| *p > 0),
            ),
            (block.from, a, b),
            rate,
        );
    }
    let gains: &EnvelopeGains = gains;
    let plain = clip.shape.plain() && !enveloped;
    let mut gain = *level;
    // A crossfade half needs no 2 ms edge fade at its end of the clip.
    let blend = [clip.shape.fade_in.blend, clip.shape.fade_out.blend];
    let seamless = [seam.edges[0] || blend[0], seam.edges[1] || blend[1]];
    let seam_lead = if blend[0] {
        seam.lead.max(block.edge)
    } else {
        seam.lead
    };
    for side in 0..BUS_CHANNELS {
        let at = Placement {
            start,
            from: block.from,
            edge: block.edge,
            fades,
            env: enveloped.then(|| gains.side(side)),
            seamless,
            seam_lead,
            seam_total: seam.total,
        };
        let target = bus.channel(side);
        let side_mix = SideMix {
            clip,
            at,
            span: a..b,
            from: *level,
            goal,
            glide: block.glide,
            ramp,
            plain,
        };
        // Mono plays on both sides; extra channels are ignored.
        gain = match &*clip.audio {
            ClipAudio::Memory { channels, .. } => {
                side_mix.run(target, &channels[side.min(sources - 1)].as_slice())
            }
            ClipAudio::Stream(stream) => side_mix.run(target, &stream.side(side)),
        };
    }
    *level = gain;
}

/// A clip's kept state across blocks.
struct Kept<'a> {
    level: &'a mut f32,
    /// A streamed clip faded out because its audio was not in memory.
    starved: &'a mut bool,
}

/// One side of a clip in one block.
struct SideMix<'a> {
    clip: &'a Clip,
    at: Placement<'a>,
    span: std::ops::Range<i64>,
    /// The level it starts at and the level it goes to.
    from: f32,
    goal: f32,
    glide: f32,
    ramp: Ramp,
    /// Without fades, repeats or envelopes.
    plain: bool,
}

impl SideMix<'_> {
    /// Mixes the side into `target`; returns the level it ends at.
    #[inline(always)]
    fn run<S: Samples>(&self, target: &mut [f32], samples: &S) -> f32 {
        let mix: fn(&mut [f32], &S, &Clip, Placement<'_>, std::ops::Range<i64>, f32) =
            if self.plain { mix } else { mix_shaped };
        let (clip, at, ramp, goal) = (self.clip, self.at, self.ramp, self.goal);
        let mut gain = self.from;
        let mut t = self.span.start;
        // Gliding or crossfading: a new gain every frame until both settle.
        while t < self.span.end && (gain != goal || ramp.active(t)) {
            gain = if (goal - gain).abs() <= self.glide {
                goal
            } else {
                gain + self.glide.copysign(goal - gain)
            };
            mix(target, samples, clip, at, t..t + 1, gain * ramp.gain(t));
            t += 1;
        }
        // Steady: the common case, kept as tight as before.
        mix(target, samples, clip, at, t..self.span.end, gain);
        gain
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
    /// The envelope gains for this side, indexed by the frame's place in the
    /// block; none for a clip without an envelope.
    env: Option<&'a [f32]>,
    /// Suppress only the artificial 2 ms edge dip at verified split seams.
    seamless: [bool; 2],
    /// Position in the first clip's preserved outer edge fade.
    seam_lead: i64,
    /// Full contiguous span used to avoid shortening its edge fade.
    seam_total: i64,
}

/// Adds `clip`'s audio at `gain` for the timeline samples `frames` into
/// `target`: the plain path, for clips without fades or repeats.
#[inline(always)]
fn mix<S: Samples>(
    target: &mut [f32],
    samples: &S,
    clip: &Clip,
    at: Placement<'_>,
    frames: std::ops::Range<i64>,
    gain: f32,
) {
    for t in frames {
        let pos = t - at.start;
        let Some(sample) = usize::try_from(clip.offset + pos)
            .ok()
            .and_then(|i| samples.at(i))
        else {
            continue;
        };
        let edge_cap = if at.seamless[0] || at.seamless[1] {
            at.seam_total
        } else {
            clip.length
        };
        target[(t - at.from) as usize] += sample
            * gain
            * edge_gain(
                pos,
                clip.length,
                edge_cap,
                at.edge,
                at.seamless,
                at.seam_lead,
            );
    }
}

/// [`mix`] for a clip with fades or repeats: each repeat plays the content
/// from the offset again, with its own edge fades.
#[inline(always)]
fn mix_shaped<S: Samples>(
    target: &mut [f32],
    samples: &S,
    clip: &Clip,
    at: Placement<'_>,
    frames: std::ops::Range<i64>,
    gain: f32,
) {
    let shape = &clip.shape;
    for t in frames {
        let pos = t - at.start;
        let (inner, span, repeat, looping) = match shape.period {
            Some(period) if period > 0 => {
                let progressed = pos + clip.shape.phase;
                let repeat = progressed.div_euclid(period) * period;
                (
                    progressed - repeat,
                    period.min(clip.length + clip.shape.phase - repeat),
                    repeat,
                    true,
                )
            }
            _ => (pos, clip.length, 0, false),
        };
        let Some(sample) = usize::try_from(clip.offset + inner)
            .ok()
            .and_then(|i| samples.at(i))
        else {
            continue;
        };
        let seamless = if looping {
            [
                false,
                at.seamless[1] && repeat + span >= clip.length + clip.shape.phase,
            ]
        } else {
            at.seamless
        };
        let fade =
            shape.fade_in_gain(at.fades, pos) * shape.fade_out_gain(at.fades, pos, clip.length);
        let shaped = edge_gain(inner, span, span, at.edge, seamless, at.seam_lead) * fade;
        let slot = (t - at.from) as usize;
        let env = at.env.map_or(1.0, |e| e[slot]);
        target[slot] += sample * gain * shaped * env;
    }
}

/// The edge fade for frame `pos` of a clip `length` frames long.
fn edge_gain(
    pos: i64,
    length: i64,
    edge_span: i64,
    edge: i64,
    seamless: [bool; 2],
    seam_lead: i64,
) -> f32 {
    let edge = edge.min(edge_span / 2).max(1);
    let from_start = if seamless[0] {
        (pos + seam_lead + 1).min(edge)
    } else {
        (pos + 1).min(edge)
    };
    let to_end = if seamless[1] {
        edge
    } else {
        (length - pos).min(edge)
    };
    (from_start.min(to_end) as f32 / edge as f32).min(1.0)
}

/// Resolves candidate split seams against this block's tempo map. This is a
/// bounded linear pass over metadata built off the audio thread and allocates
/// nothing. Locate and loop declick remain owned by the engine transport.
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

    /// The page pools the lists playing read from: at most one each, and the
    /// old list's only when it is another pool.
    fn pools(&self) -> [Option<&PagePool>; 2] {
        let current = self.current.pool.as_deref();
        let old = self
            .swap
            .as_ref()
            .and_then(|s| s.old.pool.as_deref())
            .filter(|old| current.is_none_or(|c| !std::ptr::eq(*old, c)));
        [current, old]
    }

    fn enter_pools(&self) {
        self.pools().into_iter().flatten().for_each(PagePool::enter);
    }

    fn leave_pools(&self) {
        self.pools().into_iter().flatten().for_each(PagePool::leave);
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
            // Stopped, and only instrument and effect tails ring. The engine
            // renders instead of idling while a note rings, so lists sent
            // meanwhile are taken here, or a held live note would fill the
            // queue and the interface would hold its next edits back.
            self.idle();
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
            ahead: i64::from(rate / LOOKAHEAD_PER_SECOND),
        };
        // Pages read in this block stay in memory until it ends.
        self.enter_pools();
        set_seams(&mut self.current, cx);
        let crossfade = self.swap.as_mut().map(|swap| {
            if swap.length == 0 {
                swap.length = i64::from((rate / SWAP_PER_SECOND).max(1));
            }
            Crossfade {
                end: block.from + (swap.length - swap.done),
                length: swap.length,
            }
        });
        let mut work = Work {
            block,
            gains: &mut self.gains,
        };
        let set = &mut *self.current;
        for i in 0..set.clips.len() {
            let ramp = match crossfade {
                Some(x) if set.swapped[i] => Ramp::In(x),
                _ => Ramp::Steady,
            };
            play(
                &set.clips[i],
                Kept {
                    level: &mut set.levels[i],
                    starved: &mut set.starved[i],
                },
                cx,
                &mut work,
                ramp,
                ClipRender {
                    fades: &set.fades[i],
                    seam: Seam {
                        edges: set.seams[i],
                        lead: set.seam_lead[i],
                        total: set.seam_total[i],
                    },
                },
                out,
            );
        }
        if let (Some(swap), Some(x)) = (self.swap.as_mut(), crossfade) {
            set_seams(&mut swap.old, cx);
            let old = &mut *swap.old;
            for i in (0..old.clips.len()).filter(|&i| old.swapped[i]) {
                play(
                    &old.clips[i],
                    Kept {
                        level: &mut old.levels[i],
                        starved: &mut old.starved[i],
                    },
                    cx,
                    &mut work,
                    Ramp::Out(x),
                    ClipRender {
                        fades: &old.fades[i],
                        seam: Seam {
                            edges: old.seams[i],
                            lead: old.seam_lead[i],
                            total: old.seam_total[i],
                        },
                    },
                    out,
                );
            }
            if !cx.tail {
                swap.done += frames;
            }
        }
        self.leave_pools();
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
        assert_eq!(edge_gain(0, 1_000, 1_000, 10, [false; 2], 0), 0.1);
        assert_eq!(edge_gain(9, 1_000, 1_000, 10, [false; 2], 0), 1.0);
        assert_eq!(edge_gain(500, 1_000, 1_000, 10, [false; 2], 0), 1.0);
        assert_eq!(edge_gain(999, 1_000, 1_000, 10, [false; 2], 0), 0.1);
        // Short clips use half their length.
        assert_eq!(edge_gain(1, 4, 4, 10, [false; 2], 0), 1.0);
        assert_eq!(edge_gain(0, 4, 4, 10, [false; 2], 0), 0.5);
        assert_eq!(edge_gain(0, 1_000, 1_000, 10, [true, false], 10), 1.0);
        assert_eq!(edge_gain(999, 1_000, 1_000, 10, [false, true], 0), 1.0);
        // A split 24 frames into the outer 96-frame fade carries the
        // remaining fade into the second piece instead of snapping to unity.
        assert_eq!(
            edge_gain(0, 1_000, 1_000, 96, [true, false], 24),
            25.0 / 96.0
        );
    }

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
