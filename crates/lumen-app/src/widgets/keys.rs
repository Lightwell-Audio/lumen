//! Shortcuts drawn as keycaps (the owner's K5, M3 slice 7), in menu rows
//! and tooltips: what menus and tips draw from the keymap.

use eframe::egui::{
    self, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2,
};

use eframe::emath::GuiRounding as _;

use crate::theme;

/// Room under a keycap for its light.
const KEY_LIGHT_ROOM: f32 = 4.0;

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
/// whole size.
struct Caps {
    caps: Vec<(std::sync::Arc<egui::Galley>, bool)>,
    cap_height: f32,
    size: Vec2,
}

/// Horizontal padding inside a cap, and vertical.
const CAP_PAD: Vec2 = vec2(6.0, 2.0);

fn lay_out_caps(ui: &Ui, modifiers: &[&str], key: &str) -> Caps {
    let palette = theme::p();
    let font = FontId::proportional(theme::size::CAPTION);
    let caps: Vec<_> = modifiers
        .iter()
        .map(|m| (*m, false))
        .chain(std::iter::once((key, true)))
        .map(|(text, main)| {
            let colour = if main {
                palette.text
            } else {
                palette.text_secondary
            };
            let galley = ui
                .painter()
                .layout_no_wrap(text.to_owned(), font.clone(), colour);
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
        size: vec2(width, cap_height + KEY_LIGHT_ROOM),
    }
}

fn paint_caps(painter: &Painter, ppp: f32, caps: Caps, at: Pos2, light: Color32) {
    let palette = theme::p();
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
                Stroke::new(1.0, palette.border_strong),
                egui::StrokeKind::Inside,
            );
            let bar = Rect::from_min_max(
                pos2(cap.left() + 3.0, cap.bottom() + 2.0),
                pos2(cap.right() - 3.0, cap.bottom() + 4.0),
            )
            .round_to_pixels(ppp);
            painter.rect_filled(bar.expand(1.5), 2.0, light.gamma_multiply(0.3));
            painter.rect_filled(bar, 1.0, light);
        } else {
            painter.rect_filled(cap, theme::radius::SMALL, palette.control);
        }
        painter.galley(cap.center() - galley.size() / 2.0, galley, palette.text);
        x = cap.right() + theme::space::XS;
    }
}

/// A shortcut drawn as keycaps (the owner's K5, slice 7): each modifier a
/// plain small cap, the key outlined, with a short light under it in
/// `light`, the colour of its command's area. Allocates its own size.
/// Clickable: the shortcut editor and the palette record a new chord when
/// a shortcut is clicked.
pub fn keycaps(ui: &mut Ui, modifiers: &[&str], key: &str, light: Color32) -> egui::Response {
    let caps = lay_out_caps(ui, modifiers, key);
    let (rect, response) = ui.allocate_exact_size(caps.size, Sense::click());
    if ui.is_rect_visible(rect) {
        paint_caps(ui.painter(), ui.pixels_per_point(), caps, rect.min, light);
    }
    response
}

impl Keys {
    /// These keys as keycaps.
    pub fn caps(&self, ui: &mut Ui) -> egui::Response {
        keycaps(ui, &self.modifiers, self.key, self.light)
    }
}

/// A menu row: `label` at the left and the shortcut's keycaps at the right
/// (K5). Its accessible name is the label and the shortcut ("Add audio
/// track Ctrl+T"), as egui's menu buttons name theirs. Greyed when not
/// `enabled`.
pub fn menu_item(ui: &mut Ui, label: &str, keys: Option<&Keys>, enabled: bool) -> egui::Response {
    menu_row(ui, label, keys, enabled, None)
}

/// A [`menu_item`] that is on or off (Snap to grid, Triplet): when on, it
/// has the selected fill, as egui's selectable items do. A click leaves
/// the menu open, so several can be set.
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
    let text_colour = if enabled {
        palette.text
    } else {
        palette.text_disabled
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        text_colour,
    );
    let caps = keys.map(|k| lay_out_caps(ui, &k.modifiers, k.key));
    let pad = ui.spacing().button_padding;
    let caps_width = caps
        .as_ref()
        .map_or(0.0, |c| c.size.x + 2.0 * theme::space::M);
    let height = galley.size().y.max(caps.as_ref().map_or(0.0, |c| c.size.y)) + 2.0 * pad.y;
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
        if on {
            painter.rect_filled(rect, theme::radius::SMALL, palette.accent_muted);
        }
        if enabled && (response.hovered() || response.has_focus()) {
            painter.rect_filled(
                rect,
                theme::radius::SMALL,
                ui.visuals().widgets.hovered.weak_bg_fill,
            );
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
            paint_caps(painter, ui.pixels_per_point(), caps, at, k.light);
        }
    }
    if response.clicked() && toggle.is_none() {
        ui.close();
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
