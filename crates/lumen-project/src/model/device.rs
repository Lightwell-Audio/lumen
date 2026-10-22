//! Devices on a track: built-in and hosted plugins, and what they report.

use super::*;

/// The stable name of the built-in synth in `PluginRef::Builtin` (format
/// v12). Never the product name, and never changed.
pub const SYNTH: &str = "synth";

/// What a device does in a track's chain (format v17): an instrument makes
/// sound from MIDI, an effect processes audio. Saved with a plugin so a
/// plugin that is missing on this machine still sits in the right place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Instrument,
    /// What a device written before v17 means.
    #[default]
    Effect,
}

impl DeviceKind {
    pub fn is_default(&self) -> bool {
        *self == Self::Effect
    }
}

/// The biggest saved state of one blob of one device: the host's own limit.
pub const MAX_DEVICE_STATE: usize = 16 << 20;

/// A tail that never ends (VST3's `kInfiniteTail`).
pub const INFINITE_TAIL: u32 = u32::MAX;

/// What a plugin last said about its timing (format v17), kept so a plugin
/// that goes missing or fails keeps its place in time: the bypass holds this
/// latency (the owner's D1 and D7). Frames at `rate` Hz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reported {
    pub latency: u32,
    /// Frames the sound rings on after the input stops; [`INFINITE_TAIL`]
    /// when it never stops.
    pub tail: u32,
    /// The sample rate the two numbers are in.
    pub rate: u32,
}

impl Reported {
    /// The latency in frames at `rate`, for a device that reported it at
    /// another rate.
    pub fn latency_at(&self, rate: u32) -> u32 {
        if self.rate == 0 || self.rate == rate {
            return self.latency;
        }
        (u64::from(self.latency) * u64::from(rate)).div_ceil(u64::from(self.rate)) as u32
    }
}

/// Where on the source track a sidechain is taken from (format v20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidechainTap {
    /// Before the track's devices: mute and fader do not touch it.
    PreFx,
    /// After the track's devices, before mute and fader.
    #[default]
    PostFx,
    /// What the track plays: after mute, fader and pan.
    PostMixer,
}

/// A sidechain: another track whose sound keys an effect (format v20).
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
    /// Opaque state blob from the plugin, stored as base64. For a VST3
    /// plugin this is the component's state (and, in files before v17, all
    /// the state there was).
    #[serde(with = "base64_bytes", default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<u8>,
    /// A VST3 plugin's edit-controller state, apart from the component's
    /// (format v17); empty when it keeps none.
    #[serde(with = "base64_bytes", default, skip_serializing_if = "Vec::is_empty")]
    pub controller_state: Vec<u8>,
    /// The plugin's version when its state was captured (format v17), so an
    /// update can be told from the build that wrote the state.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plugin_version: String,
    /// The plugin's last reported latency and tail (format v17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported: Option<Reported>,
    /// Parameter values the project holds over the saved state (format v19),
    /// normalized 0 to 1 by the plugin's parameter id: what was set from
    /// Lumen or from the plugin's own window since the state was captured. A
    /// plugin restores its state and then these are set, so undo and redo of a
    /// parameter change are edits to this map. Written only when there are
    /// some; a save that captures the plugin's state afresh clears it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<u32, f64>,
    /// The track whose sound keys this effect, if it takes a sidechain
    /// (format v20): where the signal is taken from on that track. Written
    /// only when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<Sidechain>,
    /// How many times the device's sound was replaced as a whole (a preset
    /// loaded), so the running plugin can tell that the project holds another
    /// sound than the one it has, after an undo or redo, without comparing
    /// megabytes of state. Not saved: a project opens with the plugin set up
    /// from what the file holds.
    #[serde(skip)]
    pub state_rev: u32,
}

impl Device {
    /// A device with no VST3-only data: what every device was before v17.
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
        }
    }

    /// A VST3 device with no state yet.
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

    /// What the device does in a chain: the synth is an instrument, a VST3
    /// plugin says which it is, any other built-in device is an effect.
    pub fn kind(&self) -> DeviceKind {
        self.plugin.kind()
    }

    pub fn is_instrument(&self) -> bool {
        self.kind() == DeviceKind::Instrument
    }

    pub fn is_vst3(&self) -> bool {
        matches!(self.plugin, PluginRef::Vst3 { .. })
    }

    /// The built-in synth with a preset (`"poly"`, `"keys"`, ...). For a
    /// built-in device `state` is its own state; the synth's is its
    /// preset's stable name as text.
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

    /// The preset of a synth: its stable name, if the state is readable.
    pub fn preset(&self) -> Option<&str> {
        self.is_synth()
            .then(|| std::str::from_utf8(&self.state).ok())
            .flatten()
    }
}

impl PluginRef {
    /// What the plugin does in a chain: the synth is an instrument, a VST3
    /// plugin says which it is, any other built-in device is an effect.
    pub fn kind(&self) -> DeviceKind {
        match self {
            PluginRef::Vst3 { kind, .. } => *kind,
            PluginRef::Builtin { name } if name == SYNTH => DeviceKind::Instrument,
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
        /// Instrument or effect (format v17); older files hold effects.
        #[serde(default, skip_serializing_if = "DeviceKind::is_default")]
        kind: DeviceKind,
    },
    Builtin {
        name: String,
    },
}
