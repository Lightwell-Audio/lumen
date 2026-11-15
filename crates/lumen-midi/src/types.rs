use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    Input,
    Output,
}

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
    pub name: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Use {
    pub track: bool,
    pub remote: bool,
    pub sync: bool,
}

impl Use {
    pub fn any(self) -> bool {
        self.track || self.remote || self.sync
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Short {
    pub bytes: [u8; 3],
    pub len: u8,
}

impl Short {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timed {
    pub slot: u16,
    pub stamp_us: u64,
    pub arrived: std::time::Instant,
    pub message: Short,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiError {
    Open(String),
    Send(String),
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
