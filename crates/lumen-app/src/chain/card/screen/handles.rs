use eframe::egui::{
    self, Align, Color32, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use super::chrome::on_baseline;
use super::plan::Plan;
use crate::chain::face::{Drawing, MAX_HANDLES, Values};
use crate::chain::strip::{Output, Slot};
use crate::theme::personality::card as c;
use crate::widgets::param::{snap, step_by};
use crate::{motion, theme};

#[derive(Default)]
pub(super) struct Hot {
    pub lit: [f32; MAX_HANDLES],
    pub chip: Option<(Pos2, String)>,
}

pub(super) fn interact(
    ui: &mut Ui,
    plan: &Plan,
    drawing: &Drawing,
    values: &Values<'_>,
    id: egui::Id,
    out: &mut Vec<Output>,
) -> Hot {
    let mut hot = Hot::default();
    if plan.step == crate::chain::geometry::Step::Compact {
        return hot;
    }
    for (index, handle) in drawing.handles.iter().enumerate().take(MAX_HANDLES) {
        let param = handle.follows.map_or(handle.param, |f| f(values));
        let Some(slot) = values.slots.iter().find(|s| s.id == param) else {
            continue;
        };
        let also = handle.also.as_ref().and_then(|a| {
            values
                .slots
                .iter()
                .find(|s| s.id == a.param)
                .map(|s| (a, s))
        });
        let grab = (handle.grab)(values, plan.picture);
        if grab.width() <= 0.0 || grab.height() <= 0.0 {
            continue;
        }
        let id = id.with(("handle", index, param));
        let read_only = slot.read_only || also.is_some_and(|(_, other)| other.read_only);
        let sense = if read_only {
            Sense::hover()
        } else {
            Sense::click_and_drag()
        };
        let response = ui.interact(grab, id, sense);
        let name = match also {
            Some((_, other)) => format!(
                "{} ({} and {}): {}, {}",
                handle.name, slot.name, other.name, slot.text, other.text
            ),
            None => format!("{} ({}): {}", handle.name, slot.name, slot.text),
        };
        response.widget_info(|| egui::WidgetInfo::slider(!read_only, slot.value, &name));
        let active = response.hovered() || response.dragged() || response.has_focus();
        hot.lit[index] = motion::hover(ui.ctx(), id.with("lit"), active);
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(handle.cursor);
        }
        if read_only {
            continue;
        }
        let keys = if also.is_some() {
            Keys::Across
        } else {
            Keys::Both
        };
        let next = moved(
            ui,
            &response,
            id.with("drag"),
            (handle.value_at, slot, keys),
            values,
            plan.picture,
        );
        let changed =
            |next: Option<f64>, slot: &Slot| next.filter(|v| (v - slot.value).abs() > 1e-9);
        match also {
            None => {
                if let Some(value) = changed(next, slot) {
                    out.push(Output::Set(slot.id, value));
                }
            }
            Some((other, other_slot)) => {
                let second = moved(
                    ui,
                    &response,
                    id.with("drag again"),
                    (other.value_at, other_slot, Keys::Up),
                    values,
                    plan.picture,
                );
                if response.double_clicked() {
                    out.push(Output::Reset(vec![
                        (slot.id, snap(slot.default, slot.steps)),
                        (other_slot.id, snap(other_slot.default, other_slot.steps)),
                    ]));
                } else if changed(next, slot).is_some() || changed(second, other_slot).is_some() {
                    out.push(Output::Moved(vec![
                        (slot.id, next.unwrap_or(slot.value)),
                        (other_slot.id, second.unwrap_or(other_slot.value)),
                    ]));
                }
            }
        }
        if response.dragged() {
            let shown = next.unwrap_or(slot.value);
            let from = if also.is_some() {
                ui.input(|i| i.pointer.interact_pos())
                    .unwrap_or(grab.center())
            } else {
                grab.center()
            };
            hot.chip = Some((from, (handle.chip)(values, shown)));
        }
        let tip = match also {
            Some((_, other)) => format!(
                "{} ({} across, {} up and down). Drag, Shift for fine, double-click for the defaults",
                handle.name, slot.name, other.name
            ),
            None => format!(
                "{} ({}). Drag, Shift for fine, double-click for the default",
                handle.name, slot.name
            ),
        };
        response.on_hover_text(tip);
    }
    hot
}

type ValueAt = fn(&Values<'_>, Pos2, Rect) -> f64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keys {
    Both,
    Across,
    Up,
}

fn moved(
    ui: &Ui,
    response: &egui::Response,
    drag: egui::Id,
    (value_at, slot, keys): (ValueAt, &Slot, Keys),
    values: &Values<'_>,
    picture: Rect,
) -> Option<f64> {
    let (fine, free) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    let mut next = None;
    if response.dragged()
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
    {
        let (travel, last) = ui
            .data(|d| d.get_temp::<(f64, Pos2)>(drag))
            .unwrap_or_else(|| {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
                (slot.value.clamp(0.0, 1.0), origin)
            });
        let delta = value_at(values, pointer, picture) - value_at(values, last, picture);
        let travel = (travel + delta * if fine { 0.1 } else { 1.0 }).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(drag, (travel, pointer)));
        next = Some(if free {
            travel
        } else {
            snap(travel, slot.steps)
        });
    } else {
        ui.data_mut(|d| d.remove::<(f64, Pos2)>(drag));
    }
    if response.clicked() {
        response.request_focus();
    }
    if response.has_focus() {
        let (down, up) = ui.input(|i| {
            let (left, right) = (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
            );
            let (below, above) = (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp));
            match keys {
                Keys::Both => (left || below, right || above),
                Keys::Across => (left, right),
                Keys::Up => (below, above),
            }
        });
        let base = next.unwrap_or(slot.value);
        if down {
            next = Some(step_by(base, -1.0, fine, slot.steps));
        }
        if up {
            next = Some(step_by(base, 1.0, fine, slot.steps));
        }
    }
    if response.double_clicked() {
        next = Some(snap(slot.default, slot.steps));
    }
    next
}

pub(super) fn chip(painter: &egui::Painter, plan: &Plan, from: Pos2, words: &str, hue: Color32) {
    let font = egui::FontId::monospace(c::CHIP_TEXT);
    let galley = painter.layout_no_wrap(words.to_owned(), font, Color32::WHITE);
    let size = vec2(galley.size().x + 14.0, c::CHIP_HEIGHT);
    let mut rect = Rect::from_center_size(pos2(from.x, from.y - 18.0), size);
    let bounds = plan.screen.shrink(4.0);
    if rect.left() < bounds.left() {
        rect = rect.translate(vec2(bounds.left() - rect.left(), 0.0));
    }
    if rect.right() > bounds.right() {
        rect = rect.translate(vec2(bounds.right() - rect.right(), 0.0));
    }
    if rect.top() < bounds.top() {
        rect = rect.translate(vec2(0.0, bounds.top() - rect.top()));
    }
    painter.rect_filled(rect, rect.height() / 2.0, theme::personality::SCREEN);
    painter.rect_stroke(
        rect,
        rect.height() / 2.0,
        Stroke::new(theme::stroke::HAIRLINE, hue),
        StrokeKind::Inside,
    );
    on_baseline(
        painter,
        pos2(rect.center().x, rect.center().y + c::CHIP_TEXT * 0.36),
        Align::Center,
        words.to_owned(),
        egui::FontId::monospace(c::CHIP_TEXT),
        Color32::WHITE,
        0.0,
    );
}
