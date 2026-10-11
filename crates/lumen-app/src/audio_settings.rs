//! The audio settings window (M2 slice 8): driver, device, sample rate,
//! buffer size, channels, latencies, the ASIO control panel and the test
//! tone. Every change applies at once; a change the device refuses reverts
//! to the setup that worked, with the reason shown.
//!
//! Device lists and details load in the background and never block the
//! window. ASIO drivers are queried in a helper process (`probe.rs`).

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui::{self, RichText, Ui};
use lumen_audio::{DeviceId, DeviceInfo};

use crate::audio::{self, Audio, DRIVERS, Status, driver_label};
use crate::settings::AudioSettings;
use crate::{theme, widgets};

/// Rates offered for the device that is running (a running ASIO driver
/// reports only its current rate).
const COMMON_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// What the window asks the transport to do.
pub enum Action {
    Apply(AudioSettings),
    ControlPanel,
    TestTone(bool),
    DismissNotice,
    /// Something on the MIDI page.
    Midi(crate::midi::Action),
}

/// A driver's devices as (id, name), or why they could not be listed.
type DeviceList = Result<Vec<(DeviceId, String)>, String>;

enum Loaded {
    List(String, DeviceList),
    Details(String, Result<DeviceInfo, String>),
}

/// Device lists per driver and details per device, loaded in the
/// background.
pub struct Catalog {
    lists: HashMap<String, Option<DeviceList>>,
    details: HashMap<String, Option<Result<DeviceInfo, String>>>,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
}

impl Default for Catalog {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            lists: HashMap::new(),
            details: HashMap::new(),
            tx,
            rx,
        }
    }
}

impl Catalog {
    fn poll(&mut self) {
        while let Ok(loaded) = self.rx.try_recv() {
            match loaded {
                Loaded::List(driver, list) => {
                    self.lists.insert(driver, Some(list));
                }
                Loaded::Details(id, details) => {
                    self.details.insert(id, Some(details));
                }
            }
        }
    }

    /// Forgets everything (after plugging a device in or out).
    fn refresh(&mut self) {
        self.lists.clear();
        self.details.clear();
    }

    fn list(&mut self, driver: &str, ctx: &egui::Context) -> Option<&DeviceList> {
        if !self.lists.contains_key(driver) {
            self.lists.insert(driver.to_owned(), None);
            let (tx, ctx, driver) = (self.tx.clone(), ctx.clone(), driver.to_owned());
            std::thread::spawn(move || {
                // WASAPI and the system hosts describe every device as they
                // list them; keep those details too.
                let mut details = Vec::new();
                let list = match audio::backend(&driver) {
                    None => Err(format!("{driver} is not available here")),
                    Some(make) if driver == "asio" => Ok(audio::listed(&driver, make)),
                    Some(make) => {
                        let devices = make().devices();
                        let list = devices
                            .iter()
                            .map(|d| (d.id.clone(), d.name.clone()))
                            .collect();
                        details = devices;
                        Ok(list)
                    }
                };
                for info in details {
                    let _ = tx.send(Loaded::Details(info.id.id.clone(), Ok(info)));
                }
                let _ = tx.send(Loaded::List(driver, list));
                ctx.request_repaint();
            });
        }
        self.lists.get(driver).and_then(Option::as_ref)
    }

    /// Details of device `id`: probed in a helper for ASIO; already known
    /// from the list for the others.
    fn details(
        &mut self,
        driver: &str,
        id: &str,
        ctx: &egui::Context,
    ) -> Option<&Result<DeviceInfo, String>> {
        if !self.details.contains_key(id) && driver == "asio" {
            self.details.insert(id.to_owned(), None);
            let (tx, ctx, clsid) = (self.tx.clone(), ctx.clone(), id.to_owned());
            std::thread::spawn(move || {
                let result = crate::probe::probe_asio(&clsid);
                let _ = tx.send(Loaded::Details(clsid, result));
                ctx.request_repaint();
            });
        }
        self.details.get(id).and_then(Option::as_ref)
    }
}

#[derive(Default)]
pub struct Window {
    pub open: bool,
    catalog: Catalog,
    /// The driver whose devices are shown (may differ from the one in use
    /// until a device is chosen).
    driver: Option<String>,
    /// The page shown: the audio device (0) or MIDI (1).
    pub page: usize,
    /// The notice shown last, kept while it fades out.
    last_notice: Option<String>,
}

/// Width of the label column, so every section lines up.
const LABEL_WIDTH: f32 = theme::size::LABEL_COLUMN;

impl Window {
    /// The output channel names of `driver`'s device `id`, looked up in the
    /// background the first time (an ASIO driver in a helper process, as
    /// the window does). `None` while looking.
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
}

/// Fixed, so a notice or a long device name never resizes the window.
const WINDOW_WIDTH: f32 = 440.0;

/// A section heading with even spacing above and below.
pub(crate) fn section(ui: &mut Ui, title: &str) {
    ui.add_space(theme::space::M);
    ui.label(RichText::new(title).font(crate::fonts::medium(theme::size::BODY)));
    ui.add_space(theme::space::XS);
}

/// A label in the left column and `contents` beside it.
fn row(ui: &mut Ui, label: &str, contents: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, theme::size::CONTROL_SMALL),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(LABEL_WIDTH, theme::size::CONTROL_SMALL));
                // A row without a heading has no label at all: an empty
                // one is a nameless node for screen readers (slice 8).
                if !label.is_empty() {
                    ui.label(RichText::new(label).weak());
                }
            },
        );
        ui.add_space(theme::space::S);
        contents(ui);
    });
}

/// A small filled dot, drawn rather than typed (the UI font has no "●").
fn dot(ui: &mut Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

/// Warning text that wraps within the space left (minus `reserve` for a
/// button beside it), so a long driver message never widens the window.
pub(crate) fn warning(ui: &mut Ui, text: &str, reserve: f32) -> egui::Response {
    let width = (ui.available_width() - reserve).max(120.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        ui.set_max_width(width);
        ui.add(egui::Label::new(RichText::new(text).color(theme::p().warning)).wrap())
    })
    .inner
}

pub(crate) fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

fn khz(rate: u32) -> String {
    format!("{:.1} kHz", f64::from(rate) / 1000.0)
}

/// Channel pairs for a device: (first index, label). Drivers that give no
/// name get "Out 1", "In 2" and so on.
fn pairs(names: &[String], prefix: &str) -> Vec<(usize, String)> {
    let name = |i: usize| match names[i].trim() {
        "" => format!("{prefix} {}", i + 1),
        given => given.to_owned(),
    };
    (0..names.len())
        .step_by(2)
        .map(|i| match names.get(i + 1) {
            Some(_) => (
                i,
                format!("{}/{}   {} / {}", i + 1, i + 2, name(i), name(i + 1)),
            ),
            None => (i, format!("{}   {}", i + 1, name(i))),
        })
        .collect()
}

fn channels_of(pair: usize, names: &[String]) -> Vec<usize> {
    if pair + 1 < names.len() {
        vec![pair, pair + 1]
    } else {
        vec![pair]
    }
}

impl Window {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        audio: &Audio,
        midi: &crate::midi::Midi,
        test_tone: bool,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        if !self.open {
            return actions;
        }
        self.catalog.poll();
        let top_centre = egui::pos2(ctx.content_rect().center().x, 72.0);
        let close =
            crate::widgets::Dialog::new("audio settings", "Audio and MIDI settings", WINDOW_WIDTH)
                .at(top_centre, egui::Align2::CENTER_TOP)
                .show(ctx, |ui| {
                    let pages = [
                        widgets::Tab {
                            label: "Audio",
                            disabled_because: None,
                            tip: "Audio device, latency and test tone",
                        },
                        widgets::Tab {
                            label: "MIDI",
                            disabled_because: None,
                            tip: "MIDI inputs and outputs",
                        },
                    ];
                    if let Some(page) =
                        widgets::tabs(ui, egui::Id::new("settings pages"), self.page, &pages)
                    {
                        self.page = page;
                    }
                    if self.page == 1 {
                        let mut asked = Vec::new();
                        crate::midi_settings::page(ui, midi, &mut asked);
                        actions.extend(asked.into_iter().map(Action::Midi));
                        // New devices appear while the window is open.
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(300));
                    } else {
                        self.contents(ui, audio, test_tone, &mut actions);
                    }
                });
        let open = !close;
        self.open = open;
        actions
    }

    fn contents(&mut self, ui: &mut Ui, audio: &Audio, test_tone: bool, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        let current = audio.settings();
        let driver = self
            .driver
            .clone()
            .or_else(|| current.as_ref().map(|s| s.driver.clone()))
            .unwrap_or_else(|| audio::DEFAULT_DRIVER.to_owned());

        // ── Device ──────────────────────────────────────────────────────
        section(ui, "Device");
        let mut chosen = driver.clone();
        row(ui, "Driver", |ui| {
            egui::ComboBox::from_id_salt("driver")
                .selected_text(driver_label(&driver))
                .width(280.0)
                .show_ui(ui, |ui| {
                    for (key, label) in DRIVERS {
                        ui.selectable_value(&mut chosen, (*key).to_owned(), *label);
                    }
                });
        });
        if chosen != driver {
            self.driver = Some(chosen.clone());
        }
        row(ui, "Device", |ui| {
            match self.catalog.list(&chosen, &ctx) {
                None => {
                    ui.spinner();
                    ui.label("Looking for devices…");
                }
                Some(Err(e)) => {
                    warning(ui, e, 0.0);
                }
                Some(Ok(devices)) if devices.is_empty() => {
                    ui.label("No devices found");
                }
                Some(Ok(devices)) => {
                    let using = current
                        .as_ref()
                        .filter(|s| s.driver == chosen)
                        .map(|s| s.device_id.clone());
                    let selected = devices
                        .iter()
                        .find(|(id, _)| Some(&id.id) == using.as_ref())
                        .map_or("Choose a device", |(_, name)| name.as_str());
                    let mut pick = None;
                    egui::ComboBox::from_id_salt("device")
                        .selected_text(selected)
                        .width(280.0)
                        .show_ui(ui, |ui| {
                            for (id, name) in devices {
                                if ui
                                    .selectable_label(Some(&id.id) == using.as_ref(), name)
                                    .clicked()
                                {
                                    pick = Some((id.clone(), name.clone()));
                                }
                            }
                        });
                    if let Some((id, name)) = pick {
                        actions.push(Action::Apply(AudioSettings {
                            driver: chosen.clone(),
                            device_id: id.id,
                            device_name: name,
                            sample_rate: None,
                            buffer_frames: None,
                            input_channels: Vec::new(),
                            output_channels: vec![0, 1],
                            cue_channels: Vec::new(),
                        }));
                        self.driver = None;
                    }
                }
            }
            if ui
                .small_button("⟳")
                .on_hover_text("Look for devices again (after plugging one in or out)")
                .clicked()
            {
                self.catalog.refresh();
            }
        });
        // What is happening right now, in one place.
        row(ui, "", |ui| {
            if let Some(changing) = audio.changing() {
                ui.spinner();
                let width = ui.available_width().max(120.0);
                ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                    ui.set_max_width(width);
                    ui.add(egui::Label::new(changing).wrap());
                });
            } else {
                match &audio.status {
                    Status::Running { .. } => {
                        dot(ui, theme::p().accent);
                        ui.label(RichText::new("Running").color(theme::p().accent));
                    }
                    Status::Starting => {
                        ui.spinner();
                        ui.label("Starting…");
                    }
                    Status::Lost(_) => {
                        warning(ui, "Disconnected: reconnects when it is back", 0.0);
                    }
                    Status::Off(why) => {
                        let why = if why.is_empty() { "Audio is off" } else { why };
                        warning(ui, why, 0.0);
                    }
                }
            }
        });
        // The notice fades in with a small drop, and fades out after Dismiss
        // (motion: BASE), showing its last text meanwhile.
        if let Some(notice) = &audio.notice {
            self.last_notice = Some(notice.clone());
        }
        let shown = crate::motion::fade(&ctx, ui.id().with("notice"), audio.notice.is_some());
        if shown == 0.0 {
            self.last_notice = None;
        }
        if let Some(notice) = self.last_notice.clone() {
            ui.add_space((1.0 - shown) * 6.0);
            ui.scope(|ui| {
                ui.multiply_opacity(shown);
                row(ui, "", |ui| {
                    // Leaves room for the Dismiss button.
                    let label = warning(ui, &notice, 72.0);
                    if let Some(detail) = &audio.notice_detail {
                        label.on_hover_text(detail);
                    }
                    if audio.notice.is_some() && ui.small_button("Dismiss").clicked() {
                        actions.push(Action::DismissNotice);
                    }
                });
            });
        }

        // Everything below describes the device in use.
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
                // When the device is off or lost, the status line above
                // already says why.
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

        // ── Format ──────────────────────────────────────────────────────
        section(ui, "Format");
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
            egui::ComboBox::from_id_salt("rate")
                .selected_text(rate.map_or("Device default".into(), khz))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for r in rates {
                        ui.selectable_value(&mut rate, Some(r), khz(r));
                    }
                });
            if rate != shown {
                actions.push(Action::Apply(AudioSettings {
                    sample_rate: rate,
                    ..current.clone()
                }));
            }
        });
        let frames = running.map(|i| i.buffer_frames);
        row(ui, "Buffer size", |ui| match chosen.as_str() {
            "asio" => {
                ui.label(frames.map_or("—".into(), |f| format!("{f} samples")));
                let opening = audio.control_panel_opening();
                let button = ui
                    .add_enabled(
                        matches!(audio.status, Status::Running { .. }) && !opening,
                        |ui: &mut egui::Ui| crate::widgets::raised::button(ui, "Control panel…"),
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
                        32, 48, 64, 96, 128, 144, 192, 256, 384, 441, 448, 480, 512, 768, 1024,
                        2048, 4096,
                    ]
                    .into_iter()
                    .filter(|&f| sizes.allows(f))
                    .collect();
                    if choices.is_empty() {
                        choices.push(sizes.preferred);
                    }
                    let shown = current.buffer_frames.or(frames);
                    let mut chosen_frames = shown;
                    egui::ComboBox::from_id_salt("buffer")
                        .selected_text(
                            chosen_frames
                                .map_or("Device default".into(), |f| format!("{f} samples")),
                        )
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for f in choices {
                                ui.selectable_value(
                                    &mut chosen_frames,
                                    Some(f),
                                    format!("{f} samples"),
                                );
                            }
                        });
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
        });
        if let Some(e) = &audio.control_panel_error {
            row(ui, "", |ui| {
                warning(ui, &format!("The control panel did not open: {e}"), 0.0);
            });
        }
        ui.add_space(theme::space::XS);
        ui.separator();

        // ── Channels ────────────────────────────────────────────────────
        section(ui, "Channels");
        let output_names = &info.output_channels;
        row(ui, "Output", |ui| {
            ui.vertical(|ui| {
                for (pair, label) in pairs(output_names, "Out") {
                    let channels = channels_of(pair, output_names);
                    let on = current.output_channels == channels;
                    if widgets::choice_row(ui, &label, on).clicked() && !on {
                        // A cue on the new main pair would be the same
                        // outputs: it goes.
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
        });
        // The cue pair (M4 slice 4, the owner's O1): headphones, for cued
        // tracks and, if chosen, the metronome. Any stereo pair but the
        // main one.
        let cue_pairs: Vec<(usize, String)> = pairs(output_names, "Out")
            .into_iter()
            .filter(|(pair, _)| {
                let channels = channels_of(*pair, output_names);
                channels.len() == 2 && channels != current.output_channels
            })
            .collect();
        ui.add_space(theme::space::S);
        row(ui, "Cue", |ui| {
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
        });
        if chosen == "asio" && !info.input_channels.is_empty() {
            let input_names = &info.input_channels;
            ui.add_space(theme::space::S);
            row(ui, "Input", |ui| {
                ui.vertical(|ui| {
                    let none = current.input_channels.is_empty();
                    if widgets::choice_row(ui, "None", none).clicked() && !none {
                        actions.push(Action::Apply(AudioSettings {
                            input_channels: Vec::new(),
                            ..current.clone()
                        }));
                    }
                    for (pair, label) in pairs(input_names, "In") {
                        let channels = channels_of(pair, input_names);
                        let on = current.input_channels == channels;
                        if widgets::choice_row(ui, &label, on).clicked() && !on {
                            actions.push(Action::Apply(AudioSettings {
                                input_channels: channels,
                                ..current.clone()
                            }));
                        }
                    }
                    hint(ui, "Recording and input meters arrive in Milestone 9");
                });
            });
        }
        ui.add_space(theme::space::XS);
        ui.separator();

        // ── Latency ─────────────────────────────────────────────────────
        section(ui, "Latency");
        match running {
            Some(i) => {
                let ms = |frames: u32| f64::from(frames) / f64::from(i.sample_rate) * 1000.0;
                let line = |ui: &mut Ui, name: &str, frames: u32| {
                    row(ui, name, |ui| {
                        // Right-aligned in a fixed width so the decimal
                        // points and the sample counts line up.
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
            }
            None => row(ui, "", |ui| hint(ui, "Shown once the device is running")),
        }
        ui.add_space(theme::space::XS);
        ui.separator();
        ui.add_space(theme::space::XS);
        let mut tone = test_tone;
        if widgets::switch(ui, &mut tone, "Test tone (1 kHz, −20 dBFS)", true)
            .on_hover_text("A steady tone on every output, to check the audio path")
            .changed()
        {
            actions.push(Action::TestTone(tone));
        }
    }
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
