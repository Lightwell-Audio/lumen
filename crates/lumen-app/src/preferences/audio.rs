//! The Audio page of Preferences (M2 slice 8; a page of Preferences since
//! M9 R7): driver, device, sample rate, buffer size, channels, latencies,
//! the ASIO control panel and the test tone. Every change applies at once;
//! a change the device refuses reverts to the setup that worked, with the
//! reason shown.
//!
//! Device lists and details load in the background and never block the
//! window. ASIO drivers are queried in a helper process (`probe.rs`).

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui::{self, RichText, Ui};
use lumen_audio::{DeviceId, DeviceInfo};

use super::Marks;
use crate::audio::{self, Audio, DRIVERS, Status, driver_label};
use crate::measure_latency::{Asked, Cable, Card, Shown};
use crate::settings::{AudioSettings, MeasuredWith};
use crate::widgets::{hint, khz, row, section, warning};
use crate::{theme, widgets};
use widgets::Dropdown;

/// Rates offered for the device that is running (a running ASIO driver
/// reports only its current rate).
const COMMON_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// What the page asks the transport and the app to do.
pub enum Action {
    Apply(AudioSettings),
    ControlPanel,
    TestTone(bool),
    /// The recording offset, in milliseconds (M9 slice R2).
    RecordOffset(f32),
    /// "Play audio from disk" on or off (M9 slice R6).
    FromDisk(bool),
    /// Measure the round trip through a cable (M9 slice R2b).
    Measure(Cable),
    /// Make a measured offset the recording offset, remembering what it was
    /// measured with.
    UseMeasured(f32, MeasuredWith),
    DismissNotice,
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

/// The page between frames (not saved).
#[derive(Default)]
pub struct State {
    catalog: Catalog,
    /// The driver whose devices are shown (may differ from the one in use
    /// until a device is chosen).
    driver: Option<String>,
    /// The notice shown last, kept while it fades out.
    last_notice: Option<String>,
    /// The loopback helper's card in the Latency section.
    measure: Card,
}

impl State {
    /// Unfolds the loopback helper's card and brings it into view.
    pub fn open_measure(&mut self) {
        self.measure.open = true;
        self.measure.reveal = true;
    }

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
    /// The input channel names of `driver`'s device `id` (the counterpart of
    /// [`State::output_names`]). `None` while looking.
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

/// The widest the device and driver menus are drawn, and the room kept
/// beside them for the look-again button.
const DEVICE_MENU: f32 = 280.0;
const BESIDE_DEVICE_MENU: f32 = 40.0;

/// The recording offset field: milliseconds added to the latency the driver
/// reports, for drivers that report it wrongly; and the loopback helper's
/// Measure… button, which unfolds its card.
fn record_offset(ui: &mut Ui, offset_ms: f32, measure: &mut bool, actions: &mut Vec<Action>) {
    row(ui, "Recording offset", |ui| {
        let mut ms = offset_ms;
        let field = ui
            .add(
                egui::DragValue::new(&mut ms)
                    .range(-500.0..=500.0)
                    .update_while_editing(false)
                    .speed(0.1)
                    .max_decimals(1)
                    .suffix(" ms"),
            )
            .on_hover_text(
                "Moves what you record earlier (positive) or later (negative), after the latency the driver reports has been removed. Double-click to reset.",
            );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Recording offset")
        });
        if field.double_clicked() {
            ms = 0.0;
        }
        if (ms - offset_ms).abs() > f32::EPSILON {
            actions.push(Action::RecordOffset(ms));
        }
        let button = crate::widgets::raised::button(
            ui,
            if *measure { "Hide measuring" } else { "Measure…" },
        )
        .on_hover_text(
            "Measures the real round trip with a cable from an output to an input, and offers              the offset it asks for",
        );
        if button.clicked() {
            *measure = !*measure;
        }
    });
}

/// Says so when the offset was measured on another device setup than the
/// one running.
fn measured_elsewhere(ui: &mut Ui, with: &MeasuredWith, running: &MeasuredWith) {
    if with.same_setup(running) {
        return;
    }
    row(ui, "", |ui| {
        let width = ui.available_width().max(120.0);
        ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
            ui.set_max_width(width);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Measured with {} · {} at {}, {} samples. Measure again.",
                        driver_label(&with.driver),
                        with.device_name,
                        khz(with.sample_rate),
                        with.buffer_frames
                    ))
                    .small()
                    .weak(),
                )
                .wrap(),
            )
        });
    });
}

/// A small filled dot, drawn rather than typed (the UI font has no "●").
fn dot(ui: &mut Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

/// How wide a device or driver menu is drawn in the room the row has.
fn device_menu_width(ui: &Ui) -> f32 {
    (ui.available_width() - BESIDE_DEVICE_MENU).clamp(120.0, DEVICE_MENU)
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

/// What the page shows of settings it does not own, lent each frame.
#[derive(Debug, Clone, Copy)]
pub struct Lent<'a> {
    pub test_tone: bool,
    /// The recording offset, in milliseconds.
    pub record_offset_ms: f32,
    /// What the offset was measured with, if the loopback helper set it.
    pub offset_measured: Option<&'a MeasuredWith>,
    /// Why the loopback helper cannot measure now (playing, recording).
    pub measure_held: Option<&'static str>,
    /// "Play audio from disk".
    pub from_disk: bool,
}

impl State {
    /// The page: what it asks for goes in `actions`. `marks` outlines the
    /// row a search led to.
    pub fn page(
        &mut self,
        ui: &mut Ui,
        audio: &Audio,
        lent: Lent<'_>,
        marks: &Marks,
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
        let current = audio.settings();
        let driver = self
            .driver
            .clone()
            .or_else(|| current.as_ref().map(|s| s.driver.clone()))
            .unwrap_or_else(|| audio::DEFAULT_DRIVER.to_owned());

        // ── Device ──────────────────────────────────────────────────────
        section(ui, "Device");
        let mut chosen = driver.clone();
        marks.row(ui, "audio.driver", |ui| {
            row(ui, "Driver", |ui| {
                let labels: Vec<&str> = DRIVERS.iter().map(|(_, label)| *label).collect();
                let index = DRIVERS.iter().position(|(key, _)| *key == driver);
                let shown = driver_label(&driver);
                if let Some(i) = Dropdown::new("Driver", shown, device_menu_width(ui)).show(
                    ui,
                    egui::Id::new("driver"),
                    &labels,
                    index,
                ) {
                    chosen = DRIVERS[i].0.to_owned();
                }
            })
        });
        if chosen != driver {
            self.driver = Some(chosen.clone());
        }
        marks.row(ui, "audio.device", |ui| {
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
                        let names: Vec<&str> = devices.iter().map(|(_, n)| n.as_str()).collect();
                        let index = devices
                            .iter()
                            .position(|(id, _)| Some(&id.id) == using.as_ref());
                        let pick = Dropdown::new("Device", selected, device_menu_width(ui))
                            .show(ui, egui::Id::new("device"), &names, index)
                            .map(|i| devices[i].clone());
                        if let Some((id, name)) = pick {
                            actions.push(Action::Apply(AudioSettings {
                                driver: chosen.clone(),
                                device_id: id.id,
                                device_name: name,
                                sample_rate: None,
                                buffer_frames: None,
                                input_channels: Vec::new(),
                                all_inputs: true,
                                output_channels: vec![0, 1],
                                cue_channels: Vec::new(),
                            }));
                            self.driver = None;
                        }
                    }
                }
                if crate::widgets::raised::button(ui, "⟳")
                    .on_hover_text("Look for devices again (after plugging one in or out)")
                    .clicked()
                {
                    self.catalog.refresh();
                }
            })
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
                    let label = warning(ui, &notice, 84.0);
                    if let Some(detail) = &audio.notice_detail {
                        label.on_hover_text(detail);
                    }
                    if audio.notice.is_some()
                        && crate::widgets::raised::button(ui, "Dismiss").clicked()
                    {
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
        marks.row(ui, "audio.rate", |ui| {
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
        });
        let frames = running.map(|i| i.buffer_frames);
        marks.row(ui, "audio.buffer", |ui| {
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
                        let labels: Vec<String> =
                            choices.iter().map(|f| format!("{f} samples")).collect();
                        let index = shown.and_then(|s| choices.iter().position(|f| *f == s));
                        let text =
                            shown.map_or("Device default".to_owned(), |f| format!("{f} samples"));
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
        marks.row(ui, "audio.output", |ui| {
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
            })
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
        marks.row(ui, "audio.cue", |ui| row(ui, "Cue", |ui| {
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
        }));
        if !info.input_channels.is_empty() {
            let input_names = &info.input_channels;
            ui.add_space(theme::space::S);
            marks.row(ui, "audio.input", |ui| row(ui, "Input", |ui| {
                ui.vertical(|ui| {
                    // All inputs (the default) lets every track pick any
                    // input; a pair or None narrows what the device opens.
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
                });
            }));
        }
        ui.add_space(theme::space::XS);
        ui.separator();

        // ── Latency ─────────────────────────────────────────────────────
        marks.row(ui, "audio.latency", |ui| section(ui, "Latency"));
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
