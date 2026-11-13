//! The editor on the screen (M5 slice 3c): toolbar, ruler, keys and notes,
//! and the pointer. See `gesture` for what a drag means and `paint` for how
//! it looks.

use eframe::egui::{self, CursorIcon, PointerButton, Pos2, Rect, Sense, Ui, pos2, vec2};
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::{Clip, Id};

use std::sync::Arc;

use super::gesture::{Env, Kind, Preview, Resolved, tones_of};
use super::lane;
use super::paint::{self, Scene};
use super::view::{
    EditorView, KEY_WIDTH, KEY_WIDTH_DRUMS, ROW_DEFAULT, ROW_MAX, ROW_MIN, RULER_HEIGHT, Rows,
    Zone, fit_clip, pitch_name, scroll_to_row,
};
use super::{EditorZoom, MenuFor, NoteEditor, NoteOp, Tool, notes_of};
use crate::app::info::{Held, Info, Mouse};
use crate::app::navigate::KEY_ZOOM;
use crate::canvas::{self, Surface};
use crate::commands::Command;
use crate::piano_roll::{self, draw::RollView};
use crate::widgets;
use crate::{App, theme};

/// The toolbar's height above the ruler.
const TOOLBAR: f32 = crate::app::clip_header::HEADER;
/// The editor needs at least this much body to be drawn.
const MIN_BODY: f32 = 24.0;

/// The drum editor's rows in the editor's menus (M8 D2): the view switch and
/// Hide unused, and the sound under the pointer's length.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DrumMenu {
    pub track: Id,
    pub map: Id,
    pub view: crate::drum_editor::View,
    pub hiding: bool,
    /// The sound under the pointer and whether its lane shows its length, when
    /// the sound has no length override.
    pub length: Option<(u8, bool)>,
}

/// What the editor's right-click menu acts on this frame (M8 D2 adds the
/// drum editor's rows).
#[derive(Debug, Clone, Copy)]
pub(crate) struct MenuTarget {
    /// The pointer over the notes, when it is.
    pub over_body: Option<Pos2>,
    /// The hit under the pointer.
    pub hover: Option<(usize, Zone)>,
    /// The drum editor's own rows, on a mapped track.
    pub drum: Option<DrumMenu>,
}

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
        // The map the clip's track reads, when it reads one: the rows are
        // then the kit's, and the key column names each sound (M8 D1); with
        // the drum editor's Lanes chosen the editor is one lane a sound
        // (M8 D2). The lanes are the editor's cache, so a frame that changes
        // nothing scans no notes (H2). The view is read from the editor
        // itself: while it draws it is taken out of the app.
        let drums = self.clip_key_sounds(clip.id);
        let track = self.track_of_clip(clip.id);
        let view = track.map(|track| ed.drum.view(track));
        let (map, lanes) = match self.clip_drum_map(clip.id) {
            Some(map) => {
                let hide = track.is_some_and(|track| ed.drum.hides_unused(track));
                let lanes = if view == Some(crate::drum_editor::View::Lanes) {
                    Some(ed.drum.lanes_for(map, &clip, notes_of(&clip), hide))
                } else {
                    None
                };
                (Some((map.id, map.name.clone(), map.sounds.len())), lanes)
            }
            None => (None, None),
        };
        let drum: Option<Arc<Vec<crate::drum_editor::Lane>>> = map.as_ref().and(lanes);
        let lanes: &[crate::drum_editor::Lane] = drum.as_deref().map(Vec::as_slice).unwrap_or(&[]);
        if ed.row_height <= 0.0 {
            ed.row_height = ROW_DEFAULT;
        }
        let rows = if drum.is_some() {
            Rows::lanes(lanes.iter().map(|lane| lane.input))
        } else if drums.is_some() && ed.hide_unmapped {
            Rows::mapped(drums.as_deref().unwrap_or_default())
        } else {
            Rows::all()
        };
        let key_width = if drum.is_some() {
            crate::drum_editor::geometry::NAME
        } else if drums.is_some() {
            KEY_WIDTH_DRUMS
        } else {
            KEY_WIDTH
        };
        let row_height = if drum.is_some() {
            crate::drum_editor::geometry::ROW
        } else {
            ed.row_height
        };
        // The lane takes the bottom of the editor when there is room.
        let lane_height = lane::reserved(area.height() - TOOLBAR - RULER_HEIGHT, &ed.lane);
        let roll = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - lane_height));
        let lane_area = Rect::from_min_max(pos2(area.left(), roll.bottom()), area.max);
        let toolbar = Rect::from_min_size(area.min, vec2(area.width(), TOOLBAR));
        let ruler = Rect::from_min_max(
            pos2(area.left() + key_width, toolbar.bottom()),
            pos2(area.right(), toolbar.bottom() + RULER_HEIGHT),
        );
        let keys = Rect::from_min_max(
            pos2(area.left(), ruler.bottom()),
            pos2(area.left() + key_width, roll.bottom()),
        );
        let body = Rect::from_min_max(pos2(keys.right(), ruler.bottom()), roll.max);
        // Framed before the toolbar, so the grid it names is the roll's.
        if ed.framed_for != Some(clip.id) {
            self.frame_editor(ed, &clip, body, rows);
            if drum.is_some() {
                // The lanes start at the top of the map. Their row height is
                // the view's, never the roll's (M5: the roll keeps its own).
                ed.scroll = 0.0;
            }
        }
        self.editor_toolbar(
            ui,
            toolbar,
            ed,
            super::toolbar::Strip {
                clip: &clip,
                colour,
                editable,
                mapped: drums.is_some(),
                drum: map.as_ref().and(view),
            },
        );
        if body.height() < MIN_BODY || body.width() < MIN_BODY {
            return;
        }

        // The geometry: time is the editor's own view; pitch is ours too.
        let mut view = EditorView {
            body,
            time: ed.time,
            clip_start: clip.start,
            row_height,
            scroll: ed.scroll,
            rows,
            lanes: drum.is_some(),
        };
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        self.editor_zoom_keys(ed, &mut view, pointer);
        let layer = ui.layer_id();
        let on_top = |p: Pos2| ctx.layer_id_at(p) == Some(layer);
        let over_area = pointer.filter(|p| area.contains(*p) && on_top(*p));
        // Over the roll the wheel moves time and pitch; over the lane under
        // it, which shares the roll's time, it moves time only.
        let over_roll = over_area.filter(|p| roll.contains(*p));
        let over_lane = over_area.filter(|p| lane_area.contains(*p));
        if let Some(p) = over_roll.or(over_lane) {
            self.editor_wheel(&ctx, p, over_roll.is_some(), &mut view);
        }
        self.editor_edge_scroll(&ctx, ed, &mut view);
        ed.time = view.time;
        view.clamp_scroll();
        if drum.is_none() {
            // The lanes keep their row height; the roll's is the user's.
            ed.row_height = view.row_height;
        }
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
                if drum.is_some() {
                    format!("Drum lanes: {}", clip.name)
                } else {
                    format!("Note editor: {}", clip.name)
                },
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
            ed.lane.focus = false;
        }

        let modifiers = ctx.input(|i| i.modifiers);
        if editable {
            self.editor_pointer(&ctx, ed, &view, &clip, over_body, modifiers);
        }

        // What to draw.
        let free = modifiers.alt;
        let cell = self.grid_cell_at_zoom(clip.start, view.time.zoom);
        let step = cell.1 - cell.0;
        // While the quantize options are open the clip is drawn as Apply
        // would leave it, with a trail from each note that moves.
        let options_open = ed
            .quantize
            .button
            .is_some_and(|id| widgets::Menu::is_open_for(&ctx, id));
        let picks = ed.picked().to_vec();
        let shown = ed.quantize.shown(options_open, &clip, &picks, step);
        let drawn: std::sync::Arc<lumen_project::model::Clip> = shown
            .as_ref()
            .map_or_else(|| clip.clone(), |s| s.clip.clone());
        let trails: &[super::quantize::Trail] = shown.as_ref().map_or(&[], |s| s.trails.as_slice());
        let notes = notes_of(&drawn);
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
            preview.chip = if drum.is_some() {
                self.drum_chip(&resolved, &preview, &clip)
            } else {
                self.chip_text(&resolved, &preview, &clip)
            };
            gesture_pitch = preview.ghosts.first().map(|n| i32::from(n.pitch()));
            if let Resolved::Move { .. } | Resolved::MoveHits { .. } = resolved {
                // Every moved note sounds at its new pitch, as a chord; only
                // the ones whose pitch changed are struck again.
                self.audition_moved(clip.id, &tones_of(&preview.ghosts));
            }
        }
        // The lanes' hits, gathered once per change (the editor's cache, H2).
        let cells = if drum.is_some() {
            let picks = ed.picked().to_vec();
            Some(
                ed.drum
                    .cells_for(&view, &drawn, notes, &picks, &preview.hidden),
            )
        } else {
            None
        };
        let hover = if ed.gesture.is_none() {
            over_body.and_then(|p| {
                if let Some(cells) = &cells {
                    crate::drum_editor::geometry::hit_at_cells(&view, cells, step, p)
                        .map(|i| (i, Zone::Body))
                } else {
                    view.note_at(notes, p)
                }
            })
        } else {
            None
        };
        let from = view.absolute_at(body.left());
        let to = view.absolute_at(body.right());
        let meter = self.transport.timeline.meter.clone();
        let lines = self.grid.lines(&meter, from, to, view.time.zoom);
        let painter = ui.painter().clone();
        let p = theme::p();
        let look = canvas::GridLook {
            grid: self.grid,
            meter: meter.clone(),
            colors: [p.grid_bar, p.grid_beat, p.grid_sub],
        };
        if let (Some(lanes), Some((map_id, map_name, sounds))) = (drum.as_deref(), map.as_ref()) {
            // The drum editor: one lane a sound, its hits the diamond whose
            // fill is the velocity (DE A), and the map's name column.
            self.drum_surface(
                ui,
                &ctx,
                surface,
                &look,
                crate::app::clip::clip_drum::DrumFrame {
                    ed,
                    view: &view,
                    clip: &clip,
                    lanes,
                    map_id: *map_id,
                    map_name,
                    sounds: *sounds,
                    keys,
                },
            );
            if ed.gesture.is_none() {
                ed.boxed.clear();
            }
            let cells = cells.as_deref().map(Vec::as_slice).unwrap_or(&[]);
            let shows = |input: u8| ed.drum.shows_length(*map_id, input);
            let scene = crate::drum_editor::paint::Scene {
                lanes,
                notes,
                cells,
                hover: hover.map(|(i, _)| i),
                preview: &preview,
                boxed: &ed.boxed,
                clip_length: clip.length,
                step,
                shows_length: &shows,
            };
            crate::drum_editor::paint::hits(&painter, &view, &scene);
            crate::drum_editor::paint::overlay(&painter, &view, &scene);
        } else {
            // The GPU roll: its boxes are rebuilt only when the clip or the
            // picked notes change, never for a gesture's frames.
            let boxes = ed.boxes_for(&drawn, colour, rows);
            let roll_rect = Rect::from_min_max(keys.min, body.max);
            piano_roll::draw::roll(
                piano_roll::draw::RollCanvas {
                    painter: &painter,
                    surface,
                    rect: roll_rect,
                    key_width,
                },
                RollView {
                    time: view.time,
                    scroll: view.scroll,
                    row_height: view.row_height,
                    rows,
                },
                &look,
                piano_roll::draw::RollNotes {
                    boxes: boxes.0,
                    generation: boxes.1,
                    under: |_: &egui::Painter| {},
                },
                drums.as_deref(),
            );
            if ed.gesture.is_none() {
                ed.boxed.clear();
            }
            let scene = Scene {
                notes,
                preview: &preview,
                boxed: &ed.boxed,
                clip_length: clip.length,
                hover: hover.map(|(i, _)| i),
                picked: ed.picked(),
                trails,
            };
            paint::paint_overlay(&painter, &view, &scene);
            if let Some(lit) = gesture_pitch.or_else(|| over_body.map(|p| view.pitch_at(p.y))) {
                paint::paint_lit_key(&painter, &view, keys, lit);
            }
        }
        paint::paint_ruler(&painter, &view, ruler, &lines, &meter, clip.length);
        self.editor_edge_fade(&painter, body);
        self.paint_editor_playhead(&painter, view.time, view.body, ruler.top());
        if let (Some(chip), Some(p)) = (&preview.chip, ctx.input(|i| i.pointer.latest_pos())) {
            widgets::draw_hint(&painter, p + vec2(14.0, -16.0), chip, body);
        }
        if lane_height > 0.0 {
            let input = lane::LaneInput {
                clip: &clip,
                colour,
                view: &view,
                area: lane_area,
                lines: &lines,
                over: pointer.filter(|p| on_top(*p)),
                editable,
            };
            self.draw_lane(ui, ed, &input);
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
            self.editor_info(ed, hover, &view, p, drum.is_some(), &clip);
        }
        let drum_menu = map.as_ref().and_then(|(map_id, _, _)| {
            let track = track?;
            let view = ed.drum.view(track);
            let hiding = ed.drum.hides_unused(track);
            let length = hover.and_then(|(i, _)| notes.get(i)).and_then(|note| {
                let map = self.clip_drum_map(clip.id)?;
                let sound = map.sound(note.pitch())?;
                sound
                    .length
                    .is_none()
                    .then(|| (note.pitch(), ed.drum.shows_length(*map_id, note.pitch())))
            });
            Some(DrumMenu {
                track,
                map: *map_id,
                view,
                hiding,
                length,
            })
        });
        self.editor_menu(
            ui,
            &notes_response,
            ed,
            &clip,
            MenuTarget {
                over_body,
                hover,
                drum: drum_menu,
            },
        );
        if ed.gesture.is_some() {
            ctx.request_repaint();
        }
    }

    /// The wheel over the editor: Ctrl+wheel zooms time, Shift+wheel or a
    /// sideways swipe scrolls time (egui turns Shift into a horizontal
    /// scroll); over the roll (`pitch`), Alt+wheel sizes the rows and the
    /// wheel scrolls pitch. Time is the editor's own view: the arrangement
    /// does not move (ADR-0048).
    fn editor_wheel(&self, ctx: &egui::Context, pointer: Pos2, pitch: bool, view: &mut EditorView) {
        let (delta, zoom, modifiers) =
            ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
        let limits = self.zoom_limits_at(view.body.width(), view.time.left);
        if zoom != 1.0 {
            let anchor = pointer.x - view.body.left();
            view.time = view.time.zoomed(anchor, f64::from(zoom), limits);
        } else if modifiers.alt && pitch && !view.lanes {
            let wheel = if delta.y != 0.0 { delta.y } else { delta.x };
            if wheel != 0.0 {
                // Keep the row under the pointer where it is.
                let row = view.rows.row_of(view.pitch_at(pointer.y)).unwrap_or(0);
                view.row_height = (view.row_height + wheel / 20.0).clamp(ROW_MIN, ROW_MAX);
                let kept = row as f32 * view.row_height;
                view.scroll = kept - (pointer.y - view.body.top());
            }
        } else {
            if delta.x != 0.0 {
                view.time = view.time.scrolled(delta.x, limits);
            }
            if delta.y != 0.0 && pitch {
                view.scroll -= delta.y;
            }
        }
    }

    /// A drag in the roll or the lane near the body's sides scrolls the
    /// view (and near its top or bottom, for the roll, the pitches), as the
    /// arrangement and the audio editor do: faster the closer, and only once
    /// the hand moves outwards from where it pressed
    /// (`audio_editor::handles::edge_scroll`). The gesture's press, and the
    /// pencil's stroke, move with the content, so what was under them stays
    /// under them.
    fn editor_edge_scroll(
        &mut self,
        ctx: &egui::Context,
        ed: &mut NoteEditor,
        view: &mut EditorView,
    ) {
        use crate::audio_editor::handles::edge_scroll;
        let notes = ed.gesture.as_ref().is_some_and(|g| g.dragging);
        let lane = ed.lane.gesture.as_ref().is_some_and(|g| g.dragging);
        let (down, latest, from, dt) = ctx.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.latest_pos(),
                i.pointer.press_origin(),
                i.stable_dt,
            )
        });
        let (Some(p), Some(from)) = (latest, from) else {
            return;
        };
        if !down || !(notes || lane) {
            return;
        }
        let before = *view;
        let limits = self.zoom_limits_at(view.body.width(), view.time.left);
        let pull = edge_scroll(p.x, from.x, view.body.x_range(), dt);
        view.time = view.time.scrolled(pull, limits);
        if notes {
            view.scroll -= edge_scroll(p.y, from.y, view.body.y_range(), dt);
            view.clamp_scroll();
        }
        let left = view.body.left();
        let dx = view.time.x(left, before.time.left) - left;
        let dy = before.scroll - view.scroll;
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        if let Some(g) = ed.gesture.as_mut() {
            g.origin += vec2(dx, dy);
        }
        if let Some(g) = ed.lane.gesture.as_mut() {
            g.origin.x += dx;
            for q in &mut g.trail {
                q.x += dx;
            }
        }
        // Looking around pauses Follow, as in the arrangement.
        self.transport.follow_paused = true;
        ctx.request_repaint();
    }

    /// Frames the editor on `clip` (its first draw, or a new clip): the
    /// clip's span fitted to the body's width, its middle pitch centred.
    /// The view is the editor's own; the arrangement does not move.
    fn frame_editor(&self, ed: &mut NoteEditor, clip: &Clip, body: Rect, rows: Rows) {
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
        let row = rows.row_of(middle).unwrap_or(0);
        ed.scroll = scroll_to_row(row, ed.row_height, body.height());
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

    /// The roll's right edge fades into the ground, as the mixer's strips
    /// do, so notes running off the view end softly rather than at a cut.
    fn editor_edge_fade(&self, painter: &egui::Painter, body: Rect) {
        let ground = theme::p().canvas;
        let edge = theme::space::L;
        let rect = Rect::from_min_max(pos2(body.right() - edge, body.top()), body.max);
        widgets::gradient(
            painter,
            rect,
            ground.gamma_multiply(theme::alpha::NONE),
            ground,
        );
    }

    /// The chip beside the pointer in the drum editor's lanes: the sound, its
    /// bar and beat and its velocity ("Snare · 2.3.1 · vel 96").
    fn drum_chip(&self, resolved: &Resolved, preview: &Preview, clip: &Clip) -> Option<String> {
        let first = preview.ghosts.first()?;
        let name = self
            .clip_drum_map(clip.id)
            .and_then(|map| map.sound(first.pitch()))
            .map_or_else(
                || pitch_name(i32::from(first.pitch())),
                |sound| sound.name.clone(),
            );
        let bbt = self
            .transport
            .timeline
            .meter
            .bbt_at(clip.start + first.start());
        let sixteenth = bbt.ticks.0 / (PPQ / 4) + 1;
        let at = format!("{}.{}.{}", bbt.bar + 1, bbt.beat + 1, sixteenth);
        match resolved {
            Resolved::Velocity(_) => Some(format!("{name} \u{00B7} vel {}", first.velocity())),
            Resolved::Move { .. } => Some(format!(
                "{name} \u{00B7} {at} \u{00B7} vel {}",
                first.velocity()
            )),
            Resolved::Draw(_) => Some(format!(
                "{name} \u{00B7} {at} \u{00B7} vel {}",
                first.velocity()
            )),
            _ => None,
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
            Resolved::Resize { .. } => {
                Some(format!("{name} \u{00B7} {}", length_text(first.length())))
            }
            Resolved::Draw(_) => Some(format!(
                "{name} \u{00B7} vel {} \u{00B7} {}",
                first.velocity(),
                length_text(first.length())
            )),
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
        drum: bool,
        clip: &Clip,
    ) {
        if drum {
            self.drum_info(ed, hover, view, at, clip);
            return;
        }
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
                .on(Held::NONE, Mouse::DoubleClick, "add a note")
                .on(Held::NONE, Mouse::RightClick, "menu")
                .key(Command::DrawTool, "draw tool"),
        };
        self.set_info(info);
    }

    /// The info line in the lanes (M8 D2): the sound, its bar and beat and its
    /// velocity under the pointer ("Snare, bar 2 beat 3, velocity 96").
    fn drum_info(
        &mut self,
        ed: &NoteEditor,
        hover: Option<(usize, Zone)>,
        view: &EditorView,
        at: Pos2,
        clip: &Clip,
    ) {
        let map = self.clip_drum_map(clip.id);
        let lane_name = |pitch: u8| {
            map.and_then(|map| map.sound(pitch))
                .map_or_else(|| pitch_name(i32::from(pitch)), |s| s.name.clone())
        };
        let at_beat = |local: Ticks| {
            let bbt = self.transport.timeline.meter.bbt_at(clip.start + local);
            format!("bar {} beat {}", bbt.bar + 1, bbt.beat + 1)
        };
        let notes = self.editor_clip(clip.id).map(|(c, _)| c);
        let note =
            hover.and_then(|(i, _)| notes.as_ref().and_then(|c| notes_of(c).get(i)).copied());
        let info = match (ed.tool, note) {
            (Tool::Draw, Some(note)) => Info::new(format!(
                "{}, {}, velocity {}",
                lane_name(note.pitch()),
                at_beat(note.start()),
                note.velocity()
            ))
            .on(Held::NONE, Mouse::Click, "remove the hit"),
            (Tool::Draw, None) => {
                let name = lane_name(view.pitch_at(at.y).clamp(0, 127) as u8);
                Info::new(format!("Draw on {name}"))
                    .on(Held::NONE, Mouse::Click, "add a hit")
                    .on(Held::ALT, Mouse::Click, "ignore the grid")
            }
            (Tool::Pointer, Some(note)) => Info::new(format!(
                "{}, {}, velocity {}",
                lane_name(note.pitch()),
                at_beat(note.start()),
                note.velocity()
            ))
            .on(
                Held::NONE,
                Mouse::Drag,
                "move the hit; up or down sets its velocity",
            )
            .on(Held::CTRL, Mouse::Drag, "copy")
            .on(Held::ALT, Mouse::Drag, "ignore the grid")
            .on(Held::SHIFT, Mouse::Click, "add to selection")
            .on(Held::NONE, Mouse::RightClick, "hit menu")
            .key(Command::Delete, "delete"),
            (Tool::Pointer, None) => Info::new("Drum lanes")
                .on(Held::NONE, Mouse::Drag, "select with a box")
                .on(Held::NONE, Mouse::DoubleClick, "add a hit")
                .on(Held::NONE, Mouse::RightClick, "menu")
                .key(Command::DrawTool, "draw tool"),
        };
        self.set_info(info);
    }

    /// The right-click menu: for the notes, or for the empty editor, and the
    /// drum editor's own rows on a mapped track (M8 D2).
    fn editor_menu(
        &mut self,
        ui: &mut Ui,
        response: &egui::Response,
        ed: &mut NoteEditor,
        clip: &Clip,
        target: MenuTarget,
    ) {
        let MenuTarget {
            over_body,
            hover,
            drum,
        } = target;
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
        // A command a row asks for, run after the menu closes (review M4).
        let mut later: Option<Command> = None;
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
                    ui.separator();
                    if let Some(tool) = self.tools_menu(ui, ed) {
                        op = Some(tool);
                    }
                }
                MenuFor::Empty => {
                    let all = keys(self, Command::SelectAll);
                    let has = !notes_of(clip).is_empty();
                    row(ui, "Select all", all.as_ref(), has, NoteOp::SelectAll);
                    ui.separator();
                    if let Some(tool) = self.tools_menu(ui, ed) {
                        op = Some(tool);
                    }
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
            // The drum editor's own rows (M8 D2), only what can apply.
            if let Some(drum) = drum {
                ui.separator();
                if let Some((input, shown)) = drum.length
                    && widgets::menu_toggle(ui, "Show the sound's length", None, shown).clicked()
                {
                    ed.drum.toggle_length(drum.map, input);
                }
                if drum.view == crate::drum_editor::View::Lanes {
                    // The picked lane's own switches, through the commands
                    // (review M4).
                    let lane = self.drum_picked_lane().is_some();
                    let mute = keys(self, Command::DrumMuteLane);
                    if widgets::menu_item(ui, "Mute the picked lane", mute.as_ref(), lane).clicked()
                    {
                        later = Some(Command::DrumMuteLane);
                    }
                    let solo = keys(self, Command::DrumSoloLane);
                    if widgets::menu_item(ui, "Solo the picked lane", solo.as_ref(), lane).clicked()
                    {
                        later = Some(Command::DrumSoloLane);
                    }
                    ui.separator();
                    let unused = keys(self, Command::HideUnusedLanes);
                    if widgets::menu_toggle(ui, "Hide unused lanes", unused.as_ref(), drum.hiding)
                        .clicked()
                    {
                        ed.drum.toggle_unused(drum.track);
                    }
                    let keys_row = keys(self, Command::DrumKeysView);
                    if widgets::menu_item(ui, "Show the piano roll (Keys)", keys_row.as_ref(), true)
                        .clicked()
                    {
                        ed.drum.set_view(drum.track, crate::drum_editor::View::Keys);
                    }
                } else {
                    let lanes_row = keys(self, Command::DrumLanesView);
                    if widgets::menu_item(ui, "Show the drum lanes", lanes_row.as_ref(), true)
                        .clicked()
                    {
                        ed.drum
                            .set_view(drum.track, crate::drum_editor::View::Lanes);
                        ed.framed_for = None;
                    }
                }
            }
        });
        if let Some(command) = later {
            self.run(&ui.ctx().clone(), command);
        }
    }

    /// Sets the picked notes (a click or a box).
    pub(in crate::note_editor) fn select_notes(&self, ed: &mut NoteEditor, indices: Vec<usize>) {
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
