//! [`MidiSystem`]: the worker thread behind the MIDI settings.
//!
//! The UI never waits on a driver. It sends commands (switch a port on or
//! off, send a message) and reads a snapshot of the ports and a stream of
//! [`PortEvent`]s. The worker polls the driver for ports coming and going,
//! opens the ports the user switched on (again when a device is replugged),
//! and drops connections for ports that went away.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::backend::{Backend, Connection, Output};
use crate::types::{Direction, MidiError, PortId, PortInfo, Short, Timed, Use};

/// How often the worker looks for plugged and unplugged devices.
pub const POLL: Duration = Duration::from_millis(500);

/// Room for messages between the drivers and whoever consumes them.
const QUEUE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortEvent {
    Added(PortInfo),
    Removed(PortInfo),
    /// A port the user switched on would not open.
    Failed(PortId, String),
}

/// One present port, as the settings window shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortState {
    pub info: PortInfo,
    pub using: Use,
    pub open: bool,
    pub error: Option<String>,
    /// An input's slot in [`Timed`]; outputs have none.
    pub slot: Option<u16>,
}

#[derive(Default)]
struct Shared {
    ports: Vec<PortState>,
    unavailable: Option<String>,
}

enum Command {
    Use(PortId, Use),
    Send(PortId, Short),
    Rescan,
    Quit,
}

pub struct MidiSystem {
    commands: Sender<Command>,
    events: Receiver<PortEvent>,
    shared: Arc<Mutex<Shared>>,
    input: Option<rtrb::Consumer<Timed>>,
    worker: Option<JoinHandle<()>>,
}

impl MidiSystem {
    /// Starts the worker. `make` builds the backend on the worker thread
    /// (drivers may be slow or fail); if it fails the system stays empty and
    /// [`unavailable`](Self::unavailable) says why. `uses` is what the user
    /// switched on in an earlier run.
    pub fn start(
        make: impl FnOnce() -> Result<Box<dyn Backend>, MidiError> + Send + 'static,
        uses: HashMap<PortId, Use>,
        poll: Duration,
    ) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (producer, input) = rtrb::RingBuffer::new(QUEUE);
        let worker = {
            let shared = shared.clone();
            thread::Builder::new()
                .name("lumen-midi".into())
                .spawn(move || {
                    match make() {
                        Ok(backend) => {
                            Worker::new(backend, uses, shared, event_tx, producer)
                                .run(&command_rx, poll);
                        }
                        Err(e) => lock(&shared).unavailable = Some(e.to_string()),
                    };
                })
                .ok()
        };
        Self {
            commands,
            events,
            shared,
            input: Some(input),
            worker,
        }
    }

    /// Why MIDI could not start, if it could not.
    pub fn unavailable(&self) -> Option<String> {
        lock(&self.shared).unavailable.clone()
    }

    /// The ports present now.
    pub fn ports(&self) -> Vec<PortState> {
        lock(&self.shared).ports.clone()
    }

    /// The slot messages from `id` carry.
    pub fn slot_of(&self, id: &PortId) -> Option<u16> {
        lock(&self.shared)
            .ports
            .iter()
            .find(|p| p.info.id == *id)
            .and_then(|p| p.slot)
    }

    /// Plugging, unplugging and failures since the last call.
    pub fn events(&self) -> Vec<PortEvent> {
        self.events.try_iter().collect()
    }

    /// Switches a port's uses; an input with either on is opened.
    pub fn set_use(&self, id: PortId, using: Use) {
        let _ = self.commands.send(Command::Use(id, using));
    }

    /// Looks for devices now rather than at the next poll.
    pub fn rescan(&self) {
        let _ = self.commands.send(Command::Rescan);
    }

    /// Sends a message to an output that is switched on for tracks.
    pub fn send(&self, id: PortId, message: Short) {
        let _ = self.commands.send(Command::Send(id, message));
    }

    /// The stream of messages from every open input, once.
    pub fn take_input(&mut self) -> Option<rtrb::Consumer<Timed>> {
        self.input.take()
    }
}

impl Drop for MidiSystem {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

type Producer = Arc<Mutex<rtrb::Producer<Timed>>>;

struct Worker {
    backend: Box<dyn Backend>,
    /// What the user switched on, kept for ports that are not plugged in.
    uses: HashMap<PortId, Use>,
    present: Vec<PortInfo>,
    inputs: HashMap<PortId, Box<dyn Connection>>,
    outputs: HashMap<PortId, Box<dyn Output>>,
    /// Ports that would not open; tried again when switched or replugged.
    failed: HashMap<PortId, String>,
    slots: HashMap<PortId, u16>,
    shared: Arc<Mutex<Shared>>,
    events: Sender<PortEvent>,
    producer: Producer,
}

impl Worker {
    fn new(
        backend: Box<dyn Backend>,
        uses: HashMap<PortId, Use>,
        shared: Arc<Mutex<Shared>>,
        events: Sender<PortEvent>,
        producer: rtrb::Producer<Timed>,
    ) -> Self {
        Self {
            backend,
            uses,
            present: Vec::new(),
            inputs: HashMap::new(),
            outputs: HashMap::new(),
            failed: HashMap::new(),
            slots: HashMap::new(),
            shared,
            events,
            producer: Arc::new(Mutex::new(producer)),
        }
    }

    fn run(&mut self, commands: &Receiver<Command>, poll: Duration) {
        self.refresh();
        loop {
            match commands.recv_timeout(poll) {
                Ok(Command::Quit) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(command) => {
                    if !self.handle(command) {
                        return;
                    }
                    while let Ok(command) = commands.try_recv() {
                        if !self.handle(command) {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.refresh();
        }
    }

    /// Returns false to stop.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Quit => return false,
            Command::Use(id, using) => {
                self.uses.insert(id.clone(), using);
                self.failed.remove(&id);
            }
            Command::Send(id, message) => {
                if let Some(out) = self.outputs.get_mut(&id)
                    && let Err(e) = out.send(message.as_slice())
                {
                    let _ = self.events.send(PortEvent::Failed(id, e.to_string()));
                }
            }
            Command::Rescan => {}
        }
        true
    }

    fn slot(&mut self, id: &PortId) -> u16 {
        let next = self.slots.len() as u16;
        *self.slots.entry(id.clone()).or_insert(next)
    }

    /// Brings the open ports and the snapshot in line with the driver and
    /// the switches.
    fn refresh(&mut self) {
        let now = self.backend.ports();
        let before: HashSet<PortId> = self.present.iter().map(|p| p.id.clone()).collect();
        let after: HashSet<PortId> = now.iter().map(|p| p.id.clone()).collect();
        for info in self.present.clone() {
            if !after.contains(&info.id) {
                self.inputs.remove(&info.id);
                self.outputs.remove(&info.id);
                self.failed.remove(&info.id);
                let _ = self.events.send(PortEvent::Removed(info));
            }
        }
        for info in &now {
            if !before.contains(&info.id) {
                let _ = self.events.send(PortEvent::Added(info.clone()));
            }
        }
        self.present = now;

        for info in self.present.clone() {
            let id = info.id;
            let using = self.uses.get(&id).copied().unwrap_or_default();
            let wanted = match id.direction {
                Direction::Input => using.any(),
                Direction::Output => using.track,
            };
            let open = self.inputs.contains_key(&id) || self.outputs.contains_key(&id);
            if !wanted {
                self.inputs.remove(&id);
                self.outputs.remove(&id);
            } else if !open && !self.failed.contains_key(&id) {
                self.open(&id);
            }
        }
        self.publish();
    }

    fn open(&mut self, id: &PortId) {
        let result = match id.direction {
            Direction::Input => {
                let slot = self.slot(id);
                let producer = self.producer.clone();
                let sink: crate::backend::InputSink = Box::new(move |stamp_us, bytes| {
                    if let Some(message) = Short::from_bytes(bytes)
                        && let Ok(mut p) = producer.lock()
                    {
                        let _ = p.push(Timed {
                            slot,
                            stamp_us,
                            message,
                        });
                    }
                });
                self.backend.open_input(id, sink).map(|c| {
                    self.inputs.insert(id.clone(), c);
                })
            }
            Direction::Output => self.backend.open_output(id).map(|o| {
                self.outputs.insert(id.clone(), o);
            }),
        };
        if let Err(e) = result {
            let text = e.to_string();
            self.failed.insert(id.clone(), text.clone());
            let _ = self.events.send(PortEvent::Failed(id.clone(), text));
        }
    }

    fn publish(&mut self) {
        let ports: Vec<PortState> = self
            .present
            .clone()
            .into_iter()
            .map(|info| {
                let id = info.id.clone();
                PortState {
                    using: self.uses.get(&id).copied().unwrap_or_default(),
                    open: self.inputs.contains_key(&id) || self.outputs.contains_key(&id),
                    error: self.failed.get(&id).cloned(),
                    slot: (id.direction == Direction::Input).then(|| self.slot(&id)),
                    info,
                }
            })
            .collect();
        lock(&self.shared).ports = ports;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeControl;
    use std::time::Instant;

    const FAST: Duration = Duration::from_millis(5);

    fn system(fake: &FakeControl, uses: HashMap<PortId, Use>) -> MidiSystem {
        let backend = fake.backend();
        MidiSystem::start(move || Ok(Box::new(backend)), uses, FAST)
    }

    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out: {what}"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn input(name: &str) -> PortId {
        FakeControl::id(Direction::Input, name)
    }

    const ON: Use = Use {
        track: true,
        remote: false,
    };

    #[test]
    fn ports_appear_and_disappear_with_hot_plug() {
        let fake = FakeControl::new();
        let midi = system(&fake, HashMap::new());
        fake.plug(Direction::Input, "Keys");
        until("Keys listed", || midi.ports().len() == 1);
        assert!(matches!(&midi.events()[..], [PortEvent::Added(i)] if i.name == "Keys"));
        fake.unplug(Direction::Input, "Keys");
        until("Keys gone", || midi.ports().is_empty());
        assert!(matches!(&midi.events()[..], [PortEvent::Removed(i)] if i.name == "Keys"));
    }

    #[test]
    fn an_input_opens_only_when_switched_on_and_closes_when_switched_off() {
        let fake = FakeControl::new();
        fake.plug(Direction::Input, "Keys");
        let midi = system(&fake, HashMap::new());
        until("listed", || midi.ports().len() == 1);
        assert_eq!(fake.open_inputs(), 0, "off until the user says so");
        midi.set_use(input("Keys"), ON);
        until("open", || fake.open_inputs() == 1);
        midi.set_use(input("Keys"), Use::default());
        until("closed", || fake.open_inputs() == 0);
    }

    #[test]
    fn remote_alone_opens_an_input() {
        let fake = FakeControl::new();
        fake.plug(Direction::Input, "Pad");
        let midi = system(
            &fake,
            HashMap::from([(
                input("Pad"),
                Use {
                    track: false,
                    remote: true,
                },
            )]),
        );
        until("open", || fake.open_inputs() == 1);
        assert!(midi.ports()[0].using.remote);
    }

    #[test]
    fn messages_arrive_stamped_with_the_inputs_slot() {
        let fake = FakeControl::new();
        fake.plug(Direction::Input, "Keys");
        let mut midi = system(&fake, HashMap::from([(input("Keys"), ON)]));
        let mut queue = midi.take_input().unwrap();
        until("open", || fake.open_inputs() == 1);
        assert!(fake.send_to_input("Keys", 1234, &[0x90, 60, 100]));
        let got = queue.pop().unwrap();
        assert_eq!(got.stamp_us, 1234);
        assert_eq!(got.message.as_slice(), [0x90, 60, 100]);
        assert_eq!(Some(got.slot), midi.slot_of(&input("Keys")));
    }

    #[test]
    fn a_replugged_device_that_was_switched_on_opens_again() {
        let fake = FakeControl::new();
        fake.plug(Direction::Input, "Keys");
        let midi = system(&fake, HashMap::from([(input("Keys"), ON)]));
        until("open", || fake.open_inputs() == 1);
        fake.unplug(Direction::Input, "Keys");
        until("closed", || {
            fake.open_inputs() == 0 && midi.ports().is_empty()
        });
        fake.plug(Direction::Input, "Keys");
        until("reopened", || fake.open_inputs() == 1);
        assert_eq!(fake.opens(Direction::Input, "Keys"), 2);
    }

    #[test]
    fn a_port_that_will_not_open_reports_it_and_is_tried_when_switched_again() {
        let fake = FakeControl::new();
        fake.plug(Direction::Input, "Busy");
        fake.refuse(Direction::Input, "Busy", true);
        let midi = system(&fake, HashMap::from([(input("Busy"), ON)]));
        until("failed", || {
            midi.ports().first().is_some_and(|p| p.error.is_some())
        });
        assert!(
            midi.events()
                .iter()
                .any(|e| matches!(e, PortEvent::Failed(id, _) if id.name == "Busy"))
        );
        thread::sleep(Duration::from_millis(30));
        assert_eq!(fake.opens(Direction::Input, "Busy"), 0, "no retry loop");
        fake.refuse(Direction::Input, "Busy", false);
        midi.set_use(input("Busy"), ON);
        until("open", || fake.open_inputs() == 1);
        assert!(midi.ports()[0].error.is_none());
    }

    #[test]
    fn an_output_switched_on_for_tracks_takes_messages() {
        let fake = FakeControl::new();
        fake.plug(Direction::Output, "Synth");
        let out = FakeControl::id(Direction::Output, "Synth");
        let midi = system(&fake, HashMap::from([(out.clone(), ON)]));
        until("open", || fake.opens(Direction::Output, "Synth") == 1);
        midi.send(out.clone(), Short::from_bytes(&[0x90, 64, 90]).unwrap());
        until("sent", || fake.sent().len() == 1);
        assert_eq!(fake.sent()[0], (out, vec![0x90, 64, 90]));
    }

    #[test]
    fn a_driver_that_cannot_start_leaves_an_empty_system_with_the_reason() {
        let midi = MidiSystem::start(
            || Err(MidiError::Unavailable("no MIDI service".into())),
            HashMap::new(),
            FAST,
        );
        until("reason", || midi.unavailable().is_some());
        assert_eq!(midi.unavailable().as_deref(), Some("no MIDI service"));
        assert!(midi.ports().is_empty());
    }

    #[test]
    fn dropping_the_system_stops_the_worker() {
        let fake = FakeControl::new();
        fake.plug(Direction::Input, "Keys");
        let midi = system(&fake, HashMap::from([(input("Keys"), ON)]));
        until("open", || fake.open_inputs() == 1);
        drop(midi);
        assert_eq!(fake.open_inputs(), 0, "connections close with it");
    }
}
