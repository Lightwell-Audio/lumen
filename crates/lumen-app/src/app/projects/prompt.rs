use eframe::egui::{self, RichText, pos2};

use super::open::Next;
use crate::App;
use crate::theme;
use crate::widgets::{Dialog, raised};

impl App {
    pub(crate) fn take_prompt_enter(&mut self, ctx: &egui::Context) {
        self.prompt_enter = self.prompt.is_some()
            && !self.typing
            && ctx.memory(|m| m.focused().is_none())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    }

    pub(crate) fn prompt_window(&mut self, ctx: &egui::Context) {
        let Some(next) = self.prompt.clone() else {
            return;
        };
        let mut choice = None;
        let action = match next {
            Next::New => "starting a new project",
            Next::Open(_) => "opening another project",
            Next::Template(_) => "starting a project from a template",
            Next::Quit => "closing",
        };
        let title = format!("Save changes to {}?", self.document.name());
        Dialog::new("unsaved changes", &title, 380.0)
            .at(
                pos2(ctx.content_rect().center().x, 120.0),
                egui::Align2::CENTER_TOP,
            )
            .question()
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "There are unsaved changes. Save them before {action}?"
                    ))
                    .color(theme::p().text_secondary),
                );
                ui.add_space(theme::space::M);
                ui.horizontal(|ui| {
                    if raised::primary_button(ui, "Save").clicked() {
                        choice = Some(0);
                    }
                    if raised::button(ui, "Don't save").clicked() {
                        choice = Some(1);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if raised::button(ui, "Cancel").clicked() {
                            choice = Some(2);
                        }
                    });
                });
            });
        let entered = std::mem::take(&mut self.prompt_enter);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(2);
        } else if entered && choice.is_none() {
            choice = Some(0);
        }
        match choice {
            Some(0) => {
                self.prompt = None;
                self.after_save = Some(next);
                self.save(ctx, false);
            }
            Some(1) => {
                self.prompt = None;
                self.proceed(ctx, next);
            }
            Some(_) => self.prompt = None,
            None => {}
        }
    }
}
