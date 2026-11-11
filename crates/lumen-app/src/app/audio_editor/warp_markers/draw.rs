//! How the warp strip is painted (M9.5 S5b): its ground, each marker's notch
//! and the hairline through the wave, the ghost, the focus ring, and the small
//! triangles that mark the transients.

use eframe::egui::{self, Painter, Rect, Response, Stroke, pos2};
use lumen_media::detect::Analysis;

use crate::App;
use crate::audio_editor::detect::in_view;
use crate::audio_editor::paint;
use crate::audio_editor::source::WholeFile;
use crate::audio_editor::warp_strip::WarpStrip;
use crate::audio_editor::{AudioEditor, StripHover};
use crate::theme;
use crate::timeview::TimeView;

impl App {
    /// The strip's ground, drawn before the transients' triangles.
    pub(crate) fn paint_strip_ground(painter: &Painter, rect: Rect) {
        let palette = theme::p();
        painter.rect_filled(rect, 0.0, palette.raised);
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
    }

    /// The strip's notches, the ghost and a hairline down each marker
    /// through the wave, over the ground and the triangles.
    pub(crate) fn paint_warp_strip(
        &self,
        painter: &Painter,
        strip: &WarpStrip,
        body: Rect,
        ed: &AudioEditor,
        focused: Option<&(usize, Response)>,
    ) {
        let palette = theme::p();
        let rect = strip.rect;
        let view = rect.x_range();
        let strip_painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
        let wave_painter = painter.with_clip_rect(body.intersect(painter.clip_rect()));
        let dragged = ed.markers.drag.as_ref().map(|d| d.index);
        let hot = match ed.markers.hover {
            Some(StripHover::Marker(i)) => Some(i),
            _ => None,
        };
        for (index, x) in strip.flags.iter().enumerate() {
            if !view.contains(*x) {
                continue;
            }
            // Touched or chosen: the accent. Under the pointer: the notch
            // keeps its colour and its pole thickens.
            let chosen = Some(index) == dragged
                || Some(index) == ed.markers.picked
                || focused.is_some_and(|(i, _)| *i == index);
            let lit = chosen || Some(index) == hot;
            let ink = if chosen { palette.accent } else { palette.text };
            // The notch's tip, its pole and the hairline through the wave
            // share one x and one stroke.
            let x = snapped(painter, *x);
            let pole = Stroke::new(
                if lit {
                    theme::stroke::LIT
                } else {
                    theme::stroke::HAIRLINE
                },
                ink.gamma_multiply(if lit {
                    theme::WARP_POLE_LIT
                } else {
                    theme::WARP_POLE
                }),
            );
            wave_painter.vline(x, body.y_range(), pole);
            Self::paint_notch(
                &strip_painter,
                rect,
                x,
                Look {
                    ink,
                    pole,
                    ghost: false,
                    lifted: Some(index) == hot,
                },
            );
        }
        if let Some(StripHover::Ghost(g)) = ed.markers.hover
            && dragged.is_none()
            && view.contains(g.x)
        {
            let ink = palette.accent.gamma_multiply(if g.on_transient {
                1.0
            } else {
                theme::WARP_GHOST_FREE
            });
            Self::paint_notch(
                &strip_painter,
                rect,
                snapped(painter, g.x),
                Look {
                    ink,
                    pole: Stroke::new(theme::stroke::HAIRLINE, ink),
                    ghost: true,
                    lifted: false,
                },
            );
        }
        // The ring reaches past the strip (it is round the notch), so it is
        // drawn with the editor's painter, not the strip's.
        if let Some((index, response)) = focused
            && let Some(ring) = strip.notch_rect(*index)
        {
            crate::widgets::raised::focus_ring(painter, response, ring);
        }
    }

    /// The transients in the strip: a small triangle pointing down at each,
    /// except where a marker already stands (within a point). Only those in
    /// view are looked at. The triangle is the one a detected hit is drawn as
    /// on the wave too (`paint::hit_triangle`).
    pub(crate) fn paint_transient_ticks(
        painter: &Painter,
        strip: &WarpStrip,
        file: &WholeFile,
        (time, rate): (TimeView, u32),
        (analysis, sensitivity): (&Analysis, u8),
    ) {
        let Some((a, b)) = paint::visible_frames(file, time, strip.rect) else {
            return;
        };
        let rate = f64::from(rate);
        let inside = painter.with_clip_rect(strip.rect.intersect(painter.clip_rect()));
        let top = strip.rect.bottom() - theme::size::WARP_TICK_H - theme::size::WARP_TICK_FLOOR;
        for t in in_view(analysis, sensitivity, (a as f64 / rate, b as f64 / rate)) {
            let Some(beat) = file.beat_of_frame(t.seconds * rate) else {
                continue;
            };
            let x = time.x(strip.rect.left(), beat);
            // The markers are in time order: one beside this transient is
            // the first at or after a point before it.
            let next = strip.flags.partition_point(|f| *f < x - 1.0);
            if strip.flags.get(next).is_some_and(|f| *f < x + 1.0) {
                continue;
            }
            paint::hit_triangle(&inside, x, top, t.strength);
        }
    }

    /// One notch: a rounded pin centred on `x` that points down at its
    /// place, with its pole under it to the strip's floor.
    fn paint_notch(painter: &Painter, rect: Rect, x: f32, look: Look) {
        let (w, h) = (theme::size::WARP_NOTCH_W, theme::size::WARP_NOTCH_H);
        let (left, right) = (x - w / 2.0, x + w / 2.0);
        let top = rect.top() + theme::size::WARP_NOTCH_INSET;
        let r = theme::size::WARP_NOTCH_RADIUS;
        let shoulder = top + h - theme::size::WARP_NOTCH_TAPER;
        let mut points = Vec::with_capacity(12);
        for (cx, cy, from) in [(left + r, top + r, 180.0_f32), (right - r, top + r, 270.0)] {
            for step in 0..=3 {
                let angle = (from + 30.0 * step as f32).to_radians();
                points.push(pos2(cx + r * angle.cos(), cy + r * angle.sin()));
            }
        }
        points.push(pos2(right, shoulder));
        points.push(pos2(x, top + h));
        points.push(pos2(left, shoulder));
        painter.vline(x, top + h..=rect.bottom(), look.pole);
        let (fill, stroke) = if look.ghost {
            (
                look.ink.gamma_multiply(theme::WARP_GHOST_FILL),
                Stroke::new(theme::stroke::LIT, look.ink),
            )
        } else if look.lifted {
            (
                look.ink,
                Stroke::new(theme::stroke::HAIRLINE, theme::p().accent),
            )
        } else {
            (look.ink, Stroke::NONE)
        };
        painter.add(egui::Shape::convex_polygon(points, fill, stroke));
    }
}

/// How a notch is drawn: its ink, the stroke of its pole, whether it is the
/// ghost (a rim and a faint fill) and whether it is lifted (a marker under
/// the pointer, with an accent rim).
struct Look {
    ink: egui::Color32,
    pole: Stroke,
    ghost: bool,
    lifted: bool,
}

/// `x` on a whole physical pixel, so a notch's tip, its pole and the
/// hairline through the wave land on the same one.
fn snapped(painter: &Painter, x: f32) -> f32 {
    let ppp = painter.pixels_per_point();
    (x * ppp).round() / ppp
}
