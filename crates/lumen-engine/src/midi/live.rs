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
use super::kit::{DrumKit, KitKey};

/// How many live sources there can be: inputs by slot, and the keyboard.
pub const SOURCES: u8 = 64;
/// The computer keyboard's source number.
pub const KEYBOARD: u8 = SOURCES - 1;
/// The audition source (the piano roll's note preview, M5 slice 3d): its
/// messages carry the track they sound on ([`LiveIn::track`]) and are
/// played there whether or not the track listens. Like every source it has
/// its own notes, so a preview on a pitch a held key or a clip is playing
/// lets go only of itself.
pub const AUDITION: u8 = SOURCES - 2;
/// Inputs take slots below this.
pub const INPUT_SLOTS: u8 = AUDITION;

/// A live message, to be heard at device frame `frame` (the callback's
/// frame count: `RenderContext::device`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveIn {
    pub frame: u64,
    /// The device frame it was played at, before the feed's delay: where a
    /// recording puts it (M5 slice 6). Not after `frame`.
    pub played: u64,
    /// The input's slot (below [`KEYBOARD`]), or [`KEYBOARD`].
    pub source: u8,
    /// For [`AUDITION`]: the id of the track it sounds on. Other sources go
    /// to the tracks that listen.
    pub track: Option<u64>,
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
    /// The note it went out as (a drum map's output note, or the key).
    out_key: u8,
}

/// Per track: the live notes sounding, the pedal and the bend.
pub(super) struct LiveState {
    held: Vec<LiveHeld>,
    /// A live pedal is down, on this outgoing channel.
    sustain: Option<u8>,
    /// A live bend away from the centre, on this outgoing channel.
    bend: Option<u8>,
    /// Live notes refused because [`MAX_HELD`] were held already: counted
    /// with the other events dropped for lack of room.
    pub(super) dropped: u32,
}

impl LiveState {
    pub(super) fn new() -> Self {
        Self {
            held: Vec::with_capacity(MAX_HELD),
            sustain: None,
            bend: None,
            dropped: 0,
        }
    }

    /// A placeholder while the real state is borrowed: never holds.
    pub(super) fn empty() -> Self {
        Self {
            held: Vec::new(),
            sustain: None,
            bend: None,
            dropped: 0,
        }
    }

    /// Live notes held now (tests).
    pub(super) fn len(&self) -> usize {
        self.held.len()
    }

    /// Works out what `msg` from `source` sends, on outgoing channel `out`
    /// (`None` keeps the message's own), and remembers what it holds.
    /// `None` when nothing should go: a note-off for a note this track is
    /// not holding (it was let go already), a note its drum map silences
    /// (M8 D0), or a full tracker.
    pub(super) fn take(
        &mut self,
        source: u8,
        msg: Msg,
        out: Option<u8>,
        kit: Option<&DrumKit>,
    ) -> Option<Msg> {
        let channel = msg.channel();
        let to = out.unwrap_or(channel) & 15;
        let sent = Msg([(msg.0[0] & 0xF0) | to, msg.0[1], msg.0[2]]);
        match msg.kind() {
            Kind::NoteOn { key, velocity } => {
                // What the track's map makes of it: its note, its channel, or
                // nothing at all.
                let played = kit.map_or(KitKey::as_written(key), |kit| kit.key(key));
                if played.silent {
                    return None;
                }
                if self.held.len() >= MAX_HELD {
                    self.dropped = self.dropped.saturating_add(1);
                    return None;
                }
                let out_channel = played.channel.map_or(to, |c| c & 15);
                self.held.push(LiveHeld {
                    source,
                    channel,
                    key,
                    out: out_channel,
                    out_key: played.key,
                });
                Some(Msg::note_on(out_channel, played.key, velocity))
            }
            Kind::NoteOff { key, .. } => {
                let i = self
                    .held
                    .iter()
                    .position(|h| h.source == source && h.channel == channel && h.key == key)?;
                let held = self.held.swap_remove(i);
                Some(Msg::note_off(held.out, held.out_key))
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

    /// Ends the live notes whose outgoing key `ends` says, pushing each
    /// note-off: the sounds another of a choke group has just cut (M8 D0).
    pub(super) fn choke(&mut self, mut ends: impl FnMut(u8) -> bool, mut push: impl FnMut(u8, u8)) {
        let mut i = 0;
        while i < self.held.len() {
            let h = self.held[i];
            if ends(h.out_key) {
                push(h.out, h.out_key);
                self.held.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Forgets the note `take` held last, when its note-on found no room in
    /// the track's queue (which counts the drop): a note that never sounded
    /// owes no note-off and keeps no place among the [`MAX_HELD`].
    pub(super) fn refused(&mut self, msg: Msg) {
        if let Kind::NoteOn { .. } = msg.kind() {
            self.held.pop();
        }
    }

    /// Lets go of every live note, the pedal and the bend, at the start of
    /// the next part: `push` takes each message.
    pub(super) fn release_all(&mut self, mut push: impl FnMut(Event)) {
        for held in self.held.drain(..) {
            push(Event {
                offset: 0,
                msg: Msg::note_off(held.out, held.out_key),
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
    pub(super) fn clear(&mut self) {
        self.held.clear();
        self.sustain = None;
        self.bend = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_note_past_the_most_held_is_refused_and_counted() {
        let mut live = LiveState::new();
        for key in 0..MAX_HELD {
            let key = (key % 128) as u8;
            let source = (key as usize / 128) as u8;
            assert!(
                live.take(source, Msg::note_on(0, key, 100), None, None)
                    .is_some()
            );
        }
        assert_eq!(live.dropped, 0);
        assert_eq!(live.take(9, Msg::note_on(0, 1, 100), None, None), None);
        assert_eq!(live.dropped, 1, "the refused note is counted");
        // A note-off for a note not held is not a drop.
        assert_eq!(live.take(9, Msg::note_off(0, 1), None, None), None);
        assert_eq!(live.dropped, 1);
    }

    #[test]
    fn a_live_note_the_queue_refused_is_not_held() {
        let mut live = LiveState::new();
        let on = live.take(0, Msg::note_on(0, 60, 100), None, None).unwrap();
        live.refused(on);
        assert_eq!(live.len(), 0, "a note that never sounded is not held");
        assert_eq!(
            live.take(0, Msg::note_off(0, 60), None, None),
            None,
            "and owes no note-off"
        );
        // A refused pedal leaves the held notes alone.
        live.take(0, Msg::note_on(0, 61, 100), None, None).unwrap();
        live.refused(Msg::controller(0, cc::SUSTAIN, 127));
        assert_eq!(live.len(), 1);
    }

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
            live.take(1, Msg::note_on(0, 60, 90), Some(5), None),
            Some(Msg::note_on(5, 60, 90))
        );
        // Another source's off for the same key is not this note's.
        assert_eq!(live.take(2, Msg::note_off(0, 60), Some(5), None), None);
        assert_eq!(
            live.take(1, Msg::note_off(0, 60), Some(5), None),
            Some(Msg::note_off(5, 60))
        );
        assert_eq!(live.take(1, Msg::note_off(0, 60), Some(5), None), None);
        assert_eq!(live.len(), 0);
    }

    #[test]
    fn releasing_lets_go_of_notes_pedal_and_bend() {
        let mut live = LiveState::new();
        live.take(0, Msg::note_on(2, 60, 90), None, None);
        live.take(0, Msg::controller(2, cc::SUSTAIN, 127), None, None);
        live.take(0, Msg::pitch_bend(2, 300), None, None);
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
