//! A segmented control for two to five choices, and the 30 pt switch for the
//! tool being worked with: one sunk trough, a raised thumb that slides to the
//! chosen piece.

use eframe::egui::{self, FontId, Id as EguiId, Rect, Sense, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;

use crate::theme::Tone;
use crate::widgets::{Keys, note_change, raised};
use crate::{fonts, motion, theme};

use super::{menu::menu_inset, slide_between};

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
    /// The choice's shortcut, shown as K5 keycaps in its tooltip beside
    /// `tip` (the owner's KC C: no key inside the control). From the
    /// keymap, never written text.
    pub keys: Option<&'a Keys>,
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
            keys: None,
        }
    }

    /// Shows `keys` as keycaps in the choice's tooltip.
    pub fn keys(mut self, keys: Option<&'a Keys>) -> Self {
        self.keys = keys;
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
    segmented_with(ui, id, value, choices, Look::default())
}

/// [`segmented`] across the whole width it is given, the choices in equal
/// pieces, or, where the widest would not fit that way, each its own width
/// with an equal share of the room left (the browser's type filter under
/// its search field, the interface audit's B2). Where even the words do not
/// fit, it is as wide as they need, as [`segmented`] is.
pub fn segmented_full<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: EguiId,
    value: T,
    choices: &[Choice<'_, T>],
) -> Option<T> {
    segmented_with(
        ui,
        id,
        value,
        choices,
        Look {
            full: true,
            ..Look::default()
        },
    )
}

/// Width of a choice's icon (the Button card's 14 pt).
const ICON: f32 = 14.0;
/// Gap between a choice's icon and its label.
const ICON_GAP: f32 = 6.0;
/// The room a choice's icon takes before its label: the icon alone when
/// there is no label (the browser's icon tabs, M9 R7c).
fn icon_room<T>(choice: &Choice<'_, T>) -> f32 {
    match (choice.icon.is_some(), choice.label.is_empty()) {
        (false, _) => 0.0,
        (true, true) => ICON,
        (true, false) => ICON + ICON_GAP,
    }
}
/// Room either side of the letters in the 30 pt tool switch (the
/// Segmented card's `padding: 0 10px`).
const TOOL_PAD: f32 = 10.0;

/// [`segmented`] for the *tool* the user is working with (the clip
/// editors' Pointer and Draw): 30 pt high. The chosen piece is filled in
/// `lit`'s tone (`Tone::Accent`: `accent_hi` to `accent_lo`, `on_accent`
/// letters, no glow), or is the neutral raised thumb when `lit` is `None`.
/// The owner's TL C: the resting tool (Pointer) is grey and a drawing tool
/// is lit, as the arrangement's tool switch is.
pub fn segmented_tool<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: EguiId,
    value: T,
    choices: &[Choice<'_, T>],
    lit: Option<Tone>,
) -> Option<T> {
    segmented_with(
        ui,
        id,
        value,
        choices,
        Look {
            tool: true,
            lit,
            full: false,
        },
    )
}

/// The width [`segmented_tool`] takes for `choices`, outside a menu.
pub fn segmented_tool_width<T>(ui: &Ui, choices: &[Choice<'_, T>]) -> f32 {
    let font = FontId::proportional(theme::size::BODY);
    let pieces: f32 = choices
        .iter()
        .map(|c| {
            let words = ui
                .painter()
                .layout_no_wrap(c.label.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x;
            words + icon_room(c) + 2.0 * TOOL_PAD
        })
        .sum();
    pieces + 2.0 * theme::space::XXS
}

/// How a [`segmented_with`] row looks.
#[derive(Debug, Clone, Copy, Default)]
struct Look {
    /// The 30 pt tool size, with its wider padding.
    tool: bool,
    /// The chosen piece is filled in this tone instead of the thumb.
    lit: Option<Tone>,
    /// The pieces fill the width given ([`segmented_full`]).
    full: bool,
}

/// The segmented control, as [`Look`] says.
fn segmented_with<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: EguiId,
    value: T,
    choices: &[Choice<'_, T>],
    Look { tool, lit, full }: Look,
) -> Option<T> {
    let palette = theme::p();
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
    let widths: Vec<f32> = galleys
        .iter()
        .zip(choices)
        .map(|(g, c)| g.size().x + icon_room(c) + 2.0 * pad)
        .collect();
    let side = menu_inset(ui);
    // Full width: equal pieces when the widest's content fits that way with
    // a little room each side; otherwise each piece its own width and an
    // equal share of the room left.
    let widths: Vec<f32> = if full && !choices.is_empty() {
        let n = choices.len() as f32;
        let room = ui.available_width() - side - 2.0 * inset;
        let widest = widths.iter().copied().fold(0.0, f32::max) - 2.0 * pad;
        if (widest + 2.0 * theme::space::XS) * n <= room {
            vec![room / n; choices.len()]
        } else {
            let spare = (room - widths.iter().sum::<f32>()).max(0.0) / n;
            widths.iter().map(|w| w + spare).collect()
        }
    } else {
        widths
    };
    let total = widths.iter().sum::<f32>() + 2.0 * inset;
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
            response = if choice.keys.is_some() {
                crate::widgets::tip(response, tip, choice.keys, None)
            } else if choice.enabled {
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
            note_change(ui);
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
        // Glide the *index* and slide between the live segments, so a strip
        // that itself moves (a panel being resized) carries the thumb along
        // rigidly; only a change of choice glides.
        let to = segments[index].0;
        let spans: Vec<(f32, f32)> = segments
            .iter()
            .map(|(segment, ..)| (segment.left(), segment.right()))
            .collect();
        let at = motion::glide(&ctx, id.with("thumb at"), index as f32, true);
        let (left, right) = slide_between(&spans, at);
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
        raised::bare_lip(painter, thumb, piece_radius, light);
    }
    for ((segment, response, chosen), (choice, galley)) in
        segments.iter().zip(choices.iter().zip(galleys))
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
        // The label is centred in the piece; its icon is centred on the
        // label's optical middle, snapped to whole pixels.
        let icon = icon_room(choice);
        let content = galley.size().x + icon;
        let left = (segment.center().x - content / 2.0).round();
        let top = segment.center().y.round() - (galley.size().y / 2.0).round();
        let mid = (top + fonts::optical_middle(&galley)).round_to_pixels(ppp);
        if let Some(draw) = choice.icon {
            draw(painter, pos2(left + ICON / 2.0, mid), colour);
        }
        painter.galley_with_override_text_color(pos2(left + icon, top), galley.clone(), colour);
        if response.has_focus() {
            raised::paint_focus(painter, *segment, piece_radius);
        }
    }
    picked
}
