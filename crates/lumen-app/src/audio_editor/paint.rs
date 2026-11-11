//! Drawing the audio editor (M9.5 S1): the file's wave, one column per
//! physical pixel from the peaks, bright where the clip plays it (at the
//! clip's gain and through its fades, as it plays) and dimmed outside it (as
//! the file is); sample by sample at the deepest zoom; and the ruler of bars
//! and file time. Only the columns in view are drawn, so a one-hour file
//! costs what a one-second one does.

use std::sync::Arc;

use eframe::egui::{self, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use lumen_core::grid::{Level, Line};
use lumen_core::meter::MeterMap;
use lumen_media::Peaks;
use lumen_project::model::Clip;

use super::source::WholeFile;
use crate::theme;
use crate::timeview::TimeView;

/// Below this many frames per physical pixel the wave is drawn sample by
/// sample (when the samples are there). The finest peaks are 16 frames a
/// bucket, so between this and 16 they show as small steps; 4 keeps a
/// 4,096-pixel view within the samples read for one frame.
pub(crate) const SAMPLES_BELOW: f64 = 4.0;
/// From this many physical pixels per frame, each sample is a dot too.
const DOTS_FROM: f64 = 6.0;
/// The wave splits into left over right from this height (points).
pub(crate) const SPLIT_FROM: f32 = theme::size::WAVE_SPLIT;

/// How the wave looks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WaveLook {
    /// What the clip plays.
    pub bright: Color32,
    /// The rest of the file.
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
    /// The grid's lines as x and level, drawn over the clip's body under the
    /// wave (the body would otherwise cover the grid).
    pub grid: &'a [(f32, lumen_core::grid::Level)],
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

/// The level a frame the clip plays is heard at: the clip's gain (negative
/// when inverted) through its fades; 1 outside it, where the file is drawn
/// as it is.
fn level_at(input: &WaveInput<'_>, q: f64, gain: f32) -> f32 {
    if !input.file.played.contains(q) {
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
    // The file outside what the clip plays sits on a sunk wash.
    let x = |q: f64| input.time.x(lane.left(), q);
    let file_rect = Rect::from_x_y_ranges(
        x(input.file.from).max(lane.left())..=x(input.file.to).min(lane.right()),
        lane.y_range(),
    );
    if file_rect.width() > 0.0 {
        painter.rect_filled(
            file_rect,
            0.0,
            palette.well.gamma_multiply(theme::OUTSIDE_WASH),
        );
        for (a, b) in input.file.played.parts() {
            let (a, b) = (x(a).max(file_rect.left()), x(b).min(file_rect.right()));
            if b > a {
                let part = Rect::from_x_y_ranges(a..=b, lane.y_range());
                painter.rect_filled(part, 0.0, palette.clip_body);
                for (gx, level) in input.grid {
                    if (a..=b).contains(gx) {
                        let ink = match level {
                            Level::Bar => palette.clip_grid_bar,
                            Level::Beat => palette.clip_grid_beat,
                            Level::Sub => palette.clip_grid_sub,
                        };
                        painter.vline(
                            *gx,
                            lane.y_range(),
                            Stroke::new(theme::stroke::HAIRLINE, ink),
                        );
                    }
                }
            }
        }
        // The file's two ends, so where it stops reads in both themes (in
        // the light one the window and the ground past the file are both
        // near white).
        for end in [input.file.from, input.file.to] {
            let ex = x(end);
            if lane.x_range().contains(ex) {
                painter.vline(
                    ex,
                    lane.y_range(),
                    Stroke::new(theme::stroke::HAIRLINE, palette.border_strong),
                );
            }
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

/// A detected hit: the small grey triangle both places draw for one, in the
/// warp strip once the clip is warped (M9.5 S5b, ADR-0136) and at the top of
/// the wave while its Warp popover is open on a clip that is not
/// (`App::paint_transient_ticks`, `App::paint_transients`). It sits with its
/// base on `top` and its point on `x`, and `strength` (0 to 1) decides how
/// strongly it shows.
pub(crate) fn hit_triangle(painter: &Painter, x: f32, top: f32, strength: f32) {
    painter.add(egui::Shape::convex_polygon(
        hit_triangle_points(x, top),
        hit_ink(strength),
        Stroke::NONE,
    ));
}

/// The triangle a detected hit is drawn as: a base centred on `x` at `top`
/// and a point at `x` a [`theme::size::WARP_TICK_H`] below it.
fn hit_triangle_points(x: f32, top: f32) -> Vec<Pos2> {
    let (w, h) = (theme::size::WARP_TICK_W, theme::size::WARP_TICK_H);
    vec![
        pos2(x - w / 2.0, top),
        pos2(x + w / 2.0, top),
        pos2(x, top + h),
    ]
}

/// The triangle's ink: `text_secondary` at the strength-scaled alpha a hit
/// shows at, from [`theme::WARP_TICK_BASE`] for a soft one to that plus
/// [`theme::WARP_TICK_GAIN`] for the loudest.
fn hit_ink(strength: f32) -> Color32 {
    theme::p()
        .text_secondary
        .gamma_multiply(theme::WARP_TICK_BASE + theme::WARP_TICK_GAIN * strength.clamp(0.0, 1.0))
}

/// The top of a detected hit's triangle on the wave of a clip that is not
/// warped: just inside the wave body's top edge, by the gap
/// [`theme::size::WARP_TICK_FLOOR`] the warp strip keeps under its own
/// triangles, so the same triangle reads the same way in either place.
pub(crate) fn wave_hit_top(body: Rect) -> f32 {
    body.top() + theme::size::WARP_TICK_FLOOR
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
        let colour = if file.played.contains(middle_q) {
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
        let Some((q, gain)) = beat_and_gain(file, frame) else {
            continue;
        };
        let level = level_at(input, q, gain);
        let x = input.time.x(lane.left(), q);
        points.push(pos2(x, middle - value(*s) * level * half));
        colours.push(if file.played.contains(q) {
            input.look.bright
        } else {
            input.look.dim
        });
    }
    // One line per run of one colour (the played parts' edges are the only
    // changes), not one shape per pair of samples.
    let mut start = 0;
    while start + 1 < points.len() {
        let colour = colours[start];
        let mut end = start + 1;
        while end + 1 < points.len() && colours[end] == colour {
            end += 1;
        }
        painter.add(egui::Shape::line(
            points[start..=end].to_vec(),
            Stroke::new(theme::stroke::HAIRLINE, colour),
        ));
        start = end;
    }
    let spacing = points
        .windows(2)
        .next()
        .map_or(0.0, |p| f64::from((p[1].x - p[0].x) * ppp));
    if spacing >= DOTS_FROM {
        for (p, c) in points.iter().zip(&colours) {
            painter.circle_filled(*p, theme::size::SAMPLE_DOT, *c);
        }
    }
}

/// The ruler in `strip`: bar numbers on top with the grid's bar and beat
/// ticks, the file's time under them. The loop brace's band is under it.
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
    let mid = strip.top() + strip.height() / 2.0;
    // Bar numbers every 1, 2, 4… bars, by bar number, so the same bars keep
    // their labels while the view scrolls.
    let label_font = FontId::monospace(theme::size::SCALE);
    let every = bar_label_step(lines, time);
    for line in lines {
        let lx = x(line.at.as_quarters());
        match line.level {
            Level::Bar => {
                painter.vline(
                    lx,
                    strip.top() + theme::space::XS..=mid,
                    Stroke::new(theme::stroke::HAIRLINE, palette.grid_bar),
                );
                let bar = meter.bbt_at(line.at).bar;
                if bar.rem_euclid(every) == 0 {
                    let text = format!("{}", bar + 1);
                    let galley =
                        painter.layout_no_wrap(text, label_font.clone(), palette.text_secondary);
                    // A label that would be cut at the right edge is left out.
                    let at = pos2(
                        lx + theme::space::XXS + 1.0,
                        strip.top() + theme::space::XXS,
                    );
                    if at.x + galley.size().x <= strip.right() {
                        painter.galley(at, galley, palette.text_secondary);
                    }
                }
            }
            Level::Beat => {
                painter.vline(
                    lx,
                    mid - theme::size::BEAT_TICK..=mid,
                    Stroke::new(theme::stroke::HAIRLINE, palette.grid_beat),
                );
            }
            Level::Sub => {}
        }
    }
    // The file's own time, from its first frame, at a step that keeps the
    // labels apart: device-rate frames over the rate. The step comes from
    // the span at the strip's centre, so after a big warp drag the spans
    // beside it can run much faster and their labels would overprint: each
    // label is drawn only if it clears the last one drawn ([`thinned_labels`]).
    let candidates = time_label_candidates(file, time, strip, rate);
    // Every label keeps its tick, drawn or not.
    for (lx, _) in &candidates {
        painter.vline(
            *lx,
            strip.bottom() - theme::size::TIME_TICK..=strip.bottom(),
            Stroke::new(theme::stroke::HAIRLINE, palette.border_strong),
        );
    }
    // A label's width is asked for only once it clears the last one drawn, so
    // a compressed span lays out only the few labels it draws, and the galley
    // made for the width is the one drawn.
    let mut galleys: Vec<Option<Arc<egui::Galley>>> = vec![None; candidates.len()];
    let drawn = thinned_labels(
        candidates
            .iter()
            .map(|(lx, _)| lx + theme::space::XXS + 1.0),
        |i| {
            let galley = painter.layout_no_wrap(
                candidates[i].1.clone(),
                label_font.clone(),
                palette.text_hint,
            );
            let width = galley.size().x;
            galleys[i] = Some(galley);
            width
        },
        strip.right(),
        theme::space::XXS,
    );
    for i in drawn {
        let Some(galley) = galleys[i].take() else {
            continue;
        };
        let at = pos2(
            candidates[i].0 + theme::space::XXS + 1.0,
            strip.bottom() - theme::size::RULER_BAND - galley.size().y,
        );
        painter.galley(at, galley, palette.text_hint);
    }
    painter.hline(
        strip.x_range(),
        strip.bottom(),
        Stroke::new(theme::stroke::HAIRLINE, palette.border),
    );
}

/// How many bars a bar number stands for at this zoom: 1, 2, 4… so labels
/// keep `BAR_NUMBER_ROOM` apart.
fn bar_label_step(lines: &[Line], time: TimeView) -> i64 {
    let mut bars = lines.iter().filter(|l| l.level == Level::Bar);
    let (Some(a), Some(b)) = (bars.next(), bars.next()) else {
        return 1;
    };
    let per_bar = ((b.at - a.at).as_quarters() * time.zoom) as f32;
    let mut every = 1;
    while per_bar * (every as f32) < theme::size::BAR_NUMBER_ROOM && every < 1 << 20 {
        every *= 2;
    }
    every
}

/// The beat where device-rate `frame` of the file plays, and the gain of the
/// span it is in. The spans follow each other in frames as in time (the
/// file is mapped without a loop), so this is a binary search.
fn beat_and_gain(file: &WholeFile, frame: f64) -> Option<(f64, f32)> {
    let end =
        |s: &crate::arrangement::waves::Span| s.first_frame + (s.end - s.start) * s.frames_per_beat;
    let i = file.spans.partition_point(|s| end(s) <= frame);
    let s = file.spans.get(i).filter(|s| frame >= s.first_frame)?;
    Some((
        s.start + (frame - s.first_frame) / s.frames_per_beat,
        s.gain,
    ))
}

/// The beat where device-rate `frame` of the file plays.
pub(crate) fn beat_of_frame(file: &WholeFile, frame: f64) -> Option<f64> {
    beat_and_gain(file, frame).map(|(q, _)| q)
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

/// The file's time in `strip` before thinning: the left x of every label's
/// tick and its text, in time order. The step is the one for the span at the
/// strip's centre ([`time_step`]); a faster span elsewhere puts these
/// labels closer together, so [`label_fits`] decides which are drawn.
fn time_label_candidates(
    file: &WholeFile,
    time: TimeView,
    strip: Rect,
    rate: u32,
) -> Vec<(f32, String)> {
    let rate = f64::from(rate.max(1));
    let Some(step) = time_step(file, time, strip, rate) else {
        return Vec::new();
    };
    let from = time.beat_at(strip.left(), strip.left()).max(file.from);
    let to = time.beat_at(strip.left(), strip.right()).min(file.to);
    let (Some(a), Some(b)) = (file.frame_at(from), file.frame_at(to - 1e-9)) else {
        return Vec::new();
    };
    let (s0, s1) = (a / rate, b / rate);
    let mut labels = Vec::new();
    let mut k = (s0 / step).ceil() as i64;
    while (k as f64) * step <= s1 {
        let t = k as f64 * step;
        if let Some(q) = beat_of_frame(file, t * rate) {
            labels.push((time.x(strip.left(), q), seconds_text(t, step)));
        }
        k += 1;
    }
    labels
}

/// Whether a label whose left edge is `left` clears the right edge of the
/// last one drawn by `gap`. Labels never overprint, however much faster the
/// file runs at this part of the ruler.
fn label_fits(prev_right: Option<f32>, left: f32, gap: f32) -> bool {
    prev_right.is_none_or(|right| left >= right + gap)
}

/// The indices of the labels [`ruler`] draws, from their left edges in order:
/// a label is drawn when it clears the right edge of the last one drawn by
/// `gap` ([`label_fits`]) and its own right edge does not pass `strip_right`.
/// A label cut at the right edge is not drawn and does not move that edge.
/// `width` is asked for a label's width only once it clears the gap, so a
/// compressed span costs no layouts for the labels it drops.
fn thinned_labels(
    lefts: impl IntoIterator<Item = f32>,
    mut width: impl FnMut(usize) -> f32,
    strip_right: f32,
    gap: f32,
) -> Vec<usize> {
    let mut drawn = Vec::new();
    let mut last_right: Option<f32> = None;
    for (i, left) in lefts.into_iter().enumerate() {
        if !label_fits(last_right, left, gap) {
            continue;
        }
        let right = left + width(i);
        if right <= strip_right {
            last_right = Some(right);
            drawn.push(i);
        }
    }
    drawn
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

    /// A detected hit is one triangle wherever it is drawn: its base centred
    /// on the hit, its point on the hit and below the base, so it points down
    /// at the place it marks.
    #[test]
    fn a_detected_hit_points_down_at_its_place() {
        let (x, top) = (100.0, 20.0);
        let triangle = hit_triangle_points(x, top);
        assert_eq!(triangle.len(), 3, "a triangle");
        assert_eq!(triangle[2].x, x, "the point is where the hit is");
        assert_eq!(
            triangle[2].y,
            top + theme::size::WARP_TICK_H,
            "the point hangs below the base"
        );
        assert_eq!(triangle[0].y, top);
        assert_eq!(triangle[1].y, top);
        assert_eq!(triangle[0].x, x - theme::size::WARP_TICK_W / 2.0);
        assert_eq!(triangle[1].x, x + theme::size::WARP_TICK_W / 2.0);
    }

    /// On the wave the triangle hangs from inside the body's top edge and
    /// stays in the top band, clear of the wave's middle.
    #[test]
    fn a_hit_on_the_wave_sits_in_the_waves_top_band() {
        let body = Rect::from_min_size(pos2(0.0, 200.0), vec2(600.0, 300.0));
        let triangle = hit_triangle_points(50.0, wave_hit_top(body));
        assert!(triangle[0].y > body.top(), "just inside the top edge");
        assert!(
            triangle[2].y < body.center().y,
            "above the wave's middle: {triangle:?}"
        );
        assert!(
            triangle[2].y - body.top() <= 10.0,
            "a small mark at the top: {triangle:?}"
        );
    }

    /// The strength-to-opacity rule both places draw with: a harder hit shows
    /// more strongly, and a strength past full adds nothing more.
    #[test]
    fn a_harder_hit_shows_more_strongly() {
        let alpha = |strength: f32| hit_ink(strength).a();
        assert!(alpha(0.0) > 0, "a soft hit still shows");
        assert!(alpha(0.0) < alpha(0.5));
        assert!(alpha(0.5) < alpha(1.0));
        assert_eq!(alpha(1.0), alpha(2.0), "the strength is clamped");
    }

    #[test]
    fn the_first_label_always_fits_and_a_later_one_needs_its_gap() {
        assert!(label_fits(None, 500.0, 2.0));
        assert!(label_fits(Some(100.0), 102.0, 2.0), "exactly the gap");
        assert!(label_fits(Some(100.0), 140.0, 2.0));
        assert!(!label_fits(Some(100.0), 101.0, 2.0), "one point short");
        assert!(!label_fits(Some(100.0), 90.0, 2.0), "over the last one");
    }

    /// F4: after a warp marker is dragged far, the spans beside the one at
    /// the strip's centre run much faster, so the labels the centre's step
    /// chooses overprint there. Through [`thinned_labels`], the function the
    /// ruler itself uses, the drawn labels never overlap.
    #[test]
    fn file_time_labels_never_overprint_when_a_warp_speeds_a_span_up() {
        use crate::arrangement::waves::Span;
        use crate::audio_editor::source::Played;

        let span = |start: f64, end: f64, first_frame: f64, frames_per_beat: f64| Span {
            media: 1,
            start,
            end,
            first_frame,
            frames_per_beat,
            gain: 1.0,
        };
        // Four beats of file at 48,000 frames a beat (a second each), then
        // four at 4,800 (a tenth of a second each): the stretch a warp drag
        // of ten times the speed leaves. The step is chosen from the second
        // span, which the strip's centre is in.
        let file = WholeFile {
            spans: vec![
                span(0.0, 4.0, 0.0, 48_000.0),
                span(4.0, 8.0, 192_000.0, 4_800.0),
            ],
            from: 0.0,
            to: 8.0,
            loop_window: (0.0, 8.0),
            played: Played {
                first: (0.0, 8.0),
                second: None,
            },
            heard: (0.0, 8.0),
        };
        let time = TimeView {
            left: 0.0,
            zoom: 100.0,
        };
        let strip = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 40.0));
        let candidates = time_label_candidates(&file, time, strip, 48_000);
        assert!(candidates.len() > 8, "the fixture's labels: {candidates:?}");

        // The widths the ruler lays the labels out at.
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let width = |text: &str| {
            ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(
                        text.to_owned(),
                        FontId::monospace(theme::size::SCALE),
                        Color32::WHITE,
                    )
                    .size()
                    .x
            })
        };
        let gap = theme::space::XXS;
        let left = |lx: f32| lx + theme::space::XXS + 1.0;
        // The bug: taken as they come, some labels overlap.
        let unthinned: Vec<(f32, f32)> = candidates
            .iter()
            .map(|(lx, text)| (left(*lx), left(*lx) + width(text)))
            .collect();
        assert!(
            unthinned.windows(2).any(|pair| pair[1].0 < pair[0].1 + gap),
            "the fixture does not compress any labels: {unthinned:?}"
        );

        // The fix, as the ruler runs it: each drawn label clears the last one
        // drawn, and one cut at the right edge is left out.
        let widths: Vec<f32> = candidates.iter().map(|(_, text)| width(text)).collect();
        let drawn = thinned_labels(
            candidates.iter().map(|(lx, _)| left(*lx)),
            |i| widths[i],
            strip.right(),
            gap,
        );
        assert!(
            drawn.len() > 1 && drawn.len() < candidates.len(),
            "the thinning dropped nothing: {drawn:?} of {}",
            candidates.len()
        );
        let boxes: Vec<(f32, f32)> = drawn
            .iter()
            .map(|i| {
                let (at, right) = (left(candidates[*i].0), left(candidates[*i].0) + widths[*i]);
                assert!(right <= strip.right(), "one cut at the right edge is drawn");
                (at, right)
            })
            .collect();
        for pair in boxes.windows(2) {
            assert!(
                pair[1].0 >= pair[0].1 + gap,
                "{} overprints {}",
                pair[1].0,
                pair[0].1
            );
        }
    }
}
