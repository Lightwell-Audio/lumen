//! Design-system controls shared across the app (audit M2): the value
//! slider (drag, Shift for fine, double-click to reset, click and type,
//! arrow keys), floating dialogs and their title row, the fold arrow,
//! gradients and the accent glow line; shortcuts drawn as keycaps in
//! menus and tooltips are in `keys.rs` (M3 slice 7). The track headers use
//! them now; the mixer (M4) and devices reuse them.

use eframe::egui::{self, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};

use eframe::emath::GuiRounding as _;

use crate::{light, theme};

mod control_items;
mod dialog;
mod dropdown;
mod empty;
mod fade_out;
pub mod fader;
mod format;
mod keys;
pub mod knob;
pub mod mark;
mod menu;
pub mod meter;
pub mod param;
mod pills;
pub mod raised;
mod scroll_bar;
mod signature;
mod slider;
mod toggle;
pub mod wheel;

pub use control_items::{Ask, control_items};
pub use dialog::{Dialog, title_row, window_lip};
pub use dropdown::Dropdown;
pub use empty::{Action as EmptyAction, EmptyState, empty_state};
pub use fade_out::paint_gone;
pub use format::{db_text, ms_text, pan_text, readout_db};
pub use keys::{
    KEY_LIGHT_ROOM, Keys, Tip, caps_height, caps_width, keycaps, menu_item, menu_toggle,
    modifier_caps, submenu, tip,
};
pub use menu::Menu;
pub(crate) use menu::note_change;
pub use pills::{Pill, pill_button, pill_tabs, pill_toggle};
pub use scroll_bar::{Axis, ScrollBar, scroll_bar};
pub use signature::time_signature;
pub use slider::*;
pub use toggle::{
    Choice, Tab, choice_row, menu_caption, segmented, segmented_styled, switch, switch_noted, tabs,
    tabs_flush, toggle_button,
};

/// `text` as it is when it fits `width`, else cut to fit.
pub fn fit(painter: &egui::Painter, text: &str, font: FontId, width: f32) -> String {
    let whole = painter.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE);
    if whole.size().x <= width {
        text.to_owned()
    } else {
        shorten(painter, text, font, width)
    }
}

/// `text` cut to fit `width`, ending in "…".
pub fn shorten(painter: &egui::Painter, text: &str, font: FontId, width: f32) -> String {
    let mut cut = text.to_owned();
    while !cut.is_empty() {
        cut.pop();
        let galley =
            painter.layout_no_wrap(format!("{cut}\u{2026}"), font.clone(), theme::p().text);
        if galley.size().x <= width {
            break;
        }
    }
    format!("{cut}\u{2026}")
}

/// The fold arrow: pointing right when folded, down when unfolded (`t`
/// glides from 0 to 1).
pub fn draw_arrow(painter: &Painter, center: Pos2, t: f32, color: Color32) {
    let angle = t * std::f32::consts::FRAC_PI_2;
    let (s, c) = angle.sin_cos();
    let rotate = |x: f32, y: f32| center + vec2(x * c - y * s, x * s + y * c);
    painter.add(egui::Shape::convex_polygon(
        vec![rotate(-2.5, -4.0), rotate(3.5, 0.0), rotate(-2.5, 4.0)],
        color,
        Stroke::NONE,
    ));
}

/// The glowing accent line on a track's resize handle (`t` fades it): the
/// [`light::LIT`] level.
pub fn draw_edge_glow(painter: &Painter, x: egui::Rangef, y: f32, t: f32) {
    light::hline(painter, light::LIT, x, y, theme::p().accent, t);
}

/// The same glow along a vertical edge at `x` (the header column's edge).
pub fn draw_edge_glow_vertical(painter: &Painter, x: f32, y: egui::Rangef, t: f32) {
    light::vline(painter, light::LIT, x, y, theme::p().accent, t);
}

/// A horizontal gradient from `left` to `right` across `rect`.
pub fn gradient(painter: &Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = egui::Mesh::default();
    let i = mesh.vertices.len() as u32;
    for (pos, color) in [
        (rect.left_top(), left),
        (rect.right_top(), right),
        (rect.right_bottom(), right),
        (rect.left_bottom(), left),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// A small chip by the pointer with a live value while dragging (a fade's
/// length, a gain; M4 slice 0: what a drag does is said in the info line,
/// not here). Placed with its left edge at `at.x`, centred on `at.y`, kept
/// inside `bounds`. Returns where it was drawn.
pub fn draw_hint(painter: &Painter, at: Pos2, text: &str, bounds: Rect) -> Rect {
    let palette = theme::p();
    let font = FontId::proportional(theme::size::CAPTION);
    let galley = painter.layout_no_wrap(text.to_owned(), font, palette.text);
    let size = vec2(
        galley.size().x + 2.0 * theme::space::XS + 2.0,
        theme::size::CAPTION + 4.0,
    );
    let x = at.x.min(bounds.right() - size.x).max(bounds.left());
    let rect = Rect::from_min_size(pos2(x, at.y - size.y / 2.0), size);
    painter.rect(
        rect,
        theme::radius::SMALL,
        palette.overlay,
        Stroke::new(1.0, palette.border_strong),
        egui::StrokeKind::Inside,
    );
    let left = rect.left() + theme::space::XS;
    let mid = rect.center().y;
    painter.galley(
        pos2(left, mid - galley.size().y / 2.0),
        galley,
        palette.text,
    );
    rect
}

/// A quiet caption beside the pointer, kept inside `bounds`: the tape
/// speed while scrubbing (the owner's choice B2).
pub fn draw_caption(painter: &Painter, at: Pos2, text: &str, colour: Color32, bounds: Rect) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        FontId::proportional(theme::size::CAPTION),
        colour,
    );
    let x = at.x.min(bounds.right() - galley.size().x - 4.0);
    painter.galley(pos2(x, at.y - galley.size().y / 2.0), galley, colour);
}

/// The selection glow around a clip (the owner's choice S3, like the
/// record arm's): the [`light::LIT`] level.
pub fn draw_glow_rect(painter: &Painter, rect: Rect, radius: f32) {
    light::rect(painter, light::LIT, rect, radius, theme::p().accent);
}
