//! The real backend: WinMM on Windows, CoreMIDI on macOS, ALSA on Linux.

use std::collections::HashMap;

use midir::{
    MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection, MidiOutputPort,
};

use crate::backend::{Backend, Connection, InputSink, Output};
use crate::types::{Direction, MidiError, PortId, PortInfo};

/// The client name drivers show for the app. The app sets the product name
/// through `MidirHost::new`.
pub struct MidirHost {
    client: String,
}

impl MidirHost {
    pub fn new(client: &str) -> Result<Self, MidiError> {
        // Fail early, with the driver's reason, if there is no MIDI system.
        MidiInput::new(client).map_err(|e| MidiError::Unavailable(e.to_string()))?;
        Ok(Self {
            client: client.into(),
        })
    }
}

/// One direction's port names, with " 2", " 3" after repeated names.
fn numbered(raw: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen: HashMap<String, u32> = HashMap::new();
    raw.map(|name| {
        let n = seen.entry(name.clone()).or_insert(0);
        *n += 1;
        if *n == 1 { name } else { format!("{name} {n}") }
    })
    .collect()
}

/// Every input port with its numbered name, from one listing: a device
/// plugged or unplugged between two listings would otherwise shift the
/// index found in the first onto another port in the second.
fn input_ports(input: &MidiInput) -> Vec<(String, MidiInputPort)> {
    let ports = input.ports();
    let names = numbered(
        ports
            .iter()
            .map(|p| input.port_name(p).unwrap_or_else(|_| "MIDI input".into())),
    );
    names.into_iter().zip(ports).collect()
}

/// Every output port with its numbered name, from one listing.
fn output_ports(output: &MidiOutput) -> Vec<(String, MidiOutputPort)> {
    let ports = output.ports();
    let names = numbered(
        ports
            .iter()
            .map(|p| output.port_name(p).unwrap_or_else(|_| "MIDI output".into())),
    );
    names.into_iter().zip(ports).collect()
}

/// The port named `wanted`, if the listing has it.
fn named<P>(listing: Vec<(String, P)>, wanted: &str) -> Option<P> {
    listing
        .into_iter()
        .find_map(|(name, port)| (name == wanted).then_some(port))
}

struct Open(#[allow(dead_code, reason = "held so the port stays open")] MidiInputConnection<()>);
impl Connection for Open {}

struct Sender(MidiOutputConnection);
impl Output for Sender {
    fn send(&mut self, bytes: &[u8]) -> Result<(), MidiError> {
        self.0
            .send(bytes)
            .map_err(|e| MidiError::Send(e.to_string()))
    }
}

impl Backend for MidirHost {
    fn ports(&mut self) -> Vec<PortInfo> {
        let mut out = Vec::new();
        let ports = |direction, names: Vec<String>| {
            names
                .into_iter()
                .map(move |name| PortInfo {
                    id: PortId {
                        direction,
                        name: name.clone(),
                    },
                    name,
                })
                .collect::<Vec<_>>()
        };
        if let Ok(input) = MidiInput::new(&self.client) {
            let names = input_ports(&input).into_iter().map(|(name, _)| name);
            out.extend(ports(Direction::Input, names.collect()));
        }
        if let Ok(output) = MidiOutput::new(&self.client) {
            let names = output_ports(&output).into_iter().map(|(name, _)| name);
            out.extend(ports(Direction::Output, names.collect()));
        }
        out
    }

    fn open_input(
        &mut self,
        id: &PortId,
        mut sink: InputSink,
    ) -> Result<Box<dyn Connection>, MidiError> {
        let mut input =
            MidiInput::new(&self.client).map_err(|e| MidiError::Unavailable(e.to_string()))?;
        input.ignore(midir::Ignore::None);
        let port = named(input_ports(&input), &id.name)
            .ok_or_else(|| MidiError::Open(format!("{} is not connected", id.name)))?;
        let connection = input
            .connect(
                &port,
                "input",
                move |stamp, bytes, _| sink(stamp, bytes),
                (),
            )
            .map_err(|e| MidiError::Open(e.to_string()))?;
        Ok(Box::new(Open(connection)))
    }

    fn open_output(&mut self, id: &PortId) -> Result<Box<dyn Output>, MidiError> {
        let output =
            MidiOutput::new(&self.client).map_err(|e| MidiError::Unavailable(e.to_string()))?;
        let port = named(output_ports(&output), &id.name)
            .ok_or_else(|| MidiError::Open(format!("{} is not connected", id.name)))?;
        let connection = output
            .connect(&port, "output")
            .map_err(|e| MidiError::Open(e.to_string()))?;
        Ok(Box::new(Sender(connection)))
    }
}

#[cfg(test)]
mod tests {
    use super::{named, numbered};

    #[test]
    fn a_port_is_found_by_its_numbered_name_in_the_listing_it_came_from() {
        let listing = vec![("Keys".to_owned(), 7), ("Keys 2".to_owned(), 9)];
        assert_eq!(named(listing.clone(), "Keys 2"), Some(9));
        assert_eq!(named(listing.clone(), "Keys"), Some(7));
        assert_eq!(named(listing, "Pad"), None);
    }

    #[test]
    fn repeated_names_are_numbered() {
        let n = numbered(
            ["Keys", "Pad", "Keys", "Keys"]
                .into_iter()
                .map(String::from),
        );
        assert_eq!(n, ["Keys", "Pad", "Keys 2", "Keys 3"]);
    }
}
