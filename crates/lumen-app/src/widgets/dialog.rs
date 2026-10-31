//! Floating windows: `Dialog`, the window lip and the title row (DESIGN §5).

use eframe::egui::{self, Align2, Id as EguiId, Key, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use eframe::emath::GuiRounding as _;

use super::{fade_out, raised};
use crate::{fonts, icons, theme};

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
    /// Esc is left to the caller (`esc_to_caller`).
    esc_own: bool,
    /// Drawn in the title row instead of the title (`head`).
    head: Option<Head<'a>>,
}

type Head<'a> = Box<dyn FnOnce(&mut Ui) + 'a>;

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
            esc_own: true,
            head: None,
        }
    }

    /// The title row shows `head` (left to right, a control high) instead
    /// of the title, with the close button at the right edge as always.
    /// The title still names the window for the screen reader.
    pub fn head(mut self, head: impl FnOnce(&mut Ui) + 'a) -> Self {
        self.head = Some(Box::new(head));
        self
    }

    /// Esc does not close it: the caller decides (a menu or a field takes
    /// Esc first). The close button still closes it.
    pub fn esc_to_caller(mut self) -> Self {
        self.esc_own = false;
        self
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
        let id = EguiId::new(("dialog", self.id));
        let shown = egui::Window::new(self.title)
            .id(id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .pivot(self.pivot)
            .default_pos(self.pos)
            .show(ctx, |ui| {
                ui.set_width(self.width);
                close = match self.head {
                    Some(head) => head_row(ui, head, self.closable),
                    None => title_row(ui, self.title, self.closable),
                };
                contents(ui);
            });
        if self.closable && self.esc_own && ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        // Remembered as drawn, to fade out when it closes (M4.5).
        if let Some(shown) = shown {
            fade_out::remember(ctx, id, shown.response.layer_id);
            window_lip(ctx, shown.response.layer_id, shown.response.rect);
        }
        if close {
            ctx.request_repaint();
        }
        close
    }
}

/// The 1 pt `edge_light` along the top inside edge of a floating window
/// of `rect` on `layer` (the design system's window: radius XL, a lit top
/// edge). Call it after the window is shown, with the window's layer and
/// rect (`Dialog` does); cards that float on their own windows call it too.
pub fn window_lip(ctx: &egui::Context, layer: egui::LayerId, rect: Rect) {
    raised::lip(
        &ctx.layer_painter(layer),
        rect,
        theme::radius::XL,
        theme::p().edge_light,
    );
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

/// A title row of the caller's own: `head` from the left, a control high,
/// and the close button at the right edge, centred on the row. True when
/// the close button was clicked.
fn head_row(ui: &mut Ui, head: Head<'_>, closable: bool) -> bool {
    let width = ui.available_width();
    let height = theme::size::CONTROL.max(icons::CLOSE_SIZE);
    let close = ui
        .allocate_ui_with_layout(
            vec2(width, height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_size(vec2(width, height));
                let close = closable && icons::close_button(ui).clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), head);
                close
            },
        )
        .inner;
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
