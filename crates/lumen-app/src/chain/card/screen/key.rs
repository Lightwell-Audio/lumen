//! The sidechain chip on a built-in's screen (Gravity: "Keyed by Kick · post"),
//! from the round-2 specimen. It is the card's sidechain chooser, drawn as the
//! screen's own chip rather than as a row: the same accessible name, tooltip,
//! menu and command (`devices.sidechain`) as a plugin card's row, with a lamp
//! that lights with what arrives. Where nothing keys the effect it is a quiet
//! "Sidechain · none" chip, so the choice can always be reached from the card.

use eframe::egui::{
    self, Align2, Color32, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use super::plan::Plan;
use crate::chain::geometry::Step;
use crate::chain::sidechain::{self, Look, Source};
use crate::theme::personality::card as c;
use crate::widgets::{self, raised};

/// The index of a step in the chip's tokens.
fn at(step: Step) -> usize {
    match step {
        Step::Whole => 0,
        Step::Short => 1,
        Step::Compact => 2,
    }
}

/// Where the chip stands, `width` wide: at the screen's lower left (above the
/// dials' strip on the short card, centred on the band on the compact one).
fn place(plan: &Plan, width: f32) -> Rect {
    let k = at(plan.step);
    let height = c::KEY_HEIGHT[k];
    let left = plan.screen.left() + c::KEY_LEFT[k];
    match plan.step {
        Step::Whole | Step::Short => Rect::from_min_size(
            pos2(left, plan.picture.bottom() - c::KEY_FOOT[k] - height),
            vec2(width, height),
        ),
        Step::Compact => Rect::from_center_size(
            pos2(left + width / 2.0, plan.screen.center().y),
            vec2(width, height),
        ),
    }
}

/// Draws the chip and answers it (a click opens the sidechain menu).
pub(super) fn chip(
    ui: &mut Ui,
    plan: &Plan,
    id: egui::Id,
    look: &Look,
    (hue, deep): (Color32, Color32),
) -> Response {
    let k = at(plan.step);
    let palette = *crate::theme::p();
    let font = FontId::proportional(c::KEY_TEXT[k]);
    let (words, ink) = match &look.source {
        Source::None => (
            "Sidechain \u{00B7} none".to_owned(),
            Color32::WHITE.gamma_multiply(c::KEY_WORDS_NONE),
        ),
        Source::Gone => ("Source removed".to_owned(), palette.warning),
        Source::Track { name, .. } => (
            // The compact band is a strip of picture: the chip says only whose
            // key it is, so the picture stays in view (the tooltip and the
            // accessible name have the whole sentence).
            if plan.step == Step::Compact {
                format!("{name} \u{00B7} {}", sidechain::tap_short(look.tap))
            } else {
                format!(
                    "Keyed by {name} \u{00B7} {}",
                    sidechain::tap_short(look.tap)
                )
            },
            Color32::WHITE.gamma_multiply(c::KEY_WORDS),
        ),
    };
    let lamp = if matches!(look.source, Source::Track { .. }) {
        2.0 * c::KEY_LAMP + c::KEY_PAD / 2.0
    } else {
        0.0
    };
    let room = if plan.step == Step::Compact {
        plan.screen.width() - c::KEY_LEFT[k] * 2.0
    } else {
        plan.picture.width() * 0.6
    } - 2.0 * c::KEY_PAD
        - lamp;
    let painter = ui.painter();
    let words = widgets::fit(painter, &words, font.clone(), room.max(0.0));
    let wide = painter
        .layout_no_wrap(words.clone(), font.clone(), ink)
        .size()
        .x;
    let rect = place(plan, wide + 2.0 * c::KEY_PAD + lamp);
    let response = ui.interact(rect, sidechain::chooser_id_of(id), Sense::click());
    let spoken = sidechain::spoken(look);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, &spoken));

    let radius = rect.height() / 2.0;
    let hover = crate::motion::hover(ui.ctx(), id.with("key hover"), response.hovered());
    let painter = ui.painter();
    painter.rect_filled(rect, radius, deep);
    painter.rect_filled(
        rect,
        radius,
        hue.gamma_multiply(crate::light::EMBER_WASH * hover),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(
            crate::theme::stroke::HAIRLINE,
            hue.gamma_multiply(c::KEY_RING + (1.0 - c::KEY_RING) * hover),
        ),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.left() + c::KEY_PAD, rect.center().y),
        Align2::LEFT_CENTER,
        words,
        font,
        ink,
    );
    if lamp > 0.0 {
        // The lamp: lit by what arrives, a warning ring while the song plays
        // and the key has been silent.
        let lit = pos2(
            rect.right() - c::KEY_PAD / 2.0 - c::KEY_LAMP,
            rect.center().y,
        );
        if look.quiet {
            painter.circle_stroke(
                lit,
                c::KEY_LAMP,
                Stroke::new(crate::theme::stroke::HAIRLINE, palette.warning),
            );
        } else {
            painter.circle_filled(
                lit,
                c::KEY_LAMP,
                hue.gamma_multiply(0.25 + 0.75 * look.level),
            );
        }
    }
    raised::focus_ring(painter, &response, rect);
    response.on_hover_text(sidechain::tip(look))
}
