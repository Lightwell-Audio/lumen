//! The set of plugin devices that have a host (M6 slice 2c): who is loading,
//! who is running, who failed. The interface thread calls [`Manager::want`],
//! [`Manager::forget`] and the rest, which never wait for a plugin, and
//! [`Manager::poll`] each frame for what the host threads have to say.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use lumen_plugin_host::protocol::EditorBounds;

use crate::process::{Captured, Gate, Happened, Process, Start, Tag};

/// Plugins loading at once.
const LOADS_AT_ONCE: usize = 4;

/// What happened to a device's host.
#[derive(Debug)]
pub struct Event {
    /// The device's id in the project.
    pub device: u64,
    pub happened: Happened,
}

struct Entry {
    /// Which start of this device messages are wanted from.
    serial: u64,
    start: Start,
    process: Process,
}

pub struct Manager {
    entries: HashMap<u64, Entry>,
    next_serial: u64,
    gate: Arc<Gate>,
    out: Sender<(Tag, Happened)>,
    incoming: Receiver<(Tag, Happened)>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The keys every host takes from its plugin's window.
    keys: Vec<u8>,
}

impl Manager {
    /// `wake` is called from a host thread whenever it has something to say
    /// (the app asks for a repaint).
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (out, incoming) = mpsc::channel();
        Self {
            entries: HashMap::new(),
            next_serial: 1,
            gate: Gate::new(LOADS_AT_ONCE),
            out,
            incoming,
            wake: Arc::new(wake),
            keys: Vec::new(),
        }
    }

    /// Starts a host for `device` in the background, ending the one it had.
    /// The answer comes through [`Manager::poll`].
    pub fn want(&mut self, device: u64, start: Start) -> std::io::Result<()> {
        let serial = self.next_serial;
        self.next_serial += 1;
        let process = Process::start(
            start.clone(),
            self.gate.clone(),
            (device, serial),
            self.out.clone(),
            self.wake.clone(),
        )?;
        if !self.keys.is_empty() {
            process.capture_keys(self.keys.clone());
        }
        // Replacing the entry drops the old process, which quits.
        self.entries.insert(
            device,
            Entry {
                serial,
                start,
                process,
            },
        );
        Ok(())
    }

    /// Asks the device's plugin for its state; the answer comes from
    /// [`Manager::poll`] as [`crate::Happened::State`].
    pub fn capture(&self, device: u64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.capture();
        }
    }

    /// Starts the device's host again from `state` (the saved state, or a
    /// fresher one) with `values` set over it (the card's Reload after a
    /// crash, which should come back with the work since the project loaded).
    pub fn reload_from(
        &mut self,
        device: u64,
        state: Option<Captured>,
        values: Vec<(u32, f64)>,
    ) -> std::io::Result<bool> {
        match self.entries.get(&device).map(|e| e.start.clone()) {
            Some(mut start) => {
                if let Some(state) = state {
                    start.state = state.component;
                    start.controller_state = state.controller.unwrap_or_default();
                }
                start.values = values;
                self.want(device, start).map(|()| true)
            }
            None => Ok(false),
        }
    }

    /// Asks the device's plugin for every parameter's value; the answer comes
    /// from [`Manager::poll`] as [`crate::Happened::Values`].
    pub fn refresh(&self, device: u64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.refresh();
        }
    }

    /// Loads a state into the device's running plugin (a preset, or the
    /// project's state after an undo), then `values` over it; the answer
    /// comes from [`Manager::poll`] as [`crate::Happened::Loaded`].
    pub fn load_state(
        &self,
        device: u64,
        state: Vec<u8>,
        controller: Option<Vec<u8>>,
        values: Vec<(u32, f64)>,
    ) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.load_state(state, controller, values);
        }
    }

    /// Makes `state` what a host started again for the device starts from: a
    /// state the plugin took ([`crate::Happened::Loaded`] said so), which a
    /// card's Reload after a crash should come back to.
    pub fn keep_state(&mut self, device: u64, state: Captured) {
        if let Some(entry) = self.entries.get_mut(&device) {
            entry.start.state = state.component;
            if let Some(controller) = state.controller {
                entry.start.controller_state = controller;
            }
            entry.start.values.clear();
        }
    }

    /// Starts the device's host again with the same settings (the card's
    /// Reload).
    pub fn reload(&mut self, device: u64) -> std::io::Result<bool> {
        match self.entries.get(&device).map(|e| e.start.clone()) {
            Some(start) => self.want(device, start).map(|()| true),
            None => Ok(false),
        }
    }

    /// Ends the device's host now (owner's C3: a removed device keeps
    /// nothing running). What it said before is dropped.
    pub fn forget(&mut self, device: u64) {
        self.entries.remove(&device);
    }

    /// Asks every host for a fresh link at `rate` with blocks of at most
    /// `frames`: a new engine, or the device's block size changed.
    pub fn relink_all(&mut self, rate: f64, frames: u32) {
        for entry in self.entries.values_mut() {
            entry.start.rate = rate;
            entry.start.frames = frames;
            entry.process.relink(rate, frames);
        }
    }

    /// Asks one host for a fresh link at `rate` with blocks of at most
    /// `frames`: the plugin asked to be restarted.
    pub fn relink(&mut self, device: u64, rate: f64, frames: u32) {
        if let Some(entry) = self.entries.get_mut(&device) {
            entry.start.rate = rate;
            entry.start.frames = frames;
            entry.process.relink(rate, frames);
        }
    }

    /// Sets a parameter of the device's plugin (normalized, 0 to 1). The
    /// plugin's controller is told and the processor hears it at the start of
    /// its next block. Does nothing for a device with no host.
    pub fn set_param(&self, device: u64, id: u32, value: f64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.set_param(id, value);
        }
    }

    /// Asks the device's plugin for its text for a value; the answer comes
    /// from [`Manager::poll`] as [`crate::Happened::Text`].
    pub fn param_text(&self, device: u64, id: u32, value: f64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.param_text(id, value);
        }
    }

    /// Asks the device's plugin to read `text` the user typed for a
    /// parameter; the answer comes from [`Manager::poll`] as
    /// [`crate::Happened::Typed`].
    pub fn param_value(&self, device: u64, id: u32, text: String) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.param_value(id, text);
        }
    }

    /// Opens the device's plugin editor window, at `bounds` if given; the news
    /// comes from [`Manager::poll`] as [`crate::Happened::Editor`].
    pub fn open_editor(&self, device: u64, bounds: Option<EditorBounds>) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.open_editor(bounds);
        }
    }

    /// Closes the device's plugin editor window.
    pub fn close_editor(&self, device: u64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.close_editor();
        }
    }

    /// Hides the device's plugin editor window without closing it, or shows
    /// it again.
    pub fn show_editor(&self, device: u64, visible: bool) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.show_editor(visible);
        }
    }

    /// Ends the device's host, for a plugin whose window stopped answering.
    pub fn end(&self, device: u64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.end();
        }
    }

    /// Has every host take `keys` (virtual-key codes) from its plugin's
    /// window when pressed alone, now and when started; empty gives them back.
    /// The presses come from [`Manager::poll`] as [`crate::Happened::Key`].
    pub fn capture_keys(&mut self, keys: Vec<u8>) {
        if keys == self.keys {
            return;
        }
        for entry in self.entries.values() {
            entry.process.capture_keys(keys.clone());
        }
        self.keys = keys;
    }

    /// Whether `device` has a host started for it (loading, running or failed).
    pub fn has(&self, device: u64) -> bool {
        self.entries.contains_key(&device)
    }

    /// The devices that have a host started.
    pub fn devices(&self) -> impl Iterator<Item = u64> + '_ {
        self.entries.keys().copied()
    }

    /// What the host threads said since the last call. Messages from a
    /// start that was replaced or forgotten are left out.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        while let Ok(((device, serial), happened)) = self.incoming.try_recv() {
            if self
                .entries
                .get(&device)
                .is_some_and(|e| e.serial == serial)
            {
                events.push(Event { device, happened });
            }
        }
        events
    }

    /// Asks every host to quit and waits for them: when Lumen closes.
    pub fn shutdown(&mut self) {
        for (_, entry) in self.entries.drain() {
            entry.process.join();
        }
    }
}
