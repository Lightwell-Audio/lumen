//! Design-system controls shared across the app (audit M2): the value
//! slider (drag, Shift for fine, double-click to reset, click and type,
//! arrow keys), floating dialogs and their title row, the fold arrow,
//! gradients and the accent glow line, and shortcuts drawn as keycaps in
//! menus and tooltips (M3 slice 7). The track headers use them now; the
//! mixer (M4) and devices reuse them.

use eframe::egui::{
    self, Align2, Color32, FontId, Id as EguiId, Key, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2,
    pos2, vec2,
};

use eframe::emath::GuiRounding as _;

use crate::{fonts, icons, motion, theme};

/// A floating window in the design system (DESIGN §5): no system title
/// bar; our own title row with the title in the display face and, when it
/// can be closed, the close button at the right edge; a fixed width that
/// content wraps inside, so it never widens; Esc closes it. Every dialog
/// uses this, so they look and behave alike (owner, after slice 4a: the
/// close buttons sat in different places).
pub struct Dialog<'a> {
    id: &'a str,
    title: &'a str,
    width: f32,
    pos: Pos2,
    pivot: Align2,
    closable: bool,
}

impl<'a> Dialog<'a> {
    /// `id` stays the same while the title may change ("Save changes to
    /// Drums?").
    pub fn new(id: &'a str, title: &'a str, width: f32) -> Self {
        Self {
            id,
            title,
            width,
            pos: pos2(0.0, 0.0),
            pivot: Align2::LEFT_TOP,
            closable: true,
        }
    }

    /// Where it first opens: `pivot` of the dialog at `pos`.
    pub fn at(mut self, pos: Pos2, pivot: Align2) -> Self {
        self.pos = pos;
        self.pivot = pivot;
        self
    }

    /// Without a close button or Esc: the dialog asks a question and its
    /// buttons answer it.
    pub fn question(mut self) -> Self {
        self.closable = false;
        self
    }

    /// Shows the dialog; true when the user asked to close it.
    pub fn show(self, ctx: &egui::Context, contents: impl FnOnce(&mut Ui)) -> bool {
        let mut close = false;
        egui::Window::new(self.title)
            .id(EguiId::new(("dialog", self.id)))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .pivot(self.pivot)
            .default_pos(self.pos)
            .show(ctx, |ui| {
                ui.set_width(self.width);
                close = title_row(ui, self.title, self.closable);
                contents(ui);
            });
        if self.closable && ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        close
    }
}

/// A floating window's title row, shared by `Dialog` and the cards that
/// float beside what they edit: the title in the display face, wrapping
/// inside the width, and, when `closable`, the close button at the right
/// edge. The button is centred on the title's first letter, not on its
/// line, whose lower part is room for descenders: centred on the line it
/// sat visibly high (owner, MT-APP-14 run 3). Then the space before the
/// content. True when the close button was clicked.
pub fn title_row(ui: &mut Ui, title: &str, closable: bool) -> bool {
    let width = ui.available_width();
    let reserve = if closable {
        icons::CLOSE_SIZE + ui.spacing().item_spacing.x
    } else {
        0.0
    };
    let galley =
        egui::WidgetText::from(egui::RichText::new(title).font(fonts::display(theme::size::TITLE)))
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Wrap),
                width - reserve,
                egui::TextStyle::Body,
            );
    let middle = letter_middle(&galley).unwrap_or(galley.size().y / 2.0);
    // The button may reach above the text: push both down so it fits.
    let half = icons::CLOSE_SIZE / 2.0;
    let drop = if closable {
        (half - middle).max(0.0)
    } else {
        0.0
    };
    let height = (drop + galley.size().y).max(if closable { drop + middle + half } else { 0.0 });
    let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let text = Rect::from_min_size(row.left_top() + vec2(0.0, drop), galley.size());
    ui.place(text, egui::Label::new(galley));
    let mut close = false;
    if closable {
        let button = Rect::from_center_size(
            pos2(row.right() - half, row.top() + drop + middle),
            Vec2::splat(icons::CLOSE_SIZE),
        )
        .round_to_pixels(ui.pixels_per_point());
        close = ui
            .place(button, |ui: &mut Ui| icons::close_button(ui))
            .clicked();
    }
    ui.add_space(theme::space::XS);
    close
}

/// The height of the middle of `galley`'s first drawn letter, from its
/// top.
fn letter_middle(galley: &egui::Galley) -> Option<f32> {
    let placed = galley.rows.first()?;
    let glyph = placed.row.glyphs.iter().find(|g| !g.uv_rect.is_nothing())?;
    Some(placed.pos.y + glyph.pos.y + glyph.uv_rect.offset.y + glyph.uv_rect.size.y / 2.0)
}

/// "−3.0 dB", "−∞ dB".
pub fn db_text(db: f32) -> String {
    if db <= -59.9 {
        "\u{2212}\u{221E} dB".into()
    } else if db.abs() < 0.05 {
        "0.0 dB".into()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

/// "0 ms", "125 ms", "1.25 s".
pub fn ms_text(ms: f32) -> String {
    if ms < 999.5 {
        format!("{:.0} ms", ms.max(0.0))
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// "C", "L 30", "R 100".
pub fn pan_text(pan: f32) -> String {
    let n = (pan * 100.0).round() as i32;
    match n {
        0 => "C".into(),
        n if n < 0 => format!("L {}", -n),
        n => format!("R {n}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SliderKind {
    /// −60 to +6 dB, filled from the left; resets to 0 dB.
    Volume,
    /// −1 (left) to +1 (right), filled from the centre; resets to centre.
    Pan,
    /// A clip's gain (slice 6e): −60 to +12 dB, filled from the left;
    /// resets to 0 dB.
    ClipGain,
    /// A length in milliseconds, 0 to `max_ms`, filled from the left;
    /// resets to 0 (a fade's length, slice 6e).
    Millis { max_ms: f32 },
}

impl SliderKind {
    pub fn range(self) -> (f32, f32) {
        match self {
            SliderKind::Volume => (-60.0, 6.0),
            SliderKind::Pan => (-1.0, 1.0),
            SliderKind::ClipGain => (-60.0, 12.0),
            SliderKind::Millis { max_ms } => (0.0, max_ms.max(0.0)),
        }
    }

    pub fn text(self, value: f32) -> String {
        match self {
            SliderKind::Volume | SliderKind::ClipGain => db_text(value),
            SliderKind::Pan => pan_text(value),
            SliderKind::Millis { .. } => ms_text(value),
        }
    }

    /// Reads typed text: "-6", "−6 dB"; "C", "L 30", "R30", "-30".
    pub fn parse(self, text: &str) -> Option<f32> {
        let text = text.trim().replace('\u{2212}', "-").to_ascii_uppercase();
        match self {
            SliderKind::Volume | SliderKind::ClipGain => {
                text.trim_end_matches("DB").trim().parse().ok()
            }
            SliderKind::Millis { .. } => {
                let text = text.replace(',', ".");
                if let Some(ms) = text.strip_suffix("MS") {
                    ms.trim().parse().ok()
                } else if let Some(s) = text.strip_suffix('S') {
                    s.trim().parse::<f32>().ok().map(|s| s * 1000.0)
                } else {
                    text.parse().ok()
                }
            }
            SliderKind::Pan => {
                if text == "C" {
                    return Some(0.0);
                }
                let number = |s: &str| s.trim().parse::<f32>().ok().map(|n| n / 100.0);
                if let Some(rest) = text.strip_prefix('L') {
                    number(rest).map(|n| -n)
                } else if let Some(rest) = text.strip_prefix('R') {
                    number(rest)
                } else {
                    number(&text)
                }
            }
        }
    }
}

/// A slider with its value inside it. Drag (Shift for fine steps),
/// double-click to reset, click then type a value (Enter applies, Esc
/// cancels), or use the arrow keys when it has focus. Returns the value.
pub fn value_slider(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    value: f32,
    kind: SliderKind,
    name: &str,
    look: theme::Controls,
) -> f32 {
    let palette = *theme::p();
    let (lo, hi) = kind.range();
    let typing_id = id.with("typing");
    let typing: Option<String> = ui.data(|d| d.get_temp(typing_id));
    if let Some(mut text) = typing {
        // Typing a value.
        let edit = ui.put(
            rect,
            egui::TextEdit::singleline(&mut text)
                .id(id.with("edit"))
                .font(FontId::proportional(theme::size::CAPTION))
                .horizontal_align(egui::Align::Center)
                .margin(vec2(2.0, 2.0)),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if cancel || edit.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            if !cancel && let Some(v) = kind.parse(&text) {
                return v.clamp(lo, hi);
            }
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
        return value;
    }

    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_text(match kind {
            SliderKind::Volume => {
                "Volume: drag (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Pan => {
                "Pan: drag (Shift for fine), double-click to centre, or click and type L 30 or R 30"
            }
            SliderKind::ClipGain => {
                "Gain: drag (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Millis { .. } => {
                "Length: drag (Shift for fine), double-click for none, or click and type 40 ms or 1.2 s"
            }
        });
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            true,
            f64::from(value),
            format!("{name}: {}", kind.text(value)),
        )
    });
    let mut out = value;
    let fine = ui.input(|i| i.modifiers.shift);
    if response.dragged() {
        let dx = response.drag_delta().x;
        out += dx / rect.width() * (hi - lo) * if fine { 0.1 } else { 1.0 };
    }
    if response.clicked() {
        response.request_focus();
    }
    if response.double_clicked() {
        out = 0.0;
    }
    if response.has_focus() {
        let (left, right, typed) = ui.input(|i| {
            let typed = i.events.iter().find_map(|e| match e {
                egui::Event::Text(t)
                    if t.chars()
                        .all(|c| c.is_ascii_digit() || "-.,lLrRcCsS".contains(c)) =>
                {
                    Some(t.clone())
                }
                _ => None,
            });
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp),
                typed,
            )
        });
        let step = (hi - lo) / if fine { 660.0 } else { 66.0 };
        if left {
            out -= step;
        }
        if right {
            out += step;
        }
        if let Some(text) = typed {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
    }
    let out = out.clamp(lo, hi);

    // Drawing.
    let p = ui.painter();
    let hover = motion::hover(
        ui.ctx(),
        id.with("hover"),
        response.hovered() || response.dragged(),
    );
    let radius = theme::radius::SMALL;
    p.rect_filled(rect, radius, look.fill.lerp_to_gamma(look.hover, hover));
    let x_at = |v: f32| rect.left() + (v - lo) / (hi - lo) * rect.width();
    match kind {
        SliderKind::Volume | SliderKind::ClipGain | SliderKind::Millis { .. } => {
            let x = x_at(out);
            p.rect_filled(
                Rect::from_min_max(rect.min, pos2(x, rect.bottom())),
                radius,
                look.press,
            );
            p.vline(
                x,
                rect.shrink2(vec2(0.0, 3.0)).y_range(),
                Stroke::new(2.0, palette.start_marker),
            );
        }
        SliderKind::Pan => {
            let center = rect.center().x;
            let x = x_at(out);
            // Grey at the centre, the accent at the edges (owner request).
            let strength = out.abs().powf(1.2);
            let fill = look.press.lerp_to_gamma(palette.accent, strength);
            p.rect_filled(
                Rect::from_x_y_ranges(center.min(x)..=center.max(x), rect.y_range()),
                0.0,
                fill,
            );
            // The centre mark: short ticks at the edges, clear of the value
            // text (a full line ran through the "C").
            for y in [
                rect.top()..=rect.top() + 3.0,
                rect.bottom() - 3.0..=rect.bottom(),
            ] {
                p.vline(center, y, Stroke::new(1.0, palette.text_hint));
            }
        }
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        kind.text(out),
        FontId::proportional(theme::size::CAPTION),
        palette.text,
    );
    if response.has_focus() {
        p.rect_stroke(
            rect,
            radius,
            Stroke::new(1.5, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
    out
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

/// The glowing accent line on a track's resize handle (`t` fades it).
pub fn draw_edge_glow(painter: &Painter, x: egui::Rangef, y: f32, t: f32) {
    let accent = theme::p().accent;
    for (width, alpha) in [(6.0, 0.12), (3.0, 0.25)] {
        painter.hline(x, y, Stroke::new(width, accent.gamma_multiply(alpha * t)));
    }
    painter.hline(x, y, Stroke::new(1.5, accent.gamma_multiply(t)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_naturally() {
        assert_eq!(db_text(0.0), "0.0 dB");
        assert_eq!(db_text(-3.0), "\u{2212}3.0 dB");
        assert_eq!(db_text(6.0), "+6.0 dB");
        assert_eq!(db_text(-60.0), "\u{2212}\u{221E} dB");
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-0.3), "L 30");
        assert_eq!(pan_text(1.0), "R 100");
    }

    #[test]
    fn typed_values_are_understood() {
        let v = SliderKind::Volume;
        assert_eq!(v.parse("-6"), Some(-6.0));
        assert_eq!(v.parse("\u{2212}4.5 dB"), Some(-4.5));
        assert_eq!(v.parse("loud"), None);
        let p = SliderKind::Pan;
        assert_eq!(p.parse("c"), Some(0.0));
        assert_eq!(p.parse("L 30"), Some(-0.3));
        assert_eq!(p.parse("r100"), Some(1.0));
        assert_eq!(p.parse("-50"), Some(-0.5));
    }
}

/// The mark in front of a hint's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintMark {
    /// A double arrow: dragging resizes.
    Resize,
    /// Four arrows: dragging moves.
    Move,
    None,
}

/// A small chip naming what a drag will do (MT-APP-13 run 1: the owner
/// could not tell whether a drag would move or resize). Placed with its
/// left edge at `at.x`, centred on `at.y`, kept inside `bounds`. Returns
/// where it was drawn.
pub fn draw_hint(painter: &Painter, at: Pos2, mark: HintMark, text: &str, bounds: Rect) -> Rect {
    let palette = theme::p();
    let font = FontId::proportional(theme::size::CAPTION);
    let galley = painter.layout_no_wrap(text.to_owned(), font, palette.text);
    let mark_width = if mark == HintMark::None { 0.0 } else { 12.0 };
    let size = vec2(
        galley.size().x + mark_width + 2.0 * theme::space::XS + 2.0,
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
    let stroke = Stroke::new(1.2, palette.accent);
    let arrow = |from: Pos2, to: Pos2| {
        painter.line_segment([from, to], stroke);
        let d = (to - from).normalized() * 2.5;
        let n = vec2(-d.y, d.x);
        painter.line_segment([to, to - d + n], stroke);
        painter.line_segment([to, to - d - n], stroke);
        painter.line_segment([from, from + d + n], stroke);
        painter.line_segment([from, from + d - n], stroke);
    };
    let centre = pos2(left + 4.5, mid);
    match mark {
        HintMark::Resize => arrow(centre - vec2(4.5, 0.0), centre + vec2(4.5, 0.0)),
        HintMark::Move => {
            arrow(centre - vec2(4.5, 0.0), centre + vec2(4.5, 0.0));
            arrow(centre - vec2(0.0, 4.5), centre + vec2(0.0, 4.5));
        }
        HintMark::None => {}
    }
    painter.galley(
        pos2(left + mark_width, mid - galley.size().y / 2.0),
        galley,
        palette.text,
    );
    rect
}

/// A quiet caption beside the pointer, saying what a drag will do (the
/// owner's choice B2), kept inside `bounds`.
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
/// record arm's): a crisp accent line with two soft layers outside it.
pub fn draw_glow_rect(painter: &Painter, rect: Rect, radius: f32) {
    let accent = theme::p().accent;
    for (grow, width, alpha) in [(3.0, 4.0, 0.12), (1.5, 2.5, 0.28)] {
        painter.rect_stroke(
            rect.expand(grow),
            radius + grow,
            Stroke::new(width, accent.gamma_multiply(alpha)),
            egui::StrokeKind::Middle,
        );
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.5, accent),
        egui::StrokeKind::Inside,
    );
}

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
