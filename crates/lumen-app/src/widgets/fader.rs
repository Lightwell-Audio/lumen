use eframe::egui::{self, CursorIcon, Id as EguiId, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;

use super::db_text;
use super::knob::{level, position};
use super::raised;
use crate::{motion, theme};

const GROOVE: f32 = 4.0;
const GROOVE_RADIUS: u8 = 2;
const INDEX: f32 = 2.0;
const INDEX_INSET: f32 = 4.0;
pub const CAP: egui::Vec2 = vec2(22.0, 12.0);

pub fn y_of(travel: egui::Rangef, db: f32) -> f32 {
    travel.max - position(db) * travel.span()
}

pub fn fader(
    ui: &mut Ui,
    (x, travel): (f32, egui::Rangef),
    id: EguiId,
    db: f32,
    name: &str,
    look: theme::Controls,
) -> Option<f32> {
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
    let notches = super::wheel::steps(ui, id, area);
    if notches != 0.0 {
        let step = notches * if fine { 0.1 } else { 1.0 };
        let from = if db.is_finite() { db } else { -60.0 };
        out = Some((from + step).clamp(-60.0, super::knob::MAX_DB));
    }
    if response.double_clicked() {
        out = Some(0.0);
    }

    let painter = ui.painter();
    let groove = Rect::from_x_y_ranges(x - GROOVE / 2.0..=x + GROOVE / 2.0, travel);
    raised::paint_sunken(painter, groove, GROOVE_RADIUS);
    let shown = out.unwrap_or(db);
    let hover = motion::hover(
        ui.ctx(),
        id.with("hover"),
        response.hovered() || response.dragged(),
    );
    let cap = Rect::from_center_size(pos2(x, y_of(travel, shown)), CAP)
        .round_to_pixels(ui.ctx().pixels_per_point());
    let radius = theme::radius::for_height(cap.height());
    let face = raised::Face::new(look)
        .hover(hover)
        .pressed(response.dragged());
    let body = raised::paint(painter, cap, radius, &face);
    painter.hline(
        body.x_range().shrink(INDEX_INSET),
        body.center().y,
        Stroke::new(INDEX, theme::p().text),
    );
    if response.has_focus() {
        raised::paint_focus(painter, cap, radius);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fader_shares_the_send_knobs_taper() {
        let travel = egui::Rangef::new(0.0, 100.0);
        assert_eq!(y_of(travel, 0.0), 25.0);
        assert_eq!(y_of(travel, 6.0), 0.0);
        assert_eq!(y_of(travel, f32::NEG_INFINITY), 100.0);
    }
}
