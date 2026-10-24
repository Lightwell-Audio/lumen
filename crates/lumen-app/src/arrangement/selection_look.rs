//! How the clip selection is drawn (M3 slice 6a, the owner's S3 and H1):
//! selected clips glow, the hovered header or body brightens,
//! a time range is a band with accent brackets, and the
//! marquee is a soft rounded box. Split from `clips.rs` (M4 review M1).

use eframe::egui::{self, Painter, Rect, Stroke, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_project::model::Project;

use super::clips::{CLAMP, INSET, span};
use super::handles::{ClipHandles, HandleLook, draw_handles};
use super::{Arrangement, Row, RowKind, TITLE, select};
use crate::timeview::TimeView;
use crate::widgets::draw_glow_rect;
use crate::{light, theme};

impl Arrangement {
    /// The selection, in the owner's choices: selected clips glow (S3),
    /// the hovered header or body brightens,
    /// a time range is a band with accent brackets along its top, and
    /// the marquee is a soft rounded box.
    pub(super) fn draw_selection(
        &self,
        painter: &Painter,
        canvas: Rect,
        project: &Project,
        rows: &[Row],
        time: TimeView,
    ) {
        let palette = theme::p();
        let painter = painter.with_clip_rect(canvas);
        let lane = |row: &Row| {
            Rect::from_min_size(
                pos2(canvas.left(), canvas.top() + row.y - self.top),
                vec2(canvas.width(), row.height),
            )
        };
        let x = |t: Ticks| time.x(canvas.left(), t.as_quarters());
        // Deep in, a clip or range can be hundreds of millions of points
        // wide: everything drawn here is cut to the lanes in view (plus a
        // margin, so edges outside stay outside). A dashed line across a
        // whole long clip once asked for 64 GiB (owner, after slice 6c).
        let (near_left, near_right) = (canvas.left() - 16.0, canvas.right() + 16.0);
        let shown = |x0: f32, x1: f32| (x0.max(near_left), x1.min(near_right));

        if let Some(range) = &self.range {
            let (x0, x1) = shown(x(range.start), x(range.end));
            let mut top = None;
            for row in rows {
                let RowKind::Track(index) = row.kind else {
                    continue;
                };
                if !range.tracks.contains(&project.tracks[index].id) {
                    continue;
                }
                let band = Rect::from_x_y_ranges(x0..=x1, lane(row).y_range());
                painter.rect_filled(band, 0.0, palette.accent.gamma_multiply(0.16));
                let edge = Stroke::new(1.0, palette.accent.gamma_multiply(0.6));
                painter.vline(x0, band.y_range(), edge);
                painter.vline(x1, band.y_range(), edge);
                top.get_or_insert(band.top());
            }
            if let Some(y) = top {
                let stroke = Stroke::new(2.0, palette.accent);
                let y = y + 1.0;
                painter.line_segment([pos2(x0, y), pos2(x1, y)], stroke);
                for edge in [x0, x1] {
                    painter.line_segment([pos2(edge, y), pos2(edge, y + 5.0)], stroke);
                }
            }
        }

        for row in rows {
            let RowKind::Track(index) = row.kind else {
                continue;
            };
            let lane = lane(row);
            if lane.bottom() < canvas.top() || lane.top() > canvas.bottom() {
                continue;
            }
            for clip in &project.tracks[index].clips {
                let selected = self.clips.contains(&clip.id);
                let hovered = self.hover.filter(|h| h.clip == clip.id);
                let (left, right) = span(time, canvas, clip);
                if right < canvas.left() || left > canvas.right() {
                    continue;
                }
                let (x0, x1) = shown(left, right);
                let rect = Rect::from_x_y_ranges(
                    x0..=x1.max(x0 + 1.0),
                    lane.top() + INSET..=lane.bottom() - INSET,
                );
                // Each repeat of a looping clip starts at a notch: from the
                // first in view, not the clip's start.
                if let Some(period) = clip.loop_length.filter(|p| *p > Ticks::ZERO) {
                    let step = (period.as_quarters() * time.zoom) as f32;
                    if step >= 6.0 {
                        let view = crate::ruler::ticks_at(time.left);
                        let skipped = ((view - clip.start).0 / period.0).max(1);
                        let mut at = clip.start + Ticks(skipped * period.0);
                        while at < clip.start + clip.length {
                            let x = x(at);
                            if x > canvas.right() {
                                break;
                            }
                            if x >= canvas.left() {
                                painter.add(egui::Shape::dashed_line(
                                    &[pos2(x, rect.top() + TITLE), pos2(x, rect.bottom())],
                                    Stroke::new(1.0, palette.text.gamma_multiply(0.3)),
                                    2.0,
                                    2.0,
                                ));
                                painter.circle_filled(
                                    pos2(x, rect.top() + TITLE),
                                    2.0,
                                    palette.text.gamma_multiply(0.6),
                                );
                            }
                            at = at + period;
                        }
                    }
                }
                // Fades and gain (slice 6d), placed by the clip's whole
                // extent and drawn only where they are in view.
                // A frozen track's clips are clamped like locked ones.
                let locked = clip.locked || project.tracks[index].frozen.is_some();
                let whole = Rect::from_x_y_ranges(left..=right, rect.y_range());
                if locked && right - left >= 4.0 * CLAMP {
                    // K3: clamped at both ends; it has no handles to show.
                    let clamp = palette.text.gamma_multiply(0.55);
                    for x in [left, right - CLAMP] {
                        if (near_left..=near_right).contains(&x) {
                            let bar = Rect::from_x_y_ranges(x..=x + CLAMP, rect.y_range());
                            painter.rect_filled(bar, 0.0, clamp);
                        }
                    }
                }
                // At fit-all thousands of clips are too narrow for handles.
                // Reject those before cloning a clip or resolving its join.
                let handles = (!locked)
                    .then(|| ClipHandles::of(clip, whole, x))
                    .flatten()
                    .and_then(|_| {
                        let shown =
                            crate::crossfades::preview(project, project.tracks[index].id, clip);
                        ClipHandles::of(&shown, whole, x)
                    });
                if let Some(h) = handles {
                    let view = egui::Rangef::new(near_left, near_right);
                    let track = &project.tracks[index];
                    // The colours of the two clips a crossfade blends.
                    let crossing = h
                        .crossfade
                        .then(|| {
                            track
                                .clips
                                .iter()
                                .find(|c| c.id != clip.id && c.start == clip.start + clip.length)
                                .map(|next| {
                                    (
                                        super::clip_color(track, clip),
                                        super::clip_color(track, next),
                                    )
                                })
                        })
                        .flatten();
                    draw_handles(
                        &painter,
                        &h,
                        view,
                        HandleLook {
                            hovered: hovered.is_some(),
                            hot: hovered.and_then(|h| h.handle),
                            crossing,
                        },
                    );
                }
                if !selected && hovered.is_none() {
                    continue;
                }
                if let Some(select::Hover {
                    edge: Some(edge), ..
                }) = hovered
                {
                    // L3: the side that would trim glows as a rail.
                    let side = match edge {
                        select::Edge::Start => rect.left() + 1.5,
                        select::Edge::End => rect.right() - 1.5,
                    };
                    painter.vline(
                        side,
                        rect.y_range(),
                        Stroke::new(6.0, palette.accent.gamma_multiply(0.15)),
                    );
                    painter.vline(
                        side,
                        rect.y_range(),
                        Stroke::new(3.0, palette.accent.gamma_multiply(0.6)),
                    );
                } else if let Some(select::Hover {
                    region,
                    edge: None,
                    handle: None,
                    ..
                }) = hovered
                {
                    let title_bottom = select::title_bottom(rect);
                    let wash = match region {
                        select::ClipRegion::Header => rect.with_max_y(title_bottom),
                        select::ClipRegion::Body => rect.with_min_y(title_bottom),
                    };
                    painter.rect_filled(wash, 0.0, palette.text.gamma_multiply(light::EMBER_WASH));
                }
                if selected {
                    draw_glow_rect(&painter, rect, f32::from(theme::radius::SMALL));
                }
            }
        }

        if let Some(area) = self.marquee {
            painter.rect(
                area,
                f32::from(theme::radius::MEDIUM),
                palette.accent.gamma_multiply(0.12),
                Stroke::new(1.0, palette.accent.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
        }
    }
}
