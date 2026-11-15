use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui;
use lumen_devices::Preset;
use lumen_midi::{Direction, Use};

use crate::App;
use crate::audition::Tone;
use crate::midi::out::Output;
use crate::midi::play as midi_play;
use crate::midi::play::used_ports;
use crate::midi::{Action, Midi};
use lumen_project::model::Id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuPort {
    pub id: String,
    pub label: String,
    pub present: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MenuPorts(pub Vec<MenuPort>);

impl MenuPorts {
    pub(crate) fn of(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<MenuPorts>(egui::Id::new("menu-midi-outputs")))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MenuInputs(pub Vec<MenuPort>);

impl MenuInputs {
    pub(crate) fn of(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<MenuInputs>(egui::Id::new("menu-midi-inputs")))
            .unwrap_or_default()
    }
}

impl App {
    pub(crate) fn choose_instrument(&mut self, preset: Preset) {
        let ids = self.selected_midi_tracks();
        self.document.set_instrument(&ids, Some(preset));
    }

    pub(crate) fn panic(&mut self) {
        self.release_computer_keyboard();
        self.audition.forget();
        self.transport.panic();
    }

    pub(crate) fn play_key(&mut self, msg: lumen_core::midi::Msg) -> bool {
        self.live_keys.as_mut().is_some_and(|keys| keys.send(msg))
    }

    pub(crate) fn audition_play(&mut self, track: Id, pitch: u8, velocity: u8) -> bool {
        self.audition_tones(track, &[Tone { pitch, velocity }], true)
    }

    pub(crate) fn audition_tones(&mut self, track: Id, tones: &[Tone], fresh: bool) -> bool {
        self.audition.set_enabled(self.settings.midi.audition);
        let now = std::time::Instant::now();
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

    pub(crate) fn audition_release(&mut self) {
        if let Some(keys) = self.live_keys.as_mut() {
            self.audition.release(keys);
        }
    }

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

    pub(crate) fn listen_inputs(&self) -> crate::midi::play::Inputs {
        let mut selected = self.arrangement.selected.clone();
        selected.sort();
        let ports = self
            .midi
            .ports()
            .into_iter()
            .filter(|p| p.info.id.direction == Direction::Input && p.using.track)
            .filter_map(|p| Some((p.info.id.name.clone(), p.slot?)))
            .collect();
        crate::midi::play::Inputs {
            selected,
            ports,
            replacing: self.replacing(),
            record_window: self.record_window(),
            record_timing: self.record_timing(),
        }
    }

    pub(crate) fn lend_live_input(&mut self) {
        let inputs = self.listen_inputs();
        let counts = self
            .midi
            .ports()
            .into_iter()
            .filter(|p| p.info.id.direction == Direction::Input)
            .filter_map(|p| {
                Some((
                    p.slot?,
                    self.midi.activity(&p.info.id),
                    self.midi.held(&p.info.id),
                ))
            })
            .collect();
        self.arrangement.live = crate::arrangement::Live {
            inputs,
            counts,
            keyboard: self
                .live_keys
                .as_ref()
                .map_or(0, crate::live::LiveKeys::activity),
            keyboard_held: self
                .live_keys
                .as_ref()
                .is_some_and(crate::live::LiveKeys::held),
        };
    }

    pub(crate) fn update_midi_hardware(&mut self, ctx: &egui::Context) {
        if self.live_keys.is_none() {
            let (feed, keys, tap) = crate::live::live(
                self.midi.take_input(),
                Arc::clone(&self.transport.audio.clock),
            );
            if self.transport.audio.midi.set_live(Box::new(feed)).is_ok() {
                self.live_keys = Some(keys);
                self.controllers.tap = Some(tap);
            }
        }
        if std::mem::take(&mut self.transport.panic_midi) {
            self.midi.panic(self.lanes.midi_ports.names());
        }
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
        let (sync, delays) = self.sync_ports();
        self.transport.audio.midi.set_sync(sync);
        if let Some(out) = &mut self.midi_out {
            out.set_ports(self.lanes.midi_ports.names());
            out.set_sync_delays(&delays);
            let latency = self.transport.audio.info.as_ref();
            out.set_latency(latency.map_or(0, |i| i.output_latency));
        }
        let state = (
            self.document.session(),
            self.document.state(),
            self.midi_ports_seen,
        );
        if self.midi_checked != Some(state) && self.open_used_outputs() {
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
