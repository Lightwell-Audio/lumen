//! A built-in's card as a set piece (M8, the owner's round 2, 2026-11-12;
//! DESIGN.md §7k): a header tinted with the device's hue, a deep screen with the
//! device's picture, a row of lit dials. What differs from device to device is
//! the device's own `chain/face` drawing; this draws the rest, at each step the
//! panel's height allows (whole, short, compact; the folded rail is
//! [`rail`]).

use std::hash::{Hash, Hasher};

use eframe::egui::{self, Color32, CornerRadius, Rect, Response, Shadow, Stroke, StrokeKind, Ui};
use lumen_devices::params::ParamInfo;

use super::{Face, Look, Out};
use crate::chain::face::{Drawing, Parts, Scene, Values};
use crate::chain::geometry::Step;
use crate::chain::strip::{self, Output};
use crate::theme::personality::card as c;
use crate::widgets::raised;
use crate::{motion, theme};

mod chrome;
mod controls;
mod glass;
mod handles;
mod parts;
mod plan;

use controls::live;
use plan::Plan;

/// What a built-in adds to its card: its hue and drawing, the values the drawing
/// reads, the step the panel's height asks for and what the device is hearing.
pub(in crate::chain) struct Personal<'a> {
    pub hue: Color32,
    pub drawing: &'static Drawing,
    pub slots: &'a [strip::Slot],
    pub table: &'static [ParamInfo],
    pub step: Step,
    /// What the device hears, 0 to 1 (0 while motion is reduced).
    pub level: f32,
    /// A drawing made of parts (Iris's bands), the one it has picked, the
    /// device's live analysis when it has one and the rate the project runs at.
    pub parts: Option<&'static Parts>,
    pub picked: usize,
    pub spectrum: Option<&'a [f32]>,
    pub rate: u32,
    /// The tempo at the playhead, for a drawing that keeps time with the song.
    pub bpm: f32,
}

/// A meter's peak (dB, both sides; silence is negative infinity) as a level, 0
/// to 1, over the range [`c::LEVEL_FLOOR_DB`] to 0.
pub(in crate::chain) fn level_of(peak: [f32; 2]) -> f32 {
    let db = peak[0].max(peak[1]);
    if db.is_finite() {
        ((db - c::LEVEL_FLOOR_DB) / -c::LEVEL_FLOOR_DB).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The level a screen shows: what the device hears, or nothing while motion is
/// reduced (the picture is then the values alone).
fn seen(level: f32, still: bool) -> f32 {
    if still { 0.0 } else { level }
}

/// How far the readouts of a changed preset have come back, 0 to 1: it starts
/// at 0 when the sound's name changes and rises over the base time.
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

/// A whole, short or compact built-in card in `rect`. Returns what it answers.
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

    // The device's power: off dims the screen and drains the hue to a cool grey
    // with a short fade, and lighting it again powers the screen up.
    let off = look.bypass || !look.running;
    let power = motion::fade(&ctx, id.with("lit"), !off);
    let hue = live(personal.hue, power);

    glass::tint(&painter, plan.header, hue);
    let power_button = chrome::power(ui, plan.power, id, look, hue);
    chrome::titles(&painter, &plan, look, off);
    let field = chrome::preset(ui, &plan, id, look, hue);

    // The screen: its ground, then what the handles show, then the picture.
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
    if let Some((from, words)) = &hot.chip {
        handles::chip(&painter, &plan, *from, words, hue);
    }
    if matches!(look.face, Face::Ready)
        && look.parameters > 0
        && chrome::all_link(ui, &plan, id, look.parameters)
    {
        asked.push(Output::All);
    }

    // The edge: chosen is a 1.5 pt accent line, otherwise the strong edge, and
    // Ember washes a card under the hand.
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
        sidechain: None,
        outputs: None,
    }
}

/// A folded built-in: the hue's bar glowing with the level, the name set on end
/// and the power button. Returns the power button's response.
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
