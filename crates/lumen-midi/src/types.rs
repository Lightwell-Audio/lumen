//! Ports, messages and errors.

use std::fmt;

/// Which way a port carries messages, from Lumen's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    Input,
    Output,
}

/// A port's identity: its direction and name (with a number after the name
/// when two ports share one). Stable while the device stays plugged in, and
/// across runs for the same device, so settings can refer to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortId {
    pub direction: Direction,
    pub name: String,
}

impl fmt::Display for PortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub id: PortId,
    /// What to show: the name the driver gave.
    pub name: String,
}

/// What a port is for. An input with either switch on is opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Use {
    /// Tracks may take notes from it (or send to it, for an output).
    pub track: bool,
    /// Controls may be mapped from it (MIDI learn, M5 slice 7).
    pub remote: bool,
}

impl Use {
    pub fn any(self) -> bool {
        self.track || self.remote
    }
}

/// A channel message of up to three bytes. System exclusive and other long
/// messages are not carried yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Short {
    pub bytes: [u8; 3],
    pub len: u8,
}

impl Short {
    /// `None` for an empty or longer message.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > 3 {
            return None;
        }
        let mut out = [0; 3];
        out[..bytes.len()].copy_from_slice(bytes);
        Some(Self {
            bytes: out,
            len: bytes.len() as u8,
        })
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// A message from an input, stamped by the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timed {
    /// The input's slot (see `MidiSystem::slot_of`).
    pub slot: u16,
    /// The driver's timestamp in microseconds, from an origin of its own.
    pub stamp_us: u64,
    pub message: Short,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiError {
    /// The port is gone or busy; the text says what the driver said.
    Open(String),
    Send(String),
    /// The MIDI system could not start at all.
    Unavailable(String),
}

impl fmt::Display for MidiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MidiError::Open(t) | MidiError::Send(t) | MidiError::Unavailable(t) => f.write_str(t),
        }
    }
}

impl std::error::Error for MidiError {}
