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
use crate::{fonts, light, motion, theme};

/// A switch's track: the knob travels along it.
const TRACK: egui::Vec2 = vec2(30.0, 16.0);
/// How wide a switch with no label is, for tables that line switches up in
/// columns.
pub const SWITCH_WIDTH: f32 = TRACK.x;
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
        super::note_change(ui);
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
        let words = super::shorten(painter, note, font.clone(), room);
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

/// A context menu's header (the interface audit's M2): what the menu acts
/// on ("Volume \u{00B7} Kick"), in line with the menu's labels, with a
/// divider under it, so it reads as a title rather than a stray label.
pub fn menu_header(ui: &mut Ui, text: &str) {
    let side = menu_inset(ui);
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: side.round() as i8,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            // A long track name ends in "…" rather than widening the menu.
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .small()
                        .color(theme::p().text_secondary),
                )
                .truncate(),
            )
        });
    ui.separator();
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
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, on, label)
    });
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
    let colour = match (enabled, on) {
        (false, _) => palette.text_disabled,
        (true, true) => palette.accent,
        (true, false) => palette.text,
    };
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
    /// The choice's shortcut, shown as K5 keycaps in its tooltip beside
    /// `tip` (the owner's KC C: no key inside the control). From the
    /// keymap, never written text.
    pub keys: Option<&'a super::Keys>,
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
    pub fn keys(mut self, keys: Option<&'a super::Keys>) -> Self {
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
                super::tip(response, tip, choice.keys, None)
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

/// One tab of [`tabs_with`].
#[derive(Debug, Clone, Copy)]
pub struct Tab<'a> {
    pub label: &'a str,
    /// A small word after the label, in caption type: what the tab shows
    /// (the opened clip's name, DP1). Truncates past `theme::size::TAB_NOTE`.
    pub note: Option<&'a str>,
    /// Said on hover: why an unbuilt tab cannot be opened.
    pub disabled_because: Option<&'a str>,
    pub tip: &'a str,
}

/// How a row of [`tabs_with`] is laid out.
#[derive(Debug, Clone, Copy, Default)]
pub struct TabsLayout {
    /// The first tab has no room to its left: its word lines up with a
    /// pane title above it.
    pub flush: bool,
    /// The tabs from this index on stand at the right end of the row,
    /// after a 1 × 14 pt hairline (the detail panel's Mixer, DP1).
    pub right_from: Option<usize>,
    /// Give this tab the keyboard focus this frame.
    pub focus: Option<usize>,
}

/// A hairline between groups of a tab row: 1 × 14 pt in `border_strong`,
/// centred in a row `height` tall at `x`.
pub fn tab_divider(ui: &Ui, x: f32, middle: f32) {
    let line = Rect::from_center_size(
        pos2(x, middle),
        vec2(theme::stroke::HAIRLINE, theme::size::TAB_DIVIDER),
    );
    ui.painter()
        .rect_filled(line, 0.0, theme::p().border_strong);
}

/// A row of tabs, laid out by `layout`: with notes, a group at the right end and
/// a tab to focus. The underline slides across both groups.
pub fn tabs_with(
    ui: &mut Ui,
    id: EguiId,
    chosen: usize,
    tabs: &[Tab<'_>],
    layout: TabsLayout,
) -> Option<usize> {
    let palette = theme::p();
    let gap = theme::space::XS;
    let note_room = theme::space::XS + theme::space::XXS;
    let mut picked = None;
    let mut spans = Vec::with_capacity(tabs.len());
    let mut bottom = 0.0;
    // Each tab's words, laid out first so the right-hand group's width is
    // known before it is placed.
    let laid: Vec<_> = tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
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
            let note = tab.note.map(|note| {
                let ink = if i == chosen {
                    palette.text_secondary
                } else {
                    palette.text_hint
                };
                let mut job = egui::text::LayoutJob::simple_singleline(
                    note.to_owned(),
                    FontId::proportional(theme::size::CAPTION),
                    ink,
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: theme::size::TAB_NOTE,
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('\u{2026}'),
                };
                ui.painter().layout_job(job)
            });
            let left_room = if layout.flush && i == 0 {
                0.0
            } else {
                theme::space::M
            };
            let words = galley.size().x + note.as_ref().map_or(0.0, |n| note_room + n.size().x);
            let size = vec2(left_room + words + theme::space::M, theme::size::CONTROL);
            (enabled, colour, galley, note, left_room, size)
        })
        .collect::<Vec<_>>();
    let widths: Vec<f32> = laid.iter().map(|l| l.5.x).collect();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, (tab, (enabled, colour, galley, note, left_room, size))) in
            tabs.iter().zip(laid).enumerate()
        {
            if layout.right_from == Some(i) {
                // The rest stand at the right end, after a hairline.
                let rest: f32 =
                    widths[i..].iter().sum::<f32>() + (widths.len() - i - 1) as f32 * gap;
                let room = (ui.available_width() - rest - gap).max(gap);
                let (spacer, _) = ui.allocate_exact_size(vec2(room, size.y), Sense::hover());
                // At the tab's own edge, so its word stands 12 pt after it.
                tab_divider(ui, spacer.right() + gap, spacer.center().y);
            }
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
            if layout.focus == Some(i) {
                response.request_focus();
            }
            let response = match tab.disabled_because {
                Some(why) => response.on_disabled_hover_text(why).on_hover_text(why),
                None => response.on_hover_text(tab.tip),
            };
            let hover = response.hovered() && enabled && !selected;
            let label_right = rect.left() + left_room + galley.size().x;
            ui.painter().galley_with_override_text_color(
                pos2(
                    rect.left() + left_room,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                if hover { palette.text } else { colour },
            );
            if let Some(note) = note {
                // On the label's baseline: the caption sits a little lower.
                let at = pos2(
                    label_right + note_room,
                    rect.center().y - note.size().y / 2.0 + theme::stroke::HAIRLINE,
                );
                ui.painter().galley(at, note, palette.text_hint);
            }
            raised::focus_ring(ui.painter(), &response, rect);
            if response.clicked() && !selected {
                picked = Some(i);
            }
            // The underline spans the words, whatever room the tab has
            // around them.
            spans.push((rect.left() + left_room, rect.right() - theme::space::M));
            bottom = rect.bottom();
        }
    });
    // The tab just picked counts as chosen, so the click lands where it is
    // drawn.
    let index = picked.unwrap_or(chosen);
    if index < spans.len() {
        // Glide the *index* and slide between the live tabs, so a strip
        // that itself moves (a panel being resized) carries the underline
        // along rigidly; only a change of choice glides.
        let ctx = ui.ctx().clone();
        let at = motion::glide(&ctx, id.with("underline at"), index as f32, true);
        let (left, right) = slide_between(&spans, at);
        let line = Rect::from_min_max(
            pos2(left, bottom - theme::stroke::FOCUS),
            pos2(right, bottom),
        );
        light::glow(ui.painter(), light::LIT, line, 1.0, palette.accent, 1.0);
        ui.painter().rect_filled(line, 1.0, palette.accent);
    }
    picked
}

/// The left and right edges at position `at` along `spans` (each a tab's or
/// segment's left and right): `at` 0 is the first span, 1 the second, and in
/// between each edge is blended. Clamped to the list; empty gives zeros.
fn slide_between(spans: &[(f32, f32)], at: f32) -> (f32, f32) {
    let Some(last) = spans.len().checked_sub(1) else {
        return (0.0, 0.0);
    };
    let at = at.clamp(0.0, last as f32);
    let i = (at.floor() as usize).min(last);
    let j = (i + 1).min(last);
    let t = at - i as f32;
    let (a, b) = (spans[i], spans[j]);
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A switch flipped while it is scrolled out of view is already in its new
    /// place when it comes back (the outputs menu: "All on", then scroll up,
    /// showed the knobs gliding across).
    #[test]
    fn a_switch_changed_out_of_view_does_not_glide_when_it_returns() {
        let ctx = context();
        let mut id = None;
        let mut frame = |on: bool, visible: bool| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                if !visible {
                    ui.set_clip_rect(Rect::from_min_size(pos2(-100.0, -100.0), vec2(1.0, 1.0)));
                }
                let mut on = on;
                id = Some(switch(ui, &mut on, "Out", true).id);
            });
            output.textures_delta.clear();
        };
        frame(false, true);
        frame(true, false);
        frame(true, true);
        let knob = id.expect("a switch was drawn").with("knob");
        assert_eq!(
            motion::glide(&ctx, knob, 1.0, true),
            1.0,
            "on at once, not gliding from off"
        );
    }

    #[test]
    fn sliding_lands_on_each_span_and_blends_between() {
        let spans = [(0.0, 10.0), (14.0, 44.0), (48.0, 60.0)];
        assert_eq!(slide_between(&spans, 0.0), (0.0, 10.0));
        assert_eq!(slide_between(&spans, 1.0), (14.0, 44.0));
        assert_eq!(slide_between(&spans, 2.0), (48.0, 60.0));
        assert_eq!(slide_between(&spans, 0.5), (7.0, 27.0));
        assert_eq!(slide_between(&spans, 1.5), (31.0, 52.0));
    }

    #[test]
    fn sliding_is_clamped_to_the_list() {
        let spans = [(0.0, 10.0), (14.0, 44.0)];
        assert_eq!(slide_between(&spans, -3.0), (0.0, 10.0));
        assert_eq!(slide_between(&spans, 7.0), (14.0, 44.0));
    }

    /// One frame at `t` seconds of `draw` in a strip whose left edge is
    /// at `x`: every shape it painted.
    fn frame(
        ctx: &egui::Context,
        t: f64,
        x: f32,
        draw: &mut dyn FnMut(&mut Ui),
    ) -> Vec<egui::Shape> {
        let input = egui::RawInput {
            time: Some(t),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let area = Rect::from_min_size(pos2(x, 20.0), vec2(400.0, 100.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| draw(ui));
        });
        output.textures_delta.clear();
        output.shapes.into_iter().map(|s| s.shape).collect()
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        // Fonts take effect from the next frame.
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        ctx
    }

    /// The left edge of the 2 pt accent underline among `shapes`.
    fn underline_left(shapes: &[egui::Shape]) -> f32 {
        shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Rect(r)
                    if r.fill == theme::p().accent && (r.rect.height() - 2.0).abs() < 1e-3 =>
                {
                    Some(r.rect.left())
                }
                _ => None,
            })
            .expect("the underline was painted")
    }

    /// The left edge of the thumb (the only gradient mesh) among `shapes`.
    fn thumb_left(shapes: &[egui::Shape]) -> f32 {
        shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Mesh(m) => Some(m.calc_bounds().left()),
                _ => None,
            })
            .expect("the thumb was painted")
    }

    #[test]
    fn a_moving_tab_strip_carries_the_underline_rigidly() {
        let ctx = context();
        let tab = |label| Tab {
            label,
            note: None,
            disabled_because: None,
            tip: "",
        };
        let list = [tab("Clip"), tab("Mixer")];
        let mut draw = |ui: &mut Ui| {
            tabs_with(ui, EguiId::new("tabs"), 1, &list, TabsLayout::default());
        };
        let first = underline_left(&frame(&ctx, 0.0, 0.0, &mut draw));
        let still = underline_left(&frame(&ctx, 0.016, 0.0, &mut draw));
        assert_eq!(first, still);
        // The strip moves 37 pt (a panel resizes): so does the underline,
        // at once, with no glide of its own.
        let moved = underline_left(&frame(&ctx, 0.032, 37.0, &mut draw));
        assert!((moved - still - 37.0).abs() < 1e-3, "{still} -> {moved}");
    }

    #[test]
    fn a_moving_segmented_control_carries_the_thumb_rigidly() {
        let ctx = context();
        let list = [Choice::new(0, "Pointer"), Choice::new(1, "Draw")];
        let mut draw = |ui: &mut Ui| {
            segmented(ui, EguiId::new("tool"), 1, &list);
        };
        let first = thumb_left(&frame(&ctx, 0.0, 0.0, &mut draw));
        let still = thumb_left(&frame(&ctx, 0.016, 0.0, &mut draw));
        assert_eq!(first, still);
        let moved = thumb_left(&frame(&ctx, 0.032, 37.0, &mut draw));
        assert!((moved - still - 37.0).abs() < 1e-3, "{still} -> {moved}");
    }

    #[test]
    fn icons_sit_on_the_labels_optical_middle() {
        let ctx = context();
        let dot: fn(&egui::Painter, egui::Pos2, egui::Color32) = |painter, at, colour| {
            painter.circle_filled(at, 1.0, colour);
        };
        let list = [
            Choice::new(0, "Pointer").icon(dot),
            Choice::new(1, "Draw").icon(dot),
        ];
        let shapes = frame(&ctx, 0.0, 0.0, &mut |ui| {
            segmented_tool(ui, EguiId::new("tool"), 0, &list, None);
        });
        let dots: Vec<f32> = shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Circle(circle) if circle.radius == 1.0 => Some(circle.center.y),
                _ => None,
            })
            .collect();
        let labels: Vec<f32> = shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => {
                    Some(text.pos.y + crate::fonts::optical_middle(&text.galley))
                }
                _ => None,
            })
            .collect();
        assert_eq!(dots.len(), 2);
        assert_eq!(labels.len(), 2, "no keycap is drawn in the control (KC C)");
        for (dot, label) in dots.iter().zip(&labels) {
            assert!(
                (dot - label).abs() <= 0.5,
                "icon at {dot}, label's middle at {label}"
            );
        }
    }

    /// The colour each label is drawn in, by its text.
    fn label_colours(shapes: &[egui::Shape]) -> Vec<(String, Option<egui::Color32>)> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.override_text_color))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_tool_switch_is_grey_for_pointer_and_lit_for_draw() {
        // The owner's TL C: the resting tool is the neutral thumb, a drawing
        // tool is filled in the accent.
        let ctx = context();
        let list = [Choice::new(0, "Pointer"), Choice::new(1, "Draw")];
        let resting = frame(&ctx, 0.0, 0.0, &mut |ui| {
            segmented_tool(ui, EguiId::new("tool"), 0, &list, None);
        });
        let palette = theme::p();
        let on_accent = theme::lit(Tone::Accent).text;
        let colours = label_colours(&resting);
        assert!(
            colours.contains(&("Pointer".into(), Some(palette.text))),
            "{colours:?}"
        );
        assert!(
            !colours.iter().any(|(_, c)| *c == Some(on_accent)),
            "{colours:?}"
        );
        let drawing = frame(&ctx, 0.0, 0.0, &mut |ui| {
            segmented_tool(ui, EguiId::new("tool"), 1, &list, Some(Tone::Accent));
        });
        let colours = label_colours(&drawing);
        assert!(
            colours.contains(&("Draw".into(), Some(on_accent))),
            "{colours:?}"
        );
    }

    #[test]
    fn one_span_and_none() {
        assert_eq!(slide_between(&[(5.0, 9.0)], 0.0), (5.0, 9.0));
        assert_eq!(slide_between(&[(5.0, 9.0)], 3.0), (5.0, 9.0));
        assert_eq!(slide_between(&[], 1.0), (0.0, 0.0));
    }
}
