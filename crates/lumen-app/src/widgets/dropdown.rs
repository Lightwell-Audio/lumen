use eframe::egui::{
    self, FontId, Id, Key, Modifiers, Sense, Ui, WidgetType, pos2, text::LayoutJob, vec2,
};

use eframe::emath::GuiRounding as _;

use super::{Menu, choice_row, raised};
use crate::{motion, theme};

const CHEVRON: f32 = 8.0;

pub struct Dropdown<'a> {
    name: &'a str,
    shown: &'a str,
    width: f32,
}

impl<'a> Dropdown<'a> {
    pub fn new(name: &'a str, shown: &'a str, width: f32) -> Self {
        Self { name, shown, width }
    }

    pub fn show<S: AsRef<str>>(
        self,
        ui: &mut Ui,
        id: Id,
        options: &[S],
        chosen: Option<usize>,
    ) -> Option<usize> {
        let ctx = ui.ctx().clone();
        let (rect, _) =
            ui.allocate_exact_size(vec2(self.width, theme::size::CONTROL), Sense::hover());
        let response = ui.interact(rect, id, Sense::click());
        let name = accessible_name(self.name, self.shown);
        let enabled = ui.is_enabled();
        response
            .widget_info(|| egui::WidgetInfo::labeled(WidgetType::ComboBox, enabled, name.clone()));
        let was_open = ctx
            .data(|d| d.get_temp::<bool>(id.with("was open")))
            .unwrap_or(false);
        let open_before = Menu::is_open_for(&ctx, response.id);
        let mut picked = None;
        keep_arrows(&ctx, response.id);
        if response.has_focus() && !open_before {
            let delta = ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    1
                } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            });
            if delta != 0 {
                picked = stepped(chosen, options.len(), delta);
            }
        }

        let mut menu_pick = None;
        let focus_pending = ctx
            .data(|d| d.get_temp::<bool>(id.with("focus first")))
            .unwrap_or(false);
        let rows_key = id.with("rows");
        let width = self.width;
        Menu::below(&response).show(|ui| {
            let text = theme::p().text;
            let widest = options
                .iter()
                .map(|option| {
                    let galley = ui.fonts_mut(|f| {
                        f.layout_no_wrap(
                            option.as_ref().to_owned(),
                            FontId::proportional(theme::size::BODY),
                            text,
                        )
                    });
                    galley.size().x
                })
                .fold(0.0_f32, f32::max);
            ui.set_width(width.max(widest + 2.0 * theme::space::S));
            let rows_before: Vec<Id> = ui.ctx().data(|d| d.get_temp(rows_key)).unwrap_or_default();
            for row in &rows_before {
                keep_arrows(ui.ctx(), *row);
            }
            let moved = list_keys(ui);
            let mut rows = Vec::with_capacity(options.len());
            egui::ScrollArea::vertical()
                .max_height(theme::size::MENU_LIST)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    raised::ring_room(ui, |ui| {
                        for (i, option) in options.iter().enumerate() {
                            let on = chosen == Some(i);
                            let row = choice_row(ui, option.as_ref(), on);
                            if focus_pending && on {
                                row.request_focus();
                            }
                            if row.gained_focus() {
                                ui.scroll_to_rect(raised::ring_rect(row.rect), None);
                            }
                            rows.push(row.id);
                            if row.clicked() {
                                if !on {
                                    menu_pick = Some(i);
                                }
                                ui.close();
                            }
                        }
                    });
                });
            if moved != 0 {
                move_focus(ui.ctx(), &rows_before, moved);
            }
            ui.ctx().data_mut(|d| d.insert_temp(rows_key, rows));
        });

        let open = Menu::is_open_for(&ctx, response.id);
        ctx.data_mut(|d| {
            d.insert_temp(id.with("was open"), open);
            d.insert_temp(id.with("focus first"), open && !was_open);
        });
        if open && !was_open {
            ctx.request_repaint();
        }
        let clicked_away = ctx.input(|i| i.pointer.any_pressed());
        let escaped = was_open && !open && !clicked_away;
        if escaped {
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        }
        if menu_pick.is_some() || escaped {
            response.request_focus();
        }
        if let Some(i) = menu_pick {
            picked = Some(i);
        }

        if ui.is_rect_visible(rect) {
            let hover = motion::hover(&ctx, response.id.with("hover"), response.hovered() || open);
            let face = raised::Face::new(theme::controls(false))
                .hover(hover)
                .pressed(response.is_pointer_button_down_on())
                .enabled(enabled);
            let rect = rect.round_to_pixels(ui.pixels_per_point());
            let radius = theme::radius::for_height(rect.height());
            let body = raised::paint(ui.painter(), rect, radius, &face);
            let pad = theme::space::S;
            let text_width = (body.width() - 2.0 * pad - CHEVRON - pad).max(1.0);
            let mut job = LayoutJob::simple_singleline(
                self.shown.to_owned(),
                FontId::proportional(theme::size::BODY),
                face.text(),
            );
            job.wrap = egui::text::TextWrapping {
                max_width: text_width,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('\u{2026}'),
            };
            let galley = ui.painter().layout_job(job);
            let at = pos2(body.left() + pad, body.center().y - galley.size().y / 2.0).round();
            ui.painter()
                .galley_with_override_text_color(at, galley, face.text());
            let chevron = pos2(body.right() - pad - CHEVRON / 2.0, body.center().y);
            let chevron_ink = if enabled {
                theme::p().text_secondary
            } else {
                theme::p().text_disabled
            };
            super::draw_arrow(ui.painter(), chevron, 1.0, chevron_ink);
            raised::focus_ring(ui.painter(), &response, rect);
        }
        picked
    }
}

fn accessible_name(name: &str, shown: &str) -> String {
    format!("{name}: {shown}")
}

fn stepped(chosen: Option<usize>, len: usize, delta: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let target = match chosen {
        None if delta > 0 => 0,
        None => len - 1,
        Some(i) => (i as i64 + i64::from(delta)).clamp(0, len as i64 - 1) as usize,
    };
    (Some(target) != chosen).then_some(target)
}

fn keep_arrows(ctx: &egui::Context, id: Id) {
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
}

fn list_keys(ui: &mut Ui) -> i32 {
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            1
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            -1
        } else {
            0
        }
    })
}

fn move_focus(ctx: &egui::Context, rows: &[Id], delta: i32) {
    if rows.is_empty() {
        return;
    }
    let at = rows
        .iter()
        .position(|row| ctx.memory(|m| m.has_focus(*row)));
    let next = match at {
        Some(i) => (i as i64 + i64::from(delta)).clamp(0, rows.len() as i64 - 1) as usize,
        None if delta > 0 => 0,
        None => rows.len() - 1,
    };
    ctx.memory_mut(|m| m.request_focus(rows[next]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_step_within_the_list() {
        assert_eq!(stepped(Some(1), 3, 1), Some(2));
        assert_eq!(stepped(Some(1), 3, -1), Some(0));
        assert_eq!(stepped(Some(2), 3, 1), None);
        assert_eq!(stepped(Some(0), 3, -1), None);
    }

    #[test]
    fn with_nothing_chosen_down_takes_the_first_and_up_the_last() {
        assert_eq!(stepped(None, 3, 1), Some(0));
        assert_eq!(stepped(None, 3, -1), Some(2));
        assert_eq!(stepped(None, 0, 1), None);
    }

    #[test]
    fn names_carry_the_value() {
        assert_eq!(accessible_name("Driver", "ASIO"), "Driver: ASIO");
    }
}
