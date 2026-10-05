//! Dense views painted directly with egui's painter: viewport culling plus
//! level of detail (items narrower than a few pixels merge into spans).

use eframe::egui::{
    Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2,
};

use crate::data::{MAX_NOTE_LEN, Note, Peaks, SAMPLE_RATE, SECONDS_PER_BEAT, Track};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x1B, 0x1D, 0x22);
const ROW_ALT: Color32 = Color32::from_rgb(0x20, 0x22, 0x28);
const GRID: Color32 = Color32::from_rgb(0x2C, 0x2F, 0x37);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub left_beat: f32,
    pub px_per_beat: f32,
    pub top_px: f32,
    pub row_h: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct DrawStats {
    pub items: usize,
    pub merged_spans: usize,
    pub labels: usize,
    pub wave_columns: usize,
}

fn grid(painter: &Painter, rect: Rect, view: &View) {
    // Bars every 4 beats; thin out until lines are at least 8 px apart.
    let mut step = 4.0;
    while step * view.px_per_beat < 8.0 {
        step *= 2.0;
    }
    let mut beat = (view.left_beat / step).floor() * step;
    while beat < view.left_beat + rect.width() / view.px_per_beat {
        let x = rect.left() + (beat - view.left_beat) * view.px_per_beat;
        painter.vline(x, rect.y_range(), Stroke::new(1.0, GRID));
        beat += step;
    }
}

pub fn arrangement(
    painter: &Painter,
    rect: Rect,
    tracks: &[Track],
    peaks: &Peaks,
    view: &View,
) -> DrawStats {
    let mut stats = DrawStats::default();
    painter.rect_filled(rect, 0.0, BACKGROUND);
    grid(painter, rect, view);
    let first_track = (view.top_px / view.row_h).floor().max(0.0) as usize;
    let last_track =
        (((view.top_px + rect.height()) / view.row_h).ceil() as usize).min(tracks.len());
    let view_start = view.left_beat;
    let view_end = view.left_beat + rect.width() / view.px_per_beat;
    let x_at = |beat: f32| rect.left() + (beat - view.left_beat) * view.px_per_beat;
    let samples_per_px = (SECONDS_PER_BEAT * SAMPLE_RATE / view.px_per_beat).max(1.0) as usize;
    let mut waves = Mesh::default();

    for (index, track) in tracks.iter().enumerate().take(last_track).skip(first_track) {
        let y0 = rect.top() + index as f32 * view.row_h - view.top_px;
        let lane = Rect::from_x_y_ranges(rect.x_range(), y0 + 1.0..=y0 + view.row_h - 1.0);
        if index % 2 == 1 {
            painter.rect_filled(lane, 0.0, ROW_ALT);
        }
        let first = track
            .clips
            .partition_point(|c| c.start + c.len < view_start);
        let mut span: Option<(f32, f32)> = None;
        let flush = |span: &mut Option<(f32, f32)>, stats: &mut DrawStats| {
            if let Some((a, b)) = span.take() {
                let r = Rect::from_x_y_ranges(a..=b.max(a + 1.0), lane.y_range());
                painter.rect_filled(r, 0.0, track.color.gamma_multiply(0.6));
                stats.merged_spans += 1;
            }
        };
        for clip in &track.clips[first..] {
            if clip.start > view_end {
                break;
            }
            let (x0, x1) = (x_at(clip.start), x_at(clip.start + clip.len));
            if x1 - x0 < 3.0 {
                match &mut span {
                    Some((_, end)) if x0 <= *end + 1.0 => *end = end.max(x1),
                    _ => {
                        flush(&mut span, &mut stats);
                        span = Some((x0, x1));
                    }
                }
                continue;
            }
            flush(&mut span, &mut stats);
            stats.items += 1;
            let body = Rect::from_x_y_ranges(
                x0.max(rect.left() - 4.0)..=x1.min(rect.right() + 4.0),
                lane.y_range(),
            );
            painter.rect_filled(body, 3.0, track.color.gamma_multiply(0.28));
            painter.rect_stroke(body, 3.0, Stroke::new(1.0, track.color), StrokeKind::Inside);
            let label_h = if view.row_h >= 30.0 && body.width() > 40.0 {
                painter.text(
                    body.left_top() + vec2(5.0, 2.0),
                    Align2::LEFT_TOP,
                    &clip.name,
                    FontId::proportional(11.0),
                    TEXT,
                );
                stats.labels += 1;
                14.0
            } else {
                0.0
            };
            if track.audio && body.width() > 8.0 && view.row_h >= 24.0 {
                let center = (body.top() + label_h + body.bottom()) / 2.0;
                let half = (body.bottom() - body.top() - label_h) / 2.0 - 2.0;
                let mut x = body.left().max(rect.left()).floor();
                while x < body.right().min(rect.right()) {
                    let beat = view.left_beat + (x - rect.left()) / view.px_per_beat - clip.start;
                    let sample =
                        clip.wave_offset + (beat * SECONDS_PER_BEAT * SAMPLE_RATE) as usize;
                    let (lo, hi) = peaks.range(sample, samples_per_px);
                    waves.add_colored_rect(
                        Rect::from_min_max(
                            pos2(x, center - hi * half),
                            pos2(x + 1.0, center - lo * half + 1.0),
                        ),
                        track.color,
                    );
                    stats.wave_columns += 1;
                    x += 1.0;
                }
            }
        }
        flush(&mut span, &mut stats);
    }
    painter.add(Shape::mesh(waves));
    stats
}

pub fn piano_roll(painter: &Painter, rect: Rect, notes: &[Note], view: &View) -> DrawStats {
    let mut stats = DrawStats::default();
    painter.rect_filled(rect, 0.0, BACKGROUND);
    let top_row = (view.top_px / view.row_h).floor().max(0.0) as i32;
    let rows = (rect.height() / view.row_h).ceil() as i32 + 1;
    let pitch_y =
        |pitch: u8| rect.top() + (127 - i32::from(pitch)) as f32 * view.row_h - view.top_px;
    for row in top_row..top_row + rows {
        let pitch = 127 - row;
        if matches!(pitch.rem_euclid(12), 1 | 3 | 6 | 8 | 10) {
            let y = rect.top() + row as f32 * view.row_h - view.top_px;
            painter.rect_filled(
                Rect::from_x_y_ranges(rect.x_range(), y..=y + view.row_h),
                0.0,
                ROW_ALT,
            );
        }
    }
    grid(painter, rect, view);
    let view_start = view.left_beat;
    let view_end = view.left_beat + rect.width() / view.px_per_beat;
    let visible_pitches = (127 - (top_row + rows)).max(0)..=(127 - top_row).min(127);
    let x_at = |beat: f32| rect.left() + (beat - view.left_beat) * view.px_per_beat;
    let first = notes.partition_point(|n| n.start + MAX_NOTE_LEN < view_start);
    let mut spans: [Option<(f32, f32)>; 128] = [None; 128];
    let flush = |pitch: u8, span: (f32, f32), stats: &mut DrawStats| {
        let y = pitch_y(pitch);
        let r = Rect::from_x_y_ranges(
            span.0..=span.1.max(span.0 + 1.0),
            y + 1.0..=y + view.row_h - 1.0,
        );
        painter.rect_filled(r, 0.0, Color32::from_rgb(0x5B, 0x9B, 0xE6));
        stats.merged_spans += 1;
    };
    for note in &notes[first..] {
        if note.start > view_end {
            break;
        }
        if !visible_pitches.contains(&i32::from(note.pitch)) || note.start + note.len < view_start {
            continue;
        }
        let (x0, x1) = (x_at(note.start), x_at(note.start + note.len));
        let slot = &mut spans[note.pitch as usize];
        if x1 - x0 < 2.0 {
            match slot {
                Some((_, end)) if x0 <= *end + 1.0 => *end = end.max(x1),
                _ => {
                    if let Some(span) = slot.take() {
                        flush(note.pitch, span, &mut stats);
                    }
                    *slot = Some((x0, x1));
                }
            }
            continue;
        }
        stats.items += 1;
        let y = pitch_y(note.pitch);
        let shade = 0.45 + f32::from(note.velocity) / 127.0 * 0.55;
        let fill = Color32::from_rgb(0x5B, 0x9B, 0xE6).gamma_multiply(shade);
        let r = Rect::from_min_max(Pos2::new(x0, y + 1.0), Pos2::new(x1, y + view.row_h - 1.0));
        painter.rect_filled(r, 2.0, fill);
    }
    for (pitch, span) in spans.iter().enumerate() {
        if let Some(span) = span {
            flush(pitch as u8, *span, &mut stats);
        }
    }
    stats
}
