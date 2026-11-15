use eframe::egui::{
    self, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2,
};

use eframe::egui::containers::menu::SubMenuButton;
use eframe::emath::GuiRounding as _;

use super::raised;
use crate::icons::{self, Icon};
use crate::theme::{self, stroke};

pub const KEY_LIGHT_ROOM: f32 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Keys {
    pub modifiers: Vec<&'static str>,
    pub key: &'static str,
    pub light: Color32,
    pub text: String,
}

struct Caps {
    caps: Vec<(std::sync::Arc<egui::Galley>, bool, f32)>,
    cap_height: f32,
    size: Vec2,
}

const CAP_PAD: Vec2 = vec2(6.0, 2.0);
const LIGHT_INSET: f32 = 3.0;
const LIGHT_GAP: f32 = 2.0;
const LIGHT_HEIGHT: f32 = 2.0;
const LIGHT_HALO: f32 = 1.5;
const LIGHT_HALO_ALPHA: f32 = 0.3;

fn lay_out_caps(ui: &Ui, modifiers: &[&str], key: Option<&str>) -> Caps {
    let palette = theme::p();
    let font = FontId::proportional(theme::size::KEYCAP);
    let letter_width = ui
        .painter()
        .layout_no_wrap("W".to_owned(), font.clone(), palette.text)
        .size()
        .x;
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
            let width = if main && text.len() == 1 && text.as_bytes()[0].is_ascii_alphabetic() {
                letter_width
            } else {
                galley.size().x
            } + 2.0 * CAP_PAD.x;
            (galley, main, width)
        })
        .collect();
    let cap_height = caps.iter().map(|(g, _, _)| g.size().y).fold(0.0, f32::max) + 2.0 * CAP_PAD.y;
    let width = caps.iter().map(|(_, _, width)| *width).sum::<f32>()
        + theme::space::XS * (caps.len() - 1) as f32;
    Caps {
        caps,
        cap_height,
        size: vec2(width, cap_height + 2.0 * KEY_LIGHT_ROOM),
    }
}

fn paint_caps(painter: &Painter, ppp: f32, caps: Caps, at: Pos2, light: Color32, lit: bool) {
    let palette = theme::p();
    let at = at + vec2(0.0, KEY_LIGHT_ROOM);
    let mut x = at.x;
    for (galley, main, width) in caps.caps {
        let cap =
            Rect::from_min_size(pos2(x, at.y), vec2(width, caps.cap_height)).round_to_pixels(ppp);
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

pub fn caps_height(ui: &Ui) -> f32 {
    lay_out_caps(ui, &[], Some("X")).size.y
}

pub fn caps_width(ui: &Ui, modifiers: &[&str], key: Option<&str>) -> f32 {
    if modifiers.is_empty() && key.is_none() {
        return 0.0;
    }
    lay_out_caps(ui, modifiers, key).size.x
}

impl Keys {
    pub fn caps(&self, ui: &mut Ui) -> egui::Response {
        keycaps(ui, &self.modifiers, self.key, self.light, false)
    }

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

pub fn menu_item(ui: &mut Ui, label: &str, keys: Option<&Keys>, enabled: bool) -> egui::Response {
    menu_row(ui, label, keys, enabled, None, None)
}

fn row_height(text: f32) -> f32 {
    theme::size::MENU_ROW.max(text + 2.0 * theme::space::XS)
}

fn touching<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let gap = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let out = add(ui);
    ui.spacing_mut().item_spacing.y = gap;
    out
}

fn caps_top(centre_y: f32, size: Vec2) -> f32 {
    centre_y - size.y / 2.0 - KEY_LIGHT_ROOM / 2.0
}

fn highlight_of(rect: Rect) -> Rect {
    rect.expand2(vec2(theme::space::XS, 0.0))
}

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
    let slot = ui.painter().add(egui::Shape::Noop);
    let gap = ui.spacing().item_spacing.y;
    let (response, inner) = touching(ui, |ui| {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = gap;
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
                .min_size(vec2(0.0, row_height(text_height)));
            SubMenuButton::from_button(button).ui(ui, add_contents)
        })
        .inner
    });
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
    if let Some(sub) = inner {
        raised::lip(
            &ui.ctx().layer_painter(sub.response.layer_id),
            sub.response.rect,
            theme::radius::LARGE,
            palette.edge_light,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
    raised::focus_ring(
        ui.painter(),
        &response,
        response.rect.shrink(raised::FOCUS_OUTSET),
    );
    response
}

pub fn menu_toggle(ui: &mut Ui, label: &str, keys: Option<&Keys>, on: bool) -> egui::Response {
    menu_row(ui, label, keys, true, Some(on), None)
}

pub fn menu_toggle_icon(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    keys: Option<&Keys>,
    on: bool,
) -> egui::Response {
    menu_row(ui, label, keys, true, Some(on), Some(icon))
}

fn menu_row(
    ui: &mut Ui,
    label: &str,
    keys: Option<&Keys>,
    enabled: bool,
    toggle: Option<bool>,
    icon: Option<Icon>,
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
    let height = row_height(galley.size().y);
    let icon_width = icon.map_or(0.0, |_| theme::size::MENU_ICON + theme::space::S);
    let needed = icon_width + galley.size().x + caps_width + 2.0 * pad.x;
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
    let (rect, response) = touching(ui, |ui| ui.allocate_exact_size(vec2(width, height), sense));
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
        if hovered {
            painter.rect_filled(highlight, theme::radius::MEDIUM, palette.ember);
        }
        raised::focus_ring(painter, &response, rect.shrink(raised::FOCUS_OUTSET));
        if let Some(icon) = icon {
            let square = Rect::from_center_size(
                pos2(
                    rect.left() + pad.x + theme::size::MENU_ICON / 2.0,
                    rect.center().y,
                ),
                Vec2::splat(theme::size::MENU_ICON),
            );
            painter.extend(icons::shapes(icon, square, text_colour));
        }
        painter.galley(
            pos2(
                rect.left() + pad.x + icon_width,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            text_colour,
        );
        if let (Some(caps), Some(k)) = (caps, keys) {
            let at = pos2(
                rect.right() - pad.x - caps.size.x,
                caps_top(rect.center().y, caps.size),
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

#[derive(Debug, Clone, Copy, Default)]
pub struct Tip<'a> {
    pub keys: Option<&'a Keys>,
    pub hint: Option<&'a str>,
    pub command: Option<crate::commands::Command>,
}

pub fn tip(
    response: egui::Response,
    label: &str,
    keys: Option<&Keys>,
    hint: Option<&str>,
) -> egui::Response {
    let show = |ui: &mut Ui| {
        ui.set_max_width(theme::size::TIP_WIDTH);
        ui.horizontal(|ui| {
            ui.label(label);
            if let Some(keys) = keys {
                keys.caps(ui);
            }
        });
        if let Some(hint) = hint {
            ui.add(
                egui::Label::new(egui::RichText::new(hint).color(theme::p().text_secondary)).wrap(),
            );
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

    fn in_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
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
    fn single_letter_keycaps_have_equal_widths() {
        let widths = in_ui(|ui| {
            ('A'..='Z')
                .map(|letter| {
                    keycaps(ui, &[], &letter.to_string(), theme::p().accent, false)
                        .rect
                        .width()
                })
                .collect::<Vec<_>>()
        });
        assert!(widths.iter().all(|width| *width == widths[0]), "{widths:?}");
    }

    #[test]
    fn rows_with_and_without_keycaps_are_equally_tall() {
        let keys = Keys {
            modifiers: vec!["Ctrl"],
            key: "D",
            light: Color32::RED,
            text: "Ctrl+D".to_owned(),
        };
        let (plain, keyed, pitch) = in_ui(|ui| {
            let plain = menu_item(ui, "Duplicate", None, true).rect;
            let keyed = menu_item(ui, "Duplicate", Some(&keys), true).rect;
            (plain.height(), keyed.height(), keyed.top() - plain.top())
        });
        assert_eq!(plain, keyed);
        assert_eq!(keyed, theme::size::MENU_ROW);
        assert_eq!(pitch, theme::size::MENU_ROW);
    }

    #[test]
    fn a_cap_and_its_light_are_centred_together_in_a_row() {
        let caps = in_ui(|ui| lay_out_caps(ui, &["Ctrl"], Some("D")));
        let top = caps_top(100.0, caps.size) + KEY_LIGHT_ROOM;
        let bottom = top + caps.cap_height + LIGHT_GAP + LIGHT_HEIGHT;
        assert!(
            ((top + bottom) / 2.0 - 100.0).abs() < 1e-3,
            "{top} {bottom}"
        );
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
