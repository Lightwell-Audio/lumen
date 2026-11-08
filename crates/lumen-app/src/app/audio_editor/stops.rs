//! The audio editor's other Tab stops (M9.5 S2b, D17 and D28): the wave,
//! the envelope while a pill is lit, and the clip's loop with its two ends.
//! Their keys are taken before the commands see them (`audio_handle_keys`),
//! and run while the editor draws, where the file and the envelope's frame
//! are at hand: an edge placed by a key lands on a zero crossing as one
//! placed by the hand does.
//!
//! - **Clip wave:** Left and Right move the edit cursor by the grid (Alt
//!   finely); the first press puts it at the playhead, if that is in the
//!   heard stretch, or at the stretch's start. Home and End go to the
//!   stretch's ends. Shift with them picks a range from the cursor, which
//!   stays its anchor while the range stands.
//! - **Envelope:** Ctrl with Left or Right picks the point before or after,
//!   the arrows move it, Delete takes it away, Enter adds one at the cursor.
//! - **Clip loop:** Enter loops the clip or stops it; on a loop's start or
//!   end, Left and Right move it by the grid.

use eframe::egui::{self, Event, EventFilter, Key, Modifiers, Rect, Response, Sense, Ui};
use lumen_core::time::Ticks;
use lumen_project::edit::{self, clip_loop::LoopEdit};
use lumen_project::model::Clip;

use super::gestures::WaveArea;
use super::range::heard;
use crate::App;
use crate::arrangement::auto_view::Frame;
use crate::arrangement::select::TimeRange;
use crate::audio_editor::AudioEditor;
use crate::audio_editor::brace::{Brace, BracePart};
use crate::widgets::raised;

/// A Tab stop of the editor that is not a handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    Ruler,
    Wave,
    Envelope,
    Loop,
    LoopStart,
    LoopEnd,
}

impl Stop {
    const ALL: [Stop; 6] = [
        Stop::Ruler,
        Stop::Wave,
        Stop::Envelope,
        Stop::Loop,
        Stop::LoopStart,
        Stop::LoopEnd,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Stop::Ruler => "ruler",
            Stop::Wave => "wave",
            Stop::Envelope => "envelope",
            Stop::Loop => "loop",
            Stop::LoopStart => "loop start",
            Stop::LoopEnd => "loop end",
        }
    }
}

/// The stable id of a stop.
pub(crate) fn stop_id(stop: Stop) -> egui::Id {
    egui::Id::new(("audio editor stop", stop.key()))
}

/// The stop with the keyboard.
pub(crate) fn focused(ctx: &egui::Context) -> Option<Stop> {
    Stop::ALL
        .into_iter()
        .find(|stop| ctx.memory(|m| m.has_focus(stop_id(*stop))))
}

/// Takes the keys `stop` answers from `events`.
fn take_keys(events: &mut Vec<Event>, stop: Stop) -> Vec<(Key, Modifiers)> {
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
        let across = matches!(key, Key::ArrowLeft | Key::ArrowRight);
        let up = matches!(key, Key::ArrowUp | Key::ArrowDown);
        let plain = modifiers == Modifiers::NONE;
        let alt = modifiers == Modifiers::ALT;
        let ours = match stop {
            Stop::Wave => {
                let shift =
                    modifiers == Modifiers::SHIFT || modifiers == Modifiers::SHIFT | Modifiers::ALT;
                (across && (plain || alt || shift))
                    || (matches!(key, Key::Home | Key::End)
                        && (plain || modifiers == Modifiers::SHIFT))
            }
            Stop::Envelope => {
                ((across || up) && (plain || alt))
                    || (across && modifiers.command_only())
                    || (plain && matches!(key, Key::Enter | Key::Delete | Key::Backspace))
            }
            Stop::Ruler => across && (plain || alt),
            Stop::Loop => plain && key == Key::Enter,
            Stop::LoopStart | Stop::LoopEnd => across && (plain || alt),
        };
        if ours {
            taken.push((key, modifiers));
        }
        !ours
    });
    taken
}

impl App {
    /// The keys of a focused stop, before the commands see them: kept for
    /// the editor's next drawing.
    pub(crate) fn audio_stop_keys(&mut self, ctx: &egui::Context) {
        let Some(stop) = focused(ctx) else {
            return;
        };
        let keys = ctx.input_mut(|i| take_keys(&mut i.events, stop));
        self.audio_editor
            .keys
            .extend(keys.into_iter().map(|(key, mods)| (stop, key, mods)));
    }

    /// The keys kept for the stops, run with the file and the envelope at
    /// hand.
    pub(super) fn run_stop_keys(
        &mut self,
        ctx: &egui::Context,
        clip: &Clip,
        (area, frame): (&WaveArea<'_>, &Frame),
        ed: &mut AudioEditor,
    ) {
        let keys = std::mem::take(&mut ed.keys);
        let mut envelope = Vec::new();
        for (stop, key, mods) in keys {
            match stop {
                Stop::Ruler => self.ruler_key(ed.time.zoom, key, mods),
                Stop::Wave => self.wave_key(ctx, clip, key, mods, area, ed),
                Stop::Envelope => envelope.push((key, mods)),
                Stop::Loop => {
                    if self.opened_clip_takes_edits(clip.id) {
                        self.toggle_clip_loop(clip);
                    }
                }
                Stop::LoopStart | Stop::LoopEnd => {
                    self.loop_edge_key(ed.time.zoom, clip, stop == Stop::LoopStart, key, mods);
                }
            }
        }
        if !envelope.is_empty() {
            self.envelope_keys(clip, frame, ed, &envelope);
        }
    }

    /// A key on the wave: the cursor and the range (D28).
    fn wave_key(
        &mut self,
        ctx: &egui::Context,
        clip: &Clip,
        key: Key,
        mods: Modifiers,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) {
        let (from, to) = heard(area.file);
        if to <= from {
            return;
        }
        let head = self.transport.playhead();
        let first = if (from..=to).contains(&head) {
            head
        } else {
            from
        };
        let fine = mods.alt;
        let zoom = ed.time.zoom;
        let step = |app: &Self, ed: &mut AudioEditor, t: Ticks, direction: i64| {
            let moved = app.key_step(zoom, t, direction, fine).clamp(from, to);
            app.crossing_at(ctx, ed, area, moved, fine)
                .0
                .clamp(from, to)
        };
        let direction = match key {
            Key::ArrowLeft => -1,
            Key::ArrowRight => 1,
            _ => 0,
        };
        if mods.shift {
            let anchor = ed.anchor.or(ed.cursor).unwrap_or(first);
            ed.anchor = Some(anchor);
            ed.cursor = Some(anchor);
            let moving = self
                .range_in_editor(clip.id, area.file)
                .map(|(a, b)| {
                    super::range::ticks_of(if (a - anchor.as_quarters()).abs() < 1e-9 {
                        b
                    } else {
                        a
                    })
                })
                .unwrap_or(anchor);
            let end = match key {
                Key::Home => from,
                Key::End => to,
                _ => step(self, ed, moving, direction),
            };
            let Some((_, track)) = self.clip_track_index(clip.id) else {
                return;
            };
            let range = TimeRange::over(self.document.project(), anchor, end, &[track]);
            self.arrangement.clips.clear();
            self.arrangement.selected =
                range.as_ref().map(|r| r.tracks.clone()).unwrap_or_default();
            self.arrangement.range = range;
            return;
        }
        let at = match (key, ed.cursor) {
            (Key::Home, _) => from,
            (Key::End, _) => to,
            (_, None) => first,
            (_, Some(at)) => step(self, ed, at, direction),
        };
        ed.cursor = Some(at);
        ed.anchor = None;
        self.select_opened_clip(clip.id);
    }

    /// Left or Right on a loop's start or end: it moves by the grid, one
    /// undo step a press.
    fn loop_edge_key(&mut self, zoom: f64, clip: &Clip, start: bool, key: Key, mods: Modifiers) {
        if !self.opened_clip_takes_edits(clip.id) {
            return;
        }
        let Some(turn) = edit::clip_loop::turn(&self.transport.timeline.tempo, clip) else {
            return;
        };
        let direction = if key == Key::ArrowLeft { -1 } else { 1 };
        let edge = if start { turn.origin } else { turn.end };
        let to = self.key_step(zoom, edge, direction, mods.alt);
        let change = if start {
            LoopEdit::Start(to)
        } else {
            LoopEdit::End(to)
        };
        let base = self.document.project().clone();
        self.document.set_clip_loop(&base, clip.id, change, None);
        self.select_opened_clip(clip.id);
    }

    /// The stops' places and names, every frame: the wave first, before the
    /// handles. Returns its response when it has the keyboard.
    pub(super) fn wave_stop(
        &self,
        ui: &mut Ui,
        body: Rect,
        clip: &Clip,
        ed: &AudioEditor,
    ) -> Option<Response> {
        let response = ui.interact(body, stop_id(Stop::Wave), Sense::focusable_noninteractive());
        let name = match ed.cursor {
            Some(at) => format!(
                "Clip wave, cursor at {}",
                crate::position::format_bbt(&self.transport.timeline, at)
            ),
            None => format!("Clip wave: {}", clip.name),
        };
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, name.clone()));
        let holds = ed.cursor.is_some() || self.arrangement.range.is_some();
        lock_arrows(ui, &response, holds);
        response.has_focus().then_some(response)
    }

    /// The envelope's stop (after the handles), and the loop's.
    pub(super) fn later_stops(
        &self,
        ui: &mut Ui,
        clip: &Clip,
        (body, brace): (Rect, &Brace),
        ed: &AudioEditor,
    ) -> Option<(Stop, Response)> {
        let mut focused = None;
        if let Some(kind) = ed.pill {
            let response = ui.interact(
                body.shrink(raised::FOCUS_OUTSET),
                stop_id(Stop::Envelope),
                Sense::focusable_noninteractive(),
            );
            let count = clip.envelopes.get(kind).map_or(0, |l| l.len());
            let name = format!("{} envelope, {count} points", kind.name());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Other, true, name.clone())
            });
            lock_arrows(ui, &response, ed.hands.picks.is_some());
            if response.has_focus() {
                focused = Some((Stop::Envelope, response));
            }
        }
        let looping = brace.ends.is_some();
        let on = if looping { "on" } else { "off" };
        let response = ui.interact(
            brace.band.shrink(1.0),
            stop_id(Stop::Loop),
            Sense::focusable_noninteractive(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                true,
                looping,
                format!("Clip loop {on}"),
            )
        });
        if response.has_focus() {
            focused = Some((Stop::Loop, response));
        }
        if looping && let Some(turn) = edit::clip_loop::turn(&self.transport.timeline.tempo, clip) {
            for (stop, part, at, label) in [
                (
                    Stop::LoopStart,
                    BracePart::Start,
                    turn.origin,
                    "Loop start at",
                ),
                (Stop::LoopEnd, BracePart::End, turn.end, "Loop end at"),
            ] {
                let Some(rect) = brace.end_rect(part) else {
                    continue;
                };
                let rect = rect.intersect(brace.band.shrink(1.0));
                let response = ui.interact(rect, stop_id(stop), Sense::focusable_noninteractive());
                let name = format!(
                    "{label} {}",
                    crate::position::format_bbt(&self.transport.timeline, at)
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, name.clone())
                });
                lock_arrows(ui, &response, false);
                if response.has_focus() {
                    focused = Some((stop, response));
                }
            }
        }
        focused
    }
}

/// While `response` has the keyboard the arrows are its own, not egui's for
/// moving the focus; with `escape`, Esc is too, so it clears what the stop
/// holds (the range, then the cursor; the picks) and the stop keeps the
/// keyboard.
fn lock_arrows(ui: &Ui, response: &Response, escape: bool) {
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape,
                    ..EventFilter::default()
                },
            );
        });
    }
}

/// The brace's part a loop stop stands for.
pub(super) fn brace_part(stop: Stop) -> BracePart {
    match stop {
        Stop::LoopStart => BracePart::Start,
        Stop::LoopEnd => BracePart::End,
        _ => BracePart::Band,
    }
}

impl App {
    /// The ruler's Tab stop (M9.5 S2c): Left and Right move the playhead by
    /// the grid, and with Alt finely.
    pub(super) fn ruler_stop(&self, ui: &mut Ui, scale: Rect) -> Option<Response> {
        let response = ui.interact(
            scale,
            stop_id(Stop::Ruler),
            Sense::focusable_noninteractive(),
        );
        let name = format!(
            "Editor ruler, playhead at {}",
            crate::position::format_bbt(&self.transport.timeline, self.transport.position())
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, name.clone())
        });
        lock_arrows(ui, &response, false);
        response.has_focus().then_some(response)
    }

    /// Left or Right on the ruler: the playhead moves by the grid.
    fn ruler_key(&mut self, zoom: f64, key: Key, mods: Modifiers) {
        let direction = if key == Key::ArrowLeft { -1 } else { 1 };
        let at = self.key_step(zoom, self.transport.position(), direction, mods.alt);
        self.transport.locate(at.max(Ticks::ZERO));
        self.transport.follow_paused = true;
    }
}
