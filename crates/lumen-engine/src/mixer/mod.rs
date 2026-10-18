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
mod hosts;
mod meter;
mod plan;
mod smooth;
mod spec;

pub use compile::compile;
pub use delay::MAX_DEVICE_LATENCY;
pub use hosts::MAX_HOSTS;
pub use meter::{Level, RMS_SECONDS};
pub use plan::{BUS_CHANNELS, Bus, Buses, CHUNK, Plan};

pub use spec::{
    Compensation, CueOut, Destination, Device, NodeSpec, Routing, RoutingError, SendSpec,
    SidechainSpec, Tap,
};

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use compile::compile_metered;
use hosts::{HostMsg, HostPool, Hosting};
use lumen_devices::Guard;
use meter::{LoudnessReader, LoudnessTap, MeterBank, SIDE_KEY};
use plan::Ramp;

use crate::declick::fade_frames;
use crate::source::{Jump, Mode, Output, RenderContext, Source};
use crate::timeline::Timeline;

const QUEUE: usize = 4;
/// Attach and detach messages waiting for the audio side.
const HOST_QUEUE: usize = 64;
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

    /// A plugin host is attached to this source, as [`Source::hosted`].
    fn hosted(&self) -> bool {
        false
    }

    /// Called before [`TrackSource::render`] for the same `frames`, ahead of
    /// the engine's other work for the block: a source whose instruments run
    /// in plugin hosts hands them their blocks here, so the hosts work while
    /// the audio clips render, and collects them in `render` (ADR-0069). A
    /// source that does not need it ignores the call, and `render` does
    /// everything.
    fn begin(&mut self, _cx: &RenderContext<'_>, _frames: usize) {}
}

/// Two track sources played together into the same buses: the audio
/// clips and the MIDI instruments (M5 slice 1).
pub struct Layered<A, B>(pub A, pub B);

impl<A: TrackSource, B: TrackSource> TrackSource for Layered<A, B> {
    fn render(&mut self, cx: &RenderContext<'_>, buses: &mut Buses<'_>) {
        self.0.render(cx, buses);
        self.1.render(cx, buses);
    }

    fn begin(&mut self, cx: &RenderContext<'_>, frames: usize) {
        self.0.begin(cx, frames);
        self.1.begin(cx, frames);
    }

    fn hosted(&self) -> bool {
        self.0.hosted() || self.1.hosted()
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
    /// Hosts on their way to the audio side, and ones it let go of.
    hosts: rtrb::Producer<HostMsg>,
    hosts_retired: rtrb::Consumer<Guard>,
    /// A host message waits: set here, cleared when the audio side takes
    /// them, so a callback with none to take reads one flag.
    hosts_pending: Arc<AtomicBool>,
}

impl MixerHandle {
    /// Attaches a plugin host as device `id`, the id a
    /// [`Device::Hosted`] names. A host already attached under `id` is
    /// replaced (and freed here, by [`MixerHandle::collect_garbage`]).
    /// Gives the guard back when the queue is full: try again next frame.
    pub fn attach_host(&mut self, id: u64, guard: Guard) -> Result<(), Guard> {
        match self.hosts.push(HostMsg::Attach(id, guard)) {
            Ok(()) => {
                self.hosts_pending.store(true, Ordering::Release);
                Ok(())
            }
            Err(rtrb::PushError::Full(HostMsg::Attach(_, guard))) => Err(guard),
            Err(rtrb::PushError::Full(HostMsg::Detach(_))) => unreachable!("an attach was sent"),
        }
    }

    /// Takes the host `id` away. Returns whether the message was queued.
    pub fn detach_host(&mut self, id: u64) -> bool {
        let sent = self.hosts.push(HostMsg::Detach(id)).is_ok();
        if sent {
            self.hosts_pending.store(true, Ordering::Release);
        }
        sent
    }

    /// Compiles `routing` and sends it. Returns whether it was sent: false
    /// when the queue is full (send it again next frame). A routing that
    /// cannot play is refused, and the current one keeps playing.
    pub fn set(&mut self, routing: &Routing) -> Result<bool, RoutingError> {
        // A node's slot, and one more for a node with a sidechain, which
        // reads what arrives for it.
        let keys = |node: &NodeSpec| {
            [
                Some(node.id),
                (!node.sidechains.is_empty()).then_some(node.id | SIDE_KEY),
            ]
        };
        let fresh = routing
            .nodes
            .iter()
            .flat_map(keys)
            .flatten()
            .filter(|key| !self.slots.contains_key(key))
            .count();
        if self.slots.len() + fresh > self.meters.capacity() {
            self.meters = MeterBank::new((routing.nodes.len() * 3).max(MIN_METERS));
            self.slots.clear();
        }
        for key in routing.nodes.iter().flat_map(keys).flatten() {
            let next = self.slots.len() as u32;
            self.slots.entry(key).or_insert(next);
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

    /// What arrived for node `id`'s sidechain since it was last taken
    /// (peaks and the clip since then, RMS now). `None` for a node without
    /// one.
    pub fn take_side_level(&self, id: u64) -> Option<Level> {
        self.slots
            .get(&(id | SIDE_KEY))
            .map(|&slot| self.meters.take(slot))
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
        // A host dropped here closes its link to its process.
        while self.hosts_retired.pop().is_ok() {
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
    /// The plugin hosts the plan's hosted devices find by id.
    hosts: HostPool,
    host_msgs: rtrb::Consumer<HostMsg>,
    host_retired: rtrb::Producer<Guard>,
    hosts_pending: Arc<AtomicBool>,
    /// The device rate of the last block (0 before the first), for how long
    /// a host's quiet must last.
    rate: u32,
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
    let (host_tx, host_rx) = rtrb::RingBuffer::new(HOST_QUEUE);
    let (host_retired_tx, host_retired_rx) = rtrb::RingBuffer::new(HOST_QUEUE + 1);
    let hosts_pending = Arc::new(AtomicBool::new(false));
    (
        MixerHandle {
            incoming: incoming_tx,
            retired: retired_rx,
            meters: MeterBank::new(MIN_METERS),
            slots: HashMap::new(),
            loudness: reader,
            compensation: Compensation::default(),
            hosts: host_tx,
            hosts_retired: host_retired_rx,
            hosts_pending: Arc::clone(&hosts_pending),
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
            hosts: HostPool::new(),
            host_msgs: host_rx,
            host_retired: host_retired_tx,
            hosts_pending,
            rate: 0,
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
        // Skipping a list needs room for it and for the one it replaces
        // in the end, or the stash would be overwritten and a list freed
        // here; newer lists then wait a block.
        while self.retired.slots() >= 2
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

    /// Applies the attach and detach messages that arrived. Real-time safe:
    /// the pool and the queues have a fixed size, and a message waits while
    /// the queue that takes the replaced host back is full, so a host is
    /// never dropped here.
    #[inline]
    fn take_hosts(&mut self) {
        // A callback with nothing to take reads one flag.
        if self.hosts_pending.load(Ordering::Relaxed) {
            self.apply_hosts();
        }
    }

    #[inline(never)]
    fn apply_hosts(&mut self) {
        // Cleared before the queue is read, so a message sent meanwhile sets
        // it again and is not lost.
        if !self.hosts_pending.swap(false, Ordering::AcqRel) {
            return;
        }
        while self.host_retired.slots() >= 1 {
            let Ok(message) = self.host_msgs.pop() else {
                break;
            };
            let gone = match message {
                HostMsg::Attach(id, guard) => match self.hosts.insert(id, guard) {
                    Ok(replaced) => replaced,
                    Err(refused) => Some(refused),
                },
                HostMsg::Detach(id) => self.hosts.remove(id),
            };
            if let Some(guard) = gone {
                // Room was checked above, and nothing else pushes here.
                let _ = self.host_retired.push(guard);
            }
        }
        // Messages left because the retired queue was full wait for the next
        // block.
        if !self.host_msgs.is_empty() {
            self.hosts_pending.store(true, Ordering::Release);
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
            self.take_hosts();
            self.rate = cx.sample_rate;
        }
        let rate = cx.sample_rate;
        let advance = !cx.tail;
        // A scrub is heard as it plays: no path waits for another's latency.
        let scrubbing = cx.mode == Mode::Scrub;
        self.plan.lines.set_bypass(scrubbing);
        if let Some(swap) = self.swap.as_mut() {
            swap.old.lines.set_bypass(scrubbing);
        }
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
            // Hosts start their blocks before the clips render.
            self.tracks.begin(&part_cx, n);
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
                    let mut hosting = Hosting {
                        pool: Some(&mut self.hosts),
                        deadline: part_cx.deadline,
                    };
                    self.plan
                        .process(&mut part, rate, Ramp::Full, advance, tap, &mut hosting);
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
                    // The plan fading out plays its fallbacks: a host is
                    // asked once for a block.
                    let mut hosting = Hosting {
                        pool: Some(&mut self.hosts),
                        deadline: part_cx.deadline,
                    };
                    self.plan
                        .process(&mut part, rate, ramp_in, advance, tap, &mut hosting);
                    let ramp_out = Ramp::Fade(&fade_out[..n]);
                    swap.old.process(
                        &mut part,
                        rate,
                        ramp_out,
                        advance,
                        None,
                        &mut Hosting::none(),
                    );
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
        if advance {
            self.hosts.age(frames);
        }
    }

    fn idle(&mut self, sample_rate: u32, frames: usize) {
        self.rate = sample_rate;
        // Routings and clip lists sent while stopped are taken as they
        // come: a full queue would hold the UI's next ones back until Play,
        // and playback would start on stale ones (found by the M4 exit
        // walkthrough). Nothing plays, so nothing crossfades.
        self.finish_swap(true);
        self.take_new(false);
        self.take_hosts();
        self.tracks.idle();
        self.plan.idle_meters(sample_rate, frames);
    }

    fn locate(&mut self, timeline: &Timeline, sample_rate: u32, sample: i64, jump: Jump) {
        self.blend = None;
        if jump == Jump::Locate {
            // A move of the playhead in the song: no effect's tail carries
            // over to the new place (the engine crossfades the jump itself).
            self.hosts.flush();
        }
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
                recording: false,
                looping: None,
                deadline: None,
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

    fn hosted(&self) -> bool {
        // Hosts waiting to be attached count, so the callback that takes
        // them already has its deadline.
        self.tracks.hosted() || self.hosts.len() > 0 || self.hosts_pending.load(Ordering::Relaxed)
    }

    fn stopped(&mut self) {
        self.tracks.stopped();
    }

    fn panic(&mut self) {
        self.tracks.panic();
    }

    fn ringing(&self) -> bool {
        // A plugin effect rings out after a Stop until its output has been
        // quiet for a while: a delay or a reverb is not cut off.
        self.tracks.ringing() || self.hosts.sounding(self.rate)
    }
}
