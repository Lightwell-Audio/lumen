//! The keys and commands on automation lanes (M7 slices A1 and A2). They
//! work on the lane of the chosen track: A shows or hides it, a command adds
//! a point at the playhead, the arrows move the picked points, Delete takes
//! them away, copy, paste and duplicate repeat them, and commands give the
//! segments a shape or put a tool in hand.

use eframe::egui;
use lumen_core::automation::{Lane, Shape};
use lumen_core::curve::Curve;
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::edit::automation as pure;
use lumen_project::model::{Id, Track};

use crate::App;
use crate::arrangement::auto_scale as scale;
use crate::arrangement::auto_state::{LaneClip, Picks, Tool};
use crate::arrangement::auto_view::without;

/// How far an arrow moves a picked point's value, as a share of the lane's
/// height, and with the fine modifier.
const VALUE_STEP: f32 = 0.02;
const VALUE_STEP_FINE: f32 = 0.005;

/// The value `track`'s own control holds for `target` now: where a lane
/// that is just starting should begin, so adding a point does not jump.
fn control_value(track: &Track, target: Target) -> f32 {
    match target {
        Target::Volume => track.gain_db,
        Target::Pan => track.pan,
        Target::Trim => track.trim_db,
        Target::Mute => f32::from(u8::from(track.mute)),
        Target::Send { to } => track
            .sends
            .iter()
            .find(|s| s.to == to)
            .map_or(0.0, |s| s.level.0),
        Target::Device { .. } => scale::default_value(target),
    }
}

impl App {
    /// The tracks the lane commands act on: the selected tracks, else the
    /// track last clicked in the lanes.
    pub(crate) fn auto_tracks(&self) -> Vec<Id> {
        if self.arrangement.selected.is_empty() {
            self.arrangement.insert.into_iter().collect()
        } else {
            self.arrangement.selected.clone()
        }
    }

    /// The lane the commands act on: the first chosen track that shows one.
    pub(crate) fn auto_chosen(&self) -> Option<(Id, Target)> {
        let shown = &self.arrangement.auto.shown;
        self.auto_tracks()
            .into_iter()
            .find_map(|t| shown.get(&t).map(|target| (t, *target)))
    }

    /// Whether the lane under the commands has points of its own yet.
    pub(crate) fn auto_chosen_has_lane(&self) -> bool {
        self.auto_chosen().is_some_and(|(track, target)| {
            self.document
                .project()
                .tracks
                .iter()
                .find(|t| t.id == track)
                .is_some_and(|t| t.lane(target).is_some())
        })
    }

    /// Points are picked and the keys mean them: not while the clip editor
    /// or the Devices tab holds the keyboard.
    pub(crate) fn auto_keys_picked(&self) -> bool {
        self.arrangement.auto.picks.is_some() && !self.note_keys() && !self.chain.live
    }

    /// A lane has the keyboard: it was pressed last, and the clip editor and
    /// the Devices tab do not hold it.
    pub(crate) fn auto_keys_lane(&self) -> bool {
        self.arrangement.auto.focus
            && self.auto_chosen().is_some()
            && !self.note_keys()
            && !self.chain.live
    }

    /// Something to paste into the lane that has the keyboard.
    pub(crate) fn auto_can_paste(&self) -> bool {
        self.auto_keys_lane() && self.arrangement.auto.clip.is_some()
    }

    /// The picks, if they are on the lane the commands act on.
    fn auto_picks(&self) -> Option<Picks> {
        let (track, target) = self.auto_chosen()?;
        self.arrangement.auto.picked_on(track, target).cloned()
    }

    /// Shows or hides the lanes of the chosen tracks.
    pub(crate) fn auto_show(&mut self) {
        let tracks = self.auto_tracks();
        self.arrangement
            .auto
            .toggle(self.document.project(), &tracks);
    }

    /// Opens the parameter chooser of the chosen lane.
    pub(crate) fn auto_choose(&self, ctx: &egui::Context) {
        if let Some((track, _)) = self.auto_chosen() {
            crate::widgets::Menu::open_for(
                ctx,
                egui::Id::new(("automation chip", track)).with("lane"),
            );
        }
    }

    /// Adds a point at the playhead to the chosen lane, holding the value
    /// the lane has there, or the control's own when the lane is new.
    pub(crate) fn auto_add_point(&mut self) {
        let Some((track, target)) = self.auto_chosen() else {
            return;
        };
        let at = self.transport.position();
        let project = self.document.project();
        let Some(held) = project.tracks.iter().find(|t| t.id == track) else {
            return;
        };
        let value = held
            .lane(target)
            .and_then(|l| l.lane.value_at(at))
            .unwrap_or_else(|| control_value(held, target));
        self.add_auto_point(track, target, at, value, false);
    }

    /// Switches the chosen lane on or off, keeping its points.
    pub(crate) fn auto_toggle_lane(&mut self) {
        let Some((track, target)) = self.auto_chosen() else {
            return;
        };
        let on = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.lane(target))
            .is_some_and(|l| l.enabled);
        let result = self.document.set_lane_enabled(track, target, !on);
        self.arrangement.note_lane(result);
    }

    /// Takes the chosen lane away, points and all.
    pub(crate) fn auto_remove_lane(&mut self) {
        let Some((track, target)) = self.auto_chosen() else {
            return;
        };
        let result = self.document.remove_lane(track, target);
        self.arrangement.note_lane(result);
        self.arrangement.auto.picks = None;
    }

    /// Takes the picked points out of their lane. The last one takes the
    /// lane with it, and the control is its own again.
    pub(crate) fn auto_delete(&mut self) {
        let Some(picks) = self.arrangement.auto.picks.take() else {
            return;
        };
        if let Some(lane) = self.lane_points_of(picks.track, picks.target) {
            let next = without(&lane, &picks.list());
            let result = self
                .document
                .set_lane(picks.track, picks.target, next, false);
            self.arrangement.note_lane(result);
        }
    }

    /// Moves the picked points: `time` grid steps (earlier is negative) and
    /// `value` steps of the lane's height (up is positive). Fine steps for
    /// the value are a quarter the size.
    pub(crate) fn auto_nudge(&mut self, time: i32, value: i32, fine: bool) {
        let Some(picks) = self.arrangement.auto.picks.clone() else {
            return;
        };
        let Some(lane) = self.lane_points_of(picks.track, picks.target) else {
            return;
        };
        let list = picks.list();
        let Some(first) = list.first().and_then(|&i| lane.points().get(i).copied()) else {
            return;
        };
        let mut at = first.at;
        if time > 0 {
            at = self.grid_cell(at).1;
        } else if time < 0 {
            let (start, _) = self.grid_cell(at);
            at = if start < at {
                start
            } else {
                self.grid_cell(at - Ticks(1)).0
            };
        }
        let target = picks.target;
        let map = |v: f32| {
            if value == 0 {
                v
            } else if target == Target::Mute {
                if value > 0 { 1.0 } else { 0.0 }
            } else {
                let step = if fine { VALUE_STEP_FINE } else { VALUE_STEP };
                scale::value(target, scale::fraction(target, v) + value as f32 * step)
            }
        };
        if let Some(done) = pure::shifted(&lane, &list, at - first.at, map) {
            // One undo step for each press: not a drag.
            let result = self
                .document
                .set_lane(picks.track, picks.target, done.lane, false);
            self.arrangement.note_lane(result);
        }
    }

    /// Picks every point of the chosen lane.
    pub(crate) fn auto_select_all(&mut self) {
        let Some((track, target)) = self.auto_chosen() else {
            return;
        };
        let count = self
            .lane_points_of(track, target)
            .map_or(0, |lane| lane.len());
        self.arrangement.auto.picks = (count > 0).then(|| Picks {
            track,
            target,
            indices: (0..count).collect(),
        });
    }

    /// Gives the segments leaving the picked points `shape`.
    pub(crate) fn auto_shape_segments(&mut self, shape: Shape) {
        let Some(picks) = self.auto_picks() else {
            return;
        };
        let Some(lane) = self.lane_points_of(picks.track, picks.target) else {
            return;
        };
        if let Some(next) = pure::with_shape(&lane, &picks.list(), shape) {
            let result = self
                .document
                .set_lane(picks.track, picks.target, next, false);
            self.arrangement.note_lane(result);
        }
    }

    /// The curve a segment takes when it is first made curved: the S, which
    /// reads as bent at once and meets its neighbours smoothly.
    pub(crate) fn auto_curve() -> Shape {
        Shape::Curve(Curve::S_CURVE)
    }

    /// Puts `tool` in hand.
    pub(crate) fn auto_use_tool(&mut self, tool: Tool) {
        self.arrangement.auto.use_tool(tool);
    }

    /// B: the pencil, or the pointer again.
    pub(crate) fn auto_toggle_draw(&mut self) {
        let next = if self.arrangement.auto.tool == Tool::Pencil {
            Tool::Pointer
        } else {
            Tool::Pencil
        };
        self.arrangement.auto.use_tool(next);
    }

    /// Keeps the picked points to paste.
    pub(crate) fn auto_copy(&mut self) {
        let Some(picks) = self.auto_picks() else {
            return;
        };
        let Some(lane) = self.lane_points_of(picks.track, picks.target) else {
            return;
        };
        if let Some(copied) = pure::copy(&lane, &picks.list()) {
            self.arrangement.auto.clip = Some(LaneClip {
                copied,
                range: picks.target.range(),
            });
        }
    }

    /// Pastes what was copied at the playhead, over what is there.
    pub(crate) fn auto_paste(&mut self) {
        let Some((track, target)) = self.auto_chosen() else {
            return;
        };
        let Some(clip) = self.arrangement.auto.clip.clone() else {
            return;
        };
        if clip.range != target.range() {
            self.arrangement.note = Some("Those points belong to another kind of parameter");
            return;
        }
        let lane = self.lane_points_of(track, target).unwrap_or_default();
        let at = self.transport.position();
        let Some((next, indices)) = pure::paste(&lane, &clip.copied, at) else {
            self.arrangement.note = Some("They do not fit there");
            return;
        };
        self.commit_points(track, target, next, indices);
    }

    /// Repeats the picked points right after themselves.
    pub(crate) fn auto_duplicate(&mut self) {
        let Some(picks) = self.auto_picks() else {
            return;
        };
        let Some(lane) = self.lane_points_of(picks.track, picks.target) else {
            return;
        };
        let Some((next, indices)) = pure::duplicate(&lane, &picks.list()) else {
            self.arrangement.note = Some("A lane holds 65,536 points at most");
            return;
        };
        self.commit_points(picks.track, picks.target, next, indices);
    }

    /// Writes `lane` as one undo step and picks `indices` of it.
    fn commit_points(&mut self, track: Id, target: Target, lane: Lane, indices: Vec<usize>) {
        match self.document.set_lane(track, target, lane, false) {
            Ok(()) => {
                self.arrangement.auto.picks = Some(Picks {
                    track,
                    target,
                    indices: indices.into_iter().collect(),
                });
            }
            Err(refused) => self.arrangement.note = Some(refused.reason()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::model::{Color, Level, Send, TrackKind};

    #[test]
    fn a_new_lane_starts_where_its_control_is() {
        let mut track = Track::new(Id(1), "T", TrackKind::Audio, Color(0));
        track.gain_db = -9.0;
        track.pan = 0.25;
        track.trim_db = 3.0;
        track.mute = true;
        track.sends.push(Send {
            to: Id(5),
            level: Level(-12.0),
            pre_fader: false,
        });
        assert_eq!(control_value(&track, Target::Volume), -9.0);
        assert_eq!(control_value(&track, Target::Pan), 0.25);
        assert_eq!(control_value(&track, Target::Trim), 3.0);
        assert_eq!(control_value(&track, Target::Mute), 1.0);
        assert_eq!(control_value(&track, Target::Send { to: Id(5) }), -12.0);
        assert_eq!(control_value(&track, Target::Send { to: Id(6) }), 0.0);
    }
}
