//! The warp markers' keyboard (M9.5 S5b): which marker has it among the Tab
//! stops drawn last frame, the keys it answers, the stops themselves in time
//! order, and bringing a marker the keys moved into view.

use eframe::egui::{self, EventFilter, Key, Modifiers, Response, Sense, Ui};

use lumen_project::edit::warp::{Refused, origin};
use lumen_project::edit::warp_markers::{MarkerEdit, held_at};
use lumen_project::model::{Clip, Id};

use super::span_words;
use super::{AT_LIMIT, OUTSIDE_FILE, stop_id};
use crate::App;
use crate::audio_editor::AudioEditor;
use crate::audio_editor::warp_strip::WarpStrip;

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

impl App {
    /// The keys taken for the focused marker, run with the clip and the view
    /// at hand.
    pub(crate) fn run_marker_keys(&mut self, clip: &Clip, ed: &mut AudioEditor) {
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
    pub(crate) fn marker_stops(
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
    pub(crate) fn reveal_marker(
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
}
