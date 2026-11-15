mod catalog;
mod devices;
mod latency;

use catalog::*;
use devices::*;
use latency::*;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui::{self, RichText, Ui};
use lumen_audio::{DeviceId, DeviceInfo};

use super::Marks;
use crate::audio::{self, Audio, Status, driver_label};
use crate::measure_latency::{Asked, Cable, Card, Shown};
use crate::settings::{AudioSettings, MeasuredWith};
use crate::widgets::{hint, khz, row, section, warning};
use crate::{theme, widgets};
use widgets::Dropdown;

const COMMON_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

pub enum Action {
    Apply(AudioSettings),
    ControlPanel,
    TestTone(bool),
    RecordOffset(f32),
    FromDisk(bool),
    Measure(Cable),
    UseMeasured(f32, MeasuredWith),
    DismissNotice,
}

type DeviceList = Result<Vec<(DeviceId, String)>, String>;

#[derive(Default)]
pub struct State {
    catalog: Catalog,
    driver: Option<String>,
    last_notice: Option<String>,
    measure: Card,
    refocus: Option<egui::Id>,
}

impl State {
    pub fn open_measure(&mut self) {
        self.measure.open = true;
        self.measure.reveal = true;
    }

    pub fn output_names(
        &mut self,
        driver: &str,
        id: &str,
        ctx: &egui::Context,
    ) -> Option<Result<Vec<String>, String>> {
        self.catalog.poll();
        if driver != "asio" {
            self.catalog.list(driver, ctx);
        }
        self.catalog.details(driver, id, ctx).map(|d| {
            d.as_ref()
                .map(|i| i.output_channels.clone())
                .map_err(Clone::clone)
        })
    }
    pub fn input_names(
        &mut self,
        driver: &str,
        id: &str,
        ctx: &egui::Context,
    ) -> Option<Result<Vec<String>, String>> {
        self.catalog.poll();
        if driver != "asio" {
            self.catalog.list(driver, ctx);
        }
        self.catalog.details(driver, id, ctx).map(|d| {
            d.as_ref()
                .map(|i| i.input_channels.clone())
                .map_err(Clone::clone)
        })
    }
}

const DEVICE_MENU: f32 = 280.0;
const BESIDE_DEVICE_MENU: f32 = 40.0;

#[derive(Debug, Clone, Copy)]
pub struct Lent<'a> {
    pub test_tone: bool,
    pub record_offset_ms: f32,
    pub offset_measured: Option<&'a MeasuredWith>,
    pub measure_held: Option<&'static str>,
    pub from_disk: bool,
}

impl State {
    pub fn page(
        &mut self,
        ui: &mut Ui,
        audio: &Audio,
        lent: Lent<'_>,
        marks: &Marks,
        actions: &mut Vec<Action>,
    ) {
        self.refocused(ui, audio, actions, |state, ui, locked, actions| {
            state.rows(ui, audio, lent, marks, locked, actions);
        });
    }

    pub fn device(&mut self, ui: &mut Ui, audio: &Audio, marks: &Marks, actions: &mut Vec<Action>) {
        self.refocused(ui, audio, actions, |state, ui, locked, actions| {
            state.catalog.poll();
            state.device_rows(ui, audio, marks, locked, actions);
        });
    }

    fn refocused(
        &mut self,
        ui: &mut Ui,
        audio: &Audio,
        actions: &mut Vec<Action>,
        add: impl FnOnce(&mut Self, &mut Ui, bool, &mut Vec<Action>),
    ) {
        let locked = !audio.can_change();
        if !locked && let Some(id) = self.refocus.take() {
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        let applied = |actions: &[Action]| {
            actions
                .iter()
                .filter(|a| matches!(a, Action::Apply(_)))
                .count()
        };
        let before = applied(actions);
        add(self, ui, locked, actions);
        if applied(actions) > before {
            self.refocus = ui.ctx().memory(|m| m.focused());
        }
    }

    pub fn show_driver(&mut self, driver: &str) {
        self.driver = Some(driver.to_owned());
    }

    pub fn asio_drivers(&mut self, ctx: &egui::Context) -> Option<Vec<(DeviceId, String)>> {
        if !audio::drivers_for(audio::DEFAULT_DRIVER)
            .iter()
            .any(|(key, _)| *key == "asio")
        {
            return Some(Vec::new());
        }
        self.catalog.poll();
        match self.catalog.list("asio", ctx)? {
            Ok(list) => Some(list.clone()),
            Err(_) => Some(Vec::new()),
        }
    }

    fn rows(
        &mut self,
        ui: &mut Ui,
        audio: &Audio,
        lent: Lent<'_>,
        marks: &Marks,
        locked: bool,
        actions: &mut Vec<Action>,
    ) {
        self.catalog.poll();
        let Lent {
            test_tone,
            record_offset_ms,
            offset_measured,
            measure_held,
            from_disk,
        } = lent;
        let ctx = ui.ctx().clone();
        let chosen = self.device_rows(ui, audio, marks, locked, actions);
        let current = audio.settings();

        let Some(current) = current.as_ref().filter(|s| s.driver == chosen) else {
            return;
        };
        let info = match self
            .catalog
            .details(&chosen, &current.device_id, &ctx)
            .cloned()
        {
            None => {
                row(ui, "", |ui| {
                    ui.spinner();
                    ui.label("Checking the device…");
                });
                return;
            }
            Some(Err(e)) => {
                if matches!(audio.status, Status::Running { .. }) {
                    row(ui, "", |ui| {
                        warning(ui, &e, 0.0);
                    });
                }
                return;
            }
            Some(Ok(info)) => info,
        };
        let running = audio.info.as_ref();
        ui.add_space(theme::space::XS);
        ui.separator();

        section(ui, "Format");
        marks.row(ui, "audio.rate", |ui| {
            device_controls(ui, locked, |ui| {
                row(ui, "Sample rate", |ui| {
                    if chosen == "wasapi" {
                        ui.label(running.map_or("—".into(), |i| khz(i.sample_rate)));
                        hint(ui, "set in the Windows sound settings");
                        return;
                    }
                    let rates: Vec<u32> = if chosen == "asio" {
                        COMMON_RATES.to_vec()
                    } else {
                        info.sample_rates.clone()
                    };
                    let now = running.map(|i| i.sample_rate);
                    let shown = current.sample_rate.or(now);
                    let mut rate = shown;
                    let labels: Vec<String> = rates.iter().map(|r| khz(*r)).collect();
                    let index = shown.and_then(|s| rates.iter().position(|r| *r == s));
                    let text = shown.map_or("Device default".to_owned(), khz);
                    if let Some(i) = Dropdown::new("Sample rate", &text, 120.0).show(
                        ui,
                        egui::Id::new("rate"),
                        &labels,
                        index,
                    ) {
                        rate = Some(rates[i]);
                    }
                    if rate != shown {
                        actions.push(Action::Apply(AudioSettings {
                            sample_rate: rate,
                            ..current.clone()
                        }));
                    }
                })
            })
        });
        let frames = running.map(|i| i.buffer_frames);
        marks.row(ui, "audio.buffer", |ui| {
            device_controls(ui, locked, |ui| {
                row(ui, "Buffer size", |ui| match chosen.as_str() {
                    "asio" => {
                        ui.label(frames.map_or("—".into(), |f| format!("{f} samples")));
                        let opening = audio.control_panel_opening();
                        let button = ui
                        .add_enabled(
                            matches!(audio.status, Status::Running { .. }) && !opening,
                            |ui: &mut egui::Ui| {
                                crate::widgets::raised::button(ui, "Control panel…")
                            },
                        )
                        .on_hover_text(
                            "Set the buffer size (and clock source) in the driver's own settings",
                        )
                        .on_disabled_hover_text("Available while the driver is running");
                        if button.clicked() {
                            actions.push(Action::ControlPanel);
                        }
                        if opening {
                            ui.spinner();
                            hint(ui, "opening…");
                        }
                    }
                    "wasapi" => {
                        ui.label(frames.map_or("—".into(), |f| format!("{f} samples")));
                        hint(ui, "set by Windows");
                    }
                    _ => match info.buffer_sizes {
                        Some(sizes) => {
                            let mut choices: Vec<u32> = [
                                32, 48, 64, 96, 128, 144, 192, 256, 384, 441, 448, 480, 512, 768,
                                1024, 2048, 4096,
                            ]
                            .into_iter()
                            .filter(|&f| sizes.allows(f))
                            .collect();
                            if choices.is_empty() {
                                choices.push(sizes.preferred);
                            }
                            let shown = current.buffer_frames.or(frames);
                            let mut chosen_frames = shown;
                            let labels: Vec<String> =
                                choices.iter().map(|f| format!("{f} samples")).collect();
                            let index = shown.and_then(|s| choices.iter().position(|f| *f == s));
                            let text = shown
                                .map_or("Device default".to_owned(), |f| format!("{f} samples"));
                            if let Some(i) = Dropdown::new("Buffer size", &text, 120.0).show(
                                ui,
                                egui::Id::new("buffer"),
                                &labels,
                                index,
                            ) {
                                chosen_frames = Some(choices[i]);
                            }
                            if chosen_frames != shown {
                                actions.push(Action::Apply(AudioSettings {
                                    buffer_frames: chosen_frames,
                                    ..current.clone()
                                }));
                            }
                        }
                        None => {
                            ui.label(frames.map_or("—".into(), |f| format!("{f} samples")));
                        }
                    },
                })
            })
        });
        if let Some(e) = &audio.control_panel_error {
            row(ui, "", |ui| {
                warning(ui, &format!("The control panel did not open: {e}"), 0.0);
            });
        }
        ui.add_space(theme::space::XS);
        ui.separator();

        section(ui, "Channels");
        let output_names = &info.output_channels;
        marks.row(ui, "audio.output", |ui| {
            device_controls(ui, locked, |ui| {
                row(ui, "Output", |ui| {
                    ui.vertical(|ui| {
                        for (pair, label) in pairs(output_names, "Out") {
                            let channels = channels_of(pair, output_names);
                            let on = current.output_channels == channels;
                            if widgets::choice_row(ui, &label, on).clicked() && !on {
                                let cue = if current.cue_channels == channels {
                                    Vec::new()
                                } else {
                                    current.cue_channels.clone()
                                };
                                actions.push(Action::Apply(AudioSettings {
                                    output_channels: channels,
                                    cue_channels: cue,
                                    ..current.clone()
                                }));
                            }
                        }
                    });
                })
            })
        });
        let cue_pairs: Vec<(usize, String)> = pairs(output_names, "Out")
            .into_iter()
            .filter(|(pair, _)| {
                let channels = channels_of(*pair, output_names);
                channels.len() == 2 && channels != current.output_channels
            })
            .collect();
        ui.add_space(theme::space::S);
        marks.row(ui, "audio.cue", |ui| device_controls(ui, locked, |ui| row(ui, "Cue", |ui| {
            ui.vertical(|ui| {
                let none = current.cue_channels.is_empty();
                if widgets::choice_row(ui, "None", none).clicked() && !none {
                    actions.push(Action::Apply(AudioSettings {
                        cue_channels: Vec::new(),
                        ..current.clone()
                    }));
                }
                for (pair, label) in &cue_pairs {
                    let channels = channels_of(*pair, output_names);
                    let on = current.cue_channels == channels;
                    if widgets::choice_row(ui, label, on).clicked() && !on {
                        actions.push(Action::Apply(AudioSettings {
                            cue_channels: channels,
                            ..current.clone()
                        }));
                    }
                }
                hint(
                    ui,
                    if cue_pairs.is_empty() {
                        "This device has no second pair of outputs for a cue"
                    } else {
                        "For headphones: tracks cued from the master's Solo/Cue switch, and the metronome if set to Cue"
                    },
                );
            });
        })));
        if !info.input_channels.is_empty() {
            let input_names = &info.input_channels;
            ui.add_space(theme::space::S);
            marks.row(ui, "audio.input", |ui| device_controls(ui, locked, |ui| row(ui, "Input", |ui| {
                ui.vertical(|ui| {
                    if widgets::choice_row(ui, "All inputs", current.all_inputs).clicked()
                        && !current.all_inputs
                    {
                        actions.push(Action::Apply(AudioSettings {
                            input_channels: Vec::new(),
                            all_inputs: true,
                            ..current.clone()
                        }));
                    }
                    for (pair, label) in pairs(input_names, "In") {
                        let channels = channels_of(pair, input_names);
                        let on = !current.all_inputs && current.input_channels == channels;
                        if widgets::choice_row(ui, &label, on).clicked() && !on {
                            actions.push(Action::Apply(AudioSettings {
                                input_channels: channels,
                                all_inputs: false,
                                ..current.clone()
                            }));
                        }
                    }
                    let none = !current.all_inputs && current.input_channels.is_empty();
                    if widgets::choice_row(ui, "None", none).clicked() && !none {
                        actions.push(Action::Apply(AudioSettings {
                            input_channels: Vec::new(),
                            all_inputs: false,
                            ..current.clone()
                        }));
                    }
                    hint(
                        ui,
                        "The inputs tracks can record from and monitor; choose one in a track's In menu",
                    );
                    for name in running.map_or(&[][..], |i| &i.silent_inputs[..]) {
                        warning(ui, &silent_input_words(name), 0.0);
                    }
                });
            })));
        }
        ui.add_space(theme::space::XS);
        ui.separator();

        marks.row(ui, "audio.latency", |ui| section(ui, "Latency"));
        match running {
            Some(i) => {
                let ms = |frames: u32| f64::from(frames) / f64::from(i.sample_rate) * 1000.0;
                let line = |ui: &mut Ui, name: &str, frames: u32| {
                    row(ui, name, |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(56.0, 20.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.set_min_size(egui::vec2(56.0, 20.0));
                                ui.label(format!("{:.1} ms", ms(frames)));
                            },
                        );
                        ui.add_space(theme::space::XS);
                        hint(ui, &format!("{frames} samples"));
                    });
                };
                if i.input_channels > 0 {
                    line(ui, "Input", i.input_latency);
                }
                line(ui, "Output", i.output_latency);
                if i.input_channels > 0 {
                    line(ui, "Round trip", i.input_latency + i.output_latency);
                }
                row(ui, "", |ui| {
                    hint(
                        ui,
                        "As the driver reports them, including its safety buffers",
                    );
                });
                if i.input_channels > 0 {
                    let was_open = self.measure.open;
                    marks.row(ui, "audio.offset", |ui| {
                        record_offset(ui, record_offset_ms, &mut self.measure.open, actions)
                    });
                    self.measure.reveal |= self.measure.open && !was_open;
                    let running = MeasuredWith {
                        driver: current.driver.clone(),
                        device_id: current.device_id.clone(),
                        device_name: current.device_name.clone(),
                        sample_rate: i.sample_rate,
                        buffer_frames: i.buffer_frames,
                    };
                    if let Some(with) = offset_measured {
                        measured_elsewhere(ui, with, &running);
                    }
                    if self.measure.open {
                        let shown = Shown {
                            measurement: &audio.measurement,
                            outputs: &audio.open_outputs(),
                            inputs: &audio.open_inputs(),
                            output_names: &info.output_channels,
                            input_names: &info.input_channels,
                            held: measure_held,
                            offset_ms: record_offset_ms,
                            offset_measured,
                            running: &running,
                        };
                        match self.measure.show(ui, shown) {
                            Some(Asked::Measure(cable)) => actions.push(Action::Measure(cable)),
                            Some(Asked::Use(ms, with)) => {
                                actions.push(Action::UseMeasured(ms, with))
                            }
                            None => {}
                        }
                    }
                }
            }
            None => row(ui, "", |ui| hint(ui, "Shown once the device is running")),
        }
        ui.add_space(theme::space::XS);
        ui.separator();
        ui.add_space(theme::space::XS);
        marks.row(ui, "audio.test_tone", |ui| {
            let mut tone = test_tone;
            if widgets::switch(ui, &mut tone, "Test tone (1 kHz, −20 dBFS)", true)
                .on_hover_text("A steady tone on every output, to check the audio path")
                .changed()
            {
                actions.push(Action::TestTone(tone));
            }
        });
        marks.row(ui, "audio.from_disk", |ui| {
            let mut disk = from_disk;
            if widgets::switch(ui, &mut disk, "Play audio from disk", true)
                .on_hover_text(
                    "Files of half a minute or more are read from disk as they play, so a long \
                     project needs less memory and opens sooner. Shorter files stay in memory.",
                )
                .changed()
            {
                actions.push(Action::FromDisk(disk));
            }
        });
    }
}

fn silent_input_words(name: &str) -> String {
    let look = if cfg!(windows) {
        "Windows may be keeping apps from it: see Settings, Privacy & security, Microphone"
    } else {
        "The system may be keeping apps from it: see its privacy settings for the microphone"
    };
    format!("{name} would not open, so its inputs are silent. {look}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_group_into_pairs_with_a_mono_remainder() {
        let names: Vec<String> = ["L", "R", "Mic"].map(String::from).to_vec();
        let p = pairs(&names, "In");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].0, 0);
        assert!(p[0].1.starts_with("1/2"));
        assert!(p[1].1.starts_with("3 "));
        assert_eq!(channels_of(0, &names), vec![0, 1]);
        assert_eq!(channels_of(2, &names), vec![2]);
        let blank: Vec<String> = vec![String::new(), " ".into()];
        assert!(pairs(&blank, "Out")[0].1.contains("Out 1 / Out 2"));
    }
}
