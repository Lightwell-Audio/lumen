use eframe::egui::{Pos2, Rect, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use crate::note_editor::view::EditorView;
use crate::theme;

pub(crate) const ROW: f32 = theme::size::LANE_ROW;

pub(crate) const NAME: f32 = theme::size::LANE_NAME;

pub(crate) const BAR: f32 = theme::size::LANE_BAR;

pub(crate) const ICON: f32 = theme::size::LANE_ICON;
pub(crate) const ICON_X: f32 = theme::size::LANE_ICON_X;

pub(crate) const NAME_X: f32 = theme::size::LANE_NAME_X;

pub(crate) const BUTTON_W: f32 = theme::size::LANE_BUTTON_W;
pub(crate) const BUTTON_H: f32 = theme::size::LANE_BUTTON_H;
const BUTTON_GAP: f32 = theme::size::LANE_BUTTON_GAP;
const BUTTONS_RIGHT: f32 = theme::size::LANE_BUTTONS_RIGHT;
const BUTTONS: f32 = 2.0 * BUTTON_W + BUTTON_GAP;

const HIT_ROW: f32 = theme::size::LANE_HIT_ROW;

const HIT_STEP: f32 = theme::size::LANE_HIT_STEP;

const HIT_LEAST: f32 = 1.5;

const BUCKET: f32 = 2.0;

pub(crate) const MAX_CELLS: usize = 2400;

pub(crate) const LENGTH_LINE: f32 = 2.0;

pub(crate) fn hit_radius(view: &EditorView, step: Ticks) -> f32 {
    let step_points = (step.as_quarters() * view.time.zoom) as f32;
    (view.row_height * HIT_ROW)
        .min(step_points * HIT_STEP)
        .max(HIT_LEAST)
}

pub(crate) fn hit_rect(view: &EditorView, note: Note, step: Ticks) -> Rect {
    let r = hit_radius(view, step);
    let top = view.row_top(i32::from(note.pitch()));
    let onset = view.x_of(note.start());
    Rect::from_min_max(
        pos2(onset - r, top + view.row_height / 2.0 - r),
        pos2(onset + r, top + view.row_height / 2.0 + r),
    )
}

pub(crate) fn hit_centre(rect: Rect) -> Pos2 {
    pos2(rect.left() + rect.width() / 2.0, rect.center().y)
}

pub(crate) fn inside_diamond(rect: Rect, p: Pos2) -> bool {
    let centre = hit_centre(rect);
    let r = rect.width() / 2.0 + 1.0;
    (p.x - centre.x).abs() + (p.y - centre.y).abs() <= r
}

pub(crate) fn hit_at(view: &EditorView, notes: &[Note], step: Ticks, p: Pos2) -> Option<usize> {
    if !view.body.contains(p) {
        return None;
    }
    notes.iter().enumerate().rev().find_map(|(i, note)| {
        let rect = hit_rect(view, *note, step);
        (rect.intersects(view.body) && inside_diamond(rect, p)).then_some(i)
    })
}

pub(crate) fn row_at(view: &EditorView, y: f32) -> Option<usize> {
    view.lane_at(y)
}

pub(crate) fn row_rect(view: &EditorView, row: usize) -> Rect {
    let top = view.body.top() + row as f32 * view.row_height - view.scroll;
    Rect::from_min_max(
        pos2(view.body.left(), top),
        pos2(view.body.right(), top + view.row_height),
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cell {
    pub row: usize,

    pub x: f32,
    pub velocity: u8,

    pub picked: bool,

    pub muted: bool,

    pub note: usize,
}

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

        if x < view.body.left() - view.row_height || x > view.body.right() + view.row_height {
            continue;
        }

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
        let centre = pos2(cell.x, top + view.row_height / 2.0);
        let d = Rect::from_center_size(centre, vec2(2.0 * r, 2.0 * r));
        inside_diamond(d, p).then_some(cell.note)
    })
}

pub(crate) fn length_bar(view: &EditorView, note: Note, step: Ticks) -> Option<(f32, f32)> {
    let r = hit_radius(view, step);
    let x = view.x_of(note.start());
    let end = view.x_of(note.end());
    (end > x + r).then_some((x + r, end))
}

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
    fn a_hit_is_a_diamond_centred_on_the_onset() {
        let rows = Rows::lanes([36_u8, 38]);
        let step = Ticks(PPQ / 4);
        let mut v = view(rows);

        v.time.zoom = 400.0;
        let note = Note::new(q(1), q(0), 36, 100);
        let rect = hit_rect(&v, note, step);
        let r = rect.width() / 2.0;
        assert_eq!(rect.left(), v.x_of(q(1)) - r);
        assert!(
            (r - (v.row_height * HIT_ROW)).abs() < 1e-4,
            "a third of the row: {r}"
        );
        assert_eq!(rect.center().y, v.body.top() + ROW / 2.0);

        let c = hit_centre(rect);
        assert!(inside_diamond(rect, c));
        assert!(inside_diamond(rect, pos2(c.x - r, c.y)));
        assert!(!inside_diamond(
            rect,
            pos2(rect.left() + 0.5, rect.top() + 0.5)
        ));

        assert_eq!(hit_at(&v, &[note], step, c), Some(0));
        assert_eq!(hit_at(&v, &[note], step, c + vec2(0.0, ROW)), None);

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

        assert!(all.iter().all(|c| c.velocity != 120 || c.row != 1));

        let centre = pos2(all[0].x, v.body.top() + ROW / 2.0);
        assert_eq!(
            hit_at_cells(&v, &all, Ticks::quarters(1), centre),
            Some(all[0].note)
        );

        let capacity = seen.capacity();
        cells_into(&v, &notes, &[1], &[], &mut seen, &mut all);
        assert_eq!(seen.capacity(), capacity.max(seen.capacity()));
        assert_eq!(all.len(), 2);

        let mut scrolled = view(Rows::lanes([36_u8, 38, 42]));
        scrolled.scroll = 3.0 * ROW;
        cells_into(&scrolled, &notes, &[], &[], &mut seen, &mut all);
        assert!(all.is_empty());

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
        let name = Rect::from_min_max(pos2(0.0, 0.0), pos2(NAME, ROW));
        let (mute, solo) = buttons(name);
        assert_eq!(mute.left(), NAME - 52.0);
        assert_eq!(solo.right(), NAME - 8.0);
        assert_eq!((mute.width(), mute.height()), (20.0, 18.0));
        assert_eq!(icon_rect(name).left(), 10.0);
        assert_eq!(icon_rect(name).width(), 16.0);
    }

    #[test]
    fn a_hit_is_centred_on_its_onset_so_the_grid_line_runs_through_it() {
        let v = view(Rows::lanes([36_u8]));
        let note = Note::new(q(1), q(1), 36, 100);
        let rect = hit_rect(&v, note, Ticks(240));
        assert_eq!(hit_centre(rect).x, v.x_of(q(1)));
        assert!(rect.left() < v.x_of(q(1)) && rect.right() > v.x_of(q(1)));

        let on = pos2(v.x_of(q(1)), rect.center().y);
        assert_eq!(hit_at(&v, &[note], Ticks(240), on), Some(0));
        let left = pos2(v.x_of(q(1)) - rect.width() / 4.0, rect.center().y);
        assert_eq!(hit_at(&v, &[note], Ticks(240), left), Some(0));
    }

    #[test]
    fn a_length_bar_runs_from_the_hit_to_the_notes_end() {
        let v = view(Rows::lanes([36_u8]));
        let note = Note::new(q(1), q(2), 36, 100);
        let (from, to) = length_bar(&v, note, Ticks(240)).expect("a bar");
        let rect = hit_rect(&v, note, Ticks(240));
        assert_eq!(from, rect.right(), "from the right tip");
        assert_eq!(to, v.x_of(q(3)));
        assert!(
            length_bar(&v, Note::new(q(1), Ticks(1), 36, 100), Ticks(240)).is_none(),
            "a hit with no length has no bar"
        );
    }
}
