//! The routing a project describes: nodes, their sends, sidechains and
//! devices, and where each one goes. [`super::compile`] turns it into a
//! [`super::Plan`].

use lumen_core::pan::PanLaw;

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

/// Where a sidechain is taken from on the node that feeds it: one of
/// three points (owner, ADR-0041).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Tap {
    /// After trim and polarity, before the node's devices. The fader and
    /// mute do not touch it, so a muted track can still drive a sidechain.
    PreFx,
    /// After the node's devices, before mute and the fader.
    #[default]
    PostFx,
    /// After mute, the fader and pan: what the node plays.
    PostMixer,
}

/// A sidechain into a node's second input, which its devices can listen
/// to (a compressor in M8, VST3 sidechain inputs in M6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SidechainSpec {
    pub from: u64,
    pub tap: Tap,
}

/// A device in a node's chain, after trim and polarity and before mute and
/// the fader. Real devices join in M6 (VST3) and M8 (built-in).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Device {
    /// The synthetic latent device (M4 slice 6): a pure delay of `frames`,
    /// which it reports as its latency. With `listen`, it adds the node's
    /// sidechain input to what it delays, as a compressor's listen button
    /// does, so tests can hear a sidechain line up.
    Delay { frames: u32, listen: bool },
    /// A plugin effect in a host process (M6 slice 2b), found in the mixer's
    /// host pool by the id `host`. `latency` is what the plugin last
    /// reported, in frames: delay compensation counts it, and it is the
    /// delay the device falls back to while its host is bypassed, failed or
    /// absent, so timing never shifts (the owner's D1 and D7). A change of
    /// it is a new routing.
    Hosted { host: u64, latency: u32 },
}

impl Device {
    /// Frames the device's output trails its input.
    pub fn latency(&self) -> u32 {
        match *self {
            Self::Delay { frames, .. } => frames.min(super::MAX_DEVICE_LATENCY),
            Self::Hosted { latency, .. } => latency.min(super::MAX_DEVICE_LATENCY),
        }
    }
}

/// One node of the routing: a track, a group, a return or the master.
///
/// The signal path: the node's input (its clips, the outputs of the nodes
/// routed into it and the sends into it) → trim → polarity → devices →
/// mute → pre-fader sends → volume → pan → post-fader sends → output.
/// Mute silences sends too; the output can be silent while
/// the sends play (a soloed return plays wet only). Sidechains taken from
/// the node leave at their [`Tap`].
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
    /// False when the node's output is muted, or silenced by another
    /// track's solo.
    pub audible: bool,
    /// False when its sends are silent too.
    pub sends_audible: bool,
    /// Cued (M4 slice 4): heard on the cue bus after trim and polarity,
    /// before mute and the fader, whatever mute and solo say.
    pub cue: bool,
    pub output: Destination,
    pub sends: Vec<SendSpec>,
    /// The node's devices, in order (M4 slice 6).
    pub devices: Vec<Device>,
    /// Frames the node's own source trails the timeline: a plugin instrument
    /// plays its notes this late (M6 slice 6). The source is not delayed,
    /// and the engine delays every other path to meet it.
    pub source: u32,
    /// Sidechains into the node's second input (M4 slice 6).
    pub sidechains: Vec<SidechainSpec>,
}

impl NodeSpec {
    /// A node at unity, centred with the balance law (unity at the
    /// centre), heard, with no devices, and routed to `output`.
    pub fn unity(id: u64, output: Destination) -> Self {
        Self {
            id,
            trim: 1.0,
            invert: false,
            volume: 1.0,
            pan: 0.0,
            law: PanLaw::Balance,
            audible: true,
            sends_audible: true,
            cue: false,
            output,
            sends: Vec::new(),
            devices: Vec::new(),
            source: 0,
            sidechains: Vec::new(),
        }
    }

    /// Frames the node's devices add.
    pub fn latency(&self) -> u32 {
        self.devices
            .iter()
            .map(Device::latency)
            .fold(0u32, u32::saturating_add)
    }
}

/// The whole routing, in any order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Routing {
    pub nodes: Vec<NodeSpec>,
    /// Where cued nodes are heard, if anywhere.
    pub cue: Option<CueOut>,
    /// The node whose loudness and true peak are measured (the master),
    /// if any (M4 slice 5).
    pub measure: Option<u64>,
}

/// The cue bus's output (M4 slice 4): a pair of device channels and a
/// level. Cued nodes add to it before their fader, for listening on
/// headphones while the main output plays on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueOut {
    /// The pair's first channel (0 is channel 1).
    pub channel: u16,
    /// Linear gain.
    pub volume: f32,
}

/// Why a routing cannot be played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingError {
    /// Two nodes share an id.
    Duplicate(u64),
    /// An output, send or sidechain names a node that is not there.
    Unknown { from: u64, to: u64 },
    /// The nodes feed each other in a loop (through outputs, sends or
    /// sidechains).
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

/// The delay compensation a routing needs (M4 slice 6): how far the
/// device outputs trail what the tracks play, and the latent nodes on the
/// longest path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Compensation {
    /// Frames. The engine plays the timeline this far ahead, so what is
    /// heard lines up with the playhead (read ahead, owner, ADR-0041).
    pub frames: u32,
    /// `(node, frames its devices add)` along the longest path, from the
    /// first node to the last; only nodes that add latency.
    pub path: Vec<(u64, u32)>,
}
