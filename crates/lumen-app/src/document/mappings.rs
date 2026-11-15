use lumen_core::automation::FLOOR_DB;
use lumen_project::automation::Target;
use lumen_project::mapping::{self, Mapping};
use lumen_project::model::{Id, Level};

use super::Document;
use super::mixer::find;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapRefusal {
    NoTrack,
    NoTarget,
    Full,
    TimingParameter,
}

impl MapRefusal {
    pub fn reason(self) -> &'static str {
        match self {
            MapRefusal::NoTrack => "That track is gone",
            MapRefusal::NoTarget => "There is nothing there to map",
            MapRefusal::Full => "A project holds 1,024 controller mappings at most",
            MapRefusal::TimingParameter => {
                "That is the device's timing, so a controller cannot move it"
            }
        }
    }
}

fn is_timing_parameter(project: &lumen_project::model::Project, target: Target) -> bool {
    let Target::Device { device, param } = target else {
        return false;
    };
    crate::document::find_device(project, device)
        .is_some_and(|(_, d)| lumen_devices::timing_parameter(crate::builtin::name_of(d), param))
}

impl Document {
    pub fn mapping_of(&self, track: Id, target: Target) -> Option<Mapping> {
        self.project()
            .mappings
            .iter()
            .copied()
            .find(|m| m.track == track && m.target == target)
    }

    pub fn mapping_at(&self, channel: u8, number: u8) -> Option<Mapping> {
        self.project()
            .mappings
            .iter()
            .copied()
            .find(|m| m.hears(channel, number))
    }

    pub fn set_mapping(
        &mut self,
        track: Id,
        target: Target,
        channel: u8,
        number: u8,
    ) -> Result<(), MapRefusal> {
        let held = find(self.project(), track).ok_or(MapRefusal::NoTrack)?;
        if !held.has_target(target) {
            return Err(MapRefusal::NoTarget);
        }
        if is_timing_parameter(self.project(), target) {
            return Err(MapRefusal::TimingParameter);
        }
        let new = Mapping {
            channel,
            number,
            track,
            target,
        };
        let list = mapping::with(&self.project().mappings, new).ok_or(MapRefusal::Full)?;
        self.seal();
        self.history
            .apply("Map controller", None, |p| p.mappings = list);
        self.seal();
        Ok(())
    }

    pub fn clear_mapping(&mut self, track: Id, target: Target) -> bool {
        if self.mapping_of(track, target).is_none() {
            return false;
        }
        let list = mapping::without(&self.project().mappings, track, target);
        self.seal();
        self.history
            .apply("Clear controller mapping", None, |p| p.mappings = list);
        self.seal();
        true
    }

    pub fn clear_all_mappings(&mut self) -> usize {
        let n = self.project().mappings.len();
        if n > 0 {
            self.seal();
            self.history
                .apply("Clear controller mappings", None, |p| p.mappings.clear());
            self.seal();
        }
        n
    }

    pub fn move_control(&mut self, track: Id, target: Target, value: f32) -> bool {
        let Some(held) = find(self.project(), track) else {
            return false;
        };
        if !held.has_target(target) || !value.is_finite() {
            return false;
        }
        if is_timing_parameter(self.project(), target) {
            return false;
        }
        let value = target.clamp(value);
        let now = lumen_project::automation::own_value(held, target);
        if now == value || (now.is_infinite() && value <= FLOOR_DB) {
            return false;
        }
        match target {
            Target::Volume => self.set_volume(track, value),
            Target::Pan => self.set_pan(track, value),
            Target::Trim => self.set_trim(track, value),
            Target::Mute => self.set_mute(&[track], value >= 0.5),
            Target::Send { to } => {
                let level = if value <= FLOOR_DB {
                    Level::SILENT
                } else {
                    Level(value)
                };
                if self.set_send(track, to, level).is_err() {
                    return false;
                }
            }
            Target::ClipGain { .. } | Target::ClipPan { .. } | Target::ClipPitch { .. } => {
                return false;
            }
            Target::Device { device, param } => {
                self.override_param(device, param);
                let merge = format!("mapped {:x} {param}", device.0);
                let _ = self.set_device_params(
                    device,
                    &[(param, f64::from(value))],
                    "Move plugin control",
                    Some(&merge),
                );
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::model::TrackKind;

    use super::*;

    fn doc() -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Audio, None);
        doc.seal();
        (doc, track)
    }

    fn own(doc: &Document, track: Id, target: Target) -> f32 {
        lumen_project::automation::own_value(find(doc.project(), track).unwrap(), target)
    }

    #[test]
    fn the_lookahead_cannot_be_mapped_and_a_mapping_to_it_is_ignored() {
        use lumen_devices::dynamics::limiter;
        let (mut doc, track) = doc();
        let values = crate::builtin::defaults(limiter::NAME);
        let device = doc
            .add_device_with(
                track,
                lumen_project::model::PluginRef::Builtin {
                    name: limiter::NAME.to_owned(),
                },
                &values,
                crate::document::Slot::End,
            )
            .expect("the device");
        let look = Target::Device {
            device,
            param: limiter::LOOK,
        };
        assert_eq!(
            doc.set_mapping(track, look, 0, 21),
            Err(MapRefusal::TimingParameter)
        );
        assert!(doc.project().mappings.is_empty());
        let held = |doc: &Document| {
            find(doc.project(), track)
                .expect("the track")
                .devices
                .iter()
                .find(|d| d.id == device)
                .and_then(|d| d.params.get(&limiter::LOOK).copied())
        };
        let before = held(&doc);
        assert!(!doc.move_control(track, look, 0.9));
        assert_eq!(held(&doc), before);
        let ceiling = Target::Device {
            device,
            param: limiter::CEILING,
        };
        assert!(doc.set_mapping(track, ceiling, 0, 21).is_ok());
    }

    #[test]
    fn mapping_is_one_undo_step_and_undo_removes_it() {
        let (mut doc, track) = doc();
        doc.set_mapping(track, Target::Volume, 0, 7).unwrap();
        assert_eq!(doc.mapping_at(0, 7).map(|m| m.target), Some(Target::Volume));
        assert_eq!(doc.undo().as_deref(), Some("Map controller"));
        assert!(doc.mapping_at(0, 7).is_none());
        doc.redo();
        assert!(doc.mapping_of(track, Target::Volume).is_some());
    }

    #[test]
    fn learning_a_taken_controller_moves_it() {
        let (mut doc, track) = doc();
        doc.set_mapping(track, Target::Volume, 0, 7).unwrap();
        doc.set_mapping(track, Target::Pan, 0, 7).unwrap();
        assert!(doc.mapping_of(track, Target::Volume).is_none());
        assert_eq!(doc.project().mappings.len(), 1);
    }

    #[test]
    fn a_control_the_track_does_not_have_cannot_be_mapped() {
        let (mut doc, track) = doc();
        let r = doc.set_mapping(track, Target::Send { to: Id(99) }, 0, 7);
        assert_eq!(r, Err(MapRefusal::NoTarget));
        assert_eq!(
            doc.set_mapping(Id(999_999), Target::Volume, 0, 7),
            Err(MapRefusal::NoTrack)
        );
        assert_eq!(
            doc.set_mapping(track, Target::Volume, 16, 7),
            Err(MapRefusal::Full)
        );
    }

    #[test]
    fn clearing_removes_the_mapping_in_one_step() {
        let (mut doc, track) = doc();
        assert!(!doc.clear_mapping(track, Target::Volume));
        doc.set_mapping(track, Target::Volume, 0, 7).unwrap();
        assert!(doc.clear_mapping(track, Target::Volume));
        assert!(doc.project().mappings.is_empty());
        doc.undo();
        assert_eq!(doc.project().mappings.len(), 1);
    }

    #[test]
    fn a_mapped_move_is_a_hand_on_the_control() {
        let (mut doc, track) = doc();
        let lane = Lane::new(vec![
            Point::new(Ticks::ZERO, -12.0),
            Point::new(Ticks::quarters(4), 0.0),
        ])
        .unwrap();
        doc.set_lane(track, Target::Volume, lane, false).unwrap();
        doc.seal();
        assert!(doc.move_control(track, Target::Volume, -6.0));
        assert!(
            doc.overrides().contains(track, Target::Volume),
            "takes the lane over"
        );
        assert_eq!(doc.take_touched(), vec![(track, Target::Volume)]);
        assert!(doc.move_control(track, Target::Volume, -4.0));
        assert!(doc.move_control(track, Target::Volume, -2.0));
        assert_eq!(own(&doc, track, Target::Volume), -2.0);
        assert_eq!(doc.undo().as_deref(), Some("Change volume"));
        assert_eq!(own(&doc, track, Target::Volume), 0.0);
    }

    #[test]
    fn a_value_the_control_holds_is_no_move() {
        let (mut doc, track) = doc();
        assert!(!doc.move_control(track, Target::Volume, 0.0));
        assert!(doc.take_touched().is_empty());
        assert!(!doc.move_control(track, Target::Volume, f32::NAN));
        assert!(!doc.move_control(Id(999_999), Target::Volume, -3.0));
    }

    #[test]
    fn every_kind_of_control_moves() {
        let (mut doc, track) = doc();
        assert!(doc.move_control(track, Target::Pan, 0.5));
        assert!(doc.move_control(track, Target::Trim, 3.0));
        assert!(doc.move_control(track, Target::Mute, 1.0));
        let t = find(doc.project(), track).unwrap();
        assert_eq!((t.pan, t.trim_db, t.mute), (0.5, 3.0, true));
        assert!(doc.move_control(track, Target::Mute, 0.0));
        assert!(!find(doc.project(), track).unwrap().mute);
    }
}
