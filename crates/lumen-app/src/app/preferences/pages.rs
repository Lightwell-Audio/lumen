//! The pages of Preferences (M9 R7). Audio and MIDI draw themselves
//! (`preferences/audio.rs`, `preferences/midi.rs`); the rest change the
//! app's own settings, mostly through the commands the palette has, so a
//! switch here and its command are one setting.

use std::time::Instant;

use eframe::egui::{self, Ui};
use lumen_devices::Preset;
use lumen_engine::{ClickSound, MetronomeSettings};
use lumen_project::model::Monitor;

use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::preferences::{self, Marks, Page, audio};
use crate::record_settings::CountIn;
use crate::transport::record;
use crate::widgets::{hint, row, section};
use crate::{App, midi, settings, theme, widgets};

impl App {
    /// The page shown now.
    pub(super) fn preferences_page(
        &mut self,
        ui: &mut Ui,
        marks: &Marks,
        midi_asked: &mut Vec<midi::Action>,
    ) {
        match self.preferences.page {
            Page::Audio => self.audio_page(ui, marks),
            Page::Midi => self.midi_page(ui, marks, midi_asked),
            Page::Recording => self.recording_page(ui, marks),
            Page::Editing => self.editing_page(ui, marks),
            Page::Mixing => self.mixing_page(ui, marks),
            Page::Plugins => self.plugins_page(ui, marks),
            Page::Files => self.files_page(ui, marks),
            Page::Appearance => self.appearance_page(ui, marks),
        }
    }

    fn audio_page(&mut self, ui: &mut Ui, marks: &Marks) {
        let lent = audio::Lent {
            test_tone: self.transport.test_tone(),
            record_offset_ms: self.settings.record.offset_ms,
            offset_measured: self.settings.offset_measured.as_ref(),
            measure_held: self.transport.measure_held(),
            from_disk: self.settings.playback.from_disk,
        };
        let mut actions = Vec::new();
        self.preferences
            .audio
            .page(ui, &self.transport.audio, lent, marks, &mut actions);
        for action in actions {
            let Some(kept) = self.transport.audio_action(action) else {
                continue;
            };
            match kept {
                audio::Action::RecordOffset(ms) => {
                    // Typed by hand: no longer the measured one.
                    self.settings.record.offset_ms = ms;
                    self.settings.offset_measured = None;
                }
                audio::Action::UseMeasured(ms, with) => {
                    self.settings.record.offset_ms = ms;
                    self.settings.offset_measured = Some(with);
                }
                audio::Action::FromDisk(on) => self.settings.playback.from_disk = on,
                _ => continue,
            }
            self.settings_changed = Some(Instant::now());
        }
    }

    fn midi_page(&mut self, ui: &mut Ui, marks: &Marks, midi_asked: &mut Vec<midi::Action>) {
        let shown = preferences::midi::Shown {
            midi: &self.midi,
            saved: &self.settings.midi,
        };
        preferences::midi::page(ui, shown, marks, midi_asked);
        section(ui, "Editing");
        marks.row(ui, "midi.audition", |ui| {
            let mut on = self.settings.midi.audition;
            if widgets::switch(ui, &mut on, "Play notes while editing", true)
                .on_hover_text("Notes sound as you draw, move or click them in the editors")
                .changed()
            {
                self.settings.midi.audition = on;
                self.settings_changed = Some(Instant::now());
            }
        });
    }

    fn recording_page(&mut self, ui: &mut Ui, marks: &Marks) {
        let record = self.settings.record;
        section(ui, "Recording");
        marks.row(ui, "record.count_in", |ui| {
            row(ui, "Count-in", |ui| {
                let counts = CountIn::ALL
                    .map(|c| widgets::Choice::new(c, c.name()).named(record::count_in_name(c)));
                if let Some(count_in) =
                    widgets::segmented(ui, ui.id().with("count in"), record.count_in, &counts)
                {
                    self.page_command(ui, record::count_in_command(count_in));
                }
            });
        });
        marks.row(ui, "record.overdub", |ui| {
            row(ui, "Takes", |ui| {
                let modes = [
                    widgets::Choice::new(false, "Replace")
                        .named("Takes: Replace")
                        .tip("A take replaces the clip under it"),
                    widgets::Choice::new(true, "Overdub")
                        .named("Takes: Overdub")
                        .tip("A take keeps what is under it and adds to it"),
                ];
                if let Some(overdub) =
                    widgets::segmented(ui, ui.id().with("overdub"), record.overdub, &modes)
                    && overdub != record.overdub
                {
                    self.page_command(ui, Command::Overdub);
                }
            });
        });
        marks.row(ui, "record.keep_played", |ui| {
            self.command_switch(ui, Command::KeepPlayedNotes, record.keep_played);
        });
        ui.add_space(theme::space::XS);
        if widgets::raised::button(ui, "Recording offset\u{2026}")
            .on_hover_text("The offset that lines recordings up is on the Audio page, with the loopback helper")
            .clicked()
            && let Some(offset) = preferences::ROWS.iter().find(|r| r.key == "audio.offset")
        {
            self.preferences.choose(offset, ui.input(|i| i.time));
        }

        section(ui, "Metronome");
        let mut metronome = self.transport.metronome;
        marks.row(ui, "record.metronome_volume", |ui| {
            row(ui, "Volume", |ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(theme::size::PREFERENCES_SLIDER, theme::size::CONTROL_SMALL),
                    egui::Sense::hover(),
                );
                let default = MetronomeSettings::default().volume;
                metronome.volume = widgets::value_slider(
                    ui,
                    rect,
                    ui.id().with("metronome volume"),
                    metronome.volume,
                    widgets::SliderKind::Percent { default },
                    "Metronome volume",
                    theme::controls(false),
                );
            });
        });
        marks.row(ui, "record.metronome_sound", |ui| {
            row(ui, "Sound", |ui| {
                let sounds = ClickSound::ALL.map(|s| widgets::Choice::new(s, s.name()));
                if let Some(sound) =
                    widgets::segmented(ui, ui.id().with("sound"), metronome.sound, &sounds)
                {
                    metronome.sound = sound;
                }
            });
        });
        marks.row(ui, "record.metronome_output", |ui| {
            row(ui, "Output", |ui| {
                // Its own output, never through the master (the owner's D6).
                let cue = settings::CUE_CHANNEL;
                let ready = self.transport.audio.cue_ready();
                let outputs = [
                    widgets::Choice::new(0, "Main"),
                    widgets::Choice::new(cue, "Cue")
                        .enabled(ready)
                        .tip(if ready {
                            "The cue pair, for headphones"
                        } else {
                            "Choose a Cue output on the Audio page first"
                        }),
                ];
                if let Some(channel) =
                    widgets::segmented(ui, ui.id().with("output"), metronome.channel, &outputs)
                {
                    metronome.channel = channel;
                }
            });
        });
        marks.row(ui, "record.only_while_recording", |ui| {
            self.command_switch(
                ui,
                Command::RecordOnlyMetronome,
                record.only_while_recording,
            );
        });
        if metronome != self.transport.metronome {
            // Saved with the settings once they settle.
            self.transport.restore_metronome(metronome);
            widgets::note_change(ui);
        }
    }

    fn editing_page(&mut self, ui: &mut Ui, marks: &Marks) {
        let before = self.settings.editing.clone();
        section(ui, "History");
        marks.row(ui, "edit.undo_steps", |ui| {
            row(ui, "Undo steps", |ui| {
                let labels = settings::Editing::UNDO_STEPS.map(steps_label);
                let names = labels.clone().map(|l| format!("Undo steps: {l}"));
                let steps: Vec<_> = settings::Editing::UNDO_STEPS
                    .iter()
                    .zip(labels.iter().zip(&names))
                    .map(|(n, (label, name))| widgets::Choice::new(*n, label).named(name))
                    .collect();
                let chosen = self.settings.editing.undo_steps;
                if let Some(picked) =
                    widgets::segmented(ui, ui.id().with("undo steps"), chosen, &steps)
                {
                    self.settings.editing.undo_steps = picked;
                }
            });
        });
        if self.settings.editing.undo_steps != self.document.undo_limit() {
            row(ui, "", |ui| {
                hint(ui, "Used from the next project opened or made");
            });
        }
        marks.row(ui, "edit.autosave", |ui| {
            row(ui, "Autosave every", |ui| {
                let intervals = crate::autosave::INTERVALS.map(|d| {
                    let seconds = d.as_secs();
                    let (label, name) = match seconds {
                        30 => ("30 s", "Autosave every 30 seconds"),
                        60 => ("1 min", "Autosave every minute"),
                        _ => ("2 min", "Autosave every 2 minutes"),
                    };
                    widgets::Choice::new(seconds, label).named(name)
                });
                let chosen = self.settings.editing.autosave_seconds;
                if let Some(picked) =
                    widgets::segmented(ui, ui.id().with("autosave"), chosen, &intervals)
                {
                    self.settings.editing.autosave_seconds = picked;
                }
            });
        });
        row(ui, "", |ui| {
            hint(
                ui,
                "A copy is kept this soon after each change, so a crash loses at most this much work",
            );
        });

        section(ui, "Automation");
        marks.row(ui, "edit.lock_envelopes", |ui| {
            let on = self.settings.editing.lock_envelopes;
            self.command_switch(ui, Command::LockEnvelopes, on);
        });

        section(ui, "New tracks");
        marks.row(ui, "edit.new_audio_monitor", |ui| {
            row(ui, "Audio monitor", |ui| {
                let modes = [
                    widgets::Choice::new(Monitor::In, "In")
                        .named("New audio tracks: monitor In")
                        .tip("The input is always heard"),
                    widgets::Choice::new(Monitor::Auto, "Auto")
                        .named("New audio tracks: monitor Auto")
                        .tip("The input is heard while the track is armed"),
                    widgets::Choice::new(Monitor::Off, "Off")
                        .named("New audio tracks: monitor Off")
                        .tip("The input is never heard"),
                ];
                let chosen = self.settings.editing.new_tracks.audio_monitor;
                if let Some(picked) =
                    widgets::segmented(ui, ui.id().with("new monitor"), chosen, &modes)
                {
                    self.settings.editing.new_tracks.audio_monitor = picked;
                }
            });
        });
        marks.row(ui, "edit.new_midi_instrument", |ui| {
            row(ui, "Instrument", |ui| {
                let options: Vec<String> = std::iter::once("None".to_owned())
                    .chain(Preset::ALL.iter().map(|p| format!("Synth: {}", p.name())))
                    .collect();
                let chosen = self
                    .settings
                    .editing
                    .new_tracks
                    .for_document()
                    .instrument
                    .map_or(0, |p| {
                        1 + Preset::ALL.iter().position(|q| *q == p).unwrap_or(0)
                    });
                let width = theme::size::PREFERENCES_SLIDER.min(ui.available_width());
                if let Some(i) =
                    widgets::Dropdown::new("New MIDI tracks: instrument", &options[chosen], width)
                        .show(ui, ui.id().with("new instrument"), &options, Some(chosen))
                {
                    self.settings.editing.new_tracks.midi_instrument =
                        i.checked_sub(1).map(|i| Preset::ALL[i].id().to_owned());
                }
            });
        });
        if self.settings.editing != before {
            self.settings_changed = Some(Instant::now());
        }
    }

    fn mixing_page(&mut self, ui: &mut Ui, marks: &Marks) {
        section(ui, "Solo");
        marks.row(ui, "mix.additive_solo", |ui| {
            let on = self.settings.mixing.additive_solo;
            self.command_switch(ui, Command::AdditiveSolo, on);
        });
        section(ui, "Meters");
        marks.row(ui, "mix.track_colour_meters", |ui| {
            let on = self.settings.mixing.track_colour_meters;
            self.command_switch(ui, Command::TrackColourMeters, on);
        });
    }

    fn plugins_page(&mut self, ui: &mut Ui, marks: &Marks) {
        if self.safe.on {
            ui.add_space(theme::space::M);
            widgets::warning(ui, crate::app::commands::availability::SAFE_MODE, 0.0);
        }
        self.plugin_folder_rows(ui, marks);

        self.blocked_plugins_rows(ui, marks);

        section(ui, "Scanning");
        marks.row(ui, "plugins.scan_at_startup", |ui| {
            let on = self.settings.plugins.scan_at_startup;
            self.command_switch(ui, Command::DevicesScanAtStartup, on);
        });
        ui.add_space(theme::space::XS);
        marks.row(ui, "plugins.rescan", |ui| {
            ui.horizontal(|ui| {
                self.command_button(ui, Command::DevicesRescan, "Rescan plugins");
                self.command_button(ui, Command::DevicesRescanAll, "Rescan all plugins");
            });
        });
    }

    /// Your plugin folders with Remove, Add plugin folder…, and the
    /// standard folders. The first-run window shows them too (M9 R7b).
    pub(crate) fn plugin_folder_rows(&mut self, ui: &mut Ui, marks: &Marks) {
        marks.row(ui, "plugins.folders", |ui| section(ui, "Your folders"));
        let folders = self.settings.plugins.folders.clone();
        if folders.is_empty() {
            hint(
                ui,
                "None yet. Plugins are looked for in the standard folders below.",
            );
        }
        let mut removed = None;
        for folder in &folders {
            if folder_row(ui, folder, true) {
                removed = Some(folder);
            }
        }
        if let Some(folder) = removed {
            self.remove_plugin_folder(ui.ctx(), folder);
        }
        ui.add_space(theme::space::XS);
        self.command_button(ui, Command::DevicesAddFolder, "Add plugin folder\u{2026}");

        marks.row(ui, "plugins.standard", |ui| section(ui, "Standard folders"));
        for folder in lumen_plugin_catalog::discover::standard_roots() {
            folder_row(ui, &folder.display().to_string(), false);
        }
    }

    /// The plugins a scan blocked, each with why and Retry; the browser lists
    /// them too, under "Blocked".
    fn blocked_plugins_rows(&mut self, ui: &mut Ui, marks: &Marks) {
        marks.row(ui, "plugins.blocked", |ui| section(ui, "Blocked plugins"));
        let blocked: Vec<_> = self
            .plugins
            .catalog()
            .blocked()
            .iter()
            .map(|b| (b.name.clone(), b.path.clone(), b.reason.headline()))
            .collect();
        if blocked.is_empty() {
            hint(
                ui,
                "None. A plugin that fails while it is scanned is listed here.",
            );
            return;
        }
        hint(ui, lumen_plugin_catalog::blocklist::WHEN_RETRIED);
        // During a scan, Retry waits for it to end (ADR-0114).
        let ready = !self.safe.on;
        let mut retried = None;
        for (name, path, why) in &blocked {
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(width, theme::size::CONTROL),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.set_min_size(egui::vec2(width, theme::size::CONTROL));
                    let button = ui
                        .add_enabled_ui(ready, |ui| widgets::raised::button(ui, "Retry"))
                        .inner;
                    let label = format!("Retry {name}");
                    button.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, ready, &label)
                    });
                    let button = button
                        .on_disabled_hover_text(crate::app::commands::availability::SAFE_MODE);
                    if button
                        .on_hover_text("Scan it again; it is listed if it loads")
                        .clicked()
                    {
                        retried = Some(path.to_string_lossy().into_owned());
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let (icon, _) = ui.allocate_exact_size(
                            egui::vec2(theme::size::MENU_ICON, theme::size::MENU_ICON),
                            egui::Sense::hover(),
                        );
                        ui.painter()
                            .extend(icons::shapes(Icon::Warning, icon, theme::p().warning));
                        ui.add(egui::Label::new(name.as_str()).truncate())
                            .on_hover_text(format!("{why}\n{}", path.display()));
                    });
                },
            );
        }
        if let Some(module) = retried {
            self.plugins
                .retry(&module, &self.settings.plugins, ui.ctx());
        }
    }

    fn files_page(&mut self, ui: &mut Ui, marks: &Marks) {
        self.projects_folder_row(ui, marks);
        section(ui, "Importing");
        marks.row(ui, "files.copy_imported", |ui| {
            let mut on = self.settings.projects.copy_imported;
            if widgets::switch(ui, &mut on, "Copy imported audio into the project", true)
                .on_hover_text(
                    "When saving, audio from outside the project folder is copied into its \
                     Samples/Imported folder, so the project keeps working when moved",
                )
                .changed()
            {
                self.settings.projects.copy_imported = on;
                self.settings_changed = Some(Instant::now());
            }
        });
        self.cache_rows(ui, marks);
        section(ui, "Problems");
        marks.row(ui, "files.crash_reports", |ui| {
            let dir = self.crash_dir.clone();
            let shown = ui
                .add_enabled_ui(dir.is_some(), |ui| {
                    widgets::raised::button(ui, "Show crash reports")
                })
                .inner;
            let shown = match &dir {
                Some(dir) => shown.on_hover_text(format!(
                    "Opens {} in the file manager. Reports stay on this computer.",
                    dir.display()
                )),
                None => shown.on_disabled_hover_text("Crash reports are not kept for this run"),
            };
            if shown.clicked()
                && let Some(dir) = dir
                && self.templates.reveals
            {
                crate::crash::reveal(&dir);
            }
        });
    }

    fn appearance_page(&mut self, ui: &mut Ui, marks: &Marks) {
        let before = self.settings.appearance.clone();
        section(ui, "Appearance");
        marks.row(ui, "look.theme", |ui| {
            row(ui, "Theme", |ui| {
                let chosen = theme::Appearance::from_key(&self.settings.appearance.theme);
                let themes = theme::Appearance::ALL.map(|a| widgets::Choice::new(a, a.name()));
                if let Some(picked) = widgets::segmented(ui, ui.id().with("theme"), chosen, &themes)
                {
                    self.settings.appearance.theme = picked.key().to_owned();
                }
            });
        });
        marks.row(ui, "look.scale", |ui| {
            row(ui, "Interface size", |ui| {
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
            });
        });
        marks.row(ui, "look.reduce_motion", |ui| {
            widgets::switch(
                ui,
                &mut self.settings.appearance.reduce_motion,
                "Reduce motion",
                true,
            )
            .on_hover_text("Things change in place instead of sliding or fading");
        });
        self.appearance_chosen(&before, ui.ctx());
        marks.row(ui, "look.info_line", |ui| {
            let on = self.settings.appearance.info_line;
            self.command_switch(ui, Command::ToggleInfoLine, on);
        });
    }

    /// A switch for the setting `command` turns on and off, named as the
    /// palette names it; its hover tells the command's keys.
    fn command_switch(&mut self, ui: &mut Ui, command: Command, on: bool) {
        let name = command.name();
        let ready = self.availability(ui.ctx(), command);
        let mut shown = on;
        let response = widgets::switch(ui, &mut shown, name, ready.is_ok());
        let keys = self.keymap.keys(command);
        let response = widgets::tip(response, name, keys.as_ref(), Some(command.info().about));
        if response.changed() {
            self.page_command(ui, command);
        }
    }

    /// A button for `command`; greyed with the reason when it cannot run.
    fn command_button(&mut self, ui: &mut Ui, command: Command, label: &str) {
        let ready = self.availability(ui.ctx(), command);
        let response = ui
            .add_enabled_ui(ready.is_ok(), |ui| widgets::raised::button(ui, label))
            .inner;
        let keys = self.keymap.keys(command);
        let response = widgets::tip(
            response,
            command.name(),
            keys.as_ref(),
            Some(command.info().about),
        );
        let response = match ready {
            Err(why) => response.on_disabled_hover_text(why),
            Ok(()) => response,
        };
        if response.clicked() {
            self.page_command(ui, command);
        }
    }

    /// Runs `command` from a page, as the palette would.
    fn page_command(&mut self, ui: &Ui, command: Command) {
        let ctx = ui.ctx().clone();
        if self.availability(&ctx, command).is_ok() {
            self.run(&ctx, command);
        }
    }
}

/// A plugin folder's row, a control high: its path, cut short before the
/// right edge, and a Remove button there when it is one of the user's (the
/// standard folders are always searched, so theirs is quiet). True when
/// Remove was clicked.
fn folder_row(ui: &mut Ui, folder: &str, yours: bool) -> bool {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, theme::size::CONTROL),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, theme::size::CONTROL));
            let removed = yours && {
                let response = widgets::raised::button(ui, "Remove")
                    .on_hover_text("Stop looking for plugins in this folder");
                let name = format!("Remove {folder}");
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name)
                });
                response.clicked()
            };
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let text = egui::RichText::new(folder);
                let text = if yours { text } else { text.weak() };
                ui.add(egui::Label::new(text).truncate())
                    .on_hover_text(folder);
            });
            removed
        },
    )
    .inner
}

/// An undo depth as the choice shows it: "1,000".
fn steps_label(steps: usize) -> String {
    let thousands = steps / 1_000;
    if thousands > 0 && steps.is_multiple_of(1_000) {
        format!("{thousands},000")
    } else {
        steps.to_string()
    }
}
