//! Where the lanes and their hits are (M8 slice D2): the geometry the lanes
//! share with the piano roll's [`EditorView`], and what is drum-only — a hit's
//! diamond, the lane under a pointer, and the hits gathered per pixel so only
//! what is in view is drawn (and a clip of a hundred thousand notes costs what
//! its picture costs).
//!
//! A lane's height is fixed at [`ROW`] (P's 28 pt: `2 × space::M + space::XS`,
//! D1's table row) and its name column is [`NAME`] (176 pt, D1's key column
//! and the specimen's). A hit is a diamond whose left tip sits on the onset, as
//! a note's left edge does, and its half-width is [`hit_radius`]: a third of
//! the row, no more than half the grid step, so neighbours stay apart until the
//! zoom packs them.

use eframe::egui::{Pos2, Rect, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use crate::note_editor::view::EditorView;
use crate::theme;

/// A lane's height in points (the design's lane row: P's 28, D1's table row).
pub(crate) const ROW: f32 = theme::size::LANE_ROW;
/// The name column's width (the design's lane column: P's lanes and D1's key
/// column).
pub(crate) const NAME: f32 = theme::size::LANE_NAME;
/// The colour bar down a lane's left edge.
pub(crate) const BAR: f32 = theme::size::LANE_BAR;
/// A sound's drawing on its lane, and where it starts.
pub(crate) const ICON: f32 = theme::size::LANE_ICON;
pub(crate) const ICON_X: f32 = theme::size::LANE_ICON_X;
/// Where a lane's name starts.
pub(crate) const NAME_X: f32 = theme::size::LANE_NAME_X;
/// The lane's mute and solo toggles: their size, their gap, and the room kept
/// between them and the column's right edge (the specimen's block).
pub(crate) const BUTTON_W: f32 = theme::size::LANE_BUTTON_W;
pub(crate) const BUTTON_H: f32 = theme::size::LANE_BUTTON_H;
const BUTTON_GAP: f32 = theme::size::LANE_BUTTON_GAP;
const BUTTONS_RIGHT: f32 = theme::size::LANE_BUTTONS_RIGHT;
const BUTTONS: f32 = 2.0 * BUTTON_W + BUTTON_GAP;
/// The most of the row a hit's diamond takes, either way.
const HIT_ROW: f32 = theme::size::LANE_HIT_ROW;
/// The most of the grid step a hit's diamond takes.
const HIT_STEP: f32 = theme::size::LANE_HIT_STEP;
/// A hit's least half-width, so it is visible when the zoom packs many.
const HIT_LEAST: f32 = 1.5;
/// The narrowest picture column one hit is gathered into (points).
const BUCKET: f32 = 2.0;
/// The most cells one frame draws (a deeper zoom gathers more hits per cell
/// rather than drawing them all). A cell is one or two shapes, so this bounds
/// the tessellation a picture of any size costs.
pub(crate) const MAX_CELLS: usize = 2400;
/// The width of a length bar drawn from a hit (one constant, so it cannot
/// drift).
pub(crate) const LENGTH_LINE: f32 = 2.0;

/// The half-width of a hit's diamond in `view` at the grid `step`.
pub(crate) fn hit_radius(view: &EditorView, step: Ticks) -> f32 {
    let step_points = (step.as_quarters() * view.time.zoom) as f32;
    (view.row_height * HIT_ROW)
        .min(step_points * HIT_STEP)
        .max(HIT_LEAST)
}

/// Where a hit is drawn: the diamond's left tip on its onset, its middle on
/// the row's middle.
pub(crate) fn hit_rect(view: &EditorView, note: Note, step: Ticks) -> Rect {
    let r = hit_radius(view, step);
    let top = view.row_top(i32::from(note.pitch()));
    let left = view.x_of(note.start());
    Rect::from_min_max(
        pos2(left, top + view.row_height / 2.0 - r),
        pos2(left + 2.0 * r, top + view.row_height / 2.0 + r),
    )
}

/// The diamond's middle, for its outline.
pub(crate) fn hit_centre(rect: Rect) -> Pos2 {
    pos2(rect.left() + rect.width() / 2.0, rect.center().y)
}

/// Whether `p` is inside the diamond `rect` (with a point of grace).
pub(crate) fn inside_diamond(rect: Rect, p: Pos2) -> bool {
    let centre = hit_centre(rect);
    let r = rect.width() / 2.0 + 1.0;
    (p.x - centre.x).abs() + (p.y - centre.y).abs() <= r
}

/// The hit under `p`, latest first (drawn on top), if any.
pub(crate) fn hit_at(view: &EditorView, notes: &[Note], step: Ticks, p: Pos2) -> Option<usize> {
    if !view.body.contains(p) {
        return None;
    }
    notes.iter().enumerate().rev().find_map(|(i, note)| {
        let rect = hit_rect(view, *note, step);
        (rect.intersects(view.body) && inside_diamond(rect, p)).then_some(i)
    })
}

/// The lane (row) at `y`, if it is one of the rows shown: no row below the
/// last lane (M8 D2 review M1).
pub(crate) fn row_at(view: &EditorView, y: f32) -> Option<usize> {
    view.lane_at(y)
}

/// A lane's whole row across `view`'s body, for its background and its name.
pub(crate) fn row_rect(view: &EditorView, row: usize) -> Rect {
    let top = view.body.top() + row as f32 * view.row_height - view.scroll;
    Rect::from_min_max(
        pos2(view.body.left(), top),
        pos2(view.body.right(), top + view.row_height),
    )
}

/// One gathered hit: the leftmost onset in a picture column of a lane, and
/// the loudest velocity there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cell {
    pub row: usize,
    /// The left tip of the diamond (the onset) in points.
    pub x: f32,
    pub velocity: u8,
    /// One of the hits gathered here is picked.
    pub picked: bool,
    /// Every hit gathered here is muted.
    pub muted: bool,
    /// The note whose onset the cell was made from (the leftmost): hover, the
    /// length bar and the menu ask it, never the whole clip (H2).
    pub note: usize,
}

/// The picture columns a row gets: the bucket is widened until the cells fit
/// [`MAX_CELLS`]. Returns points per column.
pub(crate) fn bucket(view: &EditorView) -> f32 {
    let rows = view.rows.len().max(1);
    let by_width = BUCKET;
    let wanted = (view.body.width() / by_width).ceil().max(1.0) * rows as f32;
    if wanted <= MAX_CELLS as f32 {
        by_width
    } else {
        (view.body.width() * rows as f32 / MAX_CELLS as f32)
            .ceil()
            .max(by_width)
    }
}

/// The hits of `notes` that are in view, one cell per picture column of each
/// lane: `picked` marks the cells a picked note is in and `hidden` leaves the
/// notes a gesture is showing elsewhere out. A note outside the map's rows, or
/// off the body, is not here at all. `seen` is the caller's buffer, so a
/// frame allocates nothing (H2).
pub(crate) fn cells_into(
    view: &EditorView,
    notes: &[Note],
    picked: &[usize],
    hidden: &[usize],
    seen: &mut Vec<Option<Cell>>,
    out: &mut Vec<Cell>,
) {
    out.clear();
    let rows = view.rows.len();
    if rows == 0 || view.body.width() <= 0.0 {
        seen.clear();
        return;
    }
    let bucket = bucket(view);
    let cols = ((view.body.width() / bucket).ceil() as usize + 1).max(1);
    let first = ((view.scroll / view.row_height).floor().max(0.0) as usize).min(rows);
    let last =
        ((((view.scroll + view.body.height()) / view.row_height).ceil() as usize) + 1).min(rows);
    if last <= first {
        seen.clear();
        return;
    }
    seen.clear();
    seen.resize((last - first) * cols, None);
    for (index, note) in notes.iter().enumerate() {
        if hidden.binary_search(&index).is_ok() {
            continue;
        }
        let Some(row) = view.rows.row_of(i32::from(note.pitch())) else {
            continue;
        };
        if row < first || row >= last {
            continue;
        }
        let x = view.x_of(note.start());
        if x < view.body.left() - 2.0 || x > view.body.right() {
            continue;
        }
        // A hit past the body's right edge was skipped; the last column is
        // where a partly seen one sits.
        let col = (((x - view.body.left()) / bucket).floor() as isize).clamp(0, cols as isize - 1)
            as usize;
        let slot = &mut seen[(row - first) * cols + col];
        let picked_here = picked.binary_search(&index).is_ok();
        let muted = note.muted();
        match slot {
            Some(cell) => {
                if x < cell.x {
                    cell.x = x;
                    cell.note = index;
                }
                cell.velocity = cell.velocity.max(note.velocity());
                cell.picked |= picked_here;
                cell.muted &= muted;
            }
            None => {
                *slot = Some(Cell {
                    row,
                    x,
                    velocity: note.velocity().min(127),
                    picked: picked_here,
                    muted,
                    note: index,
                })
            }
        }
    }
    out.extend(seen.iter().flatten().copied());
}

/// The hits the pointer is on, from the gathered cells (a few thousand at
/// most) rather than the clip's notes (H2): the latest cell drawn wins, as the
/// latest note drawn does.
pub(crate) fn hit_at_cells(
    view: &EditorView,
    cells: &[Cell],
    step: Ticks,
    p: Pos2,
) -> Option<usize> {
    if !view.body.contains(p) {
        return None;
    }
    let r = hit_radius(view, step);
    cells.iter().rev().find_map(|cell| {
        let top = view.body.top() + cell.row as f32 * view.row_height - view.scroll;
        let centre = pos2(cell.x + r, top + view.row_height / 2.0);
        let d = Rect::from_center_size(centre, vec2(2.0 * r, 2.0 * r));
        inside_diamond(d, p).then_some(cell.note)
    })
}

/// Where a hit's length bar starts and ends on the lane (a lane set to show
/// length): the bar runs on from the hit's right tip to the note's end, and a
/// note no longer than its diamond has no bar.
pub(crate) fn length_bar(view: &EditorView, note: Note, step: Ticks) -> Option<(f32, f32)> {
    let r = hit_radius(view, step);
    let x = view.x_of(note.start());
    let end = view.x_of(note.end());
    (end > x + r).then_some((x + r, end))
}

/// The positions of a lane's right-hand toggles in a name column `rect` wide:
/// the specimen's two 20 pt pieces, 4 pt apart and 8 pt in from the column's
/// right edge.
pub(crate) fn buttons(name: Rect) -> (Rect, Rect) {
    let top = name.center().y - BUTTON_H / 2.0;
    let left = name.right() - BUTTONS_RIGHT - BUTTONS;
    let mute = Rect::from_min_size(pos2(left, top), vec2(BUTTON_W, BUTTON_H));
    let solo = Rect::from_min_size(
        pos2(left + BUTTON_W + BUTTON_GAP, top),
        vec2(BUTTON_W, BUTTON_H),
    );
    (mute, solo)
}

/// Where a lane's drawing is in a name column `rect` wide: 16 pt at 10 pt in
/// (the specimen's icon column).
pub(crate) fn icon_rect(name: Rect) -> Rect {
    Rect::from_min_size(
        pos2(name.left() + ICON_X, name.center().y - ICON / 2.0),
        vec2(ICON, ICON),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piano_roll::draw::Rows;
    use crate::timeview::TimeView;
    use lumen_core::time::PPQ;

    fn view(rows: Rows) -> EditorView {
        EditorView {
            body: Rect::from_min_max(pos2(200.0, 40.0), pos2(1000.0, 320.0)),
            time: TimeView {
                left: 0.0,
                zoom: 40.0,
            },
            clip_start: Ticks::ZERO,
            row_height: ROW,
            scroll: 0.0,
            rows,
            lanes: true,
        }
    }

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    #[test]
    fn a_hit_is_a_diamond_whose_left_tip_is_the_onset() {
        let rows = Rows::lanes([36_u8, 38]);
        let step = Ticks(PPQ / 4);
        let mut v = view(rows);
        // Wide enough that the row bounds the diamond, and in view.
        v.time.zoom = 400.0;
        let note = Note::new(q(1), q(0), 36, 100);
        let rect = hit_rect(&v, note, step);
        assert_eq!(rect.left(), v.x_of(q(1)));
        let r = rect.width() / 2.0;
        assert!(
            (r - (v.row_height * HIT_ROW)).abs() < 1e-4,
            "a third of the row: {r}"
        );
        assert_eq!(rect.center().y, v.body.top() + ROW / 2.0);
        // The diamond: the middle and the four tips are in, the corners out.
        let c = hit_centre(rect);
        assert!(inside_diamond(rect, c));
        assert!(inside_diamond(rect, pos2(c.x - r, c.y)));
        assert!(!inside_diamond(
            rect,
            pos2(rect.left() + 0.5, rect.top() + 0.5)
        ));
        // Found where it is drawn, not where its row is not.
        assert_eq!(hit_at(&v, &[note], step, c), Some(0));
        assert_eq!(hit_at(&v, &[note], step, c + vec2(0.0, ROW)), None);
        // A narrow zoom keeps hits apart by half the step.
        let mut near = view(Rows::lanes([36_u8, 38]));
        near.time.zoom = 10.0;
        let expected = (10.0 * 0.25 * HIT_STEP).max(HIT_LEAST);
        assert_eq!(hit_radius(&near, step), expected);
    }

    #[test]
    fn rows_run_in_the_maps_order_from_the_top() {
        let v = view(Rows::lanes([36_u8, 38, 42]));
        assert_eq!(row_at(&v, v.body.top() + 1.0), Some(0));
        assert_eq!(row_at(&v, v.body.top() + ROW + 1.0), Some(1));
        assert_eq!(
            row_at(&v, v.body.bottom() - 1.0),
            None,
            "three rows, not four"
        );
        assert_eq!(row_rect(&v, 1).top(), v.body.top() + ROW);
        // Scrolled down a row, the first row is partly above the body.
        let mut scrolled = view(Rows::lanes([36_u8, 38, 42]));
        scrolled.scroll = ROW / 2.0;
        assert_eq!(row_rect(&scrolled, 0).top(), v.body.top() - ROW / 2.0);
        assert_eq!(row_at(&scrolled, v.body.top() + 1.0), Some(0));
    }

    #[test]
    fn the_picture_gathers_hits_per_column_and_row_keeping_the_loudest() {
        let notes = [
            Note::new(Ticks(0), Ticks(60), 36, 40),
            Note::new(Ticks(1), Ticks(60), 36, 90),
            Note::new(Ticks(2), Ticks(60), 36, 70),
            Note::new(Ticks(0), Ticks(60), 42, 120),
            Note::new(Ticks(0), Ticks(60), 60, 120),
        ];
        let v = view(Rows::lanes([36_u8, 38, 42]));
        let (mut seen, mut all) = (Vec::new(), Vec::new());
        cells_into(&v, &notes, &[1], &[], &mut seen, &mut all);
        assert_eq!(all.len(), 2, "one cell a lane: {all:?}");
        assert_eq!(all[0].row, 0);
        assert_eq!(all[0].velocity, 90, "the loudest of the three");
        assert!(all[0].picked, "the picked hit is in the cell");
        assert_eq!(all[0].note, 0, "the leftmost onset's note");
        assert_eq!(all[1].row, 2);
        assert!(!all[1].picked);
        // A note of a lane the map does not show is left out.
        assert!(all.iter().all(|c| c.velocity != 120 || c.row != 1));
        // The pointer finds the cell's note without the clip's notes.
        let r = hit_radius(&v, Ticks::quarters(1));
        let centre = pos2(all[0].x + r, v.body.top() + ROW / 2.0);
        assert_eq!(
            hit_at_cells(&v, &all, Ticks::quarters(1), centre),
            Some(all[0].note)
        );
        // Drawn again: the buffers are reused, not rebuilt.
        let capacity = seen.capacity();
        cells_into(&v, &notes, &[1], &[], &mut seen, &mut all);
        assert_eq!(seen.capacity(), capacity.max(seen.capacity()));
        assert_eq!(all.len(), 2);
        // Draws nothing when no lane is in view.
        let mut scrolled = view(Rows::lanes([36_u8, 38, 42]));
        scrolled.scroll = 3.0 * ROW;
        cells_into(&scrolled, &notes, &[], &[], &mut seen, &mut all);
        assert!(all.is_empty());
        // A deeply zoomed-out view of a hundred thousand hits stays within
        // the cells a frame draws.
        let many: Vec<Note> = (0..100_000)
            .map(|i| Note::new(Ticks(i * 4), Ticks(10), 36, (i % 127) as u8))
            .collect();
        let mut fitted = view(Rows::lanes([36_u8, 38, 42]));
        fitted.time.zoom = 0.02;
        cells_into(&fitted, &many, &[], &[], &mut seen, &mut all);
        assert!(all.len() <= MAX_CELLS, "{} cells", all.len());
    }

    #[test]
    fn the_mute_and_solo_toggles_end_eight_points_in_and_the_icon_starts_at_ten() {
        // The specimen's name column (chosen.html): name.right() - 52 for the
        // block of two 20 pt pieces, so its right edge is 8 pt in; the
        // drawing at 10.
        let name = Rect::from_min_max(pos2(0.0, 0.0), pos2(NAME, ROW));
        let (mute, solo) = buttons(name);
        assert_eq!(mute.left(), NAME - 52.0);
        assert_eq!(solo.right(), NAME - 8.0);
        assert_eq!((mute.width(), mute.height()), (20.0, 18.0));
        assert_eq!(icon_rect(name).left(), 10.0);
        assert_eq!(icon_rect(name).width(), 16.0);
    }

    #[test]
    fn a_length_bar_runs_from_the_hit_to_the_notes_end() {
        let v = view(Rows::lanes([36_u8]));
        let note = Note::new(q(1), q(2), 36, 100);
        let (from, to) = length_bar(&v, note, Ticks(240)).expect("a bar");
        let rect = hit_rect(&v, note, Ticks(240));
        assert_eq!(from, hit_centre(rect).x);
        assert_eq!(to, v.x_of(q(3)));
        assert!(
            length_bar(&v, Note::new(q(1), Ticks(1), 36, 100), Ticks(240)).is_none(),
            "a hit with no length has no bar"
        );
    }
}
