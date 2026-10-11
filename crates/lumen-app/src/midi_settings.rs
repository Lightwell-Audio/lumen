//! The MIDI page of the settings window (M5 slice 0): every input and
//! output with its switches, and what to do when there are none.

use eframe::egui::{self, RichText, Ui};
use lumen_midi::{Direction, PortState, Use};

use crate::audio_settings::{hint, section, warning};
use crate::midi::{Action, Midi};
use crate::{theme, widgets};

/// Room for a port's name, so a long one is cut short rather than pushing
/// the switches out of the window.
const NAME_WIDTH: f32 = 196.0;

pub fn page(ui: &mut Ui, midi: &Midi, actions: &mut Vec<Action>) {
    if let Some(why) = midi.unavailable() {
        ui.add_space(theme::space::M);
        warning(ui, &why, 0.0);
        return;
    }
    let ports = midi.ports();
    let (inputs, outputs): (Vec<_>, Vec<_>) = ports
        .iter()
        .partition(|p| p.info.id.direction == Direction::Input);
    section(ui, "Inputs");
    list(ui, &inputs, "No MIDI inputs found", actions);
    section(ui, "Outputs");
    list(ui, &outputs, "No MIDI outputs found", actions);
    ui.add_space(theme::space::S);
    hint(
        ui,
        "Track: tracks may use the port. Remote: controls can be mapped to it. \
         Devices appear when plugged in.",
    );
    ui.add_space(theme::space::XS);
    if crate::widgets::raised::button(ui, "Look for devices")
        .on_hover_text("Check for new ports now rather than at the next poll")
        .clicked()
    {
        actions.push(Action::Rescan);
    }
}

fn list(ui: &mut Ui, ports: &[&PortState], empty: &str, actions: &mut Vec<Action>) {
    if ports.is_empty() {
        hint(ui, empty);
        return;
    }
    for port in ports {
        let mut using = port.using;
        ui.horizontal(|ui| {
            dot(ui, port);
            ui.add_sized(
                [NAME_WIDTH, theme::size::CONTROL_SMALL],
                egui::Label::new(&port.info.name).truncate(),
            )
            .on_hover_text(&port.info.name);
            let is_input = port.info.id.direction == Direction::Input;
            let name = &port.info.name;
            let what = if is_input { "input" } else { "output" };
            ui.label(RichText::new("Track").small().weak());
            let mut track = using.track;
            let response = widgets::switch(ui, &mut track, "", true).on_hover_text(if is_input {
                "Tracks can record from and play through this input"
            } else {
                "Tracks can send to this output"
            });
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    track,
                    format!("Track {name} {what}"),
                )
            });
            using.track = track;
            if is_input {
                ui.label(RichText::new("Remote").small().weak());
                let mut remote = using.remote;
                let response = widgets::switch(ui, &mut remote, "", true)
                    .on_hover_text("Controls can be mapped to this input (MIDI learn)");
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        true,
                        remote,
                        format!("Remote {name} {what}"),
                    )
                });
                using.remote = remote;
            }
        });
        if let Some(why) = &port.error {
            warning(ui, &format!("Could not open: {why}"), 0.0);
        }
        if using != port.using {
            actions.push(Action::Use(port.info.id.clone(), Use { ..using }));
        }
    }
}

/// Open ports are lit, failed ones warn, the rest are quiet.
fn dot(ui: &mut Ui, port: &PortState) {
    let palette = theme::p();
    let colour = if port.error.is_some() {
        palette.warning
    } else if port.open {
        palette.accent
    } else {
        palette.text_disabled
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, colour);
    response.on_hover_text(if port.error.is_some() {
        "Could not open"
    } else if port.open {
        "Open"
    } else {
        "Not in use"
    });
}
