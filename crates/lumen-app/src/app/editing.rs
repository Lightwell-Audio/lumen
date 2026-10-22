//! Clip editing on the lanes: a clip's header moves it (Ctrl+drag copies), its body
//! selects a time range, and a drag in empty lanes selects a time range on
//! the tracks it spans (ADR-0049; Ctrl+Shift+drag draws a clip marquee
//! instead). A range and the track selection travel together. Clicks select
//! (Ctrl toggles, Shift adds). Everything snaps to the grid (Alt places
//! freely), and every drag is one undo step.

use std::collections::BTreeSet;

use eframe::egui::{self, CursorIcon, Modifiers, Pos2, Rect};
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::grid::draw_snap_guide;
use super::info::{Held, Info, Mouse};
use super::navigate::ViewStep;
use crate::arrangement::Arrangement;
use crate::arrangement::select::{
    ClipRegion, ClipStep, Edge, EmptyLaneDrag, Gesture, Hover, TimeRange, clip_step, drag_moves,
    empty_lane_drag,
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
            self.arrangement.gesture = if self.arrangement.tool == super::tools::ArrangementTool::Split {
                let at = self.snapped(self.ticks_at(canvas, origin.x), modifiers.alt);
                Some(Gesture::Split { from: at, to: at })
            } else {
                self.start_gesture(&ctx, origin, canvas, modifiers)
            };
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(p) = response.interact_pointer_pos()
        {
            if let Some(Gesture::Split { from, .. }) = self.arrangement.gesture {
                let to = self.snapped(self.ticks_at(canvas, p.x), modifiers.alt);
                self.arrangement.gesture = Some(Gesture::Split { from, to });
                draw_snap_guide(painter, self.nav.view.x(canvas.left(), to.as_quarters()), canvas);
                ctx.set_cursor_icon(CursorIcon::Crosshair);
            } else {
                self.continue_gesture(&ctx, p, canvas, modifiers.alt, painter);
            }
        }
        if response.drag_stopped() {
            if let Some(Gesture::Split { to, .. }) = self.arrangement.gesture {
                self.split_at(to);
            }
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

        // A double-click resets a handle; on a MIDI clip it opens the clip
        // editor; on an empty spot of a MIDI track's lane it makes a
        // one-bar clip there (snapped) and opens it (M5 slice 3c).
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            match self.hit(&ctx, p, canvas) {
                Some(hit) if hit.handle.is_some() => self.reset_handle(&hit),
                Some(hit) => self.open_note_editor(hit.clip),
                None => {
                    if let Some(track) = self.lane_track(&ctx, canvas, p) {
                        let at = self.snapped(self.ticks_at(canvas, p.x), modifiers.alt);
                        self.put_midi_clip(track, crate::document::NewClip::Bar(at));
                    }
                }
            }
        }

        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            if self.arrangement.tool == super::tools::ArrangementTool::Split {
                self.split_at(self.snapped(self.ticks_at(canvas, p.x), modifiers.alt));
                return;
            }
            let hit = self.hit(&ctx, p, canvas);
            // A click on a clip's header (its title strip) locates to the
            // clip's start rather than to the clicked spot.
            let header_start = hit
                .filter(|h| h.region == ClipRegion::Header && h.handle.is_none())
                .and_then(|h| {
                    let track = self.document.project().tracks.get(h.track)?;
                    track.clips.iter().find(|c| c.id == h.clip).map(|c| c.start)
                });
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
            // While stopped, a plain lane click also locates (snapped;
            // Alt places freely), and a click on a clip's header locates to
            // the clip's start. While playing, selecting or editing clips
            // must leave playback running from its current place. Ctrl and
            // Shift clicks only build a selection.
            if !adding {
                if !self.transport.state().playing {
                    let target = header_start
                        .unwrap_or_else(|| self.snapped(self.ticks_at(canvas, p.x), modifiers.alt));
                    self.transport.locate(target.max(Ticks::ZERO));
                    self.transport.follow_paused = true;
                }
                // The clicked track is where Paste lands now; the headers'
                // selection gives way to it.
                self.arrangement.insert = self.lane_track(&ctx, canvas, p);
                self.arrangement.selected.clear();
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
                let at = self.snapped(self.ticks_at(canvas, p.x), modifiers.alt);
                self.arrangement.menu_spot =
                    Some((at.max(Ticks::ZERO), self.lane_track(&ctx, canvas, p)));
                MenuFor::Lanes
            };
        }

        // The H1 hover: which region is under the pointer, and what it does.
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
                    .on(Held::NONE, Mouse::Drag, "select time")
                    .on(Held::CTRL_SHIFT, Mouse::Drag, "select clips")
                    .on(
                        Held::NONE,
                        Mouse::Click,
                        if self.transport.state().playing {
                            "clear selection"
                        } else {
                            "move the playhead"
                        },
                    )
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
        let (cursor, info) = match (hit.edge, hit.region) {
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
            (None, ClipRegion::Header) => (
                if slip {
                    CursorIcon::ResizeColumn
                } else if modifiers.command {
                    CursorIcon::Copy
                } else {
                    CursorIcon::Grab
                },
                Info::new(format!("Clip {name}"))
                    .on(Held::NONE, Mouse::Drag, "move")
                    .on(Held::NONE, Mouse::Click, "go to its start")
                    .on(Held::CTRL, Mouse::Drag, "copy")
                    .on(Held::CTRL_ALT, Mouse::Drag, "slip the audio")
                    .key(Command::Duplicate, "duplicate")
                    .key(Command::MuteClips, "mute")
                    .key(Command::ClipProperties, "properties"),
            ),
            (None, ClipRegion::Body) => (
                CursorIcon::Text,
                Info::new(format!("Clip {name}, body"))
                    .on(Held::NONE, Mouse::Drag, "select time")
                    .on(Held::CTRL_SHIFT, Mouse::Drag, "select clips")
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

    /// The track whose lane is at `p`, if any.
    fn lane_track(&self, ctx: &egui::Context, canvas: Rect, p: Pos2) -> Option<Id> {
        let project = self.document.project();
        let pending = self.lanes.pending().len();
        self.arrangement
            .track_at(ctx, project, pending, p.y - canvas.top())
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
            return self.empty_lane_gesture(ctx, origin, canvas, modifiers);
        };
        if hit.handle.is_some() {
            return self.start_handle_gesture(&hit, origin, canvas);
        }
        let grabbed = self
            .document
            .project()
            .tracks
            .get(hit.track)
            .and_then(|t| t.clips.iter().find(|c| c.id == hit.clip));
        let locked = grabbed.is_none_or(|c| c.locked);
        if let Some(edge) = hit.edge {
            let clip = grabbed.filter(|c| !c.locked)?;
            let side = match edge {
                Edge::Start => clip.start,
                Edge::End => clip.start + clip.length,
            };
            let grab = side - self.ticks_at(canvas, origin.x);
            self.arrangement.clips = BTreeSet::from([hit.clip]);
            self.arrangement.range = None;
            self.selected_clips_now();
            return Some(Gesture::Trim {
                base: std::sync::Arc::new(self.document.project().clone()),
                clip: hit.clip,
                edge,
                looping: edge == Edge::End && modifiers.shift,
                grab,
            });
        }
        if hit.region == ClipRegion::Header && modifiers.command && modifiers.alt {
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
        if hit.region == ClipRegion::Body {
            return self.empty_lane_gesture(ctx, origin, canvas, modifiers);
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

    /// What a drag from empty lane space does (ADR-0049): a time range from
    /// the press down to the pointer's track, or, with Ctrl+Shift, a marquee
    /// that keeps the clips already selected when Shift or Ctrl is held.
    fn empty_lane_gesture(
        &mut self,
        ctx: &egui::Context,
        origin: Pos2,
        canvas: Rect,
        modifiers: Modifiers,
    ) -> Option<Gesture> {
        let pending = self.lanes.pending().len();
        let track = self.arrangement.track_index_at(
            ctx,
            self.document.project(),
            pending,
            canvas,
            origin.y,
        );
        if let (EmptyLaneDrag::Range, Some(track)) = (empty_lane_drag(modifiers), track) {
            self.arrangement.clips.clear();
            self.arrangement.range = None;
            return Some(Gesture::Range {
                at: self.ticks_at(canvas, origin.x),
                track,
            });
        }
        let kept = if modifiers.shift || modifiers.command {
            self.arrangement.clips.clone()
        } else {
            BTreeSet::new()
        };
        self.arrangement.range = None;
        Some(Gesture::Marquee { from: origin, kept })
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
            Gesture::Split { from, .. } => {
                let to = self.snapped(self.ticks_at(canvas, p.x), free);
                self.arrangement.gesture = Some(Gesture::Split { from, to });
            }
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
                grab,
            } => {
                // The edge keeps its distance from the pointer: picked up a
                // few pixels inside, it does not jump to the pointer.
                let to = self.snapped(self.ticks_at(canvas, p.x) + grab, free);
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
                    region: ClipRegion::Header,
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
                let range = TimeRange::between(project, a, b, track, row.unwrap_or(track));
                // The range's tracks are the selected ones (ADR-0049).
                self.arrangement.selected =
                    range.as_ref().map(|r| r.tracks.clone()).unwrap_or_default();
                self.arrangement.range = range;
                draw_snap_guide(painter, x(b), guides);
                ctx.set_cursor_icon(CursorIcon::Text);
                self.locators.selected = None;
            }
        }
    }

    /// Splits selected clips at `at`, or clips in the selected range at both edges.
    pub(crate) fn split_at(&mut self, at: Ticks) {
        let (clips, points) = if let Some(range) = &self.arrangement.range {
            let clips = self.document.project().tracks.iter()
                .filter(|t| range.tracks.contains(&t.id))
                .flat_map(|t| t.clips.iter())
                .filter(|c| c.start < range.end && c.start + c.length > range.start)
                .map(|c| c.id).collect::<Vec<_>>();
            (clips, vec![range.start, range.end])
        } else {
            (self.arrangement.clips.iter().copied().collect(), vec![at])
        };
        if clips.is_empty() { return; }
        self.arrangement.clips = self.document.split_clips(&clips, &points).into_iter().collect();
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
                if self.can_edit(Edit::Paste) {
                    // Where the lanes were right-clicked, not the playhead.
                    let run = Edit::Paste.command();
                    let keys = self.keymap.keys(run);
                    if widgets::menu_item(ui, run.name(), keys.as_ref(), true).clicked()
                        && let Some(clipboard) = self.clipboard.clone()
                    {
                        let project = self.document.project();
                        let (at, track) = self
                            .arrangement
                            .menu_spot
                            .unwrap_or((self.transport.position(), None));
                        let track = track
                            .and_then(|t| project.tracks.iter().position(|x| x.id == t))
                            .unwrap_or(clipboard.from_track);
                        self.paste(&clipboard, at, track, "Paste");
                    }
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
                // on the first selected track, the track last clicked in the
                // lanes, or where it was copied from.
                let (at, track) = match &range {
                    Some(r) => (r.start, r.tracks.first().and_then(|&t| index_of(t))),
                    None => (
                        self.transport.position(),
                        self.arrangement
                            .selected
                            .first()
                            .copied()
                            .or(self.arrangement.insert)
                            .and_then(index_of),
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
            // The pasted range is selected on the tracks it landed on, as a
            // range drag selects its tracks (ADR-0049).
            let landed = edit::landing_tracks(project, clipboard, track);
            let range = TimeRange::over(project, at, at + clipboard.length, &landed);
            self.arrangement.selected =
                range.as_ref().map(|r| r.tracks.clone()).unwrap_or_default();
            self.arrangement.range = range;
            self.arrangement.clips.clear();
        } else {
            self.arrangement.clips = pasted.into_iter().collect();
            self.arrangement.range = None;
        }
        self.selected_clips_now();
    }
}
