//! A track's type, as a glyph drawn beside its name: a waveform for audio,
//! keys for MIDI, stacked bars for a group, a return arrow for a return.

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, vec2};
use lumen_project::model::TrackKind;

use crate::theme;

/// A 14 pt icon for the track type: a waveform for audio, keys for MIDI,
/// stacked bars for a group, a return arrow for a return.
pub fn draw_type_icon(painter: &Painter, center: Pos2, kind: TrackKind, color: Color32) {
    let stroke = Stroke::new(theme::stroke::GLYPH_WIDE, color);
    let at = |x: f32, y: f32| center + vec2(x, y);
    match kind {
        TrackKind::Audio => {
            for (x, h) in [(-5.0, 1.0), (-2.5, 3.5), (0.0, 6.0), (2.5, 2.5), (5.0, 1.0)] {
                painter.line_segment([at(x, -h), at(x, h)], stroke);
            }
        }
        TrackKind::Midi => {
            let r = Rect::from_center_size(center, vec2(12.0, 12.0));
            painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
            for x in [-2.0, 2.0] {
                painter.line_segment([at(x, -6.0), at(x, 1.5)], stroke);
            }
        }
        TrackKind::Group | TrackKind::Master => {
            for y in [-4.0, 0.0, 4.0] {
                painter.line_segment([at(-6.0, y), at(6.0, y)], stroke);
            }
        }
        TrackKind::Return => {
            painter.line_segment([at(5.0, -5.0), at(5.0, 1.0)], stroke);
            painter.line_segment([at(5.0, 1.0), at(-5.0, 1.0)], stroke);
            painter.line_segment([at(-5.0, 1.0), at(-2.0, -2.0)], stroke);
            painter.line_segment([at(-5.0, 1.0), at(-2.0, 4.0)], stroke);
        }
    }
}
