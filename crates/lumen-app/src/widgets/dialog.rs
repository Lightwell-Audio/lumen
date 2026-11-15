use eframe::egui::{self, Align2, Id as EguiId, Key, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use eframe::emath::GuiRounding as _;

use super::{fade_out, raised};
use crate::{fonts, icons, theme};

pub struct Dialog<'a> {
    id: &'a str,
    title: &'a str,
    width: f32,
    pos: Pos2,
    pivot: Align2,
    closable: bool,
    esc_own: bool,
    head: Option<Head<'a>>,
}

type Head<'a> = Box<dyn FnOnce(&mut Ui) + 'a>;

impl<'a> Dialog<'a> {
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

    pub fn head(mut self, head: impl FnOnce(&mut Ui) + 'a) -> Self {
        self.head = Some(Box::new(head));
        self
    }

    pub fn esc_to_caller(mut self) -> Self {
        self.esc_own = false;
        self
    }

    pub fn at(mut self, pos: Pos2, pivot: Align2) -> Self {
        self.pos = pos;
        self.pivot = pivot;
        self
    }

    pub fn question(mut self) -> Self {
        self.closable = false;
        self
    }

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

pub fn window_lip(ctx: &egui::Context, layer: egui::LayerId, rect: Rect) {
    raised::lip(
        &ctx.layer_painter(layer),
        rect,
        theme::radius::XL,
        theme::p().edge_light,
    );
}

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

fn letter_middle(galley: &egui::Galley) -> Option<f32> {
    let placed = galley.rows.first()?;
    let glyph = placed.row.glyphs.iter().find(|g| !g.uv_rect.is_nothing())?;
    Some(placed.pos.y + glyph.pos.y + glyph.uv_rect.offset.y + glyph.uv_rect.size.y / 2.0)
}
