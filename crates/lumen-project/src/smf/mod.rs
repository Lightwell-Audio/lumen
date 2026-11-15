mod read;
mod write;

pub use read::{ReadError, read};
pub use write::{DIVISION, Layout, WriteError, write};

use lumen_core::meter::MeterPoint;
use lumen_core::time::{TempoPoint, Ticks};

use crate::model::{ClipEvent, Note};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MidiFile {
    pub tracks: Vec<FileTrack>,
    pub tempo: Vec<TempoPoint>,
    pub meter: Vec<MeterPoint>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileTrack {
    pub name: Option<String>,
    pub channel: u8,
    pub source: usize,
    pub notes: Vec<Note>,
    pub events: Vec<ClipEvent>,
}

impl FileTrack {
    pub fn end(&self) -> Ticks {
        let notes = self.notes.iter().map(|n| n.end());
        let events = self.events.iter().map(|e| e.at);
        notes.chain(events).max().unwrap_or(Ticks::ZERO)
    }
}

impl MidiFile {
    pub fn end(&self) -> Ticks {
        self.tracks
            .iter()
            .map(FileTrack::end)
            .max()
            .unwrap_or(Ticks::ZERO)
    }
}
