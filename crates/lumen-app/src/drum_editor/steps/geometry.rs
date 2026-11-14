//! Where the step grid's cells are (M8 slice D3): the page's width divided into
//! its steps, a row a sound at [`ROW`] points, the cell a pointer is over, and
//! the time axis that makes the editor's own geometry (`EditorView`: the
//! ruler's playhead, the grid lines, `x_of`) agree with the cells.
//!
//! A cell is the specimen's inset well: `INSET` points in from its step's
//! edges on every side. Nothing here reads the document.

use eframe::egui::{Pos2, Rect, pos2, vec2};
use lumen_core::time::Ticks;

use super::Layout;
use crate::theme;
use crate::timeview::TimeView;

/// A row's height (the specimen's 34 pt).
pub(crate) const ROW: f32 = theme::size::STEP_ROW;
/// How far a cell is inset from its step's edges.
pub(crate) const INSET: f32 = theme::size::STEP_INSET;
/// The strip's height above the rows (the grid's controls).
pub(crate) const STRIP: f32 = theme::size::STEP_STRIP;

/// The time axis that fits the page into `width` points: the page's first step
/// at the body's left edge, its last one ending at the right edge. The
/// editor's `EditorView` is built with it, so the playhead and `x_of` agree
/// with the cells.
pub(crate) fn time_view(width: f32, clip_start: Ticks, layout: &Layout) -> TimeView {
    let quarters = layout.step.as_quarters() * layout.count as f64;
    let zoom = (f64::from(width.max(1.0)) / quarters.max(1e-9)).max(1e-6);
    let first = clip_start + Ticks(layout.step.0 * layout.first);
    TimeView {
        left: first.as_quarters(),
        zoom,
    }
}

/// The grid as it is on the screen: the rows' body, the page's layout, the
/// scroll, and which sound each row is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grid<'a> {
    pub body: Rect,
    pub layout: Layout,
    pub scroll: f32,
    pub pitches: &'a [u8],
}

impl Grid<'_> {
    /// A step's width on the screen.
    pub fn step_width(&self) -> f32 {
        self.body.width() / self.layout.count as f32
    }

    /// The whole step of column `k` of row `row`, before the inset.
    pub fn slot(&self, row: usize, k: usize) -> Rect {
        let w = self.step_width();
        let left = self.body.left() + k as f32 * w;
        let top = self.body.top() + row as f32 * ROW - self.scroll;
        Rect::from_min_size(pos2(left, top), vec2(w, ROW))
    }

    /// The cell's well, inset from its step on every side (never negative).
    pub fn cell_rect(&self, row: usize, k: usize) -> Rect {
        let slot = self.slot(row, k);
        let inset = INSET.min(slot.width() / 4.0).min(slot.height() / 4.0);
        slot.shrink(inset)
    }

    /// The rows that are in view: the first and one past the last.
    pub fn rows_in_view(&self) -> (usize, usize) {
        let rows = self.pitches.len();
        let first = ((self.scroll / ROW).floor().max(0.0) as usize).min(rows);
        let last = ((((self.scroll + self.body.height()) / ROW).ceil() as usize) + 1).min(rows);
        (first, last.max(first))
    }

    /// The row and column under `p`, if one is there: no row below the last
    /// sound, no column past the page.
    pub fn cell_at(&self, p: Pos2) -> Option<(usize, usize)> {
        if !self.body.contains(p) {
            return None;
        }
        let row = ((p.y - self.body.top() + self.scroll) / ROW).floor();
        if row < 0.0 || row as usize >= self.pitches.len() {
            return None;
        }
        let k = ((p.x - self.body.left()) / self.step_width()).floor();
        (k >= 0.0 && (k as usize) < self.layout.count).then_some((row as usize, k as usize))
    }

    /// The row of the sound `pitch`, if the grid has it.
    pub fn row_of(&self, pitch: u8) -> Option<usize> {
        self.pitches.iter().position(|p| *p == pitch)
    }

    /// The step index of column `k`.
    pub fn index_of(&self, k: usize) -> i64 {
        self.layout.first + k as i64
    }

    /// The column of step `index`, when the page has it.
    pub fn column_of(&self, index: i64) -> Option<usize> {
        let k = index - self.layout.first;
        (k >= 0 && (k as usize) < self.layout.count).then_some(k as usize)
    }

    /// Whether step `index` is past the clip's end: the grid shows it quiet
    /// and a press there does nothing.
    pub fn past_end(&self, index: i64) -> bool {
        index >= self.layout.in_clip
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::PPQ;

    fn layout() -> Layout {
        Layout {
            step: Ticks(PPQ / 4),
            count: 16,
            first: 16,
            in_clip: 24,
            pages: 2,
            group: 4,
        }
    }

    fn grid(pitches: &[u8]) -> Grid<'_> {
        Grid {
            body: Rect::from_min_max(pos2(200.0, 60.0), pos2(1000.0, 60.0 + 3.0 * ROW)),
            layout: layout(),
            scroll: 0.0,
            pitches,
        }
    }

    #[test]
    fn the_axis_puts_the_pages_first_step_at_the_left_and_its_end_at_the_right() {
        let l = layout();
        let clip_start = Ticks::quarters(8);
        let view = time_view(800.0, clip_start, &l);
        // The page starts four beats into the clip (16 sixteenths in).
        let first = (clip_start + Ticks(l.step.0 * l.first)).as_quarters();
        assert_eq!(view.x(200.0, first), 200.0);
        let end = first + 4.0;
        assert!((view.x(200.0, end) - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn a_cell_is_inset_from_its_step_and_found_by_the_pointer() {
        let pitches = [36_u8, 38, 42];
        let g = grid(&pitches);
        assert_eq!(g.step_width(), 50.0);
        let slot = g.slot(1, 2);
        assert_eq!(slot.left(), 300.0);
        assert_eq!(slot.top(), 60.0 + ROW);
        let cell = g.cell_rect(1, 2);
        assert_eq!(cell.left(), slot.left() + INSET);
        assert_eq!(cell.top(), slot.top() + INSET);
        assert_eq!(cell.width(), 50.0 - 2.0 * INSET);
        // The pointer maps back to the cell, in the inset or the gap.
        assert_eq!(g.cell_at(cell.center()), Some((1, 2)));
        assert_eq!(g.cell_at(slot.min + vec2(0.5, 0.5)), Some((1, 2)));
        assert_eq!(g.cell_at(pos2(199.0, 70.0)), None, "left of the grid");
        assert_eq!(g.cell_at(pos2(500.0, 60.0 + 3.0 * ROW - 0.5)), Some((2, 6)));
        let few = [36_u8, 38];
        assert_eq!(
            grid(&few).cell_at(pos2(500.0, 60.0 + 2.0 * ROW + 1.0)),
            None,
            "no row below the last sound"
        );
    }

    #[test]
    fn a_scrolled_grid_finds_its_rows_where_they_are_drawn() {
        let pitches = [36_u8, 38, 42, 46, 49];
        let mut g = grid(&pitches);
        g.scroll = ROW * 1.5;
        let (first, last) = g.rows_in_view();
        assert_eq!(first, 1);
        assert!(last > first && last <= 5);
        let cell = g.cell_rect(2, 0);
        assert_eq!(g.cell_at(cell.center()), Some((2, 0)));
        assert_eq!(g.row_of(46), Some(3));
        assert_eq!(g.row_of(40), None);
    }

    #[test]
    fn steps_map_to_columns_of_the_page_and_the_clips_end_is_known() {
        let pitches = [36_u8];
        let g = grid(&pitches);
        assert_eq!(g.index_of(3), 19);
        assert_eq!(g.column_of(19), Some(3));
        assert_eq!(g.column_of(15), None, "the page before");
        assert_eq!(g.column_of(32), None, "the page after");
        assert!(!g.past_end(23));
        assert!(g.past_end(24), "the clip holds 24 steps");
    }

    #[test]
    fn a_very_narrow_cell_is_never_inset_away() {
        let pitches = [36_u8];
        let mut g = grid(&pitches);
        g.body = Rect::from_min_max(pos2(0.0, 0.0), pos2(32.0, ROW));
        let cell = g.cell_rect(0, 0);
        assert!(cell.width() > 0.0 && cell.height() > 0.0, "{cell:?}");
    }
}
