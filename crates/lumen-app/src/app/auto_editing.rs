//! The pointer and the keys on automation lanes (M7 slice A1). A lane shown
//! over a track's clips takes the pointer there: a click adds a point (or
//! picks the one under it), a drag moves a point, or adds one and moves it.
//! Ctrl reaches the clip under the lane instead. Points snap in time to
//! the grid (Alt places them freely) and never pass their neighbours.
//!
//! Every edit is one undo step, and a drag is one step in all. The keys
//! work on the lane of the chosen track: A shows or hides it, a command adds
//! a point at the playhead, the arrows move the picked point and Delete
//! takes it away.

use eframe::egui::{self, CursorIcon, Pos2, Rect};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::{Id, Track};

use super::editing::MenuFor;
use super::grid::draw_snap_guide;
use super::info::{Held, Info, Mouse};
use crate::App;
use crate::arrangement::auto_scale as scale;
use crate::arrangement::auto_view::{Frame, Picked, inserted, moved, removed};
use crate::commands::Command;

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
    /// The lane under `p`, if the track there shows one.
    fn lane_hit(&self, ctx: &egui::Context, canvas: Rect, p: Pos2) -> Option<(Id, Target, Frame)> {
        self.arrangement.lane_at(
            ctx,
            canvas,
            self.document.project(),
            self.lanes.pending().len(),
            self.nav.view,
            p,
        )
    }

    /// The lane's points as the document holds them.
    fn lane_points_of(&self, track: Id, target: Target) -> Option<lumen_core::automation::Lane> {
        self.document
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.lane(target))
            .map(|l| l.lane.clone())
    }

    /// Lane gestures, from the lanes' `response`. Returns whether a lane took
    /// the event, in which case the clips under it get none of it.
    pub(crate) fn auto_editing(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        canvas: Rect,
        painter: &egui::Painter,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let modifiers = ui.input(|i| i.modifiers);
        // A drag that began on a point goes on wherever the pointer is.
        if let Some(drag) = self.arrangement.auto.dragging {
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(p) = response.interact_pointer_pos()
            {
                self.auto_drag(&ctx, drag, p, canvas, modifiers.alt, painter);
            }
            if response.drag_stopped() {
                self.arrangement.auto.dragging = None;
            }
            return true;
        }
        if self.arrangement.auto.shown.is_empty() {
            self.arrangement.auto.hover = None;
            return false;
        }
        // Ctrl reaches the clips under the lane.
        let command = modifiers.command;

        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
            && !command
            && let Some((track, target, frame)) = self.lane_hit(&ctx, canvas, origin)
        {
            self.arrangement.hover = None;
            self.auto_press(track, target, &frame, origin, modifiers.alt);
            return true;
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            if !command && let Some((track, target, frame)) = self.lane_hit(&ctx, canvas, p) {
                self.auto_click(track, target, &frame, p, modifiers.alt);
                return true;
            }
            // A click anywhere else lets go of the point.
            self.arrangement.auto.picked = None;
            return false;
        }
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos()
            && !command
            && self.lane_hit(&ctx, canvas, p).is_some()
        {
            return true;
        }
        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
            && !command
            && let Some((track, _, frame)) = self.lane_hit(&ctx, canvas, p)
        {
            // The lane's menu is the lanes' (the grid, paste): the clips
            // under the lane are not what was clicked.
            let at = self.snapped(frame.ticks_at(p.x), modifiers.alt);
            self.arrangement.menu_spot = Some((at.max(Ticks::ZERO), Some(track)));
            self.menu_for = MenuFor::Lanes;
            return true;
        }

        if self.arrangement.gesture.is_some() || egui::Popup::is_any_open(&ctx) {
            return false;
        }
        self.arrangement.auto.hover = None;
        let Some(p) = response.hover_pos() else {
            return false;
        };
        if command {
            return false;
        }
        let Some((track, target, frame)) = self.lane_hit(&ctx, canvas, p) else {
            return false;
        };
        self.arrangement.hover = None;
        self.auto_hover(&ctx, track, target, &frame, p);
        true
    }

    /// The pointer over a lane: which point, the cursor, and the info line.
    fn auto_hover(
        &mut self,
        ctx: &egui::Context,
        track: Id,
        target: Target,
        frame: &Frame,
        p: Pos2,
    ) {
        let lane = self.lane_points_of(track, target);
        let near = lane
            .as_ref()
            .and_then(|l| frame.nearest(target, l, p).map(|i| (i, l.points()[i])));
        let project = self.document.project();
        let name = scale::name(project, target);
        let (cursor, info) = match near {
            Some((index, point)) => {
                self.arrangement.auto.hover = Some(Picked {
                    track,
                    target,
                    index,
                });
                (
                    CursorIcon::Grab,
                    Info::new(format!(
                        "{name} point \u{00B7} {}",
                        scale::text(target, point.value)
                    ))
                    .on(Held::NONE, Mouse::Drag, "move")
                    .on(Held::NONE, Mouse::Click, "pick")
                    .key(Command::Delete, "delete"),
                )
            }
            None => (
                CursorIcon::Crosshair,
                Info::new(format!("{name} lane"))
                    .on(Held::NONE, Mouse::Click, "add a point")
                    .on(Held::NONE, Mouse::Drag, "add a point and move it")
                    .on(Held::ALT, Mouse::Click, "add it off the grid")
                    .on(Held::CTRL, Mouse::Click, "reach the clips under the lane")
                    .key(Command::AutomationShow, "hide lanes"),
            ),
        };
        ctx.set_cursor_icon(cursor);
        self.set_info(info);
    }

    /// A press that becomes a drag: on a point, picks it up; on empty lane,
    /// adds a point there and picks that up.
    fn auto_press(&mut self, track: Id, target: Target, frame: &Frame, origin: Pos2, free: bool) {
        let lane = self.lane_points_of(track, target);
        if let Some(index) = lane.as_ref().and_then(|l| frame.nearest(target, l, origin)) {
            let here = Picked {
                track,
                target,
                index,
            };
            self.arrangement.auto.picked = Some(here);
            self.arrangement.auto.dragging = Some(here);
            return;
        }
        let at = self.snapped(frame.ticks_at(origin.x), free);
        let value = frame.value_at(target, origin.y);
        if let Some(here) = self.add_auto_point(track, target, at, value, true) {
            self.arrangement.auto.dragging = Some(here);
        }
    }

    /// A click on a lane: picks the point under it, or adds one.
    fn auto_click(&mut self, track: Id, target: Target, frame: &Frame, p: Pos2, free: bool) {
        let lane = self.lane_points_of(track, target);
        if let Some(index) = lane.as_ref().and_then(|l| frame.nearest(target, l, p)) {
            self.arrangement.auto.picked = Some(Picked {
                track,
                target,
                index,
            });
            return;
        }
        let at = self.snapped(frame.ticks_at(p.x), free);
        let value = frame.value_at(target, p.y);
        self.add_auto_point(track, target, at, value, false);
    }

    /// Adds a point to `track`'s lane for `target` (a new lane if it has
    /// none) and picks it. `gesture` joins the edit to the drag it starts.
    fn add_auto_point(
        &mut self,
        track: Id,
        target: Target,
        at: Ticks,
        value: f32,
        gesture: bool,
    ) -> Option<Picked> {
        let lane = self.lane_points_of(track, target);
        let Some((made, index)) = inserted(target, lane.as_ref(), at, value) else {
            self.arrangement.note = Some("A lane holds 100,000 points at most");
            return None;
        };
        match self.document.set_lane(track, target, made, gesture) {
            Ok(()) => {
                let here = Picked {
                    track,
                    target,
                    index,
                };
                self.arrangement.auto.picked = Some(here);
                Some(here)
            }
            Err(refused) => {
                self.arrangement.note = Some(refused.reason());
                None
            }
        }
    }

    /// The picked point follows the pointer.
    fn auto_drag(
        &mut self,
        ctx: &egui::Context,
        drag: Picked,
        p: Pos2,
        canvas: Rect,
        free: bool,
        painter: &egui::Painter,
    ) {
        let Some((target, frame)) = self.arrangement.lane_frame(
            ctx,
            canvas,
            self.document.project(),
            self.lanes.pending().len(),
            self.nav.view,
            drag.track,
        ) else {
            return;
        };
        if target != drag.target {
            return;
        }
        let at = self.snapped(frame.ticks_at(p.x), free);
        let value = frame.value_at(target, p.y);
        if let Some(lane) = self.lane_points_of(drag.track, target)
            && let Some(next) = moved(&lane, drag.index, at, value)
        {
            let _ = self.document.set_lane(drag.track, target, next, true);
        }
        draw_snap_guide(painter, frame.x(at), canvas);
        ctx.set_cursor_icon(CursorIcon::Grabbing);
    }

    // ---- the keys ----

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

    /// A point is picked and the keys mean it: not while the clip editor or
    /// the Devices tab holds the keyboard.
    pub(crate) fn auto_keys_picked(&self) -> bool {
        self.arrangement.auto.picked.is_some() && !self.note_keys() && !self.chain.live
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
        self.arrangement.auto.picked = None;
    }

    /// Takes the picked point out of its lane. The last one takes the lane
    /// with it, and the control is its own again.
    pub(crate) fn auto_delete(&mut self) {
        let Some(here) = self.arrangement.auto.picked.take() else {
            return;
        };
        if let Some(lane) = self.lane_points_of(here.track, here.target) {
            let next = removed(&lane, here.index);
            let result = self.document.set_lane(here.track, here.target, next, false);
            self.arrangement.note_lane(result);
        }
    }

    /// Moves the picked point: `time` grid steps (earlier is negative) and
    /// `value` steps of the lane's height (up is positive). Fine steps for
    /// the value are a quarter the size.
    pub(crate) fn auto_nudge(&mut self, time: i32, value: i32, fine: bool) {
        let Some(here) = self.arrangement.auto.picked else {
            return;
        };
        let Some(lane) = self.lane_points_of(here.track, here.target) else {
            return;
        };
        let Some(point) = lane.points().get(here.index).copied() else {
            return;
        };
        let mut at = point.at;
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
        let mut v = point.value;
        if value != 0 {
            if here.target == Target::Mute {
                v = if value > 0 { 1.0 } else { 0.0 };
            } else {
                let step = if fine { VALUE_STEP_FINE } else { VALUE_STEP };
                let f = scale::fraction(here.target, v) + value as f32 * step;
                v = scale::value(here.target, f);
            }
        }
        if let Some(next) = moved(&lane, here.index, at, v) {
            // One undo step for each press: not a drag.
            let result = self.document.set_lane(here.track, here.target, next, false);
            self.arrangement.note_lane(result);
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
