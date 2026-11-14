//! A built-in card's dials and pills: Lumen's own controls (drag, Shift for
//! fine, the wheel, double-click for the default, click and type, the arrow
//! keys, the right-click menu with automation and MIDI learn), lit in the
//! device's hue, under the idea name the device gives them.

use eframe::egui::{self, Align, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2};

use super::chrome::on_baseline;
use super::plan::Plan;
use crate::chain::face::{Control, Drawing, Kind, Parts, Values};
use crate::chain::strip::{Output, Slot};
use crate::theme::personality::card as c;
use crate::widgets::param::{DialLook, ParamLook, param_dial, start_typing};
use crate::widgets::raised;
use crate::{motion, theme, widgets};

/// The ink of the names and values under the dials: the card's on the whole
/// and the compact card, the screen's white on the short one.
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
            Self::card()
        }
    }

    /// The card's own ink, for what sits on the card and not on the screen.
    pub(super) fn card() -> Self {
        let palette = theme::p();
        Self {
            name: palette.text_secondary,
            value: palette.text,
        }
    }
}

/// What a control says to a screen reader and in tooltips: its idea name, and
/// the parameter's own beside it when they differ ("Ø L (Polarity L)").
fn said(idea: &str, parameter: &str) -> String {
    if idea.eq_ignore_ascii_case(parameter) || parameter.ends_with(idea) {
        parameter.to_owned()
    } else {
        format!("{idea} ({parameter})")
    }
}

/// What a dial is called under it: the idea name, and a shorter one for the
/// short and compact cards.
#[derive(Clone, Copy)]
struct Idea {
    name: &'static str,
    short: &'static str,
    shown: fn(&str) -> String,
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
    picked: (Option<&Parts>, usize, &Values<'_>),
    out: &mut Vec<Output>,
) {
    let ink = Ink::of(plan);
    let (parts, picked, values) = picked;
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
                // On a drawing with parts a dial is the picked part's, in its
                // colour: the parameter and the name are the part's, and a part
                // with nothing for it (a notch has no gain) leaves it still.
                let aim = match parts {
                    Some(p) => Some((p.aim)(values, picked, index)),
                    // A dial that stands for another parameter by the values.
                    None => control.follows.map(|f| Some(f(values))),
                };
                let colour = match parts {
                    Some(p) => live((p.colour)(picked), power),
                    None => control.lit.map_or(hue, |own| live(own, power)),
                };
                let idea = Idea {
                    name: control.name,
                    short: control.short.unwrap_or(control.name),
                    shown: control.shown,
                };
                match aim {
                    None => dial(ui, plan, idea, slot, cell, id, colour, ink, out),
                    Some(Some(aim)) => {
                        let Some(aimed) = slots.iter().find(|s| s.id == aim.param) else {
                            continue;
                        };
                        let idea = Idea {
                            name: aim.name,
                            short: control.short.unwrap_or(aim.name),
                            shown: control.shown,
                        };
                        // A dial that follows a parameter is a new control when
                        // the parameter changes: a drag in progress ends there.
                        let id = if control.follows.is_some() {
                            id.with(aim.param)
                        } else {
                            id
                        };
                        dial(ui, plan, idea, aimed, cell, id, colour, ink, out);
                    }
                    Some(None) => {
                        let still = Slot {
                            text: "\u{2014}".to_owned(),
                            read_only: true,
                            ..slot.clone()
                        };
                        dial(ui, plan, idea, &still, cell, id, colour, ink, out);
                    }
                }
            }
            // A pill sits on the card, under the screen, at every step.
            Kind::Pill => pill(ui, (control, values), slot, cell, id, hue, Ink::card(), out),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dial(
    ui: &mut Ui,
    plan: &Plan,
    idea: Idea,
    slot: &Slot,
    cell: Rect,
    id: egui::Id,
    hue: Color32,
    ink: Ink,
    out: &mut Vec<Output>,
) {
    let name = said(idea.name, &slot.name);
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
        let labels = plan.labels;
        let name_font = FontId::proportional(labels.name);
        let shown_name = match plan.step {
            crate::chain::geometry::Step::Whole => idea.name,
            _ => idea.short,
        };
        let idea_text = widgets::fit(painter, shown_name, name_font.clone(), labels.room);
        on_baseline(
            painter,
            pos2(x, foot + labels.name_under),
            Align::Center,
            idea_text,
            name_font,
            ink.name,
            0.0,
        );
        let value_font = FontId::monospace(labels.value);
        let value = widgets::fit(
            painter,
            &(idea.shown)(&slot.text),
            value_font.clone(),
            labels.room,
        );
        on_baseline(
            painter,
            pos2(x, foot + labels.value_under),
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
        automation_rows(ui, slot, out);
    });
}

/// The menu's last rows: Automate and Learn where the parameter allows them,
/// and the reason where it does not (a device's timing), as the strip's menu
/// says it.
fn automation_rows(ui: &mut Ui, slot: &Slot, out: &mut Vec<Output>) {
    if slot.automatable {
        if let Some(ask) = widgets::control_items(ui, slot.mapped, slot.waiting) {
            out.push(Output::Ask(slot.id, ask));
        }
    } else {
        widgets::menu_caption(ui, "Not automatable: it is the device's timing.");
    }
}

/// A pill: a two-state switch under an idea name ("Ø L"), lit in the device's
/// hue while it is on. A click flips it.
#[allow(clippy::too_many_arguments)]
fn pill(
    ui: &mut Ui,
    (control, values): (&Control, &Values<'_>),
    slot: &Slot,
    rect: Rect,
    id: egui::Id,
    hue: Color32,
    ink: Ink,
    out: &mut Vec<Output>,
) {
    let on = control
        .switch
        .map_or(slot.value >= 0.5, |s| (s.on)(slot.value));
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
        let turned_on = control.switch.map_or(1.0, |s| (s.turn_on)(values));
        out.push(Output::Choose(slot.id, if on { 0.0 } else { turned_on }));
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
        automation_rows(ui, slot, out);
    });
}
