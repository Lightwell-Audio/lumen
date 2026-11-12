//! A switch for anything on or off: its label at the left, the track and knob
//! at the right of a row as wide as there is room for, the knob gliding
//! across when it is flipped.

use eframe::egui::{self, Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;

use crate::widgets::{note_change, raised, shorten};
use crate::{motion, theme};

use super::menu::menu_inset;

/// A switch's track: the knob travels along it.
const TRACK: egui::Vec2 = vec2(30.0, 16.0);
/// How wide a switch with no label is, for tables that line switches up in
/// columns.
pub const SWITCH_WIDTH: f32 = TRACK.x;
/// Room between the knob and the track's edge.
const KNOB_INSET: f32 = 2.0;
/// A switch named `label`, with the label at the left and the switch at
/// the right of a row as wide as there is room for (in a menu, the menu's
/// width, in from its edges as its rows' labels and keycaps are). The
/// whole row toggles `on`; the knob glides across (`GLIDE`) and, on, is
/// the accent's gradient in an `accent_muted` track. `enabled` false greys
/// it out.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, enabled: bool) -> egui::Response {
    switch_noted(ui, on, label, enabled, "")
}

/// [`switch`] with a quiet `note` in the row, right-aligned beside the
/// switch (shortened to the room between the label and the switch). The row
/// stays one widget, so nothing is laid out after it that could widen it.
pub fn switch_noted(
    ui: &mut Ui,
    on: &mut bool,
    label: &str,
    enabled: bool,
    note: &str,
) -> egui::Response {
    let palette = theme::p();
    let text = if enabled {
        palette.text
    } else {
        palette.text_disabled
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        text,
    );
    let label_width = galley.size().x;
    let gap = if label.is_empty() {
        0.0
    } else {
        theme::space::M
    };
    let side = menu_inset(ui);
    let needed = galley.size().x + gap + TRACK.x + 2.0 * side;
    let width = if ui.is_sizing_pass() || label.is_empty() {
        needed
    } else {
        needed.max(ui.available_width())
    };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(width, theme::size::CONTROL_SMALL), sense);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
        note_change(ui);
    }
    let state = *on;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, state, label)
    });
    let ctx = ui.ctx().clone();
    if !ui.is_rect_visible(rect) {
        // Out of view (scrolled away): follow the state without gliding, so
        // that when the row comes back it is where it should be and does not
        // glide there from where it was last seen.
        motion::glide(
            &ctx,
            response.id.with("knob"),
            if state { 1.0 } else { 0.0 },
            false,
        );
        return response;
    }
    let hover = motion::hover(&ctx, response.id.with("hover"), response.hovered());
    let travel = motion::glide(
        &ctx,
        response.id.with("knob"),
        if state { 1.0 } else { 0.0 },
        true,
    );
    let painter = ui.painter();
    let inner = rect.shrink2(vec2(side, 0.0));
    painter.galley(
        pos2(inner.left(), inner.center().y - galley.size().y / 2.0),
        galley,
        text,
    );
    let track = Align2::RIGHT_CENTER
        .anchor_size(inner.right_center(), TRACK)
        .round_to_pixels(ui.pixels_per_point());
    if !note.is_empty() {
        let font = FontId::proportional(theme::size::CAPTION);
        let room = (track.left() - theme::space::M - (inner.left() + label_width + gap)).max(0.0);
        let words = shorten(painter, note, font.clone(), room);
        painter.text(
            pos2(track.left() - theme::space::M, track.center().y),
            Align2::RIGHT_CENTER,
            words,
            font,
            palette.text_hint,
        );
    }
    // The track is a well with the shade lip; on, it turns `accent_muted`
    // with the 38% accent hairline (the Segmented card's switch).
    raised::paint_sunken(painter, track, theme::radius::FULL);
    if travel > 0.0 {
        painter.rect_filled(
            track,
            theme::radius::FULL,
            palette.accent_muted.gamma_multiply(travel),
        );
        painter.rect_stroke(
            track,
            theme::radius::FULL,
            Stroke::new(
                theme::stroke::HAIRLINE,
                palette.accent.gamma_multiply(theme::ON_HAIRLINE * travel),
            ),
            egui::StrokeKind::Inside,
        );
    }
    if enabled && hover > 0.0 {
        painter.rect_filled(
            track,
            theme::radius::FULL,
            palette.ember.gamma_multiply(hover),
        );
    }
    let radius = TRACK.y / 2.0 - KNOB_INSET;
    let x = track.left() + KNOB_INSET + radius + travel * (TRACK.x - 2.0 * (KNOB_INSET + radius));
    let knob = Rect::from_center_size(pos2(x, track.center().y), vec2(2.0 * radius, 2.0 * radius));
    if enabled {
        // Off: `text` to `text_secondary`; on: the accent's gradient.
        raised::gradient(
            painter,
            knob,
            radius,
            palette.text.lerp_to_gamma(palette.accent_hi, travel),
            palette
                .text_secondary
                .lerp_to_gamma(palette.accent_lo, travel),
        );
    } else {
        painter.circle_filled(knob.center(), radius, palette.text_disabled);
    }
    if response.has_focus() {
        raised::paint_focus(painter, track, theme::radius::FULL);
    }
    response
}
