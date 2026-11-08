//! A clip's envelope in the audio editor (M9.5 S2b, D20 and the owner's DR
//! A): the Gain and Pan pills show one envelope over the wave, and the
//! arrangement's lane gestures edit it. Pointer adds, picks and moves points
//! (Shift picks several or a box); Draw is the pencil, thinned as it goes.
//! The trim grips and fade handles stay live, and Ctrl reaches the wave's
//! own gestures. The editor keeps its own gesture, picks and hover
//! (`AudioEditor::hands`), so the arrangement's lanes keep theirs.
//!
//! A point is drawn where it is heard (`auto_view::EnvelopeClock`): at its
//! seconds from the clock the engine reads it on, measured from the origin
//! the editor lays the file from.

use std::sync::Arc;

use eframe::egui::{
    self, CursorIcon, Modifiers, Painter, PointerButton, Pos2, Rect, Response, Stroke, vec2,
};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::edit::clip_loop;
use lumen_project::envelope::Kind;
use lumen_project::model::{Clip, Id};

use super::gestures::WaveArea;
use crate::App;
use crate::app::info::{Held, Info, Mouse};
use crate::arrangement::auto_state::{Picked, Tool as LaneTool};
use crate::arrangement::auto_view::{EnvelopeClock, Frame, draw_point, polyline};
use crate::audio_editor::AudioEditor;
use crate::audio_editor::handles::{self, EditorHandles};
use crate::audio_editor::source::WholeFile;
use crate::commands::Command;
use crate::note_editor::Tool;
use crate::theme;
use crate::timeview::TimeView;

impl App {
    /// The envelope the pills show for `clip`: its track and target.
    pub(super) fn envelope_target(&self, ed: &AudioEditor, clip: Id) -> Option<(Id, Target)> {
        let kind = ed.pill?;
        let (_, track) = self.clip_track_index(clip)?;
        Some((track, Target::envelope(clip, kind)))
    }

    /// The envelope's frame over the wave in `body`: its values fill the
    /// wave's height, and its points are drawn where they are heard.
    pub(crate) fn envelope_frame(&self, clip: &Clip, body: Rect, time: TimeView) -> Frame {
        let tempo = self.transport.timeline.tempo.clone();
        let (clock, origin_seconds) = match clip_loop::turn(&tempo, clip) {
            Some(turn) => (clip.start - clip.loop_phase, turn.origin_seconds),
            None => (clip.start, tempo.seconds_at(clip.start)),
        };
        let mut frame = Frame::of_row(body, body.left(), time);
        frame.values = Some(body.shrink2(vec2(0.0, theme::size::AUTO_PAD)));
        frame.clock = Some(Arc::new(EnvelopeClock {
            tempo,
            clock,
            start: clip.start,
            origin_seconds,
        }));
        frame
    }

    /// The envelope's pointer work, before the wave's: returns whether it
    /// took a press, a click or a drag. A press on a trim grip or fade
    /// handle, or with Ctrl, is left to the wave. Over the envelope it says
    /// what it does (`envelope_hover`), and the wave says nothing.
    pub(super) fn envelope_pointer(
        &mut self,
        response: &Response,
        clip: &Clip,
        (frame, handles): (&Frame, &EditorHandles),
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) -> bool {
        let ctx = response.ctx.clone();
        let modifiers = ctx.input(|i| i.modifiers);
        ed.envelope_hover = false;
        ed.hands.tool = match ed.tool {
            Tool::Pointer => LaneTool::Pointer,
            Tool::Draw => LaneTool::Pencil,
        };
        if let Some(active) = ed.hands.active.as_ref() {
            let (track, target) = (active.track, active.target);
            let down = ctx.input(|i| i.pointer.primary_down());
            if down && let Some(p) = ctx.input(|i| i.pointer.latest_pos()) {
                self.envelope_edge_scroll(&ctx, p, area, ed);
                let frame = self.envelope_frame(clip, area.body, ed.time);
                let overlay = ctx
                    .layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("audio editor envelope"),
                    ))
                    .with_clip_rect(area.body);
                let mut hands = std::mem::take(&mut ed.hands);
                if self.envelope_target(ed, clip.id) == Some((track, target)) {
                    self.lane_update(
                        &mut hands,
                        Some((&overlay, area.body)),
                        p,
                        &frame,
                        modifiers,
                    );
                }
                ed.hands = hands;
            }
            if !down || response.drag_stopped() {
                Self::lane_finish(&mut ed.hands);
            }
            return true;
        }
        let Some((track, target)) = self.envelope_target(ed, clip.id) else {
            ed.hover_point = None;
            return false;
        };
        let on_handle = |p: Pos2| handles.part_at(p).is_some();
        if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ctx.input(|i| i.pointer.press_origin())
            && !modifiers.command
            && !on_handle(origin)
        {
            if self.opened_clip_takes_edits(clip.id) {
                let mut hands = std::mem::take(&mut ed.hands);
                self.lane_begin(&mut hands, track, target, frame, origin, modifiers);
                ed.hands = hands;
                ed.cursor = None;
            }
            return true;
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
            && !modifiers.command
            && !on_handle(p)
        {
            if ed.tool == Tool::Pointer && self.opened_clip_takes_edits(clip.id) {
                let mut hands = std::mem::take(&mut ed.hands);
                self.lane_click(&mut hands, track, target, frame, p, modifiers);
                ed.hands = hands;
            }
            return true;
        }
        ed.hover_point = None;
        let Some(p) = response.hover_pos() else {
            return false;
        };
        if modifiers.command || on_handle(p) {
            return false;
        }
        let lane = self.lane_points_of(track, target);
        ed.hover_point = lane
            .as_ref()
            .filter(|_| ed.tool == Tool::Pointer)
            .and_then(|l| frame.nearest(target, l, p));
        let name = self
            .arrangement
            .auto
            .name_of(self.document.project(), target);
        let info = match (ed.tool, ed.hover_point.zip(lane.as_ref())) {
            (Tool::Pointer, Some((i, l))) => {
                ctx.set_cursor_icon(CursorIcon::Grab);
                Info::new(format!(
                    "{name} point \u{00B7} {}",
                    self.arrangement
                        .auto
                        .value_text(target, l.points()[i].value)
                ))
                .on(Held::NONE, Mouse::Drag, "move")
                .on(Held::NONE, Mouse::Click, "pick")
                .on(Held::SHIFT, Mouse::Click, "add to the picks")
                .key(Command::Delete, "delete")
            }
            (Tool::Pointer, None) => {
                ctx.set_cursor_icon(CursorIcon::Crosshair);
                Info::new(format!("{name} envelope"))
                    .on(Held::NONE, Mouse::Click, "add a point")
                    .on(Held::NONE, Mouse::Drag, "add a point and move it")
                    .on(Held::SHIFT, Mouse::Drag, "pick the points in a box")
                    .on(Held::ALT, Mouse::Click, "add it off the grid")
                    .on(Held::CTRL, Mouse::Drag, "pick a range")
            }
            (Tool::Draw, _) => {
                ctx.set_cursor_icon(CursorIcon::Crosshair);
                Info::new(format!("{name} envelope \u{00B7} pencil"))
                    .on(Held::NONE, Mouse::Drag, "draw")
                    .on(Held::CTRL, Mouse::Drag, "pick a range")
                    .key(Command::DrawTool, "back to the pointer")
            }
        };
        self.set_info(info);
        ed.envelope_hover = true;
        false
    }

    /// A drag past the wave's side scrolls the view (D29), as a trim does.
    fn envelope_edge_scroll(
        &mut self,
        ctx: &egui::Context,
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
    }

    /// The envelope over the wave: a wash over the part it applies to, the
    /// rest line, its curve and its points.
    pub(super) fn paint_envelope(
        &self,
        painter: &Painter,
        clip: &Clip,
        frame: &Frame,
        file: &WholeFile,
        ed: &AudioEditor,
    ) {
        let Some((track, target)) = self.envelope_target(ed, clip.id) else {
            return;
        };
        let body = frame.row;
        let palette = theme::p();
        let ink = palette.accent;
        // The part the envelope applies to: one turn of a loop, or the clip.
        let (a, b) = if clip.loop_length.is_some_and(|p| p > Ticks::ZERO) {
            file.loop_window
        } else {
            (
                clip.start.as_quarters(),
                (clip.start + clip.length).as_quarters(),
            )
        };
        let x = |q: f64| frame.view.x(frame.canvas_left, q);
        let (left, right) = (x(a).max(body.left()), x(b).min(body.right()));
        if right <= left {
            return;
        }
        let span = Rect::from_x_y_ranges(left..=right, body.y_range());
        let painter = painter.with_clip_rect(span.intersect(painter.clip_rect()));
        painter.rect_filled(span, 0.0, ink.gamma_multiply(0.05));
        painter.hline(
            left..=right,
            frame.y(target, target.default_value()),
            Stroke::new(
                theme::stroke::HAIRLINE,
                palette.border_strong.gamma_multiply(0.6),
            ),
        );
        let lane = self.lane_points_of(track, target);
        let Some(lane) = lane.as_ref() else {
            return;
        };
        let line = polyline(frame, target, lane, (left, right));
        if line.len() >= 2 {
            painter.add(egui::Shape::line(
                line,
                Stroke::new(theme::stroke::LIT, ink),
            ));
        }
        let picks = ed.hands.picks.as_ref().filter(|p| p.on(track, target));
        let moving = ed.hands.active.as_ref().and_then(|a| match &a.how {
            crate::arrangement::auto_state::Gesture::Move(m) => Some(m.anchor),
            _ => None,
        });
        let points = lane.points();
        let first = points.partition_point(|p| frame.x(p.at) < left - 8.0);
        for (i, point) in points.iter().enumerate().skip(first) {
            let at = frame.pos(target, point);
            if at.x > right + 8.0 {
                break;
            }
            let picked = picks.is_some_and(|p| p.indices.contains(&i));
            let hot = ed.hover_point == Some(i) || moving == Some(i);
            draw_point(&painter, at, ink, picked, hot);
        }
    }

    /// The envelope's keyboard stop (D28): Ctrl with Left or Right picks the
    /// point before or after, the arrows move the picked one, Delete takes it
    /// away, and Enter adds a point at the cursor, or the playhead, holding
    /// the envelope's value there. Each press is one undo step.
    pub(super) fn envelope_keys(
        &mut self,
        clip: &Clip,
        frame: &Frame,
        ed: &mut AudioEditor,
        keys: &[(egui::Key, Modifiers)],
    ) {
        let Some((track, target)) = self.envelope_target(ed, clip.id) else {
            return;
        };
        let pick = |index: usize| {
            Some(crate::arrangement::auto_state::Picks::only(Picked {
                track,
                target,
                index,
            }))
        };
        for &(key, mods) in keys {
            // Read again each press: the one before may have changed them.
            let lane = self.lane_points_of(track, target);
            let count = lane.as_ref().map_or(0, |l| l.len());
            let picked = ed
                .hands
                .picks
                .as_ref()
                .filter(|p| p.on(track, target))
                .and_then(|p| p.indices.iter().next().copied());
            let editable = self.opened_clip_takes_edits(clip.id);
            match key {
                egui::Key::ArrowLeft | egui::Key::ArrowRight if mods.command => {
                    if count > 0 {
                        let index = if key == egui::Key::ArrowRight {
                            picked.map_or(0, |i| (i + 1).min(count - 1))
                        } else {
                            picked.map_or(count - 1, |i| i.saturating_sub(1))
                        };
                        ed.hands.picks = pick(index);
                    }
                }
                egui::Key::Enter if editable => {
                    let at = ed.cursor.unwrap_or_else(|| self.transport.playhead());
                    let lane_at = frame.lane(at);
                    let value = lane
                        .as_ref()
                        .and_then(|l| l.value_at(lane_at))
                        .unwrap_or(target.default_value());
                    let mut hands = std::mem::take(&mut ed.hands);
                    self.add_lane_point(&mut hands, track, target, lane_at, value, false);
                    ed.hands = hands;
                }
                egui::Key::Delete | egui::Key::Backspace if editable => {
                    if let (Some(lane), Some(index)) = (&lane, picked) {
                        let next = crate::arrangement::auto_view::without(lane, &[index]);
                        let result = self.document.set_lane(track, target, next, false);
                        self.arrangement.note_lane(result);
                        ed.hands.picks = None;
                    }
                }
                egui::Key::ArrowLeft
                | egui::Key::ArrowRight
                | egui::Key::ArrowUp
                | egui::Key::ArrowDown
                    if editable =>
                {
                    let (time, value) = match key {
                        egui::Key::ArrowLeft => (-1, 0),
                        egui::Key::ArrowRight => (1, 0),
                        egui::Key::ArrowUp => (0, 1),
                        _ => (0, -1),
                    };
                    if let (Some(lane), Some(index)) = (&lane, picked) {
                        self.nudge_envelope_point(
                            frame,
                            (track, target),
                            lane,
                            index,
                            (time, value, mods.alt),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// Moves point `index` of `lane` by a grid step in the editor's time
    /// (`time`, earlier is negative) and a step of its height (`value`).
    fn nudge_envelope_point(
        &mut self,
        frame: &Frame,
        (track, target): (Id, Target),
        lane: &lumen_core::automation::Lane,
        index: usize,
        (time, value, fine): (i32, i32, bool),
    ) {
        use crate::arrangement::auto_scale as scale;
        let Some(point) = lane.points().get(index).copied() else {
            return;
        };
        let shown = frame.shown(point.at);
        let moved = if time > 0 {
            self.grid_cell_at_zoom(shown, frame.view.zoom).1
        } else if time < 0 {
            let (start, _) = self.grid_cell_at_zoom(shown, frame.view.zoom);
            if start < shown {
                start
            } else {
                self.grid_cell_at_zoom(shown - Ticks(1), frame.view.zoom).0
            }
        } else {
            shown
        };
        let by = if time == 0 {
            Ticks::ZERO
        } else {
            frame.lane(moved) - point.at
        };
        let step = if fine {
            crate::app::auto_keys::VALUE_STEP_FINE
        } else {
            crate::app::auto_keys::VALUE_STEP
        };
        let map = |v: f32| {
            if value == 0 {
                v
            } else {
                scale::value(target, scale::fraction(target, v) + value as f32 * step)
            }
        };
        if let Some(done) = lumen_project::edit::automation::shifted(lane, &[index], by, map) {
            let result = self.document.set_lane(track, target, done.lane, false);
            self.arrangement.note_lane(result);
        }
    }

    /// Lights a pill, or puts it out: one envelope at a time.
    pub(crate) fn audio_editor_pill(&mut self, kind: Kind) {
        self.audio_editor.toggle_pill(kind);
    }

    /// B in the editor: Pointer and Draw. Draw with no pill lit lights Gain,
    /// so the pencil always works (DR A).
    pub(crate) fn audio_editor_toggle_draw(&mut self) {
        self.audio_editor.toggle_draw();
    }
}

impl App {
    /// The editor's envelope has picked points.
    pub(crate) fn audio_envelope_picked(&self) -> bool {
        self.audio_editor.pill.is_some() && self.audio_editor.hands.picks.is_some()
    }

    /// Takes the editor's picked envelope points away (Delete).
    pub(crate) fn audio_envelope_delete(&mut self) {
        let Some(picks) = self.audio_editor.hands.picks.take() else {
            return;
        };
        let Some((clip, _)) = picks.target.clip() else {
            return;
        };
        if !self.opened_clip_takes_edits(clip) {
            return;
        }
        if let Some(lane) = self.lane_points_of(picks.track, picks.target) {
            let next = crate::arrangement::auto_view::without(&lane, &picks.list());
            let result = self
                .document
                .set_lane(picks.track, picks.target, next, false);
            self.arrangement.note_lane(result);
        }
    }
}
