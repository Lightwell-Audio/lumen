//! Warp markers in the audio editor (M9.5 S5b, the owner's G B and P A): the
//! warp strip's pointer, its keys, its Tab stops and its drawing.
//!
//! - **Hover** the strip: a ghost marker at the nearest transient, or at the
//!   pointer. Pressing the ghost makes a marker and moves it in one gesture;
//!   double-click adds one without moving it. A marker is dragged by its flag;
//!   Shift and drag slides the audio under it without moving the marker;
//!   double-click on a marker deletes it; Alt bypasses the grid.
//! - **Keyboard:** each marker is a Tab stop, in time order. Left and Right
//!   move it by the grid (Alt finely), Delete takes it away. Add warp marker
//!   puts one at the edit cursor (the playhead when there is none).
//! - A drag shows the speed of the two spans beside the marker; at the limit
//!   (25% and 400%) the marker stops and the chip turns to the warning colour.
//!
//! One drag is one undo step: each frame redoes the edit from the project the
//! drag began on (`Document::edit_warp_marker`). The edits themselves are
//! `lumen_project::edit::warp_markers`.

use std::sync::Arc;

use eframe::egui::{
    self, CursorIcon, EventFilter, Key, Modifiers, Painter, PointerButton, Pos2, Rect, Response,
    Sense, Stroke, Ui, pos2,
};
use lumen_core::warp;
use lumen_media::detect::Analysis;
use lumen_project::edit::warp::{Refused, origin, within};
use lumen_project::edit::warp_markers::{MarkerEdit, held_at};
use lumen_project::model::{Clip, Id};

use super::gestures::WaveArea;
use super::range::ticks_of;
use crate::App;
use crate::app::info::{Held, Info, Mouse};
use crate::audio_editor::detect::in_view;
use crate::audio_editor::paint;
use crate::audio_editor::source::WholeFile;
use crate::audio_editor::warp_strip::{self, Ghost, WarpStrip};
use crate::audio_editor::{AudioEditor, MarkerDrag, Slide, StripHover, handles};
use crate::commands::Command;
use crate::document::warp::with_marker_edit;
use crate::timeview::TimeView;
use crate::{theme, widgets};

/// Said when a marker cannot go further.
const AT_LIMIT: &str = "limit: a span plays between 25% and 400% of the file's speed";
const OUTSIDE_FILE: &str = "the clip would start outside its file";

/// The stable id of marker `index`'s Tab stop.
fn stop_id(index: usize) -> egui::Id {
    egui::Id::new(("audio editor warp marker", index))
}

/// The marker with the keyboard, among the stops drawn last frame.
pub(crate) fn focused(ctx: &egui::Context, ids: &[egui::Id]) -> Option<usize> {
    ids.iter().position(|id| ctx.memory(|m| m.has_focus(*id)))
}

/// Takes the keys a focused marker answers: Left and Right (with Alt for
/// fine; with Shift they slide the audio under the marker instead), and
/// Delete.
pub(crate) fn take_keys(events: &mut Vec<egui::Event>) -> Vec<(Key, Modifiers)> {
    let mut taken = Vec::new();
    events.retain(|event| {
        let egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = *event
        else {
            return true;
        };
        let plain = modifiers == Modifiers::NONE;
        let ours = (matches!(key, Key::ArrowLeft | Key::ArrowRight)
            && (plain
                || modifiers == Modifiers::ALT
                || modifiers == Modifiers::SHIFT
                || modifiers == (Modifiers::SHIFT | Modifiers::ALT)))
            || (plain && matches!(key, Key::Delete | Key::Backspace));
        if ours {
            taken.push((key, modifiers));
        }
        !ours
    });
    taken
}

/// The percentage a span plays at, as it reads: "112%".
fn percent(ratio: f64) -> String {
    format!("{:.0}%", ratio * 100.0)
}

/// What the span chip says for marker `index` of `clip`: "Before 112% · after
/// 94%", with only the side a marker has.
pub(crate) fn span_words(clip: &Clip, index: usize) -> Option<String> {
    let w = clip.warp.as_ref()?;
    let (before, after) = warp::ratios_beside(&w.markers, w.original_bpm, index);
    Some(match (before, after) {
        (Some(a), Some(b)) => format!("Before {} \u{00B7} after {}", percent(a), percent(b)),
        (Some(a), None) => format!("Before {}", percent(a)),
        (None, Some(b)) => format!("After {}", percent(b)),
        (None, None) => "Only marker".to_owned(),
    })
}

impl App {
    /// The clip's marker in the project's clip `id`: where `base` holds it.
    fn clip_in(project: &lumen_project::model::Project, id: Id) -> Option<&Clip> {
        project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == id)
            .map(|c| &**c)
    }

    /// The warp strip's pointer work: hover (a marker, or the ghost), a drag
    /// that moves a marker or makes one, double-click, and the right-click
    /// menu.
    pub(super) fn warp_strip_pointer(
        &mut self,
        response: &Response,
        clip: &Clip,
        strip: &WarpStrip,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) {
        let ctx = response.ctx.clone();
        let left = strip.rect.left();
        let found = self.found_for(clip).map(|(_, a, s)| (a, s));
        let ghost_at = |x: f32, ed: &AudioEditor| -> Option<Ghost> {
            warp_strip::ghost(
                area.file,
                (area.rate, ed.time),
                left,
                x,
                found.as_ref().map(|(a, s)| (&**a, *s)),
            )
        };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(press) = ctx.input(|i| i.pointer.press_origin())
            && strip.rect.contains(press)
            && self.opened_clip_takes_edits(clip.id)
        {
            let shift = ctx.input(|i| i.modifiers.shift);
            let ghost = ghost_at(press.x, ed);
            if let Some(drag) =
                self.start_marker_drag((clip, strip), (press, shift), ghost, area, ed)
            {
                ed.markers.picked = Some(drag.index);
                ed.markers.drag = Some(drag);
                ed.markers.held = None;
                self.select_opened_clip(clip.id);
            }
        }
        if let Some(drag) = ed.markers.drag.clone() {
            let down = ctx.input(|i| i.pointer.primary_down());
            if down && let Some(p) = ctx.input(|i| i.pointer.latest_pos()) {
                self.step_marker_drag(&ctx, clip, &drag, p, area, ed);
            }
            if !down || response.drag_stopped() {
                ed.markers.drag = None;
                ed.markers.held = None;
            }
            return;
        }

        let pointer = response.hover_pos();
        ed.markers.hover = pointer.and_then(|p| match strip.hit(p) {
            Some(i) => Some(StripHover::Marker(i)),
            None => ghost_at(p.x, ed).map(StripHover::Ghost),
        });
        if response.double_clicked() && self.opened_clip_takes_edits(clip.id) {
            match ed.markers.hover {
                Some(StripHover::Marker(i)) => self.remove_warp_marker(clip.id, i, ed),
                Some(StripHover::Ghost(g)) => {
                    self.select_opened_clip(clip.id);
                    self.add_warp_marker(clip.id, MarkerEdit::AddOn(g.source), ed);
                }
                None => {}
            }
        } else if response.clicked()
            && let Some(StripHover::Marker(i)) = ed.markers.hover
        {
            // A click picks the marker, and the keyboard goes to it.
            ed.markers.picked = Some(i);
            ctx.memory_mut(|m| m.request_focus(stop_id(i)));
        }
        if response.secondary_clicked() {
            ed.markers.menu = match ed.markers.hover {
                Some(StripHover::Marker(i)) => Some(i),
                _ => None,
            };
        }
        let menu = ed.markers.menu;
        widgets::Menu::context(response).show(|ui| self.warp_strip_menu(ui, clip, menu, ed));
        self.warp_hover_info(&ctx, ed);
    }

    /// The right-click menu of the strip: the marker under the pointer, and
    /// the clip's markers.
    fn warp_strip_menu(
        &mut self,
        ui: &mut Ui,
        clip: &Clip,
        marker: Option<usize>,
        ed: &mut AudioEditor,
    ) {
        if let Some(i) = marker {
            widgets::menu_caption(ui, "Warp marker");
            if widgets::menu_item(ui, "Delete marker", None, true).clicked() {
                self.remove_warp_marker(clip.id, i, ed);
                ui.close();
            }
        }
        let many = clip.warp.as_ref().is_some_and(|w| w.markers.len() > 1);
        let keys = self.keymap.keys(Command::ClearWarpMarkers);
        if widgets::menu_item(ui, "Remove all warp markers", keys.as_ref(), many).clicked() {
            self.clear_warp_markers(clip.id, ed);
            ui.close();
        }
    }

    /// What the pointer on the strip does, in the info line, and its cursor.
    fn warp_hover_info(&mut self, ctx: &egui::Context, ed: &AudioEditor) {
        match ed.markers.hover {
            Some(StripHover::Marker(_)) => {
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                self.set_info(
                    Info::new("Warp marker")
                        .on(Held::NONE, Mouse::Drag, "stretch the audio beside it")
                        .on(Held::SHIFT, Mouse::Drag, "slide the audio under it")
                        .on(Held::ALT, Mouse::Drag, "ignore the grid")
                        .on(Held::NONE, Mouse::DoubleClick, "delete")
                        .on(Held::NONE, Mouse::RightClick, "marker menu"),
                );
            }
            Some(StripHover::Ghost(g)) => {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                let what = if g.on_transient {
                    "Warp marker \u{00B7} on a transient"
                } else {
                    "Warp marker"
                };
                self.set_info(
                    Info::new(what)
                        .on(Held::NONE, Mouse::Drag, "make a marker and move it")
                        .on(Held::NONE, Mouse::DoubleClick, "add a marker here")
                        .key(Command::AddWarpMarker, "add one at the cursor"),
                );
            }
            None => {}
        }
    }

    /// Picks the marker or makes the one a drag moves: `None` when there is
    /// nothing to pick up or no marker can be made.
    fn start_marker_drag(
        &mut self,
        (clip, strip): (&Clip, &WarpStrip),
        (press, shift): (Pos2, bool),
        ghost: Option<Ghost>,
        area: &WaveArea<'_>,
        ed: &AudioEditor,
    ) -> Option<MarkerDrag> {
        let (index, base, slide) = match strip.hit(press) {
            Some(i) => {
                let marker = *clip.warp.as_ref()?.markers.get(i)?;
                let slide = shift.then(|| {
                    let beat = ed.time.beat_at(strip.rect.left(), press.x);
                    Slide {
                        source: marker.source,
                        beat,
                        per_beat: seconds_per_beat(area, strip.flags[i], strip.rect.left(), ed),
                    }
                });
                (i, Arc::new(self.document.project().clone()), slide)
            }
            None => {
                let ghost = ghost?;
                let (made, index) = with_marker_edit(
                    self.document.project(),
                    clip.id,
                    MarkerEdit::AddOn(ghost.source),
                )?;
                (index?, Arc::new(made), None)
            }
        };
        let held = Self::clip_in(&base, clip.id)?;
        let at = held.warp.as_ref()?.markers.get(index)?.at;
        let under = ticks_of(ed.time.beat_at(strip.rect.left(), press.x));
        let grab = origin(held) + at - under;
        Some(MarkerDrag {
            index,
            base,
            grab,
            slide,
        })
    }

    /// One frame of a marker drag at `p`: the view scrolls at the wave's side,
    /// the edit is redone from where the drag began, and the chip says the
    /// speeds beside the marker.
    fn step_marker_drag(
        &mut self,
        ctx: &egui::Context,
        clip: &Clip,
        drag: &MarkerDrag,
        p: Pos2,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) {
        let dt = ctx.input(|i| i.stable_dt);
        let from = ctx.input(|i| i.pointer.press_origin()).map_or(p.x, |o| o.x);
        let pull = handles::edge_scroll(p.x, from, area.body.x_range(), dt);
        let moved = ed.time.scrolled(pull, area.limits);
        if moved != ed.time {
            ed.time = moved;
            self.transport.follow_paused = true;
            ctx.request_repaint();
        }
        let left = area.body.left();
        let Some(held) = Self::clip_in(&drag.base, clip.id) else {
            return;
        };
        let change = match drag.slide {
            Some(slide) => {
                let now = ed.time.beat_at(left, p.x);
                MarkerEdit::Slide {
                    index: drag.index,
                    source: slide.source - (now - slide.beat) * slide.per_beat,
                }
            }
            None => {
                let free = ctx.input(|i| i.modifiers.alt);
                let under = ticks_of(ed.time.beat_at(left, p.x)) + drag.grab;
                let target = self.snapped_at_zoom(under, free, ed.time.zoom);
                MarkerEdit::Move {
                    index: drag.index,
                    to: target - origin(held),
                }
            }
        };
        match self
            .document
            .edit_warp_marker(&drag.base, clip.id, change, Some("warp marker"))
        {
            Ok(done) => ed.markers.held = done.held.then_some(AT_LIMIT),
            Err(Refused::Unchanged) => {
                // Still where it was: held, if the pointer asks for more.
                if let MarkerEdit::Move { index, to } = change
                    && let Some(now) = self.shown_clip(clip.id)
                {
                    ed.markers.held = held_at(&now, index, to).then_some(AT_LIMIT);
                }
            }
            Err(_) => ed.markers.held = Some(OUTSIDE_FILE),
        }
        ctx.set_cursor_icon(if drag.slide.is_some() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::ResizeHorizontal
        });
        let now = self.shown_clip(clip.id);
        let words = now.as_ref().and_then(|c| span_words(c, drag.index));
        let overlay = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("audio editor warp marker"),
            ))
            .with_clip_rect(area.body.union(Rect::from_min_max(
                pos2(area.body.left(), area.body.top() - theme::size::WARP_STRIP),
                area.body.max,
            )));
        if let Some(words) = words {
            let text = match ed.markers.held {
                Some(_) => format!("{words} \u{00B7} limit"),
                None => words,
            };
            widgets::draw_hint_tone(
                &overlay,
                pos2(p.x + 14.0, p.y + 14.0),
                &text,
                area.body,
                ed.markers.held.is_some(),
            );
        }
        if let Some(why) = ed.markers.held {
            self.set_info(Info::new(format!("Warp marker \u{00B7} {why}")));
        } else if drag.slide.is_some() {
            self.set_info(Info::new("Warp marker").on(
                Held::SHIFT,
                Mouse::Drag,
                "slide the audio under it",
            ));
        }
    }

    /// Adds a marker (a ghost's second, or the cursor's): one undo step.
    /// The new marker is picked and takes the keyboard.
    pub(crate) fn add_warp_marker(&mut self, id: Id, change: MarkerEdit, ed: &mut AudioEditor) {
        let base = self.document.project().clone();
        match self.document.edit_warp_marker(&base, id, change, None) {
            Ok(done) => {
                if let Some(i) = done.index {
                    ed.markers.picked = Some(i);
                    ed.markers.reveal = Some(i);
                    ed.markers.focus_next = Some(i);
                }
            }
            Err(Refused::Unchanged) => {
                self.say("There is a warp marker here already".to_owned());
            }
            Err(_) => self.say(format!("Can't add a warp marker: {OUTSIDE_FILE}")),
        }
    }

    /// Deletes marker `index`. The keyboard goes to the one before it.
    pub(crate) fn remove_warp_marker(&mut self, id: Id, index: usize, ed: &mut AudioEditor) {
        let base = self.document.project().clone();
        match self
            .document
            .edit_warp_marker(&base, id, MarkerEdit::Remove(index), None)
        {
            Ok(_) => {
                ed.markers.picked = None;
                ed.markers.focus_next = index.checked_sub(1).or(Some(0));
            }
            Err(_) => self.say("A warped clip keeps at least one warp marker".to_owned()),
        }
    }

    /// Removes every warp marker but one: the clip plays at the file's tempo.
    pub(crate) fn clear_warp_markers(&mut self, id: Id, ed: &mut AudioEditor) {
        let base = self.document.project().clone();
        if self
            .document
            .edit_warp_marker(&base, id, MarkerEdit::Clear, None)
            .is_ok()
        {
            ed.markers.picked = None;
        }
    }

    /// The Add warp marker command: a marker at the edit cursor of the opened
    /// clip, the playhead when there is none.
    pub(crate) fn add_warp_marker_command(&mut self) {
        let Some(id) = self.detail_audio_clip() else {
            return;
        };
        if !self.opened_clip_takes_edits(id) {
            return;
        }
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let mut ed = std::mem::take(&mut self.audio_editor);
        let at = ed.cursor.unwrap_or_else(|| self.transport.playhead());
        self.select_opened_clip(id);
        // A cursor in a later repeat of a loop is the same place in the first.
        self.add_warp_marker(id, MarkerEdit::Add(within(&clip, at)), &mut ed);
        self.audio_editor = ed;
    }

    /// The Remove all warp markers command, for the opened clip.
    pub(crate) fn clear_warp_markers_command(&mut self) {
        let Some(id) = self.detail_audio_clip() else {
            return;
        };
        if !self.opened_clip_takes_edits(id) {
            return;
        }
        let mut ed = std::mem::take(&mut self.audio_editor);
        self.select_opened_clip(id);
        self.clear_warp_markers(id, &mut ed);
        self.audio_editor = ed;
    }

    /// Why a marker command cannot run, if it cannot: the opened clip is
    /// warped.
    pub(crate) fn markers_unavailable(&self) -> Result<(), &'static str> {
        let Some(id) = self.detail_audio_clip() else {
            return Err("Open a warped audio clip in the editor first");
        };
        match self.clip(id) {
            Some(clip) if clip.warp.is_some() => Ok(()),
            _ => Err("Warp the clip first"),
        }
    }

    /// The keys taken for the focused marker, run with the clip and the view
    /// at hand.
    pub(super) fn run_marker_keys(&mut self, clip: &Clip, ed: &mut AudioEditor) {
        let keys = std::mem::take(&mut ed.markers.keys);
        for (index, key, mods) in keys {
            if matches!(key, Key::Delete | Key::Backspace) {
                if self.opened_clip_takes_edits(clip.id) {
                    self.select_opened_clip(clip.id);
                    self.remove_warp_marker(clip.id, index, ed);
                }
                continue;
            }
            if mods.shift {
                self.slide_warp_marker(clip.id, index, key, mods.alt, ed);
            } else {
                self.nudge_warp_marker(clip.id, index, key, mods.alt, ed);
            }
        }
    }

    /// Shift and Left or Right on a marker: the audio under it slides by the
    /// grid (the marker stays), an undo step a press. The same edit as the
    /// Shift and drag.
    fn slide_warp_marker(
        &mut self,
        id: Id,
        index: usize,
        key: Key,
        fine: bool,
        ed: &mut AudioEditor,
    ) {
        if !self.opened_clip_takes_edits(id) {
            return;
        }
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let Some((marker, bpm)) = clip
            .warp
            .as_ref()
            .and_then(|w| Some((*w.markers.get(index)?, w.original_bpm)))
        else {
            return;
        };
        let direction = if key == Key::ArrowLeft { -1 } else { 1 };
        // The step is a grid cell of the file's own beats; the audio moves
        // the way the key points (a drag to the right moves it later).
        let here = origin(&clip) + marker.at;
        let step = self.key_step(ed.time.zoom, here, direction, fine) - here;
        let seconds = step.as_quarters() * 60.0 / bpm.max(1.0);
        let source = marker.source - seconds;
        let base = self.document.project().clone();
        self.select_opened_clip(id);
        match self
            .document
            .edit_warp_marker(&base, id, MarkerEdit::Slide { index, source }, None)
        {
            Ok(done) => {
                ed.markers.reveal = Some(index);
                if done.held {
                    self.say(format!("This marker can't slide further: {AT_LIMIT}"));
                }
            }
            Err(Refused::Unchanged) => {}
            Err(_) => self.say(format!("This marker can't slide further: {OUTSIDE_FILE}")),
        }
    }

    /// Left or Right on a marker: it moves by the grid, an undo step a press.
    fn nudge_warp_marker(
        &mut self,
        id: Id,
        index: usize,
        key: Key,
        fine: bool,
        ed: &mut AudioEditor,
    ) {
        if !self.opened_clip_takes_edits(id) {
            return;
        }
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let Some(marker) = clip
            .warp
            .as_ref()
            .and_then(|w| w.markers.get(index))
            .copied()
        else {
            return;
        };
        let direction = if key == Key::ArrowLeft { -1 } else { 1 };
        let start = origin(&clip);
        let to = self.key_step(ed.time.zoom, start + marker.at, direction, fine) - start;
        let base = self.document.project().clone();
        self.select_opened_clip(id);
        match self
            .document
            .edit_warp_marker(&base, id, MarkerEdit::Move { index, to }, None)
        {
            Ok(done) => {
                ed.markers.reveal = Some(index);
                if done.held {
                    self.say(format!("This marker can't go further: {AT_LIMIT}"));
                }
            }
            Err(Refused::Unchanged) => {
                if held_at(&clip, index, to) {
                    self.say(format!("This marker can't go further: {AT_LIMIT}"));
                }
            }
            Err(_) => self.say(format!("This marker can't go further: {OUTSIDE_FILE}")),
        }
    }

    /// "Warp marker at bar 4 beat 2, before 112% after 94%": a marker's
    /// accessible name.
    pub(crate) fn marker_name(&self, clip: &Clip, index: usize) -> String {
        let Some(marker) = clip.warp.as_ref().and_then(|w| w.markers.get(index)) else {
            return "Warp marker".to_owned();
        };
        let at = self.place_words(origin(clip) + marker.at);
        let speed =
            span_words(clip, index).map_or_else(String::new, |w| format!(", {}", w.to_lowercase()));
        format!("Warp marker at {at}{speed}")
    }

    /// The markers' Tab stops, created in time order. Returns the one with
    /// the keyboard.
    pub(super) fn marker_stops(
        &mut self,
        ui: &mut Ui,
        clip: &Clip,
        strip: &WarpStrip,
        ed: &mut AudioEditor,
    ) -> Option<(usize, Response)> {
        let mut focused = None;
        ed.markers.ids.clear();
        for index in 0..strip.flags.len() {
            let Some(rect) = strip.flag_rect(index) else {
                continue;
            };
            let id = stop_id(index);
            ed.markers.ids.push(id);
            if ed.markers.focus_next == Some(index) {
                ed.markers.focus_next = None;
                ui.memory_mut(|m| m.request_focus(id));
            }
            let response = ui.interact(rect, id, Sense::focusable_noninteractive());
            let name = self.marker_name(clip, index);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, name.clone())
            });
            if response.has_focus() {
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        response.id,
                        EventFilter {
                            horizontal_arrows: true,
                            ..EventFilter::default()
                        },
                    );
                });
                if ed.markers.focused_before != Some(index) {
                    ed.markers.reveal = Some(index);
                    ed.markers.picked = Some(index);
                    ui.ctx().request_repaint();
                }
                focused = Some((index, response));
            }
        }
        ed.markers.focused_before = focused.as_ref().map(|(i, _)| *i);
        focused
    }

    /// Brings the marker asked for into view.
    pub(super) fn reveal_marker(
        &mut self,
        strip: &WarpStrip,
        limits: crate::timeview::Limits,
        ed: &mut AudioEditor,
    ) {
        let Some(index) = ed.markers.reveal.take() else {
            return;
        };
        let Some(x) = strip.flags.get(index) else {
            return;
        };
        let beat = ed.time.beat_at(strip.rect.left(), *x);
        let shown = ed.time.showing(beat, beat, limits);
        if shown != ed.time {
            ed.time = shown;
            self.transport.follow_paused = true;
        }
    }

    /// The strip's ground, drawn before the transients' triangles.
    pub(super) fn paint_strip_ground(painter: &Painter, rect: Rect) {
        let palette = theme::p();
        painter.rect_filled(rect, 0.0, palette.raised);
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
    }

    /// The strip's notches, the ghost and a hairline down each marker
    /// through the wave, over the ground and the triangles.
    pub(super) fn paint_warp_strip(
        &self,
        painter: &Painter,
        strip: &WarpStrip,
        body: Rect,
        ed: &AudioEditor,
        focused: Option<&(usize, Response)>,
    ) {
        let palette = theme::p();
        let rect = strip.rect;
        let view = rect.x_range();
        let strip_painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
        let wave_painter = painter.with_clip_rect(body.intersect(painter.clip_rect()));
        let dragged = ed.markers.drag.as_ref().map(|d| d.index);
        let hot = match ed.markers.hover {
            Some(StripHover::Marker(i)) => Some(i),
            _ => None,
        };
        for (index, x) in strip.flags.iter().enumerate() {
            if !view.contains(*x) {
                continue;
            }
            // Touched or chosen: the accent. Under the pointer: the notch
            // keeps its colour and its pole thickens.
            let chosen = Some(index) == dragged
                || Some(index) == ed.markers.picked
                || focused.is_some_and(|(i, _)| *i == index);
            let lit = chosen || Some(index) == hot;
            let ink = if chosen { palette.accent } else { palette.text };
            // The notch's tip, its pole and the hairline through the wave
            // share one x and one stroke.
            let x = snapped(painter, *x);
            let pole = Stroke::new(
                if lit {
                    theme::stroke::LIT
                } else {
                    theme::stroke::HAIRLINE
                },
                ink.gamma_multiply(if lit {
                    theme::WARP_POLE_LIT
                } else {
                    theme::WARP_POLE
                }),
            );
            wave_painter.vline(x, body.y_range(), pole);
            Self::paint_notch(
                &strip_painter,
                rect,
                x,
                Look {
                    ink,
                    pole,
                    ghost: false,
                    lifted: Some(index) == hot,
                },
            );
        }
        if let Some(StripHover::Ghost(g)) = ed.markers.hover
            && dragged.is_none()
            && view.contains(g.x)
        {
            let ink = palette.accent.gamma_multiply(if g.on_transient {
                1.0
            } else {
                theme::WARP_GHOST_FREE
            });
            Self::paint_notch(
                &strip_painter,
                rect,
                snapped(painter, g.x),
                Look {
                    ink,
                    pole: Stroke::new(theme::stroke::HAIRLINE, ink),
                    ghost: true,
                    lifted: false,
                },
            );
        }
        // The ring reaches past the strip (it is round the notch), so it is
        // drawn with the editor's painter, not the strip's.
        if let Some((index, response)) = focused
            && let Some(ring) = strip.notch_rect(*index)
        {
            crate::widgets::raised::focus_ring(painter, response, ring);
        }
    }

    /// The transients in the strip: a small triangle pointing down at each,
    /// except where a marker already stands (within a point). Only those in
    /// view are looked at.
    pub(super) fn paint_transient_ticks(
        painter: &Painter,
        strip: &WarpStrip,
        file: &WholeFile,
        (time, rate): (TimeView, u32),
        (analysis, sensitivity): (&Analysis, u8),
    ) {
        let Some((a, b)) = paint::visible_frames(file, time, strip.rect) else {
            return;
        };
        let rate = f64::from(rate);
        let (w, h) = (theme::size::WARP_TICK_W, theme::size::WARP_TICK_H);
        let ink = theme::p().text_secondary;
        let inside = painter.with_clip_rect(strip.rect.intersect(painter.clip_rect()));
        let top = strip.rect.bottom() - h - theme::size::WARP_TICK_FLOOR;
        for t in in_view(analysis, sensitivity, (a as f64 / rate, b as f64 / rate)) {
            let Some(beat) = file.beat_of_frame(t.seconds * rate) else {
                continue;
            };
            let x = time.x(strip.rect.left(), beat);
            // The markers are in time order: one beside this transient is
            // the first at or after a point before it.
            let next = strip.flags.partition_point(|f| *f < x - 1.0);
            if strip.flags.get(next).is_some_and(|f| *f < x + 1.0) {
                continue;
            }
            inside.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x - w / 2.0, top),
                    pos2(x + w / 2.0, top),
                    pos2(x, top + h),
                ],
                ink.gamma_multiply(
                    theme::WARP_TICK_BASE + theme::WARP_TICK_GAIN * t.strength.clamp(0.0, 1.0),
                ),
                Stroke::NONE,
            ));
        }
    }

    /// One notch: a rounded pin centred on `x` that points down at its
    /// place, with its pole under it to the strip's floor.
    fn paint_notch(painter: &Painter, rect: Rect, x: f32, look: Look) {
        let (w, h) = (theme::size::WARP_NOTCH_W, theme::size::WARP_NOTCH_H);
        let (left, right) = (x - w / 2.0, x + w / 2.0);
        let top = rect.top() + theme::size::WARP_NOTCH_INSET;
        let r = theme::size::WARP_NOTCH_RADIUS;
        let shoulder = top + h - theme::size::WARP_NOTCH_TAPER;
        let mut points = Vec::with_capacity(12);
        for (cx, cy, from) in [(left + r, top + r, 180.0_f32), (right - r, top + r, 270.0)] {
            for step in 0..=3 {
                let angle = (from + 30.0 * step as f32).to_radians();
                points.push(pos2(cx + r * angle.cos(), cy + r * angle.sin()));
            }
        }
        points.push(pos2(right, shoulder));
        points.push(pos2(x, top + h));
        points.push(pos2(left, shoulder));
        painter.vline(x, top + h..=rect.bottom(), look.pole);
        let (fill, stroke) = if look.ghost {
            (
                look.ink.gamma_multiply(theme::WARP_GHOST_FILL),
                Stroke::new(theme::stroke::LIT, look.ink),
            )
        } else if look.lifted {
            (
                look.ink,
                Stroke::new(theme::stroke::HAIRLINE, theme::p().accent),
            )
        } else {
            (look.ink, Stroke::NONE)
        };
        painter.add(egui::Shape::convex_polygon(points, fill, stroke));
    }
}

/// How a notch is drawn: its ink, the stroke of its pole, whether it is the
/// ghost (a rim and a faint fill) and whether it is lifted (a marker under
/// the pointer, with an accent rim).
struct Look {
    ink: egui::Color32,
    pole: Stroke,
    ghost: bool,
    lifted: bool,
}

/// `x` on a whole physical pixel, so a notch's tip, its pole and the
/// hairline through the wave land on the same one.
fn snapped(painter: &Painter, x: f32) -> f32 {
    let ppp = painter.pixels_per_point();
    (x * ppp).round() / ppp
}

/// The seconds of the file in a beat at the flag `x`.
fn seconds_per_beat(area: &WaveArea<'_>, x: f32, left: f32, ed: &AudioEditor) -> f64 {
    let beat = ed.time.beat_at(left, x);
    area.file
        .span_at(beat)
        .map_or(0.5, |s| s.frames_per_beat / f64::from(area.rate.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;
    use lumen_project::model::Warp;

    fn warped(markers: &[(f64, i64)]) -> Clip {
        use lumen_project::model::{ClipContent, Id};
        let mut warp = Warp::new(120.0, 0.0);
        warp.markers = markers
            .iter()
            .map(|(s, q)| warp::Marker::new(*s, Ticks::quarters(*q)))
            .collect();
        Clip {
            id: Id(1),
            name: "Loop".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(8),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: Some(warp),
            content: ClipContent::Audio {
                media: Id(9),
                offset: 0,
                gain_db: 0.0,
            },
        }
    }

    #[test]
    fn the_chip_says_the_speed_of_each_side_as_a_percentage() {
        // Half a second in a beat is the file's own speed at 120 BPM.
        let clip = warped(&[(0.0, 0), (0.5, 1), (1.5, 2)]);
        assert_eq!(
            span_words(&clip, 1).as_deref(),
            Some("Before 100% \u{00B7} after 200%")
        );
        assert_eq!(span_words(&clip, 0).as_deref(), Some("After 100%"));
        assert_eq!(span_words(&clip, 2).as_deref(), Some("Before 200%"));
        assert_eq!(
            span_words(&warped(&[(0.0, 0)]), 0).as_deref(),
            Some("Only marker")
        );
    }
}
