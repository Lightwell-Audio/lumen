//! The mixer graph (M4 slice 1, ADR-0035).
//!
//! Each track is a node with a stereo bus. A [`TrackSource`] (the clip
//! player) plays each track's audio into its bus; the [`Plan`] then runs
//! the nodes in order: trim, polarity and mute, pre-fader sends, volume
//! and pan, post-fader sends, and the output (a group, a return, the
//! master, or a pair of device outputs). Groups sum their children and
//! returns sum the sends into them, because they are simply nodes that
//! others feed.
//!
//! The UI side compiles the project's [`Routing`] into a plan and sends
//! it through a lock-free queue; the audio thread takes it at the start of
//! a block and hands the old one back to be freed, as clip lists are.
//! Levels carry over by node id and glide (`smooth`). A plan whose shape
//! differs (outputs or sends changed) crossfades in over 5 ms while the
//! old one fades out, both fed the same track audio, so rerouting while
//! playing never clicks. New plans wait while a crossfade runs.
//!
//! The plan the listener hears meters every node's output and measures the
//! loudness of the node the routing names (`meter`); the handle reads
//! them.
//!
//! **Delay compensation** (M4 slice 6, ADR-0041): each node reports the
//! latency of its devices, and the plan delays every path that would
//! arrive early (`delay`), so parallel paths line up sample for sample,
//! sends and sidechains included. The mixer reports the total as its
//! [`Source::latency`], and the engine renders that far ahead of the
//! playhead. While latent, the mixer declicks jumps where the tracks play
//! into their buses, so the crossfade is heard when the jump is: the
//! engine's crossfade at the output would come too early.

mod compile;
mod delay;
mod meter;
mod plan;
mod smooth;
mod spec;

pub use compile::compile;
pub use delay::MAX_DEVICE_LATENCY;
pub use meter::{Level, RMS_SECONDS};
pub use plan::{BUS_CHANNELS, Bus, Buses, CHUNK, Plan};
pub use spec::{
    Compensation, CueOut, Destination, Device, NodeSpec, Routing, RoutingError, SendSpec,
    SidechainSpec, Tap,
};

use std::collections::HashMap;
use std::sync::Arc;

use compile::compile_metered;
use meter::{LoudnessReader, LoudnessTap, MeterBank};
use plan::Ramp;

use crate::declick::fade_frames;
use crate::source::{Jump, Mode, Output, RenderContext, Source};
use crate::timeline::Timeline;

const QUEUE: usize = 4;
/// A plan of a new shape crossfades in over 5 ms.
const SWAP_PER_SECOND: u32 = 200;

/// Audio per track, played into the tracks' buses. Every method must be
/// real-time safe, as for [`Source`].
pub trait TrackSource: Send {
    /// Adds `buses.frames()` frames from timeline sample `cx.start` to the
    /// buses of the tracks that play. A track with no bus is not heard.
    fn render(&mut self, cx: &RenderContext<'_>, buses: &mut Buses<'_>);

    /// Playback starts or jumps, as [`Source::locate`].
    fn locate(&mut self, _timeline: &Timeline, _sample_rate: u32, _sample: i64, _jump: Jump) {}

    /// A block passed without rendering, as [`Source::idle`]: take what
    /// the UI sent meanwhile, so its queues never fill while stopped.
    fn idle(&mut self) {}

    /// Playback stopped or paused, as [`Source::stopped`].
    fn stopped(&mut self) {}

    /// Panic, as [`Source::panic`].
    fn panic(&mut self) {}

    /// Something still rings or waits to be sent, as [`Source::ringing`].
    fn ringing(&self) -> bool {
        false
    }
}

/// Two track sources played together into the same buses: the audio
/// clips and the MIDI instruments (M5 slice 1).
pub struct Layered<A, B>(pub A, pub B);

impl<A: TrackSource, B: TrackSource> TrackSource for Layered<A, B> {
    fn render(&mut self, cx: &RenderContext<'_>, buses: &mut Buses<'_>) {
        self.0.render(cx, buses);
        self.1.render(cx, buses);
    }

    fn locate(&mut self, timeline: &Timeline, sample_rate: u32, sample: i64, jump: Jump) {
        self.0.locate(timeline, sample_rate, sample, jump);
        self.1.locate(timeline, sample_rate, sample, jump);
    }

    fn idle(&mut self) {
        self.0.idle();
        self.1.idle();
    }

    fn stopped(&mut self) {
        self.0.stopped();
        self.1.stopped();
    }

    fn panic(&mut self) {
        self.0.panic();
        self.1.panic();
    }

    fn ringing(&self) -> bool {
        self.0.ringing() || self.1.ringing()
    }
}

/// The smallest meter bank: room for this many nodes before a bigger one.
const MIN_METERS: usize = 64;

/// UI side: replaces the routing, frees old plans and reads the meters.
/// May allocate.
pub struct MixerHandle {
    incoming: rtrb::Producer<Box<Plan>>,
    retired: rtrb::Consumer<Box<Plan>>,
    /// Where the plans write their levels.
    meters: Arc<MeterBank>,
    /// Each node's slot in `meters`. Slots are not reused while the bank
    /// lasts (a new node never shows an old one's level); when it fills, a
    /// bigger bank takes only the nodes in use.
    slots: HashMap<u64, u32>,
    loudness: LoudnessReader,
    /// The compensation of the last routing sent.
    compensation: Compensation,
}

impl MixerHandle {
    /// Compiles `routing` and sends it. Returns whether it was sent: false
    /// when the queue is full (send it again next frame). A routing that
    /// cannot play is refused, and the current one keeps playing.
    pub fn set(&mut self, routing: &Routing) -> Result<bool, RoutingError> {
        let fresh = routing
            .nodes
            .iter()
            .filter(|n| !self.slots.contains_key(&n.id))
            .count();
        if self.slots.len() + fresh > self.meters.capacity() {
            self.meters = MeterBank::new((routing.nodes.len() * 2).max(MIN_METERS));
            self.slots.clear();
        }
        for node in &routing.nodes {
            let next = self.slots.len() as u32;
            self.slots.entry(node.id).or_insert(next);
        }
        let slots = &self.slots;
        let plan = compile_metered(routing, Some(Arc::clone(&self.meters)), |id| {
            slots.get(&id).copied().unwrap_or(plan::NO_METER)
        })?;
        let compensation = plan.compensation().clone();
        let sent = self.incoming.push(Box::new(plan)).is_ok();
        if sent {
            self.compensation = compensation;
        }
        Ok(sent)
    }

    /// The delay compensation of the last routing sent: how far the
    /// device outputs trail the tracks, and the latent nodes on the
    /// longest path.
    pub fn compensation(&self) -> &Compensation {
        &self.compensation
    }

    /// What node `id` played since its level was last taken: peaks and the
    /// clip since then, and its RMS now. `None` for a node not routed.
    pub fn take_level(&self, id: u64) -> Option<Level> {
        self.slots.get(&id).map(|&slot| self.meters.take(slot))
    }

    /// Hands the mean square of every 100 ms loudness block the measured
    /// node finished since the last call to `block`, oldest first, for
    /// `lumen_core::loudness::Loudness`.
    pub fn take_loudness(&mut self, block: impl FnMut(f32)) {
        self.loudness.drain(block);
    }

    /// The measured node's highest true peak since the last call (linear).
    pub fn take_true_peak(&self) -> f32 {
        self.loudness.take_true_peak()
    }

    /// Frees plans the audio side has replaced. Call regularly.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

/// Audio side: runs the plan over what `T` plays. The engine's [`Source`].
pub struct Mixer<T> {
    tracks: T,
    incoming: rtrb::Consumer<Box<Plan>>,
    retired: rtrb::Producer<Box<Plan>>,
    /// A replaced plan that did not fit in the retired queue yet.
    stash: Option<Box<Plan>>,
    plan: Box<Plan>,
    /// The plan `plan` replaced, while it fades out.
    swap: Option<Swap>,
    /// Loudness and true peak of the measured node.
    loudness: LoudnessTap,
    /// The timeline sample the tracks play next.
    cursor: i64,
    /// A jump being blended where the tracks play in (latent plans only).
    blend: Option<Blend>,
}

/// Frames blended so far of `length` (0 until the first block sets it
/// from the device rate).
struct Blend {
    done: usize,
    length: usize,
}

impl Blend {
    /// A blend from what the plan's input tail holds.
    fn new() -> Self {
        Self { done: 0, length: 0 }
    }
}

struct Swap {
    old: Box<Plan>,
    /// Frames played of `length` (set on the first block, from its rate).
    done: usize,
    length: usize,
}

/// Creates the mixer pair around `tracks`, starting with an empty routing
/// (silence). Allocates, so call it off the audio thread.
pub fn mixer<T: TrackSource>(tracks: T) -> (MixerHandle, Mixer<T>) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    let (tap, reader) = meter::loudness();
    (
        MixerHandle {
            incoming: incoming_tx,
            retired: retired_rx,
            meters: MeterBank::new(MIN_METERS),
            slots: HashMap::new(),
            loudness: reader,
            compensation: Compensation::default(),
        },
        Mixer {
            tracks,
            incoming: incoming_rx,
            retired: retired_tx,
            stash: None,
            plan: Box::new(Plan::empty()),
            swap: None,
            loudness: tap,
            cursor: 0,
            blend: None,
        },
    )
}

impl<T> Mixer<T> {
    pub fn tracks(&self) -> &T {
        &self.tracks
    }

    pub fn tracks_mut(&mut self) -> &mut T {
        &mut self.tracks
    }

    /// Takes the newest plan if one arrived and no crossfade is running.
    /// With `fade`, a plan of a new shape crossfades in. Real-time safe.
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
        while self.stash.is_none()
            && let Ok(newer) = self.incoming.pop()
        {
            self.retire(std::mem::replace(&mut new, newer));
        }
        let reshaped = new.topology != self.plan.topology;
        // The old plan plays on only while a new shape crossfades in.
        new.carry(&mut self.plan, !(fade && reshaped));
        // A new plan has no input tail to blend from.
        self.blend = None;
        let relatent = new.latency() != self.plan.latency();
        let old = std::mem::replace(&mut self.plan, new);
        if fade && reshaped {
            self.swap = Some(Swap {
                old,
                done: 0,
                length: 0,
            });
            // New delay lines start silent: what the tracks play fades
            // into them, so it comes out of them without a step.
            if relatent && self.plan.input_tail(0).is_some() {
                self.blend = Some(Blend::new());
            }
        } else {
            self.retire(old);
        }
    }

    /// Hands `old` back to be freed, keeping it if the queue is full. Only
    /// called while the stash is empty.
    fn retire(&mut self, old: Box<Plan>) {
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

impl<T: TrackSource> Source for Mixer<T> {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
        if !cx.tail {
            self.take_new(true);
        }
        let rate = cx.sample_rate;
        let advance = !cx.tail;
        let frames = out.frames();
        let mut done = 0;
        while done < frames {
            let n = CHUNK.min(frames - done);
            let part_cx = RenderContext {
                start: cx.start + done as i64,
                stream: cx.stream + done as u64,
                device: cx.device + done as u64,
                ..*cx
            };
            self.plan.clear(n);
            self.tracks.render(&part_cx, &mut self.plan.buses(n));
            if let Some(swap) = self.swap.as_mut() {
                swap.old.clear(n);
                swap.old.copy_inputs(&self.plan, n);
            }
            // Only the plan the jump or the new latency belongs to blends.
            if advance {
                if let Some(blend) = self.blend.as_mut() {
                    if blend.length == 0 {
                        blend.length = fade_frames(rate).min(CHUNK);
                    }
                    self.plan.blend_inputs(blend.done, n, blend.length);
                    blend.done += n;
                }
                self.blend.take_if(|b| b.done >= b.length);
                self.cursor = part_cx.start + n as i64;
            }
            let mut part = out.part(done, n);
            match self.swap.as_mut() {
                None => {
                    let tap = Some(&mut self.loudness);
                    self.plan.process(&mut part, rate, Ramp::Full, advance, tap);
                }
                Some(swap) => {
                    if swap.length == 0 {
                        swap.length = (rate / SWAP_PER_SECOND).max(1) as usize;
                    }
                    // In and out always add up to one.
                    let mut fade_in = [0.0f32; CHUNK];
                    let mut fade_out = [0.0f32; CHUNK];
                    for k in 0..n {
                        let x = ((swap.done + k) as f32 + 0.5) / swap.length as f32;
                        fade_in[k] = x.min(1.0);
                        fade_out[k] = 1.0 - fade_in[k];
                    }
                    // Only the plan fading in is metered.
                    let ramp_in = Ramp::Fade(&fade_in[..n]);
                    let tap = Some(&mut self.loudness);
                    self.plan.process(&mut part, rate, ramp_in, advance, tap);
                    let ramp_out = Ramp::Fade(&fade_out[..n]);
                    swap.old.process(&mut part, rate, ramp_out, advance, None);
                    if advance {
                        swap.done += n;
                    }
                }
            }
            if advance {
                self.finish_swap(false);
            }
            done += n;
        }
    }

    fn idle(&mut self, sample_rate: u32, frames: usize) {
        // Routings and clip lists sent while stopped are taken as they
        // come: a full queue would hold the UI's next ones back until Play,
        // and playback would start on stale ones (found by the M4 exit
        // walkthrough). Nothing plays, so nothing crossfades.
        self.finish_swap(true);
        self.take_new(false);
        self.tracks.idle();
        self.plan.idle_meters(sample_rate, frames);
    }

    fn locate(&mut self, timeline: &Timeline, sample_rate: u32, sample: i64, jump: Jump) {
        self.blend = None;
        if jump != Jump::Play && self.plan.latency() > 0 {
            // Latent: the engine's crossfade would come before the jump is
            // heard. Blend where the tracks play in instead, from what they
            // would have played next; a plan crossfade carries on.
            let cx = RenderContext {
                timeline,
                sample_rate,
                start: self.cursor,
                tail: true,
                stopping: false,
                mode: Mode::Play,
                stream: 0,
                device: 0,
            };
            let length = fade_frames(sample_rate).min(CHUNK);
            if let Some(mut tail) = self.plan.input_tail(length) {
                self.tracks.render(&cx, &mut tail);
                self.blend = Some(Blend::new());
            }
        } else {
            // The engine crossfades the jump: a plan crossfade ends here,
            // and levels start at their targets rather than gliding.
            self.finish_swap(true);
            self.take_new(false);
            self.plan.settle();
            self.blend = None;
            if self.plan.latency() > 0 {
                // Playback starts: the delay lines forget what played
                // before, and the tracks fade in from silence.
                self.plan.clear_lines();
                if self.plan.input_tail(0).is_some() {
                    self.blend = Some(Blend::new());
                }
            }
        }
        self.tracks.locate(timeline, sample_rate, sample, jump);
        self.cursor = sample;
    }

    fn latency(&self) -> u32 {
        self.plan.latency()
    }

    fn stopped(&mut self) {
        self.tracks.stopped();
    }

    fn panic(&mut self) {
        self.tracks.panic();
    }

    fn ringing(&self) -> bool {
        self.tracks.ringing()
    }
}
