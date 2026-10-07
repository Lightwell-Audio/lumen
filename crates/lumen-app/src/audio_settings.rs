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
use crate::theme;

/// Rates offered for the device that is running (a running ASIO driver
/// reports only its current rate).
const COMMON_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// What the window asks the transport to do.
pub enum Action {
    Apply(AudioSettings),
    ControlPanel,
    TestTone(bool),
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
                    Some(_) if driver == "asio" => Ok(audio::listed(
                        &driver,
                        audio::backend(&driver).expect("checked"),
                    )),
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
    control_panel_error: Option<String>,
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
                format!("{}/{}  ({} / {})", i + 1, i + 2, name(i), name(i + 1)),
            ),
            None => (i, format!("{}  ({})", i + 1, name(i))),
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
    pub fn show(&mut self, ctx: &egui::Context, audio: &Audio, test_tone: bool) -> Vec<Action> {
        let mut actions = Vec::new();
        if !self.open {
            return actions;
        }
        self.catalog.poll();
        let mut open = self.open;
        let top_centre = egui::pos2(ctx.content_rect().center().x, 72.0);
        egui::Window::new("Audio settings")
            .open(&mut open)
            .pivot(egui::Align2::CENTER_TOP)
            .default_pos(top_centre)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| self.contents(ui, audio, test_tone, &mut actions));
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

        egui::Grid::new("audio settings")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                // Driver.
                ui.label("Driver");
                let mut chosen = driver.clone();
                egui::ComboBox::from_id_salt("driver")
                    .selected_text(driver_label(&driver))
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (key, label) in DRIVERS {
                            ui.selectable_value(&mut chosen, (*key).to_owned(), *label);
                        }
                    });
                if chosen != driver {
                    self.driver = Some(chosen.clone());
                }
                ui.end_row();

                // Device.
                ui.label("Device");
                ui.horizontal(|ui| {
                    match self.catalog.list(&chosen, &ctx) {
                        None => {
                            ui.spinner();
                            ui.label("Looking for devices…");
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(e).color(theme::WARNING));
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
                                .width(260.0)
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
                                    // ASIO devices record as well as play: use the
                                    // first input pair when there is one.
                                    input_channels: if chosen == "asio" {
                                        vec![0, 1]
                                    } else {
                                        Vec::new()
                                    },
                                    output_channels: vec![0, 1],
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
                ui.end_row();

                // The rest only applies to the device in use.
                let Some(current) = current.as_ref().filter(|s| s.driver == chosen) else {
                    return;
                };
                let details = self
                    .catalog
                    .details(&chosen, &current.device_id, &ctx)
                    .cloned();
                let info = match details {
                    None => {
                        ui.label("");
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Checking the device…");
                        });
                        ui.end_row();
                        return;
                    }
                    Some(Err(e)) => {
                        ui.label("");
                        ui.label(RichText::new(e).color(theme::WARNING));
                        ui.end_row();
                        return;
                    }
                    Some(Ok(info)) => info,
                };
                let running = audio.info.as_ref();

                // Sample rate.
                ui.label("Sample rate");
                if chosen == "wasapi" {
                    ui.label(format!(
                        "{} (set in the Windows sound settings)",
                        running.map_or("—".into(), |i| format!(
                            "{:.1} kHz",
                            f64::from(i.sample_rate) / 1000.0
                        ))
                    ));
                } else {
                    let rates: Vec<u32> = if chosen == "asio" {
                        COMMON_RATES.to_vec()
                    } else {
                        info.sample_rates.clone()
                    };
                    let now = running.map(|i| i.sample_rate);
                    let mut rate = current.sample_rate.or(now);
                    egui::ComboBox::from_id_salt("rate")
                        .selected_text(rate.map_or("Device default".into(), |r| {
                            format!("{:.1} kHz", f64::from(r) / 1000.0)
                        }))
                        .show_ui(ui, |ui| {
                            for r in rates {
                                ui.selectable_value(
                                    &mut rate,
                                    Some(r),
                                    format!("{:.1} kHz", f64::from(r) / 1000.0),
                                );
                            }
                        });
                    if rate != current.sample_rate.or(now) {
                        actions.push(Action::Apply(AudioSettings {
                            sample_rate: rate,
                            ..current.clone()
                        }));
                    }
                }
                ui.end_row();

                // Buffer size.
                ui.label("Buffer size");
                let frames = running.map(|i| i.buffer_frames);
                match chosen.as_str() {
                    "asio" => {
                        ui.horizontal(|ui| {
                            ui.label(frames.map_or("—".into(), |f| format!("{f} samples")));
                            ui.label(
                                RichText::new("set in the driver's control panel")
                                    .small()
                                    .weak(),
                            );
                        });
                    }
                    "wasapi" => {
                        ui.label(format!(
                            "{} (set by Windows)",
                            frames.map_or("—".into(), |f| format!("{f} samples"))
                        ));
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
                            let mut chosen_frames = current.buffer_frames.or(frames);
                            egui::ComboBox::from_id_salt("buffer")
                                .selected_text(
                                    chosen_frames.map_or("Device default".into(), |f| {
                                        format!("{f} samples")
                                    }),
                                )
                                .show_ui(ui, |ui| {
                                    for f in choices {
                                        ui.selectable_value(
                                            &mut chosen_frames,
                                            Some(f),
                                            format!("{f} samples"),
                                        );
                                    }
                                });
                            if chosen_frames != current.buffer_frames.or(frames) {
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
                }
                ui.end_row();

                // Channels.
                for (title, names, selected, is_input) in [
                    (
                        "Outputs",
                        &info.output_channels,
                        &current.output_channels,
                        false,
                    ),
                    (
                        "Inputs",
                        &info.input_channels,
                        &current.input_channels,
                        true,
                    ),
                ] {
                    if names.is_empty() || (is_input && chosen != "asio") {
                        continue;
                    }
                    ui.label(title);
                    ui.vertical(|ui| {
                        // One pair is used at a time in M2; the mixer (M4) routes
                        // more.
                        let prefix = if is_input { "In" } else { "Out" };
                        for (pair, label) in pairs(names, prefix) {
                            let channels = channels_of(pair, names);
                            let on = *selected == channels;
                            if ui.radio(on, label).clicked() && !on {
                                let mut next = current.clone();
                                if is_input {
                                    next.input_channels = channels;
                                } else {
                                    next.output_channels = channels;
                                }
                                actions.push(Action::Apply(next));
                            }
                        }
                        if is_input {
                            ui.label(
                                RichText::new("Recording and input meters arrive in Milestone 9")
                                    .small()
                                    .weak(),
                            );
                        }
                    });
                    ui.end_row();
                }

                // Latency.
                ui.label("Latency");
                match running {
                    Some(i) => {
                        let ms =
                            |frames: u32| f64::from(frames) / f64::from(i.sample_rate) * 1000.0;
                        let text = if i.input_channels == 0 {
                            format!("output {:.1} ms (no inputs in use)", ms(i.output_latency))
                        } else {
                            format!(
                                "input {:.1} ms · output {:.1} ms · round trip {:.1} ms",
                                ms(i.input_latency),
                                ms(i.output_latency),
                                i.round_trip_ms()
                            )
                        };
                        ui.label(text).on_hover_text(
                            "As the driver reports it, including its safety buffers",
                        );
                    }
                    None => {
                        ui.label("—");
                    }
                }
                ui.end_row();
            });

        ui.separator();
        ui.horizontal(|ui| {
            let running = matches!(audio.status, Status::Running { .. });
            if audio.driver == "asio"
                && ui
                    .add_enabled(running, egui::Button::new("Control panel…"))
                    .on_hover_text("Open the driver's own settings (buffer size, clock source)")
                    .on_disabled_hover_text("The driver must be running")
                    .clicked()
            {
                actions.push(Action::ControlPanel);
            }
            let mut tone = test_tone;
            if ui.checkbox(&mut tone, "Test tone").changed() {
                actions.push(Action::TestTone(tone));
            }
        });
        if let Some(e) = &self.control_panel_error {
            ui.label(RichText::new(e).color(theme::WARNING));
        }
        if let Some(notice) = &audio.notice {
            ui.label(RichText::new(notice).color(theme::WARNING));
        }
        match &audio.status {
            Status::Starting => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Starting…");
                });
            }
            Status::Lost(why) => {
                ui.label(
                    RichText::new(format!("Device lost, reconnecting: {why}"))
                        .color(theme::WARNING),
                );
            }
            Status::Off(why) if !why.is_empty() => {
                ui.label(RichText::new(why).color(theme::WARNING));
            }
            _ => {}
        }
    }

    /// Records the result of opening the control panel.
    pub fn control_panel_result(&mut self, result: Result<(), String>) {
        self.control_panel_error = result.err();
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
