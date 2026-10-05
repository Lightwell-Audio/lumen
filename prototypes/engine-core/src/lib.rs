//! P-ENGINE prototype: the smallest real-time core that tests ADR-0003.
//!
//! What it demonstrates:
//! - The audio side ([`Engine`]) never allocates or frees. Graphs are built on
//!   a non-real-time thread, handed over through a lock-free queue, and
//!   handed back through another queue to be freed off the audio thread.
//! - Live commands travel through a preallocated single-producer ring.
//! - Timeline events (gain automation) are applied at exact sample positions,
//!   and all DSP runs sample by sample, so output is bit-identical whatever
//!   the block size. Offline rendering is therefore the same code path.
//! - Swapping in a new graph carries DSP state across by track id, so an
//!   unchanged graph swaps in inaudibly.
//!
//! Prototype quality: no parallelism, plugins, transport or real devices.

use std::sync::Arc;

/// Largest block processed in one pass; bigger requests are chunked.
pub const MAX_BLOCK: usize = 4096;

const QUEUE_CAPACITY: usize = 256;
const GRAPH_QUEUE_CAPACITY: usize = 4;

/// Live control messages from the UI side. They take effect at the start of
/// the next block, so they are not part of deterministic rendering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    SetTrackGain { track_id: u32, gain: f32 },
    Seek { position: u64 },
}

/// Description of one track, used to build a [`Graph`] off the audio thread.
#[derive(Debug, Clone)]
pub struct TrackDesc {
    pub id: u32,
    /// Offset into the shared source signal, emulating different clips.
    pub source_offset: usize,
    pub cutoff_hz: f32,
    /// -1 (left) ..= 1 (right), constant-power law.
    pub pan: f32,
    /// Timeline gain changes: (absolute sample position, target gain).
    pub gain_automation: Vec<(u64, f32)>,
}

/// An immutable-shape processing graph. Built and freed off the audio thread.
#[derive(Debug)]
pub struct Graph {
    source: Arc<[f32]>,
    tracks: Vec<TrackNode>,
}

#[derive(Debug)]
struct TrackNode {
    id: u32,
    source_offset: usize,
    filter: Biquad,
    pan_left: f32,
    pan_right: f32,
    gain: Smoothed,
    automation: Vec<(u64, f32)>,
    automation_cursor: usize,
}

impl Graph {
    /// Builds a graph. Allocates, so call it off the audio thread.
    ///
    /// # Panics
    /// If `source` is empty.
    pub fn new(sample_rate: f32, source: Arc<[f32]>, tracks: &[TrackDesc]) -> Self {
        assert!(!source.is_empty(), "source signal must not be empty");
        let mut nodes: Vec<TrackNode> = tracks
            .iter()
            .map(|desc| {
                let angle = (desc.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
                let mut automation = desc.gain_automation.clone();
                automation.sort_by_key(|&(at, _)| at);
                TrackNode {
                    id: desc.id,
                    source_offset: desc.source_offset,
                    filter: Biquad::lowpass(sample_rate, desc.cutoff_hz),
                    pan_left: angle.cos(),
                    pan_right: angle.sin(),
                    gain: Smoothed::new(1.0, sample_rate),
                    automation,
                    automation_cursor: 0,
                }
            })
            .collect();
        nodes.sort_by_key(|node| node.id);
        Self {
            source,
            tracks: nodes,
        }
    }

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Copies DSP state from `old` for tracks with the same id. Real-time safe.
    fn adopt_state_from(&mut self, old: &Graph, position: u64) {
        for node in &mut self.tracks {
            if let Ok(index) = old.tracks.binary_search_by_key(&node.id, |old| old.id) {
                let previous = &old.tracks[index];
                node.filter.state = previous.filter.state;
                node.gain = previous.gain;
            }
            node.automation_cursor = node.automation.partition_point(|&(at, _)| at < position);
        }
    }

    fn set_position(&mut self, position: u64) {
        for node in &mut self.tracks {
            node.automation_cursor = node.automation.partition_point(|&(at, _)| at < position);
        }
    }
}

/// UI-side handle: sends commands and graphs, frees retired graphs.
#[derive(Debug)]
pub struct EngineHandle {
    commands: rtrb::Producer<Command>,
    graphs: rtrb::Producer<Box<Graph>>,
    retired: rtrb::Consumer<Box<Graph>>,
}

impl EngineHandle {
    /// Queues a live command. Returns it back if the queue is full.
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        self.commands
            .push(command)
            .map_err(|rtrb::PushError::Full(c)| c)
    }

    /// Queues a graph to replace the current one. Returns it back if full.
    pub fn replace_graph(&mut self, graph: Graph) -> Result<(), Box<Graph>> {
        self.graphs
            .push(Box::new(graph))
            .map_err(|rtrb::PushError::Full(g)| g)
    }

    /// Frees graphs the audio side has retired. Call regularly off the audio thread.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while let Ok(graph) = self.retired.pop() {
            drop(graph);
            freed += 1;
        }
        freed
    }
}

/// Audio-side engine. Every method is real-time safe.
#[derive(Debug)]
pub struct Engine {
    commands: rtrb::Consumer<Command>,
    graphs: rtrb::Consumer<Box<Graph>>,
    retired: rtrb::Producer<Box<Graph>>,
    graph: Box<Graph>,
    /// A retired graph that did not fit in the queue; retried next block.
    stash: Option<Box<Graph>>,
    position: u64,
}

/// Creates the engine pair. Allocates, so call it off the audio thread.
pub fn engine(initial: Graph) -> (EngineHandle, Engine) {
    let (command_tx, command_rx) = rtrb::RingBuffer::new(QUEUE_CAPACITY);
    let (graph_tx, graph_rx) = rtrb::RingBuffer::new(GRAPH_QUEUE_CAPACITY);
    // One more slot than graphs in flight, so retiring never has to wait.
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(GRAPH_QUEUE_CAPACITY + 1);
    let handle = EngineHandle {
        commands: command_tx,
        graphs: graph_tx,
        retired: retired_rx,
    };
    let engine = Engine {
        commands: command_rx,
        graphs: graph_rx,
        retired: retired_tx,
        graph: Box::new(initial),
        stash: None,
        position: 0,
    };
    (handle, engine)
}

impl Engine {
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Renders one block of stereo audio. `left` and `right` must have equal
    /// lengths; any length is accepted.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        assert_eq!(left.len(), right.len(), "channel lengths differ");
        self.retry_stash();
        self.swap_in_new_graph();
        self.apply_commands();
        for (l, r) in left.chunks_mut(MAX_BLOCK).zip(right.chunks_mut(MAX_BLOCK)) {
            self.render(l, r);
        }
    }

    fn retry_stash(&mut self) {
        if let Some(graph) = self.stash.take()
            && let Err(rtrb::PushError::Full(graph)) = self.retired.push(graph)
        {
            self.stash = Some(graph);
        }
    }

    fn swap_in_new_graph(&mut self) {
        // Only one swap per block, and only when the retired graph has a home.
        if self.stash.is_some() {
            return;
        }
        if let Ok(mut incoming) = self.graphs.pop() {
            incoming.adopt_state_from(&self.graph, self.position);
            let old = std::mem::replace(&mut self.graph, incoming);
            if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
                self.stash = Some(old);
            }
        }
    }

    fn apply_commands(&mut self) {
        // Bounded so a flood of commands cannot blow the block's time budget.
        for _ in 0..QUEUE_CAPACITY {
            let Ok(command) = self.commands.pop() else {
                break;
            };
            match command {
                Command::SetTrackGain { track_id, gain } => {
                    if let Ok(i) = self.graph.tracks.binary_search_by_key(&track_id, |t| t.id) {
                        self.graph.tracks[i].gain.target = gain;
                    }
                }
                Command::Seek { position } => {
                    self.position = position;
                    self.graph.set_position(position);
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        let start = self.position;
        let frames = left.len();
        let source = &*self.graph.source;
        for track in &mut self.graph.tracks {
            let mut offset = 0;
            while offset < frames {
                let now = start + offset as u64;
                // Apply every automation point that is due at this sample.
                while let Some(&(at, gain)) = track.automation.get(track.automation_cursor) {
                    if at > now {
                        break;
                    }
                    track.gain.target = gain;
                    track.automation_cursor += 1;
                }
                let segment_end = match track.automation.get(track.automation_cursor) {
                    Some(&(at, _)) => frames.min((at - start) as usize),
                    None => frames,
                };
                track.render(
                    source,
                    now,
                    &mut left[offset..segment_end],
                    &mut right[offset..segment_end],
                );
                offset = segment_end;
            }
        }
        self.position += frames as u64;
    }
}

impl TrackNode {
    fn render(&mut self, source: &[f32], position: u64, left: &mut [f32], right: &mut [f32]) {
        let len = source.len();
        let mut read = (self.source_offset + (position % len as u64) as usize) % len;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let x = self.filter.tick(source[read]) * self.gain.next();
            *l += x * self.pan_left;
            *r += x * self.pan_right;
            read += 1;
            if read == len {
                read = 0;
            }
        }
    }
}

/// One-pole parameter smoother, advanced per sample.
#[derive(Debug, Clone, Copy)]
struct Smoothed {
    value: f32,
    target: f32,
    coefficient: f32,
}

impl Smoothed {
    fn new(value: f32, sample_rate: f32) -> Self {
        // About 5 ms time constant.
        let coefficient = 1.0 - (-1.0 / (0.005 * sample_rate)).exp();
        Self {
            value,
            target: value,
            coefficient,
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.coefficient;
        self.value
    }
}

/// RBJ low-pass biquad, transposed direct form II.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    state: [f32; 2],
}

impl Biquad {
    fn lowpass(sample_rate: f32, cutoff_hz: f32) -> Self {
        let cutoff = cutoff_hz.clamp(10.0, sample_rate * 0.45);
        let w0 = std::f32::consts::TAU * cutoff / sample_rate;
        let alpha = w0.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos_w0) / 2.0 / a0,
            b1: (1.0 - cos_w0) / a0,
            b2: (1.0 - cos_w0) / 2.0 / a0,
            a1: -2.0 * cos_w0 / a0,
            a2: (1.0 - alpha) / a0,
            state: [0.0; 2],
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.state[0];
        self.state[0] = self.b1 * x - self.a1 * y + self.state[1];
        self.state[1] = self.b2 * x - self.a2 * y;
        y
    }
}

/// Deterministic pseudo-random test signal, the same on every machine.
pub fn test_source(len: usize) -> Arc<[f32]> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 40) as f32 / (1_u64 << 24) as f32) * 0.5 - 0.25
        })
        .collect()
}

/// A synthetic session: `count` tracks with varied filters, pans and a
/// gain automation point every `automation_every` samples.
pub fn test_tracks(count: u32, automation_every: u64, automation_points: u64) -> Vec<TrackDesc> {
    (0..count)
        .map(|id| TrackDesc {
            id,
            source_offset: id as usize * 7919,
            cutoff_hz: 200.0 + id as f32 * 97.0,
            pan: (id as f32 / count.max(1) as f32) * 2.0 - 1.0,
            gain_automation: (1..=automation_points)
                // Irregular positions so events fall mid-block for every block size.
                .map(|k| {
                    (
                        k * automation_every + u64::from(id) * 13,
                        if k % 2 == 0 { 1.0 } else { 0.25 },
                    )
                })
                .collect(),
        })
        .collect()
}
