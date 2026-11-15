use lumen_core::midi::Msg;

use super::super::hold;
use super::super::{MidiOut, MidiTrack, Target};

pub(super) const OUT_QUEUE: usize = 8_192;
const OUT_KEPT_FOR_RELEASES: usize = 8 * hold::KEPT_FOR_RELEASES;

pub(in crate::midi) struct Out {
    pub(super) tx: rtrb::Producer<MidiOut>,
    pub(super) dropped: u32,
}

impl Out {
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

    pub(in crate::midi) fn forward(&mut self, track: &mut MidiTrack, frame: u64) {
        if let Target::Port { port, .. } = track.target {
            for event in track.state.events.drain(..) {
                self.at(frame + u64::from(event.offset), port, event.msg);
            }
        }
    }
}
