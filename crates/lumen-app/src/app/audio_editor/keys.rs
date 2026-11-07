//! The audio editor's handles from the keyboard (M9.5 D17, D18): each is a
//! Tab stop with a name that says its value. Left and Right move it by the
//! grid, Up and Down set a gain or bend a fade, Alt goes finely, Enter types
//! the value, and Delete takes a fade away or puts the gain back. Every press
//! is one undo step.

use eframe::egui::{self, Event, EventFilter, Key, Modifiers, Popup, Response, Sense, Ui};
use lumen_core::curve::{self, Curve};
use lumen_core::fade::Fade;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::edit;
use lumen_project::model::{Clip, ClipContent, Id};

use crate::App;
use crate::app::clip_commands::FINE;
use crate::arrangement::handles::Handle;
use crate::arrangement::select::Edge;
use crate::audio_editor::handles::{EditorHandles, Part, STOPS, parse_db, parse_length};
use crate::audio_editor::{AudioEditor, Popover};
use crate::widgets::{self, raised};
use crate::{position, theme};

/// How far a press bends a fade: the dot's level, finely with Alt.
const BEND_STEP: f32 = 0.05;
const BEND_FINE: f32 = 0.01;
/// How far a press moves the gain line, in dB, finely with Alt.
const GAIN_STEP: f32 = 1.0;
const GAIN_FINE: f32 = 0.1;
/// The typed value's field.
const FIELD_WIDTH: f32 = 140.0;
/// The widest a popover's message gets before it wraps.
const POPOVER_WIDTH: f32 = 240.0;

/// The stable id of a handle's Tab stop, seen every frame even scrolled away.
pub(crate) fn stop_id(part: Part) -> egui::Id {
    egui::Id::new(("audio editor handle", part.key()))
}

/// The handle with the keyboard.
pub(crate) fn focused_stop(ctx: &egui::Context) -> Option<Part> {
    STOPS
        .into_iter()
        .find(|part| ctx.memory(|m| m.has_focus(stop_id(*part))))
}

/// Takes the keys a focused handle answers from `events`: the arrows, with
/// nothing or Alt held, Enter, and Delete. An arrow it has no use for does
/// not reach anything else (Alt and Up would move the track).
fn take_keys(events: &mut Vec<Event>) -> Vec<(Key, bool)> {
    let mut taken = Vec::new();
    events.retain(|event| {
        let Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = *event
        else {
            return true;
        };
        let arrow = matches!(
            key,
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown
        );
        let plain = modifiers == Modifiers::NONE;
        let ours = (arrow && (plain || modifiers == Modifiers::ALT))
            || (plain && matches!(key, Key::Enter | Key::Delete | Key::Backspace));
        if ours {
            taken.push((key, modifiers.alt));
        }
        !ours
    });
    taken
}

impl App {
    /// The keys of a focused handle, before the commands see them.
    pub(crate) fn audio_handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused()
            || self.typing
            || self.rebind.is_some()
            || self.window_open()
            || Popup::is_any_open(ctx)
        {
            return;
        }
        let (Some(part), Some(id)) = (focused_stop(ctx), self.detail_audio_clip()) else {
            return;
        };
        for (key, fine) in ctx.input_mut(|i| take_keys(&mut i.events)) {
            self.handle_key(ctx, id, part, key, fine);
        }
    }

    fn handle_key(&mut self, ctx: &egui::Context, id: Id, part: Part, key: Key, fine: bool) {
        let across = match key {
            Key::ArrowLeft => -1,
            Key::ArrowRight => 1,
            _ => 0,
        };
        let up = match key {
            Key::ArrowUp => 1.0,
            Key::ArrowDown => -1.0,
            _ => 0.0,
        };
        let enter = key == Key::Enter;
        let delete = matches!(key, Key::Delete | Key::Backspace);
        let acts = match part {
            Part::Trim(_) => across != 0 || enter,
            Part::Handle(Handle::Gain) => up != 0.0 || enter || delete,
            Part::Handle(_) => across != 0 || up != 0.0 || enter || delete,
        };
        if !acts || !self.opened_clip_takes_edits(id) {
            return;
        }
        if enter {
            self.open_audio_popover(ctx, id, part);
            return;
        }
        self.select_opened_clip(id);
        match part {
            Part::Trim(edge) => self.trim_by_key(id, edge, across, fine),
            Part::Handle(Handle::Gain) if delete => {
                self.reset_clip_handle(id, Handle::Gain);
            }
            Part::Handle(Handle::Gain) => {
                let step = if fine { GAIN_FINE } else { GAIN_STEP };
                self.gain_by_key(id, up * step);
            }
            Part::Handle(Handle::Fade(edge) | Handle::Bend(edge)) => {
                if delete {
                    self.reset_clip_handle(id, Handle::Fade(edge));
                } else if across != 0 {
                    self.fade_by_key(id, edge, across, fine);
                } else {
                    let step = if fine { BEND_FINE } else { BEND_STEP };
                    self.bend_by_key(id, edge, up * step);
                }
            }
        }
        self.audio_editor.reveal = Some(part);
    }

    /// The grid line next to `t` in `direction`, or a fine step from it.
    fn key_step(&self, t: Ticks, direction: i64, fine: bool) -> Ticks {
        if fine {
            return t + Ticks(FINE.0 * direction);
        }
        let zoom = self.audio_editor.time.zoom;
        if direction > 0 {
            return self.grid_cell_at_zoom(t, zoom).1;
        }
        let (from, _) = self.grid_cell_at_zoom(t, zoom);
        if from < t {
            from
        } else {
            self.grid_cell_at_zoom((t - Ticks(1)).max(Ticks::ZERO), zoom)
                .0
        }
    }

    fn trim_by_key(&mut self, id: Id, edge: Edge, direction: i64, fine: bool) {
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let side = match edge {
            Edge::Start => clip.start,
            Edge::End => clip.start + clip.length,
        };
        let to = self.key_step(side, direction, fine);
        let base = self.document.project().clone();
        self.document.trim_clip(&base, id, edge, to, false, None);
    }

    /// Where the handle of the fade at `edge` is now: where a drag would put
    /// it to leave it as it is.
    fn fade_handle_at(&self, clip: &Clip, edge: Edge) -> Ticks {
        let (fade, incoming) = self.fade_of(clip, edge);
        match edge {
            // The clip a crossfade runs into starts where it ends.
            Edge::Start if incoming => clip.start,
            Edge::Start => clip.start + fade.length,
            Edge::End => clip.start + clip.length - fade.length,
        }
    }

    /// The fade at `edge` of `shown` (as `App::shown_clip` gives it) and
    /// whether a crossfade runs into the clip there. The preview marks the
    /// fade-out side: its fade-out is the crossfade out of it, and the
    /// `crossfade` flag says that one is there; an incoming clip's fade-in is
    /// cleared. So the fade-in side is read from the clip as saved, where
    /// `crossfade` means one runs in and the fade-in is its length and curve.
    fn fade_of(&self, shown: &Clip, edge: Edge) -> (Fade, bool) {
        match edge {
            Edge::Start => self.clip(shown.id).map_or_else(
                || (shown.fade_in, false),
                |saved| (saved.fade_in, saved.crossfade),
            ),
            Edge::End => (shown.fade_out, false),
        }
    }

    fn fade_by_key(&mut self, id: Id, edge: Edge, direction: i64, fine: bool) {
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let now = self.fade_handle_at(&clip, edge);
        let at = self.key_step(now, direction, fine);
        let from = self.fade_begins(id, edge);
        self.fade_to(id, edge, from, at, None);
    }

    fn bend_by_key(&mut self, id: Id, edge: Edge, by: f32) {
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let (fade, _) = self.fade_of(&clip, edge);
        let handle = fade.curve.handle();
        let [u, v] = handle.dot;
        let bent = curve::Handle {
            dot: [u, (v + by).clamp(curve::DOT_MIN, curve::DOT_MAX)],
            ..handle
        };
        self.document
            .set_fade_curve(&[id], edge, Curve::from_handle(bent), None);
    }

    fn gain_by_key(&mut self, id: Id, by: f32) {
        let Some(ClipContent::Audio { gain_db, .. }) = self.clip(id).map(|c| c.content.clone())
        else {
            return;
        };
        let range = crate::document::CLIP_GAIN;
        let db = (gain_db + by).clamp(*range.start(), *range.end());
        self.document.set_clip_gain(&[id], db, None);
    }

    /// Slides the opened clip's audio by one grid step of the editor (the
    /// keyboard's slip, M9.5 D17).
    pub(crate) fn slip_opened_clip(&mut self, direction: i64) {
        let Some(id) = self.detail_audio_clip() else {
            return;
        };
        let Some(start) = self.clip(id).map(|c| c.start) else {
            return;
        };
        if !self.opened_clip_takes_edits(id) {
            return;
        }
        let (from, to) = self.grid_cell_at_zoom(start, self.audio_editor.time.zoom);
        let base = self.document.project().clone();
        self.document
            .slip_clip(&base, id, Ticks((to.0 - from.0) * direction), None);
        self.select_opened_clip(id);
    }

    /// "Trim start at bar 3 beat 1", "Fade in 125 ms, bent 30%", "Clip gain
    /// −3.0 dB": a handle's accessible name, with its value (M9.5 D18).
    pub(super) fn stop_name(&self, clip: &Clip, part: Part) -> String {
        let place = |t: Ticks| {
            let bbt = self.transport.timeline.meter.bbt_at(t);
            let sixteenth = bbt.ticks.0 / (PPQ / 4);
            let beat = format!("bar {} beat {}", bbt.bar + 1, bbt.beat + 1);
            if sixteenth > 0 {
                format!("{beat} sixteenth {}", sixteenth + 1)
            } else {
                beat
            }
        };
        match part {
            Part::Trim(Edge::Start) => format!("Trim start at {}", place(clip.start)),
            Part::Trim(Edge::End) => format!("Trim end at {}", place(clip.start + clip.length)),
            Part::Handle(Handle::Gain) => {
                let db = match clip.content {
                    ClipContent::Audio { gain_db, .. } => gain_db,
                    _ => 0.0,
                };
                format!("Clip gain {}", widgets::db_text(db))
            }
            Part::Handle(Handle::Fade(edge) | Handle::Bend(edge)) => self.fade_name(clip, edge),
        }
    }

    fn fade_name(&self, clip: &Clip, edge: Edge) -> String {
        let label = match edge {
            Edge::Start => "Fade in",
            Edge::End => "Fade out",
        };
        let (fade, incoming) = self.fade_of(clip, edge);
        if edge == Edge::End && self.crossing_into(clip.id).is_some() {
            return format!("{label}: {}", self.crossing_text(clip.id));
        }
        if incoming {
            return format!("{label}: {}", self.crossfade_text(clip.id));
        }
        if fade.length <= Ticks::ZERO {
            return format!("{label} none");
        }
        let [u, v] = fade.curve.handle().dot;
        let room = curve::DOT_MAX - curve::DOT_MIN;
        let bent = ((v - u).abs() / room * 100.0).round();
        let length = self.fade_text(clip.id, edge);
        if bent >= 1.0 {
            format!("{label} {length}, bent {bent:.0}%")
        } else {
            format!("{label} {length}")
        }
    }

    /// The handles' Tab stops: each has its id and name every frame, focus
    /// or not, wherever the handle is. Returns the one with the keyboard and
    /// its response.
    pub(super) fn audio_stops(
        &mut self,
        ui: &mut Ui,
        id: Id,
        handles: &EditorHandles,
        ed: &mut AudioEditor,
    ) -> Option<(Part, Response)> {
        let clip = self.shown_clip(id)?;
        let mut focused = None;
        for part in STOPS {
            let rect = handles.stop_rect(part);
            let response = ui.interact(rect, stop_id(part), Sense::focusable_noninteractive());
            let name = self.stop_name(&clip, part);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, name.clone())
            });
            if response.has_focus() {
                // The arrows are its own, not egui's for moving the focus.
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        response.id,
                        EventFilter {
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            ..EventFilter::default()
                        },
                    );
                });
                if ed.focused_before != Some(part) {
                    ed.reveal = Some(part);
                    ui.ctx().request_repaint();
                }
                focused = Some((part, response.clone()));
            }
            self.audio_popover(ui, id, part, &response, ed);
        }
        ed.focused_before = focused.as_ref().map(|(part, _)| *part);
        focused
    }

    /// Enter on a handle: its value in a field, under it.
    fn open_audio_popover(&mut self, ctx: &egui::Context, id: Id, part: Part) {
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let text = self.popover_text(&clip, part);
        self.audio_editor.popover = Some(Popover {
            part,
            initial: text.clone(),
            text,
            error: None,
            focused: false,
        });
        widgets::Menu::open_for(ctx, stop_id(part));
    }

    /// The value as the field starts with it.
    fn popover_text(&self, clip: &Clip, part: Part) -> String {
        match part {
            Part::Trim(edge) => {
                let side = match edge {
                    Edge::Start => clip.start,
                    Edge::End => clip.start + clip.length,
                };
                position::format_bbt(&self.transport.timeline, side)
            }
            Part::Handle(Handle::Gain) => match clip.content {
                ClipContent::Audio { gain_db, .. } => format!("{gain_db:.1}"),
                _ => String::new(),
            },
            Part::Handle(Handle::Fade(edge) | Handle::Bend(edge)) => {
                let (fade, incoming) = self.fade_of(clip, edge);
                let length = fade.length;
                let from = match edge {
                    // A crossfade into the clip ends at its start.
                    Edge::Start if incoming => clip.start - length,
                    Edge::Start => clip.start,
                    Edge::End => clip.start + clip.length - length,
                };
                let tempo = edit::tempo_of(self.document.project());
                let seconds = tempo.seconds_at(from + length) - tempo.seconds_at(from);
                format!("{:.0} ms", seconds * 1_000.0)
            }
        }
    }

    /// Draws the popover of `part` while it is open, and when it closes gives
    /// the keyboard back to the handle.
    fn audio_popover(
        &mut self,
        ui: &Ui,
        id: Id,
        part: Part,
        response: &Response,
        ed: &mut AudioEditor,
    ) {
        if ed.popover.as_ref().is_none_or(|p| p.part != part) {
            return;
        }
        let ctx = ui.ctx().clone();
        let Some(mut popover) = ed.popover.take() else {
            return;
        };
        let done = widgets::Menu::below(response)
            .show(|ui| self.popover_contents(ui, id, &mut popover))
            .unwrap_or(false);
        if done {
            widgets::Menu::close_for(&ctx, response.id);
        }
        if done || !widgets::Menu::is_open_for(&ctx, response.id) {
            // A click elsewhere is leaving the field; Esc is not.
            let esc = ctx.input(|i| i.key_pressed(Key::Escape));
            if !done && !esc {
                // A value that can't be read has nowhere left to be said but
                // the notice.
                if let Err(why) = self.apply_typed(id, &popover) {
                    self.say(why);
                }
            }
            response.request_focus();
            return;
        }
        ed.popover = Some(popover);
    }

    /// The field, what is wrong with it, and on a fade its shapes. True when
    /// the value is in and the popover should close.
    fn popover_contents(&mut self, ui: &mut Ui, id: Id, popover: &mut Popover) -> bool {
        let part = popover.part;
        let name = match part {
            Part::Trim(Edge::Start) => "Trim start at",
            Part::Trim(Edge::End) => "Trim end at",
            Part::Handle(Handle::Gain) => "Clip gain in dB",
            Part::Handle(Handle::Fade(Edge::Start) | Handle::Bend(Edge::Start)) => "Fade in length",
            Part::Handle(Handle::Fade(Edge::End) | Handle::Bend(Edge::End)) => "Fade out length",
        };
        ui.set_max_width(POPOVER_WIDTH);
        widgets::menu_caption(ui, name);
        let edit_id = stop_id(part).with("typed");
        let field = ui.add(
            egui::TextEdit::singleline(&mut popover.text)
                .id(edit_id)
                .desired_width(FIELD_WIDTH)
                .font(egui::FontId::monospace(theme::size::CAPTION)),
        );
        field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
        if !popover.focused {
            field.request_focus();
            if field.has_focus() {
                popover.focused = true;
                let mut state =
                    egui::text_edit::TextEditState::load(ui.ctx(), edit_id).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(popover.text.chars().count()),
                    )));
                state.store(ui.ctx(), edit_id);
            }
            ui.ctx().request_repaint();
        }
        if field.changed() {
            popover.error = None;
        }
        let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        let mut done = false;
        if field.lost_focus() && !esc {
            match self.apply_typed(id, popover) {
                Ok(()) => {
                    popover.initial.clone_from(&popover.text);
                    done = enter;
                }
                Err(why) => {
                    popover.error = Some(why);
                    field.request_focus();
                }
            }
        }
        if let Some(error) = &popover.error {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(error)
                        .size(theme::size::CAPTION)
                        .color(theme::p().warning),
                )
                .wrap(),
            );
        }
        if let Some(edge) = part.fade_edge() {
            ui.separator();
            self.fade_shape_rows(ui, id, edge);
        } else if raised::button(ui, "Done").clicked() {
            match self.apply_typed(id, popover) {
                Ok(()) => done = true,
                Err(why) => popover.error = Some(why),
            }
        }
        done
    }

    /// Takes the popover's text as the handle's new value. Nothing happens,
    /// and no undo step is made, when it was not changed.
    fn apply_typed(&mut self, id: Id, popover: &Popover) -> Result<(), String> {
        if popover.text.trim() == popover.initial.trim() {
            return Ok(());
        }
        self.select_opened_clip(id);
        match popover.part {
            Part::Trim(edge) => {
                let to = position::parse(&popover.text, &self.transport.timeline)?;
                let base = self.document.project().clone();
                self.document.trim_clip(&base, id, edge, to, false, None);
            }
            Part::Handle(Handle::Gain) => {
                let range = crate::document::CLIP_GAIN;
                let db = parse_db(&popover.text)?.clamp(*range.start(), *range.end());
                self.document.set_clip_gain(&[id], db, None);
            }
            Part::Handle(Handle::Fade(edge) | Handle::Bend(edge)) => {
                let seconds = parse_length(&popover.text)?;
                self.fade_typed(id, edge, seconds);
            }
        }
        Ok(())
    }

    /// The fade at `edge` set to last `seconds`, wherever the tempo puts that.
    fn fade_typed(&mut self, id: Id, edge: Edge, seconds: f64) {
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let tempo = edit::tempo_of(self.document.project());
        let from = self.fade_begins(id, edge);
        let at = match (edge, from) {
            // A crossfade into this clip ends that long after it begins.
            (Edge::Start, Some(begin)) => tempo.ticks_at_seconds(tempo.seconds_at(begin) + seconds),
            (Edge::Start, None) => tempo.ticks_at_seconds(tempo.seconds_at(clip.start) + seconds),
            (Edge::End, _) => {
                let end = clip.start + clip.length;
                tempo.ticks_at_seconds((tempo.seconds_at(end) - seconds).max(0.0))
            }
        };
        self.fade_to(id, edge, from, at, None);
    }
}
