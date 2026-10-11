//! On/off and small choices (the interface audit's T3, the owner's A):
//! a switch for anything on or off, and a segmented control for two to
//! five choices. They replace egui's checkboxes and radio buttons, the only
//! controls that still looked like egui; longer lists are menu rows
//! (`menu_toggle`). Their look is the design system's Segmented card
//! (ADR-0052): sunk troughs, raised neutral thumbs, Ember under the
//! pointer, a hairline for what is on.

use eframe::egui::{self, Align2, FontId, Id as EguiId, Rect, Sense, Stroke, Ui, pos2, vec2};

use eframe::emath::GuiRounding as _;

use super::raised;
use crate::theme::Tone;
use crate::{light, motion, theme};

/// A switch's track: the knob travels along it.
const TRACK: egui::Vec2 = vec2(30.0, 16.0);
/// Room between the knob and the track's edge.
const KNOB_INSET: f32 = 2.0;

/// How far a control in a menu sits in from its edges: as far as the menu's
/// rows put their labels and keycaps (`button_padding`), so switches,
/// segmented controls and captions line up with them. Nothing elsewhere.
pub(super) fn menu_inset(ui: &Ui) -> f32 {
    if egui::containers::menu::is_in_menu(ui) {
        ui.spacing().button_padding.x
    } else {
        0.0
    }
}

/// A switch named `label`, with the label at the left and the switch at
/// the right of a row as wide as there is room for (in a menu, the menu's
/// width, in from its edges as its rows' labels and keycaps are). The
/// whole row toggles `on`; the knob glides across (`GLIDE`) and, on, is
/// the accent's gradient in an `accent_muted` track. `enabled` false greys
/// it out.
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
        super::note_change(ui);
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
    let inner = rect.shrink2(vec2(side, 0.0));
    painter.galley(
        pos2(inner.left(), inner.center().y - galley.size().y / 2.0),
        galley,
        text,
    );
    let track = Align2::RIGHT_CENTER
        .anchor_size(inner.right_center(), TRACK)
        .round_to_pixels(ui.pixels_per_point());
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

/// A small heading over a group of controls in a menu (Theme, Mixing,
/// Fade in): quiet, and in line with the menu's labels.
pub fn menu_caption(ui: &mut Ui, text: &str) {
    let side = menu_inset(ui);
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: side.round() as i8,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| ui.label(egui::RichText::new(text).small().weak()));
}

/// One choice in a list of them, as a row as wide as there is room for:
/// for choices too many or too long for a [`segmented`] control (a
/// device's channel pairs). The chosen row is `accent_muted` with accent
/// letters; others take the Ember wash under the pointer. A radio button
/// named `label`.
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
    if response.clicked() && !on {
        super::note_change(ui);
    }
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, label));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
    let painter = ui.painter();
    let radius = theme::radius::for_height(rect.height());
    if on {
        painter.rect_filled(rect, f32::from(radius), palette.accent_muted);
    } else if hover > 0.0 {
        painter.rect_filled(rect, f32::from(radius), palette.ember.gamma_multiply(hover));
    }
    let colour = if on { palette.accent } else { palette.text };
    painter.galley_with_override_text_color(
        pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    raised::focus_ring(painter, &response, rect);
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
    /// An icon before the label, drawn centred on the point it is given in
    /// the colour given; on the same line as the label.
    pub icon: Option<fn(&egui::Painter, egui::Pos2, egui::Color32)>,
    /// The choice's key, drawn as a small keycap after the label (from the
    /// keymap, never written text). Short keys only.
    pub hint: Option<&'a str>,
}

impl<'a, T> Choice<'a, T> {
    pub fn new(value: T, label: &'a str) -> Self {
        Self {
            value,
            label,
            enabled: true,
            tip: None,
            name: None,
            icon: None,
            hint: None,
        }
    }

    /// Shows `key` as a keycap after the label.
    pub fn hint(mut self, key: Option<&'a str>) -> Self {
        self.hint = key.filter(|k| k.chars().count() <= 3);
        self
    }

    pub fn icon(mut self, icon: fn(&egui::Painter, egui::Pos2, egui::Color32)) -> Self {
        self.icon = Some(icon);
        self
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

/// A row of two to five `choices` sharing one sunk trough, the chosen one
/// under a raised thumb that slides to it (`GLIDE`). Each choice is a radio
/// button named by its label. Returns the choice clicked this frame, if it
/// differs from `value`.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: EguiId,
    value: T,
    choices: &[Choice<'_, T>],
) -> Option<T> {
    segmented_styled(ui, id, value, choices, None)
}

/// Width of a choice's icon (the Button card's 14 pt).
const ICON: f32 = 14.0;
/// Gap between a choice's icon and its label.
const ICON_GAP: f32 = 6.0;
/// Room either side of the letters in the 30 pt tool switch (the
/// Segmented card's `padding: 0 10px`).
const TOOL_PAD: f32 = 10.0;

/// [`segmented`] for the *tool* the user is working with (the piano roll's
/// Pointer and Draw): 30 pt high, with the chosen piece filled in `lit`'s
/// tone (`Tone::Accent`: `accent_hi` to `accent_lo`, `on_accent` letters, no
/// glow) instead of the neutral raised thumb. `None` is [`segmented`]: 20 pt
/// and neutral (the Segmented card, ADR-0052).
pub fn segmented_styled<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: EguiId,
    value: T,
    choices: &[Choice<'_, T>],
    lit: Option<Tone>,
) -> Option<T> {
    let palette = theme::p();
    let tool = lit.is_some();
    let pad = if tool { TOOL_PAD } else { theme::space::S };
    let font = FontId::proportional(theme::size::BODY);
    let galleys: Vec<_> = choices
        .iter()
        .map(|c| {
            ui.painter()
                .layout_no_wrap(c.label.to_owned(), font.clone(), palette.text)
        })
        .collect();
    let inset = theme::space::XXS;
    let height = if tool {
        theme::size::CONTROL_LARGE
    } else {
        theme::size::CONTROL_SMALL
    };
    // Pieces sit 2 pt inside the trough with its radius less 2.
    let trough_radius = theme::radius::for_height(height);
    let piece_radius = trough_radius.saturating_sub(inset as u8);
    let caps: Vec<_> = choices
        .iter()
        .map(|c| {
            c.hint.map(|key| {
                ui.painter().layout_no_wrap(
                    key.to_owned(),
                    FontId::monospace(theme::size::SCALE),
                    palette.text_hint,
                )
            })
        })
        .collect();
    let widths: Vec<f32> = galleys
        .iter()
        .zip(choices)
        .zip(&caps)
        .map(|((g, c), cap)| {
            let icon = if c.icon.is_some() {
                ICON + ICON_GAP
            } else {
                0.0
            };
            g.size().x + icon + cap_width(cap.as_deref()) + 2.0 * pad
        })
        .collect();
    let total = widths.iter().sum::<f32>() + 2.0 * inset;
    let side = menu_inset(ui);
    let (row, _) = ui.allocate_exact_size(vec2(total + side, height), Sense::hover());
    let rect = Rect::from_min_size(pos2(row.left() + side, row.top()), vec2(total, height));
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
            super::note_change(ui);
        }
        segments.push((segment, response, chosen));
    }
    if !ui.is_rect_visible(rect) {
        return picked;
    }
    let ctx = ui.ctx().clone();
    let ppp = ui.pixels_per_point();
    let painter = ui.painter();
    let controls = theme::controls(false);
    raised::paint_sunken(painter, rect.round_to_pixels(ppp), trough_radius);
    // The raised thumb slides to the chosen segment, the newly picked one
    // at once so the click lands where it is drawn.
    let target = picked
        .and_then(|p| choices.iter().position(|c| c.value == p))
        .or_else(|| choices.iter().position(|c| c.value == value));
    if let Some(index) = target {
        let to = segments[index].0;
        let left = motion::glide(&ctx, id.with("thumb left"), to.left(), true);
        let right = motion::glide(&ctx, id.with("thumb right"), to.right(), true);
        let thumb = Rect::from_x_y_ranges(left..=right, to.y_range()).round_to_pixels(ppp);
        // `thumb_hi` to `thumb` with an edge light and no border; a tool is
        // filled in its tone, its stronger edge light on top.
        let (top, bottom, light) = match lit {
            Some(tone) => {
                let on = theme::lit(tone);
                (on.top, on.bottom, on.light)
            }
            None => (palette.thumb_hi, palette.thumb, palette.edge_light),
        };
        raised::gradient(painter, thumb, f32::from(piece_radius), top, bottom);
        raised::lip(painter, thumb, piece_radius, light);
    }
    for (((segment, response, chosen), (choice, galley)), cap) in
        segments.iter().zip(choices.iter().zip(galleys)).zip(caps)
    {
        let on = *chosen || picked == Some(choice.value);
        // An unchosen piece under the pointer takes the Ember wash.
        let hover = motion::hover(
            &ctx,
            response.id.with("hover"),
            response.hovered() && choice.enabled && !on,
        );
        if hover > 0.0 {
            painter.rect_filled(
                *segment,
                f32::from(piece_radius),
                palette.ember.gamma_multiply(hover),
            );
        }
        let colour = if !choice.enabled {
            palette.text_disabled
        } else if on {
            lit.map_or(palette.text, |tone| theme::lit(tone).text)
        } else if response.hovered() {
            palette.text
        } else {
            controls.text
        };
        // Icon and label share one centre line, snapped to whole pixels.
        let icon = if choice.icon.is_some() {
            ICON + ICON_GAP
        } else {
            0.0
        };
        let content = galley.size().x + icon + cap_width(cap.as_deref());
        let left = (segment.center().x - content / 2.0).round();
        let mid = segment.center().y.round();
        if let Some(draw) = choice.icon {
            draw(painter, pos2(left + ICON / 2.0, mid), colour);
        }
        painter.galley_with_override_text_color(
            pos2(left + icon, mid - (galley.size().y / 2.0).round()),
            galley.clone(),
            colour,
        );
        if let Some(cap) = cap {
            let at = pos2(left + icon + galley.size().x + CAP_GAP, mid);
            paint_cap(painter, at, cap, colour);
        }
        if response.has_focus() {
            raised::paint_focus(painter, *segment, piece_radius);
        }
    }
    picked
}

/// Gap between a choice's label and its keycap.
const CAP_GAP: f32 = 6.0;
/// Padding either side of a keycap's letters.
const CAP_PAD: f32 = 4.0;

/// The room a keycap takes after a label, with its gap.
fn cap_width(cap: Option<&egui::Galley>) -> f32 {
    cap.map_or(0.0, |g| CAP_GAP + g.size().x + 2.0 * CAP_PAD)
}

/// A small keycap whose left edge is at `at.x`, centred on `at.y`: a
/// hairline outline and the key in the mono face, in `ink` at a lower
/// strength so it reads as a hint beside the label. Not the full K5 keycap
/// (`keys::keycaps` is 25 pt tall with its light, and a 20 pt trough's
/// pieces are 16 pt): the shortcut is also in the choice's tooltip as K5.
fn paint_cap(
    painter: &egui::Painter,
    at: egui::Pos2,
    galley: std::sync::Arc<egui::Galley>,
    ink: egui::Color32,
) {
    let size = vec2(galley.size().x + 2.0 * CAP_PAD, galley.size().y + 2.0);
    let rect = Rect::from_min_size(pos2(at.x, at.y - (size.y / 2.0).round()), size);
    painter.rect_stroke(
        rect,
        theme::radius::SMALL,
        Stroke::new(theme::stroke::HAIRLINE, ink.gamma_multiply(0.45)),
        egui::StrokeKind::Inside,
    );
    painter.galley_with_override_text_color(
        pos2(rect.left() + CAP_PAD, rect.top() + 1.0),
        galley,
        ink.gamma_multiply(0.75),
    );
}

/// A button that stays lit while `on` (Browser, Detail): raised like every
/// button when off, `accent_muted` with accent letters and a 38% accent
/// hairline when on (the Button card, ADR-0052; the interface audit's T4:
/// off still reads as a button).
pub fn toggle_button(ui: &mut Ui, on: &mut bool, label: &str) -> egui::Response {
    let palette = theme::p();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        palette.text,
    );
    let pad = theme::space::S;
    let size = vec2(galley.size().x + 2.0 * pad, theme::size::CONTROL);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let state = *on;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, state, label)
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let lit = motion::hover(ui.ctx(), response.id.with("lit"), state);
    let face = raised::face_of(ui, &response).lit(lit, Tone::On);
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let body = raised::paint(
        ui.painter(),
        rect,
        theme::radius::for_height(rect.height()),
        &face,
    );
    raised::centre_galley(ui.painter(), body, galley, face.text());
    raised::focus_ring(ui.painter(), &response, rect);
    response
}

/// One tab of [`tabs`].
#[derive(Debug, Clone, Copy)]
pub struct Tab<'a> {
    pub label: &'a str,
    /// Said on hover: why an unbuilt tab cannot be opened.
    pub disabled_because: Option<&'a str>,
    pub tip: &'a str,
}

/// A row of tabs as words, 12 pt of room either side of each and 4 pt
/// between tabs; the chosen one is in `text` over a 2 pt accent underline,
/// inset by the same 12 pt and lit with Lit's soft layer, that slides to
/// it (`GLIDE`). Tabs that cannot open yet are in `text_disabled` with why
/// on hover (the interface audit's T4, the owner's A). Returns the index
/// clicked, if another.
pub fn tabs(ui: &mut Ui, id: EguiId, chosen: usize, tabs: &[Tab<'_>]) -> Option<usize> {
    let palette = theme::p();
    let gap = theme::space::XS;
    let mut picked = None;
    let mut chosen_rect = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, tab) in tabs.iter().enumerate() {
            let enabled = tab.disabled_because.is_none();
            let colour = if !enabled {
                palette.text_disabled
            } else if i == chosen {
                palette.text
            } else {
                palette.text_secondary
            };
            let galley = ui.painter().layout_no_wrap(
                tab.label.to_owned(),
                FontId::proportional(theme::size::BODY),
                colour,
            );
            let size = vec2(
                galley.size().x + 2.0 * theme::space::M,
                theme::size::CONTROL,
            );
            let sense = if enabled {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(size, sense);
            let selected = i == chosen;
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    enabled,
                    selected,
                    format!("{} tab", tab.label),
                )
            });
            let response = match tab.disabled_because {
                Some(why) => response.on_disabled_hover_text(why).on_hover_text(why),
                None => response.on_hover_text(tab.tip),
            };
            let hover = response.hovered() && enabled && !selected;
            ui.painter().galley_with_override_text_color(
                rect.center() - galley.size() / 2.0,
                galley,
                if hover { palette.text } else { colour },
            );
            raised::focus_ring(ui.painter(), &response, rect);
            if response.clicked() && !selected {
                picked = Some(i);
            }
            if Some(i) == picked.or(Some(chosen)) {
                chosen_rect = Some(rect);
            }
        }
    });
    if let Some(rect) = chosen_rect {
        let ctx = ui.ctx().clone();
        let left = motion::glide(&ctx, id.with("underline left"), rect.left(), true);
        let right = motion::glide(&ctx, id.with("underline right"), rect.right(), true);
        let inset = theme::space::M;
        let line = Rect::from_min_max(
            pos2(left + inset, rect.bottom() - 2.0),
            pos2(right - inset, rect.bottom()),
        );
        light::glow(ui.painter(), light::LIT, line, 1.0, palette.accent, 1.0);
        ui.painter().rect_filled(line, 1.0, palette.accent);
    }
    picked
}
