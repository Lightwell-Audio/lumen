use lumen_core::pan::PanLaw;

use super::auto::NodeAutomation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Destination {
    Node(u64),
    Hardware(u16),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SendSpec {
    pub to: u64,
    pub gain: f32,
    pub pre_fader: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Tap {
    PreFx,
    #[default]
    PostFx,
    PostMixer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SidechainSpec {
    pub from: u64,
    pub tap: Tap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Device {
    Delay {
        frames: u32,
        listen: bool,
    },
    Hosted {
        host: u64,
        latency: u32,
    },
    Builtin {
        device: u64,
        latency: u32,
        bypass: bool,
    },
}

impl Device {
    pub fn latency(&self) -> u32 {
        match *self {
            Self::Delay { frames, .. } => frames.min(super::MAX_DEVICE_LATENCY),
            Self::Hosted { latency, .. } | Self::Builtin { latency, .. } => {
                latency.min(super::MAX_DEVICE_LATENCY)
            }
        }
    }

    pub fn bypass(&self) -> bool {
        matches!(*self, Self::Builtin { bypass: true, .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeSpec {
    pub id: u64,
    pub trim: f32,
    pub invert: bool,
    pub volume: f32,
    pub pan: f32,
    pub law: PanLaw,
    pub audible: bool,
    pub sends_audible: bool,
    pub cue: bool,
    pub output: Destination,
    pub sends: Vec<SendSpec>,
    pub devices: Vec<Device>,
    pub source: u32,
    pub sidechains: Vec<SidechainSpec>,
    pub values: Vec<(u64, Vec<(u32, f32)>)>,
    pub automation: Option<NodeAutomation>,
}

impl NodeSpec {
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
            values: Vec::new(),
            automation: None,
        }
    }

    pub fn latency(&self) -> u32 {
        self.devices
            .iter()
            .map(Device::latency)
            .fold(0u32, u32::saturating_add)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Routing {
    pub nodes: Vec<NodeSpec>,
    pub cue: Option<CueOut>,
    pub measure: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueOut {
    pub channel: u16,
    pub volume: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingError {
    Duplicate(u64),
    Unknown { from: u64, to: u64 },
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Compensation {
    pub frames: u32,
    pub path: Vec<(u64, u32)>,
}
