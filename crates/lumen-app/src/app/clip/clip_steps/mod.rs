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

mod draw;
mod strip;

const NUMBERED_PAGES: u32 = 8;
const COUNT_NAMES: [(&str, &str); 4] = [
    ("8", "8 steps a page"),
    ("16", "16 steps a page"),
    ("32", "32 steps a page"),
    ("64", "64 steps a page"),
];
const PAGE_NUMBERS: [&str; NUMBERED_PAGES as usize] = ["1", "2", "3", "4", "5", "6", "7", "8"];

pub(crate) fn grid_id(clip: Id) -> egui::Id {
    egui::Id::new(("drum steps grid", clip.0))
}

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

pub(crate) struct StepsOut {
    pub response: egui::Response,
    pub over_body: Option<Pos2>,
    pub hover: Option<(usize, Zone)>,
}

struct Context {
    clip: Arc<Clip>,
    layout: GridLayout,
    setup: Setup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepKey {
    Move(Move),
    Toggle { accent: bool },
    Page(i32),
}

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
}

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

        assert_eq!(events.len(), 3, "{events:?}");

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
