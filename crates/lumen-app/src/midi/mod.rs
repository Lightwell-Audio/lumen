pub(crate) mod export;
pub(crate) mod files;
pub(crate) mod out;
pub(crate) mod play;
pub(crate) mod runs;
pub(crate) mod sends;

use std::collections::HashMap;
use std::time::Duration;

use lumen_core::product::PRODUCT_NAME;
use lumen_midi::fake::FakeControl;
use lumen_midi::{Direction, MidiSystem, PortEvent, PortId, PortState, Use};

use crate::settings;

pub const SYNC_DELAY_MS: (i32, i32) = (-20, 50);

#[derive(Clone)]
pub enum Source {
    System,
    Off,
    Test(FakeControl),
}

impl Source {
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

pub enum Action {
    Use(PortId, Use),
    Rescan,
    SyncDelay(String, i32),
}

pub struct Midi {
    system: Option<MidiSystem>,
    pub notices: Vec<String>,
}

fn direction_key(direction: Direction) -> &'static str {
    match direction {
        Direction::Input => "input",
        Direction::Output => "output",
    }
}

impl Midi {
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

    pub fn ports(&self) -> Vec<PortState> {
        let mut ports = self
            .system
            .as_ref()
            .map(MidiSystem::ports)
            .unwrap_or_default();
        ports.sort_by(|a, b| a.info.id.cmp(&b.info.id));
        ports
    }

    pub fn take_input(&mut self) -> Option<rtrb::Consumer<lumen_midi::Timed>> {
        self.system.as_mut().and_then(MidiSystem::take_input)
    }

    pub fn activity(&self, id: &PortId) -> u32 {
        self.system.as_ref().map_or(0, |s| s.activity(id))
    }

    pub fn held(&self, id: &PortId) -> bool {
        self.system.as_ref().is_some_and(|s| s.held(id))
    }

    pub fn sender(&self) -> Option<lumen_midi::MidiSender> {
        self.system.as_ref().map(MidiSystem::sender)
    }

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

    pub fn unavailable(&self) -> Option<String> {
        match &self.system {
            None => Some("MIDI is switched off for this run".into()),
            Some(s) => s.unavailable(),
        }
    }

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

    pub fn remember(saved: &mut settings::Midi, id: &PortId, using: Use) {
        let key = direction_key(id.direction);
        let delay = saved
            .ports
            .iter()
            .find(|p| p.direction == key && p.name == id.name)
            .map_or(0, |p| {
                p.sync_delay_ms.clamp(SYNC_DELAY_MS.0, SYNC_DELAY_MS.1)
            });
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

    pub fn sync_delay(saved: &settings::Midi, name: &str) -> i32 {
        saved
            .ports
            .iter()
            .find(|p| p.direction == "output" && p.name == name)
            .map_or(0, |p| {
                p.sync_delay_ms.clamp(SYNC_DELAY_MS.0, SYNC_DELAY_MS.1)
            })
    }

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
    fn a_saved_clock_delay_is_read_within_its_limits() {
        let port = |ms| settings::MidiPort {
            direction: "output".into(),
            name: "Drum machine".into(),
            track: false,
            remote: false,
            sync: true,
            sync_delay_ms: ms,
        };
        let delay = |ms| {
            Midi::sync_delay(
                &settings::Midi {
                    ports: vec![port(ms)],
                    ..Default::default()
                },
                "Drum machine",
            )
        };
        assert_eq!(delay(-7), -7);
        assert_eq!(delay(i32::MAX), SYNC_DELAY_MS.1);
        assert_eq!(delay(i32::MIN), SYNC_DELAY_MS.0);
        assert_eq!(Midi::sync_delay(&settings::Midi::default(), "Absent"), 0);
    }

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
