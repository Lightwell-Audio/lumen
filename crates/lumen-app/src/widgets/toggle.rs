//! On/off and small choices (the interface audit's T3, the owner's A):
//! a switch for anything on or off, and a segmented control for two to
//! five choices. They replace egui's checkboxes and radio buttons, the only
//! controls that still looked like egui; longer lists are menu rows
//! (`menu_toggle`).

use eframe::egui::{
    self, Align2, FontId, Id as EguiId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::{motion, theme};

/// A switch's track: the knob travels along it.
const TRACK: egui::Vec2 = vec2(30.0, 16.0);
/// Room between the knob and the track's edge.
const KNOB_INSET: f32 = 2.0;

/// A switch named `label`, with the label at the left and the switch at
/// the right of a row as wide as there is room for (in a menu, the menu's
/// width). The whole row toggles `on`; the knob glides across (`GLIDE`)
/// and lights with the accent when on. `enabled` false greys it out.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, enabled: bool) -> egui::Response {
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
    let gap = if label.is_empty() {
        0.0
    } else {
        theme::space::M
    };
    let needed = galley.size().x + gap + TRACK.x;
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
    }
    let state = *on;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, state, label)
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let ctx = ui.ctx().clone();
    let hover = motion::hover(&ctx, response.id.with("hover"), response.hovered());
    let travel = motion::glide(
        &ctx,
        response.id.with("knob"),
        if state { 1.0 } else { 0.0 },
        true,
    );
    let painter = ui.painter();
    painter.galley(
        pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        text,
    );
    let track = Align2::RIGHT_CENTER.anchor_size(rect.right_center(), TRACK);
    let controls = theme::controls(false);
    let off = controls.press.lerp_to_gamma(controls.hover, hover * 0.5);
    let fill = off.lerp_to_gamma(palette.accent_muted, travel);
    painter.rect_filled(track, TRACK.y / 2.0, fill);
    let radius = TRACK.y / 2.0 - KNOB_INSET;
    let x = track.left() + KNOB_INSET + radius + travel * (TRACK.x - 2.0 * (KNOB_INSET + radius));
    let knob = if enabled {
        palette.text_secondary.lerp_to_gamma(palette.accent, travel)
    } else {
        palette.text_disabled
    };
    painter.circle_filled(pos2(x, track.center().y), radius, knob);
    if response.has_focus() {
        painter.rect_stroke(
            track.expand(2.0),
            TRACK.y / 2.0 + 2.0,
            Stroke::new(2.0, palette.accent),
            StrokeKind::Inside,
        );
    }
    response
}

/// One choice in a list of them, as a row as wide as there is room for:
/// for choices too many or too long for a [`segmented`] control (a
/// device's channel pairs). The chosen row has the thumb's fill and accent
/// letters; others lighten under the pointer. A radio button named `label`.
pub fn choice_row(ui: &mut Ui, label: &str, on: bool) -> egui::Response {
    let palette = theme::p();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        palette.text,
    );
    let pad = theme::space::S;
    let needed = galley.size().x + 2.0 * pad;
    let width = if ui.is_sizing_pass() {
        needed
    } else {
        needed.max(ui.available_width())
    };
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, theme::size::CONTROL), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, label));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
    let painter = ui.painter();
    let controls = theme::controls(false);
    if on {
        painter.rect_filled(rect, theme::radius::SMALL, palette.accent_muted);
    } else if hover > 0.0 {
        painter.rect_filled(
            rect,
            theme::radius::SMALL,
            controls.fill.gamma_multiply(hover),
        );
    }
    let colour = if on { palette.accent } else { palette.text };
    painter.galley_with_override_text_color(
        pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            theme::radius::SMALL,
            Stroke::new(2.0, palette.accent),
            StrokeKind::Inside,
        );
    }
    response
}

/// One choice of a [`segmented`] control.
#[derive(Debug, Clone, Copy)]
pub struct Choice<'a, T> {
    pub value: T,
    pub label: &'a str,
    pub enabled: bool,
    /// Said on hover: why a greyed choice cannot be taken, or what a
    /// choice means.
    pub tip: Option<&'a str>,
    /// Its accessible name, when the label alone is ambiguous ("Fade in:
    /// Linear" beside "Fade out: Linear").
    pub name: Option<&'a str>,
}

impl<'a, T> Choice<'a, T> {
    pub fn new(value: T, label: &'a str) -> Self {
        Self {
            value,
            label,
            enabled: true,
            tip: None,
            name: None,
        }
    }

    pub fn named(mut self, name: &'a str) -> Self {
        self.name = Some(name);
        self
    }

    pub fn tip(mut self, tip: &'a str) -> Self {
        self.tip = Some(tip);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// A row of two to five `choices` sharing one track, the chosen one under
/// a thumb that slides to it (`GLIDE`). Each choice is a radio button named
/// by its label. Returns the choice clicked this frame, if it differs from
/// `value`.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: EguiId,
    value: T,
    choices: &[Choice<'_, T>],
) -> Option<T> {
    let palette = theme::p();
    let pad = theme::space::S;
    let font = FontId::proportional(theme::size::BODY);
    let galleys: Vec<_> = choices
        .iter()
        .map(|c| {
            ui.painter()
                .layout_no_wrap(c.label.to_owned(), font.clone(), palette.text)
        })
        .collect();
    let inset = theme::space::XXS;
    let height = theme::size::CONTROL_SMALL;
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 2.0 * pad).collect();
    let total = widths.iter().sum::<f32>() + 2.0 * inset;
    let (rect, _) = ui.allocate_exact_size(vec2(total, height), Sense::hover());
    let mut picked = None;
    let mut x = rect.left() + inset;
    let mut segments = Vec::with_capacity(choices.len());
    for (i, (choice, width)) in choices.iter().zip(&widths).enumerate() {
        let segment = Rect::from_min_max(
            pos2(x, rect.top() + inset),
            pos2(x + width, rect.bottom() - inset),
        );
        x += width;
        let sense = if choice.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let mut response = ui.interact(segment, id.with(i), sense);
        if let Some(tip) = choice.tip {
            response = if choice.enabled {
                response.on_hover_text(tip)
            } else {
                response.on_disabled_hover_text(tip)
            };
        }
        let chosen = choice.value == value;
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::RadioButton,
                choice.enabled,
                chosen,
                choice.name.unwrap_or(choice.label),
            )
        });
        if response.clicked() && !chosen {
            picked = Some(choice.value);
        }
        segments.push((segment, response, chosen));
    }
    if !ui.is_rect_visible(rect) {
        return picked;
    }
    let ctx = ui.ctx().clone();
    let painter = ui.painter();
    let controls = theme::controls(false);
    painter.rect_filled(rect, theme::radius::SMALL, controls.fill);
    // The thumb slides to the chosen segment, the newly picked one at once
    // so the click lands where it is drawn.
    let target = picked
        .and_then(|p| choices.iter().position(|c| c.value == p))
        .or_else(|| choices.iter().position(|c| c.value == value));
    if let Some(index) = target {
        let to = segments[index].0;
        let left = motion::glide(&ctx, id.with("thumb left"), to.left(), true);
        let right = motion::glide(&ctx, id.with("thumb right"), to.right(), true);
        let thumb = Rect::from_x_y_ranges(left..=right, to.y_range());
        painter.rect_filled(thumb, theme::radius::SMALL - 1, palette.accent_muted);
    }
    for ((segment, response, chosen), (choice, galley)) in
        segments.iter().zip(choices.iter().zip(galleys))
    {
        let colour = if !choice.enabled {
            palette.text_disabled
        } else if *chosen || picked == Some(choice.value) {
            palette.accent
        } else if response.hovered() {
            palette.text
        } else {
            controls.text
        };
        painter.galley_with_override_text_color(
            segment.center() - galley.size() / 2.0,
            galley,
            colour,
        );
        if response.has_focus() {
            painter.rect_stroke(
                *segment,
                theme::radius::SMALL,
                Stroke::new(2.0, palette.accent),
                StrokeKind::Inside,
            );
        }
    }
    picked
}
