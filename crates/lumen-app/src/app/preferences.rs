//! The app menu (click "Lumen"): files, undo, appearance (theme, interface
//! size, Reduce motion) and audio settings; and saving settings in the
//! background (audit H1).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use lumen_core::product::PRODUCT_NAME;

use crate::app::projects::Next;
use crate::commands::Command;
use crate::{App, fonts, motion, settings, theme, widgets};

impl App {
    /// The product name opens the app menu: appearance (theme, motion, UI
    /// scale), additive solo and audio settings. Preferences proper arrive
    /// in M9.
    pub(crate) fn app_menu(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.appearance.clone();
        // Lit while its menu shows, as egui's menu buttons are.
        let open = widgets::Menu::is_open_for(ui.ctx(), ui.next_auto_id());
        let button = ui
            .add(
                egui::Button::new(
                    RichText::new(PRODUCT_NAME).font(fonts::display(theme::size::WORDMARK)),
                )
                .selected(open),
            )
            .on_hover_text("Appearance, interface size and audio settings");
        widgets::Menu::below(&button).show(|ui| {
            // The project setting must remain reachable at the minimum
            // window size without pushing the menu's lower controls away.
            let height = ui.ctx().content_rect().height() - theme::space::M * 2.0;
            // Popup areas begin with a provisional height. Expand their
            // layout constraint before the scroll area measures its viewport.
            ui.set_max_height(height);
            egui::ScrollArea::vertical()
                .id_salt("app menu scroll")
                .max_height(height)
                .show(ui, |ui| {
                    let ctx = ui.ctx().clone();
                    self.menu_command(ui, Command::NewProject, "New");
                    self.menu_command(ui, Command::OpenProject, "Open\u{2026}");
                    let recent = self.settings.projects.recent.clone();
                    ui.add_enabled_ui(!recent.is_empty(), |ui| {
                        widgets::submenu(ui, "Open recent", |ui| {
                            for path in recent {
                                let label = Path::new(&path).file_stem().map_or_else(
                                    || path.clone(),
                                    |s| s.to_string_lossy().into_owned(),
                                );
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
                    self.menu_command(ui, Command::Import, "Import audio or MIDI\u{2026}");
                    self.menu_command(ui, Command::ExportMidiFile, "Export MIDI file\u{2026}");
                    widgets::switch(
                        ui,
                        &mut self.settings.projects.copy_imported,
                        "Copy imported audio into the project",
                        true,
                    )
                    .on_hover_text("Saving copies outside audio into Samples/Imported");
                    let keys = self.keymap.keys(Command::ToggleAutoCrossfade);
                    if widgets::menu_toggle(
                        ui,
                        "Automatic crossfades",
                        keys.as_ref(),
                        self.document.project().auto_crossfade,
                    )
                    .clicked()
                    {
                        self.run(&ctx, Command::ToggleAutoCrossfade);
                    }
                    ui.separator();
                    let undo = self.document.history().undo_label().map(str::to_owned);
                    let redo = self.document.history().redo_label().map(str::to_owned);
                    let undo_text = undo.map_or("Undo".to_owned(), |l| format!("Undo {l}"));
                    let redo_text = redo.map_or("Redo".to_owned(), |l| format!("Redo {l}"));
                    self.menu_command(ui, Command::Undo, &undo_text);
                    self.menu_command(ui, Command::Redo, &redo_text);
                    if ui.button("History…").clicked() {
                        self.show_history = true;
                        ui.close();
                    }
                    ui.separator();
                    widgets::menu_caption(ui, "Theme");
                    let chosen = theme::Appearance::from_key(&self.settings.appearance.theme);
                    let themes = theme::Appearance::ALL.map(|a| widgets::Choice::new(a, a.name()));
                    if let Some(picked) =
                        widgets::segmented(ui, ui.id().with("theme"), chosen, &themes)
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
                    if let Some(picked) =
                        widgets::segmented(ui, ui.id().with("scale"), scale, &sizes)
                    {
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
                    ui.separator();
                    widgets::menu_caption(ui, "Mixing");
                    if widgets::switch(
                        ui,
                        &mut self.settings.mixing.additive_solo,
                        "Additive solo",
                        true,
                    )
                    .on_hover_text("A click on Solo adds the track; Ctrl+click solos it alone")
                    .changed()
                    {
                        self.settings_changed = Some(Instant::now());
                    }
                    if widgets::switch(
                        ui,
                        &mut self.settings.mixing.track_colour_meters,
                        "Meters in track colours",
                        true,
                    )
                    .on_hover_text("Meters take their track's colour instead of the accent")
                    .changed()
                    {
                        self.settings_changed = Some(Instant::now());
                    }
                    ui.separator();
                    if crate::widgets::menu_item(ui, "Audio and MIDI settings\u{2026}", None, true)
                        .clicked()
                    {
                        self.transport.settings_window.open = true;
                        ui.close();
                    }
                    self.menu_command(ui, Command::OpenShortcuts, "Keyboard shortcuts\u{2026}");
                });
        });
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
        // The clock is read once: read twice, the time can cross the end of
        // the wait between the reads and the subtraction underflows.
        if let Some(left) = settle_left(changed.elapsed(), Duration::from_millis(500)) {
            ctx.request_repaint_after(left);
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

/// How much longer a change has to settle, or `None` once `settle` has passed.
fn settle_left(elapsed: Duration, settle: Duration) -> Option<Duration> {
    settle.checked_sub(elapsed).filter(|left| !left.is_zero())
}

#[cfg(test)]
mod settle_tests {
    use super::*;

    #[test]
    fn a_change_waits_out_what_is_left_and_then_is_written() {
        let settle = Duration::from_millis(500);
        assert_eq!(
            settle_left(Duration::from_millis(120), settle),
            Some(Duration::from_millis(380))
        );
        assert_eq!(settle_left(settle, settle), None, "exactly settled");
        // A slow frame that comes after the wait ended: the subtraction this
        // replaces panicked here.
        assert_eq!(settle_left(Duration::from_millis(900), settle), None);
    }
}
