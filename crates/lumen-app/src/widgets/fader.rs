//! The channel fader (M4 slice 5, the owner's S1 console): a thin groove
//! with a cap, on the send knobs' taper (`knob::position`: 0 dB at three
//! quarters of the travel, +6 dB at the top, silence at the bottom), so a
//! fader and a send at the same level sit at the same place.
//!
//! It acts as every value control does (DESIGN.md §7): drag (Shift for
//! fine), double-click for 0 dB, arrow keys when focused, the wheel a dB a
//! notch (Shift a tenth; `wheel.rs`). Its value is typed in the field
//! under it on the strip.

use eframe::egui::{self, CursorIcon, Id as EguiId, Key, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::db_text;
use super::knob::{level, position};
use crate::{motion, theme};

/// The groove's width and the cap's size.
const GROOVE: f32 = 4.0;
pub const CAP: egui::Vec2 = vec2(22.0, 12.0);

/// Where `db` sits in `travel` (top is loudest).
pub fn y_of(travel: egui::Rangef, db: f32) -> f32 {
    travel.max - position(db) * travel.span()
}

/// A fader along `travel` (vertical) at `x`; the cap moves within it.
/// Returns a new level in dB (`NEG_INFINITY` for silence) when the user
/// changed it.
pub fn fader(
    ui: &mut Ui,
    (x, travel): (f32, egui::Rangef),
    id: EguiId,
    db: f32,
    name: &str,
    look: theme::Controls,
) -> Option<f32> {
    let palette = *theme::p();
    let area = Rect::from_x_y_ranges(
        x - CAP.x / 2.0..=x + CAP.x / 2.0,
        travel.min - CAP.y / 2.0..=travel.max + CAP.y / 2.0,
    );
    let response = ui
        .interact(area, id, Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeVertical)
        .on_hover_text("Volume: drag or wheel (Shift for fine), double-click for 0 dB");
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            true,
            f64::from(position(db)),
            format!("{name}: {}", db_text(db.max(-60.0))),
        )
    });
    let fine = ui.input(|i| i.modifiers.shift);
    let mut at = position(db);
    let before = at;
    // As the send knob: a drag keeps its own place on the travel, so slow
    // moves up from silence add up.
    let drag_id = id.with("drag");
    if response.dragged()
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
    {
        let (mut along, last) = ui
            .data(|d| d.get_temp::<(f32, egui::Pos2)>(drag_id))
            .unwrap_or_else(|| {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
                (position(db), origin)
            });
        let dy = pointer.y - last.y;
        along = (along - dy / travel.span() * if fine { 0.1 } else { 1.0 }).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(drag_id, (along, pointer)));
        at = along;
    } else {
        ui.data_mut(|d| d.remove::<(f32, egui::Pos2)>(drag_id));
    }
    if response.clicked() {
        response.request_focus();
    }
    if response.has_focus() {
        let (down, up) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowRight),
            )
        });
        let step = if fine { 0.005 } else { 0.03 };
        if down {
            at -= step;
        }
        if up {
            at += step;
        }
    }
    let at = at.clamp(0.0, 1.0);
    let mut out = (at != before).then(|| level(at));
    // The wheel: a dB a notch, from −60 dB at the bottom.
    let notches = super::wheel::steps(ui, id, area);
    if notches != 0.0 {
        let step = notches * if fine { 0.1 } else { 1.0 };
        let from = if db.is_finite() { db } else { -60.0 };
        out = Some((from + step).clamp(-60.0, super::knob::MAX_DB));
    }
    if response.double_clicked() {
        out = Some(0.0);
    }

    // Drawing: the groove, then the cap at the level shown.
    let painter = ui.painter();
    let groove = Rect::from_x_y_ranges(x - GROOVE / 2.0..=x + GROOVE / 2.0, travel);
    painter.rect_filled(groove, GROOVE / 2.0, palette.well);
    let shown = out.unwrap_or(db);
    let hover = motion::hover(
        ui.ctx(),
        id.with("hover"),
        response.hovered() || response.dragged(),
    );
    let cap = Rect::from_center_size(pos2(x, y_of(travel, shown)), CAP)
        .round_to_pixels(ui.ctx().pixels_per_point());
    let fill = if response.dragged() {
        look.press
    } else {
        look.hover.lerp_to_gamma(look.press, hover)
    };
    painter.rect_filled(cap, theme::radius::SMALL, fill);
    painter.rect_stroke(
        cap,
        theme::radius::SMALL,
        Stroke::new(1.0, palette.border_strong),
        egui::StrokeKind::Inside,
    );
    painter.hline(
        cap.x_range().shrink(4.0),
        cap.center().y,
        Stroke::new(1.0, palette.text),
    );
    if response.has_focus() {
        painter.rect_stroke(
            cap.expand(2.0),
            theme::radius::SMALL,
            Stroke::new(2.0, palette.accent),
            egui::StrokeKind::Outside,
        );
    }
    out
}

use eframe::emath::GuiRounding as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fader_shares_the_send_knobs_taper() {
        let travel = egui::Rangef::new(0.0, 100.0);
        // 0 dB three quarters up, +6 at the top, silence at the bottom.
        assert_eq!(y_of(travel, 0.0), 25.0);
        assert_eq!(y_of(travel, 6.0), 0.0);
        assert_eq!(y_of(travel, f32::NEG_INFINITY), 100.0);
    }
}
