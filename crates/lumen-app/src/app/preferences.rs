//! The app menu (click "Lumen"): files, undo, appearance (theme, interface
//! size, Reduce motion) and audio settings; and saving settings in the
//! background (audit H1).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use lumen_core::product::PRODUCT_NAME;

use crate::app::projects::Next;
use crate::{App, fonts, motion, settings, theme};

impl App {
    /// The product name opens the app menu: appearance (theme, motion, UI
    /// scale) and audio settings. Preferences proper arrive in M9.
    pub(crate) fn app_menu(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.appearance.clone();
        let menu = ui.menu_button(
            RichText::new(PRODUCT_NAME).font(fonts::display(theme::size::WORDMARK)),
            |ui| {
                let ctx = ui.ctx().clone();
                if ui
                    .add(egui::Button::new("New").shortcut_text("Ctrl+N"))
                    .clicked()
                {
                    self.request(&ctx, Next::New);
                }
                if ui
                    .add(egui::Button::new("Open\u{2026}").shortcut_text("Ctrl+O"))
                    .clicked()
                {
                    self.request(&ctx, Next::Open(None));
                }
                let recent = self.settings.projects.recent.clone();
                ui.add_enabled_ui(!recent.is_empty(), |ui| {
                    ui.menu_button("Open recent", |ui| {
                        for path in recent {
                            let label = Path::new(&path)
                                .file_stem()
                                .map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
                            if ui.button(label).on_hover_text(&path).clicked() {
                                self.request(&ctx, Next::Open(Some(PathBuf::from(&path))));
                                ui.close();
                            }
                        }
                    });
                });
                if ui
                    .add(egui::Button::new("Save").shortcut_text("Ctrl+S"))
                    .clicked()
                {
                    self.save(&ctx, false);
                }
                if ui
                    .add(egui::Button::new("Save as\u{2026}").shortcut_text("Ctrl+Shift+S"))
                    .clicked()
                {
                    self.save(&ctx, true);
                }
                ui.checkbox(
                    &mut self.settings.projects.copy_imported,
                    "Copy imported audio into the project",
                )
                .on_hover_text(
                    "When saving, audio from outside the project folder is copied into \
                     Samples/Imported, so the project keeps working if the originals move",
                );
                ui.separator();
                let undo = self.document.history().undo_label().map(str::to_owned);
                let redo = self.document.history().redo_label().map(str::to_owned);
                let undo_text = undo.map_or("Undo".to_owned(), |l| format!("Undo {l}"));
                let redo_text = redo.map_or("Redo".to_owned(), |l| format!("Redo {l}"));
                if ui
                    .add_enabled(
                        self.document.history().can_undo(),
                        egui::Button::new(undo_text).shortcut_text("Ctrl+Z"),
                    )
                    .clicked()
                {
                    self.undo(false);
                }
                if ui
                    .add_enabled(
                        self.document.history().can_redo(),
                        egui::Button::new(redo_text).shortcut_text("Ctrl+Shift+Z"),
                    )
                    .clicked()
                {
                    self.undo(true);
                }
                if ui.button("History…").clicked() {
                    self.show_history = true;
                    ui.close();
                }
                ui.separator();
                ui.label(RichText::new("Theme").small().weak());
                let mut chosen = theme::Appearance::from_key(&self.settings.appearance.theme);
                for option in theme::Appearance::ALL {
                    ui.radio_value(&mut chosen, option, option.name());
                }
                self.settings.appearance.theme = chosen.key().to_owned();
                ui.separator();
                ui.label(RichText::new("Interface size").small().weak());
                let mut scale = self.settings.appearance.scale();
                for option in settings::UI_SCALES {
                    ui.radio_value(&mut scale, option, format!("{:.0}%", option * 100.0));
                }
                self.settings.appearance.ui_scale = scale;
                ui.separator();
                ui.checkbox(&mut self.settings.appearance.reduce_motion, "Reduce motion")
                    .on_hover_text("Things change in place instead of sliding or fading");
                ui.separator();
                if ui.button("Audio settings…").clicked() {
                    self.transport.settings_window.open = true;
                    ui.close();
                }
                if ui.button("Keyboard shortcuts…").clicked() {
                    self.open_shortcuts();
                    ui.close();
                }
            },
        );
        menu.response
            .on_hover_text("Appearance, interface size and audio settings");
        if self.settings.appearance != before {
            self.appearance_override = false;
            self.saved_appearance = self.settings.appearance.clone();
            self.apply_appearance(ui.ctx());
            self.settings_changed = Some(Instant::now());
        }
    }

    /// Applies the saved (or one-off) appearance: palette and egui style,
    /// motion, and UI scale.
    pub(crate) fn apply_appearance(&mut self, ctx: &egui::Context) {
        let appearance = &self.settings.appearance;
        motion::set_reduced(appearance.reduce_motion);
        theme::apply(
            ctx,
            theme::Appearance::from_key(&appearance.theme),
            motion::time(motion::BASE),
        );
        ctx.set_zoom_factor(appearance.scale());
        self.system_theme = ctx.system_theme();
    }

    /// Notes settings changes and writes them once they settle.
    pub(crate) fn save_settings_when_settled(&mut self, ctx: &egui::Context) {
        // A device setup that worked (chosen in the audio settings).
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
        if self.transport.time_format != self.settings.time_format {
            self.settings.time_format = self.transport.time_format;
            self.settings_changed = Some(Instant::now());
        }
        let Some(changed) = self.settings_changed else {
            return;
        };
        let settle = Duration::from_millis(500);
        if changed.elapsed() < settle {
            ctx.request_repaint_after(settle - changed.elapsed());
            return;
        }
        self.write_settings();
    }

    /// Queues the settings for the background writer.
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
