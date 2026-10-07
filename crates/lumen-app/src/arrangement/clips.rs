//! The lanes and their clips (M3 slice 4a): dark clips with a
//! track-coloured title bar, the armed and soloed lane hints, dimmed clips
//! on tracks that cannot be heard, and the hover lookup for the file card.

use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use lumen_project::model::{ClipContent, Project};

use super::{Arrangement, RowKind, TITLE, color_of};
use crate::document::audible;
use crate::lanes::Lanes;
use crate::media::{Item, State};
use crate::theme;
use crate::widgets::{draw_edge_glow, gradient};

impl Arrangement {
    /// The lanes and clips in `canvas`; `x_of` maps quarter notes to x.
    pub fn lanes(
        &self,
        ctx: &egui::Context,
        painter: &Painter,
        canvas: Rect,
        project: &Project,
        lanes: &mut Lanes,
        x_of: impl Fn(f32) -> f32,
    ) {
        let palette = *theme::p();
        let audible = audible(project);
        let rows = self.rows(ctx, project, lanes.pending().len());
        for row in rows {
            let lane = Rect::from_min_size(
                pos2(canvas.left(), canvas.top() + row.y - self.top),
                vec2(canvas.width(), row.height),
            );
            if lane.bottom() < canvas.top() || lane.top() > canvas.bottom() {
                continue;
            }
            match row.kind {
                RowKind::Track(index) => {
                    let track = &project.tracks[index];
                    if track.armed {
                        // A red wash, strongest at the left, with thin
                        // red edges (owner's choice: "glow and breathe").
                        let fade = Rect::from_min_size(
                            lane.min,
                            vec2((lane.width() * 0.18).min(220.0), lane.height()),
                        );
                        painter.rect_filled(lane, 0.0, palette.record.gamma_multiply(0.05));
                        gradient(
                            painter,
                            fade,
                            palette.record.gamma_multiply(0.12),
                            Color32::TRANSPARENT,
                        );
                        let edge = Stroke::new(1.0, palette.record.gamma_multiply(0.45));
                        painter.hline(canvas.x_range(), lane.top() + 0.5, edge);
                        painter.hline(canvas.x_range(), lane.bottom() - 1.5, edge);
                    } else if track.solo {
                        painter.rect_filled(lane, 0.0, palette.accent.gamma_multiply(0.06));
                    }
                    let heard = audible(track);
                    for clip in &track.clips {
                        let x0 = x_of(clip.start.as_quarters() as f32);
                        let x1 = x_of((clip.start + clip.length).as_quarters() as f32);
                        if x1 < canvas.left() || x0 > canvas.right() {
                            continue;
                        }
                        let rect = Rect::from_x_y_ranges(
                            x0..=x1.max(x0 + 2.0),
                            lane.top() + 3.0..=lane.bottom() - 3.0,
                        );
                        let (item, key) = match &clip.content {
                            ClipContent::Audio { media, .. } => {
                                (lanes.library.get(media.0).map(item_state), media.0)
                            }
                            ClipContent::Midi { .. } => (Some(ClipState::Ready), clip.id.0),
                        };
                        let shown = lanes.shown(key, ctx);
                        let progress = match &clip.content {
                            ClipContent::Audio { media, .. } => lanes.library.progress(media.0),
                            ClipContent::Midi { .. } => None,
                        };
                        draw_clip(
                            painter,
                            rect,
                            lane,
                            &ClipLook {
                                name: &clip.name,
                                color: color_of(track.color),
                                state: item.unwrap_or(ClipState::Missing),
                                progress,
                                alpha: if heard { shown } else { shown * 0.42 },
                            },
                        );
                    }
                }
                RowKind::Pending(i) => {
                    let Some(&(id, at)) = lanes.pending().get(i) else {
                        continue;
                    };
                    let x0 = x_of(at.as_quarters() as f32);
                    let rect = Rect::from_x_y_ranges(
                        x0..=x0 + 160.0,
                        lane.top() + 3.0..=lane.bottom() - 3.0,
                    );
                    let name = lanes
                        .library
                        .get(id)
                        .map_or_else(String::new, |i| i.name.clone());
                    let state = lanes.library.get(id).map_or(ClipState::Loading, item_state);
                    let progress = lanes.library.progress(id);
                    let shown = lanes.shown(id, ctx);
                    let color =
                        theme::CLIP_COLORS[(project.tracks.len() + i) % theme::CLIP_COLORS.len()];
                    draw_clip(
                        painter,
                        rect,
                        lane,
                        &ClipLook {
                            name: &name,
                            color,
                            state,
                            progress,
                            alpha: shown,
                        },
                    );
                }
            }
            painter.hline(
                canvas.x_range(),
                lane.bottom() - 0.5,
                Stroke::new(1.0, palette.border.gamma_multiply(0.6)),
            );
            // The resize glow continues across the lane.
            if let (RowKind::Track(index), Some((hot, t))) = (row.kind, self.hot_edge)
                && project.tracks[index].id == hot
            {
                draw_edge_glow(painter, canvas.x_range(), lane.bottom(), t);
            }
        }
    }

    /// The file of the audio clip under `pointer`, for the hover card.
    pub fn item_at<'a>(
        &self,
        ctx: &egui::Context,
        pointer: Pos2,
        canvas: Rect,
        project: &Project,
        lanes: &'a Lanes,
        x_of: impl Fn(f32) -> f32,
    ) -> Option<&'a Item> {
        let rows = self.rows(ctx, project, lanes.pending().len());
        let row = rows.iter().find(|r| {
            let y = canvas.top() + r.y - self.top;
            (y..y + r.height).contains(&pointer.y)
        })?;
        match row.kind {
            RowKind::Track(index) => project.tracks[index].clips.iter().find_map(|clip| {
                let x0 = x_of(clip.start.as_quarters() as f32);
                let x1 = x_of((clip.start + clip.length).as_quarters() as f32);
                match &clip.content {
                    ClipContent::Audio { media, .. } if (x0..=x1).contains(&pointer.x) => {
                        lanes.library.get(media.0)
                    }
                    _ => None,
                }
            }),
            RowKind::Pending(i) => {
                let &(id, at) = lanes.pending().get(i)?;
                let x0 = x_of(at.as_quarters() as f32);
                (x0..=x0 + 160.0)
                    .contains(&pointer.x)
                    .then(|| lanes.library.get(id))
                    .flatten()
            }
        }
    }
}

/// What a clip's audio is doing, for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClipState {
    Ready,
    Loading,
    Failed,
    Missing,
}

fn item_state(item: &Item) -> ClipState {
    match item.state {
        State::Ready(_) => ClipState::Ready,
        State::Loading => ClipState::Loading,
        State::Failed(_) => ClipState::Failed,
    }
}

/// A clip: dark body, track-coloured border and title bar with the name.
/// `alpha` fades it in and dims it when the track cannot be heard.
/// How a clip looks: its name, track colour and load state, and `alpha`,
/// which fades it in and dims it when the track cannot be heard.
struct ClipLook<'a> {
    name: &'a str,
    color: Color32,
    state: ClipState,
    progress: Option<f32>,
    alpha: f32,
}

fn draw_clip(painter: &Painter, rect: Rect, lane: Rect, look: &ClipLook<'_>) {
    let &ClipLook {
        name,
        color,
        state,
        progress,
        alpha,
    } = look;
    let palette = theme::p();
    let radius = theme::radius::SMALL;
    let (bar, label) = match state {
        ClipState::Ready => (color, name.to_owned()),
        ClipState::Loading => (
            color.gamma_multiply(0.6),
            format!(
                "{name} \u{00B7} loading {:.0}%",
                progress.unwrap_or(0.0) * 100.0
            ),
        ),
        ClipState::Failed => (palette.record, format!("{name} \u{00B7} could not load")),
        ClipState::Missing => (palette.record, format!("{name} \u{00B7} file missing")),
    };
    painter.rect_filled(rect, radius, palette.clip_body.gamma_multiply(alpha));
    if let (ClipState::Loading, Some(p)) = (state, progress) {
        let done = Rect::from_min_size(rect.min, vec2(rect.width() * p, rect.height()));
        painter.rect_filled(done, radius, color.gamma_multiply(0.25 * alpha));
    }
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE.min(rect.height())));
    painter.rect_filled(
        title,
        egui::CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        },
        bar.gamma_multiply(alpha),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, color.gamma_multiply(0.6 * alpha)),
        egui::StrokeKind::Inside,
    );
    let visible = title.intersect(lane);
    if visible.width() > 8.0 {
        painter
            .with_clip_rect(visible.shrink2(vec2(4.0, 0.0)))
            .text(
                pos2(visible.left() + 5.0, title.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(theme::size::CAPTION),
                // Dimmed clips keep a readable name in both themes.
                if alpha < 0.9 {
                    palette.text_secondary.gamma_multiply(alpha.max(0.6))
                } else {
                    theme::CLIP_TEXT.gamma_multiply(alpha)
                },
            );
    }
}
