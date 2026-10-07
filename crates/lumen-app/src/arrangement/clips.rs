//! The lanes and their clips (M3 slice 4): dark clips with a
//! track-coloured title bar, the armed and soloed lane hints, dimmed clips
//! on tracks that cannot be heard, and the hover lookup for the file card.
//!
//! Clip bodies are drawn on the GPU (slice 4b, ADR-0026). The boxes are
//! uploaded only when the project, the media or the theme change; where
//! rows are (scrolling, folding, resizing) goes up as a small table each
//! frame. Names, and the few clips still loading for import, are egui text
//! and shapes, drawn only where they are visible.

use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use lumen_project::model::{Clip, ClipContent, Project};
use lumen_render::{BoxInstance, RowSlot, rgba};

use super::{Arrangement, Row, RowKind, TITLE, color_of};
use crate::canvas::{self, ClipFrame};
use crate::document::{Document, audible};
use crate::lanes::Lanes;
use crate::media::{Item, State};
use crate::theme;
use crate::widgets::{draw_edge_glow, gradient};

/// Space between a lane's edges and its clips.
const INSET: f32 = 3.0;
/// The opacity of clips on tracks that cannot be heard.
const DIMMED: f32 = 0.42;
/// Clips narrower than this show no name: a letter or two is noise, and
/// at wide zooms thousands of names cost more than the clips.
const NAME_MIN_WIDTH: f32 = 24.0;
/// Width of a clip still loading for import, before it has a length.
const PENDING_WIDTH: f32 = 160.0;

/// The horizontal view: the beat (quarter note) at the canvas's left edge,
/// and points per beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeView {
    pub left_beat: f32,
    pub px_per_beat: f32,
}

impl TimeView {
    fn x(self, canvas: Rect, quarters: f32) -> f32 {
        canvas.left() + (quarters - self.left_beat) * self.px_per_beat
    }

    /// The clip's left and right edges.
    fn span(self, canvas: Rect, clip: &Clip) -> (f32, f32) {
        (
            self.x(canvas, clip.start.as_quarters() as f32),
            self.x(canvas, (clip.start + clip.length).as_quarters() as f32),
        )
    }
}

/// The clip bodies on the GPU, and what they were built from.
#[derive(Debug, Default)]
pub(super) struct GpuClips {
    boxes: Arc<Vec<BoxInstance>>,
    generation: u64,
    /// The document state, media revision and palette the boxes show.
    built: Option<(u64, u64, theme::Palette)>,
    /// A clip is fading in or loading: rebuild next frame too.
    moving: bool,
}

impl Arrangement {
    /// The lanes and clips in `canvas`.
    pub fn lanes(
        &mut self,
        ctx: &egui::Context,
        painter: &Painter,
        canvas: Rect,
        doc: &Document,
        lanes: &mut Lanes,
        time: TimeView,
    ) {
        let palette = *theme::p();
        let project = doc.project();
        let audible = audible(project);
        let rows = self.rows(ctx, project, lanes.pending().len());
        let top = self.top;
        let lane_of = |row: &Row| {
            let lane = Rect::from_min_size(
                pos2(canvas.left(), canvas.top() + row.y - top),
                vec2(canvas.width(), row.height),
            );
            (lane.bottom() >= canvas.top() && lane.top() <= canvas.bottom()).then_some(lane)
        };

        // Lane hints, under the clips.
        for row in &rows {
            let (Some(lane), RowKind::Track(index)) = (lane_of(row), row.kind) else {
                continue;
            };
            let track = &project.tracks[index];
            if track.armed {
                // A red wash, strongest at the left, with thin red edges
                // (owner's choice: "glow and breathe").
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
        }

        // Clip bodies, on the GPU.
        self.refresh_boxes(ctx, doc.state(), project, lanes);
        let ppp = ctx.pixels_per_point();
        let slots = rows
            .iter()
            .filter_map(|row| match row.kind {
                RowKind::Track(index) => Some(RowSlot::new(
                    row.y * ppp,
                    row.height * ppp,
                    if audible(&project.tracks[index]) {
                        1.0
                    } else {
                        DIMMED
                    },
                )),
                RowKind::Pending(_) => None,
            })
            .collect();
        painter.add(canvas::clips_shape(
            canvas,
            ppp,
            ClipFrame {
                boxes: Arc::clone(&self.gpu.boxes),
                generation: self.gpu.generation,
                rows: slots,
                left_beat: time.left_beat,
                px_per_beat: time.px_per_beat,
                top: self.top,
                style: canvas::box_style(ppp, INSET, TITLE),
            },
        ));

        // Names; clips loading for import; lane edges.
        for row in &rows {
            let Some(lane) = lane_of(row) else {
                continue;
            };
            match row.kind {
                RowKind::Track(index) => {
                    let track = &project.tracks[index];
                    let heard = audible(track);
                    for clip in &track.clips {
                        let (x0, x1) = time.span(canvas, clip);
                        if x1 < canvas.left() || x0 > canvas.right() || x1 - x0 < NAME_MIN_WIDTH {
                            continue;
                        }
                        let rect = Rect::from_x_y_ranges(
                            x0..=x1,
                            lane.top() + INSET..=lane.bottom() - INSET,
                        );
                        let mut look = look_of(clip, color_of(track.color), lanes, ctx);
                        if !heard {
                            look.alpha *= DIMMED;
                        }
                        draw_name(painter, rect, lane, &look);
                    }
                }
                RowKind::Pending(i) => {
                    let Some(&(id, at)) = lanes.pending().get(i) else {
                        continue;
                    };
                    let x0 = time.x(canvas, at.as_quarters() as f32);
                    let rect = Rect::from_x_y_ranges(
                        x0..=x0 + PENDING_WIDTH,
                        lane.top() + INSET..=lane.bottom() - INSET,
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
                    let look = ClipLook {
                        name: &name,
                        color,
                        state,
                        progress,
                        alpha: shown,
                    };
                    draw_body(painter, rect, &look);
                    draw_name(painter, rect, lane, &look);
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

    /// Rebuilds the GPU boxes if the project, the media or the theme
    /// changed, or a clip is still fading in or loading.
    fn refresh_boxes(
        &mut self,
        ctx: &egui::Context,
        state: u64,
        project: &Project,
        lanes: &mut Lanes,
    ) {
        let palette = *theme::p();
        let key = (state, lanes.revision(), palette);
        if self.gpu.built == Some(key) && !self.gpu.moving {
            return;
        }
        let mut moving = lanes.is_loading();
        let count = project.tracks.iter().map(|t| t.clips.len()).sum();
        let mut boxes = Vec::with_capacity(count);
        for (row, track) in project.tracks.iter().enumerate() {
            let color = color_of(track.color);
            for clip in &track.clips {
                let look = look_of(clip, color, lanes, ctx);
                moving |= look.alpha < 1.0;
                boxes.push(clip_box(clip, row as u32, &look, &palette));
            }
        }
        self.gpu = GpuClips {
            boxes: Arc::new(boxes),
            generation: self.gpu.generation + 1,
            built: Some(key),
            moving,
        };
    }

    /// The file of the audio clip under `pointer`, for the hover card.
    pub fn item_at<'a>(
        &self,
        ctx: &egui::Context,
        pointer: Pos2,
        canvas: Rect,
        project: &Project,
        lanes: &'a Lanes,
        time: TimeView,
    ) -> Option<&'a Item> {
        let rows = self.rows(ctx, project, lanes.pending().len());
        let row = rows.iter().find(|r| {
            let y = canvas.top() + r.y - self.top;
            (y..y + r.height).contains(&pointer.y)
        })?;
        match row.kind {
            RowKind::Track(index) => project.tracks[index].clips.iter().find_map(|clip| {
                let (x0, x1) = time.span(canvas, clip);
                match &clip.content {
                    ClipContent::Audio { media, .. } if (x0..=x1).contains(&pointer.x) => {
                        lanes.library.get(media.0)
                    }
                    _ => None,
                }
            }),
            RowKind::Pending(i) => {
                let &(id, at) = lanes.pending().get(i)?;
                let x0 = time.x(canvas, at.as_quarters() as f32);
                (x0..=x0 + PENDING_WIDTH)
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

/// How a clip looks: its name, track colour and load state, and `alpha`,
/// which fades it in and dims it when the track cannot be heard.
struct ClipLook<'a> {
    name: &'a str,
    color: Color32,
    state: ClipState,
    progress: Option<f32>,
    alpha: f32,
}

/// A project clip's look, before dimming for its track.
fn look_of<'a>(
    clip: &'a Clip,
    color: Color32,
    lanes: &mut Lanes,
    ctx: &egui::Context,
) -> ClipLook<'a> {
    let (state, progress, key) = match &clip.content {
        ClipContent::Audio { media, .. } => (
            lanes
                .library
                .get(media.0)
                .map_or(ClipState::Missing, item_state),
            lanes.library.progress(media.0),
            media.0,
        ),
        ClipContent::Midi { .. } => (ClipState::Ready, None, clip.id.0),
    };
    ClipLook {
        name: &clip.name,
        color,
        state,
        progress,
        alpha: lanes.shown(key, ctx),
    }
}

/// The title bar's colour and text for a clip in `state`.
fn bar_and_label(look: &ClipLook<'_>, palette: &theme::Palette) -> (Color32, String) {
    let name = look.name;
    match look.state {
        ClipState::Ready => (look.color, name.to_owned()),
        ClipState::Loading => (
            look.color.gamma_multiply(0.6),
            format!(
                "{name} \u{00B7} loading {:.0}%",
                look.progress.unwrap_or(0.0) * 100.0
            ),
        ),
        ClipState::Failed => (palette.record, format!("{name} \u{00B7} could not load")),
        ClipState::Missing => (palette.record, format!("{name} \u{00B7} file missing")),
    }
}

/// A clip's body for the GPU: the same colours as [`draw_body`].
fn clip_box(clip: &Clip, row: u32, look: &ClipLook<'_>, palette: &theme::Palette) -> BoxInstance {
    let straight = |c: Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        rgba(r, g, b, a)
    };
    let a = look.alpha;
    let (bar, _) = bar_and_label(look, palette);
    BoxInstance {
        start: clip.start.as_quarters() as f32,
        end: (clip.start + clip.length).as_quarters() as f32,
        row,
        body: straight(palette.clip_body.gamma_multiply(a)),
        bar: straight(bar.gamma_multiply(a)),
        border: straight(look.color.gamma_multiply(0.6 * a)),
        fill: straight(look.color.gamma_multiply(0.25 * a)),
        progress: match look.state {
            ClipState::Loading => look.progress.unwrap_or(0.0),
            _ => 0.0,
        },
    }
}

/// A clip body drawn by egui: dark body, track-coloured border and title
/// bar. Only clips still loading for import use it; the rest are on the
/// GPU ([`clip_box`]).
fn draw_body(painter: &Painter, rect: Rect, look: &ClipLook<'_>) {
    let palette = theme::p();
    let radius = theme::radius::SMALL;
    let alpha = look.alpha;
    let (bar, _) = bar_and_label(look, palette);
    painter.rect_filled(rect, radius, palette.clip_body.gamma_multiply(alpha));
    if let (ClipState::Loading, Some(p)) = (look.state, look.progress) {
        let done = Rect::from_min_size(rect.min, vec2(rect.width() * p, rect.height()));
        painter.rect_filled(done, radius, look.color.gamma_multiply(0.25 * alpha));
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
        Stroke::new(1.0, look.color.gamma_multiply(0.6 * alpha)),
        egui::StrokeKind::Inside,
    );
}

/// A clip's name in its title bar, kept within the visible part of the
/// lane.
fn draw_name(painter: &Painter, rect: Rect, lane: Rect, look: &ClipLook<'_>) {
    let palette = theme::p();
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE.min(rect.height())));
    let visible = title.intersect(lane);
    if visible.width() <= 8.0 {
        return;
    }
    let (_, label) = bar_and_label(look, palette);
    let alpha = look.alpha;
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
