//! A drawing's parts (Iris's eight bands): the points on the screen, which are
//! the control where the picture is the idea, and the numbered pills on the
//! card, which pick one. The points are dragged, wheeled and double-clicked,
//! kept by the keys and Tab stops with a name; every edit is the one a dial
//! makes (`Output`s), so a drag is one undo step. Which parameters a point
//! moves is the drawing's own (`face::Parts`); how is the same for every
//! device with parts.

use eframe::egui::{
    self, Align, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use super::chrome::on_baseline;
use super::controls::live;
use super::plan::Plan;
use crate::chain::face::{Drawing, MAX_PARTS, Parts, Values};
use crate::chain::geometry::Step;
use crate::chain::strip::Output;
use crate::theme::personality::card as c;
use crate::widgets::raised;
use crate::{motion, theme, widgets};

/// What the parts showed and asked this frame.
#[derive(Default)]
pub(super) struct Hot {
    /// How lit each point is, for the drawing.
    pub lit: [f32; MAX_PARTS],
    /// A part picked this frame (by a click, a drag, a key or its pill).
    pub picked: Option<usize>,
}

/// The parts' points, registered on the picture: each applies what it was
/// given. The picked part is registered last, so it is on top where points
/// overlap.
#[allow(clippy::too_many_arguments)]
pub(super) fn points(
    ui: &mut Ui,
    plan: &Plan,
    drawing: &Drawing,
    parts: &Parts,
    values: &Values<'_>,
    picked: usize,
    card: egui::Id,
    out: &mut Vec<Output>,
) -> Hot {
    let mut hot = Hot::default();
    let count = parts.count.min(MAX_PARTS);
    let picture = plan.picture;
    let painter = ui.painter().with_clip_rect(picture.expand(2.0));
    // The compact card's picture is a band too thin to drag in: a point picks.
    let thin = plan.step == Step::Compact;
    let (fine, _free) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    let order = (0..count)
        .filter(|index| *index != picked)
        .chain((picked < count).then_some(picked));
    for index in order {
        let Some(at) = (parts.at)(values, index, picture) else {
            continue;
        };
        let reach = Rect::from_center_size(at, Vec2::splat(2.0 * c::PART_REACH)).intersect(picture);
        if reach.width() <= 0.0 || reach.height() <= 0.0 {
            continue;
        }
        let id = card.with(("part", index));
        let response = ui.interact(reach, id, Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, (parts.words)(values, index))
        });
        let active = response.hovered() || response.dragged() || response.has_focus();
        hot.lit[index] = motion::hover(ui.ctx(), id.with("lit"), active);
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(if thin {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Grab
            });
        }
        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if response.has_focus() {
            raised::paint_focus(&painter, reach, theme::radius::FULL);
        }
        if response.clicked() {
            response.request_focus();
            hot.picked = Some(index);
        }
        // A point reached by Tab is the picked one, so the dials follow it.
        if response.gained_focus() {
            hot.picked = Some(index);
        }
        if response.double_clicked() {
            let values = (parts.reset_point)(values, index);
            if !values.is_empty() {
                out.push(Output::Reset(values));
            }
            hot.picked = Some(index);
        }

        // A drag moves the point, as far as the pointer goes (a tenth as far
        // with Shift): where it is now is what the parameters become.
        let drag = id.with("drag");
        if response.dragged()
            && !thin
            && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
        {
            let (centre, last) = ui
                .data(|d| d.get_temp::<(Pos2, Pos2)>(drag))
                .unwrap_or_else(|| {
                    let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
                    (at, origin)
                });
            let centre = centre + (pointer - last) * if fine { c::PART_FINE } else { 1.0 };
            ui.data_mut(|d| d.insert_temp(drag, (centre, pointer)));
            moved(out, values, (parts.drag)(values, index, centre, picture));
            hot.picked = Some(index);
        } else {
            ui.data_mut(|d| d.remove::<(Pos2, Pos2)>(drag));
        }

        // The wheel is the part's width (its Q).
        let notches = widgets::wheel::steps(ui, id.with("wheel"), reach);
        if notches != 0.0 && !thin {
            if let Some((param, value)) = (parts.wheel)(values, index, notches, fine) {
                out.push(Output::Set(param, value));
            }
            hot.picked = Some(index);
        }

        // The keyboard: arrows move it, Page Up and Down are its width, Enter
        // types its frequency into the dial that takes one.
        if response.has_focus() && !thin {
            let (dx, dy, page, enter) = ui.input(|i| {
                let key = |k: Key| i32::from(i.key_pressed(k)) as f32;
                (
                    key(Key::ArrowRight) - key(Key::ArrowLeft),
                    key(Key::ArrowUp) - key(Key::ArrowDown),
                    key(Key::PageUp) - key(Key::PageDown),
                    i.key_pressed(Key::Enter) || i.key_pressed(Key::F2),
                )
            });
            if dx != 0.0 || dy != 0.0 {
                moved(
                    out,
                    values,
                    (parts.nudge)(values, index, vec2(dx, dy), fine),
                );
            }
            if page != 0.0
                && let Some((param, value)) = (parts.wheel)(values, index, page, fine)
            {
                out.push(Output::Set(param, value));
            }
            if enter {
                type_into(ui, drawing, parts, values, index, card);
            }
            hot.picked = Some(index);
        }

        // Right-click: the part's own actions.
        let switch = (parts.switch)(index);
        let on = (parts.is_on)(values, index);
        widgets::Menu::context(&response).show(|ui| {
            if widgets::menu_item(ui, "Reset this band", None, true).clicked() {
                out.push(Output::Reset((parts.reset_all)(values, index)));
                ui.close();
            }
            let words = if on {
                "Turn this band off"
            } else {
                "Turn this band on"
            };
            if widgets::menu_item(ui, words, None, true).clicked() {
                out.push(Output::Choose(switch, if on { 0.0 } else { 1.0 }));
                ui.close();
            }
            if let Some(aim) = (parts.aim)(values, index, parts.typed)
                && let Some(ask) = {
                    ui.separator();
                    widgets::control_items(ui, None, false)
                }
            {
                out.push(Output::Ask(aim.param, ask));
            }
        });
        response.on_hover_ui(|ui| {
            ui.label(format!(
                "{}. {}",
                (parts.words)(values, index),
                if thin {
                    "Click to pick it"
                } else {
                    "Drag to move it (Shift is fine), double-click to reset it, the wheel sets its Q"
                }
            ));
        });
    }
    hot
}

/// What a point asked for as one move: a gesture of one undo step, said only
/// when it changes something.
fn moved(out: &mut Vec<Output>, values: &Values<'_>, next: Vec<(u32, f64)>) {
    if next
        .iter()
        .any(|(param, value)| (values.norm(*param) as f64 - value).abs() > 1e-6)
    {
        out.push(Output::Moved(next));
    }
}

/// Opens the typing field of the dial that takes a part's frequency, with the
/// text it holds for that part. A dial's id is made from the card's.
fn type_into(
    ui: &Ui,
    drawing: &Drawing,
    parts: &Parts,
    values: &Values<'_>,
    index: usize,
    card: egui::Id,
) {
    let Some(control) = drawing.controls.get(parts.typed) else {
        return;
    };
    let Some(aim) = (parts.aim)(values, index, parts.typed) else {
        return;
    };
    let Some(slot) = values.slots.iter().find(|s| s.id == aim.param) else {
        return;
    };
    widgets::param::start_typing(ui, card.with(("control", control.param)), &slot.text);
}

/// The numbered pills under the screen: one for each part, in its colour, the
/// picked one filled; a click picks it and a double-click turns the part on or
/// off.
#[allow(clippy::too_many_arguments)]
pub(super) fn pills(
    ui: &mut Ui,
    plan: &Plan,
    parts: &Parts,
    values: &Values<'_>,
    picked: usize,
    id: egui::Id,
    power: f32,
    out: &mut Vec<Output>,
    picked_out: &mut Option<usize>,
) {
    let palette = *theme::p();
    for index in 0..parts.count.min(MAX_PARTS) {
        let Some(rect) = plan.pill_cell(parts, index) else {
            continue;
        };
        let id = id.with(("pill", index));
        let response = ui.interact(rect, id, Sense::click());
        let on = (parts.is_on)(values, index);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                true,
                on,
                (parts.pill_words)(values, index),
            )
        });
        let colour = live((parts.colour)(index), power);
        let lit = motion::hover(ui.ctx(), id.with("lit"), index == picked);
        let hot = motion::hover(ui.ctx(), id.with("hot"), response.hovered());
        let painter = ui.painter();
        let radius = rect.height() / 2.0;
        painter.rect_filled(
            rect,
            radius,
            palette
                .text_secondary
                .gamma_multiply(c::PART_PILL_REST)
                .lerp_to_gamma(colour.gamma_multiply(c::PART_PILL_PICKED), lit),
        );
        painter.rect_filled(
            rect,
            radius,
            palette
                .text
                .gamma_multiply(crate::light::EMBER_WASH * hot * theme::alpha::EDGE),
        );
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(
                theme::stroke::HAIRLINE,
                if on {
                    colour
                } else {
                    colour.gamma_multiply(c::PART_PILL_OFF)
                },
            ),
            StrokeKind::Inside,
        );
        let size = rect.height() * c::PART_PILL_TEXT_SHARE;
        on_baseline(
            painter,
            pos2(rect.center().x, rect.center().y + size * 0.36),
            Align::Center,
            (parts.number)(index),
            if on || index == picked {
                crate::fonts::medium(size)
            } else {
                FontId::proportional(size)
            },
            if on {
                palette.text
            } else {
                palette.text.gamma_multiply(c::PART_PILL_OFF_TEXT)
            },
            0.0,
        );
        raised::focus_ring(ui.painter(), &response, rect);
        if response.double_clicked() {
            out.push(Output::Choose(
                (parts.switch)(index),
                if on { 0.0 } else { 1.0 },
            ));
            *picked_out = Some(index);
        } else if response.clicked() {
            *picked_out = Some(index);
        }
        response.on_hover_ui(|ui| {
            ui.label(format!(
                "{}. Click to pick it; double-click to turn it {}",
                (parts.pill_words)(values, index),
                if on { "off" } else { "on" }
            ));
        });
    }
}
