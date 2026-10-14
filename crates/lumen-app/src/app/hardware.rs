//! MIDI tracks and the world outside the engine (M5 slice 1): choosing
//! instruments, keeping the MIDI outputs tracks name open, the sender that
//! times MIDI to them, the outputs the track menu offers, and Panic.

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui;
use lumen_devices::Preset;
use lumen_midi::{Direction, Use};

use crate::App;
use crate::audition::Tone;
use crate::midi::{Action, Midi};
use crate::midi_out::Output;
use crate::midi_play;
use crate::midi_play::used_ports;
use lumen_project::model::Id;

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

/// The inputs for the track menu: every input present, then any a MIDI track
/// of this project listens to that is not plugged in. In the egui context
/// like [`MenuPorts`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MenuInputs(pub Vec<MenuPort>);

impl MenuInputs {
    pub(crate) fn of(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<MenuInputs>(egui::Id::new("menu-midi-inputs")))
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
        self.audition.forget();
        self.transport.panic();
    }

    /// Plays `msg` from the computer keyboard on the tracks that listen.
    /// False when it could not go (no engine yet, or the queue is full).
    pub(crate) fn play_key(&mut self, msg: lumen_core::midi::Msg) -> bool {
        self.live_keys.as_mut().is_some_and(|keys| keys.send(msg))
    }

    /// Previews `pitch` on `track` (the piano roll, M5 slice 3d): a short
    /// note that lets go of the last preview. Nothing if auditioning is off
    /// in the settings or the engine is not there. True if a note started.
    pub(crate) fn audition_play(&mut self, track: Id, pitch: u8, velocity: u8) -> bool {
        self.audition_tones(track, &[Tone { pitch, velocity }], true)
    }

    /// Previews the notes `tones` on `track`: `fresh` starts a new preview,
    /// letting go of the last; otherwise the preview of the gesture moves on
    /// to `tones`, striking only the notes whose pitch changed (the moved
    /// notes of the piano roll, M5 slice 3d). True if a note started.
    pub(crate) fn audition_tones(&mut self, track: Id, tones: &[Tone], fresh: bool) -> bool {
        self.audition.set_enabled(self.settings.midi.audition);
        let now = std::time::Instant::now();
        // On a hardware track, a key the track's own clip sounds now is not
        // struck (its off would end the clip's note): `busy_keys`.
        let busy = midi_play::busy_keys(
            self.document.project(),
            track,
            self.transport.position(),
            self.transport.state().playing,
        );
        match self.live_keys.as_mut() {
            Some(keys) if fresh => self.audition.play_chord(keys, track, tones, &busy, now),
            Some(keys) => self.audition.change(keys, track, tones, &busy, now),
            None => false,
        }
    }

    /// Lets go of the previewed note: the end or cancel of a gesture, the
    /// open clip or editor changing.
    pub(crate) fn audition_release(&mut self) {
        if let Some(keys) = self.live_keys.as_mut() {
            self.audition.release(keys);
        }
    }

    /// Every frame: the preview's own end, and the reasons to let go (the
    /// transport stopped, the window lost focus, the setting turned off).
    pub(crate) fn update_audition(&mut self, ctx: &egui::Context) {
        let Some(keys) = self.live_keys.as_mut() else {
            return;
        };
        self.audition.set_enabled(self.settings.midi.audition);
        let focused = ctx.input(|i| i.focused);
        let playing = self.transport.state().playing;
        if let Some(wait) = self
            .audition
            .update(keys, std::time::Instant::now(), playing, focused)
        {
            ctx.request_repaint_after(wait);
        }
    }

    /// What decides which tracks listen: the selection and the inputs
    /// switched on for tracks.
    pub(crate) fn listen_inputs(&self) -> crate::midi_play::Inputs {
        let mut selected = self.arrangement.selected.clone();
        selected.sort();
        let ports = self
            .midi
            .ports()
            .into_iter()
            .filter(|p| p.info.id.direction == Direction::Input && p.using.track)
            .filter_map(|p| Some((p.info.id.name.clone(), p.slot?)))
            .collect();
        crate::midi_play::Inputs {
            selected,
            ports,
            replacing: self.replacing(),
        }
    }

    /// Lends the headers what they need for their listening lights: which
    /// inputs are on, and how much has come from each.
    pub(crate) fn lend_live_input(&mut self) {
        let inputs = self.listen_inputs();
        let counts = self
            .midi
            .ports()
            .into_iter()
            .filter(|p| p.info.id.direction == Direction::Input)
            .filter_map(|p| Some((p.slot?, self.midi.activity(&p.info.id))))
            .collect();
        self.arrangement.live = crate::arrangement::Live {
            inputs,
            counts,
            keyboard: self
                .live_keys
                .as_ref()
                .map_or(0, crate::live::LiveKeys::activity),
        };
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
        // MIDI clock (M5 slice 7): outputs with Sync on get it from the
        // engine, each moved by its delay in the sender.
        let (sync, delays) = self.sync_ports();
        self.transport.audio.midi.set_sync(sync);
        if let Some(out) = &mut self.midi_out {
            out.set_ports(self.lanes.midi_ports.names());
            out.set_sync_delays(&delays);
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
        let inputs = self.menu_inputs();
        if MenuInputs::of(ctx) != inputs {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("menu-midi-inputs"), inputs));
        }
        let menu = self.menu_ports();
        if MenuPorts::of(ctx) != menu {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("menu-midi-outputs"), menu));
        }
    }

    /// The outputs with Sync on, as the engine's port bits (ports 0 to 63),
    /// and each port's clock delay in milliseconds, by port number.
    fn sync_ports(&mut self) -> (u64, Vec<i32>) {
        let mut bits = 0_u64;
        let mut delays = Vec::new();
        for port in self.midi.ports() {
            let id = &port.info.id;
            if id.direction != Direction::Output || !port.using.sync {
                continue;
            }
            let index = usize::from(self.lanes.midi_ports.index(&id.name));
            if index >= 64 {
                continue;
            }
            bits |= 1 << index;
            if delays.len() <= index {
                delays.resize(index + 1, 0);
            }
            delays[index] = Midi::sync_delay(&self.settings.midi, &id.name);
        }
        (bits, delays)
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

    fn menu_inputs(&self) -> MenuInputs {
        let mut ports: Vec<MenuPort> = self
            .midi
            .ports()
            .into_iter()
            .filter(|p| p.info.id.direction == Direction::Input)
            .map(|p| MenuPort {
                id: p.info.id.name.clone(),
                label: p.info.name.clone(),
                present: true,
            })
            .collect();
        let named: BTreeSet<String> = self
            .document
            .project()
            .tracks
            .iter()
            .filter_map(|t| match &t.midi_from.source {
                lumen_project::model::FromSource::Port(name) => Some(name.clone()),
                _ => None,
            })
            .collect();
        for name in named {
            if !ports.iter().any(|p| p.id == name) {
                ports.push(MenuPort {
                    label: name.clone(),
                    id: name,
                    present: false,
                });
            }
        }
        MenuInputs(ports)
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
