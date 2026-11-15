use lumen_core::automation::{FLOOR_DB, Lane};
use serde::{Deserialize, Serialize};

use crate::model::{Id, Track};

pub const LEVEL_MAX_DB: f32 = 6.0;
pub const TRIM_RANGE_DB: f32 = 24.0;
pub const MAX_LANES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Volume,
    Pan,
    Trim,
    Mute,
    Send { to: Id },
    Device { device: Id, param: u32 },
    ClipGain { clip: Id },
    ClipPan { clip: Id },
    ClipPitch { clip: Id },
}

impl Target {
    pub fn range(self) -> (f32, f32) {
        match self {
            Target::Volume | Target::Send { .. } | Target::ClipGain { .. } => {
                (FLOOR_DB, LEVEL_MAX_DB)
            }
            Target::Trim => (-TRIM_RANGE_DB, TRIM_RANGE_DB),
            Target::Pan | Target::ClipPan { .. } => (-1.0, 1.0),
            Target::ClipPitch { .. } => crate::envelope::Kind::Pitch.range(),
            Target::Mute | Target::Device { .. } => (0.0, 1.0),
        }
    }

    pub fn default_value(self) -> f32 {
        match self {
            Target::Device { .. } => 0.5,
            _ => 0.0,
        }
    }

    pub fn scale(self) -> Target {
        match self {
            Target::ClipGain { .. } => Target::Volume,
            Target::ClipPan { .. } => Target::Pan,
            other => other,
        }
    }

    pub fn clip(self) -> Option<(Id, crate::envelope::Kind)> {
        match self {
            Target::ClipGain { clip } => Some((clip, crate::envelope::Kind::Gain)),
            Target::ClipPan { clip } => Some((clip, crate::envelope::Kind::Pan)),
            Target::ClipPitch { clip } => Some((clip, crate::envelope::Kind::Pitch)),
            _ => None,
        }
    }

    pub fn envelope(clip: Id, kind: crate::envelope::Kind) -> Target {
        match kind {
            crate::envelope::Kind::Gain => Target::ClipGain { clip },
            crate::envelope::Kind::Pan => Target::ClipPan { clip },
            crate::envelope::Kind::Pitch => Target::ClipPitch { clip },
        }
    }

    pub fn device(self) -> Option<Id> {
        match self {
            Target::Device { device, .. } => Some(device),
            _ => None,
        }
    }

    pub fn clamp(self, value: f32) -> f32 {
        let (low, high) = self.range();
        value.clamp(low, high)
    }
}

pub fn own_value(track: &Track, target: Target) -> f32 {
    match target {
        Target::Volume => track.gain_db,
        Target::Pan => track.pan,
        Target::Trim => track.trim_db,
        Target::Mute => f32::from(u8::from(track.mute)),
        Target::Send { to } => track
            .sends
            .iter()
            .find(|s| s.to == to)
            .map_or(f32::NEG_INFINITY, |s| s.level.0),
        Target::Device { device, param } => track
            .devices
            .iter()
            .find(|d| d.id == device)
            .and_then(|d| d.params.get(&param))
            .map_or_else(|| target.default_value(), |&v| v as f32),
        Target::ClipGain { .. } | Target::ClipPan { .. } | Target::ClipPitch { .. } => 0.0,
    }
}

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
    pub fn lane(&self, target: Target) -> Option<&AutomationLane> {
        self.automation.iter().find(|l| l.target == target)
    }

    pub fn lane_view(&self, target: Target) -> Option<std::borrow::Cow<'_, AutomationLane>> {
        let Some((id, kind)) = target.clip() else {
            return self.lane(target).map(std::borrow::Cow::Borrowed);
        };
        let clip = self.clips.iter().find(|c| c.id == id)?;
        let lane = clip.envelopes.get(kind)?;
        let points = lane
            .points()
            .iter()
            .map(|p| lumen_core::automation::Point {
                at: p.at + clip.start,
                ..*p
            })
            .collect();
        let lane = Lane::new(points).ok()?;
        Some(std::borrow::Cow::Owned(AutomationLane::new(target, lane)))
    }

    pub fn has_target(&self, target: Target) -> bool {
        match target {
            Target::Volume | Target::Pan | Target::Trim | Target::Mute => true,
            Target::Send { to } => self.sends.iter().any(|s| s.to == to),
            Target::Device { device, .. } => self.devices.iter().any(|d| d.id == device),
            Target::ClipGain { clip } | Target::ClipPan { clip } | Target::ClipPitch { clip } => {
                self.clips.iter().any(|c| {
                    c.id == clip && matches!(c.content, crate::model::ClipContent::Audio { .. })
                })
            }
        }
    }

    pub fn drives(&self, device: Id, param: u32) -> bool {
        self.lane(Target::Device { device, param })
            .is_some_and(|l| l.enabled)
    }

    pub fn driven_params(&self, device: Id) -> impl Iterator<Item = u32> + '_ {
        self.automation
            .iter()
            .filter(|l| l.enabled)
            .filter_map(move |l| match l.target {
                Target::Device { device: d, param } if d == device => Some(param),
                _ => None,
            })
    }

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

    pub fn take_device_lanes(&mut self, device: Id) -> Vec<AutomationLane> {
        let (taken, kept) = std::mem::take(&mut self.automation)
            .into_iter()
            .partition(|l| l.target.device() == Some(device));
        self.automation = kept;
        taken
    }
}

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
        if entry.target.clip().is_some() {
            return Err("a lane drives a clip's envelope".to_owned());
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
