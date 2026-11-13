//! The drawn faces of built-in devices (M8 slice B1): where a picture is the
//! control (the owner's BF C).
//!
//! A face keeps the card's chrome — its frame, power, preset field, footer and
//! the buttons under the face — and draws its device's own picture in the room
//! the knobs would take. Every control on a face is also in All parameters, so
//! nothing is only reachable by pointer (DESIGN.md §7k).

pub(super) mod eq;

use eframe::egui::{Color32, Painter, Rect, Stroke, StrokeKind};

use crate::theme::{self, personality};
use crate::widgets::raised;

/// The screen every face draws on: a deep tint, the same in both themes, its
/// top the device's signature colour, and a hairline of that colour round it
/// (DESIGN.md §7k). The one thing the faces share.
pub(super) fn screen(painter: &Painter, rect: Rect, hue: Color32) {
    let radius = f32::from(theme::radius::MEDIUM);
    let top = personality::SCREEN.lerp_to_gamma(hue, personality::SCREEN_TINT);
    raised::gradient(painter, rect, radius, top, personality::SCREEN);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(
            theme::stroke::HAIRLINE,
            hue.gamma_multiply(theme::alpha::INK_QUIET),
        ),
        StrokeKind::Inside,
    );
}
