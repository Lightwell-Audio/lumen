//! The editor on the screen (M5 slice 3c): toolbar, ruler, keys and notes,
//! and the pointer. See `gesture` for what a drag means and `paint` for how
//! it looks.

use std::sync::Arc;

use eframe::egui::{
    self, Align, CursorIcon, Layout, PointerButton, Pos2, Rect, Sense, Ui, UiBuilder, pos2, vec2,
};
use lumen_core::time::{PPQ, Ticks};
use lumen_project::edit::notes::{End, MIN_NOTE};
use lumen_project::model::Clip;

use super::gesture::{DRAG_THRESHOLD, Env, Gesture, Kind, Preview, Resolved, tones_of};
use super::paint::{self, Scene};
use super::view::{
    EditorView, KEY_WIDTH, ROW_DEFAULT, ROW_MAX, ROW_MIN, RULER_HEIGHT, Zone, fit_clip, pitch_name,
    scroll_to_centre,
};
use super::{EditorZoom, MenuFor, NoteEditor, NoteOp, Tool, notes_of};
use crate::app::info::{Held, Info, Mouse};
use crate::app::navigate::KEY_ZOOM;
use crate::canvas::{self, Surface};
use crate::commands::Command;
use crate::document::Resize;
use crate::piano_roll::{self, draw::RollView};
use crate::widgets::{self, Choice};
use crate::{App, theme};

/// The toolbar's height above the ruler.
const TOOLBAR: f32 = 28.0;
/// The editor needs at least this much body to be drawn.
const MIN_BODY: f32 = 24.0;

impl App {
    /// Draws the editor in `area` of `ui`: in the detail panel, or in the
    /// pop-out window.
    pub(crate) fn note_editor_surface(&mut self, ui: &mut Ui, area: Rect, surface: Surface) {
        // The editor's state is taken out while it draws, so the drawing can
        // use the rest of the app freely.
        let mut ed = std::mem::take(&mut self.note_editor);
        self.draw_editor(ui, area, surface, &mut ed);
        self.note_editor = ed;
    }

    fn draw_editor(&mut self, ui: &mut Ui, area: Rect, surface: Surface, ed: &mut NoteEditor) {
        let ctx = ui.ctx().clone();
        let Some((clip, colour)) = ed.clip.and_then(|id| self.editor_clip(id)) else {
            ed.hovered = false;
            let state = widgets::EmptyState {
                icon: None,
                icon_size: theme::space::L,
                title: "No MIDI clip open",
                hint: "Double-click a MIDI clip to edit it",
                action: None,
            };
            widgets::empty_state(ui, area, &state);
            return;
        };
        let editable = self.document.notes_editable(clip.id);
        let toolbar = Rect::from_min_size(area.min, vec2(area.width(), TOOLBAR));
        let ruler = Rect::from_min_max(
            pos2(area.left() + KEY_WIDTH, toolbar.bottom()),
            pos2(area.right(), toolbar.bottom() + RULER_HEIGHT),
        );
        let keys = Rect::from_min_max(
            pos2(area.left(), ruler.bottom()),
            pos2(area.left() + KEY_WIDTH, area.bottom()),
        );
        let body = Rect::from_min_max(pos2(keys.right(), ruler.bottom()), area.max);
        if ed.row_height <= 0.0 {
            ed.row_height = ROW_DEFAULT;
        }
        // Framed before the toolbar, so the grid it names is the roll's.
        if ed.framed_for != Some(clip.id) {
            self.frame_editor(ed, &clip, body);
        }
        self.editor_toolbar(ui, toolbar, ed, &clip, editable);
        if body.height() < MIN_BODY || body.width() < MIN_BODY {
            return;
        }

        // The geometry: time is the editor's own view; pitch is ours too.
        let mut view = EditorView {
            body,
            time: ed.time,
            clip_start: clip.start,
            row_height: ed.row_height,
            scroll: ed.scroll,
        };
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        self.editor_zoom_keys(ed, &mut view, pointer);
        let layer = ui.layer_id();
        let on_top = |p: Pos2| ctx.layer_id_at(p) == Some(layer);
        let over_area = pointer.filter(|p| area.contains(*p) && on_top(*p));
        self.editor_wheel(&ctx, over_area, &mut view);
        ed.time = view.time;
        view.clamp_scroll();
        ed.row_height = view.row_height;
        ed.scroll = view.scroll;
        ed.hovered = over_area.is_some();
        let over_body = pointer.filter(|p| body.contains(*p) && on_top(*p));
        ed.pointer_at = over_body.map(|p| (view.local_at(p.x), view.pitch_at(p.y)));

        // The areas that take the pointer.
        let id = ui.id().with(("note editor", clip.id.0));
        let notes_response = ui.interact(body, id.with("notes"), Sense::click_and_drag());
        notes_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                format!("Note editor: {}", clip.name),
            )
        });
        let ruler_response = ui.interact(ruler, id.with("ruler"), Sense::click_and_drag());
        ruler_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Editor ruler: click to move the playhead",
            )
        });
        self.editor_ruler(&ruler_response, &view);
        if notes_response.clicked() || notes_response.drag_started() {
            // A click in the notes does not keep the keyboard focus: the
            // arrows and Delete are the editor's keys, not a control's.
            ctx.memory_mut(|m| m.surrender_focus(notes_response.id));
        }

        let modifiers = ctx.input(|i| i.modifiers);
        if editable {
            self.editor_pointer(&ctx, ed, &view, &clip, over_body, modifiers);
        }

        // What to draw.
        let notes = notes_of(&clip);
        let free = modifiers.alt;
        let cell = self.grid_cell_at_zoom(clip.start, view.time.zoom);
        let step = cell.1 - cell.0;
        let content = lumen_project::edit::content_length(&clip);
        let mut preview = Preview::default();
        let mut gesture_pitch = None;
        if let (Some(g), Some(p)) = (&ed.gesture, ctx.input(|i| i.pointer.latest_pos())) {
            let snap = |t: Ticks| self.snapped_at_zoom(t, free, view.time.zoom);
            let env = Env {
                view: &view,
                notes,
                length: content,
                step,
                snap: &snap,
            };
            let resolved = g.resolve(p, &env);
            preview = resolved.preview(&g.picked, &env);
            preview.chip = self.chip_text(&resolved, &preview, &clip);
            gesture_pitch = preview.ghosts.first().map(|n| i32::from(n.pitch()));
            if let Resolved::Move { .. } = resolved {
                // Every moved note sounds at its new pitch, as a chord; only
                // the ones whose pitch changed are struck again.
                self.audition_moved(clip.id, &tones_of(&preview.ghosts));
            }
        }
        let hover = if ed.gesture.is_none() {
            over_body.and_then(|p| view.note_at(notes, p))
        } else {
            None
        };
        let from = view.absolute_at(body.left());
        let to = view.absolute_at(body.right());
        let meter = self.transport.timeline.meter.clone();
        let lines = self.grid.lines(&meter, from, to, view.time.zoom);
        let painter = ui.painter().clone();
        // The GPU roll: its boxes are rebuilt only when the clip or the
        // picked notes change, never for a gesture's frames.
        let boxes = ed.boxes_for(&clip, colour);
        let p = theme::p();
        let look = canvas::GridLook {
            grid: self.grid,
            meter: meter.clone(),
            colors: [p.grid_bar, p.grid_beat, p.grid_sub],
        };
        let roll_rect = Rect::from_min_max(keys.min, body.max);
        piano_roll::draw::roll(
            &painter,
            surface,
            roll_rect,
            KEY_WIDTH,
            RollView {
                time: view.time,
                scroll: view.scroll,
                row_height: view.row_height,
            },
            &look,
            boxes,
        );
        if ed.gesture.is_none() {
            ed.boxed.clear();
        }
        let scene = Scene {
            notes,
            preview: &preview,
            boxed: &ed.boxed,
            colour,
            clip_length: clip.length,
            hover: hover.map(|(i, _)| i),
        };
        paint::paint_overlay(&painter, &view, &scene);
        if let Some(lit) = gesture_pitch.or_else(|| over_body.map(|p| view.pitch_at(p.y))) {
            paint::paint_lit_key(&painter, &view, keys, lit);
        }
        paint::paint_ruler(&painter, &view, ruler, &lines, &meter, clip.length);
        self.editor_playhead(&painter, &view, ruler);
        if let (Some(chip), Some(p)) = (&preview.chip, ctx.input(|i| i.pointer.latest_pos())) {
            widgets::draw_hint(&painter, p + vec2(14.0, -16.0), chip, body);
        }
        ed.last_view = Some(view);

        // The pointer's shape, and what the info line says.
        if let Some(p) = over_body.or(ctx
            .input(|i| i.pointer.latest_pos())
            .filter(|_| ed.gesture.is_some()))
        {
            ctx.set_cursor_icon(self.editor_cursor(ed, hover, p));
        }
        if let Some(p) = over_body
            && !egui::Popup::is_any_open(&ctx)
        {
            self.editor_info(ed, hover, &view, p);
        }
        self.editor_menu(ui, &notes_response, ed, &clip, over_body, hover);
        if ed.gesture.is_some() {
            ctx.request_repaint();
        }
    }

    /// The tool, the clip's name, the grid, and Pop out.
    fn editor_toolbar(
        &mut self,
        ui: &mut Ui,
        toolbar: Rect,
        ed: &mut NoteEditor,
        clip: &Arc<Clip>,
        editable: bool,
    ) {
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(toolbar)
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.set_clip_rect(toolbar.intersect(ui.clip_rect()));
        let keymap = &self.keymap;
        let choices = [
            Choice::new(Tool::Pointer, "Pointer")
                .named("Pointer tool")
                .tip("Select, move and resize notes"),
            Choice::new(Tool::Draw, "Draw")
                .named("Draw tool")
                .tip("Click to add a note, drag to set its length, click a note to remove it"),
        ];
        let drawn = widgets::segmented(&mut row, ui.id().with("note tool"), ed.tool, &choices);
        if let Some(tool) = drawn {
            ed.tool = tool;
        }
        row.add_space(theme::space::M);
        let label = if editable {
            clip.name.clone()
        } else {
            format!("{} (locked)", clip.name)
        };
        row.label(
            egui::RichText::new(label)
                .color(theme::p().text_secondary)
                .size(theme::size::BODY),
        );
        row.add_space(theme::space::M);
        row.label(
            egui::RichText::new(format!(
                "Grid {}",
                self.grid_label_at(ed.time.left, ed.time.zoom)
            ))
            .color(theme::p().text_hint)
            .size(theme::size::CAPTION),
        );
        row.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut popped = ed.popped;
            let label = if popped { "Dock" } else { "Pop out" };
            let response = widgets::toggle_button(ui, &mut popped, label);
            let keys = keymap.keys(Command::PopOutEditor);
            widgets::tip(
                response,
                if ed.popped {
                    "Put the editor back in the detail panel"
                } else {
                    "Open the editor in its own window"
                },
                keys.as_ref(),
                None,
            );
            ed.popped = popped;
        });
    }

    /// The wheel over the editor: Ctrl+wheel zooms time, Shift+wheel or a
    /// sideways swipe scrolls time (egui turns Shift into a horizontal
    /// scroll), Alt+wheel sizes the rows, the wheel scrolls pitch. Time is
    /// the editor's own view: the arrangement does not move (ADR-0048).
    fn editor_wheel(&self, ctx: &egui::Context, over: Option<Pos2>, view: &mut EditorView) {
        let Some(pointer) = over else {
            return;
        };
        let (delta, zoom, modifiers) =
            ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
        let limits = self.zoom_limits_at(view.body.width(), view.time.left);
        if zoom != 1.0 {
            let anchor = pointer.x - view.body.left();
            view.time = view.time.zoomed(anchor, f64::from(zoom), limits);
        } else if modifiers.alt {
            let wheel = if delta.y != 0.0 { delta.y } else { delta.x };
            if wheel != 0.0 {
                // Keep the pitch under the pointer where it is.
                let pitch = view.pitch_at(pointer.y);
                view.row_height = (view.row_height + wheel / 20.0).clamp(ROW_MIN, ROW_MAX);
                let kept = (super::view::PITCHES - 1 - pitch) as f32 * view.row_height;
                view.scroll = kept - (pointer.y - view.body.top());
            }
        } else {
            if delta.x != 0.0 {
                view.time = view.time.scrolled(delta.x, limits);
            }
            if delta.y != 0.0 {
                view.scroll -= delta.y;
            }
        }
    }

    /// Frames the editor on `clip` (its first draw, or a new clip): the
    /// clip's span fitted to the body's width, its middle pitch centred.
    /// The view is the editor's own; the arrangement does not move.
    fn frame_editor(&self, ed: &mut NoteEditor, clip: &Clip, body: Rect) {
        ed.framed_for = Some(clip.id);
        let limits = self.zoom_limits_at(body.width(), clip.start.as_quarters());
        ed.time = fit_clip(clip.start, clip.length, body.width(), limits);
        let notes = notes_of(clip);
        let middle = if notes.is_empty() {
            60
        } else {
            let mut pitches: Vec<i32> = notes.iter().map(|n| i32::from(n.pitch())).collect();
            pitches.sort_unstable();
            pitches[pitches.len() / 2]
        };
        ed.scroll = scroll_to_centre(middle, ed.row_height, body.height());
    }

    /// The zoom keys (+ and -) while the editor has the keyboard: the roll
    /// zooms around the pointer over it, else around the playhead when that
    /// is in view, else around the middle of the body.
    fn editor_zoom_keys(&self, ed: &mut NoteEditor, view: &mut EditorView, pointer: Option<Pos2>) {
        if ed.zooms.is_empty() {
            return;
        }
        let body = view.body;
        let anchor = pointer.filter(|p| body.contains(*p)).map_or_else(
            || {
                let x = view
                    .time
                    .x(body.left(), self.transport.playhead().as_quarters());
                if body.x_range().contains(x) {
                    x
                } else {
                    body.center().x
                }
            },
            |p| p.x,
        );
        let limits = self.zoom_limits_at(body.width(), view.time.left);
        for zoom in ed.zooms.drain(..) {
            let factor = match zoom {
                EditorZoom::In => KEY_ZOOM,
                EditorZoom::Out => 1.0 / KEY_ZOOM,
            };
            view.time = view.time.zoomed(anchor - body.left(), factor, limits);
        }
    }

    /// A click or drag on the ruler moves the playhead there (snapped; Alt
    /// places it freely).
    fn editor_ruler(&mut self, response: &egui::Response, view: &EditorView) {
        if (response.clicked() || response.dragged() || response.drag_started())
            && let Some(p) = response.interact_pointer_pos()
        {
            let free = response.ctx.input(|i| i.modifiers.alt);
            let at = self.snapped_at_zoom(view.absolute_at(p.x), free, view.time.zoom);
            self.transport.locate(at.max(Ticks::ZERO));
            self.transport.follow_paused = true;
        }
    }

    fn editor_playhead(&self, painter: &egui::Painter, view: &EditorView, ruler: Rect) {
        let at = self.transport.playhead().as_quarters();
        let x = view.time.x(view.body.left(), at);
        if x < view.body.left() || x > view.body.right() {
            return;
        }
        let y = egui::Rangef::new(ruler.top(), view.body.bottom());
        let colour = theme::p().accent;
        if self.transport.state().playing {
            crate::light::vline(painter, crate::light::BEAM, x, y, colour, 1.0);
        } else {
            painter.vline(x, y, egui::Stroke::new(1.5, colour));
        }
    }

    fn chip_text(&self, resolved: &Resolved, preview: &Preview, clip: &Clip) -> Option<String> {
        let first = preview.ghosts.first()?;
        let name = pitch_name(i32::from(first.pitch()));
        let at = |local: Ticks| {
            let bbt = self.transport.timeline.meter.bbt_at(clip.start + local);
            let sixteenth = bbt.ticks.0 / (PPQ / 4) + 1;
            format!("{}.{}.{}", bbt.bar + 1, bbt.beat + 1, sixteenth)
        };
        match resolved {
            Resolved::Move { .. } => Some(format!("{name} \u{00B7} {}", at(first.start()))),
            Resolved::Resize { .. } | Resolved::Draw(_) => {
                Some(format!("{name} \u{00B7} {}", length_text(first.length())))
            }
            _ => None,
        }
    }

    fn editor_cursor(
        &self,
        ed: &NoteEditor,
        hover: Option<(usize, Zone)>,
        _at: Pos2,
    ) -> CursorIcon {
        if let Some(g) = &ed.gesture {
            return match g.kind {
                Kind::Move { .. } if g.dragging => CursorIcon::Grabbing,
                Kind::Resize { .. } => CursorIcon::ResizeHorizontal,
                _ => CursorIcon::Crosshair,
            };
        }
        match (ed.tool, hover) {
            (Tool::Draw, Some(_)) => CursorIcon::PointingHand,
            (Tool::Draw, None) => CursorIcon::Crosshair,
            (Tool::Pointer, Some((_, Zone::Start | Zone::End))) => CursorIcon::ResizeHorizontal,
            (Tool::Pointer, Some((_, Zone::Body))) => CursorIcon::Grab,
            (Tool::Pointer, None) => CursorIcon::Default,
        }
    }

    fn editor_info(
        &mut self,
        ed: &NoteEditor,
        hover: Option<(usize, Zone)>,
        view: &EditorView,
        at: Pos2,
    ) {
        let pitch = pitch_name(view.pitch_at(at.y));
        let info = match (ed.tool, hover) {
            (Tool::Draw, Some(_)) => {
                Info::new(format!("Note {pitch}")).on(Held::NONE, Mouse::Click, "remove")
            }
            (Tool::Draw, None) => Info::new(format!("Draw {pitch}"))
                .on(Held::NONE, Mouse::Click, "add a note")
                .on(Held::NONE, Mouse::Drag, "set its length")
                .on(Held::ALT, Mouse::Drag, "ignore the grid"),
            (Tool::Pointer, Some(_)) => Info::new(format!("Note {pitch}"))
                .on(Held::NONE, Mouse::Drag, "move")
                .on(Held::CTRL, Mouse::Drag, "copy")
                .on(Held::ALT, Mouse::Drag, "ignore the grid")
                .on(Held::SHIFT, Mouse::Click, "add to selection")
                .on(Held::NONE, Mouse::RightClick, "note menu")
                .key(Command::Delete, "delete"),
            (Tool::Pointer, None) => Info::new(format!("Piano roll {pitch}"))
                .on(Held::NONE, Mouse::Drag, "select with a box")
                .on(Held::NONE, Mouse::RightClick, "menu")
                .key(Command::DrawTool, "draw tool"),
        };
        self.set_info(info);
    }

    /// The right-click menu: for the notes, or for the empty editor.
    fn editor_menu(
        &mut self,
        ui: &mut Ui,
        response: &egui::Response,
        ed: &mut NoteEditor,
        clip: &Clip,
        over_body: Option<Pos2>,
        hover: Option<(usize, Zone)>,
    ) {
        if ui.input(|i| i.pointer.button_pressed(PointerButton::Secondary)) && over_body.is_some() {
            if let Some((i, _)) = hover {
                if !ed.picked().contains(&i) {
                    self.select_notes(ed, vec![i]);
                }
                ed.menu_for = MenuFor::Notes;
            } else {
                ed.menu_for = MenuFor::Empty;
            }
            ed.menu_at = ed.pointer_at.map(|(t, _)| t);
        }
        let picked = !ed.picked().is_empty();
        // Where the pointer was when the menu opened, not where it is now.
        let split_at = ed.menu_at.filter(|t| *t >= Ticks::ZERO && *t < clip.length);
        widgets::Menu::context(response).show(|ui| {
            let keys = |s: &Self, c| s.keymap.keys(c);
            let mut op = None;
            let mut row = |ui: &mut Ui, label: &str, keys, enabled, value| {
                if widgets::menu_item(ui, label, keys, enabled).clicked() {
                    op = Some(value);
                }
            };
            match ed.menu_for {
                MenuFor::Notes => {
                    let (delete, duplicate, mute) = (
                        keys(self, Command::Delete),
                        keys(self, Command::Duplicate),
                        keys(self, Command::MuteClips),
                    );
                    row(ui, "Delete", delete.as_ref(), picked, NoteOp::Delete);
                    row(
                        ui,
                        "Duplicate",
                        duplicate.as_ref(),
                        picked,
                        NoteOp::Duplicate,
                    );
                    row(
                        ui,
                        "Mute or unmute",
                        mute.as_ref(),
                        picked,
                        NoteOp::ToggleMute,
                    );
                    ui.separator();
                    let (split, glue, legato) = (
                        keys(self, Command::SplitNotes),
                        keys(self, Command::GlueNotes),
                        keys(self, Command::LegatoNotes),
                    );
                    row(
                        ui,
                        "Split here",
                        split.as_ref(),
                        picked && split_at.is_some(),
                        NoteOp::Split(split_at.unwrap_or(Ticks::ZERO)),
                    );
                    row(ui, "Glue", glue.as_ref(), picked, NoteOp::Glue);
                    row(ui, "Legato", legato.as_ref(), picked, NoteOp::Legato);
                    ui.separator();
                    row(ui, "Transpose up", None, picked, NoteOp::Transpose(1));
                    row(ui, "Transpose down", None, picked, NoteOp::Transpose(-1));
                    row(ui, "Up an octave", None, picked, NoteOp::Transpose(12));
                    row(ui, "Down an octave", None, picked, NoteOp::Transpose(-12));
                }
                MenuFor::Empty => {
                    let all = keys(self, Command::SelectAll);
                    let has = !notes_of(clip).is_empty();
                    row(ui, "Select all", all.as_ref(), has, NoteOp::SelectAll);
                }
            }
            if let Some(op) = op {
                self.apply_note_op(ed, op);
            }
            if ed.menu_for == MenuFor::Empty {
                ui.separator();
                let draw = keys(self, Command::DrawTool);
                let mut on = ed.tool == Tool::Draw;
                if widgets::menu_toggle(ui, "Draw tool", draw.as_ref(), on).clicked() {
                    on = !on;
                    ed.tool = if on { Tool::Draw } else { Tool::Pointer };
                }
                let snap = keys(self, Command::SnapToGrid);
                if widgets::menu_toggle(ui, "Snap to grid", snap.as_ref(), self.grid.snap).clicked()
                {
                    self.grid.snap = !self.grid.snap;
                }
                let label = format!("Grid: {}", self.grid_label_at(ed.time.left, ed.time.zoom));
                widgets::submenu(ui, label, |ui| self.grid_menu(ui));
            }
        });
    }

    /// Sets the picked notes (a click or a box).
    fn select_notes(&self, ed: &mut NoteEditor, indices: Vec<usize>) {
        let project = self.document.project();
        match &mut ed.selection {
            Some(selection) => {
                selection.rebind(project, indices);
            }
            None => {
                if let Some(clip) = ed.clip {
                    ed.selection = lumen_project::edit::NoteSelection::in_clip(project, clip);
                    if let Some(selection) = &mut ed.selection {
                        selection.rebind(project, indices);
                    }
                }
            }
        }
    }

    /// Presses, drags and releases in the notes: the gesture state machine.
    fn editor_pointer(
        &mut self,
        ctx: &egui::Context,
        ed: &mut NoteEditor,
        view: &EditorView,
        clip: &Arc<Clip>,
        over_body: Option<Pos2>,
        modifiers: egui::Modifiers,
    ) {
        let (pressed, down, latest) = ctx.input(|i| {
            (
                i.pointer.button_pressed(PointerButton::Primary),
                i.pointer.primary_down(),
                i.pointer.latest_pos(),
            )
        });
        let notes = notes_of(clip);
        let free = modifiers.alt;
        if ed.gesture.is_none()
            && pressed
            && let Some(p) = over_body
            && !egui::Popup::is_any_open(ctx)
        {
            self.start_note_gesture(ed, view, clip, p, modifiers);
        }
        let Some(gesture) = &mut ed.gesture else {
            return;
        };
        let at = latest.unwrap_or(gesture.origin);
        if (at - gesture.origin).length() > DRAG_THRESHOLD {
            gesture.dragging = true;
        }
        let cell = self.grid_cell_at_zoom(clip.start, view.time.zoom);
        let step = cell.1 - cell.0;
        let content = lumen_project::edit::content_length(clip);
        let snap = |t: Ticks| self.snapped_at_zoom(t, free, view.time.zoom);
        let env = Env {
            view,
            notes,
            length: content,
            step,
            snap: &snap,
        };
        if down {
            // A box selects as it grows.
            if let Kind::Marquee { base } = &gesture.kind
                && gesture.dragging
                && let Resolved::Marquee(rect) = gesture.resolve(at, &env)
            {
                let mut picked = base.clone();
                for (i, n) in notes.iter().enumerate() {
                    if view.shows(*n) && view.note_rect(*n).intersects(rect) {
                        picked.push(i);
                    }
                }
                picked.sort_unstable();
                picked.dedup();
                ed.boxed = picked;
            }
            return;
        }
        // Released.
        let resolved = gesture.resolve(at, &env);
        let gesture = ed.gesture.take().expect("a gesture was just used");
        self.end_audition();
        let base = self.document.project().clone();
        let id = clip.id;
        let doc = &mut self.document;
        let result = match resolved {
            Resolved::Move { time, pitch, copy } if time != Ticks::ZERO || pitch != 0 => {
                Some(if copy {
                    doc.copy_notes(&base, id, &gesture.picked, time, pitch, None)
                } else {
                    doc.move_notes(&base, id, &gesture.picked, time, pitch, None)
                })
            }
            Resolved::Resize { end, by, min } if by != Ticks::ZERO => {
                Some(doc.resize_notes(&base, id, &gesture.picked, Resize { end, by, min }, None))
            }
            Resolved::Draw(note) => Some(doc.add_note(None, id, note, None)),
            _ => None,
        };
        match result {
            Some(indices) => self.select_notes(ed, indices),
            None if matches!(gesture.kind, Kind::Marquee { .. }) && gesture.dragging => {
                // The box picks what it held, once, so the roll's boxes are
                // rebuilt once for it.
                let picked = std::mem::take(&mut ed.boxed);
                self.select_notes(ed, picked);
            }
            None => {
                // A click without a drag. On one of several picked notes it
                // picks that one alone.
                let shift = modifiers.shift;
                if !gesture.dragging
                    && !shift
                    && matches!(gesture.kind, Kind::Move { .. } | Kind::Resize { .. })
                {
                    self.select_notes(ed, vec![gesture.grabbed]);
                }
            }
        }
    }

    fn start_note_gesture(
        &mut self,
        ed: &mut NoteEditor,
        view: &EditorView,
        clip: &Arc<Clip>,
        p: Pos2,
        modifiers: egui::Modifiers,
    ) {
        let notes = notes_of(clip);
        let hit = view.note_at(notes, p);
        let free = modifiers.alt;
        let id = clip.id;
        match (ed.tool, hit) {
            (Tool::Draw, Some((i, _))) => {
                // A click on a note removes it.
                let removed = self.document.delete_notes(id, &[i]);
                self.select_notes(ed, removed);
            }
            (Tool::Draw, None) => {
                let abs = view.absolute_at(p.x);
                let (cell_start, cell_end) = self.grid_cell_at_zoom(abs, view.time.zoom);
                let at = if free || !self.grid.snap {
                    abs
                } else {
                    cell_start
                };
                let start = at - clip.start;
                let end = if free || !self.grid.snap {
                    start
                } else {
                    cell_end - clip.start
                };
                let content = lumen_project::edit::content_length(clip);
                if start < Ticks::ZERO || start >= content {
                    return;
                }
                let pitch = view.pitch_at(p.y).clamp(0, 127) as u8;
                self.audition_note(clip.id, pitch, super::gesture::DRAW_VELOCITY);
                ed.gesture = Some(Gesture {
                    kind: Kind::Draw {
                        start,
                        end,
                        pitch,
                        step: (cell_end - cell_start).max(MIN_NOTE),
                    },
                    origin: p,
                    clip: clip.clone(),
                    grabbed: 0,
                    picked: Vec::new(),
                    dragging: true,
                });
                self.select_notes(ed, Vec::new());
            }
            (Tool::Pointer, Some((i, zone))) => {
                let mut picked = ed.picked().to_vec();
                if modifiers.shift {
                    if let Ok(at) = picked.binary_search(&i) {
                        // Shift on a picked note lets it go.
                        picked.remove(at);
                        self.select_notes(ed, picked);
                        return;
                    }
                    picked.push(i);
                } else if !picked.contains(&i) {
                    picked = vec![i];
                }
                picked.sort_unstable();
                self.select_notes(ed, picked.clone());
                let kind = match zone {
                    Zone::Start => Kind::Resize { end: End::Start },
                    Zone::End => Kind::Resize { end: End::End },
                    Zone::Body => Kind::Move {
                        copy: modifiers.command,
                    },
                };
                // A move previews every picked note as a chord; a resize, the
                // note grabbed.
                let heard: Vec<_> = match zone {
                    Zone::Body => lumen_project::edit::notes::picked(&picked, notes.len())
                        .iter()
                        .map(|&j| notes[j])
                        .collect(),
                    _ => vec![notes[i]],
                };
                self.audition_notes(clip.id, &tones_of(&heard));
                ed.gesture = Some(Gesture {
                    kind,
                    origin: p,
                    clip: clip.clone(),
                    grabbed: i,
                    picked,
                    dragging: false,
                });
            }
            (Tool::Pointer, None) => {
                let base = if modifiers.shift {
                    ed.picked().to_vec()
                } else {
                    self.select_notes(ed, Vec::new());
                    Vec::new()
                };
                ed.gesture = Some(Gesture {
                    kind: Kind::Marquee { base },
                    origin: p,
                    clip: clip.clone(),
                    grabbed: 0,
                    picked: Vec::new(),
                    dragging: false,
                });
            }
        }
    }
}

/// "1 beat", "0.25 beat", "2.5 beats": a note's length.
fn length_text(length: Ticks) -> String {
    let beats = length.as_quarters();
    let text = format!("{beats:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if (beats - 1.0).abs() < 1e-9 {
        "1 beat".to_owned()
    } else {
        format!("{text} beats")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_read_in_beats() {
        assert_eq!(length_text(Ticks::quarters(1)), "1 beat");
        assert_eq!(length_text(Ticks::quarters(2)), "2 beats");
        assert_eq!(length_text(Ticks(PPQ / 4)), "0.25 beats");
        assert_eq!(length_text(Ticks(PPQ * 5 / 2)), "2.5 beats");
    }
}
