//! The premium control look (ADR-0046, approved by the owner from canvas
//! mock-ups): controls are raised, lit when on, and sink when pressed.
//!
//! - **Raised**: the control fill with a slight lift from top to bottom, a
//!   1 px light edge along the top inside, a `border` line and a soft drop
//!   shadow. Hover steps to the hover fill; pressed is sunken (an inset
//!   shadow) and drawn 1 px lower.
//! - **Lit** (on): the accent, warning or record gradient with a stronger
//!   light edge and a glow that is one of the light layer's levels
//!   ([`glow_level`]), never a one-off. Letters come from `theme::text_on`.
//! - **Recessed** (mute on): the well's colour, an inset shadow and hint
//!   letters.
//! - **Sunken** ([`paint_sunken`]): the well under a segmented control.
//!
//! State is never colour alone: callers keep the label, icon or shape.

use eframe::egui::{
    self, Color32, Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2,
    vec2,
};
use eframe::emath::GuiRounding as _;

use crate::theme::{self, Tone};
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
}

impl Face {
    /// A plain control on a surface, at rest.
    pub fn new(surface: theme::Controls) -> Self {
        Self {
            surface,
            hover: 0.0,
            pressed: false,
            lit: 0.0,
            tone: Tone::Accent,
        }
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
        let rest = self.surface.text.lerp_to_gamma(theme::p().text, self.hover);
        match self.tone {
            Tone::Recessed => rest.lerp_to_gamma(theme::p().text_hint, self.lit),
            tone => rest.lerp_to_gamma(theme::lit(tone).text, self.lit),
        }
    }
}

/// The light level a lit control of `tone` glows at: the chosen level for
/// the accent and solo, the needs-you level for record.
pub fn glow_level(tone: Tone) -> light::Light {
    match tone {
        Tone::Record => light::FLARE,
        _ => light::LIT,
    }
}

/// Draws a raised control in `rect` (snap it to pixels first) and returns
/// the rect its content is centred in: 1 px lower while pressed.
pub fn paint(painter: &Painter, rect: Rect, radius: u8, face: &Face) -> Rect {
    let c = theme::p();
    let r = f32::from(radius);
    let recessed = face.tone == Tone::Recessed;
    let lit = if recessed { 0.0 } else { face.lit };
    let sunk = if recessed { face.lit } else { 0.0 };
    let rest = face
        .surface
        .fill
        .lerp_to_gamma(face.surface.hover, face.hover);
    let (rest_top, rest_bottom) = theme::lift(rest);
    let fill = theme::lit(face.tone);
    let top = rest_top.lerp_to_gamma(fill.top, lit);
    let bottom = rest_bottom.lerp_to_gamma(fill.bottom, lit);
    let border = c.border.lerp_to_gamma(fill.edge, lit);

    if face.pressed {
        let body = rect.translate(vec2(0.0, 1.0));
        painter.rect_filled(body, r, face.surface.press);
        inset_shadow(painter, body, r, 1.0);
        painter.rect_stroke(body, r, Stroke::new(1.0, border), StrokeKind::Inside);
        return body;
    }
    if sunk > 0.0 {
        // Mute on: the well, sunk in, with a fading raised look.
        let body = rect;
        painter.rect_filled(body, r, rest.lerp_to_gamma(c.well, sunk));
        inset_shadow(painter, body, r, sunk);
        painter.rect_stroke(body, r, Stroke::new(1.0, c.border), StrokeKind::Inside);
        if sunk < 1.0 {
            painter.rect_stroke(
                body.shrink(1.0),
                r,
                Stroke::new(1.0, theme::edge_light().gamma_multiply(1.0 - sunk)),
                StrokeKind::Inside,
            );
        }
        return body;
    }
    // Drop shadow, then a glow when lit.
    painter.rect_filled(
        rect.translate(vec2(0.0, 1.0)),
        r,
        theme::control_shadow(1.0),
    );
    if lit > 0.0 {
        light::glow(
            painter,
            glow_level(face.tone),
            rect,
            r,
            face.tone.colour().unwrap_or(c.accent),
            lit,
        );
    }
    painter.rect_filled(rect, r, top.lerp_to_gamma(bottom, 0.5));
    gradient(painter, rect.shrink(1.0), (r - 1.0).max(0.0), top, bottom);
    // The 1 px light edge along the top, inside the border.
    let edge = theme::edge_light().lerp_to_gamma(theme::edge_light_lit(), lit);
    painter.hline(
        (rect.left() + r)..=(rect.right() - r),
        rect.top() + 1.5,
        Stroke::new(1.0, edge),
    );
    painter.rect_stroke(rect, r, Stroke::new(1.0, border), StrokeKind::Inside);
    rect
}

/// A well that controls sit sunken in (a segmented control's track): the
/// `well` colour, an inset shadow and a hairline of light along the bottom.
pub fn paint_sunken(painter: &Painter, rect: Rect, radius: u8) {
    let r = f32::from(radius);
    painter.rect_filled(rect, r, theme::p().well);
    inset_shadow(painter, rect, r, 1.0);
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(1.0, theme::p().border),
        StrokeKind::Inside,
    );
}

/// A shadow just inside the top edge of `rect`, two lines deep.
fn inset_shadow(painter: &Painter, rect: Rect, radius: f32, scale: f32) {
    let x = (rect.left() + radius)..=(rect.right() - radius);
    painter.hline(
        x.clone(),
        rect.top() + 1.5,
        Stroke::new(1.0, theme::control_shadow(1.4 * scale)),
    );
    painter.hline(
        x,
        rect.top() + 2.5,
        Stroke::new(1.0, theme::control_shadow(0.6 * scale)),
    );
}

/// A vertical gradient inside a rounded rectangle: a convex fan with the
/// colour set per vertex.
fn gradient(painter: &Painter, rect: Rect, radius: f32, top: Color32, bottom: Color32) {
    const STEPS: usize = 4;
    if rect.height() <= 0.0 || rect.width() <= 0.0 {
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

/// A raised button named `label`, sized to its letters.
#[allow(
    dead_code,
    reason = "foundation API: call sites migrate to it in the next slices (ADR-0046)"
)]
pub fn button(ui: &mut Ui, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::size::BODY),
        theme::p().text,
    );
    let size = vec2(
        galley.size().x + 2.0 * theme::space::M,
        theme::size::CONTROL,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if ui.is_rect_visible(rect) {
        let face = face_of(ui, &response);
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let body = paint(ui.painter(), rect, theme::radius::SMALL, &face);
        centre_galley(ui.painter(), body, galley, face.text());
        focus_ring(ui.painter(), &response, rect);
    }
    response
}

/// A raised square button of the control's height named `name`; `draw`
/// paints the icon centred on the point it is given, in the colour given.
#[allow(
    dead_code,
    reason = "foundation API: call sites migrate to it in the next slices (ADR-0046)"
)]
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
        let body = paint(ui.painter(), rect, theme::radius::SMALL, &face);
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

/// The 2 pt accent ring of keyboard focus, around `rect`.
pub fn focus_ring(painter: &Painter, response: &Response, rect: Rect) {
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            theme::radius::SMALL,
            Stroke::new(2.0, theme::p().accent),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glows_are_light_layer_levels() {
        assert_eq!(glow_level(Tone::Accent), light::LIT);
        assert_eq!(glow_level(Tone::Warning), light::LIT);
        assert_eq!(glow_level(Tone::Record), light::FLARE);
    }

    #[test]
    fn letters_follow_the_lit_amount() {
        let face = Face::new(theme::controls(false));
        let off = face.text();
        let on = face.lit(1.0, Tone::Accent).text();
        assert_eq!(on, theme::lit(Tone::Accent).text);
        assert_ne!(off, on);
        let mute = face.lit(1.0, Tone::Recessed).text();
        assert_eq!(mute, theme::p().text_hint);
    }
}
