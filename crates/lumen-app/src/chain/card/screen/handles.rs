//! On-screen handles: where the picture is the idea, the picture is the control
//! (drag Prism's beam spread). A handle moves one parameter, as its dial does,
//! through the same edit: the value is `Output::Set`, so a drag is one undo
//! step. Shift is fine adjustment, Alt moves freely (no snapping to a stepped
//! parameter's steps), double-click is the default, the arrow keys move it, and
//! it is a Tab stop with an accessible name. The pointer over it shows the
//! handle's cursor and lights it; while it is dragged a chip says the value.

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

/// What the handles showed this frame.
#[derive(Default)]
pub(super) struct Hot {
    /// How lit each handle is, for the drawing.
    pub lit: [f32; MAX_HANDLES],
    /// The live chip while one is dragged: where it hangs from, and its words.
    pub chip: Option<(Pos2, String)>,
}

/// Registers the handles of `drawing` on the picture, applies what they were
/// given and says what to show.
pub(super) fn interact(
    ui: &mut Ui,
    plan: &Plan,
    drawing: &Drawing,
    values: &Values<'_>,
    id: egui::Id,
    out: &mut Vec<Output>,
) -> Hot {
    let mut hot = Hot::default();
    // The compact card's picture is a band too thin to grab a tip on: the dials
    // under it are what is touched there.
    if plan.step == crate::chain::geometry::Step::Compact {
        return hot;
    }
    for (index, handle) in drawing.handles.iter().enumerate().take(MAX_HANDLES) {
        let Some(slot) = values.slots.iter().find(|s| s.id == handle.param) else {
            continue;
        };
        let grab = (handle.grab)(values, plan.picture);
        if grab.width() <= 0.0 || grab.height() <= 0.0 {
            continue;
        }
        let id = id.with(("handle", index));
        let sense = if slot.read_only {
            Sense::hover()
        } else {
            Sense::click_and_drag()
        };
        let response = ui.interact(grab, id, sense);
        let name = format!("{} ({}): {}", handle.name, slot.name, slot.text);
        response.widget_info(|| egui::WidgetInfo::slider(!slot.read_only, slot.value, &name));
        let active = response.hovered() || response.dragged() || response.has_focus();
        hot.lit[index] = motion::hover(ui.ctx(), id.with("lit"), active);
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(handle.cursor);
        }
        if slot.read_only {
            continue;
        }
        let next = moved(ui, &response, id, handle, slot, values, plan.picture);
        if let Some(value) = next.filter(|v| (v - slot.value).abs() > 1e-9) {
            out.push(Output::Set(slot.id, value));
        }
        if response.dragged() {
            let shown = next.unwrap_or(slot.value);
            hot.chip = Some((grab.center(), (handle.chip)(values, shown)));
        }
        response.on_hover_text(format!(
            "{} ({}). Drag, Shift for fine, double-click for the default",
            handle.name, slot.name
        ));
    }
    hot
}

/// The value a handle's pointer and keys ask for this frame, if any.
fn moved(
    ui: &Ui,
    response: &egui::Response,
    id: egui::Id,
    handle: &crate::chain::face::Handle,
    slot: &Slot,
    values: &Values<'_>,
    picture: Rect,
) -> Option<f64> {
    let (fine, free) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    let drag = id.with("drag");
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
        let delta =
            (handle.value_at)(values, pointer, picture) - (handle.value_at)(values, last, picture);
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
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp),
            )
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

/// The live value chip, hung above `from` and kept on the screen.
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
