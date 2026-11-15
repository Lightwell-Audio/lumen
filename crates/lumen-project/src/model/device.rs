use super::*;

pub const SYNTH: &str = "synth";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Instrument,

    #[default]
    Effect,
}

impl DeviceKind {
    pub fn is_default(&self) -> bool {
        *self == Self::Effect
    }
}

pub const MAX_DEVICE_STATE: usize = 16 << 20;

pub const INFINITE_TAIL: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reported {
    pub latency: u32,

    pub tail: u32,

    pub rate: u32,
}

impl Reported {
    pub fn latency_at(&self, rate: u32) -> u32 {
        if self.rate == 0 || self.rate == rate {
            return self.latency;
        }
        (u64::from(self.latency) * u64::from(rate)).div_ceil(u64::from(self.rate)) as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidechainTap {
    PreFx,

    #[default]
    PostFx,

    PostMixer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sidechain {
    pub from: Id,
    #[serde(default)]
    pub tap: SidechainTap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: Id,
    pub plugin: PluginRef,
    #[serde(default)]
    pub bypass: bool,

    #[serde(with = "base64_bytes", default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<u8>,

    #[serde(with = "base64_bytes", default, skip_serializing_if = "Vec::is_empty")]
    pub controller_state: Vec<u8>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plugin_version: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported: Option<Reported>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<u32, f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<Sidechain>,

    #[serde(skip)]
    pub state_rev: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler: Option<Sampler>,
}

impl Device {
    pub fn new(id: Id, plugin: PluginRef, bypass: bool, state: Vec<u8>) -> Self {
        Self {
            id,
            plugin,
            bypass,
            state,
            controller_state: Vec::new(),
            plugin_version: String::new(),
            reported: None,
            params: BTreeMap::new(),
            sidechain: None,
            state_rev: 0,
            sampler: None,
        }
    }

    pub fn vst3(id: Id, class_id: &str, name: &str, vendor: &str, kind: DeviceKind) -> Self {
        Self::new(
            id,
            PluginRef::Vst3 {
                class_id: class_id.into(),
                name: name.into(),
                vendor: vendor.into(),
                kind,
            },
            false,
            Vec::new(),
        )
    }

    pub fn kind(&self) -> DeviceKind {
        self.plugin.kind()
    }

    pub fn is_instrument(&self) -> bool {
        self.kind() == DeviceKind::Instrument
    }

    pub fn is_vst3(&self) -> bool {
        matches!(self.plugin, PluginRef::Vst3 { .. })
    }

    pub fn synth(id: Id, preset: &str) -> Self {
        Self::new(
            id,
            PluginRef::Builtin { name: SYNTH.into() },
            false,
            preset.as_bytes().to_vec(),
        )
    }

    pub fn is_synth(&self) -> bool {
        matches!(&self.plugin, PluginRef::Builtin { name } if name == SYNTH)
    }

    pub fn is_sampler(&self) -> bool {
        matches!(&self.plugin, PluginRef::Builtin { name } if name == SAMPLER)
    }

    pub fn sampler(&self) -> Option<&Sampler> {
        self.sampler.as_ref().filter(|_| self.is_sampler())
    }

    pub fn preset(&self) -> Option<&str> {
        self.is_synth()
            .then(|| std::str::from_utf8(&self.state).ok())
            .flatten()
    }
}

impl PluginRef {
    pub fn kind(&self) -> DeviceKind {
        match self {
            PluginRef::Vst3 { kind, .. } => *kind,
            PluginRef::Builtin { name } if name == SYNTH || name == SAMPLER => {
                DeviceKind::Instrument
            }
            PluginRef::Builtin { .. } => DeviceKind::Effect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum PluginRef {
    Vst3 {
        class_id: String,
        name: String,
        vendor: String,

        #[serde(default, skip_serializing_if = "DeviceKind::is_default")]
        kind: DeviceKind,
    },
    Builtin {
        name: String,
    },
}
