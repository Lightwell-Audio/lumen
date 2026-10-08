//! Clip editing on the lanes (M3 slice 6), in the owner's split-clip
//! model: a clip's top half moves it (Ctrl+drag copies), its bottom half
//! selects a time range across tracks, and a drag in empty lanes draws a
//! marquee. Clicks select (Ctrl toggles, Shift adds). Everything snaps to
//! the grid (Alt places freely), and every drag is one undo step.

use std::collections::BTreeSet;

use eframe::egui::{self, CursorIcon, Modifiers, Pos2, Rect, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::grid::draw_snap_guide;
use crate::arrangement::Arrangement;
use crate::arrangement::select::{Gesture, Half, TimeRange, drag_moves};
use crate::{App, ruler, widgets};

impl App {
    /// Clip selection, moving, copying, marquees and time ranges, from the
    /// lanes' `response` (click and drag). Draws the snap guide and the
    /// hint; the selection itself is drawn with the lanes.
    pub(crate) fn clip_editing(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        canvas: Rect,
        painter: &egui::Painter,
    ) {
        let ctx = ui.ctx().clone();
        let modifiers = ui.input(|i| i.modifiers);
        let adding = modifiers.shift || modifiers.command;

        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
        {
            self.arrangement.gesture = self.start_gesture(&ctx, origin, canvas, modifiers);
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(p) = response.interact_pointer_pos()
        {
            self.continue_gesture(&ctx, p, canvas, modifiers.alt, painter);
        }
        if response.drag_stopped() {
            if let Some(Gesture::Move {
                copies: Some(ids), ..
            }) = &self.arrangement.gesture
            {
                // The copies are what was dropped: they stay selected.
                self.arrangement.clips = ids.iter().copied().collect();
            }
            self.arrangement.gesture = None;
            self.arrangement.marquee = None;
        }

        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let hit = self.hit(&ctx, p, canvas);
            let clips = &mut self.arrangement.clips;
            match hit {
                Some(hit) if modifiers.command => {
                    if !clips.remove(&hit.clip) {
                        clips.insert(hit.clip);
                    }
                }
                Some(hit) if modifiers.shift => {
                    clips.insert(hit.clip);
                }
                Some(hit) => *clips = BTreeSet::from([hit.clip]),
                None if !adding => clips.clear(),
                None => {}
            }
            self.arrangement.range = None;
            self.selected_clips_now();
        }

        // The H1 hover: which half is under the pointer, and what it does.
        self.arrangement.hover = None;
        if self.arrangement.gesture.is_none()
            && let Some(p) = response.hover_pos()
            && let Some(hit) = self.hit(&ctx, p, canvas)
        {
            self.arrangement.hover = Some((hit.clip, hit.half));
            let locked = self
                .document
                .project()
                .tracks
                .get(hit.track)
                .and_then(|t| t.clips.iter().find(|c| c.id == hit.clip))
                .is_some_and(|c| c.locked);
            use widgets::HintMark;
            let (cursor, mark, text) = match hit.half {
                Half::Top if locked => (CursorIcon::NotAllowed, HintMark::None, "locked"),
                Half::Top if modifiers.command => {
                    (CursorIcon::Copy, HintMark::Move, "drag to copy")
                }
                Half::Top => (
                    CursorIcon::Grab,
                    HintMark::Move,
                    "drag to move · Ctrl copies",
                ),
                Half::Bottom => (CursorIcon::Text, HintMark::Resize, "drag to select time"),
            };
            ctx.set_cursor_icon(cursor);
            let row = match hit.half {
                Half::Top => hit.rect.top() + 7.0,
                Half::Bottom => hit.rect.bottom() - 7.0,
            };
            // A chip, not a bare caption: clips are busy underneath.
            widgets::draw_hint(painter, pos2(p.x + 14.0, row), mark, text, canvas);
            self.hint = Some(text.to_owned());
        }
    }

    fn hit(
        &self,
        ctx: &egui::Context,
        p: Pos2,
        canvas: Rect,
    ) -> Option<crate::arrangement::select::ClipHit> {
        self.arrangement.clip_at(
            ctx,
            p,
            canvas,
            self.document.project(),
            self.lanes.pending().len(),
            self.nav.view,
        )
    }

    fn ticks_at(&self, canvas: Rect, x: f32) -> Ticks {
        ruler::ticks_at(self.nav.view.beat_at(canvas.left(), x))
    }

    /// What a drag starting at `origin` does.
    fn start_gesture(
        &mut self,
        ctx: &egui::Context,
        origin: Pos2,
        canvas: Rect,
        modifiers: Modifiers,
    ) -> Option<Gesture> {
        let adding = modifiers.shift || modifiers.command;
        let Some(hit) = self.hit(ctx, origin, canvas) else {
            let kept = if adding {
                self.arrangement.clips.clone()
            } else {
                BTreeSet::new()
            };
            self.arrangement.range = None;
            return Some(Gesture::Marquee { from: origin, kept });
        };
        if hit.half == Half::Bottom {
            self.arrangement.clips.clear();
            return Some(Gesture::Range {
                at: self.ticks_at(canvas, origin.x),
                track: hit.track,
            });
        }
        if !self.arrangement.clips.contains(&hit.clip) {
            if !adding {
                self.arrangement.clips.clear();
            }
            self.arrangement.clips.insert(hit.clip);
        }
        self.arrangement.range = None;
        self.selected_clips_now();
        let grabbed = Arrangement::grab(self.document.project(), &self.arrangement.clips);
        if !grabbed.iter().any(|g| g.clip == hit.clip) {
            // The clip under the pointer is locked.
            return None;
        }
        let copies = modifiers
            .command
            .then(|| grabbed.iter().map(|_| self.document.fresh_id()).collect());
        Some(Gesture::Move {
            grabbed,
            anchor: hit.clip,
            at: self.ticks_at(canvas, origin.x),
            track: hit.track,
            copies,
        })
    }

    fn continue_gesture(
        &mut self,
        ctx: &egui::Context,
        p: Pos2,
        canvas: Rect,
        free: bool,
        painter: &egui::Painter,
    ) {
        let Some(gesture) = self.arrangement.gesture.clone() else {
            return;
        };
        let pending = self.lanes.pending().len();
        let project = self.document.project();
        let row = self
            .arrangement
            .track_index_at(ctx, project, pending, canvas, p.y);
        let x = |t: Ticks| self.nav.view.x(canvas.left(), t.as_quarters());
        let guides = Rect::from_x_y_ranges(canvas.x_range(), canvas.y_range());
        match gesture {
            Gesture::Move {
                grabbed,
                anchor,
                at,
                track,
                copies,
            } => {
                let delta = self.ticks_at(canvas, p.x) - at;
                let down = row.map_or(0, |r| r as isize - track as isize);
                let moves = drag_moves(project, &grabbed, anchor, delta, down, |t| {
                    self.snapped(t, free)
                });
                let landed = moves.iter().find(|m| m.clip == anchor).map(|m| m.start);
                match &copies {
                    Some(ids) => self.document.copy_clips(&moves, ids, Some("clip copy")),
                    None => self.document.move_clips(&moves, Some("clip drag")),
                }
                if let Some(start) = landed {
                    draw_snap_guide(painter, x(start), guides);
                }
                ctx.set_cursor_icon(if copies.is_some() {
                    CursorIcon::Copy
                } else {
                    CursorIcon::Grabbing
                });
            }
            Gesture::Marquee { from, kept } => {
                let area = Rect::from_two_pos(from, p);
                self.arrangement.marquee = Some(area);
                let mut clips = kept;
                clips.extend(self.arrangement.clips_in(
                    ctx,
                    area,
                    canvas,
                    project,
                    pending,
                    self.nav.view,
                ));
                self.arrangement.clips = clips;
                if !self.arrangement.clips.is_empty() {
                    self.selected_clips_now();
                }
            }
            Gesture::Range { at, track } => {
                let (a, b) = (
                    self.snapped(at, free),
                    self.snapped(self.ticks_at(canvas, p.x), free),
                );
                self.arrangement.range =
                    TimeRange::between(project, a, b, track, row.unwrap_or(track));
                draw_snap_guide(painter, x(b), guides);
                ctx.set_cursor_icon(CursorIcon::Text);
                self.arrangement.selected.clear();
                self.locators.selected = None;
            }
        }
    }

    /// Clips (or a range) were just selected: tracks and locators are not.
    fn selected_clips_now(&mut self) {
        if !self.arrangement.clips.is_empty() {
            self.arrangement.selected.clear();
            self.locators.selected = None;
        }
    }

    /// Delete removes the selected clips; Esc clears the clip selection and
    /// the range. Runs before the track keys, so selected clips take them.
    pub(crate) fn clip_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // Selecting tracks deselects clips and the range.
        if !self.arrangement.selected.is_empty() {
            self.arrangement.clips.clear();
            self.arrangement.range = None;
        }
        let pressed = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        if !self.arrangement.clips.is_empty()
            && (pressed(egui::Key::Delete) || pressed(egui::Key::Backspace))
        {
            let ids: Vec<Id> = self.arrangement.clips.iter().copied().collect();
            self.document.delete_clips(&ids);
            self.arrangement.clips.clear();
        } else if (!self.arrangement.clips.is_empty() || self.arrangement.range.is_some())
            && pressed(egui::Key::Escape)
        {
            self.arrangement.clips.clear();
            self.arrangement.range = None;
        }
    }
}
