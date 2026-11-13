//! A built-in card's dials and pills: Lumen's own controls (drag, Shift for
//! fine, the wheel, double-click for the default, click and type, the arrow
//! keys, the right-click menu with automation and MIDI learn), lit in the
//! device's hue, under the idea name the device gives them.

use eframe::egui::{self, Align, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2};

use super::chrome::on_baseline;
use super::plan::Plan;
use crate::chain::face::{Control, Drawing, Kind};
use crate::chain::strip::{Output, Slot};
use crate::theme::personality::card as c;
use crate::widgets::param::{DialLook, ParamLook, param_dial, start_typing};
use crate::widgets::raised;
use crate::{motion, theme, widgets};

/// How wide a dial's name and value may be.
const LABEL_ROOM: f32 = 56.0;

/// The ink of the names and values under the dials: the card's on the whole
/// card, the screen's white on the short one.
#[derive(Clone, Copy)]
pub(super) struct Ink {
    pub name: Color32,
    pub value: Color32,
}

impl Ink {
    pub(super) fn of(plan: &Plan) -> Self {
        if plan.on_screen() {
            Self {
                name: Color32::WHITE.gamma_multiply(c::SCREEN_NAME_ALPHA),
                value: Color32::WHITE,
            }
        } else {
            let palette = theme::p();
            Self {
                name: palette.text_secondary,
                value: palette.text,
            }
        }
    }
}

/// What a control says to a screen reader and in tooltips: its idea name, and
/// the parameter's own beside it when they differ ("Ø L (Polarity L)").
fn said(idea: &str, parameter: &str) -> String {
    if idea.eq_ignore_ascii_case(parameter) {
        parameter.to_owned()
    } else {
        format!("{idea} ({parameter})")
    }
}

/// `colour` as the screen shows it at `power` (0 to 1): its own when lit, a cool
/// grey when off.
pub(super) fn live(colour: Color32, power: f32) -> Color32 {
    theme::grey_of(colour).lerp_to_gamma(colour, power)
}

/// Paints every control of `drawing` at this step and collects what they ask
/// for.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    ui: &mut Ui,
    plan: &Plan,
    drawing: &Drawing,
    slots: &[Slot],
    id: egui::Id,
    hue: Color32,
    power: f32,
    out: &mut Vec<Output>,
) {
    let ink = Ink::of(plan);
    for (index, control) in drawing.controls.iter().enumerate() {
        let Some(slot) = slots.iter().find(|s| s.id == control.param) else {
            continue;
        };
        let Some(cell) = plan.cell(drawing, index) else {
            continue;
        };
        let id = id.with(("control", control.param));
        match control.kind {
            Kind::Knob => {
                let colour = control.lit.map_or(hue, |own| live(own, power));
                dial(ui, plan, control, slot, cell, id, colour, ink, out);
            }
            Kind::Pill => pill(ui, control, slot, cell, id, hue, ink, out),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dial(
    ui: &mut Ui,
    plan: &Plan,
    control: &Control,
    slot: &Slot,
    cell: Rect,
    id: egui::Id,
    hue: Color32,
    ink: Ink,
    out: &mut Vec<Output>,
) {
    let name = said(control.name, &slot.name);
    let (response, moved) = param_dial(
        ui,
        cell.center(),
        id,
        ParamLook {
            name: &name,
            text: &slot.text,
            value: slot.value,
            default: slot.default,
            steps: slot.steps,
            read_only: slot.read_only,
            selected: false,
        },
        DialLook {
            hue,
            radius: plan.dial_radius,
        },
    );
    if let Some(value) = moved.value {
        out.push(Output::Set(slot.id, value));
    }
    if let Some(text) = moved.typed {
        out.push(Output::Type(slot.id, text));
    }
    if slot.waiting {
        crate::arrangement::follow::ring_round_with(ui, cell, theme::p().accent);
    }
    if slot.over {
        crate::arrangement::follow::ring_round(ui, cell);
    }
    if plan.has_labels() {
        let painter = ui.painter();
        let x = cell.center().x;
        let foot = cell.center().y + plan.dial_radius;
        let name_font = FontId::proportional(c::DIAL_NAME_SIZE);
        let idea = widgets::fit(painter, control.name, name_font.clone(), LABEL_ROOM);
        on_baseline(
            painter,
            pos2(x, foot + c::DIAL_NAME_BASELINE),
            Align::Center,
            idea,
            name_font,
            ink.name,
            0.0,
        );
        let value_font = FontId::monospace(c::DIAL_VALUE_SIZE);
        let value = widgets::fit(
            painter,
            &(control.shown)(&slot.text),
            value_font.clone(),
            LABEL_ROOM,
        );
        on_baseline(
            painter,
            pos2(x, foot + c::DIAL_VALUE_BASELINE),
            Align::Center,
            value,
            value_font,
            ink.value,
            0.0,
        );
    }
    let (value, default, param, read_only) = (slot.value, slot.default, slot.id, slot.read_only);
    widgets::Menu::context(&response).show(|ui| {
        if widgets::menu_item(ui, "Type a value\u{2026}", None, !read_only).clicked() {
            start_typing(ui, id, &slot.text);
            ui.close();
        }
        if widgets::menu_item(ui, "Reset to default", None, !read_only).clicked() {
            if (value - default).abs() > 1e-9 {
                out.push(Output::Set(param, default));
            }
            ui.close();
        }
        ui.separator();
        if let Some(ask) = widgets::control_items(ui, slot.mapped, slot.waiting) {
            out.push(Output::Ask(param, ask));
        }
    });
}

/// A pill: a two-state switch under an idea name ("Ø L"), lit in the device's
/// hue while it is on. A click flips it.
#[allow(clippy::too_many_arguments)]
fn pill(
    ui: &mut Ui,
    control: &Control,
    slot: &Slot,
    rect: Rect,
    id: egui::Id,
    hue: Color32,
    ink: Ink,
    out: &mut Vec<Output>,
) {
    let on = slot.value >= 0.5;
    let response = ui.interact(rect, id, Sense::click());
    let name = said(control.name, &slot.name);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, !slot.read_only, on, &name)
    });
    let lit = motion::hover(ui.ctx(), id.with("lit"), on);
    let hot = motion::hover(ui.ctx(), id.with("hot"), response.hovered());
    let painter = ui.painter();
    let radius = rect.height() / 2.0;
    painter.rect_filled(
        rect,
        radius,
        hue.gamma_multiply(c::PILL_OFF_FILL)
            .lerp_to_gamma(hue.gamma_multiply(c::PILL_ON_FILL), lit),
    );
    painter.rect_filled(
        rect,
        radius,
        ink.value
            .gamma_multiply(crate::light::EMBER_WASH * hot * theme::alpha::EDGE),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(
            theme::stroke::HAIRLINE,
            hue.gamma_multiply(c::PILL_OFF_RING).lerp_to_gamma(hue, lit),
        ),
        StrokeKind::Inside,
    );
    on_baseline(
        painter,
        pos2(rect.center().x, rect.center().y + c::PILL_TEXT * 0.36),
        Align::Center,
        control.name.to_owned(),
        if on {
            crate::fonts::medium(c::PILL_TEXT)
        } else {
            FontId::proportional(c::PILL_TEXT)
        },
        ink.value,
        0.0,
    );
    if slot.waiting {
        crate::arrangement::follow::ring_with(ui, rect, theme::p().accent);
    }
    if slot.over {
        crate::arrangement::follow::ring(ui, rect);
    }
    raised::focus_ring(ui.painter(), &response, rect);
    let response = response.on_hover_text(format!("{name}: {}", slot.text));
    if response.clicked() && !slot.read_only {
        out.push(Output::Choose(slot.id, if on { 0.0 } else { 1.0 }));
    }
    let (value, default, param, read_only) = (slot.value, slot.default, slot.id, slot.read_only);
    widgets::Menu::context(&response).show(|ui| {
        if widgets::menu_item(ui, "Reset to default", None, !read_only).clicked() {
            if (value - default).abs() > 1e-9 {
                out.push(Output::Choose(param, default));
            }
            ui.close();
        }
        ui.separator();
        if let Some(ask) = widgets::control_items(ui, slot.mapped, slot.waiting) {
            out.push(Output::Ask(param, ask));
        }
    });
}
