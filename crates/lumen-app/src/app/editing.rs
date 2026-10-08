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
use crate::arrangement::select::{Edge, Gesture, Half, Hover, TimeRange, drag_moves};
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
    pub(crate) const ALL: [Edit; 7] = [
        Edit::Cut,
        Edit::Copy,
        Edit::Paste,
        Edit::Duplicate,
        Edit::Delete,
        Edit::DeleteTime,
        Edit::InsertTime,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Edit::Cut => "Cut",
            Edit::Copy => "Copy",
            Edit::Paste => "Paste",
            Edit::Duplicate => "Duplicate",
            Edit::Delete => "Delete",
            Edit::DeleteTime => "Delete time",
            Edit::InsertTime => "Insert time",
        }
    }

    pub(crate) fn keys(self) -> &'static str {
        match self {
            Edit::Cut => "Ctrl+X",
            Edit::Copy => "Ctrl+C",
            Edit::Paste => "Ctrl+V",
            Edit::Duplicate => "Ctrl+D",
            Edit::Delete => "Delete",
            Edit::DeleteTime => "Ctrl+Shift+Delete",
            Edit::InsertTime => "Ctrl+I",
        }
    }
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
        }

        // Right-click on a clip that is not selected selects it first, so
        // the menu acts on what was clicked.
        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
            && let Some(hit) = self.hit(&ctx, p, canvas)
            && !self.arrangement.clips.contains(&hit.clip)
            && self.arrangement.range.is_none()
        {
            self.arrangement.clips = BTreeSet::from([hit.clip]);
            self.selected_clips_now();
        }

        // The H1 hover: which half is under the pointer, and what it does.
        if self.arrangement.gesture.is_some() {
            return;
        }
        self.arrangement.hover = None;
        if let Some(p) = response.hover_pos()
            && let Some(hit) = self.hit(&ctx, p, canvas)
        {
            self.arrangement.hover = Some(Hover::from(&hit));
            let row = match hit.half {
                Half::Top => hit.rect.top() + 7.0,
                Half::Bottom => hit.rect.bottom() - 7.0,
            };
            if let Some(text) = self.handle_hover(&ctx, &hit) {
                let at = pos2(p.x + 14.0, p.y - 12.0);
                widgets::draw_hint(painter, at, widgets::HintMark::None, &text, canvas);
                self.hint = Some(text);
                return;
            }
            let locked = self
                .document
                .project()
                .tracks
                .get(hit.track)
                .and_then(|t| t.clips.iter().find(|c| c.id == hit.clip))
                .is_some_and(|c| c.locked);
            use widgets::HintMark;
            let slip = modifiers.command && modifiers.alt;
            let (cursor, mark, text) = match (hit.edge, hit.half) {
                (_, _) if locked => (CursorIcon::NotAllowed, HintMark::None, "locked"),
                (Some(Edge::End), _) if modifiers.shift => {
                    (CursorIcon::ResizeEast, HintMark::Resize, "drag to loop")
                }
                (Some(Edge::End), _) => (
                    CursorIcon::ResizeHorizontal,
                    HintMark::Resize,
                    "drag to trim · Shift loops",
                ),
                (Some(Edge::Start), _) => (
                    CursorIcon::ResizeHorizontal,
                    HintMark::Resize,
                    "drag to trim",
                ),
                (None, Half::Top) if slip => (
                    CursorIcon::ResizeColumn,
                    HintMark::Resize,
                    "drag to slip the audio",
                ),
                (None, Half::Top) if modifiers.command => (
                    CursorIcon::Copy,
                    HintMark::Move,
                    "drag to copy · add Alt to slip the audio",
                ),
                (None, Half::Top) => (
                    CursorIcon::Grab,
                    HintMark::Move,
                    "drag to move · Ctrl copies",
                ),
                (None, Half::Bottom) => (CursorIcon::Text, HintMark::Resize, "drag to select time"),
            };
            ctx.set_cursor_icon(cursor);
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

    /// The editing keys (Ableton's), on the selected clips or the time
    /// range: Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+D, Delete, Ctrl+Shift+Delete
    /// (delete time), Ctrl+I (insert time), Esc. Runs before the track
    /// keys, so a clip or range selection takes them.
    pub(crate) fn clip_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || self.typing {
            return;
        }
        // Selecting tracks deselects clips and the range.
        if !self.arrangement.selected.is_empty() {
            self.arrangement.clips.clear();
            self.arrangement.range = None;
        }
        use egui::Key;
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        let chosen = !self.arrangement.clips.is_empty() || self.arrangement.range.is_some();
        // Specific chords first: plain keys also match with Shift held.
        let command = if chosen && pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Delete) {
            Edit::DeleteTime
        } else if chosen && pressed(Modifiers::COMMAND, Key::I) {
            Edit::InsertTime
        } else if chosen && pressed(Modifiers::COMMAND, Key::C) {
            Edit::Copy
        } else if chosen && pressed(Modifiers::COMMAND, Key::X) {
            Edit::Cut
        } else if chosen && pressed(Modifiers::COMMAND, Key::D) {
            Edit::Duplicate
        } else if self.clipboard.is_some() && pressed(Modifiers::COMMAND, Key::V) {
            Edit::Paste
        } else if chosen
            && (pressed(Modifiers::NONE, Key::Delete) || pressed(Modifiers::NONE, Key::Backspace))
        {
            Edit::Delete
        } else if chosen && pressed(Modifiers::NONE, Key::Escape) {
            self.arrangement.clips.clear();
            self.arrangement.range = None;
            return;
        } else {
            return;
        };
        self.edit(command);
        ctx.request_repaint();
    }

    /// The lanes' right-click menu: the editing commands, then the grid.
    pub(crate) fn lanes_menu(&mut self, ui: &mut egui::Ui) {
        for command in Edit::ALL {
            let button = egui::Button::new(command.name()).shortcut_text(command.keys());
            if ui.add_enabled(self.can_edit(command), button).clicked() {
                self.edit(command);
                ui.close();
            }
            if command == Edit::Delete {
                ui.separator();
            }
        }
        ui.separator();
        self.clip_menu(ui);
        self.fade_menu(ui);
        self.grid_menu(ui);
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
