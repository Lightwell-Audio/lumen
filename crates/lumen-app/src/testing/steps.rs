//! Test seams for the step sequencer (M8 D3): where a step is on the screen,
//! what it holds, what the grid is set to and where the keyboard is.

use eframe::egui::Pos2;
use lumen_project::edit::steps::{self as model, Target};

use crate::App;
use crate::drum_editor::steps::geometry::Grid;

/// What one step of the open clip holds, as the grid shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepCell {
    pub on: bool,
    pub velocity: u8,
    pub off_grid: bool,
    /// A muted hit is kept under the step (a step turned off that was not a
    /// plain grid hit).
    pub held: bool,
}

/// How the grid of the open clip is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepsSetup {
    pub count: u32,
    pub resolution: String,
    /// The page shown, from 0.
    pub page: u32,
    pub pages: u32,
    pub accent: u8,
}

impl App {
    /// Where the middle of the step `index` of the sound `pitch` is, as the
    /// grid drew it last; `None` when it is on another page or scrolled out.
    pub fn steps_cell_point(&self, pitch: u8, index: i64) -> Option<Pos2> {
        let drawn = self.note_editor.drum.steps.drawn.as_ref()?;
        let grid = Grid {
            body: drawn.body,
            layout: drawn.layout,
            scroll: drawn.scroll,
            pitches: &drawn.rows,
        };
        let row = grid.row_of(pitch)?;
        let column = grid.column_of(index)?;
        let rect = grid.cell_rect(row, column);
        drawn.body.contains(rect.center()).then(|| rect.center())
    }

    /// What the step holds: `None` when no clip is open as the grid.
    pub fn steps_cell(&self, pitch: u8, index: i64) -> Option<StepCell> {
        let drawn = self.note_editor.drum.steps.drawn.as_ref()?;
        let clip = self.editor_open_clip()?;
        let (open, _) = self.editor_clip(clip)?;
        let cell = model::cell(
            crate::note_editor::notes_of(&open),
            Target { pitch, index },
            drawn.layout.step,
        );
        Some(StepCell {
            on: cell.on,
            velocity: cell.velocity,
            off_grid: cell.off_grid,
            held: cell.held,
        })
    }

    /// The grid's setting and the pages it has.
    pub fn steps_setup(&self) -> Option<StepsSetup> {
        let drawn = self.note_editor.drum.steps.drawn.as_ref()?;
        let clip = self.editor_open_clip()?;
        let setup = self.note_editor.drum.steps.setup(clip);
        Some(StepsSetup {
            count: setup.count,
            resolution: crate::drum_editor::steps::RESOLUTIONS[setup
                .resolution
                .min(crate::drum_editor::steps::RESOLUTIONS.len() - 1)]
            .label(),
            page: (drawn.layout.first / drawn.layout.count as i64) as u32,
            pages: drawn.layout.pages,
            accent: setup.accent,
        })
    }

    /// The first and last steps (indices) the page shows.
    pub fn steps_page_range(&self) -> Option<(i64, i64)> {
        let drawn = self.note_editor.drum.steps.drawn.as_ref()?;
        Some((
            drawn.layout.first,
            drawn.layout.first + drawn.layout.count as i64 - 1,
        ))
    }

    /// The step the keyboard is on: the sound and the index.
    pub fn steps_cursor(&self) -> Option<(u8, i64)> {
        let cursor = self.note_editor.drum.steps.cursor?;
        Some((cursor.pitch, cursor.index))
    }

    /// Whether the grid had the keyboard on the last frame.
    pub fn steps_has_keyboard(&self) -> bool {
        self.note_editor.drum.steps.focused
    }

    /// The step the playhead is in (its index), while playing.
    pub fn steps_playing_step(&self) -> Option<i64> {
        self.note_editor.drum.steps.playing
    }

    /// How many notes of the open clip are picked.
    pub fn picked_note_count(&self) -> usize {
        self.note_editor.picked().len()
    }

    /// Whether the grid is the view the open clip's track shows, as the
    /// header's switch names it.
    pub fn steps_shown(&self) -> bool {
        self.drum_view_shown() == Some("steps")
    }
}
