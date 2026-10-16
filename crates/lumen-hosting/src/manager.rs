//! The set of plugin devices that have a host (M6 slice 2c): who is loading,
//! who is running, who failed. The interface thread calls [`Manager::want`],
//! [`Manager::forget`] and the rest, which never wait for a plugin, and
//! [`Manager::poll`] each frame for what the host threads have to say.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::process::{Gate, Happened, Process, Start, Tag};

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

    /// Asks every host for a fresh link at `rate`, for a new engine.
    pub fn relink_all(&mut self, rate: f64) {
        for entry in self.entries.values_mut() {
            entry.start.rate = rate;
            entry.process.relink(rate);
        }
    }

    /// Asks one host for a fresh link at `rate`: the plugin asked to be
    /// restarted, or the device's rate changed.
    pub fn relink(&mut self, device: u64, rate: f64) {
        if let Some(entry) = self.entries.get_mut(&device) {
            entry.start.rate = rate;
            entry.process.relink(rate);
        }
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
