//! The flat processing plan the audio thread runs ([`Plan`]), compiled
//! from a routing by [`super::compile`]. On the audio thread a plan only
//! reads and writes what it already holds.

use std::sync::Arc;

use super::delay::{LineId, Lines, NO_LINE};
use super::meter::{LoudnessTap, MeterBank, keep};
use super::smooth::Smoothed;
use super::spec::Compensation;
pub use super::spec::Tap;
use crate::source::Output;

/// Frames a plan processes at a time; longer blocks are split.
pub const CHUNK: usize = 256;
/// Channels on every bus: stereo.
pub const BUS_CHANNELS: usize = 2;
pub(super) const BUS: usize = CHUNK * BUS_CHANNELS;

/// Where a compiled node's output goes: an index into the plan's nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Out {
    Node(u32),
    Hardware(u16),
    None,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Send {
    /// Index into the plan's nodes, always after the sending node.
    pub to: u32,
    pub to_id: u64,
    pub pre_fader: bool,
    pub gain: Smoothed,
    pub line: LineId,
}

/// A compiled device: for now the synthetic delay (its line).
#[derive(Debug, Clone, Copy)]
pub(super) struct Dev {
    pub line: LineId,
    /// Adds the node's sidechain input before delaying.
    pub listen: bool,
}

/// A sidechain leaving a node.
#[derive(Debug, Clone, Copy)]
pub(super) struct TapOut {
    /// The receiving node's sidechain bus.
    pub to: u32,
    pub tap: Tap,
    pub line: LineId,
}

/// A node without a sidechain bus.
pub(super) const NO_SIDE: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub(super) struct Node {
    pub id: u64,
    /// Delays what the tracks play into the node, when it waits for a
    /// later input.
    pub input_line: LineId,
    /// Trim × polarity, and when not `split`, × (output or sends heard).
    pub pre: Smoothed,
    /// The node has devices or sidechains taken from it, so mute comes
    /// after them (`heard`) rather than in `pre`.
    pub split: bool,
    /// 1 while the output or sends are heard; used when `split`.
    pub heard: Smoothed,
    /// 1 while the output is heard, 0 while only the sends are.
    pub gate: Smoothed,
    /// Trim × polarity while cued, else 0.
    pub cue: Smoothed,
    pub cue_line: LineId,
    /// Volume × pan, per side.
    pub post: [Smoothed; 2],
    pub out: Out,
    pub out_line: LineId,
    /// The node's sends: `sends[first..first + count]`.
    pub first: u32,
    pub count: u32,
    /// Its devices: `devices[first_device..first_device + devices]`.
    pub first_device: u32,
    pub devices: u32,
    /// The sidechains it feeds: `taps[first_tap..first_tap + taps]`.
    pub first_tap: u32,
    pub taps: u32,
    /// Its sidechain bus, or [`NO_SIDE`].
    pub side: u32,
    /// Something of it passes through a delay line, which must keep
    /// running (and draining) while the node is silent.
    pub lined: bool,
    /// Its slot in the plan's meter bank ([`NO_METER`] when unmetered).
    pub meter: u32,
}

/// The slot of a node that is not metered.
pub(super) const NO_METER: u32 = u32::MAX;

/// A compiled routing: nodes in processing order, each with a stereo bus.
pub struct Plan {
    pub(super) nodes: Vec<Node>,
    pub(super) sends: Vec<Send>,
    pub(super) devices: Vec<Dev>,
    pub(super) taps: Vec<TapOut>,
    /// Delay lines: compensation and latent devices.
    pub(super) lines: Lines,
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

/// What a plan writes its device outputs with.
#[derive(Clone, Copy)]
pub(super) enum Ramp<'a> {
    Full,
    /// Per frame: the plan is crossfading in or out.
    Fade(&'a [f32]),
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
    pub(super) fn carry(&mut self, old: &Plan) {
        if old.cue_channel.is_some() {
            self.cue_volume.carry(&old.cue_volume);
        }
        self.lines.carry(&old.lines);
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
        }
        for send in &mut self.sends {
            send.gain.settle();
        }
        self.cue_volume.settle();
    }

    /// Runs the nodes in order over the first `frames` of their buses,
    /// adding what reaches the device into `out`. With `advance` false (a
    /// tail rendered ahead of a jump), levels are read but do not move;
    /// delay lines do move, which only matters to a latent plan's stop
    /// (and playback clears them when it starts). With `tap` (the plan the
    /// listener hears, not one fading out), every node's output is metered
    /// and the measured node's loudness taken.
    pub(super) fn process(
        &mut self,
        out: &mut Output<'_>,
        rate: u32,
        ramp: Ramp<'_>,
        advance: bool,
        tap: Option<&mut LoudnessTap>,
    ) {
        let frames = out.frames();
        let mut gain = [0.0f32; CHUNK];
        let gain = &mut gain[..frames];
        let mut scratch = [0.0f32; 2 * CHUNK];
        let mut tap = tap.filter(|_| advance);
        let meters = if tap.is_some() {
            self.meters.as_deref()
        } else {
            None
        };
        let keep = keep(frames, rate);
        let silent = [0.0f32; CHUNK];
        let measured = self.measured.map_or(usize::MAX, |m| m as usize);
        // Before any node adds to another's bus, the buses hold only what
        // the tracks played.
        for (node, bus) in self.nodes.iter().zip(self.buses.as_chunks_mut::<BUS>().0) {
            if node.input_line != NO_LINE {
                let (left, right) = bus.split_at_mut(CHUNK);
                self.lines
                    .run(node.input_line, &mut left[..frames], &mut right[..frames]);
            }
        }
        for index in 0..self.nodes.len() {
            let node = &mut self.nodes[index];
            // Downstream nodes always come later, so the buses split here.
            let (head, tail) = self.buses.split_at_mut((index + 1) * BUS);
            let bus = &mut head[index * BUS..];
            let (left, right) = bus.split_at_mut(CHUNK);
            let (left, right) = (&mut left[..frames], &mut right[..frames]);
            // Where node `to`'s bus starts in `tail`.
            let target = |to: u32| (to as usize - index - 1) * BUS;

            // Cued: heard on the cue bus before mute and the fader.
            let lined_cue = node.cue_line != NO_LINE;
            if (lined_cue || !(node.cue.settled() && node.cue.value() == 0.0))
                && let Some(cue) = self.cue_bus.as_chunks_mut::<BUS>().0.first_mut()
            {
                node.cue.fill(gain, rate, advance);
                let (cue_left, cue_right) = cue.split_at_mut(CHUNK);
                if lined_cue {
                    let (a, b) = scratch.split_at_mut(CHUNK);
                    let (a, b) = (&mut a[..frames], &mut b[..frames]);
                    a.copy_from_slice(left);
                    b.copy_from_slice(right);
                    scale(a, gain);
                    scale(b, gain);
                    self.lines.run(node.cue_line, a, b);
                    add(&mut cue_left[..frames], a);
                    add(&mut cue_right[..frames], b);
                } else {
                    add_scaled(&mut cue_left[..frames], left, gain);
                    add_scaled(&mut cue_right[..frames], right, gain);
                }
            }
            if !node.lined && node.pre.settled() && node.pre.value() == 0.0 {
                // Muted and settled: nothing leaves this node.
                if let Some(meters) = meters {
                    meters.silence(node.meter, keep);
                }
                if index == measured
                    && let Some(tap) = tap.as_deref_mut()
                {
                    tap.record(&silent[..frames], &silent[..frames], rate);
                }
                continue;
            }
            node.pre.fill(gain, rate, advance);
            scale(left, gain);
            scale(right, gain);

            let taps = &self.taps[node.first_tap as usize..(node.first_tap + node.taps) as usize];
            if node.split {
                let (lines, sides) = (&mut self.lines, &mut self.sides);
                send_taps(taps, Tap::PreFx, lines, sides, left, right, &mut scratch);
                let devices =
                    node.first_device as usize..(node.first_device + node.devices) as usize;
                for device in &self.devices[devices] {
                    if device.listen && node.side != NO_SIDE {
                        let side = &self.sides[node.side as usize * BUS..][..BUS];
                        add(left, &side[..frames]);
                        add(right, &side[CHUNK..CHUNK + frames]);
                    }
                    self.lines.run(device.line, left, right);
                }
                let (lines, sides) = (&mut self.lines, &mut self.sides);
                send_taps(taps, Tap::PostFx, lines, sides, left, right, &mut scratch);
                // Mute, after the devices.
                if !(node.heard.settled() && node.heard.value() == 1.0) {
                    node.heard.fill(gain, rate, advance);
                    scale(left, gain);
                    scale(right, gain);
                }
            }

            let sends = node.first as usize..(node.first + node.count) as usize;
            for send in self.sends[sends.clone()].iter_mut().filter(|s| s.pre_fader) {
                let at = target(send.to);
                send.gain.fill(gain, rate, advance);
                let (l, r) = self.lines.through(send.line, left, right, &mut scratch);
                add_scaled(&mut tail[at..at + frames], l, gain);
                add_scaled(&mut tail[at + CHUNK..at + CHUNK + frames], r, gain);
            }

            node.post[0].fill(gain, rate, advance);
            scale(left, gain);
            node.post[1].fill(gain, rate, advance);
            scale(right, gain);

            for send in self.sends[sends].iter_mut().filter(|s| !s.pre_fader) {
                let at = target(send.to);
                send.gain.fill(gain, rate, advance);
                let (l, r) = self.lines.through(send.line, left, right, &mut scratch);
                add_scaled(&mut tail[at..at + frames], l, gain);
                add_scaled(&mut tail[at + CHUNK..at + CHUNK + frames], r, gain);
            }

            if !(node.gate.settled() && node.gate.value() == 1.0) {
                if node.gate.settled() && !node.lined {
                    // Only the sends are heard.
                    if let Some(meters) = meters {
                        meters.silence(node.meter, keep);
                    }
                    if index == measured
                        && let Some(tap) = tap.as_deref_mut()
                    {
                        tap.record(&silent[..frames], &silent[..frames], rate);
                    }
                    continue;
                }
                node.gate.fill(gain, rate, advance);
                scale(left, gain);
                scale(right, gain);
            }
            if node.split {
                let (lines, sides) = (&mut self.lines, &mut self.sides);
                send_taps(
                    taps,
                    Tap::PostMixer,
                    lines,
                    sides,
                    left,
                    right,
                    &mut scratch,
                );
            }

            // What the node plays on: after fader, pan and the gate.
            if let Some(meters) = meters {
                meters.record(node.meter, [left, right], keep);
            }
            if index == measured
                && let Some(tap) = tap.as_deref_mut()
            {
                tap.record(left, right, rate);
            }

            match node.out {
                Out::Node(to) => {
                    let at = target(to);
                    let (l, r) = self.lines.through(node.out_line, left, right, &mut scratch);
                    add(&mut tail[at..at + frames], l);
                    add(&mut tail[at + CHUNK..at + CHUNK + frames], r);
                }
                Out::Hardware(pair) => {
                    let (l, r) = self.lines.through(node.out_line, left, right, &mut scratch);
                    let first = usize::from(pair) * 2;
                    for (side, samples) in [(0, l), (1, r)] {
                        if first + side >= out.channels() {
                            break;
                        }
                        let device = out.channel(first + side);
                        match ramp {
                            Ramp::Full => add(device, samples),
                            Ramp::Fade(fade) => add_scaled(device, samples, fade),
                        }
                    }
                }
                Out::None => {}
            }
        }
        self.cue_out(out, rate, ramp, advance);
    }

    /// Writes the cue bus to its pair of device channels at the cue level.
    fn cue_out(&mut self, out: &mut Output<'_>, rate: u32, ramp: Ramp<'_>, advance: bool) {
        let Some(first) = self.cue_channel.map(usize::from) else {
            return;
        };
        let Some(cue) = self.cue_bus.as_chunks_mut::<BUS>().0.first_mut() else {
            return;
        };
        let frames = out.frames();
        let mut gain = [0.0f32; CHUNK];
        let gain = &mut gain[..frames];
        self.cue_volume.fill(gain, rate, advance);
        let (left, right) = cue.split_at_mut(CHUNK);
        for (side, samples) in [(0, &mut left[..frames]), (1, &mut right[..frames])] {
            if first + side >= out.channels() {
                break;
            }
            scale(samples, gain);
            let device = out.channel(first + side);
            match ramp {
                Ramp::Full => add(device, samples),
                Ramp::Fade(fade) => add_scaled(device, samples, fade),
            }
        }
    }
}

/// Adds `left` and `right`, through their lines, to the sidechain buses
/// of the taps taken at `which`.
fn send_taps(
    taps: &[TapOut],
    which: Tap,
    lines: &mut Lines,
    sides: &mut [f32],
    left: &[f32],
    right: &[f32],
    scratch: &mut [f32; 2 * CHUNK],
) {
    for tap in taps.iter().filter(|t| t.tap == which) {
        let (l, r) = lines.through(tap.line, left, right, scratch);
        let side = &mut sides[tap.to as usize * BUS..][..BUS];
        add(&mut side[..l.len()], l);
        add(&mut side[CHUNK..CHUNK + r.len()], r);
    }
}

#[inline]
fn scale(samples: &mut [f32], gain: &[f32]) {
    for (s, g) in samples.iter_mut().zip(gain) {
        *s *= g;
    }
}

#[inline]
fn add(target: &mut [f32], samples: &[f32]) {
    for (t, s) in target.iter_mut().zip(samples) {
        *t += s;
    }
}

#[inline]
fn add_scaled(target: &mut [f32], samples: &[f32], gain: &[f32]) {
    for ((t, s), g) in target.iter_mut().zip(samples).zip(gain) {
        *t += s * g;
    }
}

/// The track buses of the plan in use, for a [`TrackSource`] to play
/// into.
///
/// [`TrackSource`]: super::TrackSource
pub struct Buses<'a> {
    by_id: &'a [(u64, u32)],
    data: &'a mut [f32],
    frames: usize,
}

impl Buses<'_> {
    /// Frames in this part.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The bus of track `id`; `None` when the routing has no such track
    /// (its audio is not heard).
    pub fn track(&mut self, id: u64) -> Option<Bus<'_>> {
        let index = self
            .by_id
            .binary_search_by_key(&id, |&(id, _)| id)
            .ok()
            .map(|i| self.by_id[i].1 as usize)?;
        Some(Bus {
            data: &mut self.data[index * BUS..(index + 1) * BUS],
            frames: self.frames,
        })
    }
}

/// A track's stereo bus for this part. Sources add to it.
pub struct Bus<'a> {
    data: &'a mut [f32],
    frames: usize,
}

impl Bus<'_> {
    /// Side 0 (left) or 1 (right): `frames` samples, zeroed at the start
    /// of the part.
    pub fn channel(&mut self, side: usize) -> &mut [f32] {
        let start = side.min(1) * CHUNK;
        &mut self.data[start..start + self.frames]
    }
}
