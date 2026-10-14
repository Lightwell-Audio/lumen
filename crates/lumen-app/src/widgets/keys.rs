//! Shortcuts drawn as keycaps (the owner's K5, M3 slice 7), in menu rows
//! and tooltips: what menus and tips draw from the keymap. The menu rows
//! follow the design system's Menu card (ADR-0052): a `radius::MEDIUM`
//! highlight reaching `space::XS` past the row, Ember under the pointer,
//! `accent_muted` with `accent` letters for a toggle that is on.

use eframe::egui::{
    self, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2,
};

use eframe::egui::containers::menu::SubMenuButton;
use eframe::emath::GuiRounding as _;

use super::raised;
use crate::theme::{self, stroke};

/// Room under a keycap for its light.
pub const KEY_LIGHT_ROOM: f32 = 4.0;

/// A shortcut to show (the owner's K5): its modifiers, its key, the light
/// of its command's area, and how it reads ("Ctrl+D").
#[derive(Debug, Clone, PartialEq)]
pub struct Keys {
    pub modifiers: Vec<&'static str>,
    pub key: &'static str,
    pub light: Color32,
    pub text: String,
}

/// Keycaps laid out: each cap's text, whether it is the main key, and the
/// whole size. The size keeps the light's room above the caps as well as
/// below, so whatever centres keycaps centres the caps themselves and the
/// light hangs below (the owner: shortcuts sat high in menu rows).
struct Caps {
    caps: Vec<(std::sync::Arc<egui::Galley>, bool)>,
    cap_height: f32,
    size: Vec2,
}

/// Horizontal padding inside a cap, and vertical.
const CAP_PAD: Vec2 = vec2(6.0, 2.0);
/// The light under the main key: how far in from the cap's sides it starts,
/// how far below the cap, how tall it is, and the halo around it (K5).
const LIGHT_INSET: f32 = 3.0;
const LIGHT_GAP: f32 = 2.0;
const LIGHT_HEIGHT: f32 = 2.0;
const LIGHT_HALO: f32 = 1.5;
/// How strong the halo is, of the light's colour.
const LIGHT_HALO_ALPHA: f32 = 0.3;

fn lay_out_caps(ui: &Ui, modifiers: &[&str], key: Option<&str>) -> Caps {
    let palette = theme::p();
    let font = FontId::proportional(theme::size::KEYCAP);
    let caps: Vec<_> = modifiers
        .iter()
        .map(|m| (*m, false))
        .chain(key.map(|key| (key, true)))
        .map(|(text, main)| {
            let colour = if main {
                palette.text
            } else {
                palette.text_secondary
            };
            // Centred in their row as labels are (egui lays a label out with
            // its row's alignment, a plain galley at the bottom), so a cap's
            // letters sit on the same pixel row as the words beside them
            // (owner: at 100% they sat a pixel high in the info line).
            let job = egui::text::LayoutJob::single_section(
                text.to_owned(),
                egui::TextFormat {
                    font_id: font.clone(),
                    color: colour,
                    valign: egui::Align::Center,
                    ..Default::default()
                },
            );
            let galley = ui.painter().layout_job(job);
            (galley, main)
        })
        .collect();
    let cap_height = caps.iter().map(|(g, _)| g.size().y).fold(0.0, f32::max) + 2.0 * CAP_PAD.y;
    let width = caps
        .iter()
        .map(|(g, _)| g.size().x + 2.0 * CAP_PAD.x)
        .sum::<f32>()
        + theme::space::XS * (caps.len() - 1) as f32;
    Caps {
        caps,
        cap_height,
        size: vec2(width, cap_height + 2.0 * KEY_LIGHT_ROOM),
    }
}

/// Paints `caps` in the room laid out for them, its top left at `at`. On a
/// `lit` surface (a highlighted menu row) the key's outline steps up, as
/// controls do on selected rows, or it would vanish into the highlight.
fn paint_caps(painter: &Painter, ppp: f32, caps: Caps, at: Pos2, light: Color32, lit: bool) {
    let palette = theme::p();
    let at = at + vec2(0.0, KEY_LIGHT_ROOM);
    let mut x = at.x;
    for (galley, main) in caps.caps {
        let cap = Rect::from_min_size(
            pos2(x, at.y),
            vec2(galley.size().x + 2.0 * CAP_PAD.x, caps.cap_height),
        )
        .round_to_pixels(ppp);
        if main {
            painter.rect_stroke(
                cap,
                theme::radius::SMALL,
                Stroke::new(
                    stroke::HAIRLINE,
                    if lit {
                        palette.text_secondary
                    } else {
                        palette.border_strong
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let bar = Rect::from_min_max(
                pos2(cap.left() + LIGHT_INSET, cap.bottom() + LIGHT_GAP),
                pos2(
                    cap.right() - LIGHT_INSET,
                    cap.bottom() + LIGHT_GAP + LIGHT_HEIGHT,
                ),
            )
            .round_to_pixels(ppp);
            painter.rect_filled(
                bar.expand(LIGHT_HALO),
                LIGHT_HEIGHT,
                light.gamma_multiply(LIGHT_HALO_ALPHA),
            );
            painter.rect_filled(bar, LIGHT_HEIGHT / 2.0, light);
        } else {
            painter.rect_filled(cap, theme::radius::SMALL, theme::controls(lit).fill);
        }
        painter.galley(cap.center() - galley.size() / 2.0, galley, palette.text);
        x = cap.right() + theme::space::XS;
    }
}

/// A shortcut drawn as keycaps (the owner's K5, slice 7): each modifier a
/// plain small cap, the key outlined, with a short light under it in
/// `light`, the colour of its command's area. Allocates its own size.
/// Clickable: the shortcut editor and the palette record a new chord when
/// a shortcut is clicked. On a `lit` surface (the palette's chosen row) the
/// caps step up, as in a highlighted menu row.
pub fn keycaps(
    ui: &mut Ui,
    modifiers: &[&str],
    key: &str,
    light: Color32,
    lit: bool,
) -> egui::Response {
    let caps = lay_out_caps(ui, modifiers, Some(key));
    let (rect, response) = ui.allocate_exact_size(caps.size, Sense::click());
    if ui.is_rect_visible(rect) {
        paint_caps(
            ui.painter(),
            ui.pixels_per_point(),
            caps,
            rect.min,
            light,
            lit,
        );
    }
    response
}

/// Modifiers alone as plain keycaps ("Ctrl" before a drag, in the info
/// line), the same size as [`keycaps`] draws them. Draws nothing for none.
pub fn modifier_caps(ui: &mut Ui, modifiers: &[&str]) -> egui::Response {
    if modifiers.is_empty() {
        return ui.allocate_response(Vec2::ZERO, Sense::hover());
    }
    let caps = lay_out_caps(ui, modifiers, None);
    let (rect, response) = ui.allocate_exact_size(caps.size, Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_caps(
            ui.painter(),
            ui.pixels_per_point(),
            caps,
            rect.min,
            Color32::TRANSPARENT,
            false,
        );
    }
    response
}

/// How tall keycaps are with their light's room: the height of a row of
/// them, to centre such a row.
pub fn caps_height(ui: &Ui) -> f32 {
    lay_out_caps(ui, &[], Some("X")).size.y
}

/// How wide keycaps for `modifiers` and `key` are, to plan a row.
pub fn caps_width(ui: &Ui, modifiers: &[&str], key: Option<&str>) -> f32 {
    if modifiers.is_empty() && key.is_none() {
        return 0.0;
    }
    lay_out_caps(ui, modifiers, key).size.x
}

impl Keys {
    /// These keys as keycaps.
    pub fn caps(&self, ui: &mut Ui) -> egui::Response {
        keycaps(ui, &self.modifiers, self.key, self.light, false)
    }

    /// These keys as keycaps that only show them (the info line): not a
    /// control, so nothing to click or name.
    pub fn picture(&self, ui: &mut Ui) {
        let caps = lay_out_caps(ui, &self.modifiers, Some(self.key));
        let (rect, _) = ui.allocate_exact_size(caps.size, Sense::hover());
        if ui.is_rect_visible(rect) {
            paint_caps(
                ui.painter(),
                ui.pixels_per_point(),
                caps,
                rect.min,
                self.light,
                false,
            );
        }
    }
}

/// A menu row: `label` at the left and the shortcut's keycaps at the right
/// (K5). Its accessible name is the label and the shortcut ("Add audio
/// track Ctrl+T"), as egui's menu buttons name theirs. Greyed when not
/// `enabled`.
pub fn menu_item(ui: &mut Ui, label: &str, keys: Option<&Keys>, enabled: bool) -> egui::Response {
    menu_row(ui, label, keys, enabled, None)
}

/// How tall a menu row is: the height of keycaps with their light's room,
/// plus `space::XS` above and below (the Menu card), so rows with and
/// without shortcuts line up and the light stays well inside the highlight.
/// `text` is the height of the row's label.
fn row_height(ui: &Ui, text: f32) -> f32 {
    let keyed = lay_out_caps(ui, &[], Some("X")).size.y;
    text.max(keyed) + 2.0 * theme::space::XS
}

/// The highlight of a menu row at `rect`: it reaches `space::XS` past the
/// row on both sides, so the label and the keycaps sit equally far in from
/// its edges (the owner: the keycaps sat against its right edge).
fn highlight_of(rect: Rect) -> Rect {
    rect.expand2(vec2(theme::space::XS, 0.0))
}

/// A submenu row: `label`, then egui's arrow (⏵) at the right. egui names
/// the button with its arrow ("Colour ⏵"), which screen readers read out;
/// this one is named by its label alone. Use it for every submenu, in
/// place of `ui.menu_button` inside a menu. Drawn as a [`menu_item`] is:
/// egui's button gets no fill of its own and the row paints Ember under
/// the pointer, while its submenu is open and on keyboard focus.
pub fn submenu<R>(
    ui: &mut Ui,
    label: impl Into<String>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::Response {
    let label = label.into();
    let palette = theme::p();
    let enabled = ui.is_enabled();
    let text_height = ui
        .painter()
        .layout_no_wrap(
            label.clone(),
            FontId::proportional(theme::size::BODY),
            palette.text,
        )
        .size()
        .y;
    // Under the button, filled once its rect is known.
    let slot = ui.painter().add(egui::Shape::Noop);
    let (response, inner) = ui
        .scope(|ui| {
            let widgets = &mut ui.visuals_mut().widgets;
            for state in [
                &mut widgets.inactive,
                &mut widgets.hovered,
                &mut widgets.active,
                &mut widgets.open,
            ] {
                state.bg_fill = Color32::TRANSPARENT;
                state.weak_bg_fill = Color32::TRANSPARENT;
                state.bg_stroke = Stroke::NONE;
            }
            let ink = |text: &str| {
                let text = egui::RichText::new(text);
                if enabled {
                    text
                } else {
                    text.color(palette.text_disabled)
                }
            };
            let button = egui::Button::new(ink(label.as_str()))
                .right_text(ink(SubMenuButton::RIGHT_ARROW))
                .min_size(vec2(0.0, row_height(ui, text_height)));
            SubMenuButton::from_button(button).ui(ui, add_contents)
        })
        .inner;
    if enabled && (response.hovered() || response.has_focus() || inner.is_some()) {
        ui.painter().set(
            slot,
            egui::Shape::rect_filled(
                highlight_of(response.rect),
                theme::radius::MEDIUM,
                palette.ember,
            ),
        );
    }
    // The submenu floats too: the same edge light as a menu's.
    if let Some(sub) = inner {
        raised::lip(
            &ui.ctx().layer_painter(sub.response.layer_id),
            sub.response.rect,
            theme::radius::LARGE,
            palette.edge_light,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
    response
}

/// A [`menu_item`] that is on or off (Snap to grid, Triplet): when on, it
/// is `accent_muted` with `accent` letters, as a toggle that is on is
/// everywhere. A click leaves the menu open, so several can be set.
pub fn menu_toggle(ui: &mut Ui, label: &str, keys: Option<&Keys>, on: bool) -> egui::Response {
    menu_row(ui, label, keys, true, Some(on))
}

/// A menu row; `toggle` is whether a toggle is on (`None` for an item,
/// which closes the menu when clicked).
fn menu_row(
    ui: &mut Ui,
    label: &str,
    keys: Option<&Keys>,
    enabled: bool,
    toggle: Option<bool>,
) -> egui::Response {
    let on = toggle == Some(true);
    let palette = theme::p();
    let text_colour = if !enabled {
        palette.text_disabled
    } else if on {
        palette.accent
    } else {
        palette.text
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        text_colour,
    );
    let caps = keys.map(|k| lay_out_caps(ui, &k.modifiers, Some(k.key)));
    let pad = ui.spacing().button_padding;
    let caps_width = caps.as_ref().map_or(0.0, |c| c.size.x + theme::space::M);
    let height = row_height(ui, galley.size().y);
    // A menu is sized in a first pass from what its rows need; only after
    // that does a row fill the menu's width, so the keycaps line up at its
    // right edge without stretching the menu.
    let needed = galley.size().x + caps_width + 2.0 * pad.x;
    let width = if ui.is_sizing_pass() {
        needed
    } else {
        needed.max(ui.available_width())
    };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);
    let name = match keys {
        Some(k) => format!("{label} {}", k.text),
        None => label.to_owned(),
    };
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, &name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let highlight = highlight_of(rect);
        let hovered = enabled && (response.hovered() || response.has_focus());
        if on {
            painter.rect_filled(highlight, theme::radius::MEDIUM, palette.accent_muted);
        }
        // Ember follows the pointer directly (the Menu card).
        if hovered {
            painter.rect_filled(highlight, theme::radius::MEDIUM, palette.ember);
        }
        painter.galley(
            pos2(rect.left() + pad.x, rect.center().y - galley.size().y / 2.0),
            galley,
            text_colour,
        );
        if let (Some(caps), Some(k)) = (caps, keys) {
            let at = pos2(
                rect.right() - pad.x - caps.size.x,
                rect.center().y - caps.size.y / 2.0,
            );
            paint_caps(
                painter,
                ui.pixels_per_point(),
                caps,
                at,
                k.light,
                on || hovered,
            );
        }
    }
    if response.clicked() {
        if toggle.is_none() {
            ui.close();
        } else {
            super::note_change(ui);
        }
    }
    response
}

/// What a button's tooltip adds to its name: its shortcut as keycaps and a
/// hint ("Right-click for punch in and out").
#[derive(Debug, Clone, Copy, Default)]
pub struct Tip<'a> {
    pub keys: Option<&'a Keys>,
    pub hint: Option<&'a str>,
}

/// A tooltip naming what `response` does, with its shortcut as keycaps and
/// a `hint` under it ("Right-click for punch in and out").
pub fn tip(
    response: egui::Response,
    label: &str,
    keys: Option<&Keys>,
    hint: Option<&str>,
) -> egui::Response {
    let show = |ui: &mut Ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            if let Some(keys) = keys {
                keys.caps(ui);
            }
        });
        if let Some(hint) = hint {
            ui.label(egui::RichText::new(hint).color(theme::p().text_secondary));
        }
    };
    if response.enabled() {
        response.on_hover_ui(show)
    } else {
        response.on_disabled_hover_ui(show)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `f` in one frame of a fresh context with the app's fonts.
    fn in_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        // Fonts take effect from the next frame.
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let mut out = None;
        let mut f = Some(f);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                out = Some(f(ui));
            }
        })
        .textures_delta
        .clear();
        out.expect("the frame ran")
    }

    #[test]
    fn rows_with_and_without_keycaps_are_equally_tall() {
        let keys = Keys {
            modifiers: vec!["Ctrl"],
            key: "D",
            light: Color32::RED,
            text: "Ctrl+D".to_owned(),
        };
        let (plain, keyed, expected) = in_ui(|ui| {
            let plain = menu_item(ui, "Duplicate", None, true).rect.height();
            let keyed = menu_item(ui, "Duplicate", Some(&keys), true).rect.height();
            (plain, keyed, caps_height(ui) + 2.0 * theme::space::XS)
        });
        assert_eq!(plain, keyed);
        // The caps with their light's room, plus `space::XS` on each side.
        assert_eq!(keyed, expected);
    }

    #[test]
    fn the_highlight_reaches_past_the_row_on_both_sides() {
        let row = Rect::from_min_size(pos2(10.0, 10.0), vec2(100.0, 30.0));
        let highlight = highlight_of(row);
        assert_eq!(highlight.left(), row.left() - theme::space::XS);
        assert_eq!(highlight.right(), row.right() + theme::space::XS);
        assert_eq!(highlight.height(), row.height());
    }
}
