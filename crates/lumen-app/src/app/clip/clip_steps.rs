//! The step sequencer in the Clip tab (M8 slice D3, the owner's SQ A): the
//! grid's strip (step count, resolution, accent, page), the pointer on its
//! cells, its keys, and the page commands. The grid itself (its geometry and
//! picture) is `crate::drum_editor::steps`; the notes, the selection, the
//! history and the commands are the note editor's, so Steps is another view of
//! the one editor, not a second editor: a step is edited through one named
//! `Document` method (`edit_steps`, one undo step per gesture) and the notes it
//! leaves picked are the editor's selection.
//!
//! None of this is saved: the grid a clip is set to (count, resolution, page,
//! accent) lasts while the app runs, like Lanes' hidden sounds (D3's ADR).

use std::sync::Arc;

use eframe::egui::{
    self, Align, Event, EventFilter, Key, Layout, Modifiers, Popup, Pos2, Rect, RichText, Sense,
    Stroke, Ui, UiBuilder, WidgetInfo, WidgetType, vec2,
};
use lumen_core::time::Ticks;
use lumen_project::edit::steps::{self as model, Cell, Change, STEP_VELOCITY, Target};
use lumen_project::model::{Clip, Id};

use crate::app::info::{Held, Info, Mouse};
use crate::canvas::Surface;
use crate::drum_editor::steps::geometry::{self, Grid, ROW};
use crate::drum_editor::steps::input::{self, Gesture, Move};
use crate::drum_editor::steps::paint::{self, Live, Scene};
use crate::drum_editor::steps::{
    ACCENTS, COUNTS, Drawn, Layout as GridLayout, RESOLUTIONS, Setup, fits,
};
use crate::drum_editor::{Lane, View};
use crate::note_editor::view::{EditorView, Rows, Zone};
use crate::note_editor::{NoteEditor, notes_of};
use crate::widgets::{self, Choice, raised};
use crate::{App, theme};

/// The most pages the strip shows as numbers; beyond it, Previous and Next.
const NUMBERED_PAGES: u32 = 8;
const COUNT_NAMES: [(&str, &str); 4] = [
    ("8", "8 steps a page"),
    ("16", "16 steps a page"),
    ("32", "32 steps a page"),
    ("64", "64 steps a page"),
];
const PAGE_NUMBERS: [&str; NUMBERED_PAGES as usize] = ["1", "2", "3", "4", "5", "6", "7", "8"];

/// The grid's Tab stop: stable for a clip, so the keys find it between frames.
pub(crate) fn grid_id(clip: Id) -> egui::Id {
    egui::Id::new(("drum steps grid", clip.0))
}

/// What one frame of the grid needs from the editor that draws it.
pub(crate) struct StepsFrame<'a> {
    pub ed: &'a mut NoteEditor,
    pub clip: &'a Arc<Clip>,
    pub lanes: &'a [Lane],
    pub map_id: Id,
    pub map_name: &'a str,
    pub sounds: usize,
    pub editable: bool,
    pub area: Rect,
    pub strip: Rect,
    pub keys: Rect,
    pub body: Rect,
    pub surface: Surface,
}

/// What the grid tells the editor that drew it: the area the menu opens on,
/// the pointer over the cells, and the note under it.
pub(crate) struct StepsOut {
    pub response: egui::Response,
    pub over_body: Option<Pos2>,
    pub hover: Option<(usize, Zone)>,
}

/// The facts the keys and the page commands act on, read from the app.
struct Context {
    clip: Arc<Clip>,
    layout: GridLayout,
    setup: Setup,
}

/// A key the grid answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepKey {
    Move(Move),
    Toggle { accent: bool },
    Page(i32),
}

/// Takes the keys a focused grid answers out of `events`: the arrows, Home and
/// End, Space and Enter (Shift for the accent), Page Up and Page Down, with
/// nothing else held. Nothing it takes reaches the commands, so Space toggles
/// a step while the grid has the focus and never starts the transport there.
fn take_keys(events: &mut Vec<Event>) -> Vec<StepKey> {
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
        let plain = modifiers == Modifiers::NONE;
        let shift = modifiers == Modifiers::SHIFT;
        let step = match key {
            Key::ArrowLeft if plain => Some(StepKey::Move(Move::Left)),
            Key::ArrowRight if plain => Some(StepKey::Move(Move::Right)),
            Key::ArrowUp if plain => Some(StepKey::Move(Move::Up)),
            Key::ArrowDown if plain => Some(StepKey::Move(Move::Down)),
            Key::Home if plain => Some(StepKey::Move(Move::Home)),
            Key::End if plain => Some(StepKey::Move(Move::End)),
            Key::Space | Key::Enter if plain => Some(StepKey::Toggle { accent: false }),
            Key::Space | Key::Enter if shift => Some(StepKey::Toggle { accent: true }),
            Key::PageUp if plain => Some(StepKey::Page(-1)),
            Key::PageDown if plain => Some(StepKey::Page(1)),
            _ => None,
        };
        if let Some(step) = step {
            taken.push(step);
        }
        step.is_none()
    });
    taken
}

impl App {
    /// A drum clip is opened: Steps when it fits the grid, Lanes when it does
    /// not, unless the track's view was chosen (SQ A; the owner's double-click
    /// rule).
    pub(crate) fn pick_drum_view_for(&mut self, clip: Id) {
        if self.clip_drum_map(clip).is_none() {
            return;
        }
        let Some(track) = self.track_of_clip(clip) else {
            return;
        };
        let Some((open, _)) = self.editor_clip(clip) else {
            return;
        };
        let meter = self.transport.timeline.meter.meter_at(open.start);
        let fits = fits(lumen_project::edit::content_length(&open), meter);
        if self.note_editor.drum.open_view(track, fits) {
            self.note_editor.framed_for = None;
        }
    }

    /// The open clip and its grid, when the open clip is a drum clip shown as
    /// Steps.
    fn steps_context(&self) -> Option<Context> {
        let ed = &self.note_editor;
        let id = ed.clip?;
        let track = self.track_of_clip(id)?;
        if ed.drum.view(track) != View::Steps {
            return None;
        }
        let (clip, _) = self.editor_clip(id)?;
        let meter = self.transport.timeline.meter.meter_at(clip.start);
        let setup = ed.drum.steps.setup(id);
        let layout = setup.layout(lumen_project::edit::content_length(&clip), meter)?;
        Some(Context {
            clip,
            layout,
            setup,
        })
    }

    /// Whether the page after (or before) the shown one exists: the commands'
    /// availability.
    pub(crate) fn steps_page_available(&self, next: bool) -> Result<(), &'static str> {
        let Some(context) = self.steps_context() else {
            return Err("Open a drum clip in the Steps view first");
        };
        let page = context.layout.first / context.layout.count as i64;
        if next && page + 1 >= i64::from(context.layout.pages) {
            return Err("This is the last page");
        }
        if !next && page == 0 {
            return Err("This is the first page");
        }
        Ok(())
    }

    /// Shows the page `by` pages on (negative for before): the keyboard's step
    /// follows to the same column.
    pub(crate) fn steps_turn_page(&mut self, by: i64) {
        let Some(context) = self.steps_context() else {
            return;
        };
        let count = context.layout.count as i64;
        let page = context.layout.first / count + by;
        self.steps_show_page(&context, page);
    }

    fn steps_show_page(&mut self, context: &Context, page: i64) {
        let count = context.layout.count as i64;
        let was = context.layout.first / count;
        let setup = context.setup.with_page(page, context.layout.pages);
        let steps = &mut self.note_editor.drum.steps;
        steps.set_setup(context.clip.id, setup);
        if let Some(cursor) = steps.cursor.as_mut() {
            let moved = i64::from(setup.page) - was;
            cursor.index =
                (cursor.index + moved * count).clamp(0, (context.layout.in_clip - 1).max(0));
        }
    }

    /// The keys of a focused grid, before the commands see them.
    pub(crate) fn steps_handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused()
            || self.typing
            || self.rebind.is_some()
            || self.window_open()
            || Popup::is_any_open(ctx)
        {
            return;
        }
        let Some(context) = self.steps_context() else {
            return;
        };
        if !ctx.memory(|m| m.has_focus(grid_id(context.clip.id))) {
            return;
        }
        let keys = ctx.input_mut(|i| take_keys(&mut i.events));
        if keys.is_empty() {
            return;
        }
        let mut ed = std::mem::take(&mut self.note_editor);
        for key in keys {
            self.steps_key(&mut ed, key);
        }
        self.note_editor = ed;
        ctx.request_repaint();
    }

    fn steps_key(&mut self, ed: &mut NoteEditor, key: StepKey) {
        // Each key reads the grid as the last one left it.
        let Some(id) = ed.clip else {
            return;
        };
        let Some((clip, _)) = self.editor_clip(id) else {
            return;
        };
        let meter = self.transport.timeline.meter.meter_at(clip.start);
        let setup = ed.drum.steps.setup(id);
        let Some(layout) = setup.layout(lumen_project::edit::content_length(&clip), meter) else {
            return;
        };
        let Some(lanes) = self.drum_lanes(id) else {
            return;
        };
        let pitches: Vec<u8> = lanes.iter().map(|lane| lane.input).collect();
        let Some(&first_pitch) = pitches.first() else {
            return;
        };
        let cursor = ed
            .drum
            .steps
            .cursor
            .filter(|c| pitches.contains(&c.pitch) && c.index < layout.in_clip.max(1))
            .unwrap_or(Target {
                pitch: first_pitch,
                index: layout.first,
            });
        let row = pitches.iter().position(|p| *p == cursor.pitch).unwrap_or(0);
        match key {
            StepKey::Move(movement) => {
                let (row, index) = input::step_cursor(
                    (row, cursor.index),
                    movement,
                    pitches.len(),
                    layout.in_clip,
                );
                ed.drum.steps.cursor = Some(Target {
                    pitch: pitches[row],
                    index,
                });
                let page = index / layout.count as i64;
                ed.drum
                    .steps
                    .set_setup(id, setup.with_page(page, layout.pages));
            }
            StepKey::Page(by) => {
                let page = layout.first / layout.count as i64 + i64::from(by);
                let next = setup.with_page(page, layout.pages);
                let moved = i64::from(next.page) - layout.first / layout.count as i64;
                ed.drum.steps.set_setup(id, next);
                ed.drum.steps.cursor = Some(Target {
                    pitch: cursor.pitch,
                    index: (cursor.index + moved * layout.count as i64)
                        .clamp(0, (layout.in_clip - 1).max(0)),
                });
            }
            StepKey::Toggle { accent } => {
                ed.drum.steps.cursor = Some(cursor);
                if cursor.index >= layout.in_clip {
                    return;
                }
                let cell = model::cell(notes_of(&clip), cursor, layout.step);
                let change = if accent {
                    input::accent_toggle(
                        cursor,
                        cell.on,
                        cell.on && cell.velocity >= setup.accent,
                        setup.accent,
                    )
                } else {
                    Change::Toggle(cursor, STEP_VELOCITY)
                };
                self.commit_step(ed, &clip, layout.step, change);
            }
        }
    }

    /// Commits one change to the grid: one undo step, the notes the step
    /// leaves on picked (the editor's selection), and the sound heard when the
    /// step is on.
    pub(crate) fn commit_step(
        &mut self,
        ed: &mut NoteEditor,
        clip: &Arc<Clip>,
        step: Ticks,
        change: Change,
    ) {
        if !self.document.notes_editable(clip.id) {
            return;
        }
        let picked = self.document.edit_steps(clip.id, step, &[change], None);
        let heard = picked.first().copied().and_then(|i| {
            let (open, _) = self.editor_clip(clip.id)?;
            notes_of(&open).get(i).copied()
        });
        self.select_notes(ed, picked);
        if let Some(note) = heard {
            self.audition_note(clip.id, note.pitch(), note.velocity());
        }
    }

    /// Draws the grid: the name column, the strip, the cells and what the
    /// pointer and the keyboard do on them.
    pub(crate) fn draw_steps(&mut self, ui: &mut Ui, frame: StepsFrame<'_>) -> StepsOut {
        let StepsFrame {
            ed,
            clip,
            lanes,
            map_id,
            map_name,
            sounds,
            editable,
            area,
            strip,
            keys,
            body,
            surface,
        } = frame;
        let ctx = ui.ctx().clone();
        let response = ui.interact(body, grid_id(clip.id), Sense::click_and_drag());
        let blank = |response: egui::Response| StepsOut {
            response,
            over_body: None,
            hover: None,
        };
        let content = lumen_project::edit::content_length(clip);
        let meter = self.transport.timeline.meter.meter_at(clip.start);
        let mut setup = ed.drum.steps.setup(clip.id);
        let Some(layout) = setup.layout(content, meter) else {
            return blank(response);
        };
        // The page the layout kept inside the clip is the page.
        let page = (layout.first / layout.count as i64) as u32;
        if page != setup.page {
            setup.page = page;
            ed.drum.steps.set_setup(clip.id, setup);
        }
        let pitches: Vec<u8> = lanes.iter().map(|lane| lane.input).collect();

        // The geometry: the page fills the body, so the editor's own
        // `x_of` agrees with the cells.
        let mut view = EditorView {
            body,
            time: geometry::time_view(body.width(), clip.start, &layout),
            clip_start: clip.start,
            row_height: ROW,
            scroll: ed.scroll,
            rows: Rows::lanes(pitches.iter().copied()),
            lanes: true,
        };
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let layer = ui.layer_id();
        let on_top = |p: Pos2| ctx.layer_id_at(p) == Some(layer);
        let over_area = pointer.filter(|p| area.contains(*p) && on_top(*p));
        if over_area.is_some() {
            let (delta, ctrl) = ctx.input(|i| (i.smooth_scroll_delta, i.modifiers.command));
            if delta.y != 0.0 && !ctrl {
                view.scroll -= delta.y;
            }
        }
        view.clamp_scroll();
        ed.scroll = view.scroll;
        ed.time = view.time;
        ed.zooms.clear();
        ed.hovered = over_area.is_some();
        let over_body = pointer.filter(|p| body.contains(*p) && on_top(*p));
        ed.pointer_at = over_body.map(|p| (view.local_at(p.x), view.pitch_at(p.y)));
        if response.clicked() || response.drag_started() {
            ed.lane.focus = false;
        }

        let grid = Grid {
            body,
            layout,
            scroll: view.scroll,
            pitches: &pitches,
        };
        ed.drum.steps.drawn = Some(Drawn {
            layout,
            body,
            row_height: ROW,
            scroll: view.scroll,
            rows: pitches.clone(),
        });

        // The ground, the names, and the empty state.
        self.drum_surface(
            ui,
            &ctx,
            surface,
            None,
            crate::app::clip::clip_drum::DrumFrame {
                ed,
                view: &view,
                clip,
                lanes,
                map_id,
                map_name,
                sounds,
                keys,
                steps: true,
            },
        );

        // The strip: the grid's own controls.
        if let Some(next) = self.steps_strip(ui, strip, clip, setup, &layout) {
            ed.drum.steps.set_setup(clip.id, next);
            ed.drum.steps.gesture = None;
        }

        if lanes.is_empty() {
            return blank(response);
        }

        let notes = notes_of(clip);
        let cells = ed.drum.steps.cells_for(clip, notes, &pitches, &layout);

        // The pointer: a press on a step starts a gesture; a vertical drag
        // sets its velocity; the release commits one undo step.
        let modifiers = ctx.input(|i| i.modifiers);
        let (pressed, down, latest, escape) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.latest_pos(),
                i.key_pressed(Key::Escape),
            )
        });
        let hover_cell = over_body.and_then(|p| grid.cell_at(p));
        if pressed
            && editable
            && ed.drum.steps.gesture.is_none()
            && let Some(p) = over_body
            && let Some((row, column)) = hover_cell
            && !grid.past_end(grid.index_of(column))
        {
            let target = Target {
                pitch: pitches[row],
                index: grid.index_of(column),
            };
            let cell = cells
                .get(row * layout.count + column)
                .copied()
                .unwrap_or_default();
            ed.drum.steps.gesture = Some(Gesture::press(
                target,
                row,
                column,
                &cell,
                p,
                modifiers.shift,
                setup.accent,
            ));
            ed.drum.steps.cursor = Some(target);
            response.request_focus();
        }
        let mut live = None;
        let mut chip = None;
        if let Some(mut gesture) = ed.drum.steps.gesture {
            if escape || !editable {
                ed.drum.steps.gesture = None;
            } else if down {
                if let Some(p) = latest {
                    gesture.drag(p);
                }
                if gesture.is_drag() {
                    live = Some(Live {
                        row: gesture.row,
                        column: gesture.column,
                        velocity: gesture.velocity(),
                    });
                    chip = Some(paint::velocity_chip(gesture.velocity(), setup.accent));
                }
                ed.drum.steps.gesture = Some(gesture);
                ctx.request_repaint();
            } else {
                ed.drum.steps.gesture = None;
                self.commit_step(ed, clip, layout.step, gesture.release(setup.accent));
            }
        }

        // What the frame shows (the document may have changed above).
        let cells = if ed.drum.steps.gesture.is_none() {
            let now = self.editor_clip(clip.id).map(|(c, _)| c);
            match now {
                Some(now) if !Arc::ptr_eq(&now, clip) => {
                    ed.drum
                        .steps
                        .cells_for(&now, notes_of(&now), &pitches, &layout)
                }
                _ => cells,
            }
        } else {
            cells
        };
        let focused = response.has_focus();
        if focused {
            ctx.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..EventFilter::default()
                    },
                );
            });
            if ed.drum.steps.cursor.is_none() {
                ed.drum.steps.cursor = Some(Target {
                    pitch: pitches[0],
                    index: layout.first,
                });
            }
        }
        ed.drum.steps.focused = focused;
        let cursor_cell = ed
            .drum
            .steps
            .cursor
            .filter(|_| focused)
            .and_then(|c| Some((grid.row_of(c.pitch)?, grid.column_of(c.index)?)));
        let picked = picked_cells(&grid, notes, ed.picked());
        let playing = self
            .transport
            .state()
            .playing
            .then(|| {
                let local = self.transport.playhead() - clip.start;
                if local < Ticks::ZERO {
                    return None;
                }
                let index = local.0 / layout.step.0;
                (!grid.past_end(index))
                    .then(|| grid.column_of(index))
                    .flatten()
            })
            .flatten();
        ed.drum.steps.playing = playing.map(|column| grid.index_of(column));
        let scene = Scene {
            grid,
            lanes,
            cells: &cells,
            accent: setup.accent,
            playing,
            hover: hover_cell.filter(|_| ed.drum.steps.gesture.is_none() || live.is_some()),
            cursor: cursor_cell,
            picked: &picked,
            live,
        };
        let painter = ui.painter().clone();
        paint::grid(&painter, &scene);
        paint::overlay(&painter, &scene);
        if let (Some(chip), Some(p)) = (&chip, latest) {
            widgets::draw_hint(&painter, p + vec2(14.0, -16.0), chip, body);
        }
        if playing.is_some() {
            ctx.request_repaint();
        }

        // The accessible name says where the keyboard's step is and what it
        // holds; the info line says what the pointer can do.
        let name = self.step_name(
            clip,
            lanes,
            &grid,
            ed.drum.steps.cursor,
            &cells,
            setup.accent,
        );
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, name.clone()));
        if over_body.is_some() && !Popup::is_any_open(&ctx) {
            let mut info = Info::new("Step grid")
                .on(Held::NONE, Mouse::Click, "toggle a step")
                .on(Held::NONE, Mouse::Drag, "set its velocity")
                .on(Held::SHIFT, Mouse::Click, "toggle its accent")
                .on(Held::NONE, Mouse::RightClick, "menu");
            if let Some((row, column)) = hover_cell
                && let Some(lane) = lanes.get(row)
            {
                info = Info::new(format!("{}, step {}", lane.name, grid.index_of(column) + 1))
                    .on(Held::NONE, Mouse::Click, "toggle")
                    .on(Held::NONE, Mouse::Drag, "set velocity")
                    .on(Held::SHIFT, Mouse::Click, "toggle accent")
                    .on(Held::NONE, Mouse::RightClick, "menu");
            }
            self.set_info(info);
        }
        if over_body.is_some() || ed.drum.steps.gesture.is_some() {
            ctx.set_cursor_icon(if ed.drum.steps.gesture.is_some_and(|g| g.is_drag()) {
                egui::CursorIcon::ResizeVertical
            } else if hover_cell.is_some() && editable {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Default
            });
        }
        // The note under the pointer, for the right-click menu.
        let hover = hover_cell.and_then(|(row, column)| {
            let target = Target {
                pitch: pitches[row],
                index: grid.index_of(column),
            };
            model::owned(notes, target, layout.step)
                .find(|&i| notes[i].pitch() == target.pitch && !notes[i].muted())
                .or_else(|| {
                    model::owned(notes, target, layout.step)
                        .find(|&i| notes[i].pitch() == target.pitch)
                })
                .map(|i| (i, Zone::Body))
        });
        ed.last_view = Some(view);
        StepsOut {
            response,
            over_body,
            hover,
        }
    }

    /// "Step grid, Beat. Snare, step 5 of 16, on, velocity 100, accent": what
    /// the grid says to assistive technology, for the step the keyboard is on.
    fn step_name(
        &self,
        clip: &Clip,
        lanes: &[Lane],
        grid: &Grid<'_>,
        cursor: Option<Target>,
        cells: &[Cell],
        accent: u8,
    ) -> String {
        let head = format!("Step grid: {}", clip.name);
        let Some(cursor) = cursor else {
            return head;
        };
        let (Some(row), Some(column)) = (grid.row_of(cursor.pitch), grid.column_of(cursor.index))
        else {
            return head;
        };
        let cell = cells
            .get(row * grid.layout.count + column)
            .copied()
            .unwrap_or_default();
        let sound = lanes.get(row).map_or("", |lane| lane.name.as_str());
        let mut text = format!(
            "{head}. {sound}, step {} of {}",
            cursor.index + 1,
            grid.layout.in_clip
        );
        if cell.on {
            text.push_str(&format!(", on, velocity {}", cell.velocity));
            if cell.velocity >= accent {
                text.push_str(", accent");
            }
            if cell.off_grid {
                text.push_str(", off the grid");
            }
        } else if cell.held {
            text.push_str(", off, hit kept muted");
        } else {
            text.push_str(", off");
        }
        text
    }

    /// The strip over the grid: how many steps a page has, how fine a step
    /// is, the accent, and the page. Answers the setup the user changed.
    fn steps_strip(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        clip: &Clip,
        setup: Setup,
        layout: &GridLayout,
    ) -> Option<Setup> {
        let p = theme::p();
        let painter = ui.painter().clone();
        painter.rect_filled(rect, 0.0, p.header);
        painter.hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(theme::stroke::HAIRLINE, p.border_strong),
        );
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(theme::stroke::HAIRLINE, p.border),
        );
        let inner = rect.shrink2(vec2(theme::space::M, 0.0));
        let clip_rect = rect.intersect(ui.clip_rect());
        let mut next = setup;
        let id = ui.id().with(("steps strip", clip.id.0));
        let caption = |ui: &mut Ui, text: &str| {
            ui.label(
                RichText::new(text)
                    .size(theme::size::CAPTION)
                    .color(theme::p().text_secondary),
            );
        };

        // The page, at the right (drawn first, so the left takes what is left).
        let mut right = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::right_to_left(Align::Center)),
        );
        right.set_clip_rect(clip_rect);
        right.spacing_mut().item_spacing.x = theme::space::S;
        if layout.pages > 1 {
            let page = setup.page.min(layout.pages - 1);
            right.label(
                RichText::new(format!("of {}", layout.pages))
                    .monospace()
                    .size(theme::size::SCALE)
                    .color(p.text_hint),
            );
            if layout.pages <= NUMBERED_PAGES {
                let choices: Vec<Choice<'_, u32>> = (0..layout.pages)
                    .map(|n| {
                        Choice::new(n, PAGE_NUMBERS[n as usize])
                            .named(PAGE_NAMES[n as usize])
                            .tip("Show this page of the clip's steps")
                    })
                    .collect();
                if let Some(chosen) =
                    widgets::segmented(&mut right, id.with("pages"), page, &choices)
                {
                    next = next.with_page(i64::from(chosen), layout.pages);
                }
            } else {
                let forward =
                    raised::button(&mut right, "Next").on_hover_text("Show the next page of steps");
                if forward.clicked() {
                    next = next.with_page(i64::from(page) + 1, layout.pages);
                }
                let back = raised::button(&mut right, "Previous")
                    .on_hover_text("Show the previous page of steps");
                if back.clicked() {
                    next = next.with_page(i64::from(page) - 1, layout.pages);
                }
                right.label(
                    RichText::new(format!("{} of {}", page + 1, layout.pages))
                        .monospace()
                        .size(theme::size::SCALE)
                        .color(p.text_hint),
                );
            }
            caption(&mut right, "Page");
        }
        let used = right.min_rect();
        let end = if used.width() > 0.0 {
            used.left() - theme::space::M
        } else {
            inner.right()
        };
        let room = Rect::from_x_y_ranges(inner.left()..=end.max(inner.left()), inner.y_range());
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(room)
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.set_clip_rect(Rect::from_x_y_ranges(
            clip_rect.left()..=end.max(clip_rect.left()),
            clip_rect.y_range(),
        ));
        row.spacing_mut().item_spacing.x = theme::space::M;

        let counts: Vec<Choice<'_, u32>> = COUNTS
            .iter()
            .zip(COUNT_NAMES)
            .map(|(n, (label, name))| {
                Choice::new(*n, label)
                    .named(name)
                    .tip("How many steps a page shows")
            })
            .collect();
        if let Some(count) = widgets::segmented(&mut row, id.with("count"), setup.count, &counts) {
            next.count = count;
            next.page = 0;
        }
        caption(&mut row, "steps");
        let resolution = RESOLUTIONS[setup.resolution.min(RESOLUTIONS.len() - 1)];
        let button =
            raised::button(&mut row, &resolution.label()).on_hover_text("How long a step is");
        widgets::Menu::below(&button).show(|ui| {
            for (n, r) in RESOLUTIONS.iter().enumerate() {
                if widgets::menu_toggle(ui, &r.label(), None, n == setup.resolution).clicked() {
                    next.resolution = n;
                    next.page = 0;
                    ui.close();
                }
            }
        });
        caption(&mut row, "grid");
        let accent = raised::button(&mut row, &format!("Accent {}", setup.accent))
            .on_hover_text("The velocity an accent sets (Shift-click a step)");
        widgets::Menu::below(&accent).show(|ui| {
            for a in ACCENTS {
                if widgets::menu_toggle(ui, &format!("{a}"), None, a == setup.accent).clicked() {
                    next.accent = a;
                    ui.close();
                }
            }
        });
        (next != setup).then_some(next)
    }
}

/// The page's names for assistive technology.
const PAGE_NAMES: [&str; NUMBERED_PAGES as usize] = [
    "Page 1", "Page 2", "Page 3", "Page 4", "Page 5", "Page 6", "Page 7", "Page 8",
];

/// The steps of the page whose notes are picked: the picked notes in the
/// page's window only, found by two searches (the picks are in note order).
fn picked_cells(
    grid: &Grid<'_>,
    notes: &[lumen_project::model::Note],
    picked: &[usize],
) -> Vec<(usize, usize)> {
    let step = grid.layout.step;
    let from = model::start_of(grid.layout.first, step);
    let to = model::start_of(grid.layout.first + grid.layout.count as i64, step);
    let low = notes.partition_point(|n| n.start() < from);
    let high = notes.partition_point(|n| n.start() < to);
    let first = picked.partition_point(|i| *i < low);
    let last = picked.partition_point(|i| *i < high);
    let mut cells: Vec<(usize, usize)> = picked[first..last]
        .iter()
        .filter_map(|&i| {
            let note = notes.get(i)?;
            let row = grid.row_of(note.pitch())?;
            let index = model::index_of(note.start(), step)?;
            Some((row, grid.column_of(index)?))
        })
        .collect();
    cells.sort_unstable();
    cells.dedup();
    cells
}

/// A press or a drag that needs a grid for its place is ignored when the
/// pointer is past the last row or the page's edge: kept here so the tests of
/// the pointer's mapping live with it.
#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;
    use lumen_core::time::PPQ;
    use lumen_project::model::Note;

    fn grid<'a>(pitches: &'a [u8], first: i64) -> Grid<'a> {
        Grid {
            body: Rect::from_min_max(pos2(0.0, 0.0), pos2(320.0, ROW * 3.0)),
            layout: GridLayout {
                step: Ticks(PPQ / 4),
                count: 16,
                first,
                in_clip: 32,
                pages: 2,
                group: 4,
            },
            scroll: 0.0,
            pitches,
        }
    }

    #[test]
    fn the_picked_steps_are_the_picked_notes_on_the_page_in_their_rows() {
        let pitches = [36_u8, 38];
        let step = PPQ / 4;
        let notes = vec![
            Note::new(Ticks(0), Ticks(step), 36, 100),
            Note::new(Ticks(step * 3), Ticks(step), 38, 100),
            Note::new(Ticks(step * 3 + 5), Ticks(step), 38, 90),
            Note::new(Ticks(step * 17), Ticks(step), 36, 100),
        ];
        let first = grid(&pitches, 0);
        assert_eq!(
            picked_cells(&first, &notes, &[0, 1, 2, 3]),
            vec![(0, 0), (1, 3)],
            "two notes in one step are one step; the next page's is not here"
        );
        let second = grid(&pitches, 16);
        assert_eq!(picked_cells(&second, &notes, &[0, 1, 2, 3]), vec![(0, 1)]);
        assert!(picked_cells(&first, &notes, &[]).is_empty());
    }

    #[test]
    fn the_keys_the_grid_takes_are_the_arrows_space_enter_and_pages_only() {
        let key = |key, modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let mut events = vec![
            key(Key::Space, Modifiers::NONE),
            key(Key::Space, Modifiers::SHIFT),
            key(Key::ArrowRight, Modifiers::NONE),
            key(Key::ArrowRight, Modifiers::ALT),
            key(Key::Enter, Modifiers::NONE),
            key(Key::PageDown, Modifiers::NONE),
            key(Key::Delete, Modifiers::NONE),
            key(Key::Space, Modifiers::COMMAND),
        ];
        let taken = take_keys(&mut events);
        assert_eq!(
            taken,
            vec![
                StepKey::Toggle { accent: false },
                StepKey::Toggle { accent: true },
                StepKey::Move(Move::Right),
                StepKey::Toggle { accent: false },
                StepKey::Page(1),
            ]
        );
        // What it did not take stays for the commands (Delete, Alt+Right,
        // Ctrl+Space).
        assert_eq!(events.len(), 3, "{events:?}");
        // And a release is not a press.
        let mut up = vec![Event::Key {
            key: Key::Space,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }];
        assert!(take_keys(&mut up).is_empty());
        assert_eq!(up.len(), 1);
    }
}
