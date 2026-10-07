//! Undo and redo (M3 slice 3b): the shortcuts, the History window, the
//! short note after each step, and keeping the transport in step with the
//! document.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, RichText, pos2, vec2};

use crate::widgets::Dialog;
use crate::{App, motion, theme, trace};

impl App {
    /// Undo (or redo) one step, with a brief note of what changed.
    pub(crate) fn undo(&mut self, redo: bool) {
        let done = if redo {
            self.document.redo().map(|l| format!("Redo: {l}"))
        } else {
            self.document.undo().map(|l| format!("Undo: {l}"))
        };
        if let Some(text) = done {
            trace::log(|| text.clone());
            self.toast = Some((text, Instant::now()));
        }
    }

    /// Undo and redo from the keyboard: Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y.
    /// Not while typing in a field, which has its own text undo.
    pub(crate) fn undo_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::{Key, KeyboardShortcut, Modifiers};
        let redo = ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Z,
            )) || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
        });
        let undo = ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)));
        if redo {
            self.undo(true);
        } else if undo {
            self.undo(false);
        }
    }

    /// Keeps the app in step with the document: gestures end when the
    /// pointer is released, and the transport follows tempo and meter
    /// changes from edits, undo and redo.
    pub(crate) fn sync_document(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_down()) {
            self.document.seal();
        }
        if self.synced != Some(self.document.state()) {
            self.transport.timeline_changed(self.document.project());
            self.synced = Some(self.document.state());
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.document.title()));
        }
    }

    /// The undo history: every step, the current one highlighted; click a
    /// step to go back (or forward) to it.
    pub(crate) fn history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history {
            return;
        }
        let mut jump = None;
        let close = Dialog::new("history", "History", 240.0)
            .at(
                pos2(ctx.content_rect().right() - 280.0, 72.0),
                Align2::LEFT_TOP,
            )
            .show(ctx, |ui| {
                let (done, undone) = self.document.history().steps();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        let row = |ui: &mut egui::Ui, text: &str, current: bool, future: bool| {
                            let text = if future {
                                RichText::new(text).color(theme::p().text_hint)
                            } else {
                                RichText::new(text)
                            };
                            ui.add_sized(
                                [ui.available_width(), 22.0],
                                egui::Button::selectable(current, text),
                            )
                            .clicked()
                        };
                        if row(ui, "Opened", done.is_empty(), false) {
                            jump = Some(0);
                        }
                        for (i, label) in done.iter().enumerate() {
                            if row(ui, label, i + 1 == done.len(), false) {
                                jump = Some(i + 1);
                            }
                        }
                        for (i, label) in undone.iter().enumerate() {
                            if row(ui, label, false, true) {
                                jump = Some(done.len() + i + 1);
                            }
                        }
                    });
                if done.is_empty() && undone.is_empty() {
                    ui.label(
                        RichText::new("Edits appear here. Click one to go back to it.")
                            .small()
                            .weak(),
                    );
                }
            });
        if let Some(step) = jump {
            self.document.jump_to(step);
        }
        if close {
            self.show_history = false;
        }
    }

    /// A short note after undo, redo, save or open, fading out after a
    /// moment. One line in the arrangement's top-right corner, under the
    /// ruler, clear of where clips usually are; long names are shortened
    /// with "…". `at` is that corner.
    pub(crate) fn toast(&mut self, ctx: &egui::Context, at: egui::Pos2) {
        let Some((text, since)) = &self.toast else {
            return;
        };
        const SHOWN: Duration = motion::NOTE;
        let visible = since.elapsed() < SHOWN;
        let fade = motion::fade(ctx, egui::Id::new("toast"), visible);
        if !visible && fade == 0.0 {
            self.toast = None;
            return;
        }
        if visible {
            ctx.request_repaint_after(SHOWN.saturating_sub(since.elapsed()));
        }
        const MAX_WIDTH: f32 = 340.0;
        let text = text.clone();
        let font = egui::FontId::proportional(theme::size::BODY);
        let natural = ctx
            .fonts_mut(|f| f.layout_no_wrap(text.clone(), font.clone(), theme::p().text))
            .size()
            .x;
        egui::Area::new(egui::Id::new("toast area"))
            .pivot(egui::Align2::RIGHT_TOP)
            // Drops in from just above, and lifts away as it fades.
            .fixed_pos(at - vec2(0.0, (1.0 - fade) * 6.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(fade);
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 5))
                    .show(ui, |ui| {
                        ui.set_width(natural.min(MAX_WIDTH));
                        ui.add(egui::Label::new(RichText::new(text).font(font)).truncate());
                    });
            });
    }
}
