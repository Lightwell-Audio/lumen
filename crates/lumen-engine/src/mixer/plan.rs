//! The routing a project describes ([`Routing`]) and the flat processing
//! plan the audio thread runs ([`Plan`]).
//!
//! A [`Routing`] lists nodes (tracks, groups, returns, the master) with
//! their levels, sends and output. [`compile`] checks it, orders the nodes
//! so that every node comes after everything that feeds it, and allocates
//! every buffer the plan needs. It runs on the UI side; on the audio
//! thread a plan only reads and writes what it already holds.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use lumen_core::pan::PanLaw;

use super::smooth::Smoothed;
use crate::source::Output;

/// Frames a plan processes at a time; longer blocks are split.
pub const CHUNK: usize = 256;
/// Channels on every bus: stereo.
pub const BUS_CHANNELS: usize = 2;
const BUS: usize = CHUNK * BUS_CHANNELS;

/// Where a node's output goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Destination {
    /// Another node's input (a group, a return, the master).
    Node(u64),
    /// A pair of device outputs: pair 0 is channels 1 and 2. A pair the
    /// device does not have is silent.
    Hardware(u16),
    /// Nowhere.
    None,
}

/// A send from a node into another node (a return).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SendSpec {
    pub to: u64,
    /// Linear gain.
    pub gain: f32,
    /// Taken before the node's volume and pan; otherwise after them.
    pub pre_fader: bool,
}

/// One node of the routing: a track, a group, a return or the master.
///
/// The signal path: the node's input (its clips, the outputs of the nodes
/// routed into it and the sends into it) → trim → polarity → mute →
/// pre-fader sends → volume → pan → post-fader sends → output. Mute
/// silences sends too, as in Ableton.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeSpec {
    /// The track's id; tracks address their audio to it.
    pub id: u64,
    /// Linear input gain.
    pub trim: f32,
    pub invert: bool,
    /// Linear gain.
    pub volume: f32,
    /// −1 (left) to +1 (right).
    pub pan: f32,
    pub law: PanLaw,
    /// False when the node is muted, or silenced by another track's solo.
    pub audible: bool,
    pub output: Destination,
    pub sends: Vec<SendSpec>,
}

impl NodeSpec {
    /// A node at unity, centred with the balance law (unity at the
    /// centre), heard, and routed to `output`.
    pub fn unity(id: u64, output: Destination) -> Self {
        Self {
            id,
            trim: 1.0,
            invert: false,
            volume: 1.0,
            pan: 0.0,
            law: PanLaw::Balance,
            audible: true,
            output,
            sends: Vec::new(),
        }
    }
}

/// The whole routing, in any order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Routing {
    pub nodes: Vec<NodeSpec>,
}

/// Why a routing cannot be played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingError {
    /// Two nodes share an id.
    Duplicate(u64),
    /// An output or send names a node that is not there.
    Unknown { from: u64, to: u64 },
    /// The nodes feed each other in a loop (through outputs or sends).
    Cycle,
}

impl std::fmt::Display for RoutingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate(id) => write!(f, "two nodes have the id {id}"),
            Self::Unknown { from, to } => {
                write!(f, "node {from} feeds node {to}, which is missing")
            }
            Self::Cycle => write!(f, "the routing feeds back into itself"),
        }
    }
}

impl std::error::Error for RoutingError {}

/// Where a compiled node's output goes: an index into the plan's nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Out {
    Node(u32),
    Hardware(u16),
    None,
}

#[derive(Debug, Clone, Copy)]
struct Send {
    /// Index into the plan's nodes, always after the sending node.
    to: u32,
    to_id: u64,
    pre_fader: bool,
    gain: Smoothed,
}

#[derive(Debug, Clone)]
struct Node {
    id: u64,
    /// Trim × polarity × mute.
    pre: Smoothed,
    /// Volume × pan, per side.
    post: [Smoothed; 2],
    out: Out,
    /// The node's sends: `sends[first..first + count]`.
    first: u32,
    count: u32,
}

/// A compiled routing: nodes in processing order, each with a stereo bus.
pub struct Plan {
    nodes: Vec<Node>,
    sends: Vec<Send>,
    /// `(id, index)` sorted by id: where tracks find their bus.
    by_id: Vec<(u64, u32)>,
    /// Every node's bus, `BUS` samples each: left then right, `CHUNK`
    /// frames per side.
    buses: Vec<f32>,
    /// Identifies the routing's shape (order, outputs and sends, not
    /// levels): a plan with another shape crossfades in.
    pub(super) topology: u64,
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("nodes", &self.nodes.len())
            .field("topology", &self.topology)
            .finish()
    }
}

/// Checks `routing` and builds its plan. Allocates; call it off the audio
/// thread.
pub fn compile(routing: &Routing) -> Result<Plan, RoutingError> {
    let specs = &routing.nodes;
    let mut by_id: Vec<(u64, u32)> = specs
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i as u32))
        .collect();
    by_id.sort_unstable();
    if let Some(w) = by_id.windows(2).find(|w| w[0].0 == w[1].0) {
        return Err(RoutingError::Duplicate(w[0].0));
    }
    let index_of = |from: u64, to: u64| -> Result<usize, RoutingError> {
        by_id
            .binary_search_by_key(&to, |&(id, _)| id)
            .map(|i| by_id[i].1 as usize)
            .map_err(|_| RoutingError::Unknown { from, to })
    };

    // Edges from each node to the nodes it feeds.
    let mut feeds: Vec<Vec<usize>> = vec![Vec::new(); specs.len()];
    let mut incoming = vec![0usize; specs.len()];
    for (i, spec) in specs.iter().enumerate() {
        let outputs = match spec.output {
            Destination::Node(to) => Some(to),
            _ => None,
        };
        for to in outputs.into_iter().chain(spec.sends.iter().map(|s| s.to)) {
            let j = index_of(spec.id, to)?;
            if j == i {
                return Err(RoutingError::Cycle);
            }
            feeds[i].push(j);
            incoming[j] += 1;
        }
    }

    // Kahn's order; among ready nodes, the earliest listed goes first, so
    // the same routing always compiles to the same plan.
    let mut ready: BTreeSet<usize> = (0..specs.len()).filter(|&i| incoming[i] == 0).collect();
    let mut order = Vec::with_capacity(specs.len());
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for &j in &feeds[i] {
            incoming[j] -= 1;
            if incoming[j] == 0 {
                ready.insert(j);
            }
        }
    }
    if order.len() < specs.len() {
        return Err(RoutingError::Cycle);
    }

    let mut position = vec![0u32; specs.len()];
    for (at, &i) in order.iter().enumerate() {
        position[i] = at as u32;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut nodes = Vec::with_capacity(specs.len());
    let mut sends = Vec::new();
    for &i in &order {
        let spec = &specs[i];
        let out = match spec.output {
            Destination::Node(to) => Out::Node(position[index_of(spec.id, to)?]),
            Destination::Hardware(pair) => Out::Hardware(pair),
            Destination::None => Out::None,
        };
        (spec.id, spec.output).hash(&mut hasher);
        let first = sends.len() as u32;
        for send in &spec.sends {
            (send.to, send.pre_fader).hash(&mut hasher);
            sends.push(Send {
                to: position[index_of(spec.id, send.to)?],
                to_id: send.to,
                pre_fader: send.pre_fader,
                gain: Smoothed::new(finite(send.gain)),
            });
        }
        let sign = if spec.invert { -1.0 } else { 1.0 };
        let pre = if spec.audible {
            finite(spec.trim) * sign
        } else {
            0.0
        };
        let volume = finite(spec.volume);
        let [left, right] = spec.law.gains(spec.pan);
        nodes.push(Node {
            id: spec.id,
            pre: Smoothed::new(pre),
            post: [Smoothed::new(volume * left), Smoothed::new(volume * right)],
            out,
            first,
            count: spec.sends.len() as u32,
        });
    }
    let mut by_id: Vec<(u64, u32)> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i as u32))
        .collect();
    by_id.sort_unstable();
    Ok(Plan {
        buses: vec![0.0; nodes.len() * BUS],
        nodes,
        sends,
        by_id,
        topology: hasher.finish(),
    })
}

/// A gain the audio thread can use: NaN and infinities become silence.
fn finite(gain: f32) -> f32 {
    if gain.is_finite() { gain } else { 0.0 }
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
            by_id: Vec::new(),
            buses: Vec::new(),
            topology: 0,
        }
    }

    /// Zeroes the first `frames` of every bus.
    pub(super) fn clear(&mut self, frames: usize) {
        for bus in self.buses.as_chunks_mut::<BUS>().0 {
            bus[..frames].fill(0.0);
            bus[CHUNK..CHUNK + frames].fill(0.0);
        }
    }

    pub(super) fn buses(&mut self, frames: usize) -> Buses<'_> {
        Buses {
            by_id: &self.by_id,
            data: &mut self.buses,
            frames,
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
    /// also has, so they glide to their new values. Real-time safe: one
    /// merge over two sorted lists, and a short search per send.
    pub(super) fn carry(&mut self, old: &Plan) {
        let (mut i, mut j) = (0, 0);
        while let (Some(&(a, here)), Some(&(b, there))) = (self.by_id.get(i), old.by_id.get(j)) {
            match a.cmp(&b) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    let before = &old.nodes[there as usize];
                    let node = &mut self.nodes[here as usize];
                    node.pre.carry(&before.pre);
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

    /// Every level jumps to its target (after a transport jump).
    pub(super) fn settle(&mut self) {
        for node in &mut self.nodes {
            node.pre.settle();
            node.post.iter_mut().for_each(Smoothed::settle);
        }
        for send in &mut self.sends {
            send.gain.settle();
        }
    }

    /// Runs the nodes in order over the first `frames` of their buses,
    /// adding what reaches the device into `out`. With `advance` false (a
    /// tail rendered ahead of a jump), levels are read but do not move.
    pub(super) fn process(
        &mut self,
        out: &mut Output<'_>,
        rate: u32,
        ramp: Ramp<'_>,
        advance: bool,
    ) {
        let frames = out.frames();
        let mut gain = [0.0f32; CHUNK];
        let gain = &mut gain[..frames];
        for index in 0..self.nodes.len() {
            let node = &mut self.nodes[index];
            // Downstream nodes always come later, so the buses split here.
            let (head, tail) = self.buses.split_at_mut((index + 1) * BUS);
            let bus = &mut head[index * BUS..];
            let (left, right) = bus.split_at_mut(CHUNK);
            let (left, right) = (&mut left[..frames], &mut right[..frames]);
            // Where node `to`'s bus starts in `tail`.
            let target = |to: u32| (to as usize - index - 1) * BUS;

            if node.pre.settled() && node.pre.value() == 0.0 {
                // Muted and settled: nothing leaves this node.
                continue;
            }
            node.pre.fill(gain, rate, advance);
            scale(left, gain);
            scale(right, gain);

            let sends = node.first as usize..(node.first + node.count) as usize;
            for send in self.sends[sends.clone()].iter_mut().filter(|s| s.pre_fader) {
                let at = target(send.to);
                send.gain.fill(gain, rate, advance);
                add_scaled(&mut tail[at..at + frames], left, gain);
                add_scaled(&mut tail[at + CHUNK..at + CHUNK + frames], right, gain);
            }

            node.post[0].fill(gain, rate, advance);
            scale(left, gain);
            node.post[1].fill(gain, rate, advance);
            scale(right, gain);

            for send in self.sends[sends].iter_mut().filter(|s| !s.pre_fader) {
                let at = target(send.to);
                send.gain.fill(gain, rate, advance);
                add_scaled(&mut tail[at..at + frames], left, gain);
                add_scaled(&mut tail[at + CHUNK..at + CHUNK + frames], right, gain);
            }

            match node.out {
                Out::Node(to) => {
                    let at = target(to);
                    add(&mut tail[at..at + frames], left);
                    add(&mut tail[at + CHUNK..at + CHUNK + frames], right);
                }
                Out::Hardware(pair) => {
                    let first = usize::from(pair) * 2;
                    for (side, samples) in [(0, &*left), (1, &*right)] {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, output: Destination) -> NodeSpec {
        NodeSpec::unity(id, output)
    }

    fn order(plan: &Plan) -> Vec<u64> {
        plan.nodes.iter().map(|n| n.id).collect()
    }

    #[test]
    fn nodes_come_after_everything_that_feeds_them() {
        // The master listed first, a group before its children, a return
        // fed by a send from a track listed after it.
        let mut track = node(4, Destination::Node(2));
        track.sends.push(SendSpec {
            to: 3,
            gain: 1.0,
            pre_fader: false,
        });
        let routing = Routing {
            nodes: vec![
                node(1, Destination::Hardware(0)),
                node(2, Destination::Node(1)),
                node(3, Destination::Node(1)),
                track,
                node(5, Destination::Node(2)),
            ],
        };
        let plan = compile(&routing).unwrap();
        assert_eq!(order(&plan), [4, 3, 5, 2, 1]);
    }

    #[test]
    fn the_same_routing_compiles_the_same_way() {
        let routing = Routing {
            nodes: (0..20)
                .map(|i| node(i, Destination::Node(100)))
                .chain([node(100, Destination::Hardware(0))])
                .collect(),
        };
        let a = compile(&routing).unwrap();
        let b = compile(&routing).unwrap();
        assert_eq!(order(&a), order(&b));
        assert_eq!(a.topology, b.topology);
        // Levels do not change the shape; outputs do.
        let mut louder = routing.clone();
        louder.nodes[3].volume = 0.5;
        assert_eq!(compile(&louder).unwrap().topology, a.topology);
        let mut elsewhere = routing.clone();
        elsewhere.nodes[3].output = Destination::Hardware(1);
        assert_ne!(compile(&elsewhere).unwrap().topology, a.topology);
    }

    #[test]
    fn loops_unknown_targets_and_duplicates_are_refused() {
        let cycle = Routing {
            nodes: vec![node(1, Destination::Node(2)), node(2, Destination::Node(1))],
        };
        assert_eq!(compile(&cycle).unwrap_err(), RoutingError::Cycle);
        let mut feedback = node(1, Destination::Hardware(0));
        feedback.sends.push(SendSpec {
            to: 1,
            gain: 1.0,
            pre_fader: true,
        });
        let routing = Routing {
            nodes: vec![feedback],
        };
        assert_eq!(compile(&routing).unwrap_err(), RoutingError::Cycle);
        // Through a send: a return that feeds the track sending to it.
        let mut track = node(1, Destination::Hardware(0));
        track.sends.push(SendSpec {
            to: 2,
            gain: 1.0,
            pre_fader: false,
        });
        let routing = Routing {
            nodes: vec![track, node(2, Destination::Node(1))],
        };
        assert_eq!(compile(&routing).unwrap_err(), RoutingError::Cycle);
        let unknown = Routing {
            nodes: vec![node(1, Destination::Node(9))],
        };
        assert_eq!(
            compile(&unknown).unwrap_err(),
            RoutingError::Unknown { from: 1, to: 9 }
        );
        let twice = Routing {
            nodes: vec![
                node(1, Destination::Hardware(0)),
                node(1, Destination::Hardware(0)),
            ],
        };
        assert_eq!(compile(&twice).unwrap_err(), RoutingError::Duplicate(1));
    }

    #[test]
    fn bad_gains_become_silence() {
        let mut spec = node(1, Destination::Hardware(0));
        spec.volume = f32::NAN;
        spec.trim = f32::INFINITY;
        let plan = compile(&Routing { nodes: vec![spec] }).unwrap();
        assert_eq!(plan.nodes[0].pre.target(), 0.0);
        assert_eq!(plan.nodes[0].post[0].target(), 0.0);
    }
}
