//! Controller mappings (M7 slice A5, format v23): learning one, clearing one,
//! and moving a mapped control as a hand on it would. Mapping and clearing are
//! undo steps; a controller's moves merge into one step until the gesture is
//! sealed.

use lumen_core::automation::FLOOR_DB;
use lumen_project::automation::Target;
use lumen_project::mapping::{self, Mapping};
use lumen_project::model::{Id, Level};

use super::Document;
use super::mixer::find;

/// Why a mapping was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapRefusal {
    /// No such track.
    NoTrack,
    /// The track has nothing there to move: a send it does not make, a device
    /// it does not hold.
    NoTarget,
    /// The channel or controller number is not MIDI's, or the list is full.
    Full,
}

impl MapRefusal {
    /// Why, in a few words, for a note on screen.
    pub fn reason(self) -> &'static str {
        match self {
            MapRefusal::NoTrack => "That track is gone",
            MapRefusal::NoTarget => "There is nothing there to map",
            MapRefusal::Full => "A project holds 1,024 controller mappings at most",
        }
    }
}

impl Document {
    /// The mapping that moves `track`'s `target`, if there is one.
    pub fn mapping_of(&self, track: Id, target: Target) -> Option<Mapping> {
        self.project()
            .mappings
            .iter()
            .copied()
            .find(|m| m.track == track && m.target == target)
    }

    /// The mapping controller `number` on `channel` has, if there is one.
    pub fn mapping_at(&self, channel: u8, number: u8) -> Option<Mapping> {
        self.project()
            .mappings
            .iter()
            .copied()
            .find(|m| m.hears(channel, number))
    }

    /// Makes controller `number` on `channel` move `track`'s `target`. Any
    /// mapping that controller or that control had is replaced. One undo
    /// step.
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

    /// Takes away the mapping that moves `track`'s `target`. Returns whether
    /// there was one. One undo step.
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

    /// Takes away every mapping. Returns how many there were.
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

    /// Moves `track`'s `target` to `value`, as the control itself would be
    /// moved: it takes over a lane that plays, is noted for the recorder, and
    /// is one undo step with the moves that follow it. A value the control
    /// already holds is no move. Returns whether the control was moved.
    pub fn move_control(&mut self, track: Id, target: Target, value: f32) -> bool {
        let Some(held) = find(self.project(), track) else {
            return false;
        };
        if !held.has_target(target) || !value.is_finite() {
            return false;
        }
        let value = target.clamp(value);
        let now = crate::arrangement::auto_scale::own_value(held, target);
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
        crate::arrangement::auto_scale::own_value(find(doc.project(), track).unwrap(), target)
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
        // Further moves are the same undo step.
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
