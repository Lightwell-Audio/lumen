//! Lanes the user has taken over (M7 slice A4a, D7). Moving a control whose
//! lane is on sets an override for that target: the lane plays nothing and the
//! control's own value is heard, until the lane is re-enabled. It is session
//! state, not part of the project: it is not saved, not undone, and a project
//! that is opened or made starts without any.

use std::collections::BTreeSet;

use lumen_project::automation::{AutomationLane, Target};
use lumen_project::model::{Id, Track};

use super::Document;

/// The lanes taken over, by the track that holds them and the target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides(BTreeSet<(Id, Target)>);

impl Overrides {
    /// Whether `track`'s lane for `target` is taken over.
    pub fn contains(&self, track: Id, target: Target) -> bool {
        self.0.contains(&(track, target))
    }

    /// Whether `entry`, a lane of `track`, plays: it is on and not taken over.
    pub fn plays(&self, track: &Track, entry: &AutomationLane) -> bool {
        entry.enabled && !self.contains(track.id, entry.target)
    }

    /// Whether any of `track`'s lanes is taken over.
    pub fn any_for(&self, track: Id) -> bool {
        self.0
            .range((track, Target::Volume)..)
            .next()
            .is_some_and(|(t, _)| *t == track)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Document {
    /// The lanes taken over now.
    pub fn overrides(&self) -> &Overrides {
        &self.overrides
    }

    /// Takes over `track`'s lane for `target`, if the track has one and it is
    /// on: the user moved the control it drives. Returns whether it changed.
    pub fn override_lane(&mut self, track: Id, target: Target) -> bool {
        let on = super::mixer::find(self.project(), track)
            .and_then(|t| t.lane(target))
            .is_some_and(|l| l.enabled);
        on && self.overrides.0.insert((track, target))
    }

    /// [`Document::override_lane`] for parameter `param` of `device`, a
    /// plugin's own control moved in Lumen's window or the plugin's.
    pub fn override_param(&mut self, device: Id, param: u32) -> bool {
        let Some((track, _)) = super::find_device(self.project(), device) else {
            return false;
        };
        self.override_lane(track, Target::Device { device, param })
    }

    /// Lets `track`'s lane for `target` play again. Returns whether it was
    /// taken over.
    pub fn reenable_lane(&mut self, track: Id, target: Target) -> bool {
        self.overrides.0.remove(&(track, target))
    }

    /// Lets every lane play again. Returns how many were taken over.
    pub fn reenable_all(&mut self) -> usize {
        let n = self.overrides.0.len();
        self.overrides.0.clear();
        n
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::automation::Target;
    use lumen_project::model::TrackKind;

    use super::*;

    fn with_lane() -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Audio, None);
        let lane = Lane::new(vec![
            Point::new(Ticks::ZERO, -12.0),
            Point::new(Ticks::quarters(4), 0.0),
        ])
        .unwrap();
        doc.set_lane(track, Target::Volume, lane, false).unwrap();
        (doc, track)
    }

    #[test]
    fn moving_the_fader_under_a_lane_takes_the_lane_over() {
        let (mut doc, track) = with_lane();
        assert!(doc.overrides().is_empty());
        doc.set_volume(track, -3.0);
        assert!(doc.overrides().contains(track, Target::Volume));
        assert!(doc.overrides().any_for(track));
    }

    #[test]
    fn a_control_with_no_lane_or_a_lane_that_is_off_is_not_taken_over() {
        let (mut doc, track) = with_lane();
        doc.set_pan(track, 0.3);
        assert!(doc.overrides().is_empty(), "no pan lane");
        doc.set_lane_enabled(track, Target::Volume, false).unwrap();
        doc.set_volume(track, -3.0);
        assert!(doc.overrides().is_empty(), "the lane is off already");
    }

    #[test]
    fn every_track_control_a_lane_can_drive_takes_it_over() {
        let (mut doc, track) = with_lane();
        let other = doc.add_track(TrackKind::Return, None);
        for target in [
            Target::Pan,
            Target::Trim,
            Target::Mute,
            Target::Send { to: other },
        ] {
            if let Target::Send { to } = target {
                doc.set_send(track, to, lumen_project::model::Level(-6.0))
                    .unwrap();
            }
            let lane = Lane::new(vec![Point::new(Ticks::ZERO, target.range().0)]).unwrap();
            doc.set_lane(track, target, lane, false).unwrap();
        }
        doc.set_pan(track, 0.5);
        doc.set_trim(track, 2.0);
        doc.set_mute(&[track], true);
        doc.set_send(track, other, lumen_project::model::Level(-3.0))
            .unwrap();
        for target in [
            Target::Pan,
            Target::Trim,
            Target::Mute,
            Target::Send { to: other },
        ] {
            assert!(doc.overrides().contains(track, target), "{target:?}");
        }
        assert!(!doc.overrides().contains(track, Target::Volume));
    }

    #[test]
    fn re_enabling_lets_the_lane_play_again_and_is_not_an_undo_step() {
        let (mut doc, track) = with_lane();
        doc.set_volume(track, -3.0);
        let state = doc.state();
        assert!(doc.reenable_lane(track, Target::Volume));
        assert!(!doc.reenable_lane(track, Target::Volume), "already");
        assert!(doc.overrides().is_empty());
        assert_eq!(doc.state(), state, "no edit of the project");
    }

    #[test]
    fn editing_switching_or_removing_the_lane_ends_the_override() {
        for end in 0..3 {
            let (mut doc, track) = with_lane();
            doc.set_volume(track, -3.0);
            assert!(doc.overrides().contains(track, Target::Volume));
            match end {
                0 => {
                    let lane = Lane::new(vec![Point::new(Ticks::ZERO, 0.0)]).unwrap();
                    doc.set_lane(track, Target::Volume, lane, false).unwrap();
                }
                1 => doc.set_lane_enabled(track, Target::Volume, false).unwrap(),
                _ => doc.remove_lane(track, Target::Volume).unwrap(),
            }
            assert!(doc.overrides().is_empty(), "case {end}");
        }
    }

    #[test]
    fn undo_does_not_change_what_is_taken_over() {
        let (mut doc, track) = with_lane();
        doc.set_volume(track, -3.0);
        doc.undo();
        assert!(doc.overrides().contains(track, Target::Volume));
    }

    #[test]
    fn a_new_document_starts_without_any() {
        let (mut doc, track) = with_lane();
        doc.set_volume(track, -3.0);
        let fresh = Document::new(doc.project().clone());
        assert!(fresh.overrides().is_empty());
        assert_eq!(doc.reenable_all(), 1);
    }
}
