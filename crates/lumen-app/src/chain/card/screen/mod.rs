use std::hash::{Hash, Hasher};

use eframe::egui::{self, Color32, CornerRadius, Rect, Response, Shadow, Stroke, StrokeKind, Ui};
use lumen_devices::params::ParamInfo;

use super::{Face, Look, Out};
use crate::chain::face::{Drawing, Parts, Scene, Values, cadence};
use crate::chain::geometry::Step;
use crate::chain::strip::{self, Output};
use crate::theme::personality::card as c;
use crate::widgets::raised;
use crate::{motion, theme};

mod chrome;
mod controls;
mod glass;
mod handles;
mod key;
mod parts;
mod plan;

use controls::live;
pub(in crate::chain) use glass::glass as gradient_panel;
use plan::Plan;

pub(in crate::chain) struct Personal<'a> {
    pub hue: Color32,
    pub drawing: &'static Drawing,
    pub slots: &'a [strip::Slot],
    pub table: &'static [ParamInfo],
    pub step: Step,
    pub level: f32,
    pub parts: Option<&'static Parts>,
    pub picked: usize,
    pub spectrum: Option<&'a [f32]>,
    pub rate: u32,
    pub bpm: f32,
    pub reading: Option<lumen_devices::MeterReading>,
}

pub(in crate::chain) fn level_of(peak: [f32; 2]) -> f32 {
    let db = peak[0].max(peak[1]);
    if db.is_finite() {
        ((db - c::LEVEL_FLOOR_DB) / -c::LEVEL_FLOOR_DB).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn seen(level: f32, still: bool) -> f32 {
    if still { 0.0 } else { level }
}

fn settle(ctx: &egui::Context, id: egui::Id, look: &Look<'_>) -> f32 {
    let mut hasher = std::hash::DefaultHasher::new();
    look.field.as_ref().and_then(|f| f.name).hash(&mut hasher);
    let now_seen = hasher.finish();
    let key = id.with("preset seen");
    let (seen, since): (u64, f64) = ctx
        .data(|d| d.get_temp(key))
        .unwrap_or((now_seen, f64::NEG_INFINITY));
    let since = if seen == now_seen {
        since
    } else {
        ctx.input(|i| i.time)
    };
    ctx.data_mut(|d| d.insert_temp(key, (now_seen, since)));
    motion::appear(ctx, since)
}

pub(super) fn paint(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>, body: Response) -> Out {
    let Some(personal) = look.personal.as_ref() else {
        unreachable!("a screen card has its drawing");
    };
    let drawing = personal.drawing;
    let palette = *theme::p();
    let ctx = ui.ctx().clone();
    let plan = Plan::new(rect, drawing, personal.step);
    let painter = ui
        .painter()
        .with_clip_rect(rect.expand(theme::size::SHADOW_ROOM));
    let plan = plan.for_name(chrome::name_width(&painter, &plan, look.name));
    let hover = motion::hover(&ctx, id.with("card hover"), body.hovered());
    let radius = c::RADIUS;
    painter.add(
        Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: palette.shadow,
        }
        .as_shape(rect, CornerRadius::same(c::RADIUS as u8)),
    );
    painter.rect_filled(rect, radius, palette.raised);

    let off = look.bypass || !look.running;
    let power = motion::fade(&ctx, id.with("lit"), !off);
    let hue = live(personal.hue, power);

    glass::tint(&painter, plan.header, hue);
    let power_button = chrome::power(ui, plan.power, id, look, hue);
    chrome::titles(&painter, &plan, look, off);
    let field = chrome::preset(ui, &plan, id, look, hue);

    glass::screen(
        &painter,
        plan.screen,
        hue,
        drawing.deep,
        palette.raised,
        power,
    );
    painter.rect_stroke(
        plan.screen,
        c::SCREEN_RADIUS,
        Stroke::new(theme::stroke::HAIRLINE, hue.gamma_multiply(c::SCREEN_RING)),
        StrokeKind::Inside,
    );
    let values = Values {
        slots: personal.slots,
        table: personal.table,
    };
    let mut asked = Vec::new();
    let hot = handles::interact(ui, &plan, drawing, &values, id, &mut asked);
    let parts = personal.parts;
    let picked = parts.map_or(0, |p| personal.picked.min(p.count.saturating_sub(1)));
    let part_hot = parts.map_or_else(parts::Hot::default, |p| {
        parts::points(ui, &plan, drawing, p, &values, picked, id, &mut asked)
    });
    let still = motion::reduced();
    cadence::wake(&ctx, id, personal.level, still);
    let mist = motion::fade(&ctx, id.with("mist"), personal.spectrum.is_some() && !still);
    let picture = painter.with_clip_rect(plan.picture);
    (drawing.draw)(&Scene {
        painter: &picture,
        rect: plan.picture,
        step: plan.step,
        hue,
        values,
        power,
        level: seen(personal.level, still),
        still,
        handle_lit: hot.lit,
        settle: settle(&ctx, id, look),
        picked,
        parts_lit: part_hot.lit,
        spectrum: personal.spectrum,
        mist,
        rate: personal.rate,
        bpm: personal.bpm,
        reading: personal.reading.filter(|_| !still),
        id,
    });

    controls::paint(
        ui,
        &plan,
        drawing,
        personal.slots,
        id,
        hue,
        power,
        (parts, picked, &values),
        &mut asked,
    );
    let mut pill_picked = None;
    if let Some(p) = parts {
        parts::pills(
            ui,
            &plan,
            p,
            &values,
            picked,
            id,
            power,
            &mut asked,
            &mut pill_picked,
        );
    }
    let key = look
        .sidechain
        .filter(|_| matches!(look.face, Face::Ready))
        .map(|shown| key::chip(ui, &plan, id, shown, (hue, drawing.deep)));
    if let Some((from, words)) = &hot.chip {
        handles::chip(&painter, &plan, *from, words, hue);
    }
    if matches!(look.face, Face::Ready)
        && look.parameters > 0
        && chrome::all_link(ui, &plan, id, look.parameters)
    {
        asked.push(Output::All);
    }

    painter.rect_filled(
        rect,
        radius,
        palette
            .text
            .gamma_multiply(crate::light::EMBER_WASH * hover * theme::alpha::EDGE),
    );
    let edge = if look.selected {
        Stroke::new(theme::stroke::LIT, palette.accent)
    } else {
        Stroke::new(theme::stroke::HAIRLINE, palette.border_strong)
    };
    painter.rect_stroke(rect, radius, edge, StrokeKind::Inside);
    if body.has_focus() {
        raised::paint_focus(&painter, rect, c::RADIUS as u8);
    }
    Out {
        body,
        power: power_button,
        field: Some(field),
        name: None,
        step: None,
        picked: part_hot.picked.or(pill_picked),
        action: None,
        strip: asked,
        sidechain: key,
        outputs: None,
    }
}

pub(super) fn rail(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>) -> Response {
    let Some(personal) = look.personal.as_ref() else {
        unreachable!("a screen rail has its drawing");
    };
    let power = motion::fade(ui.ctx(), id.with("lit"), !look.bypass);
    let hue = live(personal.hue, power);
    let painter = ui
        .painter()
        .with_clip_rect(rect.expand(theme::size::SHADOW_ROOM));
    chrome::rail(&painter, rect, look, hue, personal.level, look.bypass);
    let centre = egui::pos2(rect.center().x, rect.top() + c::POWER_Y);
    chrome::power(ui, centre, id, look, hue)
}

#[cfg(test)]
mod tests;
