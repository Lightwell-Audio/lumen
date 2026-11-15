use eframe::egui::{self, Painter, Rect, Stroke, pos2, vec2};
use lumen_project::model::{Id, Project, TrackKind};

use super::{Arrangement, RowKind, Zone};
use crate::document::DropInto;
use crate::theme;
use crate::widgets;

const BETWEEN: f32 = 7.0;
const GHOST_WIDTH: f32 = 140.0;

pub struct FileDrop<'a> {
    pub into: DropInto,
    pub line: f32,
    pub x: f32,
    pub label: &'a str,
}

#[derive(Debug, Clone, Copy)]
struct DropRow {
    id: Id,
    top: f32,
    height: f32,
    kind: TrackKind,
    parent: Option<Id>,
    depth: usize,
}

fn drop_place(rows: &[DropRow], y: f32, lands_on: TrackKind) -> (DropInto, f32) {
    for (i, row) in rows.iter().enumerate() {
        let bottom = row.top + row.height;
        let next = rows.get(i + 1);
        let between = |below: Option<&DropRow>| DropInto::NewTracks {
            before: below.map(|r| r.id),
            group: below.and_then(|r| r.parent),
        };
        if y < row.top + BETWEEN {
            return (between(Some(row)), row.top);
        }
        if y < bottom - BETWEEN {
            return match row.kind {
                kind if kind == lands_on => (DropInto::Track(row.id), row.top),
                TrackKind::Group => {
                    let inside = rows[i + 1..]
                        .iter()
                        .take_while(|r| r.depth > row.depth)
                        .last()
                        .unwrap_or(row);
                    let after = rows[i + 1..].iter().find(|r| r.depth <= row.depth);
                    (
                        DropInto::NewTracks {
                            before: after.map(|r| r.id),
                            group: Some(row.id),
                        },
                        inside.top + inside.height,
                    )
                }
                _ => (
                    DropInto::NewTracks {
                        before: next.map(|r| r.id),
                        group: row.parent,
                    },
                    bottom,
                ),
            };
        }
        if y < bottom {
            return (between(next), bottom);
        }
    }
    let end = rows.last().map_or(0.0, |r| r.top + r.height);
    (
        DropInto::NewTracks {
            before: None,
            group: None,
        },
        end,
    )
}

impl Arrangement {
    pub fn file_drop(
        &self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        canvas: Rect,
        y: f32,
        lands_on: TrackKind,
    ) -> (DropInto, f32) {
        let rows: Vec<DropRow> = self
            .rows(ctx, project, pending)
            .iter()
            .filter(|r| r.zone == Zone::Main)
            .filter_map(|r| match r.kind {
                RowKind::Track(i) => {
                    let t = &project.tracks[i];
                    Some(DropRow {
                        id: t.id,
                        top: r.y,
                        height: r.height,
                        kind: t.kind,
                        parent: t.parent,
                        depth: lumen_project::groups::depth(&project.tracks, t.id),
                    })
                }
                RowKind::Pending(_) | RowKind::Master => None,
            })
            .collect();
        let (into, line) = drop_place(&rows, y - canvas.top() + self.top, lands_on);
        (into, canvas.top() + line - self.top)
    }

    pub(crate) fn lane_of(
        &self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        canvas: Rect,
        id: Id,
    ) -> Option<Rect> {
        self.rows(ctx, project, pending)
            .iter()
            .find_map(|r| match r.kind {
                RowKind::Track(i) if project.tracks[i].id == id => Some(Rect::from_min_size(
                    pos2(canvas.left(), canvas.top() + r.y - self.top),
                    vec2(canvas.width(), r.height),
                )),
                _ => None,
            })
    }

    pub fn draw_file_drop(
        &self,
        ctx: &egui::Context,
        painter: &Painter,
        project: &Project,
        pending: usize,
        canvas: Rect,
        drop: &FileDrop<'_>,
    ) {
        let FileDrop {
            into,
            line,
            x,
            label,
        } = *drop;
        let palette = theme::p();
        let painter = painter.with_clip_rect(canvas);
        match into {
            DropInto::Track(id) => {
                let Some(lane) = self.lane_of(ctx, project, pending, canvas, id) else {
                    return;
                };
                painter.rect_filled(
                    lane,
                    0.0,
                    palette.accent.gamma_multiply(theme::alpha::WASH_DROP),
                );
                painter.rect_stroke(
                    lane,
                    0.0,
                    Stroke::new(
                        theme::stroke::HAIRLINE,
                        palette.accent.gamma_multiply(theme::alpha::EDGE),
                    ),
                    egui::StrokeKind::Inside,
                );
                let ghost = Rect::from_min_size(
                    pos2(x, lane.top() + super::clips::INSET),
                    vec2(GHOST_WIDTH, lane.height() - 2.0 * super::clips::INSET),
                );
                painter.rect_filled(
                    ghost,
                    theme::radius::SMALL,
                    palette.accent.gamma_multiply(theme::alpha::WASH_BAND),
                );
                painter.add(egui::Shape::dashed_line(
                    &[
                        ghost.left_top(),
                        ghost.right_top(),
                        ghost.right_bottom(),
                        ghost.left_bottom(),
                        ghost.left_top(),
                    ],
                    Stroke::new(theme::stroke::HAIRLINE, palette.accent),
                    4.0,
                    3.0,
                ));
                painter.with_clip_rect(ghost.shrink(2.0)).text(
                    pos2(ghost.left() + 5.0, ghost.top() + super::TITLE / 2.0 + 1.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(theme::size::CAPTION),
                    palette.text,
                );
            }
            DropInto::NewTracks { group, .. } => {
                painter.hline(
                    canvas.x_range(),
                    line,
                    Stroke::new(
                        theme::stroke::GLOW_WIDE,
                        palette.accent.gamma_multiply(theme::alpha::GLOW),
                    ),
                );
                painter.hline(
                    canvas.x_range(),
                    line,
                    Stroke::new(theme::stroke::EDGE, palette.accent),
                );
                let hint = group
                    .and_then(|g| project.tracks.iter().find(|t| t.id == g))
                    .map_or_else(
                        || "new track here".to_owned(),
                        |g| format!("new track in {}", g.name),
                    );
                widgets::draw_hint(&painter, pos2(x + 14.0, line), &hint, canvas);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, top: f32, kind: TrackKind, parent: Option<u64>) -> DropRow {
        DropRow {
            id: Id(id),
            top,
            height: 44.0,
            kind,
            parent: parent.map(Id),
            depth: usize::from(parent.is_some()),
        }
    }

    fn rows() -> [DropRow; 3] {
        [
            row(1, 0.0, TrackKind::Audio, None),
            row(2, 44.0, TrackKind::Midi, None),
            row(3, 88.0, TrackKind::Audio, None),
        ]
    }

    fn new(before: Option<u64>, group: Option<u64>) -> DropInto {
        DropInto::NewTracks {
            before: before.map(Id),
            group: group.map(Id),
        }
    }

    #[test]
    fn files_land_on_audio_lanes_and_make_tracks_between_them() {
        let rows = rows();
        assert_eq!(
            drop_place(&rows, 20.0, TrackKind::Audio),
            (DropInto::Track(Id(1)), 0.0)
        );
        assert_eq!(
            drop_place(&rows, 110.0, TrackKind::Audio),
            (DropInto::Track(Id(3)), 88.0)
        );
        assert_eq!(
            drop_place(&rows, 40.0, TrackKind::Audio),
            (new(Some(2), None), 44.0)
        );
        assert_eq!(
            drop_place(&rows, 47.0, TrackKind::Audio),
            (new(Some(2), None), 44.0)
        );
        assert_eq!(
            drop_place(&rows, 2.0, TrackKind::Audio),
            (new(Some(1), None), 0.0),
            "above the first"
        );
        assert_eq!(
            drop_place(&rows, 66.0, TrackKind::Audio),
            (new(Some(3), None), 88.0)
        );
        assert_eq!(
            drop_place(&rows, 128.0, TrackKind::Audio),
            (new(None, None), 132.0)
        );
        assert_eq!(
            drop_place(&rows, 400.0, TrackKind::Audio),
            (new(None, None), 132.0)
        );
        assert_eq!(
            drop_place(&[], 10.0, TrackKind::Audio),
            (new(None, None), 0.0)
        );
        assert_eq!(
            drop_place(&rows, 66.0, TrackKind::Midi),
            (DropInto::Track(Id(2)), 44.0)
        );
        assert_eq!(
            drop_place(&rows, 20.0, TrackKind::Midi),
            (new(Some(2), None), 44.0)
        );
    }

    #[test]
    fn files_go_into_groups_by_where_they_are_dropped() {
        let rows = [
            row(1, 0.0, TrackKind::Group, None),
            row(2, 44.0, TrackKind::Midi, Some(1)),
            row(3, 88.0, TrackKind::Audio, Some(1)),
            row(4, 132.0, TrackKind::Audio, None),
        ];
        assert_eq!(
            drop_place(&rows, 20.0, TrackKind::Audio),
            (new(Some(4), Some(1)), 132.0)
        );
        assert_eq!(
            drop_place(&rows, 42.0, TrackKind::Audio),
            (new(Some(2), Some(1)), 44.0)
        );
        assert_eq!(
            drop_place(&rows, 66.0, TrackKind::Audio),
            (new(Some(3), Some(1)), 88.0)
        );
        assert_eq!(
            drop_place(&rows, 130.0, TrackKind::Audio),
            (new(Some(4), None), 132.0)
        );
    }
}
