//! The flat processing plan the audio thread runs ([`Plan`]), compiled
//! from a routing by [`super::compile`]. On the audio thread a plan only
//! reads and writes what it already holds.

mod buses;
mod node;
mod process;

pub use buses::{Bus, Buses};
pub(super) use node::{Dev, NO_DEV, NO_METER, NO_SIDE, Node, Out, Pass, Ramp, Send, TapOut};

use std::sync::Arc;

use super::builtins::NO_BUILTIN;
use super::delay::Lines;
use super::hosts::{HostScratch, NO_HOST};
use super::meter::{MeterBank, keep};
use super::smooth::Smoothed;
use super::spec::Compensation;
pub use super::spec::Tap;

/// Frames a plan processes at a time; longer blocks are split.
pub const CHUNK: usize = 256;
// Plugin hosts are configured for blocks of at most this many frames.
const _: () = assert!(CHUNK <= lumen_devices::MOST_FRAMES);
/// Channels on every bus: stereo.
pub const BUS_CHANNELS: usize = 2;
pub(super) const BUS: usize = CHUNK * BUS_CHANNELS;

/// A compiled routing: nodes in processing order, each with a stereo bus.
pub struct Plan {
    pub(super) nodes: Vec<Node>,
    pub(super) sends: Vec<Send>,
    pub(super) devices: Vec<Dev>,
    pub(super) taps: Vec<TapOut>,
    /// Delay lines: compensation and latent devices.
    pub(super) lines: Lines,
    /// Where a path is delayed on its way out (`Lines::through`): both
    /// sides, `CHUNK` frames each. Held here rather than zeroed on the
    /// stack every block.
    pub(super) scratch: Box<[f32; 2 * CHUNK]>,
    /// Where hosted devices work, and whether the plan has any.
    pub(super) host_scratch: Box<HostScratch>,
    pub(super) hosted: bool,
    /// The plan names a built-in effect, so the song's tempo is read each
    /// part for it (M8 slice B3); a project without one does no such work.
    pub(super) builtins: bool,
    /// `(id, index)` sorted by id: where tracks find their bus.
    pub(super) by_id: Vec<(u64, u32)>,
    /// Every node's bus, `BUS` samples each: left then right, `CHUNK`
    /// frames per side.
    pub(super) buses: Vec<f32>,
    /// Sidechain buses, laid out as `buses`.
    pub(super) sides: Vec<f32>,
    /// What the tracks would have played after a jump, laid out as
    /// `buses`, to blend from (only in a latent plan).
    pub(super) input_tail: Vec<f32>,
    /// Identifies the routing's shape (order, outputs, sends, devices,
    /// sidechains and delay lines, not levels): a plan with another shape
    /// crossfades in.
    pub(super) topology: u64,
    /// The cue bus: `CHUNK` frames a side, as a node's.
    pub(super) cue_bus: Vec<f32>,
    /// Where the cue bus is heard (its first channel), and how loud.
    pub(super) cue_channel: Option<u16>,
    pub(super) cue_volume: Smoothed,
    /// Where the nodes' levels go, when metered.
    pub(super) meters: Option<Arc<MeterBank>>,
    /// The index of the node whose loudness is measured.
    pub(super) measured: Option<u32>,
    pub(super) compensation: Compensation,
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("nodes", &self.nodes.len())
            .field("topology", &self.topology)
            .field("compensation", &self.compensation.frames)
            .finish()
    }
}

impl Plan {
    /// An empty plan: silence.
    pub fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            sends: Vec::new(),
            devices: Vec::new(),
            taps: Vec::new(),
            lines: Lines::default(),
            scratch: Box::new([0.0; 2 * CHUNK]),
            host_scratch: HostScratch::new(),
            hosted: false,
            builtins: false,
            by_id: Vec::new(),
            buses: Vec::new(),
            sides: Vec::new(),
            input_tail: Vec::new(),
            topology: 0,
            cue_bus: Vec::new(),
            cue_channel: None,
            cue_volume: Smoothed::new(0.0),
            meters: None,
            measured: None,
            compensation: Compensation::default(),
        }
    }

    /// The delay compensation this plan applies.
    pub fn compensation(&self) -> &Compensation {
        &self.compensation
    }

    /// Frames the device outputs trail what the tracks play.
    pub(super) fn latency(&self) -> u32 {
        self.compensation.frames
    }

    /// Zeroes the first `frames` of every bus.
    pub(super) fn clear(&mut self, frames: usize) {
        for buses in [&mut self.buses, &mut self.sides, &mut self.cue_bus] {
            for bus in buses.as_chunks_mut::<BUS>().0 {
                bus[..frames].fill(0.0);
                bus[CHUNK..CHUNK + frames].fill(0.0);
            }
        }
    }

    /// Silences every delay line: playback starts. Real-time safe.
    pub(super) fn clear_lines(&mut self) {
        self.lines.clear();
    }

    pub(super) fn buses(&mut self, frames: usize) -> Buses<'_> {
        Buses {
            by_id: &self.by_id,
            data: &mut self.buses,
            frames,
        }
    }

    /// The tracks' buses for what they would have played after a jump,
    /// zeroed: `None` unless the plan is latent.
    pub(super) fn input_tail(&mut self, frames: usize) -> Option<Buses<'_>> {
        if self.input_tail.is_empty() {
            return None;
        }
        self.input_tail.fill(0.0);
        Some(Buses {
            by_id: &self.by_id,
            data: &mut self.input_tail,
            frames,
        })
    }

    /// Blends `frames` of the input tail, from its frame `done` of
    /// `length`, out of every bus as the new input comes in. Real-time
    /// safe.
    pub(super) fn blend_inputs(&mut self, done: usize, frames: usize, length: usize) {
        if self.input_tail.is_empty() {
            return;
        }
        let tails = self.input_tail.as_chunks::<BUS>().0;
        for (bus, tail) in self.buses.as_chunks_mut::<BUS>().0.iter_mut().zip(tails) {
            for side in [0, CHUNK] {
                for k in 0..frames {
                    let x = (((done + k) as f32 + 0.5) / length as f32).min(1.0);
                    let at = done + k;
                    let old = if at < CHUNK { tail[side + at] } else { 0.0 };
                    bus[side + k] = bus[side + k] * x + old * (1.0 - x);
                }
            }
        }
    }

    /// Copies the first `frames` of `from`'s buses into the buses of the
    /// same nodes here: what the tracks played, for a plan fading out.
    pub(super) fn copy_inputs(&mut self, from: &Plan, frames: usize) {
        let (mut i, mut j) = (0, 0);
        while let (Some(&(a, here)), Some(&(b, there))) = (self.by_id.get(i), from.by_id.get(j)) {
            match a.cmp(&b) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    let (here, there) = (here as usize * BUS, there as usize * BUS);
                    for side in [0, CHUNK] {
                        let source = &from.buses[there + side..there + side + frames];
                        self.buses[here + side..here + side + frames].copy_from_slice(source);
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
    }

    /// Takes over the current levels of the nodes and sends that `old`
    /// also has, so they glide to their new values, and what is in flight
    /// in the delay lines they share. Real-time safe: merges over sorted
    /// lists, a short search per send, and a copy per delay line.
    ///
    /// With `retiring`, `old` will not play again, so delay lines laid out
    /// alike are taken over without a copy ([`Lines::take_over`]).
    pub(super) fn carry(&mut self, old: &mut Plan, retiring: bool) {
        if old.cue_channel.is_some() {
            self.cue_volume.carry(&old.cue_volume);
        }
        if retiring {
            self.lines.take_over(&mut old.lines);
        } else {
            self.lines.carry(&old.lines);
        }
        let (mut i, mut j) = (0, 0);
        while let (Some(&(a, here)), Some(&(b, there))) = (self.by_id.get(i), old.by_id.get(j)) {
            match a.cmp(&b) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    let before = &old.nodes[there as usize];
                    let node = &mut self.nodes[here as usize];
                    node.pre.carry(&before.pre);
                    node.heard.carry(&before.heard);
                    node.gate.carry(&before.gate);
                    node.cue.carry(&before.cue);
                    node.post[0].carry(&before.post[0]);
                    node.post[1].carry(&before.post[1]);
                    if let (Some(new), Some(was)) = (node.auto.as_deref_mut(), &before.auto) {
                        new.carry(was);
                    }
                    // A hosted device keeps where it was in the glide
                    // between its host and its fallback; a built-in keeps where
                    // it was between its dry signal and its own.
                    let mine =
                        node.first_device as usize..(node.first_device + node.devices) as usize;
                    let theirs = before.first_device as usize
                        ..(before.first_device + before.devices) as usize;
                    for (dev, was) in self.devices[mine].iter_mut().zip(&old.devices[theirs]) {
                        if dev.host != NO_HOST && dev.host == was.host {
                            dev.wet.carry(&was.wet);
                        }
                        if dev.builtin != NO_BUILTIN && dev.builtin == was.builtin {
                            dev.wet.carry(&was.wet);
                        }
                    }
                    let mine = node.first as usize..(node.first + node.count) as usize;
                    let theirs = before.first as usize..(before.first + before.count) as usize;
                    for send in &mut self.sends[mine] {
                        if let Some(was) = old.sends[theirs.clone()]
                            .iter()
                            .find(|s| s.to_id == send.to_id && s.pre_fader == send.pre_fader)
                        {
                            send.gain.carry(&was.gain);
                        }
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
    }

    /// `frames` passed with nothing played: every node's RMS falls as in
    /// silence. Real-time safe: a load and a store per side and node.
    pub(super) fn idle_meters(&self, rate: u32, frames: usize) {
        let Some(meters) = self.meters.as_deref() else {
            return;
        };
        let keep = keep(frames, rate);
        for node in &self.nodes {
            meters.silence(node.meter, keep);
        }
    }

    /// Every level jumps to its target (after a transport jump).
    pub(super) fn settle(&mut self) {
        for node in &mut self.nodes {
            node.pre.settle();
            node.heard.settle();
            node.gate.settle();
            node.cue.settle();
            node.post.iter_mut().for_each(Smoothed::settle);
            if let Some(auto) = node.auto.as_deref_mut() {
                auto.settle();
            }
        }
        for send in &mut self.sends {
            send.gain.settle();
        }
        // A built-in is where its power says it is: playback starts (or jumps)
        // with the device heard or not, not fading in from silence, which is
        // what makes an export play what playback plays frame for frame
        // (M8 slice B0, review L9).
        for dev in &mut self.devices {
            if dev.builtin != NO_BUILTIN {
                dev.wet.set(if dev.bypass { 0.0 } else { 1.0 });
                dev.wet.settle();
            }
        }
        self.cue_volume.settle();
    }
}
