mod auto;
mod builtins;
mod compile;
mod delay;
mod hosts;
mod meter;
mod params;
mod plan;
mod smooth;
mod spec;

pub(crate) use auto::Kind as LaneKind;
pub use auto::{AutoLane, AutoPoint, KNOT, NodeAutomation, db_gain};
pub use builtins::{MAX_BUILTINS, Slot};
pub use compile::compile;
pub use delay::MAX_DEVICE_LATENCY;
pub use hosts::MAX_HOSTS;
pub use meter::{Level, RMS_SECONDS};
pub use params::{ParamLane, fill as fill_param_lane};
pub use plan::{BUS_CHANNELS, Bus, Buses, CHUNK, Plan};

pub use spec::{
    Compensation, CueOut, Destination, Device, NodeSpec, Routing, RoutingError, SendSpec,
    SidechainSpec, Tap,
};

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use builtins::{BuiltinPool, DeviceMsg};
use compile::compile_metered;
use hosts::{HostMsg, HostPool, Hosting};
use lumen_devices::Guard;
use meter::{LoudnessReader, LoudnessTap, MeterBank, SIDE_KEY};
use plan::{Pass, Ramp};

use crate::declick::fade_frames;
use crate::source::{Inputs, Jump, Mode, Output, RenderContext, Source};
use crate::timeline::Timeline;

const QUEUE: usize = 4;
const HOST_QUEUE: usize = 64;
const DEVICE_QUEUE: usize = builtins::MAX_BUILTINS;
const SWAP_PER_SECOND: u32 = 200;
pub trait TrackSource: Send {
    fn render(&mut self, cx: &RenderContext<'_>, buses: &mut Buses<'_>);

    fn input(&mut self, _inputs: &Inputs<'_>) {}

    fn locate(&mut self, _timeline: &Timeline, _sample_rate: u32, _sample: i64, _jump: Jump) {}

    fn idle(&mut self) {}

    fn stopped(&mut self) {}

    fn panic(&mut self) {}

    fn ringing(&self) -> bool {
        false
    }

    fn hosted(&self) -> bool {
        false
    }

    fn begin(&mut self, _cx: &RenderContext<'_>, _frames: usize) {}
}

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

    fn input(&mut self, inputs: &Inputs<'_>) {
        self.0.input(inputs);
        self.1.input(inputs);
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

const MIN_METERS: usize = 64;

pub struct MixerHandle {
    incoming: rtrb::Producer<Box<Plan>>,
    retired: rtrb::Consumer<Box<Plan>>,
    meters: Arc<MeterBank>,
    slots: HashMap<u64, u32>,
    loudness: LoudnessReader,
    compensation: Compensation,
    hosts: rtrb::Producer<HostMsg>,
    hosts_retired: rtrb::Consumer<Guard>,
    hosts_pending: Arc<AtomicBool>,
    devices: rtrb::Producer<DeviceMsg>,
    devices_retired: rtrb::Consumer<Box<Slot>>,
    devices_refused: rtrb::Consumer<(u64, Box<Slot>)>,
    devices_pending: Arc<AtomicBool>,
}

impl MixerHandle {
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

    pub fn detach_host(&mut self, id: u64) -> bool {
        let sent = self.hosts.push(HostMsg::Detach(id)).is_ok();
        if sent {
            self.hosts_pending.store(true, Ordering::Release);
        }
        sent
    }

    pub fn attach_builtin(&mut self, id: u64, slot: Box<Slot>) -> Result<(), Box<Slot>> {
        match self.devices.push(DeviceMsg::Attach(id, slot)) {
            Ok(()) => {
                self.devices_pending.store(true, Ordering::Release);
                Ok(())
            }
            Err(rtrb::PushError::Full(DeviceMsg::Attach(_, slot))) => Err(slot),
            Err(rtrb::PushError::Full(DeviceMsg::Detach(_))) => unreachable!("an attach was sent"),
        }
    }

    pub fn detach_builtin(&mut self, id: u64) -> bool {
        let sent = self.devices.push(DeviceMsg::Detach(id)).is_ok();
        if sent {
            self.devices_pending.store(true, Ordering::Release);
        }
        sent
    }

    pub fn take_refused_builtins(&mut self) -> Vec<u64> {
        let mut refused = Vec::new();
        while let Ok((id, slot)) = self.devices_refused.pop() {
            drop(slot);
            refused.push(id);
        }
        refused
    }

    pub fn set(&mut self, routing: &Routing) -> Result<bool, RoutingError> {
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

    pub fn compensation(&self) -> &Compensation {
        &self.compensation
    }

    pub fn take_level(&self, id: u64) -> Option<Level> {
        self.slots.get(&id).map(|&slot| self.meters.take(slot))
    }

    pub fn take_side_level(&self, id: u64) -> Option<Level> {
        self.slots
            .get(&(id | SIDE_KEY))
            .map(|&slot| self.meters.take(slot))
    }

    pub fn take_loudness(&mut self, block: impl FnMut(f32)) {
        self.loudness.drain(block);
    }

    pub fn take_true_peak(&self) -> f32 {
        self.loudness.take_true_peak()
    }

    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        while self.hosts_retired.pop().is_ok() {
            freed += 1;
        }
        while self.devices_retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

pub struct Mixer<T> {
    tracks: T,
    incoming: rtrb::Consumer<Box<Plan>>,
    retired: rtrb::Producer<Box<Plan>>,
    stash: Option<Box<Plan>>,
    plan: Box<Plan>,
    swap: Option<Swap>,
    loudness: LoudnessTap,
    cursor: i64,
    blend: Option<Blend>,
    hosts: HostPool,
    host_msgs: rtrb::Consumer<HostMsg>,
    host_retired: rtrb::Producer<Guard>,
    hosts_pending: Arc<AtomicBool>,
    builtins: BuiltinPool,
    device_msgs: rtrb::Consumer<DeviceMsg>,
    device_retired: rtrb::Producer<Box<Slot>>,
    device_refused: rtrb::Producer<(u64, Box<Slot>)>,
    device_held: Option<(u64, Box<Slot>)>,
    devices_pending: Arc<AtomicBool>,
    rate: u32,
}

struct Blend {
    done: usize,
    length: usize,
}

impl Blend {
    fn new() -> Self {
        Self { done: 0, length: 0 }
    }
}

struct Swap {
    old: Box<Plan>,
    done: usize,
    length: usize,
}

pub fn mixer<T: TrackSource>(tracks: T) -> (MixerHandle, Mixer<T>) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    let (tap, reader) = meter::loudness();
    let (host_tx, host_rx) = rtrb::RingBuffer::new(HOST_QUEUE);
    let (host_retired_tx, host_retired_rx) = rtrb::RingBuffer::new(HOST_QUEUE + 1);
    let hosts_pending = Arc::new(AtomicBool::new(false));
    let (device_tx, device_rx) = rtrb::RingBuffer::new(DEVICE_QUEUE);
    let (device_retired_tx, device_retired_rx) = rtrb::RingBuffer::new(DEVICE_QUEUE + 1);
    let (device_refused_tx, device_refused_rx) = rtrb::RingBuffer::new(DEVICE_QUEUE + 1);
    let devices_pending = Arc::new(AtomicBool::new(false));
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
            devices: device_tx,
            devices_retired: device_retired_rx,
            devices_refused: device_refused_rx,
            devices_pending: Arc::clone(&devices_pending),
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
            builtins: BuiltinPool::new(),
            device_msgs: device_rx,
            device_retired: device_retired_tx,
            device_refused: device_refused_tx,
            device_held: None,
            devices_pending,
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
        while self.retired.slots() >= 2
            && let Ok(newer) = self.incoming.pop()
        {
            self.retire(std::mem::replace(&mut new, newer));
        }
        let reshaped = new.topology != self.plan.topology;
        new.carry(&mut self.plan, !(fade && reshaped));
        self.blend = None;
        let relatent = new.latency() != self.plan.latency();
        let old = std::mem::replace(&mut self.plan, new);
        if fade && reshaped {
            self.swap = Some(Swap {
                old,
                done: 0,
                length: 0,
            });
            if relatent && self.plan.input_tail(0).is_some() {
                self.blend = Some(Blend::new());
            }
        } else {
            self.retire(old);
        }
    }

    #[inline]
    fn take_hosts(&mut self) {
        if self.hosts_pending.load(Ordering::Relaxed) {
            self.apply_hosts();
        }
        if self.devices_pending.load(Ordering::Relaxed) {
            self.apply_devices();
        }
    }

    #[inline(never)]
    fn apply_devices(&mut self) {
        if let Some(held) = self.device_held.take()
            && let Err(rtrb::PushError::Full(held)) = self.device_refused.push(held)
        {
            self.device_held = Some(held);
        }
        if !self.devices_pending.swap(false, Ordering::AcqRel) {
            return;
        }
        while self.device_retired.slots() >= 1 {
            let Ok(message) = self.device_msgs.pop() else {
                break;
            };
            let gone = match message {
                DeviceMsg::Attach(id, slot) => match self.builtins.insert(id, slot) {
                    Ok(replaced) => replaced,
                    Err(refused) => {
                        if let Err(rtrb::PushError::Full(held)) =
                            self.device_refused.push((id, refused))
                        {
                            debug_assert!(self.device_held.is_none(), "refusal ring full");
                            self.device_held = Some(held);
                            self.devices_pending.store(true, Ordering::Release);
                        }
                        None
                    }
                },
                DeviceMsg::Detach(id) => self.builtins.remove(id),
            };
            if let Some(slot) = gone {
                let _ = self.device_retired.push(slot);
            }
        }
        if !self.device_msgs.is_empty() {
            self.devices_pending.store(true, Ordering::Release);
        }
    }

    #[inline(never)]
    fn apply_hosts(&mut self) {
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
                let _ = self.host_retired.push(guard);
            }
        }
        if !self.host_msgs.is_empty() {
            self.hosts_pending.store(true, Ordering::Release);
        }
    }

    fn retire(&mut self, old: Box<Plan>) {
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
    }

    fn finish_swap(&mut self, now: bool) {
        if self.stash.is_some() {
            return;
        }
        if let Some(swap) = self.swap.take_if(|s| now || s.done >= s.length) {
            self.retire(swap.old);
        }
    }
}

impl<T: TrackSource> Mixer<T> {
    fn blend_from(&mut self, timeline: &Timeline, sample_rate: u32, mode: Mode) {
        let cx = RenderContext {
            timeline,
            sample_rate,
            start: self.cursor,
            tail: true,
            stopping: false,
            mode,
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
    }
}

impl<T: TrackSource> Source for Mixer<T> {
    fn input(&mut self, inputs: &Inputs<'_>) {
        self.tracks.input(inputs);
    }

    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
        if !cx.tail {
            self.take_new(true);
            self.take_hosts();
            self.rate = cx.sample_rate;
        }
        let rate = cx.sample_rate;
        let advance = !cx.tail;
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
            self.tracks.begin(&part_cx, n);
            self.tracks.render(&part_cx, &mut self.plan.buses(n));
            if let Some(swap) = self.swap.as_mut() {
                swap.old.clear(n);
                swap.old.copy_inputs(&self.plan, n);
            }
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
            let bpm = if self.plan.builtins {
                part_cx.timeline.bpm_at(part_cx.start, rate)
            } else {
                120.0
            };
            match self.swap.as_mut() {
                None => {
                    let tap = Some(&mut self.loudness);
                    let mut hosting = Hosting {
                        pool: Some(&mut self.hosts),
                        builtins: Some(&mut self.builtins),
                        fading: None,
                        deadline: part_cx.deadline,
                    };
                    let pass = Pass {
                        start: part_cx.start,
                        rate,
                        bpm,
                        ramp: Ramp::Full,
                        advance,
                    };
                    self.plan.process(&mut part, pass, tap, &mut hosting);
                }
                Some(swap) => {
                    if swap.length == 0 {
                        swap.length = (rate / SWAP_PER_SECOND).max(1) as usize;
                    }
                    let mut fade_in = [0.0f32; CHUNK];
                    let mut fade_out = [0.0f32; CHUNK];
                    for k in 0..n {
                        let x = ((swap.done + k) as f32 + 0.5) / swap.length as f32;
                        fade_in[k] = x.min(1.0);
                        fade_out[k] = 1.0 - fade_in[k];
                    }
                    let ramp_in = Ramp::Fade(&fade_in[..n]);
                    let tap = Some(&mut self.loudness);
                    let mut hosting = Hosting {
                        pool: Some(&mut self.hosts),
                        builtins: Some(&mut self.builtins),
                        fading: None,
                        deadline: part_cx.deadline,
                    };
                    let pass = Pass {
                        start: part_cx.start,
                        rate,
                        bpm,
                        ramp: ramp_in,
                        advance,
                    };
                    self.plan.process(&mut part, pass, tap, &mut hosting);
                    let pass = Pass {
                        ramp: Ramp::Fade(&fade_out[..n]),
                        ..pass
                    };
                    let mut fading = Hosting {
                        pool: None,
                        builtins: Some(&mut self.builtins),
                        fading: Some(&self.plan),
                        deadline: part_cx.deadline,
                    };
                    swap.old.process(&mut part, pass, None, &mut fading);
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
            self.builtins.age(frames);
        }
    }

    fn idle(&mut self, sample_rate: u32, frames: usize) {
        self.rate = sample_rate;
        self.finish_swap(true);
        self.take_new(false);
        self.take_hosts();
        self.tracks.idle();
        self.plan.idle_meters(sample_rate, frames);
    }

    fn locate(&mut self, timeline: &Timeline, sample_rate: u32, sample: i64, jump: Jump) {
        self.blend = None;
        if jump == Jump::Locate {
            self.hosts.flush();
            self.builtins.reset();
        }
        if jump != Jump::Play && self.plan.latency() > 0 {
            self.blend_from(timeline, sample_rate, Mode::Play);
        } else {
            self.finish_swap(true);
            self.take_new(false);
            self.plan.settle();
            self.blend = None;
            if self.plan.latency() > 0 {
                if self.tracks.ringing() {
                    self.blend_from(timeline, sample_rate, Mode::Ring);
                } else {
                    self.plan.clear_lines();
                    if self.plan.input_tail(0).is_some() {
                        self.blend = Some(Blend::new());
                    }
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
        self.tracks.hosted() || self.hosts.len() > 0 || self.hosts_pending.load(Ordering::Relaxed)
    }

    fn stopped(&mut self) {
        self.tracks.stopped();
    }

    fn panic(&mut self) {
        self.tracks.panic();
    }

    fn ringing(&self) -> bool {
        self.tracks.ringing() || self.hosts.sounding(self.rate) || self.builtins.sounding(self.rate)
    }
}
