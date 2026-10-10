//! Clip editing on the lanes (M3 slice 6), in the owner's split-clip
//! model: a clip's top half moves it (Ctrl+drag copies), its bottom half
//! selects a time range across tracks, and a drag in empty lanes draws a
//! marquee. Clicks select (Ctrl toggles, Shift adds). Everything snaps to
//! the grid (Alt places freely), and every drag is one undo step.

use std::collections::BTreeSet;

use eframe::egui::{self, CursorIcon, Modifiers, Pos2, Rect};
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::grid::draw_snap_guide;
use super::info::{Held, Info, Mouse};
use super::navigate::ViewStep;
use crate::arrangement::Arrangement;
use crate::arrangement::select::{
    ClipStep, Edge, Gesture, Half, Hover, TimeRange, clip_step, drag_moves,
};
use crate::commands::Command;
use lumen_project::edit;

use crate::{App, ruler, widgets};

/// The editing commands on clips and time ranges (slice 6b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edit {
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    DeleteTime,
    InsertTime,
}

impl Edit {
    /// The command that runs this edit, for its name and shortcut.
    pub(crate) fn command(self) -> Command {
        match self {
            Edit::Cut => Command::Cut,
            Edit::Copy => Command::Copy,
            Edit::Paste => Command::Paste,
            Edit::Duplicate => Command::Duplicate,
            Edit::Delete => Command::Delete,
            Edit::DeleteTime => Command::DeleteTime,
            Edit::InsertTime => Command::InsertTime,
        }
    }
}

/// What the lanes' right-click menu is for: what was right-clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MenuFor {
    Clips,
    Range,
    #[default]
    Lanes,
}

/// Where the earliest of `clips` starts.
fn clips_start(project: &lumen_project::model::Project, clips: &[Id]) -> Option<Ticks> {
    project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter(|c| clips.contains(&c.id))
        .map(|c| c.start)
        .min()
}

impl App {
    /// Moves the clip selection with an arrow key (M3 review H2): from the
    /// clip the keys last chose, while it is selected, else the first
    /// selected clip; `extend` adds the clip reached instead of selecting
    /// it alone. With nothing further that way, the clip it starts from is
    /// selected alone (so Ctrl+A then an arrow reaches one clip). The view
    /// follows it.
    pub(crate) fn step_clips(&mut self, step: ClipStep, extend: bool) {
        let chosen = &self.arrangement.clips;
        let Some(from) = self
            .arrangement
            .clip_cursor
            .filter(|id| chosen.contains(id))
            .or_else(|| self.first_selected_clip())
        else {
            return;
        };
        let to = match clip_step(self.document.project(), from, step) {
            Some(to) => to,
            None if !extend => from,
            None => return,
        };
        if !extend {
            self.arrangement.clips.clear();
        }
        self.arrangement.range = None;
        self.arrangement.clips.insert(to);
        self.arrangement.clip_cursor = Some(to);
        self.nav.steps.push(ViewStep::ShowClip(to));
    }

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
            // A drag shows its own hover (a trim rail, a handle), if any.
            self.arrangement.hover = None;
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

        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos()
            && let Some(hit) = self.hit(&ctx, p, canvas)
        {
            self.reset_handle(&hit);
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
            // A plain click anywhere on the lanes also moves the playhead
            // there (snapped; Alt places freely), as in Ableton (owner,
            // MT-APP-14 run 2). Ctrl and Shift clicks only build a
            // selection.
            if !adding {
                let target = self.snapped(self.ticks_at(canvas, p.x), modifiers.alt);
                self.transport.locate(target.max(Ticks::ZERO));
                self.transport.follow_paused = true;
            }
        }

        // A right-click decides what the menu is for (the owner's A, after
        // MT-APP-14 run 2): inside the time range, the range; on a clip,
        // the clips (a clip that is not selected is selected first, so the
        // menu acts on what was clicked); elsewhere, the lanes.
        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.menu_for = if self.in_range(&ctx, canvas, p) {
                MenuFor::Range
            } else if let Some(hit) = self.hit(&ctx, p, canvas) {
                if !self.arrangement.clips.contains(&hit.clip) {
                    self.arrangement.clips = BTreeSet::from([hit.clip]);
                    self.arrangement.range = None;
                    self.selected_clips_now();
                }
                MenuFor::Clips
            } else {
                MenuFor::Lanes
            };
        }

        // The H1 hover: which half is under the pointer, and what it does.
        // Not while a menu is open: the hint would sit on it.
        if self.arrangement.gesture.is_some() || egui::Popup::is_any_open(&ctx) {
            return;
        }
        self.arrangement.hover = None;
        let Some(p) = response.hover_pos() else {
            return;
        };
        let Some(hit) = self.hit(&ctx, p, canvas) else {
            // Empty lanes (M4 slice 0: what a drag does is in the info line,
            // not by the pointer).
            self.set_info(
                Info::new("Lanes")
                    .on(Held::NONE, Mouse::Drag, "select clips")
                    .on(Held::NONE, Mouse::Click, "move the playhead")
                    .on(Held::NONE, Mouse::RightClick, "menu")
                    .key(Command::Paste, "paste"),
            );
            return;
        };
        self.arrangement.hover = Some(Hover::from(&hit));
        if let Some(info) = self.handle_hover(&ctx, &hit) {
            self.set_info(info);
            return;
        }
        let Some(clip) = self
            .document
            .project()
            .tracks
            .get(hit.track)
            .and_then(|t| t.clips.iter().find(|c| c.id == hit.clip))
        else {
            return;
        };
        let name = format!("\u{201C}{}\u{201D}", clip.name);
        let slip = modifiers.command && modifiers.alt;
        // The cursor shows what a drag does now; the info line says it all.
        let (cursor, info) = match (hit.edge, hit.half) {
            _ if clip.locked => (
                CursorIcon::NotAllowed,
                Info::new(format!("Locked clip {name}")).on(
                    Held::NONE,
                    Mouse::RightClick,
                    "unlock it in the menu",
                ),
            ),
            (Some(Edge::End), _) => (
                if modifiers.shift {
                    CursorIcon::ResizeEast
                } else {
                    CursorIcon::ResizeHorizontal
                },
                Info::new(format!("End of {name}"))
                    .on(Held::NONE, Mouse::Drag, "trim")
                    .on(Held::SHIFT, Mouse::Drag, "loop"),
            ),
            (Some(Edge::Start), _) => (
                CursorIcon::ResizeHorizontal,
                Info::new(format!("Start of {name}")).on(Held::NONE, Mouse::Drag, "trim"),
            ),
            (None, Half::Top) => (
                if slip {
                    CursorIcon::ResizeColumn
                } else if modifiers.command {
                    CursorIcon::Copy
                } else {
                    CursorIcon::Grab
                },
                Info::new(format!("Clip {name}"))
                    .on(Held::NONE, Mouse::Drag, "move")
                    .on(Held::CTRL, Mouse::Drag, "copy")
                    .on(Held::CTRL_ALT, Mouse::Drag, "slip the audio")
                    .key(Command::Duplicate, "duplicate")
                    .key(Command::MuteClips, "mute")
                    .key(Command::ClipProperties, "properties"),
            ),
            (None, Half::Bottom) => (
                CursorIcon::Text,
                Info::new(format!("Clip {name}, lower half"))
                    .on(Held::NONE, Mouse::Drag, "select time")
                    .key(Command::LoopSelection, "loop the selection"),
            ),
        };
        ctx.set_cursor_icon(cursor);
        self.set_info(info);
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

    /// Whether `p` is inside the time range.
    fn in_range(&self, ctx: &egui::Context, canvas: Rect, p: Pos2) -> bool {
        let Some(range) = &self.arrangement.range else {
            return false;
        };
        let at = self.ticks_at(canvas, p.x);
        let project = self.document.project();
        let track = self
            .arrangement
            .track_index_at(ctx, project, self.lanes.pending().len(), canvas, p.y)
            .and_then(|i| project.tracks.get(i))
            .map(|t| t.id);
        (range.start..range.end).contains(&at) && track.is_some_and(|t| range.tracks.contains(&t))
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
        if hit.handle.is_some() {
            return self.start_handle_gesture(&hit, origin, canvas);
        }
        let locked = self
            .document
            .project()
            .tracks
            .get(hit.track)
            .and_then(|t| t.clips.iter().find(|c| c.id == hit.clip))
            .is_none_or(|c| c.locked);
        if let Some(edge) = hit.edge {
            if locked {
                return None;
            }
            self.arrangement.clips = BTreeSet::from([hit.clip]);
            self.arrangement.range = None;
            self.selected_clips_now();
            return Some(Gesture::Trim {
                base: std::sync::Arc::new(self.document.project().clone()),
                clip: hit.clip,
                edge,
                looping: edge == Edge::End && modifiers.shift,
            });
        }
        if hit.half == Half::Top && modifiers.command && modifiers.alt {
            if locked {
                return None;
            }
            self.arrangement.clips = BTreeSet::from([hit.clip]);
            self.arrangement.range = None;
            self.selected_clips_now();
            return Some(Gesture::Slip {
                base: std::sync::Arc::new(self.document.project().clone()),
                clip: hit.clip,
                at: self.ticks_at(canvas, origin.x),
            });
        }
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
            base: std::sync::Arc::new(self.document.project().clone()),
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
                base,
                grabbed,
                anchor,
                at,
                track,
                copies,
            } => {
                let delta = self.ticks_at(canvas, p.x) - at;
                let down = row.map_or(0, |r| r as isize - track as isize);
                let moves = drag_moves(&base, &grabbed, anchor, delta, down, |t| {
                    self.snapped(t, free)
                });
                let landed = moves.iter().find(|m| m.clip == anchor).map(|m| m.start);
                match &copies {
                    Some(ids) => self
                        .document
                        .copy_clips(&base, &moves, ids, Some("clip copy")),
                    None => self.document.move_clips(&base, &moves, Some("clip drag")),
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
            Gesture::Trim {
                base,
                clip,
                edge,
                looping,
            } => {
                let to = self.snapped(self.ticks_at(canvas, p.x), free);
                self.document
                    .trim_clip(&base, clip, edge, to, looping, Some("clip trim"));
                if let Some(c) = self
                    .document
                    .project()
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .find(|c| c.id == clip)
                {
                    let side = match edge {
                        Edge::Start => c.start,
                        Edge::End => c.start + c.length,
                    };
                    draw_snap_guide(painter, x(side), guides);
                }
                self.arrangement.hover = Some(Hover {
                    clip,
                    half: Half::Top,
                    edge: Some(edge),
                    handle: None,
                });
                ctx.set_cursor_icon(if looping {
                    CursorIcon::ResizeEast
                } else {
                    CursorIcon::ResizeHorizontal
                });
            }
            Gesture::Slip { base, clip, at } => {
                // Slipping is free: the audio follows the hand exactly.
                let by = self.ticks_at(canvas, p.x) - at;
                self.document.slip_clip(&base, clip, by, Some("clip slip"));
                ctx.set_cursor_icon(CursorIcon::ResizeColumn);
            }
            handle @ (Gesture::Fade { .. } | Gesture::Bend { .. } | Gesture::Gain { .. }) => {
                self.continue_handle_gesture(ctx, handle, p, canvas, painter);
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
    pub(super) fn selected_clips_now(&mut self) {
        if !self.arrangement.clips.is_empty() {
            self.arrangement.selected.clear();
            self.locators.selected = None;
        }
    }

    /// The lanes' right-click menu, for what was clicked (the owner's A):
    /// clips get the clip edits and properties, a time range its edits,
    /// empty lanes Paste and the grid. Commands that cannot run are left
    /// out rather than greyed.
    pub(crate) fn lanes_menu(&mut self, ui: &mut egui::Ui) {
        match self.menu_for {
            MenuFor::Clips => {
                use Edit::{Copy, Cut, Delete, Duplicate};
                self.edit_items(ui, &[Cut, Copy, Duplicate, Delete]);
                ui.separator();
                self.clip_menu(ui);
            }
            MenuFor::Range => {
                use Edit::{Copy, Cut, Delete, DeleteTime, Duplicate, InsertTime, Paste};
                self.edit_items(ui, &[Cut, Copy, Paste, Duplicate, Delete]);
                ui.separator();
                self.edit_items(ui, &[DeleteTime, InsertTime]);
            }
            MenuFor::Lanes => {
                if self.edit_items(ui, &[Edit::Paste]) {
                    ui.separator();
                }
                let label = format!("Grid: {}", self.grid_label());
                widgets::submenu(ui, label, |ui| self.grid_menu(ui));
                let keys = self.keymap.keys(Command::SnapToGrid);
                if widgets::menu_toggle(ui, "Snap to grid", keys.as_ref(), self.grid.snap).clicked()
                {
                    self.grid.snap = !self.grid.snap;
                }
            }
        }
    }

    /// Menu items for the `commands` that can run now; whether any could.
    fn edit_items(&mut self, ui: &mut egui::Ui, commands: &[Edit]) -> bool {
        let mut any = false;
        for &command in commands {
            if !self.can_edit(command) {
                continue;
            }
            any = true;
            let run = command.command();
            let keys = self.keymap.keys(run);
            if widgets::menu_item(ui, run.name(), keys.as_ref(), true).clicked() {
                self.edit(command);
            }
        }
        any
    }

    /// Whether `command` can run now, for menus.
    pub(crate) fn can_edit(&self, command: Edit) -> bool {
        let chosen = !self.arrangement.clips.is_empty() || self.arrangement.range.is_some();
        match command {
            Edit::Paste => self.clipboard.is_some(),
            Edit::DeleteTime | Edit::InsertTime => self.arrangement.range.is_some(),
            _ => chosen,
        }
    }

    /// Runs an editing command on the clip selection or the time range.
    pub(crate) fn edit(&mut self, command: Edit) {
        let project = self.document.project();
        let range = self.arrangement.range.clone();
        let clips: Vec<Id> = self.arrangement.clips.iter().copied().collect();
        let index_of = |id: Id| project.tracks.iter().position(|t| t.id == id);
        match command {
            Edit::Copy | Edit::Cut => {
                let copied = match &range {
                    Some(r) => edit::copy_range(project, &r.tracks, r.start, r.end),
                    None => edit::copy_clips(project, &clips),
                };
                if copied.is_none() {
                    return;
                }
                self.clipboard = copied;
                if command == Edit::Cut {
                    match &range {
                        Some(r) => self.document.clear_range("Cut", &r.tracks, r.start, r.end),
                        None => {
                            self.document.cut_clips(&clips);
                            self.arrangement.clips.clear();
                        }
                    }
                }
            }
            Edit::Paste => {
                let Some(clipboard) = self.clipboard.clone() else {
                    return;
                };
                // Into the range if there is one; otherwise at the playhead,
                // on the first selected track or where it was copied from.
                let (at, track) = match &range {
                    Some(r) => (r.start, r.tracks.first().and_then(|&t| index_of(t))),
                    None => (
                        self.transport.state().ticks,
                        self.arrangement.selected.first().and_then(|&t| index_of(t)),
                    ),
                };
                let track = track.unwrap_or(clipboard.from_track);
                self.paste(&clipboard, at, track, "Paste");
            }
            Edit::Duplicate => {
                let copied = match &range {
                    Some(r) => edit::copy_range(project, &r.tracks, r.start, r.end),
                    None => edit::copy_clips(project, &clips),
                };
                let Some(copied) = copied else {
                    return;
                };
                // Right after the selection, on the same tracks.
                let at = match &range {
                    Some(r) => r.end,
                    None => clips_start(project, &clips).unwrap_or_default() + copied.length,
                };
                self.paste(&copied, at, copied.from_track, "Duplicate");
            }
            Edit::Delete => match &range {
                Some(r) => self
                    .document
                    .clear_range("Delete", &r.tracks, r.start, r.end),
                None => {
                    self.document.delete_clips(&clips);
                    self.arrangement.clips.clear();
                }
            },
            Edit::DeleteTime => {
                if let Some(r) = &range {
                    self.document.delete_time(r.start, r.end);
                    self.arrangement.range = None;
                }
            }
            Edit::InsertTime => {
                if let Some(r) = &range {
                    self.document.insert_time(r.start, r.end - r.start);
                }
            }
        }
    }

    /// Pastes and selects what was pasted: the new clips, or, for a copied
    /// range, the same range where it landed.
    fn paste(&mut self, clipboard: &edit::Clipboard, at: Ticks, track: usize, label: &str) {
        let pasted = self.document.paste(label, clipboard, at, track);
        let project = self.document.project();
        if clipboard.range {
            self.arrangement.range = TimeRange::between(
                project,
                at,
                at + clipboard.length,
                track,
                (track + clipboard.tracks - 1).min(project.tracks.len().saturating_sub(1)),
            );
            self.arrangement.clips.clear();
        } else {
            self.arrangement.clips = pasted.into_iter().collect();
            self.arrangement.range = None;
        }
        self.selected_clips_now();
    }
}
