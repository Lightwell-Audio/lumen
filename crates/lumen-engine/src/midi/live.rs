//! Live MIDI: what players send from keyboards and controllers, played on
//! the tracks that listen (M5 slice 2, ADR-0046).
//!
//! The app hands the engine a [`LiveInput`], which runs on the audio thread
//! and gives each part the messages to be heard in it, stamped with the
//! device frame (the owner's J: each message one buffer after it was
//! played, so the spacing is kept). A track listens through its
//! [`Listen`]: the sources it takes (a bit per input, and one for the
//! computer keyboard) and the channel. What it takes goes the way its clip
//! events go: into its instrument, or out to its MIDI To port on its
//! channel.
//!
//! Live notes have their own tracker ([`LiveState`]), apart from the
//! clips', so a key held while the song plays the same note is let go by
//! its own note-off. Stop and jumps leave live notes alone (a held key
//! keeps sounding); a track that stops listening, changes its source,
//! instrument or port, or leaves, lets go of them, and Panic forgets them.

use lumen_core::midi::{Event, Kind, Msg, cc};

use super::hold::MAX_HELD;

/// How many live sources there can be: inputs by slot, and the keyboard.
pub const SOURCES: u8 = 64;
/// The computer keyboard's source number.
pub const KEYBOARD: u8 = SOURCES - 1;

/// A live message, to be heard at device frame `frame` (the callback's
/// frame count: `RenderContext::device`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveIn {
    pub frame: u64,
    /// The input's slot (below [`KEYBOARD`]), or [`KEYBOARD`].
    pub source: u8,
    pub msg: Msg,
}

/// Where live messages come from. Runs on the audio thread: every method
/// must be real-time safe.
pub trait LiveInput: Send {
    /// Hands `take` every message to be heard before device frame `end`, in
    /// order; ones whose frame is before `start` (late) are given too, and
    /// are played at `start`.
    fn pull(&mut self, start: u64, end: u64, take: &mut dyn FnMut(LiveIn));

    /// Whether messages are waiting: the engine renders while stopped
    /// until they are played.
    fn waiting(&self) -> bool;
}

/// Which live messages a track plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Listen {
    /// A bit per source (`1 << source`); none, the track does not listen.
    pub sources: u64,
    /// Only this channel (0 to 15), or every channel.
    pub channel: Option<u8>,
}

impl Listen {
    /// Not listening.
    pub const OFF: Listen = Listen {
        sources: 0,
        channel: None,
    };

    pub fn is_on(self) -> bool {
        self.sources != 0
    }

    /// Whether the track plays `msg` from `source`.
    pub fn takes(self, source: u8, msg: Msg) -> bool {
        source < SOURCES
            && self.sources & (1 << source) != 0
            && self.channel.is_none_or(|c| c == msg.channel())
            && msg.0[0] & 0xF0 != 0xF0
    }
}

/// A live note sounding: where it came from, and what went out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveHeld {
    source: u8,
    /// The channel and key it arrived on.
    channel: u8,
    key: u8,
    /// The channel it went out on.
    out: u8,
}

/// Per track: the live notes sounding, the pedal and the bend.
pub(super) struct LiveState {
    held: Vec<LiveHeld>,
    /// A live pedal is down, on this outgoing channel.
    sustain: Option<u8>,
    /// A live bend away from the centre, on this outgoing channel.
    bend: Option<u8>,
}

impl LiveState {
    pub fn new() -> Self {
        Self {
            held: Vec::with_capacity(MAX_HELD),
            sustain: None,
            bend: None,
        }
    }

    /// A placeholder while the real state is borrowed: never holds.
    pub fn empty() -> Self {
        Self {
            held: Vec::new(),
            sustain: None,
            bend: None,
        }
    }

    /// Live notes held now (tests).
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Works out what `msg` from `source` sends, on outgoing channel `out`
    /// (`None` keeps the message's own), and remembers what it holds.
    /// `None` when nothing should go: a note-off for a note this track is
    /// not holding (it was let go already), or a full tracker.
    pub fn take(&mut self, source: u8, msg: Msg, out: Option<u8>) -> Option<Msg> {
        let channel = msg.channel();
        let to = out.unwrap_or(channel) & 15;
        let sent = Msg([(msg.0[0] & 0xF0) | to, msg.0[1], msg.0[2]]);
        match msg.kind() {
            Kind::NoteOn { key, .. } => {
                if self.held.len() >= MAX_HELD {
                    return None;
                }
                self.held.push(LiveHeld {
                    source,
                    channel,
                    key,
                    out: to,
                });
                Some(sent)
            }
            Kind::NoteOff { key, .. } => {
                let i = self
                    .held
                    .iter()
                    .position(|h| h.source == source && h.channel == channel && h.key == key)?;
                let held = self.held.swap_remove(i);
                Some(Msg::note_off(held.out, held.key))
            }
            Kind::Controller { number, value } => {
                if number == cc::SUSTAIN {
                    self.sustain = (value >= 64).then_some(to);
                }
                Some(sent)
            }
            Kind::PitchBend { value } => {
                self.bend = (value != 0).then_some(to);
                Some(sent)
            }
            _ => Some(sent),
        }
    }

    /// Lets go of every live note, the pedal and the bend, at the start of
    /// the next part: `push` takes each message.
    pub fn release_all(&mut self, mut push: impl FnMut(Event)) {
        for held in self.held.drain(..) {
            push(Event {
                offset: 0,
                msg: Msg::note_off(held.out, held.key),
            });
        }
        if let Some(channel) = self.sustain.take() {
            push(Event {
                offset: 0,
                msg: Msg::controller(channel, cc::SUSTAIN, 0),
            });
        }
        if let Some(channel) = self.bend.take() {
            push(Event {
                offset: 0,
                msg: Msg::pitch_bend(channel, 0),
            });
        }
    }

    /// Forgets everything without sending (Panic silences the rest).
    pub fn clear(&mut self) {
        self.held.clear();
        self.sustain = None;
        self.bend = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_takes_its_sources_and_channel() {
        let listen = Listen {
            sources: 1 << 2 | 1 << KEYBOARD,
            channel: Some(3),
        };
        assert!(listen.takes(2, Msg::note_on(3, 60, 100)));
        assert!(listen.takes(KEYBOARD, Msg::note_on(3, 60, 100)));
        assert!(!listen.takes(1, Msg::note_on(3, 60, 100)));
        assert!(!listen.takes(2, Msg::note_on(4, 60, 100)));
        assert!(!Listen::OFF.takes(2, Msg::note_on(3, 60, 100)));
        // System messages (clock) are never played.
        assert!(
            !Listen {
                sources: !0,
                channel: None
            }
            .takes(0, Msg([0xF8, 0, 0]))
        );
    }

    #[test]
    fn note_offs_end_only_the_notes_held_and_go_out_where_they_began() {
        let mut live = LiveState::new();
        assert_eq!(
            live.take(1, Msg::note_on(0, 60, 90), Some(5)),
            Some(Msg::note_on(5, 60, 90))
        );
        // Another source's off for the same key is not this note's.
        assert_eq!(live.take(2, Msg::note_off(0, 60), Some(5)), None);
        assert_eq!(
            live.take(1, Msg::note_off(0, 60), Some(5)),
            Some(Msg::note_off(5, 60))
        );
        assert_eq!(live.take(1, Msg::note_off(0, 60), Some(5)), None);
        assert_eq!(live.len(), 0);
    }

    #[test]
    fn releasing_lets_go_of_notes_pedal_and_bend() {
        let mut live = LiveState::new();
        live.take(0, Msg::note_on(2, 60, 90), None);
        live.take(0, Msg::controller(2, cc::SUSTAIN, 127), None);
        live.take(0, Msg::pitch_bend(2, 300), None);
        let mut out = Vec::new();
        live.release_all(|e| out.push(e.msg));
        assert_eq!(
            out,
            vec![
                Msg::note_off(2, 60),
                Msg::controller(2, cc::SUSTAIN, 0),
                Msg::pitch_bend(2, 0)
            ]
        );
        let mut again = Vec::new();
        live.release_all(|e| again.push(e.msg));
        assert!(again.is_empty());
    }
}
