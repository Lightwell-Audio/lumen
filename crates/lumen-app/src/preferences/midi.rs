//! The MIDI page of Preferences (M5 slice 0; a page of Preferences since M9
//! R7): every input and output with its switches, and what to do when there
//! are none. Outputs have Sync (MIDI clock, M5 slice 7) and, while it is
//! on, its delay.

use eframe::egui::{self, RichText, Ui};
use lumen_midi::{Direction, PortState, Use};

use super::Marks;
use crate::midi::{Action, Midi, SYNC_DELAY_MS};
use crate::widgets::{SliderKind, hint, section, warning};
use crate::{settings, theme, widgets};

/// What the page shows: the ports, and what is saved for them (the clock
/// delays).
#[derive(Clone, Copy)]
pub struct Shown<'a> {
    pub midi: &'a Midi,
    pub saved: &'a settings::Midi,
}

/// The delay field's width.
const DELAY_WIDTH: f32 = 64.0;

/// Room for a port's name, so a long one is cut short rather than pushing
/// the switches out of the window.
const NAME_WIDTH: f32 = 196.0;

/// The ports. `marks` outlines the row a search led to.
pub fn page(ui: &mut Ui, shown: Shown<'_>, marks: &Marks, actions: &mut Vec<Action>) {
    let midi = shown.midi;
    if let Some(why) = midi.unavailable() {
        ui.add_space(theme::space::M);
        warning(ui, &why, 0.0);
        return;
    }
    let ports = midi.ports();
    let (inputs, outputs): (Vec<_>, Vec<_>) = ports
        .iter()
        .partition(|p| p.info.id.direction == Direction::Input);
    marks.row(ui, "midi.inputs", |ui| section(ui, "Inputs"));
    list(ui, &inputs, "No MIDI inputs found", shown.saved, actions);
    marks.row(ui, "midi.outputs", |ui| section(ui, "Outputs"));
    list(ui, &outputs, "No MIDI outputs found", shown.saved, actions);
    ui.add_space(theme::space::S);
    hint(
        ui,
        "Track: tracks may use the port. Remote: controls can be mapped to it. \
         Sync: it gets MIDI clock while the song plays, moved by its delay. \
         Devices appear when plugged in.",
    );
    ui.add_space(theme::space::XS);
    marks.row(ui, "midi.rescan", |ui| {
        if crate::widgets::raised::button(ui, "Look for devices")
            .on_hover_text("Check for new ports now rather than at the next poll")
            .clicked()
        {
            actions.push(Action::Rescan);
        }
    });
    // New devices appear while the page is open.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(300));
}

fn list(
    ui: &mut Ui,
    ports: &[&PortState],
    empty: &str,
    saved: &settings::Midi,
    actions: &mut Vec<Action>,
) {
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
            } else {
                sync(ui, name, &mut using, saved, actions);
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

/// An output's Sync switch, and its clock delay while it is on (the
/// owner's F3 and F4).
fn sync(
    ui: &mut Ui,
    name: &str,
    using: &mut Use,
    saved: &settings::Midi,
    actions: &mut Vec<Action>,
) {
    ui.label(RichText::new("Sync").small().weak());
    let mut on = using.sync;
    let response = widgets::switch(ui, &mut on, "", true)
        .on_hover_text("Sends MIDI clock, Start, Stop and the song position while the song plays");
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            on,
            format!("Sync {name} output"),
        )
    });
    using.sync = on;
    if !on {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(DELAY_WIDTH, theme::size::CONTROL_SMALL),
        egui::Sense::hover(),
    );
    let (min, max) = SYNC_DELAY_MS;
    let delay = Midi::sync_delay(saved, name);
    let kind = SliderKind::Offset {
        min_ms: min as f32,
        max_ms: max as f32,
    };
    let chosen = widgets::value_field(
        ui,
        rect,
        egui::Id::new(("sync delay", name)),
        delay as f32,
        kind,
        &format!("Sync delay of {name}"),
        theme::controls(false),
    )
    .round() as i32;
    if chosen != delay {
        actions.push(Action::SyncDelay(name.to_owned(), chosen));
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
