//! Standard MIDI Files (M5 slice 7, ADR-0062): a `.mid` file's bytes to and
//! from a [`MidiFile`], the notes and events Lumen keeps, in its own ticks.
//! Pure: no I/O. The app reads and writes the files off the UI thread.
//!
//! What a file holds that the model has no place for is dropped on reading:
//! release velocities, polyphonic aftertouch, system exclusive and meta text
//! other than track names. Only the first time signature is kept until M7's
//! time-signature lane.

mod read;
mod write;

pub use read::{ReadError, read};
pub use write::{DIVISION, Layout, WriteError, write};

use lumen_core::time::{TempoPoint, Ticks};

use crate::model::{ClipEvent, Note, TimeSignature};

/// What Lumen reads from, or writes to, a MIDI file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MidiFile {
    /// The parts, in the file's order. On reading, each file track is split
    /// by MIDI channel, so a type 0 file gives one part per channel.
    pub tracks: Vec<FileTrack>,
    /// The tempo map, first point at zero; empty when the file sets no
    /// tempo (players then assume 120 BPM).
    pub tempo: Vec<TempoPoint>,
    /// The first time signature, if the file names one.
    pub meter: Option<TimeSignature>,
}

/// One part of a file: the notes and events of one channel of one track.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileTrack {
    /// The track's name, if it has one.
    pub name: Option<String>,
    /// The MIDI channel, 0 to 15.
    pub channel: u8,
    /// Which of the file's tracks this part came from (0 on writing).
    pub source: usize,
    /// Notes in start order, from the file's start.
    pub notes: Vec<Note>,
    /// Controllers, pitch bend, pressure and program changes, in time order.
    pub events: Vec<ClipEvent>,
}

impl FileTrack {
    /// Where the last note or event ends.
    pub fn end(&self) -> Ticks {
        let notes = self.notes.iter().map(|n| n.end());
        let events = self.events.iter().map(|e| e.at);
        notes.chain(events).max().unwrap_or(Ticks::ZERO)
    }
}

impl MidiFile {
    /// Where the last part ends.
    pub fn end(&self) -> Ticks {
        self.tracks
            .iter()
            .map(FileTrack::end)
            .max()
            .unwrap_or(Ticks::ZERO)
    }
}
