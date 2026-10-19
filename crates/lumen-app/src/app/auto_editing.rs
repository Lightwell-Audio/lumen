//! The pointer on automation lanes (M7 slices A1 and A2). A lane shown over
//! a track's clips takes the pointer there. What a press does depends on the
//! tool in hand (`arrangement/auto_state.rs`): the pointer picks, moves and
//! adds points (Shift picks several, or drags a box), the pencil draws
//! freehand, a shape tool draws a ramp, sine, triangle, saw or square. Ctrl
//! reaches the clip under the lane instead.
//!
//! The gestures themselves are `auto_gestures.rs` and the keys are
//! `auto_keys.rs`. Every edit is one undo step, and a drag is one step in
//! all.

use eframe::egui::{self, CursorIcon, Pos2, Rect};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::Id;

use super::editing::MenuFor;
use super::info::{Held, Info, Mouse};
use crate::App;
use crate::arrangement::auto_scale as scale;
use crate::arrangement::auto_state::{Picked, Picks, Tool};
use crate::arrangement::auto_view::Frame;
use crate::commands::Command;

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
        // A gesture that began on a lane goes on wherever the pointer is.
        if let Some(active) = self.arrangement.auto.active.as_ref() {
            let (track, target) = (active.track, active.target);
            let down = ui.input(|i| i.pointer.any_down());
            if down
                && response.dragged_by(egui::PointerButton::Primary)
                && let Some(p) = response.interact_pointer_pos()
                && let Some((now, frame)) = self.arrangement.lane_frame(
                    &ctx,
                    canvas,
                    self.document.project(),
                    self.lanes.pending().len(),
                    self.nav.view,
                    track,
                )
                && now == target
            {
                self.auto_update(Some((painter, canvas)), p, &frame, modifiers);
            }
            if response.drag_stopped() || !down {
                self.auto_finish();
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
            self.auto_begin(track, target, &frame, origin, modifiers);
            return true;
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            if !command && let Some((track, target, frame)) = self.lane_hit(&ctx, canvas, p) {
                self.auto_click(track, target, &frame, p, modifiers);
                return true;
            }
            // A click anywhere else lets go of the points.
            self.arrangement.auto.picks = None;
            self.arrangement.auto.focus = false;
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
        let tool = self.arrangement.auto.tool;
        let near = lane
            .as_ref()
            .and_then(|l| frame.nearest(target, l, p).map(|i| (i, l.points()[i])));
        let project = self.document.project();
        let name = scale::name(project, target);
        let bend = self
            .arrangement
            .auto
            .picked_on(track, target)
            .and_then(Picks::single)
            .zip(lane.as_ref())
            .and_then(|(one, l)| frame.bend_dot(target, l, one.index))
            .is_some_and(|dot| dot.distance(p) <= crate::theme::size::AUTO_GRAB);
        let (cursor, info) = match (tool, near) {
            _ if bend => (
                CursorIcon::Grab,
                Info::new(format!("{name} segment"))
                    .on(Held::NONE, Mouse::Drag, "bend")
                    .on(Held::ALT, Mouse::Drag, "add an S shape"),
            ),
            (Tool::Pointer, Some((index, point))) => {
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
                    .on(Held::SHIFT, Mouse::Click, "add to the picks")
                    .key(Command::Delete, "delete"),
                )
            }
            (Tool::Pointer, None) => (
                CursorIcon::Crosshair,
                Info::new(format!("{name} lane"))
                    .on(Held::NONE, Mouse::Click, "add a point")
                    .on(Held::NONE, Mouse::Drag, "add a point and move it")
                    .on(Held::SHIFT, Mouse::Drag, "pick the points in a box")
                    .on(Held::ALT, Mouse::Click, "add it off the grid")
                    .on(Held::CTRL, Mouse::Click, "reach the clips under the lane")
                    .key(Command::AutomationShow, "hide lanes"),
            ),
            (Tool::Pencil, _) => (
                CursorIcon::Crosshair,
                Info::new(format!("{name} lane \u{00B7} pencil"))
                    .on(Held::NONE, Mouse::Drag, "draw")
                    .on(Held::CTRL, Mouse::Click, "reach the clips under the lane")
                    .key(Command::DrawTool, "back to the pointer"),
            ),
            (Tool::Shape(wave), _) => (
                CursorIcon::Crosshair,
                Info::new(format!(
                    "{name} lane \u{00B7} {}",
                    crate::arrangement::auto_state::wave_name(wave).to_lowercase()
                ))
                .on(Held::NONE, Mouse::Drag, "draw it across the span")
                .on(Held::ALT, Mouse::Drag, "off the grid")
                .on(Held::CTRL, Mouse::Click, "reach the clips under the lane"),
            ),
        };
        ctx.set_cursor_icon(cursor);
        self.set_info(info);
    }

    /// A click on a lane: picks the point under it, or adds one. Shift adds
    /// a point to the picks or takes it from them.
    fn auto_click(
        &mut self,
        track: Id,
        target: Target,
        frame: &Frame,
        p: Pos2,
        modifiers: egui::Modifiers,
    ) {
        self.arrangement.auto.focus = true;
        let lane = self.lane_points_of(track, target);
        if let Some(index) = lane.as_ref().and_then(|l| frame.nearest(target, l, p)) {
            let here = Picked {
                track,
                target,
                index,
            };
            if modifiers.shift {
                self.auto_toggle_pick(here);
            } else {
                self.arrangement.auto.picks = Some(Picks::only(here));
            }
            return;
        }
        if modifiers.shift {
            return;
        }
        let at = self.snapped(frame.ticks_at(p.x), modifiers.alt);
        let reach = crate::theme::size::AUTO_SNAP / frame.area().height().max(1.0);
        let height = if modifiers.alt {
            frame.height_at(p.y)
        } else {
            scale::snap_height(target, frame.height_at(p.y), reach).0
        };
        self.add_auto_point(track, target, at, scale::value(target, height), false);
    }
}
