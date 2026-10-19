//! Reordering tracks by dragging their headers (M3 slice 4d, the owner's
//! request): an accent line shows where they will land, the
//! list scrolls when the pointer nears its top or bottom edge, and the drop
//! is one undo step (`Document::move_tracks`). Esc cancels.
//!
//! With groups (M4 slice 3): a group moves with everything inside it. The
//! line steps in to the level the tracks will land at: between two tracks
//! they join the group of the track below, so under a group's last track
//! they come after the group; dropped on the middle of a group's header
//! (folded or not), they go in at its end, and the header lights. A drop
//! that would be refused (groups more than three deep) greys out, with a
//! no-entry cursor, and says why if let go there.
//!
//! Mixer strips drag the same way along x (M4 slice 5, the owner's B8),
//! returns included: tracks move among tracks, returns among returns (a
//! return's letter follows its place).

use eframe::egui::{self, CursorIcon, Key, Rect, Stroke, pos2};
use lumen_project::groups;
use lumen_project::model::{Id, TrackKind};

use super::{Arrangement, Row, RowKind, Zone};
use crate::document::{Document, Place, Refused};
use crate::theme;

/// Within this many points of the list's top or bottom, it scrolls.
const EDGE: f32 = 36.0;
/// The fastest auto-scroll, in points per second (at the very edge).
const SPEED: f32 = 900.0;

/// A drag of tracks in progress.
#[derive(Debug, Clone)]
pub(super) struct Drag {
    /// The tracks moving, in project order: the selection's outermost
    /// tracks (groups bring what is inside them).
    ids: Vec<Id>,
    /// Those and everything inside them, drawn dimmed.
    moving: Vec<Id>,
    /// Where they would land, and why the move would be refused there.
    drop: Option<DropAt>,
    refused: Option<Refused>,
    /// Started on a mixer strip: the strips follow it, not the headers.
    in_mixer: bool,
}

/// Where dragged tracks would land.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DropAt {
    /// Before this track (`None`: at the end of the list or of `group`).
    before: Option<Id>,
    /// Inside this group (`None`: at the top).
    group: Option<Id>,
    /// The line, or the group header lit instead.
    pub(super) mark: Mark,
    /// How many groups deep they land, for the line's indent.
    level: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Mark {
    /// A line at this y (content points).
    Line(f32),
    /// The row of the group they go into: its top and height.
    Onto(f32, f32),
}

/// A visible track's row (or strip, along x), as the drop sees it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Spot {
    pub(super) id: Id,
    pub(super) top: f32,
    pub(super) height: f32,
    pub(super) group: bool,
    pub(super) parent: Option<Id>,
    pub(super) depth: usize,
}

impl Arrangement {
    /// A header (or, `in_mixer`, a strip) was dragged: moves the selection
    /// (or just this track, if it is not selected). From a strip, only the
    /// selected tracks of the same kind move: tracks, or returns.
    pub(super) fn start_drag(
        &mut self,
        project: &lumen_project::model::Project,
        id: Id,
        in_mixer: bool,
    ) {
        if !self.selected.contains(&id) {
            self.selected = vec![id];
            self.anchor = Some(id);
        }
        let is_return = |id: Id| {
            project
                .tracks
                .iter()
                .any(|t| t.id == id && t.kind == TrackKind::Return)
        };
        let zone = is_return(id);
        let chosen: Vec<Id> = self
            .selected
            .iter()
            .copied()
            .filter(|&s| !in_mixer || is_return(s) == zone)
            .collect();
        let ids = groups::outermost(&project.tracks, &chosen);
        let moving = groups::with_contents(&project.tracks, &ids);
        self.drag = Some(Drag {
            ids,
            moving,
            drop: None,
            refused: None,
            in_mixer,
        });
    }

    /// The drag in progress when it started on a mixer strip: what moves,
    /// where it would land, and whether that would be refused.
    pub(super) fn strip_drag(&self) -> Option<(&[Id], Option<DropAt>, bool)> {
        self.drag
            .as_ref()
            .filter(|d| d.in_mixer)
            .map(|d| (d.moving.as_slice(), d.drop, d.refused.is_some()))
    }

    /// Follows a strip drag each frame along x, as `follow_drag` does along
    /// y: `spots` are the strips of the tracks that stay, in screen x.
    /// Returns how hard to scroll the strips (-1 to 1) near the edges of
    /// `strips`.
    pub(super) fn follow_strip_drag(
        &mut self,
        ui: &egui::Ui,
        spots: &[Spot],
        strips: Rect,
        doc: &mut Document,
    ) -> f32 {
        let Some(drag) = self.drag.as_mut().filter(|d| d.in_mixer) else {
            return 0.0;
        };
        let ctx = ui.ctx();
        let (pointer, released, cancel) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_released() || !i.pointer.primary_down(),
                i.key_pressed(Key::Escape),
            )
        });
        if cancel {
            self.drag = None;
            return 0.0;
        }
        ctx.set_cursor_icon(if drag.refused.is_some() {
            CursorIcon::NotAllowed
        } else {
            CursorIcon::Grabbing
        });
        let mut push = 0.0;
        if let Some(p) = pointer {
            let near_left = EDGE - (p.x - strips.left());
            let near_right = EDGE - (strips.right() - p.x);
            push = if near_left > 0.0 {
                -(near_left.min(EDGE) / EDGE)
            } else if near_right > 0.0 {
                near_right.min(EDGE) / EDGE
            } else {
                0.0
            };
            let at = drop_place(spots, p.x);
            if drag.drop != Some(at) {
                drag.refused = doc
                    .check_move(&drag.ids, place(doc, &drag.moving, at))
                    .err();
                drag.drop = Some(at);
            }
        }
        if released {
            if let Some(at) = drag.drop {
                let to = place(doc, &drag.moving, at);
                if let Err(refused) = doc.move_tracks(&drag.ids, to) {
                    self.note = Some(refused.reason());
                }
            }
            self.drag = None;
        }
        push
    }

    /// Whether `id` is being dragged (its header is drawn dimmed).
    pub(super) fn is_dragged(&self, id: Id) -> bool {
        self.drag.as_ref().is_some_and(|d| d.moving.contains(&id))
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
        let Some(drag) = self.drag.as_mut().filter(|d| !d.in_mixer) else {
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
        ctx.set_cursor_icon(if drag.refused.is_some() {
            CursorIcon::NotAllowed
        } else {
            CursorIcon::Grabbing
        });
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
            let tracks = &doc.project().tracks;
            let spots: Vec<Spot> = rows
                .iter()
                .filter(|r| r.zone == Zone::Main)
                .filter_map(|r| match r.kind {
                    RowKind::Track(i) if !drag.moving.contains(&tracks[i].id) => {
                        let t = &tracks[i];
                        Some(Spot {
                            id: t.id,
                            top: r.y,
                            height: r.height,
                            group: t.kind == TrackKind::Group,
                            parent: t.parent,
                            depth: groups::depth(tracks, t.id),
                        })
                    }
                    _ => None,
                })
                .collect();
            let at = drop_place(&spots, p.y - column.top() + self.top);
            // Checked when the place changes, not every frame.
            if drag.drop != Some(at) {
                drag.refused = doc
                    .check_move(&drag.ids, place(doc, &drag.moving, at))
                    .err();
                drag.drop = Some(at);
            }
        }
        if released {
            if let Some(at) = drag.drop {
                // A drop where the tracks already are records nothing; a
                // refused one changes nothing and says why.
                let to = place(doc, &drag.moving, at);
                if let Err(refused) = doc.move_tracks(&drag.ids, to) {
                    self.note = Some(refused.reason());
                }
            }
            self.drag = None;
        }
    }

    /// The drop line across `area` (the headers, then the lanes), stepped
    /// in to its level when `indented` (the headers); or the group the
    /// tracks go into, lit.
    pub(super) fn draw_drop_line(&self, painter: &egui::Painter, area: Rect, indented: bool) {
        let Some(at) = self
            .drag
            .as_ref()
            .filter(|d| !d.in_mixer)
            .and_then(|d| d.drop)
        else {
            return;
        };
        let refused = self.drag.as_ref().is_some_and(|d| d.refused.is_some());
        let accent = if refused {
            theme::p().text_disabled
        } else {
            theme::p().accent
        };
        match at.mark {
            Mark::Line(y) => {
                let y = area.top() + y - self.top;
                let left = area.left()
                    + if indented {
                        super::indent(at.level)
                    } else {
                        0.0
                    };
                if (area.top()..=area.bottom()).contains(&y) {
                    painter.line_segment(
                        [pos2(left, y), pos2(area.right(), y)],
                        Stroke::new(2.0, accent),
                    );
                }
            }
            Mark::Onto(top, height) => {
                let rect = Rect::from_min_size(
                    pos2(area.left(), area.top() + top - self.top),
                    egui::vec2(area.width(), height),
                );
                painter.rect_filled(rect, 0.0, accent.gamma_multiply(0.10));
                painter.rect_stroke(
                    rect.shrink(1.0),
                    0.0,
                    Stroke::new(2.0, accent),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

/// The move a drop at `at` makes: an index among the tracks that stay.
fn place(doc: &Document, moving: &[Id], at: DropAt) -> Place {
    let tracks = &doc.project().tracks;
    let rest = tracks.iter().filter(|t| !moving.contains(&t.id));
    let index = match at.before {
        Some(before) => rest.take_while(|t| t.id != before).count(),
        None => rest.count(),
    };
    Place {
        index,
        group: at.group,
    }
}

/// Where tracks dragged to content y `y` land, given every visible row of
/// the tracks that are not moving, from the top. A track's upper half drops
/// above it and its lower half below; a group's middle half drops into it.
/// Between two tracks, the moved ones join the group of the track below.
/// Strips use it along x.
pub(super) fn drop_place(rows: &[Spot], y: f32) -> DropAt {
    let between = |line: f32, below: Option<&Spot>| DropAt {
        before: below.map(|r| r.id),
        group: below.and_then(|r| r.parent),
        mark: Mark::Line(line),
        level: below.map_or(0, |r| r.depth),
    };
    for (i, row) in rows.iter().enumerate() {
        let (above, below) = if row.group {
            (row.height / 4.0, row.height * 3.0 / 4.0)
        } else {
            (row.height / 2.0, row.height / 2.0)
        };
        if y < row.top + above {
            return between(row.top, Some(row));
        }
        if y < row.top + below {
            return DropAt {
                before: None,
                group: Some(row.id),
                mark: Mark::Onto(row.top, row.height),
                level: row.depth + 1,
            };
        }
        if i + 1 == rows.len() || y < rows[i + 1].top {
            return between(row.top + row.height, rows.get(i + 1));
        }
    }
    between(0.0, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, top: f32, group: bool, parent: Option<u64>) -> Spot {
        Spot {
            id: Id(id),
            top,
            height: 44.0,
            group,
            parent: parent.map(Id),
            depth: usize::from(parent.is_some()),
        }
    }

    fn line(before: Option<u64>, group: Option<u64>, y: f32, level: usize) -> DropAt {
        DropAt {
            before: before.map(Id),
            group: group.map(Id),
            mark: Mark::Line(y),
            level,
        }
    }

    #[test]
    fn the_drop_lands_by_row_halves() {
        // Two 44-point rows (a dragged one between them is left out).
        let rows = [row(1, 0.0, false, None), row(3, 88.0, false, None)];
        assert_eq!(
            drop_place(&rows, 10.0),
            line(Some(1), None, 0.0, 0),
            "upper half"
        );
        assert_eq!(
            drop_place(&rows, 30.0),
            line(Some(3), None, 44.0, 0),
            "lower half"
        );
        assert_eq!(
            drop_place(&rows, 60.0),
            line(Some(3), None, 44.0, 0),
            "the gap"
        );
        assert_eq!(
            drop_place(&rows, 120.0),
            line(None, None, 132.0, 0),
            "past the last"
        );
        assert_eq!(
            drop_place(&rows, -50.0),
            line(Some(1), None, 0.0, 0),
            "above"
        );
        assert_eq!(drop_place(&[], 10.0), line(None, None, 0.0, 0));
    }

    #[test]
    fn the_drop_enters_and_leaves_groups() {
        // Drums (a group) holding Kick and Snare, then Bass.
        let rows = [
            row(1, 0.0, true, None),
            row(2, 44.0, false, Some(1)),
            row(3, 88.0, false, Some(1)),
            row(4, 132.0, false, None),
        ];
        assert_eq!(
            drop_place(&rows, 5.0),
            line(Some(1), None, 0.0, 0),
            "above the group"
        );
        let onto = drop_place(&rows, 22.0);
        assert_eq!(
            (onto.before, onto.group),
            (None, Some(Id(1))),
            "onto its header"
        );
        assert_eq!(onto.mark, Mark::Onto(0.0, 44.0));
        assert_eq!(
            drop_place(&rows, 40.0),
            line(Some(2), Some(1), 44.0, 1),
            "first inside"
        );
        assert_eq!(
            drop_place(&rows, 70.0),
            line(Some(3), Some(1), 88.0, 1),
            "between"
        );
        assert_eq!(
            drop_place(&rows, 120.0),
            line(Some(4), None, 132.0, 0),
            "under the last track inside: after the group"
        );
        // A folded group: its tracks hidden, the next row is after it.
        let folded = [row(1, 0.0, true, None), row(4, 44.0, false, None)];
        assert_eq!(drop_place(&folded, 40.0), line(Some(4), None, 44.0, 0));
        assert_eq!(
            drop_place(&folded, 20.0).group,
            Some(Id(1)),
            "into the folded group"
        );
    }
}
