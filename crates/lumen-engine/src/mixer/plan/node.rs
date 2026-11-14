//! What a compiled plan holds per node, send, device and tap, and how a
//! part of a block is processed.

use super::super::auto::NodeStages;
use super::super::delay::LineId;
use super::super::smooth::Smoothed;
use super::super::spec::Tap;
use lumen_core::pan::PanLaw;

use std::sync::Arc;

/// Where a compiled node's output goes: an index into the plan's nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::mixer) enum Out {
    Node(u32),
    Hardware(u16),
    None,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::mixer) struct Send {
    /// Index into the plan's nodes, always after the sending node.
    pub(in crate::mixer) to: u32,
    pub(in crate::mixer) to_id: u64,
    pub(in crate::mixer) pre_fader: bool,
    pub(in crate::mixer) gain: Smoothed,
    pub(in crate::mixer) line: LineId,
}

/// A compiled device: the synthetic delay (its line), a plugin effect in a
/// host (its fallback line, and the glide to its output), or a built-in effect
/// (its fallback line and the values its parameters hold).
#[derive(Debug, Clone)]
pub(in crate::mixer) struct Dev {
    pub(in crate::mixer) line: LineId,
    /// Adds the node's sidechain input before delaying.
    pub(in crate::mixer) listen: bool,
    /// The host's id in the mixer's pool, or [`NO_HOST`].
    pub(in crate::mixer) host: u64,
    /// The device's id in the mixer's built-in pool, or [`NO_BUILTIN`].
    pub(in crate::mixer) builtin: u64,
    /// The built-in's power is off: it is not asked for blocks, and the node
    /// glides to the dry fallback (M8 slice B0, review M1).
    pub(in crate::mixer) bypass: bool,
    /// What the project holds for a built-in's parameters, normalised, by id,
    /// sorted: played at the block's first frame, under the lanes' points.
    pub(in crate::mixer) values: Arc<[(u32, f32)]>,
    /// What each of `values` was when the device was last told: only a value
    /// that moved opens a queue, so a still device costs no parameter work
    /// (M8 slice B0, review L6). Worked out in `compile`; NaN before the
    /// device is told anything.
    pub(in crate::mixer) values_last: Vec<f32>,
    /// 1 while the host's output is what the node hears, 0 for the fallback.
    pub(in crate::mixer) wet: Smoothed,
    /// The device's block was handed to its host ahead of the node loop.
    pub(in crate::mixer) began: bool,
    /// The early attempt was refused: the node's own attempt is the same
    /// missed block, not another.
    pub(in crate::mixer) refused: bool,
}

/// A node with no device to hand its host early.
pub(in crate::mixer) const NO_DEV: u32 = u32::MAX;

/// A sidechain leaving a node.
#[derive(Debug, Clone, Copy)]
pub(in crate::mixer) struct TapOut {
    /// The receiving node's sidechain bus.
    pub(in crate::mixer) to: u32,
    pub(in crate::mixer) tap: Tap,
    pub(in crate::mixer) line: LineId,
}

/// A node without a sidechain bus.
pub(in crate::mixer) const NO_SIDE: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub(in crate::mixer) struct Node {
    pub(in crate::mixer) id: u64,
    /// The pan law the node pans with: a built-in device that pans follows it
    /// (M8 slice B0).
    pub(in crate::mixer) law: PanLaw,
    /// Delays what the tracks play into the node, when it waits for a
    /// later input.
    pub(in crate::mixer) input_line: LineId,
    /// Trim × polarity, and when not `split`, × (output or sends heard).
    pub(in crate::mixer) pre: Smoothed,
    /// The node has devices or sidechains taken from it, so mute comes
    /// after them (`heard`) rather than in `pre`.
    pub(in crate::mixer) split: bool,
    /// 1 while the output or sends are heard; used when `split`.
    pub(in crate::mixer) heard: Smoothed,
    /// 1 while the output is heard, 0 while only the sends are.
    pub(in crate::mixer) gate: Smoothed,
    /// Trim × polarity while cued, else 0.
    pub(in crate::mixer) cue: Smoothed,
    pub(in crate::mixer) cue_line: LineId,
    /// Volume × pan, per side.
    pub(in crate::mixer) post: [Smoothed; 2],
    pub(in crate::mixer) out: Out,
    pub(in crate::mixer) out_line: LineId,
    /// The node's sends: `sends[first..first + count]`.
    pub(in crate::mixer) first: u32,
    pub(in crate::mixer) count: u32,
    /// Its devices: `devices[first_device..first_device + devices]`.
    pub(in crate::mixer) first_device: u32,
    pub(in crate::mixer) devices: u32,
    /// Nothing feeds the node and its first device is hosted: that device's
    /// index, whose host gets its block ahead of the node loop, else
    /// [`NO_DEV`].
    pub(in crate::mixer) early: u32,
    /// The sidechains it feeds: `taps[first_tap..first_tap + taps]`.
    pub(in crate::mixer) first_tap: u32,
    pub(in crate::mixer) taps: u32,
    /// Its sidechain bus, or [`NO_SIDE`].
    pub(in crate::mixer) side: u32,
    /// Something of it passes through a delay line, which must keep
    /// running (and draining) while the node is silent.
    pub(in crate::mixer) lined: bool,
    /// Its slot in the plan's meter bank ([`NO_METER`] when unmetered).
    pub(in crate::mixer) meter: u32,
    /// The slot of its sidechain bus's reading, else [`NO_METER`].
    pub(in crate::mixer) side_meter: u32,
    /// Its automation lanes playing (M7 slice A0), if it has any.
    pub(in crate::mixer) auto: Option<Box<NodeStages>>,
}

/// The slot of a node that is not metered.
pub(in crate::mixer) const NO_METER: u32 = u32::MAX;

/// What a plan writes its device outputs with.
#[derive(Clone, Copy)]
pub(in crate::mixer) enum Ramp<'a> {
    Full,
    /// Per frame: the plan is crossfading in or out.
    Fade(&'a [f32]),
}

/// How one part of a block is processed.
#[derive(Clone, Copy)]
pub(in crate::mixer) struct Pass<'a> {
    /// The timeline sample the tracks played the first of the part's frames
    /// at, which automation lanes are read against.
    pub(in crate::mixer) start: i64,
    pub(in crate::mixer) rate: u32,
    /// The song's tempo at the part's first frame, for a built-in whose time is
    /// in notes (M8 slice B3).
    pub(in crate::mixer) bpm: f32,
    pub(in crate::mixer) ramp: Ramp<'a>,
    /// False for a tail rendered ahead of a jump: levels are read, not moved.
    pub(in crate::mixer) advance: bool,
}
