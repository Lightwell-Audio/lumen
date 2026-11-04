//! The first-run window's Plugins step (ADR-0115): the scan live and the
//! folder rows.

use super::*;

impl App {
    pub(super) fn welcome_plugins(&mut self, ui: &mut Ui) {
        self.welcome_intro(
            ui,
            "Bring your plugins",
            "VST3 plugins in the standard folders are found on their own. Add a folder if yours live elsewhere.",
            None,
        );
        self.scan_state(ui);
        self.plugin_folder_rows(ui, &Marks::default());
    }

    /// The scan as it stands: progress and Cancel, what is ready, what
    /// waits, or why there is no scan.
    pub(super) fn scan_state(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        section(ui, "Scanning");
        if self.safe.on {
            warning(
                ui,
                "Safe mode is on, so no plugins load and none are looked for.",
                0.0,
            );
            let leave = Command::LeaveSafeMode;
            if self.availability(&ctx, leave).is_ok()
                && widgets::raised::button(ui, leave.name())
                    .on_hover_text(leave.info().about)
                    .clicked()
            {
                self.run(&ctx, leave);
            }
            return;
        }
        if self.plugins.is_off() {
            warning(ui, OFF_THIS_RUN, 0.0);
            return;
        }
        let ready = welcome::plugins_ready(self.plugins.catalog());
        let blocked = self.plugins.catalog().blocked().len();
        match self.plugins.status().clone() {
            Scan::Off => {
                warning(ui, OFF_THIS_RUN, 0.0);
            }
            Scan::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading the plugin list\u{2026}");
                });
            }
            Scan::Scanning { done, total } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if total == 0 {
                        "Looking for plugins\u{2026}".to_owned()
                    } else {
                        format!("Checking plugin files: {done} of {total}")
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::raised::button(ui, "Cancel")
                            .on_hover_text("Stop scanning; what was found stays")
                            .clicked()
                        {
                            self.plugins.cancel();
                        }
                    });
                });
                progress(
                    ui,
                    if total == 0 {
                        0.0
                    } else {
                        done as f32 / total as f32
                    },
                );
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            Scan::Failed(why) => {
                warning(ui, &why, 0.0);
                if widgets::raised::button(ui, "Retry")
                    .on_hover_text("Try the scan again")
                    .clicked()
                {
                    self.plugins.rescan(
                        lumen_plugin_catalog::scan::Mode::Changed,
                        &self.settings.plugins,
                        &ctx,
                    );
                }
            }
            Scan::Idle => {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} ready to use",
                        welcome::count(ready, "plugin", "plugins")
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.plugins.waiting().is_none()
                            && widgets::raised::button(ui, "Rescan")
                                .on_hover_text("Look for new and changed plugins")
                                .clicked()
                        {
                            self.plugins.rescan(
                                lumen_plugin_catalog::scan::Mode::Changed,
                                &self.settings.plugins,
                                &ctx,
                            );
                        }
                    });
                });
            }
        }
        if blocked > 0 {
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(
                    vec2(theme::size::MENU_ICON, theme::size::MENU_ICON),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .extend(icons::shapes(Icon::Warning, icon, theme::p().warning));
                ui.label(format!(
                    "{} could not be loaded",
                    welcome::count(blocked, "plugin", "plugins")
                ));
                if ui
                    .link("See them")
                    .on_hover_text("Opens the Plugins page of Preferences, with Retry")
                    .clicked()
                    && self.availability(&ctx, Command::PreferencesPlugins).is_ok()
                {
                    self.run(&ctx, Command::PreferencesPlugins);
                }
            });
        }
        if let Some(waiting) = self.plugins.waiting() {
            for folder in waiting.folders() {
                let folder = folder.display().to_string();
                ui.add(
                    egui::Label::new(RichText::new(format!("Waiting to check {folder}")).weak())
                        .truncate(),
                )
                .on_hover_text(format!(
                    "{folder} is checked as soon as the scan under way ends"
                ));
            }
        }
    }

    // ── Ready ───────────────────────────────────────────────────────────
}

/// A thin progress bar the width of the step.
fn progress(ui: &mut Ui, fraction: f32) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::size::PROGRESS_BAR),
        egui::Sense::hover(),
    );
    let p = theme::p();
    let round = theme::size::PROGRESS_BAR / 2.0;
    ui.painter().rect_filled(rect, round, p.well);
    let filled = egui::Rect::from_min_size(
        rect.min,
        vec2(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    ui.painter().rect_filled(filled, round, p.accent);
}
