use std::sync::{Arc, Mutex};

use eframe::egui::{Mesh, Painter, Rect, Shape, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::TITLE;
use super::clips::{INSET, SPLIT_BELOW, WAVE_GAP, draw_recording_take, span_of};
use crate::recording::take_worker::Live;
use crate::theme;
use crate::timeview::TimeView;

const NAME: &str = "Recording";

const BEHIND: f32 = 0.45;

#[derive(Debug, Clone)]
pub(crate) struct AudioTakeLook {
    pub track: Id,
    pub from: Ticks,
    pub to: Ticks,
    pub live: Arc<Mutex<Live>>,
}

pub(super) fn column(middle: f32, amp: f32, lo: f32, hi: f32) -> (f32, f32) {
    let (mut top, mut bottom) = (middle - hi * amp, middle - lo * amp);
    if bottom - top < 1.0 {
        let mid = 0.5 * (top + bottom);
        (top, bottom) = (mid - 0.5, mid + 0.5);
    }
    (top, bottom)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rows {
    pub first: i32,
    pub end: i32,
    pub coverage: f32,
}

pub(super) fn rows(top: f32, bottom: f32) -> ([Rows; 3], usize) {
    let (first, end) = (top.floor() as i32, bottom.ceil() as i32);
    let mut runs = [Rows {
        first: 0,
        end: 0,
        coverage: 0.0,
    }; 3];
    let mut n = 0;
    let mut push = |first: i32, end: i32, coverage: f32| {
        if end > first && coverage > 0.0 {
            runs[n] = Rows {
                first,
                end,
                coverage,
            };
            n += 1;
        }
    };
    if end - first <= 1 {
        push(first, first + 1, bottom - top);
    } else {
        push(first, first + 1, (first + 1) as f32 - top);
        push(first + 1, end - 1, 1.0);
        push(end - 1, end, bottom - (end - 1) as f32);
    }
    (runs, n)
}

pub(super) fn draw(
    painter: &Painter,
    takes: &[AudioTakeLook],
    track: Id,
    lane: Rect,
    time: TimeView,
    canvas: Rect,
) {
    let ppp = painter.ctx().pixels_per_point();
    for take in takes.iter().filter(|t| t.track == track) {
        let (x0, x1) = span_of(time, canvas, take.from, take.to);
        if x1 < canvas.left() || x0 > canvas.right() {
            continue;
        }
        let rect = Rect::from_x_y_ranges(
            x0..=x1.max(x0 + 2.0),
            lane.top() + INSET..=lane.bottom() - INSET,
        );
        draw_recording_take(painter, rect, lane, NAME);
        let body = Rect::from_x_y_ranges(
            rect.left()..=rect.right(),
            rect.top() + TITLE + WAVE_GAP..=rect.bottom() - WAVE_GAP,
        );
        if body.height() < 4.0 {
            continue;
        }
        let Ok(live) = take.live.lock() else {
            continue;
        };
        if (live.peaks.frames() == 0 && live.previous.is_none()) || x1 <= x0 {
            continue;
        }
        let (top, height) = (body.top() * ppp, body.height() * ppp);
        let split = live.peaks.channels == 2 && body.height() >= SPLIT_BELOW;
        let halves: &[(f32, f32)] = if split {
            let half = height / 2.0;
            &[
                (top + half * 0.5, 0.5 * half - 0.5),
                (top + half * 1.5, 0.5 * half - 0.5),
            ]
        } else {
            &[(top + height * 0.5, 0.5 * height - 0.5)]
        };
        let origin = (f64::from(x0) * f64::from(ppp)).floor();
        let before = live.previous.as_ref();
        let extent = live.frames.max(before.map_or(0, |(f, _)| *f));
        let per_pixel = extent as f64 / ((f64::from(x1) - f64::from(x0)) * f64::from(ppp));
        let first = origin.max((f64::from(canvas.left()) * f64::from(ppp)).floor());
        let last = (f64::from(x1) * f64::from(ppp))
            .min(f64::from(canvas.right()) * f64::from(ppp))
            .ceil();
        let colour = theme::p().record_note;
        let scale = |v: i8| f32::from(v) / 127.0;
        let mut mesh = Mesh::default();
        let mut px = first;
        while px < last {
            let a = (px - origin) * per_pixel;
            let (peaks, fade) = if a < live.frames as f64 {
                (&live.peaks, 1.0)
            } else if let Some((_, peaks)) = before {
                (peaks, BEHIND)
            } else {
                px += 1.0;
                continue;
            };
            let [l_min, l_max, r_min, r_max] = peaks.extent(a, a + per_pixel);
            for (n, &(middle, amp)) in halves.iter().enumerate() {
                let (lo, hi) = if !split {
                    (scale(l_min.min(r_min)), scale(l_max.max(r_max)))
                } else if n == 0 {
                    (scale(l_min), scale(l_max))
                } else {
                    (scale(r_min), scale(r_max))
                };
                let (from, to) = column(middle, amp, lo, hi);
                let (runs, count) = rows(from, to);
                for run in &runs[..count] {
                    mesh.add_colored_rect(
                        Rect::from_min_max(
                            pos2(px as f32 / ppp, run.first as f32 / ppp),
                            pos2((px as f32 + 1.0) / ppp, run.end as f32 / ppp),
                        ),
                        colour.gamma_multiply(run.coverage * fade),
                    );
                }
            }
            px += 1.0;
        }
        painter
            .with_clip_rect(body.intersect(painter.clip_rect()))
            .add(Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn painted(top: f32, bottom: f32) -> Vec<(i32, i32, f32)> {
        let (runs, n) = rows(top, bottom);
        runs[..n]
            .iter()
            .map(|r| (r.first, r.end, r.coverage))
            .collect()
    }

    #[test]
    fn a_column_runs_from_the_low_to_the_high_around_the_centre() {
        assert_eq!(column(50.0, 20.0, -0.5, 1.0), (30.0, 60.0));
        assert_eq!(column(50.0, 20.0, 0.0, 0.0), (49.5, 50.5));
    }

    #[test]
    fn a_column_is_never_thinner_than_a_pixel() {
        let (top, bottom) = column(50.0, 20.0, 0.0, 0.01);
        assert!((bottom - top - 1.0).abs() < 1e-6);
        let (top, bottom) = column(50.0, 20.0, -0.01, 0.0);
        assert!((bottom - top - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_pixel_the_column_fills_is_painted_whole_wherever_the_column_sits() {
        assert_eq!(painted(10.0, 11.0), vec![(10, 11, 1.0)]);
        assert_eq!(
            painted(10.0, 13.0),
            vec![(10, 11, 1.0), (11, 12, 1.0), (12, 13, 1.0)]
        );
    }

    #[test]
    fn a_column_between_pixel_rows_shares_its_coverage_between_them() {
        assert_eq!(painted(10.5, 11.5), vec![(10, 11, 0.5), (11, 12, 0.5)]);
        let total: f32 = painted(10.25, 13.75)
            .iter()
            .map(|r| (r.1 - r.0) as f32 * r.2)
            .sum();
        assert!((total - 3.5).abs() < 1e-5, "{total}");
    }

    #[test]
    fn a_column_inside_one_row_covers_its_share_of_it() {
        let runs = painted(10.2, 10.8);
        assert_eq!(runs.len(), 1);
        assert_eq!((runs[0].0, runs[0].1), (10, 11));
        assert!((runs[0].2 - 0.6).abs() < 1e-5);
    }
}
