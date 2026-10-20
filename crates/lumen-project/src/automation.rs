//! What a track's automation lanes hold (format v22, M7 slice A0): the
//! parameter a lane drives, in the units a person reads, and the range it
//! may take.

use lumen_core::automation::{FLOOR_DB, Lane};
use serde::{Deserialize, Serialize};

use crate::model::{Id, Track};

/// The loudest a fader or send lane may go, in dB: the top of both controls.
pub const LEVEL_MAX_DB: f32 = 6.0;
/// How far input trim reaches either way, in dB.
pub const TRIM_RANGE_DB: f32 = 24.0;
/// The most lanes a track holds.
pub const MAX_LANES: usize = 256;

/// A value of a track that a lane drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// The fader, in dB.
    Volume,
    /// Pan, −1 (left) to +1 (right).
    Pan,
    /// Input trim, in dB.
    Trim,
    /// Mute: 0 is heard and 1 is muted.
    Mute,
    /// The send to the return `to`, in dB.
    Send { to: Id },
    /// A parameter of one of the track's devices, normalised 0 to 1.
    Device { device: Id, param: u32 },
}

impl Target {
    /// The lowest and highest value a lane for this target may hold.
    pub fn range(self) -> (f32, f32) {
        match self {
            Target::Volume | Target::Send { .. } => (FLOOR_DB, LEVEL_MAX_DB),
            Target::Trim => (-TRIM_RANGE_DB, TRIM_RANGE_DB),
            Target::Pan => (-1.0, 1.0),
            Target::Mute | Target::Device { .. } => (0.0, 1.0),
        }
    }

    /// Whether the value is a level in dB, which the engine turns into a
    /// gain (silent at the floor).
    pub fn is_level(self) -> bool {
        matches!(self, Target::Volume | Target::Trim | Target::Send { .. })
    }

    /// The device this target belongs to, if it is a device's.
    pub fn device(self) -> Option<Id> {
        match self {
            Target::Device { device, .. } => Some(device),
            _ => None,
        }
    }

    /// `value` brought into the target's range.
    pub fn clamp(self, value: f32) -> f32 {
        let (low, high) = self.range();
        value.clamp(low, high)
    }
}

/// A lane and the parameter it drives. A track holds at most one lane for a
/// target. A lane that is not `enabled` is kept but plays nothing, and the
/// control it drives keeps the value it was set to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub target: Target,
    pub lane: Lane,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

impl AutomationLane {
    pub fn new(target: Target, lane: Lane) -> Self {
        Self {
            target,
            lane,
            enabled: true,
        }
    }
}

impl Track {
    /// The lane that drives `target`, if the track has one.
    pub fn lane(&self, target: Target) -> Option<&AutomationLane> {
        self.automation.iter().find(|l| l.target == target)
    }

    /// Whether `target` is something this track has: its own values always,
    /// a send it makes, a device it holds.
    pub fn has_target(&self, target: Target) -> bool {
        match target {
            Target::Volume | Target::Pan | Target::Trim | Target::Mute => true,
            Target::Send { to } => self.sends.iter().any(|s| s.to == to),
            Target::Device { device, .. } => self.devices.iter().any(|d| d.id == device),
        }
    }

    /// Whether a lane that is on drives parameter `param` of `device`.
    pub fn drives(&self, device: Id, param: u32) -> bool {
        self.lane(Target::Device { device, param })
            .is_some_and(|l| l.enabled)
    }

    /// The parameters of `device` that lanes drive now.
    pub fn driven_params(&self, device: Id) -> impl Iterator<Item = u32> + '_ {
        self.automation
            .iter()
            .filter(|l| l.enabled)
            .filter_map(move |l| match l.target {
                Target::Device { device: d, param } if d == device => Some(param),
                _ => None,
            })
    }

    /// Drops the lanes of devices the track no longer has. Call it after any
    /// edit that takes a device off the track, in the same undo step.
    pub fn prune_device_lanes(&mut self) {
        let Self {
            devices,
            automation,
            ..
        } = self;
        automation.retain(|l| {
            l.target
                .device()
                .is_none_or(|d| devices.iter().any(|x| x.id == d))
        });
    }

    /// Takes out the lanes of `device`, which is on its way to another track.
    pub fn take_device_lanes(&mut self, device: Id) -> Vec<AutomationLane> {
        let (taken, kept) = std::mem::take(&mut self.automation)
            .into_iter()
            .partition(|l| l.target.device() == Some(device));
        self.automation = kept;
        taken
    }
}

/// Why a track's lanes cannot be kept.
pub(crate) fn check(track: &Track) -> Result<(), String> {
    if track.automation.len() > MAX_LANES {
        return Err(format!("more than {MAX_LANES} automation lanes"));
    }
    for (index, entry) in track.automation.iter().enumerate() {
        if track.automation[..index]
            .iter()
            .any(|e| e.target == entry.target)
        {
            return Err(format!("two lanes drive the same {:?}", entry.target));
        }
        let (low, high) = entry.target.range();
        for point in entry.lane.points() {
            if !(low..=high).contains(&point.value) {
                return Err(format!(
                    "a {:?} lane holds {} outside {low} to {high}",
                    entry.target, point.value
                ));
            }
            if point.at < lumen_core::time::Ticks::ZERO || !crate::validate::within(point.at) {
                return Err(format!(
                    "a {:?} lane has a point out of range",
                    entry.target
                ));
            }
        }
        // A lane for a send with no return named is kept: undoing the
        // return's deletion restores the send, and the engine plays no lane
        // for a send that is not there. The same holds for a device that is
        // gone.
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_target_has_the_range_its_control_has() {
        assert_eq!(Target::Volume.range(), (FLOOR_DB, 6.0));
        assert_eq!(Target::Send { to: Id(1) }.range(), (FLOOR_DB, 6.0));
        assert_eq!(Target::Trim.range(), (-24.0, 24.0));
        assert_eq!(Target::Pan.range(), (-1.0, 1.0));
        assert_eq!(Target::Mute.range(), (0.0, 1.0));
        assert_eq!(
            Target::Device {
                device: Id(2),
                param: 9
            }
            .range(),
            (0.0, 1.0)
        );
        assert_eq!(Target::Pan.clamp(3.0), 1.0);
    }

    #[test]
    fn targets_are_written_so_a_person_can_read_them() {
        let text = |t: Target| serde_json::to_string(&t).unwrap();
        assert_eq!(text(Target::Volume), "\"volume\"");
        assert_eq!(
            text(Target::Send { to: Id(255) }),
            "{\"send\":{\"to\":\"00000000000000ff\"}}"
        );
        let device = Target::Device {
            device: Id(16),
            param: 7,
        };
        assert_eq!(
            serde_json::from_str::<Target>(&text(device)).unwrap(),
            device
        );
    }
}
