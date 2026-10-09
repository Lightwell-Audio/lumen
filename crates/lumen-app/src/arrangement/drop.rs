//! Files dragged from the system onto the lanes (M3 slice 6e): over an
//! audio track's lane they land on it; near the line between two tracks,
//! below the last, or over a track of another kind, they make new tracks
//! there. While they are dragged, the lane lights with a ghost clip where
//! they will land, or a line shows where the new tracks will go.

use eframe::egui::{self, Painter, Rect, Stroke, pos2, vec2};
use lumen_project::model::{Id, Project, TrackKind};

use super::{Arrangement, RowKind};
use crate::document::DropInto;
use crate::theme;
use crate::widgets;

/// Within this many points of the line between two tracks, files make new
/// tracks there rather than landing on a track.
const BETWEEN: f32 = 7.0;
/// The ghost clip's width: the files' lengths are not known yet.
const GHOST_WIDTH: f32 = 140.0;

/// Files being dragged over the lanes: where they go, the screen y of the
/// line where new tracks would go, the x where they land, and their name
/// ("kick.wav", "3 files").
pub struct FileDrop<'a> {
    pub into: DropInto,
    pub line: f32,
    pub x: f32,
    pub label: &'a str,
}

/// Where files dragged to content y `y` go, given every track row's
/// `(id, top, height, audio)` from the top: the destination, and the y of
/// the line where new tracks would go.
fn drop_place(rows: &[(Id, f32, f32, bool)], y: f32) -> (DropInto, f32) {
    for (i, &(id, top, height, audio)) in rows.iter().enumerate() {
        let bottom = top + height;
        let next = rows.get(i + 1).map(|r| r.0);
        if y < top + BETWEEN {
            return (DropInto::NewTracks { before: Some(id) }, top);
        }
        if y < bottom - BETWEEN && audio {
            return (DropInto::Track(id), top);
        }
        if y < bottom {
            return (DropInto::NewTracks { before: next }, bottom);
        }
    }
    let end = rows.last().map_or(0.0, |r| r.1 + r.2);
    (DropInto::NewTracks { before: None }, end)
}

impl Arrangement {
    /// Where files dragged to screen y `y` over the lanes in `canvas` go,
    /// and the screen y of the line where new tracks would go.
    pub fn file_drop(
        &self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        canvas: Rect,
        y: f32,
    ) -> (DropInto, f32) {
        let rows: Vec<(Id, f32, f32, bool)> = self
            .rows(ctx, project, pending)
            .iter()
            .filter_map(|r| match r.kind {
                RowKind::Track(i) => {
                    let t = &project.tracks[i];
                    Some((t.id, r.y, r.height, t.kind == TrackKind::Audio))
                }
                RowKind::Pending(_) => None,
            })
            .collect();
        let (into, line) = drop_place(&rows, y - canvas.top() + self.top);
        (into, canvas.top() + line - self.top)
    }

    /// The lane of track `id` on screen.
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

    /// What files dragged over the lanes will do: onto a track, the lane
    /// lights and a ghost clip with their name sits where they land; as new
    /// tracks, an accent line where the tracks go, with a chip.
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
                painter.rect_filled(lane, 0.0, palette.accent.gamma_multiply(0.08));
                painter.rect_stroke(
                    lane,
                    0.0,
                    Stroke::new(1.0, palette.accent.gamma_multiply(0.5)),
                    egui::StrokeKind::Inside,
                );
                let ghost = Rect::from_min_size(
                    pos2(x, lane.top() + super::clips::INSET),
                    vec2(GHOST_WIDTH, lane.height() - 2.0 * super::clips::INSET),
                );
                painter.rect_filled(
                    ghost,
                    theme::radius::SMALL,
                    palette.accent.gamma_multiply(0.14),
                );
                painter.add(egui::Shape::dashed_line(
                    &[
                        ghost.left_top(),
                        ghost.right_top(),
                        ghost.right_bottom(),
                        ghost.left_bottom(),
                        ghost.left_top(),
                    ],
                    Stroke::new(1.0, palette.accent),
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
            DropInto::NewTracks { .. } => {
                painter.hline(
                    canvas.x_range(),
                    line,
                    Stroke::new(6.0, palette.accent.gamma_multiply(0.15)),
                );
                painter.hline(canvas.x_range(), line, Stroke::new(2.0, palette.accent));
                widgets::draw_hint(&painter, pos2(x + 14.0, line), "new track here", canvas);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // An audio track, a MIDI track and another audio track, 44 points each.
    const ROWS: [(Id, f32, f32, bool); 3] = [
        (Id(1), 0.0, 44.0, true),
        (Id(2), 44.0, 44.0, false),
        (Id(3), 88.0, 44.0, true),
    ];

    #[test]
    fn files_land_on_audio_lanes_and_make_tracks_between_them() {
        let new = |before| DropInto::NewTracks { before };
        assert_eq!(drop_place(&ROWS, 20.0), (DropInto::Track(Id(1)), 0.0));
        assert_eq!(drop_place(&ROWS, 110.0), (DropInto::Track(Id(3)), 88.0));
        // Near a line between tracks: new tracks there.
        assert_eq!(drop_place(&ROWS, 40.0), (new(Some(Id(2))), 44.0));
        assert_eq!(drop_place(&ROWS, 47.0), (new(Some(Id(2))), 44.0));
        assert_eq!(
            drop_place(&ROWS, 2.0),
            (new(Some(Id(1))), 0.0),
            "above the first"
        );
        // Over a MIDI track: a new track below it.
        assert_eq!(drop_place(&ROWS, 66.0), (new(Some(Id(3))), 88.0));
        // Below the last, or with no tracks: at the end.
        assert_eq!(drop_place(&ROWS, 128.0), (new(None), 132.0));
        assert_eq!(drop_place(&ROWS, 400.0), (new(None), 132.0));
        assert_eq!(drop_place(&[], 10.0), (new(None), 0.0));
    }
}
