use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use lumen_plugin_host::protocol::{Aux, EditorBounds};

use crate::process::{Captured, Gate, Happened, Process, Start, Tag};

const LOADS_AT_ONCE: usize = 4;

#[derive(Debug)]
pub struct Event {
    pub device: u64,
    pub happened: Happened,
}

struct Entry {
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
    keys: Vec<u8>,
}

impl Manager {
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

    pub fn capture(&self, device: u64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.capture();
        }
    }

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

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn refresh(&self, device: u64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.refresh();
        }
    }

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

    pub fn keep_state(&mut self, device: u64, state: Captured) {
        if let Some(entry) = self.entries.get_mut(&device) {
            entry.start.state = state.component;
            if let Some(controller) = state.controller {
                entry.start.controller_state = controller;
            }
            entry.start.values.clear();
        }
    }

    pub fn reload(&mut self, device: u64) -> std::io::Result<bool> {
        match self.entries.get(&device).map(|e| e.start.clone()) {
            Some(start) => self.want(device, start).map(|()| true),
            None => Ok(false),
        }
    }

    pub fn forget(&mut self, device: u64) {
        self.entries.remove(&device);
    }

    pub fn relink_all(&mut self, rate: f64, frames: u32) {
        for entry in self.entries.values_mut() {
            entry.start.rate = rate;
            entry.start.frames = frames;
            entry.process.relink(rate, frames);
        }
    }

    pub fn relink(&mut self, device: u64, rate: f64, frames: u32) {
        if let Some(entry) = self.entries.get_mut(&device) {
            entry.start.rate = rate;
            entry.start.frames = frames;
            entry.process.relink(rate, frames);
        }
    }

    pub fn relink_with(&mut self, device: u64, rate: f64, frames: u32, aux: Aux) {
        if let Some(entry) = self.entries.get_mut(&device) {
            entry.start.rate = rate;
            entry.start.frames = frames;
            entry.start.aux = aux;
            entry.process.relink_with(rate, frames, aux);
        }
    }

    pub fn set_param(&self, device: u64, id: u32, value: f64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.set_param(id, value);
        }
    }

    pub fn param_text(&self, device: u64, id: u32, value: f64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.param_text(id, value);
        }
    }

    pub fn param_texts(&self, device: u64, id: u32, values: Vec<f64>) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.param_texts(id, values);
        }
    }

    pub fn param_value(&self, device: u64, id: u32, text: String) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.param_value(id, text);
        }
    }

    pub fn open_editor(&self, device: u64, bounds: Option<EditorBounds>) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.open_editor(bounds);
        }
    }

    pub fn close_editor(&self, device: u64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.close_editor();
        }
    }

    pub fn show_editor(&self, device: u64, visible: bool) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.show_editor(visible);
        }
    }

    pub fn end(&self, device: u64) {
        if let Some(entry) = self.entries.get(&device) {
            entry.process.end();
        }
    }

    pub fn capture_keys(&mut self, keys: Vec<u8>) {
        if keys == self.keys {
            return;
        }
        for entry in self.entries.values() {
            entry.process.capture_keys(keys.clone());
        }
        self.keys = keys;
    }

    pub fn has(&self, device: u64) -> bool {
        self.entries.contains_key(&device)
    }

    pub fn devices(&self) -> impl Iterator<Item = u64> + '_ {
        self.entries.keys().copied()
    }

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

    pub fn shutdown(&mut self, grace: Duration) {
        let entries: Vec<Entry> = self.entries.drain().map(|(_, e)| e).collect();
        for entry in &entries {
            entry.process.ask_to_quit();
        }
        let end = Instant::now() + grace;
        for entry in entries {
            while !entry.process.ended() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::*;

    fn missing() -> Start {
        Start::new(
            PathBuf::from("/nonexistent/lumen-plugin-host"),
            PathBuf::from("/nonexistent/plugin.vst3"),
            [0; 16],
            false,
            48_000.0,
        )
    }

    fn drain_for(manager: &mut Manager, wait: Duration) -> Vec<Event> {
        let end = Instant::now() + wait;
        let mut all = Vec::new();
        while Instant::now() < end {
            all.extend(manager.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        all
    }

    #[test]
    fn what_a_replaced_start_says_is_not_heard() {
        let mut manager = Manager::new(|| {});
        manager.want(7, missing()).unwrap();
        manager.want(7, missing()).unwrap();
        let heard = drain_for(&mut manager, Duration::from_millis(600));
        assert_eq!(
            heard.len(),
            1,
            "one failure, from the start that is current: {heard:?}"
        );
    }

    #[test]
    fn what_a_forgotten_device_says_is_not_heard() {
        let mut manager = Manager::new(|| {});
        manager.want(7, missing()).unwrap();
        manager.forget(7);
        let heard = drain_for(&mut manager, Duration::from_millis(600));
        assert!(heard.is_empty(), "{heard:?}");
    }
}
