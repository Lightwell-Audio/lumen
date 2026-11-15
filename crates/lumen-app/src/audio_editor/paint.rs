use std::sync::Arc;

use eframe::egui::{self, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use lumen_core::grid::{Level, Line};
use lumen_core::meter::MeterMap;
use lumen_media::Peaks;
use lumen_project::model::Clip;

use super::source::WholeFile;
use crate::theme;
use crate::timeview::TimeView;

pub(crate) const SAMPLES_BELOW: f64 = 4.0;
const DOTS_FROM: f64 = 6.0;
pub(crate) const SPLIT_FROM: f32 = theme::size::WAVE_SPLIT;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WaveLook {
    pub bright: Color32,
    pub dim: Color32,
}

pub(crate) struct Samples<'a> {
    pub first: i64,
    pub frames: &'a [[i16; 2]],
}

pub(crate) struct WaveInput<'a> {
    pub file: &'a WholeFile,
    pub peaks: &'a Peaks,
    pub clip: &'a Clip,
    pub time: TimeView,
    pub look: WaveLook,
    pub split: bool,
    pub samples: Option<Samples<'a>>,
    pub grid: &'a [(f32, lumen_core::grid::Level)],
}

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

pub(crate) fn frames_per_pixel(file: &WholeFile, time: TimeView, q: f64, ppp: f32) -> Option<f64> {
    let span = file.span_at(q)?;
    Some(span.frames_per_beat / (time.zoom * f64::from(ppp)))
}

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

pub(crate) fn wave(painter: &Painter, lane: Rect, input: &WaveInput<'_>) {
    let painter = painter.with_clip_rect(lane.intersect(painter.clip_rect()));
    let palette = theme::p();
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

pub(crate) fn hit_triangle(painter: &Painter, x: f32, top: f32, strength: f32) {
    painter.add(egui::Shape::convex_polygon(
        hit_triangle_points(x, top),
        hit_ink(strength),
        Stroke::NONE,
    ));
}

fn hit_triangle_points(x: f32, top: f32) -> Vec<Pos2> {
    let (w, h) = (theme::size::WARP_TICK_W, theme::size::WARP_TICK_H);
    vec![
        pos2(x - w / 2.0, top),
        pos2(x + w / 2.0, top),
        pos2(x, top + h),
    ]
}

fn hit_ink(strength: f32) -> Color32 {
    theme::p()
        .text_secondary
        .gamma_multiply(theme::WARP_TICK_BASE + theme::WARP_TICK_GAIN * strength.clamp(0.0, 1.0))
}

pub(crate) fn wave_hit_top(body: Rect) -> f32 {
    body.top() + theme::size::WARP_TICK_FLOOR
}

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
    let label_font = FontId::monospace(theme::size::SCALE);
    let every = bar_label_step(lines, meter, time);
    let mut numbers = Vec::new();
    let mut covered: Vec<(f32, f32)> = Vec::new();
    for line in lines.iter().filter(|l| l.level == Level::Bar) {
        let bar = meter.bbt_at(line.at).bar;
        if bar.rem_euclid(every) != 0 {
            continue;
        }
        let lx = x(line.at.as_quarters());
        let galley = painter.layout_no_wrap(
            format!("{}", bar + 1),
            label_font.clone(),
            palette.text_secondary,
        );
        let at = pos2(
            lx + theme::space::XXS + 1.0,
            strip.top() + theme::space::XXS,
        );
        if at.x + galley.size().x <= strip.right() {
            covered.push((at.x, at.x + galley.size().x + theme::space::XXS));
            numbers.push((at, galley));
        }
    }
    for line in lines {
        let lx = x(line.at.as_quarters());
        if inside_any(lx, &covered) {
            continue;
        }
        match line.level {
            Level::Bar => {
                painter.vline(
                    lx,
                    strip.top() + theme::space::XS..=mid,
                    Stroke::new(theme::stroke::HAIRLINE, palette.grid_bar),
                );
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
    for (at, galley) in numbers {
        painter.galley(at, galley, palette.text_secondary);
    }
    let candidates = time_label_candidates(file, time, strip, rate);
    for (lx, _) in &candidates {
        painter.vline(
            *lx,
            strip.bottom() - theme::size::TIME_TICK..=strip.bottom(),
            Stroke::new(theme::stroke::HAIRLINE, palette.border_strong),
        );
    }
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

fn inside_any(x: f32, spans: &[(f32, f32)]) -> bool {
    spans.iter().any(|&(left, right)| x >= left && x <= right)
}

fn bar_label_step(lines: &[Line], meter: &MeterMap, time: TimeView) -> i64 {
    let mut bars = lines.iter().filter(|l| l.level == Level::Bar);
    let (Some(a), Some(b)) = (bars.next(), bars.next()) else {
        return 1;
    };
    let between = (meter.bbt_at(b.at).bar - meter.bbt_at(a.at).bar).max(1);
    let per_line = ((b.at - a.at).as_quarters() * time.zoom) as f32;
    bars_per_label(per_line / between as f32)
}

fn bars_per_label(px_per_bar: f32) -> i64 {
    let mut every = 1;
    while px_per_bar * (every as f32) < theme::size::BAR_NUMBER_ROOM && every < 1 << 20 {
        every *= 2;
    }
    every
}

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

pub(crate) fn beat_of_frame(file: &WholeFile, frame: f64) -> Option<f64> {
    beat_and_gain(file, frame).map(|(q, _)| q)
}

fn time_step(file: &WholeFile, time: TimeView, strip: Rect, rate: f64) -> Option<f64> {
    let q = time.beat_at(strip.left(), strip.center().x);
    let span = file.span_at(q).or_else(|| file.spans.first())?;
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

fn label_fits(prev_right: Option<f32>, left: f32, gap: f32) -> bool {
    prev_right.is_none_or(|right| left >= right + gap)
}

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

    #[test]
    fn a_harder_hit_shows_more_strongly() {
        let alpha = |strength: f32| hit_ink(strength).a();
        assert!(alpha(0.0) > 0, "a soft hit still shows");
        assert!(alpha(0.0) < alpha(0.5));
        assert!(alpha(0.5) < alpha(1.0));
        assert_eq!(alpha(1.0), alpha(2.0), "the strength is clamped");
    }

    #[test]
    fn bar_numbers_keep_their_room_however_narrow_a_bar_is() {
        assert_eq!(bars_per_label(9.0), 4);
        assert_eq!(bars_per_label(16.0), 2);
        assert_eq!(bars_per_label(32.0), 1);
        assert_eq!(bars_per_label(200.0), 1);
        assert_eq!(bars_per_label(0.0), 1 << 20, "a zero width ends the loop");
        for px in [3.0_f32, 5.0, 9.0, 11.0, 17.0, 31.0] {
            let every = bars_per_label(px);
            assert!(px * every as f32 >= theme::size::BAR_NUMBER_ROOM, "{px}");
        }
    }

    #[test]
    fn the_step_counts_the_bars_between_two_grid_lines() {
        use lumen_core::meter::Meter;
        use lumen_core::time::Ticks;

        let meter = MeterMap::constant(Meter::FOUR_FOUR).unwrap();
        let line = |bar: i64| Line {
            at: Ticks::quarters(bar * 4),
            level: Level::Bar,
        };
        let time = TimeView {
            left: 0.0,
            zoom: 2.25,
        };
        assert_eq!(
            bar_label_step(&[line(0), line(2), line(4)], &meter, time),
            4
        );
        assert_eq!(
            bar_label_step(&[line(0), line(1), line(2)], &meter, time),
            4
        );
        assert_eq!(bar_label_step(&[line(0)], &meter, time), 1, "one line");
    }

    #[test]
    fn a_tick_inside_a_bar_number_is_left_out() {
        let spans = [(13.0, 27.0), (61.0, 75.0)];
        assert!(!inside_any(10.0, &spans), "the number's own tick");
        assert!(inside_any(13.0, &spans));
        assert!(inside_any(20.0, &spans));
        assert!(inside_any(27.0, &spans));
        assert!(!inside_any(28.0, &spans));
        assert!(inside_any(70.0, &spans));
        assert!(!inside_any(20.0, &[]));
    }

    #[test]
    fn the_first_label_always_fits_and_a_later_one_needs_its_gap() {
        assert!(label_fits(None, 500.0, 2.0));
        assert!(label_fits(Some(100.0), 102.0, 2.0), "exactly the gap");
        assert!(label_fits(Some(100.0), 140.0, 2.0));
        assert!(!label_fits(Some(100.0), 101.0, 2.0), "one point short");
        assert!(!label_fits(Some(100.0), 90.0, 2.0), "over the last one");
    }

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
        let unthinned: Vec<(f32, f32)> = candidates
            .iter()
            .map(|(lx, text)| (left(*lx), left(*lx) + width(text)))
            .collect();
        assert!(
            unthinned.windows(2).any(|pair| pair[1].0 < pair[0].1 + gap),
            "the fixture does not compress any labels: {unthinned:?}"
        );

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
