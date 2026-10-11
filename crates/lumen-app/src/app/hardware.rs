//! MIDI tracks and the world outside the engine (M5 slice 1): choosing
//! instruments, keeping the MIDI outputs tracks name open, the sender that
//! times MIDI to them, the outputs the track menu offers, and Panic.

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui;
use lumen_devices::Preset;
use lumen_midi::{Direction, Use};

use crate::App;
use crate::midi::{Action, Midi};
use crate::midi_out::Output;
use crate::midi_play::used_ports;

/// An output the track menu offers, put in the egui context each frame
/// for menus deep in the arrangement to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuPort {
    /// What tracks save: the port's id name.
    pub id: String,
    /// What the menu shows.
    pub label: String,
    pub present: bool,
}

/// The outputs for the track menu: every output present, then any a track
/// of this project names that is not plugged in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MenuPorts(pub Vec<MenuPort>);

impl MenuPorts {
    /// Reads what the app put in `ctx` this frame.
    pub(crate) fn of(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<MenuPorts>(egui::Id::new("menu-midi-outputs")))
            .unwrap_or_default()
    }
}

impl App {
    /// Gives the selected MIDI tracks the synth on `preset`.
    pub(crate) fn choose_instrument(&mut self, preset: Preset) {
        let ids = self.selected_midi_tracks();
        self.document.set_instrument(&ids, Some(preset));
    }

    /// Panic: the engine stops and cuts everything (MIDI included), and the
    /// outputs in use are told directly too, in case it is not running.
    pub(crate) fn panic(&mut self) {
        self.release_computer_keyboard();
        self.transport.panic();
    }

    /// Plays `msg` from the computer keyboard on the tracks that listen.
    /// False when it could not go (no engine yet, or the queue is full).
    pub(crate) fn play_key(&mut self, msg: lumen_core::midi::Msg) -> bool {
        self.live_keys.as_mut().is_some_and(|keys| keys.send(msg))
    }

    /// Called every frame after the engine's lists are sent.
    pub(crate) fn update_midi_hardware(&mut self, ctx: &egui::Context) {
        // The live feed (inputs and the computer keyboard) goes to the
        // engine once; it plays on the audio thread from then on.
        if self.live_keys.is_none() {
            let (feed, keys) = crate::live::live(
                self.midi.take_input(),
                Arc::clone(&self.transport.audio.clock),
            );
            if self.transport.audio.midi.set_live(Box::new(feed)).is_ok() {
                self.live_keys = Some(keys);
            }
        }
        if std::mem::take(&mut self.transport.panic_midi) {
            self.midi.panic(self.lanes.midi_ports.names());
        }
        // The sender starts with the first track that names an output.
        if self.midi_out.is_none()
            && let Some(sender) = self.midi.sender()
            && let Some(queue) = self.transport.audio.midi.take_output()
        {
            self.midi_out = Some(Output::new(
                queue,
                sender,
                Arc::clone(&self.transport.audio.clock),
            ));
        }
        if let Some(out) = &mut self.midi_out {
            out.set_ports(self.lanes.midi_ports.names());
            let latency = self.transport.audio.info.as_ref();
            out.set_latency(latency.map_or(0, |i| i.output_latency));
        }
        // Outputs that tracks name are switched on for tracks, so they
        // open (and appear switched on in the MIDI settings).
        let state = (
            self.document.session(),
            self.document.state(),
            self.midi_ports_seen,
        );
        if self.midi_checked != Some(state) && self.open_used_outputs() {
            // Checked again only when the project or the ports change, unless
            // an output a track names is not listed yet (the list lags the
            // plug-in notice, or the device is away).
            self.midi_checked = Some(state);
        }
        let menu = self.menu_ports();
        if MenuPorts::of(ctx) != menu {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("menu-midi-outputs"), menu));
        }
    }

    /// Switches on for tracks every present output a track sends to.
    /// True when every output the tracks name is listed.
    fn open_used_outputs(&mut self) -> bool {
        let used = used_ports(self.document.project());
        if used.is_empty() {
            return true;
        }
        let ports = self.midi.ports();
        let listed = used.iter().all(|name| {
            ports
                .iter()
                .any(|p| p.info.id.direction == Direction::Output && &p.info.id.name == name)
        });
        for port in ports {
            let id = &port.info.id;
            if id.direction == Direction::Output && used.contains(&id.name) && !port.using.track {
                let using = Use {
                    track: true,
                    ..port.using
                };
                self.midi.apply(&Action::Use(id.clone(), using));
                Midi::remember(&mut self.settings.midi, id, using);
                self.settings_changed = Some(std::time::Instant::now());
            }
        }
        listed
    }

    fn menu_ports(&self) -> MenuPorts {
        let mut ports: Vec<MenuPort> = self
            .midi
            .ports()
            .into_iter()
            .filter(|p| p.info.id.direction == Direction::Output)
            .map(|p| MenuPort {
                id: p.info.id.name.clone(),
                label: p.info.name.clone(),
                present: true,
            })
            .collect();
        let named: BTreeSet<String> = used_ports(self.document.project());
        for name in named {
            if !ports.iter().any(|p| p.id == name) {
                ports.push(MenuPort {
                    label: name.clone(),
                    id: name,
                    present: false,
                });
            }
        }
        MenuPorts(ports)
    }
}
