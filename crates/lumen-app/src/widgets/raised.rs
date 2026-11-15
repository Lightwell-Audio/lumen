use eframe::egui::{
    self, Color32, Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2,
    vec2,
};
use eframe::emath::GuiRounding as _;

use crate::theme::{self, Tone, radius, stroke};
use crate::{light, motion};

#[derive(Debug, Clone, Copy)]
pub struct Face {
    pub surface: theme::Controls,
    pub hover: f32,
    pub pressed: bool,
    pub lit: f32,
    pub tone: Tone,
    pub enabled: bool,
}

impl Face {
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

    pub fn lit(mut self, lit: f32, tone: Tone) -> Self {
        self.lit = lit;
        self.tone = tone;
        self
    }

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

pub fn paint_sunken(painter: &Painter, rect: Rect, radius: u8) {
    painter.extend(sunken_shapes(rect, radius));
}

pub fn sunken_shapes(rect: Rect, radius: u8) -> [Shape; 2] {
    [
        Shape::rect_filled(rect, f32::from(radius), theme::p().well),
        lip_shape(rect, radius, theme::p().shade, Edge::Bare),
    ]
}

pub fn paint_field(painter: &Painter, rect: Rect, radius: u8, hover: f32) {
    let c = theme::p();
    painter.rect_filled(rect, f32::from(radius), c.well);
    lip(painter, rect, radius, c.shade);
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

pub fn lip(painter: &Painter, rect: Rect, radius: u8, colour: Color32) {
    painter.add(lip_shape(rect, radius, colour, Edge::UnderBorder));
}

pub(super) fn bare_lip(painter: &Painter, rect: Rect, radius: u8, colour: Color32) {
    painter.add(lip_shape(rect, radius, colour, Edge::Bare));
}

#[derive(Clone, Copy)]
enum Edge {
    Bare,
    UnderBorder,
}

fn lip_shape(rect: Rect, radius: u8, colour: Color32, edge: Edge) -> Shape {
    const STEPS: usize = 6;
    if colour.a() == 0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return Shape::Noop;
    }
    let width = stroke::HAIRLINE;
    let inset = match edge {
        Edge::Bare => 0.5 * width,
        Edge::UnderBorder => 1.5 * width,
    };
    let outer = f32::from(radius)
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);
    let r = outer - inset;
    let top = rect.top() + inset;
    if r <= 0.0 {
        return Shape::hline(
            (rect.left() + inset)..=(rect.right() - inset),
            top,
            Stroke::new(width, colour),
        );
    }
    let centre_y = top + r;
    let corners = [
        (pos2(rect.left() + outer, centre_y), 180.0_f32),
        (pos2(rect.right() - outer, centre_y), 270.0),
    ];
    let mut points = Vec::with_capacity(2 * (STEPS + 1));
    for (centre, start) in corners {
        for i in 0..=STEPS {
            let a = (start + 90.0 * i as f32 / STEPS as f32).to_radians();
            points.push(centre + vec2(a.cos(), a.sin()) * r);
        }
    }
    let colour_at =
        move |_: Rect, at: Pos2| colour.gamma_multiply(((centre_y - at.y) / r).clamp(0.0, 1.0));
    Shape::line(
        points,
        egui::epaint::PathStroke {
            width,
            color: egui::epaint::ColorMode::UV(std::sync::Arc::new(colour_at)),
            kind: StrokeKind::Middle,
        },
    )
}

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

pub fn button(ui: &mut Ui, label: &str) -> Response {
    button_inked(ui, label, None)
}

pub fn button_width(ui: &Ui, label: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::size::BODY),
        theme::p().text,
    );
    galley.size().x + 2.0 * theme::space::M
}

pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    button_face(ui, label, None, Some(Tone::Accent))
}

pub fn button_inked(ui: &mut Ui, label: &str, ink: Option<Color32>) -> Response {
    button_face(ui, label, ink, None)
}

pub fn text_button(ui: &mut Ui, label: &str, font: egui::FontId, on: bool) -> Response {
    button_face_in(ui, label, font, None, on.then_some(Tone::On))
}

fn button_face(ui: &mut Ui, label: &str, ink: Option<Color32>, lit: Option<Tone>) -> Response {
    button_face_in(
        ui,
        label,
        egui::FontId::proportional(theme::size::BODY),
        ink,
        lit,
    )
}

fn button_face_in(
    ui: &mut Ui,
    label: &str,
    font: egui::FontId,
    ink: Option<Color32>,
    lit: Option<Tone>,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, theme::p().text);
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

pub fn face_of(ui: &Ui, response: &Response) -> Face {
    let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
    Face::new(theme::controls(false))
        .hover(hover)
        .pressed(response.is_pointer_button_down_on())
}

pub fn centre_galley(
    painter: &Painter,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    colour: Color32,
) {
    let at = (rect.center() - galley.size() / 2.0).round();
    painter.galley_with_override_text_color(at, galley, colour);
}

pub fn focus_ring(painter: &Painter, response: &Response, rect: Rect) {
    if response.has_focus() {
        paint_focus(painter, rect, radius::for_height(rect.height()));
    }
}

pub const FOCUS_OUTSET: f32 = stroke::FOCUS + stroke::FOCUS;

pub fn ring_rect(rect: Rect) -> Rect {
    rect.expand2(vec2(0.0, FOCUS_OUTSET))
}

pub fn ring_room<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(0, FOCUS_OUTSET as i8))
        .show(ui, add)
        .inner
}

pub const fn ring_room_height() -> f32 {
    2.0 * FOCUS_OUTSET
}

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
