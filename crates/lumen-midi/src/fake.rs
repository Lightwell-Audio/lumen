//! A scriptable backend for tests: plug and unplug ports, inject messages
//! into inputs, read what outputs were sent, and make a port refuse to open.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::backend::{Backend, Connection, InputSink, Output};
use crate::types::{Direction, MidiError, PortId, PortInfo};

#[derive(Default)]
struct State {
    ports: Vec<PortInfo>,
    /// Open inputs: (port, connection number, sink).
    sinks: Vec<(PortId, u64, InputSink)>,
    next: u64,
    sent: Vec<(PortId, Vec<u8>)>,
    /// When each of `sent` was sent.
    sent_at: Vec<std::time::Instant>,
    refuse: HashSet<PortId>,
    opens: HashMap<PortId, u32>,
}

/// The test's handle on the fake driver.
#[derive(Clone, Default)]
pub struct FakeControl(Arc<Mutex<State>>);

impl FakeControl {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn id(direction: Direction, name: &str) -> PortId {
        PortId {
            direction,
            name: name.into(),
        }
    }

    /// Plugs a device in.
    pub fn plug(&self, direction: Direction, name: &str) {
        let id = Self::id(direction, name);
        let mut s = self.state();
        if !s.ports.iter().any(|p| p.id == id) {
            s.ports.push(PortInfo {
                id,
                name: name.into(),
            });
        }
    }

    /// Unplugs it: the port disappears and its connections die.
    pub fn unplug(&self, direction: Direction, name: &str) {
        let id = Self::id(direction, name);
        let mut s = self.state();
        s.ports.retain(|p| p.id != id);
    }

    /// Makes opening the port fail (busy in another program).
    pub fn refuse(&self, direction: Direction, name: &str, refuse: bool) {
        let id = Self::id(direction, name);
        let mut s = self.state();
        if refuse {
            s.refuse.insert(id);
        } else {
            s.refuse.remove(&id);
        }
    }

    /// A message arriving on an input at `stamp_us`. Returns whether the
    /// input was open.
    pub fn send_to_input(&self, name: &str, stamp_us: u64, bytes: &[u8]) -> bool {
        let id = Self::id(Direction::Input, name);
        let mut s = self.state();
        let mut heard = false;
        for (port, _, sink) in &mut s.sinks {
            if *port == id {
                sink(stamp_us, bytes);
                heard = true;
            }
        }
        heard
    }

    /// Everything sent to outputs so far.
    pub fn sent(&self) -> Vec<(PortId, Vec<u8>)> {
        self.state().sent.clone()
    }

    /// Everything sent to outputs so far, with the time it was sent.
    pub fn sent_timed(&self) -> Vec<(PortId, Vec<u8>, std::time::Instant)> {
        let s = self.state();
        s.sent
            .iter()
            .zip(&s.sent_at)
            .map(|((id, bytes), at)| (id.clone(), bytes.clone(), *at))
            .collect()
    }

    /// How many times the port has been opened.
    pub fn opens(&self, direction: Direction, name: &str) -> u32 {
        let id = Self::id(direction, name);
        self.state().opens.get(&id).copied().unwrap_or(0)
    }

    /// Open inputs now.
    pub fn open_inputs(&self) -> usize {
        self.state().sinks.len()
    }

    pub fn backend(&self) -> FakeBackend {
        FakeBackend(self.clone())
    }
}

pub struct FakeBackend(FakeControl);

struct Open {
    control: FakeControl,
    number: u64,
}

impl Connection for Open {}

impl Drop for Open {
    fn drop(&mut self) {
        self.control
            .state()
            .sinks
            .retain(|(_, n, _)| *n != self.number);
    }
}

struct Sender {
    control: FakeControl,
    id: PortId,
}

impl Output for Sender {
    fn send(&mut self, bytes: &[u8]) -> Result<(), MidiError> {
        let mut s = self.control.state();
        if !s.ports.iter().any(|p| p.id == self.id) {
            return Err(MidiError::Send(format!("{} is gone", self.id.name)));
        }
        s.sent.push((self.id.clone(), bytes.to_vec()));
        s.sent_at.push(std::time::Instant::now());
        Ok(())
    }
}

impl Backend for FakeBackend {
    fn ports(&mut self) -> Vec<PortInfo> {
        self.0.state().ports.clone()
    }

    fn open_input(
        &mut self,
        id: &PortId,
        sink: InputSink,
    ) -> Result<Box<dyn Connection>, MidiError> {
        let mut s = self.0.state();
        if !s.ports.iter().any(|p| p.id == *id) {
            return Err(MidiError::Open(format!("{} is not connected", id.name)));
        }
        if s.refuse.contains(id) {
            return Err(MidiError::Open(format!("{} is in use", id.name)));
        }
        *s.opens.entry(id.clone()).or_default() += 1;
        s.next += 1;
        let number = s.next;
        s.sinks.push((id.clone(), number, sink));
        Ok(Box::new(Open {
            control: self.0.clone(),
            number,
        }))
    }

    fn open_output(&mut self, id: &PortId) -> Result<Box<dyn Output>, MidiError> {
        let mut s = self.0.state();
        if !s.ports.iter().any(|p| p.id == *id) {
            return Err(MidiError::Open(format!("{} is not connected", id.name)));
        }
        if s.refuse.contains(id) {
            return Err(MidiError::Open(format!("{} is in use", id.name)));
        }
        *s.opens.entry(id.clone()).or_default() += 1;
        Ok(Box::new(Sender {
            control: self.0.clone(),
            id: id.clone(),
        }))
    }
}
