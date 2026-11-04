//! Drawing the audio editor (M9.5 S1): the file's wave, one column per
//! physical pixel from the peaks, bright inside the clip's window (at the
//! clip's gain and through its fades, as it plays) and dimmed outside it (as
//! the file is); sample by sample at the deepest zoom; and the ruler of bars
//! and file time. Only the columns in view are drawn, so a one-hour file
//! costs what a one-second one does.

use eframe::egui::{self, Align2, Color32, FontId, Painter, Rect, Stroke, pos2, vec2};
use lumen_core::grid::{Level, Line};
use lumen_core::meter::MeterMap;
use lumen_media::Peaks;
use lumen_project::model::Clip;

use super::source::WholeFile;
use crate::theme;
use crate::timeview::TimeView;

/// Below this many frames per physical pixel the wave is drawn sample by
/// sample (when the samples are there).
pub(crate) const SAMPLES_BELOW: f64 = 1.0;
/// From this many physical pixels per frame, each sample is a dot too.
const DOTS_FROM: f64 = 6.0;
/// The wave splits into left over right from this height (points).
pub(crate) const SPLIT_FROM: f32 = theme::size::WAVE_SPLIT;
/// The ruler: bar numbers on top, file time under them.
pub(crate) const RULER: f32 = theme::size::CLIP_RULER;

/// How the wave looks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WaveLook {
    /// Inside the clip's window.
    pub bright: Color32,
    /// Outside it.
    pub dim: Color32,
}

/// Samples for drawing sample by sample: the device-rate frame of the
/// first, and left and right as 16-bit fractions of full scale.
pub(crate) struct Samples<'a> {
    pub first: i64,
    pub frames: &'a [[i16; 2]],
}

/// What the wave is drawn from.
pub(crate) struct WaveInput<'a> {
    pub file: &'a WholeFile,
    pub peaks: &'a Peaks,
    pub clip: &'a Clip,
    pub time: TimeView,
    pub look: WaveLook,
    /// Left over right (a stereo file with room for two).
    pub split: bool,
    pub samples: Option<Samples<'a>>,
}

/// The frames under the view in `lane`: first and last (device rate),
/// widened by one so a line can reach the edge. `None` when no part of the
/// file is in view.
pub(crate) fn visible_frames(file: &WholeFile, time: TimeView, lane: Rect) -> Option<(i64, i64)> {
    let from = time.beat_at(lane.left(), lane.left()).max(file.from);
    let to = time.beat_at(lane.left(), lane.right()).min(file.to);
    if to <= from {
        return None;
    }
    let a = file.frame_at(from)?;
    let b = file.frame_at(to - 1e-9).or_else(|| {
        file.spans
            .last()
            .map(|s| s.first_frame + (s.end - s.start) * s.frames_per_beat)
    })?;
    Some(((a.floor() as i64 - 1).max(0), b.ceil() as i64 + 1))
}

/// Frames per physical pixel at the view's zoom, at beat `q`.
pub(crate) fn frames_per_pixel(file: &WholeFile, time: TimeView, q: f64, ppp: f32) -> Option<f64> {
    let span = file.span_at(q)?;
    Some(span.frames_per_beat / (time.zoom * f64::from(ppp)))
}

/// The level a frame inside the window plays at: the clip's gain (negative
/// when inverted) through its fades; 1 outside it, where the file is drawn
/// as it is.
fn level_at(input: &WaveInput<'_>, q: f64, gain: f32) -> f32 {
    let (start, end) = input.file.window;
    if q < start || q >= end {
        return 1.0;
    }
    let clip = input.clip;
    let mut level = gain;
    let into = q - clip.start.as_quarters();
    let fade_in = clip.fade_in.length.as_quarters();
    if fade_in > 0.0 && (0.0..fade_in).contains(&into) {
        level *= clip.fade_in.level((into / fade_in) as f32);
    }
    let left = (clip.start + clip.length).as_quarters() - q;
    let fade_out = clip.fade_out.length.as_quarters();
    if fade_out > 0.0 && (0.0..fade_out).contains(&left) {
        level *= clip.fade_out.level((left / fade_out) as f32);
    }
    level
}

/// The wave in `lane`.
pub(crate) fn wave(painter: &Painter, lane: Rect, input: &WaveInput<'_>) {
    let painter = painter.with_clip_rect(lane.intersect(painter.clip_rect()));
    let palette = theme::p();
    // The file outside the clip's window sits on a sunk wash.
    let (w0, w1) = input.file.window;
    let x = |q: f64| input.time.x(lane.left(), q);
    let file_rect = Rect::from_x_y_ranges(
        x(input.file.from).max(lane.left())..=x(input.file.to).min(lane.right()),
        lane.y_range(),
    );
    if file_rect.width() > 0.0 {
        painter.rect_filled(file_rect, 0.0, palette.well.gamma_multiply(0.7));
        let window = Rect::from_x_y_ranges(
            x(w0).max(file_rect.left())..=x(w1).min(file_rect.right()),
            lane.y_range(),
        );
        if window.width() > 0.0 {
            painter.rect_filled(window, 0.0, palette.clip_body);
        }
    }
    let lanes: Vec<(Rect, Option<usize>)> = if input.split {
        let half = lane.height() / 2.0;
        vec![
            (
                Rect::from_min_size(lane.min, vec2(lane.width(), half)),
                Some(0),
            ),
            (
                Rect::from_min_size(
                    pos2(lane.left(), lane.top() + half),
                    vec2(lane.width(), half),
                ),
                Some(1),
            ),
        ]
    } else {
        vec![(lane, None)]
    };
    let ppp = painter.pixels_per_point();
    match &input.samples {
        Some(samples) => {
            for (rect, channel) in &lanes {
                sample_lines(&painter, *rect, input, samples, *channel, ppp);
            }
        }
        None => {
            for (rect, channel) in &lanes {
                peak_columns(&painter, *rect, input, *channel, ppp);
            }
        }
    }
    if input.split {
        painter.hline(
            lane.x_range(),
            lane.center().y,
            Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
    }
}

/// One column a physical pixel wide: the extent of the frames under it.
fn peak_columns(
    painter: &Painter,
    lane: Rect,
    input: &WaveInput<'_>,
    channel: Option<usize>,
    ppp: f32,
) {
    let file = input.file;
    let pixel = 1.0 / ppp;
    let middle = lane.center().y;
    let half = lane.height() / 2.0 - theme::space::XXS;
    let first = ((input.time.x(lane.left(), file.from).max(lane.left()) - lane.left()) / pixel)
        .floor()
        .max(0.0) as usize;
    let last = ((input.time.x(lane.left(), file.to).min(lane.right()) - lane.left()) / pixel)
        .ceil()
        .max(0.0) as usize;
    let mut mesh = egui::Mesh::default();
    for column in first..last {
        let x0 = lane.left() + column as f32 * pixel;
        let q0 = input.time.beat_at(lane.left(), x0);
        let q1 = input.time.beat_at(lane.left(), x0 + pixel);
        let middle_q = (q0 + q1) / 2.0;
        let Some(span) = file.span_at(middle_q) else {
            continue;
        };
        let frame = |q: f64| {
            span.first_frame + (q.clamp(span.start, span.end) - span.start) * span.frames_per_beat
        };
        let (f0, f1) = (frame(q0), frame(q1));
        let [lmin, lmax, rmin, rmax] = input.peaks.extent(f0.min(f1), f0.max(f1));
        let (lo, hi) = match channel {
            Some(0) => (lmin, lmax),
            Some(_) => (rmin, rmax),
            None => (lmin.min(rmin), lmax.max(rmax)),
        };
        let level = level_at(input, middle_q, span.gain);
        let (a, b) = (f32::from(lo) / 127.0 * level, f32::from(hi) / 127.0 * level);
        let top = middle - a.max(b) * half;
        let bottom = middle - a.min(b) * half;
        let inside = middle_q >= file.window.0 && middle_q < file.window.1;
        let colour = if inside {
            input.look.bright
        } else {
            input.look.dim
        };
        mesh.add_colored_rect(
            Rect::from_min_max(pos2(x0, top), pos2(x0 + pixel, bottom.max(top + pixel))),
            colour,
        );
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The samples as a line through each sample's value, with a dot on each
/// when they are far apart.
fn sample_lines(
    painter: &Painter,
    lane: Rect,
    input: &WaveInput<'_>,
    samples: &Samples<'_>,
    channel: Option<usize>,
    ppp: f32,
) {
    let file = input.file;
    let middle = lane.center().y;
    let half = lane.height() / 2.0 - theme::space::XXS;
    let value = |s: [i16; 2]| match channel {
        Some(0) => f32::from(s[0]),
        Some(_) => f32::from(s[1]),
        None => (f32::from(s[0]) + f32::from(s[1])) / 2.0,
    } / 32_767.0;
    // Each sample's beat: the span it is in maps its frame back to time.
    let mut points = Vec::with_capacity(samples.frames.len());
    let mut colours = Vec::with_capacity(samples.frames.len());
    for (i, s) in samples.frames.iter().enumerate() {
        let frame = (samples.first + i as i64) as f64;
        let Some(span) = file.spans.iter().find(|sp| {
            frame >= sp.first_frame
                && frame < sp.first_frame + (sp.end - sp.start) * sp.frames_per_beat
        }) else {
            continue;
        };
        let q = span.start + (frame - span.first_frame) / span.frames_per_beat;
        let level = level_at(input, q, span.gain);
        let x = input.time.x(lane.left(), q);
        points.push(pos2(x, middle - value(*s) * level * half));
        let inside = q >= file.window.0 && q < file.window.1;
        colours.push(if inside {
            input.look.bright
        } else {
            input.look.dim
        });
    }
    for pair in points.windows(2).zip(&colours) {
        painter.line_segment([pair.0[0], pair.0[1]], Stroke::new(1.0, *pair.1));
    }
    let spacing = points
        .windows(2)
        .next()
        .map_or(0.0, |p| f64::from((p[1].x - p[0].x) * ppp));
    if spacing >= DOTS_FROM {
        for (p, c) in points.iter().zip(&colours) {
            painter.circle_filled(*p, 2.0, *c);
        }
    }
}

/// The ruler in `strip`: bar numbers on top with the grid's bar and beat
/// ticks, the file's time under them, and the clip's window as a band on
/// its bottom edge.
pub(crate) fn ruler(
    painter: &Painter,
    strip: Rect,
    time: TimeView,
    lines: &[Line],
    meter: &MeterMap,
    file: &WholeFile,
    rate: u32,
) {
    let palette = theme::p();
    let painter = painter.with_clip_rect(strip.intersect(painter.clip_rect()));
    painter.rect_filled(strip, 0.0, palette.raised);
    let x = |q: f64| time.x(strip.left(), q);
    // The clip's window, a band along the bottom (the loop brace's place).
    let (w0, w1) = file.window;
    painter.rect_filled(
        Rect::from_min_max(
            pos2(x(w0).max(strip.left()), strip.bottom() - 3.0),
            pos2(x(w1).min(strip.right()).max(x(w0)), strip.bottom()),
        ),
        0.0,
        palette.accent_muted,
    );
    let mid = strip.top() + strip.height() / 2.0;
    let mut last_label = f32::NEG_INFINITY;
    for line in lines {
        let lx = x(line.at.as_quarters());
        match line.level {
            Level::Bar => {
                painter.vline(
                    lx,
                    strip.top() + 4.0..=mid,
                    Stroke::new(1.0, palette.grid_bar),
                );
                if lx - last_label > theme::size::BAR_NUMBER_ROOM {
                    last_label = lx;
                    painter.text(
                        pos2(lx + 3.0, strip.top() + 2.0),
                        Align2::LEFT_TOP,
                        format!("{}", meter.bbt_at(line.at).bar + 1),
                        FontId::monospace(theme::size::SCALE),
                        palette.text_secondary,
                    );
                }
            }
            Level::Beat => {
                painter.vline(lx, mid - 5.0..=mid, Stroke::new(1.0, palette.grid_beat));
            }
            Level::Sub => {}
        }
    }
    // The file's own time, from its first frame, at a step that keeps the
    // labels apart: device-rate frames over the rate.
    let rate = f64::from(rate.max(1));
    if let Some(step) = time_step(file, time, strip, rate) {
        let from = time.beat_at(strip.left(), strip.left()).max(file.from);
        let to = time.beat_at(strip.left(), strip.right()).min(file.to);
        if let (Some(a), Some(b)) = (file.frame_at(from), file.frame_at(to - 1e-9)) {
            let (s0, s1) = (a / rate, b / rate);
            let mut k = (s0 / step).ceil() as i64;
            while (k as f64) * step <= s1 {
                let t = k as f64 * step;
                if let Some(q) = beat_of_frame(file, t * rate) {
                    let lx = x(q);
                    painter.vline(
                        lx,
                        strip.bottom() - 6.0..=strip.bottom(),
                        Stroke::new(1.0, palette.border_strong),
                    );
                    painter.text(
                        pos2(lx + 3.0, strip.bottom() - 3.0),
                        Align2::LEFT_BOTTOM,
                        seconds_text(t, step),
                        FontId::monospace(theme::size::SCALE),
                        palette.text_hint,
                    );
                }
                k += 1;
            }
        }
    }
    painter.hline(
        strip.x_range(),
        strip.bottom(),
        Stroke::new(1.0, palette.border),
    );
}

/// The beat where device-rate `frame` of the file plays.
fn beat_of_frame(file: &WholeFile, frame: f64) -> Option<f64> {
    file.spans
        .iter()
        .find(|s| {
            frame >= s.first_frame && frame < s.first_frame + (s.end - s.start) * s.frames_per_beat
        })
        .map(|s| s.start + (frame - s.first_frame) / s.frames_per_beat)
}

/// A step in seconds that keeps time labels `TIME_LABEL_ROOM` apart.
fn time_step(file: &WholeFile, time: TimeView, strip: Rect, rate: f64) -> Option<f64> {
    let q = time.beat_at(strip.left(), strip.center().x);
    let span = file.span_at(q).or_else(|| file.spans.first())?;
    // Points per second here.
    let per_second = time.zoom * rate / span.frames_per_beat;
    const STEPS: [f64; 19] = [
        0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0,
        60.0, 120.0, 300.0, 600.0,
    ];
    STEPS
        .iter()
        .copied()
        .find(|s| s * per_second >= f64::from(theme::size::TIME_LABEL_ROOM))
}

/// "0:01.250" style: minutes and seconds, with as many decimals as `step`
/// needs.
pub(crate) fn seconds_text(seconds: f64, step: f64) -> String {
    let decimals = if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else if step >= 0.01 {
        2
    } else {
        3
    };
    let minutes = (seconds / 60.0).floor();
    let rest = seconds - minutes * 60.0;
    let width = if decimals == 0 { 2 } else { 3 + decimals };
    format!(
        "{}:{:0width$.decimals$}",
        minutes as i64,
        rest,
        width = width,
        decimals = decimals
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_time_reads_in_minutes_and_seconds() {
        assert_eq!(seconds_text(0.0, 1.0), "0:00");
        assert_eq!(seconds_text(65.0, 5.0), "1:05");
        assert_eq!(seconds_text(1.25, 0.05), "0:01.25");
        assert_eq!(seconds_text(0.5, 0.5), "0:00.5");
        assert_eq!(seconds_text(0.0015, 0.001), "0:00.002");
    }
}
