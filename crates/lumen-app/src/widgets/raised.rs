//! The design system's material (its Material and Button cards, ADR-0052,
//! which replaces ADR-0051's lit look): controls are raised, sink when
//! pressed, and when on are `accent_muted` with an accent hairline; the
//! one primary, a lit S and the chosen tool are filled accent; a lit M is
//! `mute`.
//!
//! - **Raised**: a two-stop vertical gradient, `control_hi` to `control` (a
//!   3% step), a 1 pt `border` (`border_strong` under the pointer) and a
//!   1 pt `edge_light` along the top inside edge. Hover is Ember (`ember`
//!   over the fill), never another fill. No shadow: only windows and
//!   popovers float.
//! - **Pressed**: sunk, flat `control_press` with the 1 pt `shade` lip; the
//!   content moves 1 pt down, the box stays.
//! - **On** ([`Tone`]): `accent_muted`, accent letters, a 38% accent
//!   hairline; filled accent and mute get a stronger edge light; filled
//!   controls do not glow (their fill is their light), only recording does.
//! - **Sunk** ([`paint_sunken`]): `well` with the `shade` lip, for troughs,
//!   grooves and meters; fields add a border ([`paint_field`]).
//! - **Disabled**: a flat `control` fill and `text_disabled` letters.
//!
//! State is never colour alone: callers keep the label, icon or shape.

use eframe::egui::{
    self, Color32, Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2,
    vec2,
};
use eframe::emath::GuiRounding as _;

use crate::theme::{self, Tone, radius, stroke};
use crate::{light, motion};

/// How a control looks this frame.
#[derive(Debug, Clone, Copy)]
pub struct Face {
    /// The colours for the surface under it (`theme::controls`).
    pub surface: theme::Controls,
    /// Pointer over it, 0 to 1 (`motion::hover`).
    pub hover: f32,
    pub pressed: bool,
    /// How lit it is, 0 to 1 (`motion::hover` of the on state, so it
    /// cross-fades).
    pub lit: f32,
    pub tone: Tone,
    pub enabled: bool,
}

impl Face {
    /// A plain control on a surface, at rest.
    pub fn new(surface: theme::Controls) -> Self {
        Self {
            surface,
            hover: 0.0,
            pressed: false,
            lit: 0.0,
            tone: Tone::On,
            enabled: true,
        }
    }

    /// Greyed out: flat, `text_disabled` letters.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn hover(mut self, hover: f32) -> Self {
        self.hover = hover;
        self
    }

    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    /// Lit by `lit` (0 to 1) in `tone`.
    pub fn lit(mut self, lit: f32, tone: Tone) -> Self {
        self.lit = lit;
        self.tone = tone;
        self
    }

    /// The colour of letters and icons on this face.
    pub fn text(&self) -> Color32 {
        let c = theme::p();
        if !self.enabled {
            return c.text_disabled;
        }
        let rest = self
            .surface
            .text
            .lerp_to_gamma(c.text, if self.pressed { 1.0 } else { self.hover });
        rest.lerp_to_gamma(theme::lit(self.tone).text, self.lit)
    }
}

/// Draws a control in `rect` (snap it to pixels first): raised, pressed,
/// on or disabled as `face` says. Returns the rect its content is centred
/// in: 1 pt lower while pressed.
pub fn paint(painter: &Painter, rect: Rect, radius: u8, face: &Face) -> Rect {
    let c = theme::p();
    let r = f32::from(radius);
    if !face.enabled {
        painter.rect_filled(rect, r, face.surface.fill);
        return rect;
    }
    if face.pressed {
        painter.rect_filled(rect, r, face.surface.press);
        lip(painter, rect, radius, c.shade);
        painter.rect_stroke(
            rect,
            r,
            Stroke::new(stroke::HAIRLINE, c.border),
            StrokeKind::Inside,
        );
        return rect.translate(vec2(0.0, stroke::HAIRLINE));
    }
    let on = theme::lit(face.tone);
    let lit = face.lit;
    let top = face.surface.fill_hi.lerp_to_gamma(on.top, lit);
    let bottom = face.surface.fill.lerp_to_gamma(on.bottom, lit);
    if lit > 0.0
        && let Some(colour) = face.tone.glow()
    {
        light::glow(painter, light::FLARE, rect, r, colour, lit);
    }
    gradient(painter, rect, r, top, bottom);
    if face.hover > 0.0 {
        painter.rect_filled(rect, r, c.ember.gamma_multiply(face.hover));
    }
    let edge_light = c.edge_light.lerp_to_gamma(on.light, lit);
    lip(painter, rect, radius, edge_light);
    let border = c
        .border
        .lerp_to_gamma(c.border_strong, face.hover)
        .lerp_to_gamma(on.edge, lit);
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
    rect
}

/// A well cut into the surface (a segmented control's trough, a groove, a
/// meter): `well` with the `shade` lip along its top.
pub fn paint_sunken(painter: &Painter, rect: Rect, radius: u8) {
    painter.rect_filled(rect, f32::from(radius), theme::p().well);
    lip(painter, rect, radius, theme::p().shade);
}

/// A field (a value field, a readout): sunk, with a 1 pt `border` that
/// strengthens to `border_strong` by `hover`.
pub fn paint_field(painter: &Painter, rect: Rect, radius: u8, hover: f32) {
    let c = theme::p();
    paint_sunken(painter, rect, radius);
    painter.rect_stroke(
        rect,
        f32::from(radius),
        Stroke::new(
            stroke::HAIRLINE,
            c.border.lerp_to_gamma(c.border_strong, hover),
        ),
        StrokeKind::Inside,
    );
}

/// A 1 pt line in `colour` along the top inside edge of `rect`, clear of
/// its rounded corners: the edge light on what is raised, the lip on what
/// is sunk.
pub fn lip(painter: &Painter, rect: Rect, radius: u8, colour: Color32) {
    if colour.a() == 0 {
        return;
    }
    let inset = f32::from(radius).min(rect.width() / 2.0).max(1.0);
    painter.hline(
        (rect.left() + inset)..=(rect.right() - inset),
        rect.top() + 1.5 * stroke::HAIRLINE,
        Stroke::new(stroke::HAIRLINE, colour),
    );
}

/// A vertical gradient inside a rounded rectangle: a convex fan with the
/// colour set per vertex.
pub fn gradient(painter: &Painter, rect: Rect, radius: f32, top: Color32, bottom: Color32) {
    const STEPS: usize = 4;
    if rect.height() <= 0.0 || rect.width() <= 0.0 {
        return;
    }
    if top == bottom {
        painter.rect_filled(rect, radius, top);
        return;
    }
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0_f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let mut outline: Vec<Pos2> = Vec::with_capacity(4 * (STEPS + 1));
    for (centre, start) in corners {
        for i in 0..=STEPS {
            let a = (start + 90.0 * i as f32 / STEPS as f32).to_radians();
            outline.push(centre + vec2(a.cos(), a.sin()) * r);
        }
    }
    let at = |y: f32| top.lerp_to_gamma(bottom, ((y - rect.top()) / rect.height()).clamp(0.0, 1.0));
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.center(), at(rect.center().y));
    for p in &outline {
        mesh.colored_vertex(*p, at(p.y));
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
}

/// A raised button named `label`, sized to its letters (24 pt, radius
/// 6).
pub fn button(ui: &mut Ui, label: &str) -> Response {
    button_inked(ui, label, None)
}

/// The one primary button of a view: filled accent, `on_accent` letters.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    button_face(ui, label, None, Some(Tone::Accent))
}

/// [`button`] with its letters in `ink` (Panic's record red), still on one
/// pixel-snapped centred line.
pub fn button_inked(ui: &mut Ui, label: &str, ink: Option<Color32>) -> Response {
    button_face(ui, label, ink, None)
}

fn button_face(ui: &mut Ui, label: &str, ink: Option<Color32>, lit: Option<Tone>) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::size::BODY),
        theme::p().text,
    );
    let size = vec2(
        galley.size().x + 2.0 * theme::space::M,
        theme::size::CONTROL,
    );
    let enabled = ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let mut face = face_of(ui, &response).enabled(enabled);
        if let Some(tone) = lit {
            face = face.lit(1.0, tone);
        }
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let radius = radius::for_height(rect.height());
        let body = paint(ui.painter(), rect, radius, &face);
        let colour = match ink {
            Some(ink) if enabled => ink,
            _ => face.text(),
        };
        centre_galley(ui.painter(), body, galley, colour);
        focus_ring(ui.painter(), &response, rect);
    }
    response
}

/// A raised square button of the control's height named `name`; `draw`
/// paints the icon centred on the point it is given, in the colour given.
pub fn icon_button(
    ui: &mut Ui,
    name: &str,
    draw: impl FnOnce(&Painter, Pos2, Color32),
) -> Response {
    let size = vec2(theme::size::CONTROL, theme::size::CONTROL);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let face = face_of(ui, &response);
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let body = paint(ui.painter(), rect, radius::for_height(rect.height()), &face);
        draw(ui.painter(), body.center(), face.text());
        focus_ring(ui.painter(), &response, rect);
    }
    response
}

/// A plain face for `response`'s hover and press this frame.
pub fn face_of(ui: &Ui, response: &Response) -> Face {
    let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
    Face::new(theme::controls(false))
        .hover(hover)
        .pressed(response.is_pointer_button_down_on())
}

/// Centres `galley` in `rect` on one line, snapped to whole pixels so
/// letters and icons beside it share a baseline.
pub fn centre_galley(
    painter: &Painter,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    colour: Color32,
) {
    let at = (rect.center() - galley.size() / 2.0).round();
    painter.galley_with_override_text_color(at, galley, colour);
}

/// The 2 pt accent ring of keyboard focus, 2 pt outside `rect`, rounded
/// to follow the control (its radius taken from its height).
pub fn focus_ring(painter: &Painter, response: &Response, rect: Rect) {
    if response.has_focus() {
        paint_focus(painter, rect, radius::for_height(rect.height()));
    }
}

/// How far outside its control the focus ring reaches: the gap and the
/// ring's own stroke. Anything that clips its contents (a scroll area) pads
/// them by this much so a focused first or last row keeps its whole ring.
pub const FOCUS_OUTSET: f32 = stroke::FOCUS + stroke::FOCUS;

/// `rect` grown up and down by [`FOCUS_OUTSET`]: what to scroll into view
/// for a focused row, so its ring is not cut at the scroll area's edge.
pub fn ring_rect(rect: Rect) -> Rect {
    rect.expand2(vec2(0.0, FOCUS_OUTSET))
}

/// Runs `add` with [`FOCUS_OUTSET`] of room above and below, for the
/// contents of a vertical scroll area: it clips to its inner rect, which
/// would cut the focus ring of the first and last row. The room is part of
/// the scrolled content (add [`ring_room_height`] to a `max_height`).
pub fn ring_room<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(0, FOCUS_OUTSET as i8))
        .show(ui, add)
        .inner
}

/// The height [`ring_room`] adds to its contents.
pub const fn ring_room_height() -> f32 {
    2.0 * FOCUS_OUTSET
}

/// The focus ring around `rect` of corner `radius`, drawn now.
pub fn paint_focus(painter: &Painter, rect: Rect, radius: u8) {
    painter.rect_stroke(
        rect.expand(FOCUS_OUTSET),
        f32::from(radius) + FOCUS_OUTSET,
        Stroke::new(stroke::FOCUS, theme::p().accent),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_follow_the_lit_amount() {
        let face = Face::new(theme::controls(false));
        let off = face.text();
        let on = face.lit(1.0, Tone::Accent).text();
        assert_eq!(on, theme::lit(Tone::Accent).text);
        assert_ne!(off, on);
        let mute = face.lit(1.0, Tone::Mute).text();
        assert_eq!(mute, theme::p().on_mute);
        assert_eq!(face.enabled(false).text(), theme::p().text_disabled);
    }

    #[test]
    fn only_recording_glows() {
        assert!(Tone::On.glow().is_none());
        assert!(Tone::Accent.glow().is_none());
        assert!(Tone::Mute.glow().is_none());
        assert!(Tone::Record.glow().is_some());
    }
}
