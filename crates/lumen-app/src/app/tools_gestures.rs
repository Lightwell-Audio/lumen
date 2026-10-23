//! Direct manipulation of arrangement clips and time ranges.

use std::collections::BTreeSet;

use eframe::egui::{self, CursorIcon, Rect};
use lumen_core::time::Ticks;

use super::editing::MenuFor;
use super::grid::draw_snap_guide;
use super::info::{Held, Info, Mouse};
use super::navigate::ViewStep;
use crate::App;
use crate::arrangement::select::{ClipRegion, Edge, Gesture, Hover};
use crate::commands::Command;

impl App {
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
        if response.clicked() || response.drag_started() {
            self.arrangement.tool_keys = true;
        }

        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
        {
            // A drag shows its own hover (a trim rail, a handle), if any.
            self.arrangement.hover = None;
            self.arrangement.gesture =
                if self.arrangement.tool == super::tools::ArrangementTool::Split {
                    let at = self.snapped(self.ticks_at(canvas, origin.x), modifiers.alt);
                    let hit = self.hit(&ctx, origin, canvas);
                    let ids = hit.map_or_else(
                        || self.arrangement.clips.iter().copied().collect(),
                        |hit| {
                            if self.arrangement.clips.contains(&hit.clip) {
                                self.arrangement.clips.iter().copied().collect()
                            } else {
                                vec![hit.clip]
                            }
                        },
                    );
                    if let Some(hit) = hit {
                        self.arrangement.clips = BTreeSet::from([hit.clip]);
                        self.arrangement.range = None;
                    }
                    Some(Gesture::Split {
                        from: at,
                        to: at,
                        clips: ids,
                    })
                } else if self.arrangement.tool == super::tools::ArrangementTool::Zoom {
                    let at = self.ticks_at(canvas, origin.x);
                    Some(Gesture::Zoom { from: at, to: at })
                } else if self.arrangement.tool == super::tools::ArrangementTool::Pointer {
                    self.start_gesture(&ctx, origin, canvas, modifiers)
                } else {
                    None
                };
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(p) = response.interact_pointer_pos()
        {
            if let Some(Gesture::Split { from, clips, .. }) = self.arrangement.gesture.clone() {
                let to = self.snapped(self.ticks_at(canvas, p.x), modifiers.alt);
                self.arrangement.gesture = Some(Gesture::Split { from, to, clips });
                draw_snap_guide(
                    painter,
                    self.nav.view.x(canvas.left(), to.as_quarters()),
                    canvas,
                );
                ctx.set_cursor_icon(CursorIcon::Crosshair);
            } else if let Some(Gesture::Zoom { from, .. }) = self.arrangement.gesture {
                let to = self.ticks_at(canvas, p.x);
                self.arrangement.gesture = Some(Gesture::Zoom { from, to });
                draw_snap_guide(
                    painter,
                    self.nav.view.x(canvas.left(), to.as_quarters()),
                    canvas,
                );
                ctx.set_cursor_icon(CursorIcon::ZoomIn);
            } else {
                self.continue_gesture(&ctx, p, canvas, modifiers.alt, painter);
            }
        }
        if response.drag_stopped() {
            if let Some(Gesture::Split { to, clips, .. }) = self.arrangement.gesture.clone() {
                self.split_ids_at(&clips, to);
            }
            if let Some(Gesture::Zoom { from, to }) = self.arrangement.gesture
                && from != to
            {
                self.nav
                    .steps
                    .push(ViewStep::Fit(from.min(to), from.max(to)));
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
        if self.arrangement.tool == super::tools::ArrangementTool::Pointer
            && response.double_clicked()
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
            let tool = self.arrangement.tool;
            if tool == super::tools::ArrangementTool::Zoom {
                self.nav.steps.push(if modifiers.alt {
                    ViewStep::ZoomOut
                } else {
                    ViewStep::ZoomIn
                });
                return;
            }
            if matches!(
                tool,
                super::tools::ArrangementTool::Erase | super::tools::ArrangementTool::Mute
            ) {
                if let Some(hit) = self.hit(&ctx, p, canvas) {
                    let locked = self
                        .document
                        .project()
                        .tracks
                        .get(hit.track)
                        .and_then(|t| t.clips.iter().find(|c| c.id == hit.clip))
                        .is_none_or(|c| c.locked);
                    if !locked {
                        self.arrangement.clips = BTreeSet::from([hit.clip]);
                        self.arrangement.range = None;
                        if tool == super::tools::ArrangementTool::Erase {
                            self.document.delete_clips(&[hit.clip]);
                            self.arrangement.clips.clear();
                        } else {
                            self.toggle_clip_mute();
                        }
                    }
                }
                return;
            }
            if tool == super::tools::ArrangementTool::Draw && self.hit(&ctx, p, canvas).is_none() {
                if let Some(track) = self.lane_track(&ctx, canvas, p) {
                    let at = self.snapped(self.ticks_at(canvas, p.x), modifiers.alt);
                    self.put_midi_clip(track, crate::document::NewClip::Bar(at));
                }
                return;
            }
            if self.arrangement.tool == super::tools::ArrangementTool::Split {
                if let Some(hit) = self.hit(&ctx, p, canvas) {
                    self.arrangement.clips = BTreeSet::from([hit.clip]);
                    self.arrangement.range = None;
                }
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
        if self.arrangement.tool != super::tools::ArrangementTool::Pointer {
            let tool = self.arrangement.tool;
            use super::tools::ArrangementTool as Tool;
            let (cursor, info) = match tool {
                Tool::Pointer => (CursorIcon::Default, Info::new("Pointer tool")),
                Tool::Draw => (
                    CursorIcon::Crosshair,
                    Info::new("Draw tool").on(Held::NONE, Mouse::Click, "draw a MIDI clip"),
                ),
                Tool::Erase => (
                    CursorIcon::Crosshair,
                    Info::new("Erase tool").on(Held::NONE, Mouse::Click, "delete this clip"),
                ),
                Tool::Split => (
                    CursorIcon::Crosshair,
                    Info::new("Split tool")
                        .on(Held::NONE, Mouse::Click, "cut here")
                        .on(Held::NONE, Mouse::Drag, "aim, then release to cut")
                        .on(Held::ALT, Mouse::Click, "cut off the grid"),
                ),
                Tool::Mute => (
                    CursorIcon::PointingHand,
                    Info::new("Mute tool").on(Held::NONE, Mouse::Click, "toggle clip mute"),
                ),
                Tool::Zoom => (
                    CursorIcon::ZoomIn,
                    Info::new("Zoom tool")
                        .on(Held::NONE, Mouse::Click, "zoom in")
                        .on(Held::ALT, Mouse::Click, "zoom out")
                        .on(Held::NONE, Mouse::Drag, "zoom to the range"),
                ),
            };
            ctx.set_cursor_icon(cursor);
            self.set_info(info);
            if tool == super::tools::ArrangementTool::Split {
                let at = self.snapped(self.ticks_at(canvas, p.x), modifiers.alt);
                draw_snap_guide(
                    painter,
                    self.nav.view.x(canvas.left(), at.as_quarters()),
                    canvas,
                );
            }
            if self.hit(&ctx, p, canvas).is_some_and(|hit| {
                self.document.project().tracks[hit.track]
                    .clips
                    .iter()
                    .any(|c| c.id == hit.clip && c.locked)
            }) && !matches!(tool, super::tools::ArrangementTool::Zoom)
            {
                ctx.set_cursor_icon(CursorIcon::NotAllowed);
            }
            return;
        }
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
}
