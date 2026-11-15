mod files;
mod pages;
mod window;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use crate::app::projects::Next;
use crate::commands::Command;
use crate::{App, fonts, motion, settings, theme, widgets};

impl App {
    pub(crate) fn app_menu(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.appearance.clone();
        let open = widgets::Menu::is_open_for(ui.ctx(), ui.next_auto_id());
        let button = widgets::raised::text_button(
            ui,
            PRODUCT_NAME,
            fonts::display(theme::size::WORDMARK),
            open,
        )
        .on_hover_text("Files, undo, appearance and preferences");
        widgets::Menu::below(&button).show(|ui| {
            let ctx = ui.ctx().clone();
            self.menu_command(ui, Command::NewProject, "New");
            self.template_menu(ui);
            self.menu_command(ui, Command::OpenProject, "Open\u{2026}");
            let recent = self.settings.projects.recent.clone();
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                widgets::submenu(ui, "Open recent", |ui| {
                    for path in recent {
                        let label = Path::new(&path)
                            .file_stem()
                            .map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
                        if widgets::menu_item(ui, &label, None, true)
                            .on_hover_text(&path)
                            .clicked()
                        {
                            self.request(&ctx, Next::Open(Some(PathBuf::from(&path))));
                            ui.close();
                        }
                    }
                });
            });
            self.menu_command(ui, Command::Save, "Save");
            self.menu_command(ui, Command::SaveAs, "Save as\u{2026}");
            widgets::submenu(ui, "More ways to save", |ui| {
                self.menu_command(ui, Command::SaveCopy, "Save a copy\u{2026}");
                self.menu_command(ui, Command::CollectAndSave, "Collect and save");
                self.menu_command(ui, Command::SaveTemplate, "Save as template\u{2026}");
                self.menu_command(ui, Command::Revert, "Revert to saved\u{2026}");
            });
            ui.separator();
            self.menu_command(ui, Command::ProjectSettings, "Project settings\u{2026}");
            self.menu_command(ui, Command::MediaPool, "Project media");
            self.menu_command(ui, Command::Import, "Import audio or MIDI\u{2026}");
            self.menu_command(ui, Command::ExportMidiFile, "Export MIDI file\u{2026}");
            self.menu_command(ui, Command::ExportAudio, "Export audio\u{2026}");
            ui.separator();
            self.menu_command(ui, Command::Undo, "Undo");
            self.menu_command(ui, Command::Redo, "Redo");
            self.menu_command(ui, Command::History, "History\u{2026}");
            ui.separator();
            widgets::submenu(ui, "Appearance", |ui| {
                widgets::menu_caption(ui, "Theme");
                let chosen = theme::Appearance::from_key(&self.settings.appearance.theme);
                let themes = theme::Appearance::ALL.map(|a| widgets::Choice::new(a, a.name()));
                if let Some(picked) = widgets::segmented(ui, ui.id().with("theme"), chosen, &themes)
                {
                    self.settings.appearance.theme = picked.key().to_owned();
                }
                ui.separator();
                widgets::menu_caption(ui, "Interface size");
                let scale = self.settings.appearance.scale();
                let labels = settings::UI_SCALES.map(|s| format!("{:.0}%", s * 100.0));
                let sizes: Vec<_> = settings::UI_SCALES
                    .iter()
                    .zip(&labels)
                    .map(|(s, label)| widgets::Choice::new(*s, label))
                    .collect();
                if let Some(picked) = widgets::segmented(ui, ui.id().with("scale"), scale, &sizes) {
                    self.settings.appearance.ui_scale = picked;
                }
                ui.separator();
                widgets::switch(
                    ui,
                    &mut self.settings.appearance.reduce_motion,
                    "Reduce motion",
                    true,
                )
                .on_hover_text("Things change in place instead of sliding or fading");
            });
            self.menu_command(ui, Command::Preferences, "Preferences\u{2026}");
            self.menu_command(ui, Command::Welcome, Command::Welcome.name());
            self.menu_command(ui, Command::OpenShortcuts, "Keyboard shortcuts\u{2026}");
        });
        self.appearance_chosen(&before, ui.ctx());
    }

    pub(crate) fn appearance_chosen(&mut self, before: &settings::Appearance, ctx: &egui::Context) {
        if self.settings.appearance != *before {
            self.appearance_override = false;
            self.saved_appearance = self.settings.appearance.clone();
            self.apply_appearance(ctx);
            self.settings_changed = Some(Instant::now());
        }
    }

    pub(crate) fn apply_appearance(&mut self, ctx: &egui::Context) {
        let appearance = &self.settings.appearance;
        motion::set_reduced(appearance.reduce_motion);
        theme::apply(
            ctx,
            theme::Appearance::from_key(&appearance.theme),
            motion::time(motion::BASE),
        );
        ctx.set_zoom_factor(appearance.scale());
        self.apply_window_minimum(ctx);
        self.system_theme = ctx.system_theme();
    }

    pub(crate) fn save_settings_when_settled(&mut self, ctx: &egui::Context) {
        if let Some(audio) = self.transport.audio.confirmed.take() {
            self.settings.audio = Some(audio);
            self.settings_changed = Some(Instant::now());
        }
        let metronome = settings::Metronome::from(self.transport.metronome);
        if metronome != self.settings.metronome {
            self.settings.metronome = metronome;
            self.settings_changed = Some(Instant::now());
        }
        if self.transport.follow != self.settings.follow {
            self.settings.follow = self.transport.follow;
            self.settings_changed = Some(Instant::now());
        }
        let tab = self.browser.tab.into();
        if tab != self.settings.browser.tab {
            self.settings.browser.tab = tab;
            self.settings_changed = Some(Instant::now());
        }
        if self.transport.time_format != self.settings.time_format {
            self.settings.time_format = self.transport.time_format;
            self.settings_changed = Some(Instant::now());
        }
        let Some(changed) = self.settings_changed else {
            return;
        };
        if let Some(left) = settle_left(changed.elapsed(), crate::motion::SETTLE) {
            ctx.request_repaint_after(left);
            return;
        }
        self.write_settings();
    }

    pub(crate) fn write_settings(&mut self) {
        self.settings_changed = None;
        let mut file = self.settings.clone();
        if self.appearance_override {
            file.appearance = self.saved_appearance.clone();
        }
        if let Some(path) = &self.settings_path {
            self.settings_writer.save(path, &file);
        }
    }
}

fn settle_left(elapsed: Duration, settle: Duration) -> Option<Duration> {
    settle.checked_sub(elapsed).filter(|left| !left.is_zero())
}

#[cfg(test)]
mod settle_tests {
    use super::*;

    #[test]
    fn a_change_waits_out_what_is_left_and_then_is_written() {
        let settle = crate::motion::SETTLE;
        assert_eq!(
            settle_left(Duration::from_millis(120), settle),
            Some(Duration::from_millis(380))
        );
        assert_eq!(settle_left(settle, settle), None, "exactly settled");
        assert_eq!(settle_left(Duration::from_millis(900), settle), None);
    }
}
