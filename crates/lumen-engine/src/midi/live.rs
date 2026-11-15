use lumen_core::midi::{Event, Kind, Msg, cc};

use super::hold::MAX_HELD;
use super::kit::{DrumKit, KitKey};

pub const SOURCES: u8 = 64;
pub const KEYBOARD: u8 = SOURCES - 1;
pub const AUDITION: u8 = SOURCES - 2;
pub const INPUT_SLOTS: u8 = AUDITION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveIn {
    pub frame: u64,
    pub played: u64,
    pub source: u8,
    pub track: Option<u64>,
    pub msg: Msg,
}

pub trait LiveInput: Send {
    fn pull(&mut self, start: u64, end: u64, take: &mut dyn FnMut(LiveIn));

    fn waiting(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Listen {
    pub sources: u64,
    pub channel: Option<u8>,
}

impl Listen {
    pub const OFF: Listen = Listen {
        sources: 0,
        channel: None,
    };

    pub fn is_on(self) -> bool {
        self.sources != 0
    }

    pub fn takes(self, source: u8, msg: Msg) -> bool {
        source < SOURCES
            && self.sources & (1 << source) != 0
            && self.channel.is_none_or(|c| c == msg.channel())
            && msg.0[0] & 0xF0 != 0xF0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveHeld {
    source: u8,
    channel: u8,
    key: u8,
    out: u8,
    out_key: u8,
}

pub(super) struct LiveState {
    held: Vec<LiveHeld>,
    sustain: Option<u8>,
    bend: Option<u8>,
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

    pub(super) fn empty() -> Self {
        Self {
            held: Vec::new(),
            sustain: None,
            bend: None,
            dropped: 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.held.len()
    }

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

    pub(super) fn refused(&mut self, msg: Msg) {
        if let Kind::NoteOn { .. } = msg.kind() {
            self.held.pop();
        }
    }

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
