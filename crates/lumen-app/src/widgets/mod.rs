//! Design-system controls shared across the app (audit M2): the value
//! slider (drag, Shift for fine, double-click to reset, click and type,
//! arrow keys), floating dialogs and their title row, the fold arrow,
//! gradients and the accent glow line; shortcuts drawn as keycaps in
//! menus and tooltips are in `keys.rs` (M3 slice 7). The track headers use
//! them now; the mixer (M4) and devices reuse them.

use eframe::egui::{
    self, Align2, Color32, FontId, Id as EguiId, Key, Painter, Pos2, Rangef, Rect, Sense, Stroke,
    Ui, Vec2, pos2, vec2,
};

use eframe::emath::GuiRounding as _;

use crate::{fonts, icons, motion, theme};

mod empty;
pub mod fader;
mod keys;
pub mod knob;
mod menu;
pub mod meter;
mod signature;
mod toggle;
pub mod wheel;

pub use empty::{Action as EmptyAction, EmptyState, empty_state};
pub use keys::{
    KEY_LIGHT_ROOM, Keys, Tip, caps_height, caps_width, keycaps, menu_item, menu_toggle,
    modifier_caps, submenu, tip,
};
pub use menu::Menu;
pub(crate) use menu::note_change;
pub use signature::time_signature;
pub use toggle::{Choice, Tab, choice_row, segmented, switch, tabs, toggle_button};

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

/// A peak readout in a few characters, without " dB": "−3.0", "−12", "+6.0".
/// From ten dB up the decimal goes, so the number fits a strip's readout
/// box (the owner's report on overflowing meter numbers). Only for a level
/// above the floor.
pub fn readout_db(db: f32) -> String {
    if db.abs() >= 9.95 {
        format!("{db:+.0}").replace('-', "\u{2212}")
    } else {
        db_text(db).replace(" dB", "")
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
    /// A channel's input trim (M4 slice 5): −24 to +24 dB, filled from the
    /// centre; resets to 0 dB.
    Trim,
    /// A level as a share, 0 to 1 shown as 0–100%, filled from the left;
    /// resets to `default` (the metronome's volume, M4.5).
    Percent { default: f32 },
}

impl SliderKind {
    /// What a double-click sets.
    pub fn reset(self) -> f32 {
        match self {
            SliderKind::Percent { default } => default,
            _ => 0.0,
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            SliderKind::Volume => (-60.0, 6.0),
            SliderKind::Pan => (-1.0, 1.0),
            SliderKind::ClipGain => (-60.0, 12.0),
            SliderKind::Millis { max_ms } => (0.0, max_ms.max(0.0)),
            SliderKind::Trim => (-24.0, 24.0),
            SliderKind::Percent { .. } => (0.0, 1.0),
        }
    }

    /// What a notch of the wheel changes (Shift: a tenth of it): a dB on
    /// levels, 5% on pan, a hundredth of a length.
    pub fn wheel_step(self) -> f32 {
        match self {
            SliderKind::Volume | SliderKind::ClipGain | SliderKind::Trim => 1.0,
            SliderKind::Percent { .. } => 0.05,
            SliderKind::Pan => 0.05,
            SliderKind::Millis { max_ms } => (max_ms / 100.0).max(1.0),
        }
    }

    pub fn text(self, value: f32) -> String {
        match self {
            SliderKind::Volume | SliderKind::ClipGain | SliderKind::Trim => db_text(value),
            SliderKind::Percent { .. } => format!("{:.0}%", value * 100.0),
            SliderKind::Pan => pan_text(value),
            SliderKind::Millis { .. } => ms_text(value),
        }
    }

    /// Reads typed text: "-6", "−6 dB"; "C", "L 30", "R30", "-30".
    pub fn parse(self, text: &str) -> Option<f32> {
        let text = text.trim().replace('\u{2212}', "-").to_ascii_uppercase();
        match self {
            SliderKind::Volume | SliderKind::ClipGain | SliderKind::Trim => {
                text.trim_end_matches("DB").trim().parse().ok()
            }
            SliderKind::Percent { .. } => text
                .trim_end_matches('%')
                .trim()
                .parse::<f32>()
                .ok()
                .map(|p| p / 100.0),
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
    value_slider_styled(
        ui,
        rect,
        id,
        value,
        kind,
        name,
        SliderStyle { look, lift: 0.0 },
    )
}

/// How a [`value_slider_styled`] is drawn.
#[derive(Clone, Copy)]
pub struct SliderStyle {
    /// Its colours, from [`theme::controls`].
    pub look: theme::Controls,
    /// Points the value text is raised, for a slider that draws something
    /// along its bottom (a header's level meter).
    pub lift: f32,
}

/// [`value_slider`] drawn with `style`.
pub fn value_slider_styled(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    value: f32,
    kind: SliderKind,
    name: &str,
    style: SliderStyle,
) -> f32 {
    let SliderStyle { look, lift } = style;
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
                "Volume: drag or wheel (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Pan => {
                "Pan: drag or wheel (Shift for fine), double-click to centre, or click and type L 30 or R 30"
            }
            SliderKind::ClipGain => {
                "Gain: drag (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Millis { .. } => {
                "Length: drag (Shift for fine), double-click for none, or click and type 40 ms or 1.2 s"
            }
            SliderKind::Trim => {
                "Input trim: drag (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Percent { .. } => {
                "Drag or wheel (Shift for fine), double-click for the default, or click and type"
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
        out = kind.reset();
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
    // The wheel, when its gesture started here (the owner's B3).
    let notches = wheel::steps(ui, id, rect);
    if notches != 0.0 {
        out += notches * kind.wheel_step() * if fine { 0.1 } else { 1.0 };
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
    let text = kind.text(out);
    let galley = p.layout_no_wrap(
        text,
        FontId::proportional(theme::size::CAPTION),
        palette.text,
    );
    let text_rect =
        Align2::CENTER_CENTER.anchor_size(rect.center() - vec2(0.0, lift), galley.size());
    match kind {
        SliderKind::Volume
        | SliderKind::ClipGain
        | SliderKind::Millis { .. }
        | SliderKind::Percent { .. } => {
            let x = x_at(out);
            p.rect_filled(
                Rect::from_min_max(rect.min, pos2(x, rect.bottom())),
                radius,
                look.press,
            );
            draw_handle(p, x, rect.shrink2(vec2(0.0, 3.0)).y_range(), text_rect);
        }
        SliderKind::Pan | SliderKind::Trim => {
            let center = rect.center().x;
            let x = x_at(out);
            // Grey at the centre, the accent at the edges (owner request).
            let strength = (out / hi).abs().powf(1.2);
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
            // The fill's moving edge, as the volume's handle.
            if (x - center).abs() >= 1.0 {
                draw_handle(p, x, rect.shrink2(vec2(0.0, 3.0)).y_range(), text_rect);
            }
        }
    }
    p.galley(text_rect.min, galley, palette.text);
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

/// How far from the value's text a slider's handle starts to open, in
/// points.
const HANDLE_OPENS_WITHIN: f32 = 8.0;

/// A slider's handle at `x` over `ys`: a solid line that opens around the
/// value's `text` as it comes near, smoothly with the distance, until only
/// a tick above and below it remains (the owner's choice C, the interface
/// audit's S2: the line ran through the value). Nothing animates by
/// itself: the opening follows the handle.
fn draw_handle(p: &Painter, x: f32, ys: Rangef, text: Rect) {
    let stroke = Stroke::new(2.0, theme::p().start_marker);
    for span in handle_spans(x, ys, text) {
        p.vline(x, span, stroke);
    }
}

/// Half the height a handle opens to around the value: from the capitals'
/// tops to the baseline (about 0.36 of the size each way from the line's
/// middle), with 1.5 pt to spare; not the whole line, which fills the
/// slider.
const HANDLE_GAP_HALF: f32 = 0.36 * theme::size::CAPTION + 1.5;

/// The parts of a handle at `x` over `ys` left drawn around `text`.
fn handle_spans(x: f32, ys: Rangef, text: Rect) -> Vec<Rangef> {
    let away = (x - text.center().x).abs() - text.width() / 2.0 - 3.0;
    let near = (1.0 - away.max(0.0) / HANDLE_OPENS_WITHIN).clamp(0.0, 1.0);
    let open = near * near * (3.0 - 2.0 * near);
    let half = open * HANDLE_GAP_HALF;
    let gap = Rangef::new(text.center().y - half, text.center().y + half);
    if gap.span() < 0.5 {
        return vec![ys];
    }
    [
        Rangef::new(ys.min, gap.min.min(ys.max)),
        Rangef::new(gap.max.max(ys.min), ys.max),
    ]
    .into_iter()
    .filter(|s| s.span() >= 0.5)
    .collect()
}

/// A value shown as text in a field (a strip's volume under its fader, M4
/// slice 5): click to type (Enter applies, Esc cancels), double-click to
/// reset. Its fill follows the surface under it (`controls`). Returns the
/// value.
pub fn value_field(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    value: f32,
    kind: SliderKind,
    name: &str,
    controls: theme::Controls,
) -> f32 {
    let palette = *theme::p();
    let (lo, hi) = kind.range();
    let typing_id = id.with("typing");
    if let Some(mut text) = ui.data(|d| d.get_temp::<String>(typing_id)) {
        let edit = ui.put(
            rect,
            egui::TextEdit::singleline(&mut text)
                .id(id.with("edit"))
                .font(FontId::monospace(theme::size::CAPTION))
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
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::Text)
        .on_hover_text("Click and type a value; double-click to reset");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::TextEdit,
            true,
            format!("{name}: {}", kind.text(value)),
        )
    });
    let mut out = value;
    let notches = wheel::steps(ui, id, rect);
    if notches != 0.0 {
        let fine = ui.input(|i| i.modifiers.shift);
        out = (out + notches * kind.wheel_step() * if fine { 0.1 } else { 1.0 }).clamp(lo, hi);
    }
    if response.double_clicked() {
        ui.data_mut(|d| d.remove::<String>(typing_id));
        out = kind.reset();
    } else if response.clicked() {
        ui.data_mut(|d| d.insert_temp(typing_id, kind.text(value).replace(" dB", "")));
    }
    let hover = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    let p = ui.painter();
    p.rect_filled(
        rect,
        theme::radius::SMALL,
        controls.fill.lerp_to_gamma(controls.hover, hover),
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        kind.text(out).replace(" dB", ""),
        FontId::monospace(theme::size::CAPTION),
        palette.text,
    );
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

/// The same glow along a vertical edge at `x` (the header column's edge).
pub fn draw_edge_glow_vertical(painter: &Painter, x: f32, y: egui::Rangef, t: f32) {
    let accent = theme::p().accent;
    for (width, alpha) in [(6.0, 0.12), (3.0, 0.25)] {
        painter.vline(x, y, Stroke::new(width, accent.gamma_multiply(alpha * t)));
    }
    painter.vline(x, y, Stroke::new(1.5, accent.gamma_multiply(t)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_opens_around_the_value_as_it_nears_it() {
        let ys = Rangef::new(3.0, 17.0);
        // A value centred in a 20 pt slider, as on a strip.
        let text = Rect::from_min_max(pos2(40.0, 4.5), pos2(80.0, 15.5));
        // Far away: one solid line.
        assert_eq!(handle_spans(10.0, ys, text), vec![ys]);
        // Over the text: a tick above and below, clear of the letters.
        let over = handle_spans(60.0, ys, text);
        assert_eq!(over.len(), 2, "{over:?}");
        let middle = text.center().y;
        assert!(
            over[0].max < middle - 4.0 && over[1].min > middle + 4.0,
            "{over:?}"
        );
        assert!(over[0].span() >= 1.5 && over[1].span() >= 1.5, "{over:?}");
        // Coming near, the gap grows with the closeness, never jumps.
        let gap = |x: f32| {
            let spans = handle_spans(x, ys, text);
            if spans.len() == 2 {
                spans[1].min - spans[0].max
            } else {
                0.0
            }
        };
        let mut last = 0.0;
        for i in 0..=40 {
            let g = gap(25.0 + i as f32 * 0.5);
            assert!(g >= last - 1e-4, "{g} after {last}");
            last = g;
        }
        assert!(gap(36.0) > 0.0 && gap(36.0) < gap(37.0));
    }

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
    fn readouts_fit_a_strip_box() {
        assert_eq!(readout_db(-3.0), "\u{2212}3.0");
        assert_eq!(readout_db(6.0), "+6.0");
        assert_eq!(readout_db(-9.9), "\u{2212}9.9");
        assert_eq!(readout_db(-12.3), "\u{2212}12");
        assert_eq!(readout_db(9.96), "+10");
        assert_eq!(readout_db(-48.0), "\u{2212}48");
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
