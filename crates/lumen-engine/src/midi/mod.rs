//! MIDI on the timeline (M5 slice 1, ADR-0045).
//!
//! A [`MidiTrack`] holds a track's MIDI clips, the instrument that plays
//! them (or the hardware port they go to) and a note tracker. The
//! [`MidiPlayer`] reads the clips a block at a time: notes and controllers
//! become events at their exact frame, an instrument turns them into audio
//! that is added to the track's bus (so volume, pan, sends, groups, solo,
//! meters and delay compensation apply as they do to audio clips), and a
//! hardware track's events leave through a lock-free queue stamped with the
//! device frame at which they should be heard.
//!
//! Notes never hang: the tracker (`hold`) knows every sounding note, and
//! stop, a jump, a loop wrap, an edit, a change of instrument or port and
//! Panic all release them. Play and every jump **chase**: notes already
//! sounding at the new position start at once, and the latest controllers
//! before it are applied first.
//!
//! Tracks that listen also play live input (`live`, slice 2): a
//! [`LiveInput`] from the app gives each part its messages at their frame.
//!
//! Like clip lists, track lists are replaced whole through a queue; the
//! old list goes back to be freed on the UI side. Tracker, instrument and
//! voices carry across by track id, as pointers swap.

mod capture;
mod hold;
mod live;
mod player;

pub use capture::{Captured, WRAP};
pub use live::{AUDITION, INPUT_SLOTS, KEYBOARD, Listen, LiveIn, LiveInput, SOURCES};
pub use player::{MidiHandle, MidiPlayer, midi_player};

use std::sync::Arc;

use lumen_core::midi::Msg;
use lumen_core::time::Ticks;
use lumen_devices::Instrument;

use hold::TrackState;

/// A note, in ticks from the start of its clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiNote {
    pub start: Ticks,
    pub length: Ticks,
    pub pitch: u8,
    pub velocity: u8,
}

/// A controller, pitch bend, pressure or program message, in ticks from the
/// start of its clip. The channel in `msg` is ignored: the track's is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiControl {
    pub at: Ticks,
    pub msg: Msg,
}

/// Sorts notes into the order the player reads them (shared, so edits that
/// leave a clip alone can keep its list).
pub fn note_list(mut notes: Vec<MidiNote>) -> Arc<[MidiNote]> {
    notes.sort_by_key(|n| (n.start, n.pitch));
    notes.into()
}

/// Sorts controls by time (equal times keep their order).
pub fn control_list(mut controls: Vec<MidiControl>) -> Arc<[MidiControl]> {
    controls.sort_by_key(|c| c.at);
    controls.into()
}

/// A MIDI clip. `notes` and `controls` must be sorted ([`note_list`],
/// [`control_list`]).
#[derive(Debug, Clone)]
pub struct MidiClip {
    /// The project's clip id, to tell an edited clip from a new one.
    pub id: u64,
    pub start: Ticks,
    /// How long it lasts on the timeline, repeats included.
    pub length: Ticks,
    /// The content repeats every `period` from the start.
    pub period: Option<Ticks>,
    pub notes: Arc<[MidiNote]>,
    pub controls: Arc<[MidiControl]>,
}

impl MidiClip {
    /// Whether `other` is the same clip with the same content.
    fn same_as(&self, other: &MidiClip) -> bool {
        self.id == other.id
            && self.start == other.start
            && self.length == other.length
            && self.period == other.period
            && (Arc::ptr_eq(&self.notes, &other.notes) || self.notes == other.notes)
            && (Arc::ptr_eq(&self.controls, &other.controls) || self.controls == other.controls)
    }
}

/// Where a track's events go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The track's instrument.
    Instrument,
    /// A hardware output port, by the number the app's sender knows it by,
    /// on a channel (0 to 15).
    Port { port: u16, channel: u8 },
}

/// An instrument and the sound it plays.
pub struct InstrumentSpec {
    /// Which instrument: tracks keep their voices while it stays the same.
    pub kind: u32,
    /// Which sound of it (a preset), chosen with
    /// [`Instrument::configure`].
    pub preset: u32,
    pub instrument: Box<dyn Instrument>,
}

/// One track's MIDI: built on the UI side, played on the audio thread.
pub struct MidiTrack {
    /// The track's id: its node in the routing.
    pub id: u64,
    /// In start order. Empty for a track that plays nothing (a hardware
    /// track that is muted).
    pub clips: Vec<MidiClip>,
    pub instrument: Option<InstrumentSpec>,
    pub target: Target,
    /// The live input it plays ([`Listen::OFF`] for none).
    pub listen: Listen,
    /// The live input it keeps while the transport records
    /// ([`Listen::OFF`] for none): a recording track need not listen.
    pub record: Listen,
    state: Box<TrackState>,
    /// Set when the list arrives: the track differs from the one it replaces,
    /// so its notes are brought in line with the position.
    dirty: bool,
}

impl MidiTrack {
    /// Allocates; call off the audio thread. Sorts `clips` by start.
    pub fn new(
        id: u64,
        target: Target,
        instrument: Option<InstrumentSpec>,
        mut clips: Vec<MidiClip>,
    ) -> Self {
        clips.sort_by_key(|c| c.start);
        Self {
            id,
            clips,
            instrument,
            target,
            listen: Listen::OFF,
            record: Listen::OFF,
            state: Box::new(TrackState::new()),
            dirty: true,
        }
    }

    /// The same track, playing the live input `listen` takes.
    pub fn listening(mut self, listen: Listen) -> Self {
        self.listen = listen;
        self
    }

    /// The same track, keeping the live input `record` takes while the
    /// transport records (M5 slice 6).
    pub fn recording(mut self, record: Listen) -> Self {
        self.record = record;
        self
    }

    /// The channel its events carry.
    fn channel(&self) -> u8 {
        match self.target {
            Target::Instrument => 0,
            Target::Port { channel, .. } => channel & 15,
        }
    }

    /// Whether `other` plays the same notes the same way to the same place.
    fn same_as(&self, other: &MidiTrack) -> bool {
        self.target == other.target
            && self.instrument.as_ref().map(|i| (i.kind, i.preset))
                == other.instrument.as_ref().map(|i| (i.kind, i.preset))
            && self.clips.len() == other.clips.len()
            && self
                .clips
                .iter()
                .zip(&other.clips)
                .all(|(a, b)| a.same_as(b))
    }
}

/// What the engine sends toward hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiOut {
    /// A message for `port`, to be heard at device frame `frame` (see
    /// `RenderContext::stream`); one already past goes at once.
    At { frame: u64, port: u16, msg: Msg },
    /// Playback stopped or panicked: forget messages not sent yet. The
    /// note-offs follow.
    Flush,
}
