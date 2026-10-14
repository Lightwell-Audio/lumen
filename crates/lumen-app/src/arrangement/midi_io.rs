//! Where a MIDI track listens and plays to (M5 slice 2, the owner's R A):
//! the choices as menus, shared by the track menu and the mixer strip's
//! From and To rows, and the Monitor switch.

use eframe::egui::{Color32, FontId, Id as EguiId, Rect, Ui};
use lumen_devices::Preset;
use lumen_project::model::{FromSource, Id, MidiFrom, MidiTo, Monitor, Track, TrackKind};

use super::Arrangement;
use super::strip_output::row_button;
use crate::app::hardware::{MenuInputs, MenuPorts};
use crate::document::Document;
use crate::{theme, widgets};

/// A MIDI input as a strip shows it: "All ins", "Keyboard" or the port's
/// name, then the channel if one is chosen.
pub(super) fn from_text(from: &MidiFrom, inputs: &MenuInputs) -> String {
    let source = match &from.source {
        FromSource::All => "All ins".to_owned(),
        FromSource::Keyboard => "Keyboard".to_owned(),
        FromSource::Port(name) => inputs
            .0
            .iter()
            .find(|p| p.id == *name)
            .map_or_else(|| name.clone(), |p| p.label.clone()),
    };
    match from.channel {
        Some(c) => format!("{source} \u{b7} {}", c + 1),
        None => source,
    }
}

/// Where a MIDI track plays, as a strip shows it: the instrument's sound,
/// or the output port and channel.
pub(super) fn to_text(track: &Track, outputs: &MenuPorts) -> String {
    match &track.midi_to {
        Some(out) => {
            let name = outputs
                .0
                .iter()
                .find(|p| p.id == out.port)
                .map_or(out.port.as_str(), |p| p.label.as_str());
            format!("{name} \u{b7} {}", out.channel + 1)
        }
        None => track
            .instrument()
            .map(|d| d.preset().and_then(Preset::from_id).unwrap_or_default())
            .map_or_else(|| "None".to_owned(), |p| p.name().to_owned()),
    }
}

/// The choices of what `midi` tracks listen to: all inputs, the computer
/// keyboard, then each input present with its channels, and the inputs the
/// tracks name that are not plugged in, dimmed.
pub(super) fn from_menu(ui: &mut Ui, doc: &mut Document, midi: &[Id], current: &MidiFrom) {
    let inputs = MenuInputs::of(ui.ctx());
    let set = |doc: &mut Document, ui: &mut Ui, source: FromSource, channel: Option<u8>| {
        doc.set_midi_from(midi, MidiFrom { source, channel });
        ui.close();
    };
    if widgets::menu_toggle(ui, "All inputs", None, current.source == FromSource::All).clicked() {
        set(doc, ui, FromSource::All, None);
    }
    if widgets::menu_toggle(
        ui,
        "Computer keyboard",
        None,
        current.source == FromSource::Keyboard,
    )
    .clicked()
    {
        set(doc, ui, FromSource::Keyboard, None);
    }
    ui.separator();
    let present: Vec<_> = inputs.0.iter().filter(|p| p.present).collect();
    if present.is_empty() {
        widgets::menu_item(ui, "No MIDI inputs", None, false);
    }
    for port in present {
        let mine = current.source == FromSource::Port(port.id.clone());
        widgets::submenu(ui, port.label.clone(), |ui| {
            if widgets::menu_toggle(ui, "All channels", None, mine && current.channel.is_none())
                .clicked()
            {
                set(doc, ui, FromSource::Port(port.id.clone()), None);
            }
            for channel in 0..16_u8 {
                let name = format!("Channel {}", channel + 1);
                let on = mine && current.channel == Some(channel);
                if widgets::menu_toggle(ui, &name, None, on).clicked() {
                    set(doc, ui, FromSource::Port(port.id.clone()), Some(channel));
                }
            }
        });
    }
    let missing: Vec<_> = inputs.0.iter().filter(|p| !p.present).collect();
    if !missing.is_empty() {
        ui.separator();
        widgets::menu_caption(ui, "Not connected");
        for port in missing {
            widgets::menu_item(ui, &port.label, None, false);
        }
    }
}

/// The choices of where `midi` tracks play: the instrument, or an output
/// port and channel.
pub(super) fn to_menu(ui: &mut Ui, doc: &mut Document, midi: &[Id], to: &Option<MidiTo>) {
    let ports = MenuPorts::of(ui.ctx());
    if widgets::menu_toggle(ui, "Instrument", None, to.is_none()).clicked() {
        doc.set_midi_to(midi, None);
        ui.close();
    }
    ui.separator();
    if ports.0.is_empty() {
        widgets::menu_item(ui, "No MIDI outputs", None, false);
    }
    for port in &ports.0 {
        let label = if port.present {
            port.label.clone()
        } else {
            format!("{} (not connected)", port.label)
        };
        widgets::submenu(ui, label, |ui| {
            for channel in 0..16_u8 {
                let chosen = to
                    .as_ref()
                    .is_some_and(|t| t.port == port.id && t.channel == channel);
                let name = format!("Channel {}", channel + 1);
                if widgets::menu_toggle(ui, &name, None, chosen).clicked() {
                    let out = MidiTo {
                        port: port.id.clone(),
                        channel,
                    };
                    doc.set_midi_to(midi, Some(out));
                    ui.close();
                }
            }
        });
    }
}

/// The Monitor row of the track menu: a caption and In, Auto, Off.
pub(super) fn monitor_row(ui: &mut Ui, doc: &mut Document, ids: &[Id], current: Monitor) {
    widgets::menu_caption(ui, "Monitor");
    let choices = [
        widgets::Choice::new(Monitor::In, "In"),
        widgets::Choice::new(Monitor::Auto, "Auto"),
        widgets::Choice::new(Monitor::Off, "Off"),
    ];
    if let Some(monitor) = widgets::segmented(ui, ui.id().with("monitor"), current, &choices) {
        doc.set_monitor(ids, monitor);
        ui.close();
    }
}

/// Whether "From" and "To" fit before their values in a strip's rows of
/// width `row`: when either does not, both rows show the values alone (the
/// tooltips and names say which is which), so the pair reads alike.
fn prefixes_fit(ui: &Ui, row: Rect, track: &Track) -> bool {
    let from = from_text(&track.midi_from, &MenuInputs::of(ui.ctx()));
    let to = to_text(track, &MenuPorts::of(ui.ctx()));
    let font = FontId::proportional(theme::size::CAPTION);
    // The room `row_button` gives text.
    let room = row.width() - 20.0;
    [format!("From  {from}"), format!("To  {to}")]
        .into_iter()
        .all(|text| {
            ui.painter()
                .layout_no_wrap(text, font.clone(), Color32::WHITE)
                .size()
                .x
                <= room
        })
}

impl Arrangement {
    /// A MIDI strip's From row; a click opens the choices.
    pub(super) fn input_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        row: Rect,
        id: EguiId,
    ) {
        let text = from_text(&track.midi_from, &MenuInputs::of(ui.ctx()));
        let name = format!("{} MIDI From in the mixer: {text}", track.name);
        let response = row_button(
            ui,
            row,
            id,
            "from",
            &if prefixes_fit(ui, row, track) {
                format!("From  {text}")
            } else {
                text
            },
            &name,
            "Where this track listens",
        );
        let midi = self.midi_targets(doc, track.id);
        let current = track.midi_from.clone();
        widgets::Menu::below(&response).show(|ui| {
            ui.set_min_width(160.0);
            from_menu(ui, doc, &midi, &current);
        });
    }

    /// A MIDI strip's To row; a click opens the choices.
    pub(super) fn output_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        row: Rect,
        id: EguiId,
    ) {
        let text = to_text(track, &MenuPorts::of(ui.ctx()));
        let name = format!("{} MIDI To in the mixer: {text}", track.name);
        let response = row_button(
            ui,
            row,
            id,
            "to",
            &if prefixes_fit(ui, row, track) {
                format!("To  {text}")
            } else {
                text
            },
            &name,
            "What this track plays",
        );
        let midi = self.midi_targets(doc, track.id);
        let current = track.midi_to.clone();
        widgets::Menu::below(&response).show(|ui| {
            ui.set_min_width(160.0);
            to_menu(ui, doc, &midi, &current);
        });
    }

    /// The MIDI tracks a change on `id` applies to: the selection when `id`
    /// is in it, otherwise `id`.
    pub(super) fn midi_targets(&self, doc: &Document, id: Id) -> Vec<Id> {
        let targets = self.targets(id);
        doc.project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Midi && targets.contains(&t.id))
            .map(|t| t.id)
            .collect()
    }
}
