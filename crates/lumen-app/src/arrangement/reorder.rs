//! Reordering tracks by dragging their headers (M3 slice 4d, the owner's
//! Ableton-style request): an accent line shows where they will land, the
//! list scrolls when the pointer nears its top or bottom edge, and the drop
//! is one undo step (`Document::move_tracks`). Esc cancels.

use eframe::egui::{self, CursorIcon, Key, Rect, Stroke, pos2};
use lumen_project::model::Id;

use super::{Arrangement, Row, RowKind};
use crate::document::Document;
use crate::theme;

/// Within this many points of the list's top or bottom, it scrolls.
const EDGE: f32 = 36.0;
/// The fastest auto-scroll, in points per second (at the very edge).
const SPEED: f32 = 900.0;

/// A drag of tracks in progress.
#[derive(Debug, Clone)]
pub(super) struct Drag {
    /// The tracks moving, in project order.
    ids: Vec<Id>,
    /// Where they would land: an index among the other tracks, and the y
    /// of the line (content points).
    drop: Option<(usize, f32)>,
}

impl Arrangement {
    /// A header was dragged: moves the selection (or just this track, if it
    /// is not selected).
    pub(super) fn start_drag(&mut self, project: &lumen_project::model::Project, id: Id) {
        if !self.selected.contains(&id) {
            self.selected = vec![id];
            self.anchor = Some(id);
        }
        let ids = project
            .tracks
            .iter()
            .filter(|t| self.selected.contains(&t.id))
            .map(|t| t.id)
            .collect();
        self.drag = Some(Drag { ids, drop: None });
    }

    /// Whether `id` is being dragged (its header is drawn dimmed).
    pub(super) fn is_dragged(&self, id: Id) -> bool {
        self.drag.as_ref().is_some_and(|d| d.ids.contains(&id))
    }

    /// Follows a drag each frame: the drop place, auto-scroll near the
    /// edges, and the drop itself on release. `column` is the header column;
    /// `content` the height of every row.
    pub(super) fn follow_drag(
        &mut self,
        ui: &egui::Ui,
        column: Rect,
        rows: &[Row],
        content: f32,
        doc: &mut Document,
    ) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        let ctx = ui.ctx();
        let (pointer, released, cancel, dt) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_released() || !i.pointer.primary_down(),
                i.key_pressed(Key::Escape),
                i.stable_dt.min(0.1),
            )
        });
        if cancel {
            self.drag = None;
            return;
        }
        ctx.set_cursor_icon(CursorIcon::Grabbing);
        if let Some(p) = pointer {
            // Scroll when near the top or bottom, faster the closer.
            let near_top = EDGE - (p.y - column.top());
            let near_bottom = EDGE - (column.bottom() - p.y);
            let push = if near_top > 0.0 {
                -near_top.min(EDGE)
            } else if near_bottom > 0.0 {
                near_bottom.min(EDGE)
            } else {
                0.0
            };
            if push != 0.0 {
                let max = (content - column.height() + super::FOLDED).max(0.0);
                self.top = (self.top + push / EDGE * SPEED * dt).clamp(0.0, max);
                ctx.request_repaint();
            }
            let places: Vec<(f32, f32, bool)> = rows
                .iter()
                .filter_map(|r| match r.kind {
                    RowKind::Track(i) => {
                        let id = doc.project().tracks[i].id;
                        Some((r.y, r.height, drag.ids.contains(&id)))
                    }
                    RowKind::Pending(_) => None,
                })
                .collect();
            drag.drop = Some(drop_place(&places, p.y - column.top() + self.top));
        }
        if released {
            if let Some((index, _)) = drag.drop
                && moves(doc, &drag.ids, index)
            {
                doc.move_tracks(&drag.ids, index);
            }
            self.drag = None;
        }
    }

    /// The drop line across `area` (the headers, then the lanes).
    pub(super) fn draw_drop_line(&self, painter: &egui::Painter, area: Rect) {
        let Some((_, y)) = self.drag.as_ref().and_then(|d| d.drop) else {
            return;
        };
        let y = area.top() + y - self.top;
        if (area.top()..=area.bottom()).contains(&y) {
            painter.line_segment(
                [pos2(area.left(), y), pos2(area.right(), y)],
                Stroke::new(2.0, theme::p().accent),
            );
        }
    }
}

/// Where tracks dragged to content y `y` land, given every track row's
/// `(top, height, moving)`: the index among the tracks that are not moving,
/// and the y of the line. A row's upper half drops above it, its lower half
/// below.
fn drop_place(rows: &[(f32, f32, bool)], y: f32) -> (usize, f32) {
    let mut index = 0;
    let mut line = 0.0;
    for &(top, height, moving) in rows {
        if moving {
            continue;
        }
        if y < top + height / 2.0 {
            return (index, top);
        }
        index += 1;
        line = top + height;
    }
    (index, line)
}

/// Whether moving `ids` to `index` changes the order at all.
fn moves(doc: &Document, ids: &[Id], index: usize) -> bool {
    let tracks = &doc.project().tracks;
    let rest: Vec<Id> = tracks
        .iter()
        .map(|t| t.id)
        .filter(|id| !ids.contains(id))
        .collect();
    let mut after = rest;
    let at = index.min(after.len());
    after.splice(at..at, ids.iter().copied());
    after.iter().zip(tracks.iter()).any(|(a, t)| *a != t.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Three 44-point rows; the middle one is being dragged.
    const ROWS: [(f32, f32, bool); 3] =
        [(0.0, 44.0, false), (44.0, 44.0, true), (88.0, 44.0, false)];

    #[test]
    fn the_drop_lands_by_row_halves_and_skips_the_dragged_rows() {
        assert_eq!(drop_place(&ROWS, 10.0), (0, 0.0), "upper half of the first");
        assert_eq!(drop_place(&ROWS, 30.0), (1, 88.0), "lower half: after it");
        // Over the dragged row itself: it does not count.
        assert_eq!(drop_place(&ROWS, 60.0), (1, 88.0));
        assert_eq!(drop_place(&ROWS, 120.0), (2, 132.0), "past the last");
        assert_eq!(drop_place(&ROWS, -50.0), (0, 0.0), "above the list");
    }

    #[test]
    fn a_drop_where_the_tracks_already_are_is_not_a_move() {
        let mut doc = Document::new(crate::empty_project());
        let a = doc.add_track(lumen_project::model::TrackKind::Audio, None);
        let b = doc.add_track(lumen_project::model::TrackKind::Audio, None);
        let c = doc.add_track(lumen_project::model::TrackKind::Audio, None);
        // b sits at index 1 among a and c: dropping it there changes nothing.
        assert!(!moves(&doc, &[b], 1));
        assert!(moves(&doc, &[b], 0));
        assert!(moves(&doc, &[a], 2));
        assert!(!moves(&doc, &[c], 2));
    }
}
