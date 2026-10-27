//! The time signature as one field, "4 / 4" (the interface audit's S7): it
//! was a drag value and a combo box side by side. A piece of the transport
//! bar's numeric well (the interface audit's TB1): its digits in Martian
//! Mono, the Ember wash on the half under the pointer. Drag up or down, or turn
//! the wheel, over the top number to change the beats; over the bottom one
//! to change the beat unit. Click and type "7/8" (Enter applies, Esc
//! cancels); double-click for 4/4; arrow keys when focused.

use eframe::egui::{self, Align2, FontId, Id as EguiId, Key, Rect, Sense, Ui, pos2};
use eframe::emath::GuiRounding as _;

use super::{raised, well, wheel};
use crate::{motion, theme};

/// The beat units a signature can have.
pub const UNITS: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];
/// Points of drag for one step.
const DRAG_STEP: f32 = 12.0;
/// The field's width: room for "12 / 16".
pub const WIDTH: f32 = 56.0;

/// Reads "7/8", "7 / 8", "7 8" or "7" (keeping `unit`); `None` when it is
/// not a signature Lumen has.
pub fn parse(text: &str, unit: u32, max_beats: u32) -> Option<(u32, u32)> {
    let mut parts = text
        .split(|c: char| c == '/' || c.is_whitespace())
        .filter(|p| !p.is_empty());
    let beats: u32 = parts.next()?.parse().ok()?;
    let unit = match parts.next() {
        Some(u) => u.parse().ok()?,
        None => unit,
    };
    if parts.next().is_some() || !(1..=max_beats).contains(&beats) || !UNITS.contains(&unit) {
        return None;
    }
    Some((beats, unit))
}

/// The time signature field, in `rect` (a piece of a well the caller has
/// painted); returns a new signature when it changed.
pub fn time_signature(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    (beats, unit): (u32, u32),
    max_beats: u32,
) -> Option<(u32, u32)> {
    let palette = theme::p();
    let font = FontId::monospace(theme::size::BODY);
    let typing_id = id.with("typing");
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let radius = well::piece_radius(rect.height());
    if let Some(mut text) = ui.data(|d| d.get_temp::<String>(typing_id)) {
        // Typed in place: the piece keeps its size, the well is its ground.
        let mut field = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let edit = field.add_sized(
            rect.size(),
            egui::TextEdit::singleline(&mut text)
                .id(id.with("edit"))
                .font(font)
                .frame(egui::Frame::NONE)
                .vertical_align(egui::Align::Center)
                .horizontal_align(egui::Align::Center),
        );
        raised::paint_focus(ui.painter(), rect, radius);
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if cancel || edit.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            if !cancel {
                return parse(&text, unit, max_beats).filter(|&s| s != (beats, unit));
            }
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
        return None;
    }

    let response = ui.interact(rect, id, Sense::click_and_drag());
    let response = super::tip(
        response,
        "Time signature",
        None,
        Some("Drag or use the wheel over a number. Click to type 7/8; double-click for 4/4."),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::DragValue,
            true,
            format!("Time signature: {beats}/{unit}"),
        )
    });
    let mut out = (beats, unit);
    let pointer = response.hover_pos().or(response.interact_pointer_pos());
    let over_unit = pointer.is_some_and(|p| p.x > rect.center().x);
    let step_unit = |unit: u32, by: i32| {
        let i = UNITS.iter().position(|&u| u == unit).unwrap_or(2) as i32;
        UNITS[(i + by).clamp(0, UNITS.len() as i32 - 1) as usize]
    };
    let step_beats = |beats: u32, by: i32| (beats as i32 + by).clamp(1, max_beats as i32) as u32;

    // Drag: up adds, down takes away, a step every few points.
    let travel_id = id.with("travel");
    if response.drag_started() {
        ui.data_mut(|d| d.insert_temp(travel_id, 0.0_f32));
    }
    if response.dragged() {
        let mut travel: f32 = ui.data(|d| d.get_temp(travel_id)).unwrap_or(0.0);
        travel -= response.drag_delta().y;
        travel += response.drag_delta().x;
        let steps = (travel / DRAG_STEP).trunc();
        if steps != 0.0 {
            travel -= steps * DRAG_STEP;
            if over_unit {
                out.1 = step_unit(out.1, steps as i32);
            } else {
                out.0 = step_beats(out.0, steps as i32);
            }
        }
        ui.data_mut(|d| d.insert_temp(travel_id, travel));
    }
    let notches = wheel::steps(ui, id, rect);
    if notches != 0.0 {
        let by = notches.signum() as i32;
        if over_unit {
            out.1 = step_unit(out.1, by);
        } else {
            out.0 = step_beats(out.0, by);
        }
    }
    if response.double_clicked() {
        out = (4, 4);
    } else if response.clicked() {
        ui.data_mut(|d| d.insert_temp(typing_id, format!("{beats}/{unit}")));
    }
    if response.has_focus() {
        let (up, down) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
        if up {
            out.0 = step_beats(out.0, 1);
        }
        if down {
            out.0 = step_beats(out.0, -1);
        }
    }

    if ui.is_rect_visible(rect) {
        let hover = motion::hover(
            ui.ctx(),
            id.with("hover"),
            response.hovered() || response.dragged(),
        );
        let controls = theme::controls(false);
        let painter = ui.painter();
        // Ember on the half the wheel or a drag would change.
        if hover > 0.0 {
            let half = if over_unit {
                Rect::from_min_max(pos2(rect.center().x, rect.top()), rect.max)
            } else {
                Rect::from_min_max(rect.min, pos2(rect.center().x, rect.bottom()))
            };
            painter.with_clip_rect(half).rect_filled(
                rect,
                f32::from(radius),
                palette.ember.gamma_multiply(hover),
            );
        }
        // The half the wheel or a drag would change brightens.
        let lit = |right: bool| hover > 0.0 && over_unit == right;
        let colour = |right: bool| {
            if lit(right) {
                palette.text
            } else {
                controls.text.lerp_to_gamma(palette.text, 0.6)
            }
        };
        let mid = rect.center();
        painter.text(
            pos2(mid.x - theme::space::XS - 1.0, mid.y),
            Align2::RIGHT_CENTER,
            out.0.to_string(),
            font.clone(),
            colour(false),
        );
        painter.text(
            mid,
            Align2::CENTER_CENTER,
            "/",
            font.clone(),
            palette.text_hint,
        );
        painter.text(
            pos2(mid.x + theme::space::XS + 1.0, mid.y),
            Align2::LEFT_CENTER,
            out.1.to_string(),
            font,
            colour(true),
        );
        if response.has_focus() {
            raised::paint_focus(painter, rect, radius);
        }
    }
    (out != (beats, unit)).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_signatures_are_understood() {
        assert_eq!(parse("7/8", 4, 64), Some((7, 8)));
        assert_eq!(parse(" 6 / 8 ", 4, 64), Some((6, 8)));
        assert_eq!(parse("5 4", 4, 64), Some((5, 4)));
        assert_eq!(parse("3", 8, 64), Some((3, 8)), "the unit stays");
        assert_eq!(parse("7/7", 4, 64), None, "no such unit");
        assert_eq!(parse("0/4", 4, 64), None);
        assert_eq!(parse("65/4", 4, 64), None, "past the most beats");
        assert_eq!(parse("four", 4, 64), None);
    }
}
