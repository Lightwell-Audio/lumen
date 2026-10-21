//! The Mappings window (M7 slice A5): every controller mapping in the project,
//! which knob moves which control, with a way to clear each, and what the
//! controllers last said.

use eframe::egui::{self, Align, Align2, Layout, RichText, pos2};

use crate::widgets::{Dialog, raised};
use crate::{App, theme};

impl App {
    pub(crate) fn mappings_window(&mut self, ctx: &egui::Context) {
        if !self.controllers.window {
            return;
        }
        let mappings = self.document.project().mappings.clone();
        let mut clear = None;
        let mut clear_all = false;
        let mut stop = false;
        let close = Dialog::new("controller mappings", "Controller mappings", 420.0)
            .at(
                pos2(ctx.content_rect().center().x, 72.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                if let Some((track, target)) = self.controllers.learning {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Waiting for a controller to move {}",
                            self.control_name(track, target)
                        ));
                        if raised::button(ui, "Stop").clicked() {
                            stop = true;
                        }
                    });
                    ui.add_space(theme::space::S);
                }
                if mappings.is_empty() {
                    ui.label(
                        RichText::new(
                            "No controller is mapped. Right-click a control, choose \
                             \u{201c}Learn a controller\u{201d}, then move a knob or fader.",
                        )
                        .small()
                        .weak(),
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for m in &mappings {
                            let gone =
                                lumen_project::routing::all_tracks(self.document.project())
                                    .find(|t| t.id == m.track)
                                    .is_none_or(|t| !t.has_target(m.target));
                            let name = if gone {
                                "A control that is gone".to_owned()
                            } else {
                                self.control_name(m.track, m.target)
                            };
                            ui.horizontal(|ui| {
                                let words = format!(
                                    "Controller {} \u{b7} channel {}",
                                    m.number,
                                    m.channel + 1
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let button = raised::button(ui, "Clear");
                                    button.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            format!("Clear the controller of {name}"),
                                        )
                                    });
                                    if button.clicked() {
                                        clear = Some((m.track, m.target));
                                    }
                                    ui.label(RichText::new(words).monospace().small());
                                    ui.with_layout(
                                        Layout::left_to_right(Align::Center),
                                        |ui| ui.add(egui::Label::new(&name).truncate()),
                                    );
                                });
                            });
                        }
                    });
                ui.add_space(theme::space::S);
                if let Some(last) = self.controllers.last {
                    ui.label(
                        RichText::new(format!(
                            "Last heard: controller {} on channel {}, value {}",
                            last.number,
                            last.channel + 1,
                            last.value
                        ))
                        .small()
                        .weak(),
                    );
                }
                let dropped = self.controllers.dropped();
                if dropped > 0 {
                    ui.label(
                        RichText::new(format!(
                            "{dropped} controller messages were lost: they came faster than they could be read."
                        ))
                        .small()
                        .color(theme::p().warning),
                    );
                }
                if !mappings.is_empty() && raised::button(ui, "Clear all").clicked() {
                    clear_all = true;
                }
            });
        if stop {
            self.stop_learning();
        }
        if let Some((track, target)) = clear {
            self.clear_controller(track, target);
        }
        if clear_all {
            let n = self.document.clear_all_mappings();
            self.say(format!("Cleared {n} controller mappings"));
        }
        if close {
            self.controllers.window = false;
        }
    }
}
