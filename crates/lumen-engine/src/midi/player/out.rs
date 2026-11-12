//! The hardware output queue: messages waiting for the app's sender, with
//! room kept for the note-offs that let go.

use lumen_core::midi::Msg;

use super::super::hold;
use super::super::{MidiOut, MidiTrack, Target};

/// Messages waiting for the app's sender.
pub(super) const OUT_QUEUE: usize = 8_192;
/// Slots at the end of the hardware queue kept for what lets go: a release
/// for every note eight hardware tracks can hold. Eight is not a limit on
/// tracks: past it, note-offs share the ordinary room, which only a sender
/// thousands of messages behind fills.
const OUT_KEPT_FOR_RELEASES: usize = 8 * hold::KEPT_FOR_RELEASES;

/// Where messages for hardware go.
pub(in crate::midi) struct Out {
    pub(super) tx: rtrb::Producer<MidiOut>,
    pub(super) dropped: u32,
}

impl Out {
    /// Queues `message` for the sender, or counts it dropped. As a track's
    /// queue does, the last [`OUT_KEPT_FOR_RELEASES`] slots take only the
    /// note-offs of held notes (`hold::ends_a_held_note`) and Flush, so a
    /// full queue refuses new notes before a note-off. A pedal or a bend put
    /// back at rest takes ordinary room here: by the time it is forwarded it
    /// cannot be told from a clip's own, and a flood of those must not use
    /// up the room the note-offs need.
    pub(super) fn push(&mut self, message: MidiOut) {
        let releases = match message {
            MidiOut::At { msg, .. } => hold::ends_a_held_note(msg),
            MidiOut::Flush => true,
        };
        let full = if releases {
            self.tx.slots() == 0
        } else {
            self.tx.slots() <= OUT_KEPT_FOR_RELEASES
        };
        if full || self.tx.push(message).is_err() {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    pub(in crate::midi) fn at(&mut self, frame: u64, port: u16, msg: Msg) {
        self.push(MidiOut::At { frame, port, msg });
    }

    /// Sends a hardware track's queued events from device frame `frame`.
    pub(in crate::midi) fn forward(&mut self, track: &mut MidiTrack, frame: u64) {
        if let Target::Port { port, .. } = track.target {
            for event in track.state.events.drain(..) {
                self.at(frame + u64::from(event.offset), port, event.msg);
            }
        }
    }
}
