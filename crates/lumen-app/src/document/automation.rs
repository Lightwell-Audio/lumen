//! A track's automation lanes (M7 slice A0, format v22): making a lane,
//! replacing the points of one, switching one off and taking one away. Each
//! is one undo step; the drag of a point merges into one. A lane follows the
//! thing it drives: a device's lanes go with the device when it moves or is
//! copied and go when it is removed (`devices.rs`), a send's lane goes with
//! its return (`mixer.rs`).

use std::sync::Arc;

use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;
use lumen_project::automation::{AutomationLane, MAX_LANES, Target};
use lumen_project::model::Id;

use super::Document;
use super::mixer::tracks_mut;

/// A lane edit that is one move of a drag.
#[derive(Debug, Clone, Copy)]
enum Gesture {
    Drag,
}

/// Why a lane edit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneRefusal {
    /// No such track.
    NoTrack,
    /// The track has nothing the lane could drive: a send it does not make,
    /// a device it does not hold.
    NoTarget,
    /// The track already has the most lanes it may.
    TooMany,
    /// A point before the start of the song.
    BeforeStart,
}

impl LaneRefusal {
    /// Why, in a few words, for a note on screen.
    pub fn reason(self) -> &'static str {
        match self {
            LaneRefusal::NoTrack => "That track is gone",
            LaneRefusal::NoTarget => "The track has nothing to automate there",
            LaneRefusal::TooMany => "A track holds 256 lanes at most",
            LaneRefusal::BeforeStart => "Automation cannot start before the song does",
        }
    }
}

impl Document {
    /// Makes `track`'s lane for `target` hold `lane`, making the lane if the
    /// track has none, and switching it on. Values are brought into the
    /// target's range. An empty lane takes the lane away. A drag passes
    /// `gesture`, and its moves are one undo step.
    pub fn set_lane(
        &mut self,
        track: Id,
        target: Target,
        lane: Lane,
        gesture: bool,
    ) -> Result<(), LaneRefusal> {
        self.write_lane(track, target, lane, gesture.then_some(Gesture::Drag), None)
    }

    /// Writes a recorded pass as `lane` (M7 slice A4b): one undo step named
    /// "Record automation", never merged with the step before it. As any
    /// edit of a lane, it lets the lane play again if the user had taken it
    /// over.
    pub fn record_lane(
        &mut self,
        track: Id,
        target: Target,
        lane: Lane,
    ) -> Result<(), LaneRefusal> {
        self.seal();
        let done = self.write_lane(track, target, lane, None, Some("Record automation"));
        self.seal();
        done
    }

    fn write_lane(
        &mut self,
        track: Id,
        target: Target,
        lane: Lane,
        gesture: Option<Gesture>,
        named: Option<&str>,
    ) -> Result<(), LaneRefusal> {
        let held = super::mixer::find(self.project(), track).ok_or(LaneRefusal::NoTrack)?;
        if !held.has_target(target) {
            return Err(LaneRefusal::NoTarget);
        }
        if lane.points().first().is_some_and(|p| p.at < Ticks::ZERO) {
            return Err(LaneRefusal::BeforeStart);
        }
        if let Some((clip, kind)) = target.clip() {
            return self.write_envelope(track, (clip, kind), lane, gesture.is_some());
        }
        let exists = held.lane(target).is_some();
        if lane.is_empty() {
            return self.remove_lane(track, target);
        }
        if !exists && held.automation.len() >= MAX_LANES {
            return Err(LaneRefusal::TooMany);
        }
        let points = lane
            .points()
            .iter()
            .map(|p| Point {
                value: target.clamp(p.value),
                ..*p
            })
            .collect();
        let lane = Lane::new(points).expect("the same points in the same order");
        let label = named.unwrap_or(if exists {
            "Edit automation"
        } else {
            "Add automation lane"
        });
        // Drawing on a lane is asking to hear it.
        self.reenable_lane(track, target);
        let merge = gesture.map(|_| format!("automation {:x} {target:?}", track.0));
        self.history.apply(label, merge.as_deref(), |project| {
            if let Some(held) = tracks_mut(project).find(|t| t.id == track) {
                let held = Arc::make_mut(held);
                match held.automation.iter_mut().find(|l| l.target == target) {
                    Some(entry) => {
                        entry.lane = lane.clone();
                        entry.enabled = true;
                    }
                    None => held
                        .automation
                        .push(AutomationLane::new(target, lane.clone())),
                }
            }
        });
        Ok(())
    }

    /// Makes the envelope of `kind` of the audio clip `clip` hold `lane`,
    /// which is in song time (the lane editor's clock); it is kept from the
    /// clip's start. Values are brought into the kind's range and an empty lane
    /// takes the envelope away. A drag's moves are one undo step.
    fn write_envelope(
        &mut self,
        track: Id,
        (clip, kind): (Id, lumen_project::envelope::Kind),
        lane: Lane,
        gesture: bool,
    ) -> Result<(), LaneRefusal> {
        let held = super::mixer::find(self.project(), track).ok_or(LaneRefusal::NoTrack)?;
        let start = held
            .clips
            .iter()
            .find(|c| c.id == clip)
            .ok_or(LaneRefusal::NoTarget)?
            .start;
        let (low, high) = kind.range();
        let points = lane
            .points()
            .iter()
            .map(|p| Point {
                at: p.at - start,
                value: p.value.clamp(low, high),
                ..*p
            })
            .collect();
        let lane = Lane::new(points).expect("the same points in the same order");
        let exists = held
            .clips
            .iter()
            .find(|c| c.id == clip)
            .is_some_and(|c| c.envelopes.get(kind).is_some());
        let label = match (lane.is_empty(), exists) {
            (true, _) => "Remove clip envelope",
            (false, true) => "Edit clip envelope",
            (false, false) => "Add clip envelope",
        };
        let merge = gesture.then(|| format!("envelope {:x} {kind:?}", clip.0));
        self.history.apply(label, merge.as_deref(), |project| {
            if let Some(held) = tracks_mut(project).find(|t| t.id == track)
                && let Some(c) = Arc::make_mut(held)
                    .clips
                    .iter_mut()
                    .find(|c| c.id == clip)
            {
                Arc::make_mut(c).envelopes.set(kind, lane.clone());
            }
        });
        Ok(())
    }

    /// Switches `track`'s lane for `target` on or off, keeping its points. A
    /// lane that is off plays nothing, and the control it drives keeps the
    /// value it was set to. The lane already so records nothing.
    pub fn set_lane_enabled(
        &mut self,
        track: Id,
        target: Target,
        enabled: bool,
    ) -> Result<(), LaneRefusal> {
        let held = super::mixer::find(self.project(), track).ok_or(LaneRefusal::NoTrack)?;
        if held.lane(target).is_none() {
            return Err(LaneRefusal::NoTarget);
        }
        let label = if enabled {
            "Turn automation on"
        } else {
            "Turn automation off"
        };
        self.reenable_lane(track, target);
        self.history.apply(label, None, |project| {
            if let Some(held) = tracks_mut(project).find(|t| t.id == track) {
                let held = Arc::make_mut(held);
                if let Some(entry) = held.automation.iter_mut().find(|l| l.target == target) {
                    entry.enabled = enabled;
                }
            }
        });
        Ok(())
    }

    /// Takes `track`'s lane for `target` away, points and all.
    pub fn remove_lane(&mut self, track: Id, target: Target) -> Result<(), LaneRefusal> {
        let held = super::mixer::find(self.project(), track).ok_or(LaneRefusal::NoTrack)?;
        if let Some((clip, kind)) = target.clip() {
            return self.write_envelope(track, (clip, kind), Lane::default(), false);
        }
        if held.lane(target).is_none() {
            return Err(LaneRefusal::NoTarget);
        }
        self.reenable_lane(track, target);
        self.history
            .apply("Remove automation lane", None, |project| {
                if let Some(held) = tracks_mut(project).find(|t| t.id == track) {
                    Arc::make_mut(held)
                        .automation
                        .retain(|l| l.target != target);
                }
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::automation::Shape;
    use lumen_project::model::{DeviceKind, Level, PluginRef, TrackKind};

    use super::*;
    use crate::document::Slot;

    fn lane(points: &[(i64, f32)]) -> Lane {
        Lane::new(
            points
                .iter()
                .map(|&(q, v)| Point::new(Ticks::quarters(q), v))
                .collect(),
        )
        .unwrap()
    }

    fn doc_with(kinds: &[TrackKind]) -> (Document, Vec<Id>) {
        let mut doc = Document::new(crate::empty_project());
        for &kind in kinds {
            doc.add_track(kind, None);
        }
        let ids = doc.project().tracks.iter().map(|t| t.id).collect();
        (doc, ids)
    }

    fn lanes(doc: &Document, id: Id) -> Vec<AutomationLane> {
        super::super::mixer::find(doc.project(), id)
            .unwrap()
            .automation
            .clone()
    }

    fn effect(doc: &mut Document, on: Id) -> Id {
        doc.add_device(
            on,
            PluginRef::Vst3 {
                class_id: "ABCDEF0123456789ABCDEF0123456789".into(),
                name: "Room".into(),
                vendor: "Vendor".into(),
                kind: DeviceKind::Effect,
            },
            Slot::End,
        )
        .unwrap()
    }

    #[test]
    fn a_lane_is_made_edited_switched_and_removed_one_step_each() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let t = ids[0];
        assert_eq!(
            doc.set_lane(t, Target::Volume, lane(&[(0, -6.0), (4, 0.0)]), false),
            Ok(())
        );
        assert_eq!(doc.history().undo_label(), Some("Add automation lane"));
        assert_eq!(
            doc.set_lane(t, Target::Volume, lane(&[(0, -3.0)]), false),
            Ok(())
        );
        assert_eq!(doc.history().undo_label(), Some("Edit automation"));
        assert_eq!(lanes(&doc, t).len(), 1, "a track has one lane for a target");
        assert_eq!(doc.set_lane_enabled(t, Target::Volume, false), Ok(()));
        assert!(!lanes(&doc, t)[0].enabled);
        // Already off: nothing recorded.
        let steps = doc.history().steps().0.len();
        assert_eq!(doc.set_lane_enabled(t, Target::Volume, false), Ok(()));
        assert_eq!(doc.history().steps().0.len(), steps);
        assert_eq!(doc.remove_lane(t, Target::Volume), Ok(()));
        assert!(lanes(&doc, t).is_empty());
        // Four steps since the track was added.
        for _ in 0..4 {
            doc.undo();
        }
        assert!(lanes(&doc, t).is_empty(), "undone to the start");
        for _ in 0..4 {
            doc.redo();
        }
        assert!(lanes(&doc, t).is_empty(), "redone to the end");
    }

    #[test]
    fn a_drag_of_a_lane_is_one_undo_step_that_restores_the_exact_lane() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let t = ids[0];
        doc.set_lane(t, Target::Pan, lane(&[(0, -1.0), (4, 1.0)]), false)
            .unwrap();
        let before = lanes(&doc, t);
        let steps = doc.history().steps().0.len();
        for k in 1..40 {
            doc.set_lane(
                t,
                Target::Pan,
                lane(&[(0, -1.0), (4, 1.0 - k as f32 / 40.0)]),
                true,
            )
            .unwrap();
        }
        assert_eq!(doc.history().undo_label(), Some("Edit automation"));
        doc.undo();
        assert_eq!(lanes(&doc, t), before);
        assert_eq!(
            doc.history().steps().0.len(),
            steps,
            "the whole drag was one step"
        );
    }

    #[test]
    fn values_are_brought_into_the_targets_range() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let t = ids[0];
        doc.set_lane(t, Target::Pan, lane(&[(0, -7.0), (1, 9.0)]), false)
            .unwrap();
        doc.set_lane(t, Target::Volume, lane(&[(0, 40.0), (1, -500.0)]), false)
            .unwrap();
        let held = lanes(&doc, t);
        let values: Vec<f32> = held
            .iter()
            .flat_map(|l| l.lane.points().iter().map(|p| p.value))
            .collect();
        assert_eq!(values, [-1.0, 1.0, 6.0, -100.0]);
        lumen_project::validate(doc.project()).unwrap();
    }

    #[test]
    fn an_empty_lane_takes_the_lane_away() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let t = ids[0];
        doc.set_lane(t, Target::Trim, lane(&[(0, 3.0)]), false)
            .unwrap();
        doc.set_lane(t, Target::Trim, Lane::default(), false)
            .unwrap();
        assert!(lanes(&doc, t).is_empty());
        assert_eq!(
            doc.set_lane(t, Target::Trim, Lane::default(), false),
            Err(LaneRefusal::NoTarget),
            "there is nothing to take away"
        );
    }

    #[test]
    fn refusals_say_why_and_change_nothing() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Return]);
        let t = ids[0];
        let steps = doc.history().steps().0.len();
        let gone = Id(9_999_999);
        let one = || lane(&[(0, 0.0)]);
        assert_eq!(
            doc.set_lane(gone, Target::Volume, one(), false),
            Err(LaneRefusal::NoTrack)
        );
        assert_eq!(
            doc.set_lane(t, Target::Send { to: ids[1] }, one(), false),
            Err(LaneRefusal::NoTarget),
            "a send the track does not make"
        );
        assert_eq!(
            doc.set_lane(
                t,
                Target::Device {
                    device: gone,
                    param: 0
                },
                one(),
                false
            ),
            Err(LaneRefusal::NoTarget)
        );
        let early = Lane::new(vec![Point::new(Ticks(-1), 0.0)]).unwrap();
        assert_eq!(
            doc.set_lane(t, Target::Volume, early, false),
            Err(LaneRefusal::BeforeStart)
        );
        assert_eq!(
            doc.set_lane_enabled(t, Target::Volume, true),
            Err(LaneRefusal::NoTarget)
        );
        assert_eq!(
            doc.remove_lane(t, Target::Volume),
            Err(LaneRefusal::NoTarget)
        );
        assert_eq!(doc.history().steps().0.len(), steps);
    }

    #[test]
    fn a_track_holds_the_most_lanes_and_no_more() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let t = ids[0];
        let device = effect(&mut doc, t);
        for param in 0..MAX_LANES as u32 {
            doc.set_lane(
                t,
                Target::Device { device, param },
                lane(&[(0, 0.5)]),
                false,
            )
            .unwrap();
        }
        assert_eq!(
            doc.set_lane(t, Target::Volume, lane(&[(0, 0.0)]), false),
            Err(LaneRefusal::TooMany)
        );
        // A lane that is there can still be edited.
        assert_eq!(
            doc.set_lane(
                t,
                Target::Device { device, param: 0 },
                lane(&[(0, 0.7)]),
                false
            ),
            Ok(())
        );
    }

    #[test]
    fn a_send_lane_goes_with_the_return_and_undo_brings_both_back() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Return]);
        let (t, ret) = (ids[0], ids[1]);
        doc.set_send(t, ret, Level(-6.0)).unwrap();
        doc.set_lane(
            t,
            Target::Send { to: ret },
            lane(&[(0, -20.0), (4, 0.0)]),
            false,
        )
        .unwrap();
        doc.set_lane(t, Target::Volume, lane(&[(0, -3.0)]), false)
            .unwrap();
        doc.delete_tracks(&[ret]);
        let held = lanes(&doc, t);
        assert_eq!(held.len(), 1, "only the fader lane is left");
        assert_eq!(held[0].target, Target::Volume);
        lumen_project::validate(doc.project()).unwrap();
        doc.undo();
        assert_eq!(lanes(&doc, t).len(), 2);
    }

    #[test]
    fn a_devices_lanes_go_with_it_when_it_moves_are_copied_with_a_copy_and_go_when_it_does() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Audio]);
        let (a, b) = (ids[0], ids[1]);
        let device = effect(&mut doc, a);
        let target = Target::Device { device, param: 7 };
        doc.set_lane(a, target, lane(&[(0, 0.2), (4, 0.9)]), false)
            .unwrap();
        doc.set_lane(a, Target::Volume, lane(&[(0, -2.0)]), false)
            .unwrap();

        // Moving to another track takes the lane along, in the same step.
        let steps = doc.history().steps().0.len();
        doc.move_device(a, device, b, Slot::End).unwrap();
        assert_eq!(doc.history().steps().0.len(), steps + 1);
        assert_eq!(lanes(&doc, a).len(), 1);
        assert_eq!(lanes(&doc, b)[0].target, target);
        // Within one chain nothing moves.
        doc.move_device(b, device, b, Slot::At(0)).unwrap();
        assert_eq!(lanes(&doc, b).len(), 1);

        // A copy is a new device with its own copy of the lane.
        let copy = doc.copy_device(b, device, a, Slot::End).unwrap();
        let copied = lanes(&doc, a);
        let copied = copied
            .iter()
            .find(|l| l.target.device() == Some(copy))
            .expect("the copy's lane");
        assert_eq!(copied.lane, lanes(&doc, b)[0].lane);
        assert_eq!(lanes(&doc, b).len(), 1, "the original keeps its own");
        lumen_project::validate(doc.project()).unwrap();

        // Removing a device takes its lanes; undo puts both back.
        doc.remove_device(b, device).unwrap();
        assert!(lanes(&doc, b).is_empty());
        doc.undo();
        assert_eq!(lanes(&doc, b).len(), 1);
        // Undoing the copy and the move puts the lane back where it was.
        doc.undo();
        doc.undo();
        assert_eq!(lanes(&doc, a).len(), 2);
        assert!(lanes(&doc, b).is_empty());
    }

    #[test]
    fn a_saved_project_keeps_its_lanes() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let t = ids[0];
        doc.set_lane(
            t,
            Target::Volume,
            Lane::new(vec![
                Point::new(Ticks::quarters(0), -12.0).with_shape(Shape::Step),
                Point::new(Ticks::quarters(4), 0.0),
            ])
            .unwrap(),
            false,
        )
        .unwrap();
        let bytes = lumen_project::to_bytes(doc.project()).unwrap();
        let again = lumen_project::from_bytes(&bytes).unwrap();
        assert_eq!(&again, doc.project());
    }
}
