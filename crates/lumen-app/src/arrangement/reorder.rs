use eframe::egui::{self, CursorIcon, Key, Rect, Stroke, pos2};
use lumen_project::groups;
use lumen_project::model::{Id, TrackKind};

use super::{Arrangement, Row, RowKind, Zone};
use crate::document::{Document, Place, Refused};
use crate::theme;

const EDGE: f32 = theme::size::EDGE_SCROLL;
const SPEED: f32 = crate::motion::EDGE_SCROLL_SPEED;

#[derive(Debug, Clone)]
pub(super) struct Drag {
    ids: Vec<Id>,
    moving: Vec<Id>,
    drop: Option<DropAt>,
    refused: Option<Refused>,
    in_mixer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DropAt {
    before: Option<Id>,
    group: Option<Id>,
    pub(super) mark: Mark,
    level: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Mark {
    Line(f32),
    Onto(f32, f32),
}

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

    pub(super) fn strip_drag(&self) -> Option<(&[Id], Option<DropAt>, bool)> {
        self.drag
            .as_ref()
            .filter(|d| d.in_mixer)
            .map(|d| (d.moving.as_slice(), d.drop, d.refused.is_some()))
    }

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

    pub(super) fn is_dragged(&self, id: Id) -> bool {
        self.drag.as_ref().is_some_and(|d| d.moving.contains(&id))
    }

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
    }

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
                        Stroke::new(theme::stroke::EDGE, accent),
                    );
                }
            }
            Mark::Onto(top, height) => {
                let rect = Rect::from_min_size(
                    pos2(area.left(), area.top() + top - self.top),
                    egui::vec2(area.width(), height),
                );
                painter.rect_filled(rect, 0.0, accent.gamma_multiply(theme::alpha::WASH));
                painter.rect_stroke(
                    rect.shrink(1.0),
                    0.0,
                    Stroke::new(theme::stroke::EDGE, accent),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

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
        let folded = [row(1, 0.0, true, None), row(4, 44.0, false, None)];
        assert_eq!(drop_place(&folded, 40.0), line(Some(4), None, 44.0, 0));
        assert_eq!(
            drop_place(&folded, 20.0).group,
            Some(Id(1)),
            "into the folded group"
        );
    }
}
