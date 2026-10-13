//! MIDI for the app: the device layer (`lumen-midi`) with the user's
//! switches, saved in the settings. Listing and opening ports happens on
//! the layer's own thread; the UI reads a snapshot (M5 slice 0, ADR-0044).

use std::collections::HashMap;
use std::time::Duration;

use lumen_core::product::PRODUCT_NAME;
use lumen_midi::fake::FakeControl;
use lumen_midi::{Direction, MidiSystem, PortEvent, PortId, PortState, Use};

use crate::settings;

/// The clock delay's range in milliseconds (the owner's F4).
pub const SYNC_DELAY_MS: (i32, i32) = (-20, 50);

/// Where MIDI ports come from.
#[derive(Clone)]
pub enum Source {
    /// The operating system's MIDI.
    System,
    /// No MIDI at all (`--midi off`).
    Off,
    /// A scripted driver, for tests (`--midi test:fake` starts an empty one).
    Test(FakeControl),
}

impl Source {
    /// Parses `--midi`.
    pub fn parse(arg: &str) -> Result<Self, String> {
        match arg {
            "system" => Ok(Source::System),
            "off" => Ok(Source::Off),
            "test:fake" => Ok(Source::Test(FakeControl::new())),
            _ => Err(format!(
                "--midi takes system, off or test:fake, not {arg:?}"
            )),
        }
    }
}

/// What the settings window asks for.
pub enum Action {
    Use(PortId, Use),
    Rescan,
    /// An output's clock delay in milliseconds (M5 slice 7): saved only.
    SyncDelay(String, i32),
}

pub struct Midi {
    system: Option<MidiSystem>,
    /// Plugging and failures worth telling the user, newest last.
    pub notices: Vec<String>,
}

fn direction_key(direction: Direction) -> &'static str {
    match direction {
        Direction::Input => "input",
        Direction::Output => "output",
    }
}

impl Midi {
    /// `wake` is called from a driver's thread when a playing message
    /// arrives, so the interface can sleep between them.
    pub fn start(source: Source, saved: &settings::Midi, wake: lumen_midi::Waker) -> Self {
        let uses: HashMap<PortId, Use> = saved
            .ports
            .iter()
            .filter_map(|p| {
                let direction = match p.direction.as_str() {
                    "input" => Direction::Input,
                    "output" => Direction::Output,
                    _ => return None,
                };
                let id = PortId {
                    direction,
                    name: p.name.clone(),
                };
                Some((
                    id,
                    Use {
                        track: p.track,
                        remote: p.remote,
                        sync: p.sync,
                    },
                ))
            })
            .collect();
        let system = match source {
            Source::Off => None,
            Source::System => Some(MidiSystem::start_with_waker(
                || {
                    lumen_midi::midir_host::MidirHost::new(PRODUCT_NAME)
                        .map(|h| Box::new(h) as Box<dyn lumen_midi::Backend>)
                },
                uses,
                lumen_midi::system::POLL,
                Some(wake),
            )),
            Source::Test(fake) => {
                let backend = fake.backend();
                Some(MidiSystem::start_with_waker(
                    move || Ok(Box::new(backend)),
                    uses,
                    Duration::from_millis(20),
                    Some(wake),
                ))
            }
        };
        Self {
            system,
            notices: Vec::new(),
        }
    }

    /// The ports present now, inputs first.
    pub fn ports(&self) -> Vec<PortState> {
        let mut ports = self
            .system
            .as_ref()
            .map(MidiSystem::ports)
            .unwrap_or_default();
        ports.sort_by(|a, b| a.info.id.cmp(&b.info.id));
        ports
    }

    /// Messages from every open input, once: the live feed takes them.
    pub fn take_input(&mut self) -> Option<rtrb::Consumer<lumen_midi::Timed>> {
        self.system.as_mut().and_then(MidiSystem::take_input)
    }

    /// The traffic count of a port (see `MidiSystem::activity`); 0 when
    /// MIDI is off or the port is not there.
    pub fn activity(&self, id: &PortId) -> u32 {
        self.system.as_ref().map_or(0, |s| s.activity(id))
    }

    /// A handle that sends from other threads, if MIDI is on.
    pub fn sender(&self) -> Option<lumen_midi::MidiSender> {
        self.system.as_ref().map(MidiSystem::sender)
    }

    /// Sends All Notes Off and All Sound Off on every channel of the named
    /// outputs (those that are open). The engine does the same when it
    /// runs; this covers it not running.
    pub fn panic(&self, names: &[String]) {
        let Some(system) = &self.system else { return };
        for name in names {
            let id = PortId {
                direction: Direction::Output,
                name: name.clone(),
            };
            for channel in 0..16 {
                for number in [
                    lumen_core::midi::cc::ALL_NOTES_OFF,
                    lumen_core::midi::cc::ALL_SOUND_OFF,
                ] {
                    let msg = lumen_core::midi::Msg::controller(channel, number, 0);
                    if let Some(short) = lumen_midi::Short::from_bytes(msg.bytes()) {
                        system.send(id.clone(), short);
                    }
                }
            }
        }
    }

    /// Why MIDI is not available, if it is not.
    pub fn unavailable(&self) -> Option<String> {
        match &self.system {
            None => Some("MIDI is switched off for this run".into()),
            Some(s) => s.unavailable(),
        }
    }

    /// Plugging and unplugging since the last call, as notices. True when
    /// the ports changed (the window should redraw).
    pub fn poll(&mut self) -> bool {
        let Some(system) = &self.system else {
            return false;
        };
        let events = system.events();
        let changed = !events.is_empty();
        for event in events {
            if let PortEvent::Failed(id, why) = event {
                self.notices
                    .push(format!("MIDI {} could not open: {why}", id.name));
            }
        }
        changed
    }

    pub fn apply(&self, action: &Action) {
        let Some(system) = &self.system else { return };
        match action {
            Action::Use(id, using) => system.set_use(id.clone(), *using),
            Action::Rescan => system.rescan(),
            Action::SyncDelay(..) => {}
        }
    }

    /// Folds a port's new switches into what is saved. Ports that are
    /// switched off everywhere are forgotten.
    pub fn remember(saved: &mut settings::Midi, id: &PortId, using: Use) {
        let key = direction_key(id.direction);
        let delay = saved
            .ports
            .iter()
            .find(|p| p.direction == key && p.name == id.name)
            .map_or(0, |p| p.sync_delay_ms);
        saved
            .ports
            .retain(|p| !(p.direction == key && p.name == id.name));
        if using.any() {
            saved.ports.push(settings::MidiPort {
                direction: key.into(),
                name: id.name.clone(),
                track: using.track,
                remote: using.remote,
                sync: using.sync,
                sync_delay_ms: delay,
            });
        }
    }

    /// The clock delay saved for the output `name`, in milliseconds.
    pub fn sync_delay(saved: &settings::Midi, name: &str) -> i32 {
        saved
            .ports
            .iter()
            .find(|p| p.direction == "output" && p.name == name)
            .map_or(0, |p| p.sync_delay_ms)
    }

    /// Saves the clock delay of the output `name` (a port with Sync on is
    /// always remembered).
    pub fn set_sync_delay(saved: &mut settings::Midi, name: &str, ms: i32) {
        let ms = ms.clamp(SYNC_DELAY_MS.0, SYNC_DELAY_MS.1);
        if let Some(port) = saved
            .ports
            .iter_mut()
            .find(|p| p.direction == "output" && p.name == name)
        {
            port.sync_delay_ms = ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_are_remembered_by_port_and_forgotten_when_off() {
        let id = PortId {
            direction: Direction::Input,
            name: "Keys".into(),
        };
        let mut saved = settings::Midi::default();
        Midi::remember(
            &mut saved,
            &id,
            Use {
                track: true,
                remote: false,
                sync: false,
            },
        );
        Midi::remember(
            &mut saved,
            &id,
            Use {
                track: true,
                remote: true,
                sync: false,
            },
        );
        assert_eq!(saved.ports.len(), 1);
        assert!(saved.ports[0].remote);
        Midi::remember(&mut saved, &id, Use::default());
        assert!(saved.ports.is_empty());
    }

    #[test]
    fn the_flag_names_its_sources() {
        assert!(matches!(Source::parse("off"), Ok(Source::Off)));
        assert!(matches!(Source::parse("test:fake"), Ok(Source::Test(_))));
        assert!(Source::parse("jack").is_err());
    }
}
