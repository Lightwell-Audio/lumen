//! The Audio page's device rows: driver, device, rate, buffer and channels,
//! locked while a change is under way (ADR-0113).

use super::*;

/// Why the device controls are disabled while a change runs.
pub(super) const WAIT_FOR_CHANGE: &str =
    "Available once the device has answered the change under way";

/// Device controls: disabled while a change runs, saying why on hover.
pub(super) fn device_controls<R>(ui: &mut Ui, locked: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let shown = ui.add_enabled_ui(!locked, add);
    if locked {
        shown.response.on_hover_text(WAIT_FOR_CHANGE);
    }
    shown.inner
}

/// How wide a device or driver menu is drawn in the room the row has.
pub(super) fn device_menu_width(ui: &Ui) -> f32 {
    (ui.available_width() - BESIDE_DEVICE_MENU).clamp(120.0, DEVICE_MENU)
}

/// Channel pairs for a device: (first index, label). Drivers that give no
/// name get "Out 1", "In 2" and so on.
pub(super) fn pairs(names: &[String], prefix: &str) -> Vec<(usize, String)> {
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

pub(super) fn channels_of(pair: usize, names: &[String]) -> Vec<usize> {
    if pair + 1 < names.len() {
        vec![pair, pair + 1]
    } else {
        vec![pair]
    }
}

impl State {
    /// The Device section; returns the driver whose devices it shows.
    pub(super) fn device_rows(
        &mut self,
        ui: &mut Ui,
        audio: &Audio,
        marks: &Marks,
        locked: bool,
        actions: &mut Vec<Action>,
    ) -> String {
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
        let drivers = audio::drivers_for(&audio.driver);
        marks.row(ui, "audio.driver", |ui| {
            device_controls(ui, locked, |ui| {
                row(ui, "Driver", |ui| {
                    let labels: Vec<&str> = drivers.iter().map(|(_, label)| *label).collect();
                    let index = drivers.iter().position(|(key, _)| *key == driver);
                    let shown = driver_label(&driver);
                    if let Some(i) = Dropdown::new("Driver", shown, device_menu_width(ui)).show(
                        ui,
                        ui.id().with("driver"),
                        &labels,
                        index,
                    ) {
                        chosen = drivers[i].0.to_owned();
                    }
                })
            })
        });
        if chosen != driver {
            self.driver = Some(chosen.clone());
        }
        marks.row(ui, "audio.device", |ui| {
            device_controls(ui, locked, |ui| {
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
                            let names: Vec<&str> =
                                devices.iter().map(|(_, n)| n.as_str()).collect();
                            let index = devices
                                .iter()
                                .position(|(id, _)| Some(&id.id) == using.as_ref());
                            let pick = Dropdown::new("Device", selected, device_menu_width(ui))
                                .show(ui, ui.id().with("device"), &names, index)
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
            })
        });
        // What is happening right now, in one place.
        row(ui, "", |ui| {
            if let Some(why) = audio.not_answering() {
                warning(ui, &why, 0.0);
            } else if let Some(changing) = audio.changing() {
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

        chosen
    }
}
