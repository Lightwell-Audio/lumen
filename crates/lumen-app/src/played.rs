//! Capture MIDI (M9 slice R3, ADR-0104): what was played on MIDI tracks while
//! not recording, kept in memory so a phrase is never lost.
//!
//! The engine hands over every message a track listens to or is armed for
//! when the transport is not recording (`lumen_engine::Captured` with a
//! [`Heard`] other than `Recording`). [`Played`] keeps the newest ten minutes
//! of them, at most [`MOST_MESSAGES`], in memory only: nothing is saved, and
//! the buffer is emptied when the project closes or the setting is switched
//! off.
//!
//! The messages fall into **phrases**: a new one begins whenever the
//! transport starts or stops (the state of the messages changes) and
//! whenever the playhead jumps back (a loop wrap or a locate). The newest
//! phrase of a track that has a note is what Capture MIDI turns into a take
//! ([`take_of`]), which is placed as an Overdub so nothing under it is lost:
//!
//! - played while the transport moved: each message keeps the timeline
//!   position it was heard at, less the output latency, like a recording;
//! - played while stopped: the first note lands on the playhead and the rest
//!   keep their time apart from the device frame, at the project's tempo.
//!
//! The take begins on the bar line at or before its first note and ends on
//! the bar line at or after its last, so the clip is a whole number of bars.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use lumen_core::meter::MeterMap;
use lumen_core::midi::Kind;
use lumen_core::time::Ticks;
use lumen_engine::{Captured, Heard, WRAP};
use lumen_project::edit::record::Take;

use crate::recorder::Capture;

/// How long a message is kept.
pub const KEPT_FOR: Duration = Duration::from_secs(10 * 60);
/// The most messages kept; the oldest go first.
pub const MOST_MESSAGES: usize = 20_000;

/// A message and the phrase it belongs to.
#[derive(Debug, Clone, Copy)]
struct Entry {
    phrase: u64,
    heard: Instant,
    message: Captured,
}

/// One track's newest phrase that has a note: its messages in the order they
/// were played, and whether the transport moved.
#[derive(Debug, Clone, PartialEq)]
pub struct Phrase {
    /// Which phrase of the buffer it is, to forget it once captured.
    id: u64,
    pub track: u64,
    pub state: Heard,
    pub messages: Vec<Captured>,
}

/// The rolling buffer.
#[derive(Debug, Default)]
pub struct Played {
    entries: VecDeque<Entry>,
    phrase: u64,
    /// The state and timeline sample of the newest kept message, to see a
    /// phrase begin.
    last: Option<(Heard, i64)>,
}

impl Played {
    /// Keeps `message` (a recording's messages are not kept), heard `now`.
    pub fn push(&mut self, message: &Captured, now: Instant) {
        if message.state == Heard::Recording || message.source == WRAP {
            return;
        }
        let begins = match self.last {
            None => true,
            Some((state, at)) => {
                state != message.state || (message.state == Heard::Playing && message.at < at)
            }
        };
        if begins {
            self.phrase += 1;
        }
        self.last = Some((message.state, message.at));
        self.entries.push_back(Entry {
            phrase: self.phrase,
            heard: now,
            message: *message,
        });
        self.forget(now);
    }

    /// Drops what is too old or beyond the most kept.
    pub fn forget(&mut self, now: Instant) {
        while self.entries.len() > MOST_MESSAGES {
            self.entries.pop_front();
        }
        while self
            .entries
            .front()
            .is_some_and(|e| now.saturating_duration_since(e.heard) > KEPT_FOR)
        {
            self.entries.pop_front();
        }
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.last = None;
    }

    /// Forgets `phrase`, once it has been captured: asking again does not
    /// make a second clip of it.
    pub fn remove(&mut self, phrase: &Phrase) {
        self.entries
            .retain(|e| !(e.phrase == phrase.id && e.message.track == phrase.track));
    }

    #[cfg(test)]
    /// How many messages are kept.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The track that was played last.
    pub fn last_track(&self) -> Option<u64> {
        self.entries.back().map(|e| e.message.track)
    }

    #[cfg(test)]
    /// Whether any kept phrase of `track` has a note.
    pub fn has_notes(&self, track: u64) -> bool {
        self.entries
            .iter()
            .any(|e| e.message.track == track && is_note_on(&e.message))
    }

    /// The newest phrase of `track` that has a note.
    pub fn newest(&self, track: u64) -> Option<Phrase> {
        let phrase = self
            .entries
            .iter()
            .rev()
            .find(|e| e.message.track == track && is_note_on(&e.message))?
            .phrase;
        let messages: Vec<Captured> = self
            .entries
            .iter()
            .filter(|e| e.phrase == phrase && e.message.track == track)
            .map(|e| e.message)
            .collect();
        let state = messages.first()?.state;
        Some(Phrase {
            id: phrase,
            track,
            state,
            messages,
        })
    }
}

fn is_note_on(c: &Captured) -> bool {
    matches!(c.msg.kind(), Kind::NoteOn { velocity, .. } if velocity > 0)
}

/// What a phrase becomes as a take: where it is placed and what it holds.
/// `playhead` is the timeline sample of the playhead, where a phrase played
/// while stopped begins; `latency` the output latency in samples, taken off a
/// phrase played while the transport moved; `ticks` turns a timeline sample
/// into project ticks. None if the phrase holds no note.
pub fn take_of(
    phrase: &Phrase,
    playhead: i64,
    latency: i64,
    ticks: &dyn Fn(i64) -> Ticks,
    meter: &MeterMap,
) -> Option<Take> {
    let first = phrase.messages.first()?;
    let (latency, base) = match phrase.state {
        Heard::Stopped => (0, Some((first.frame, playhead))),
        _ => (latency, None),
    };
    let mut capture = Capture::new();
    let mut end = Ticks::ZERO;
    for message in &phrase.messages {
        let mut message = *message;
        if let Some((from, to)) = base {
            message.at = to + (message.frame as i64 - from as i64);
        }
        end = end.max(ticks(message.at.max(0)));
        capture.push(&message, latency, ticks);
    }
    // Keys still down end with the phrase.
    capture.close_all(end);
    let track = first.track;
    let mut take = capture.take(track, (Ticks::ZERO, Ticks::ZERO));
    let begin = take.notes.iter().map(|n| n.start).min()?;
    let last = take
        .notes
        .iter()
        .map(|n| n.start + n.length)
        .max()
        .unwrap_or(begin);
    // Whole bars: from the bar line at or before the first note to the one
    // at or after the last note's end.
    take.start = meter.bar_start(meter.bbt_at(begin).bar);
    let tail = meter.bbt_at(last);
    take.end = if meter.bar_start(tail.bar) >= last {
        meter.bar_start(tail.bar)
    } else {
        meter.bar_start(tail.bar + 1)
    };
    take.end = take.end.max(take.start + Ticks(1));
    Some(take)
}

#[cfg(test)]
mod tests {
    use lumen_core::meter::{Meter, MeterMap};
    use lumen_core::midi::Msg;
    use lumen_core::time::PPQ;

    use super::*;

    fn heard(track: u64, state: Heard, at: i64, frame: u64, msg: Msg) -> Captured {
        Captured {
            track,
            source: 0,
            at,
            pass: 0,
            frame,
            state,
            msg,
        }
    }

    fn on(key: u8) -> Msg {
        Msg::note_on(0, key, 90)
    }

    fn off(key: u8) -> Msg {
        Msg::note_off(0, key)
    }

    fn meter() -> MeterMap {
        MeterMap::constant(Meter::FOUR_FOUR).unwrap()
    }

    /// One sample is one tick.
    fn ticks(sample: i64) -> Ticks {
        Ticks(sample)
    }

    const BAR: i64 = 4 * PPQ;

    #[test]
    fn a_recording_is_not_kept_and_a_wrap_marker_is_not_either() {
        let mut played = Played::default();
        let now = Instant::now();
        played.push(&heard(1, Heard::Recording, 0, 0, on(60)), now);
        let mut marker = heard(1, Heard::Playing, 0, 0, Msg([0, 0, 0]));
        marker.source = WRAP;
        played.push(&marker, now);
        assert!(played.is_empty());
    }

    #[test]
    fn a_new_phrase_begins_when_the_transport_starts_or_stops() {
        let mut played = Played::default();
        let now = Instant::now();
        played.push(&heard(1, Heard::Stopped, 0, 100, on(60)), now);
        played.push(&heard(1, Heard::Stopped, 0, 200, off(60)), now);
        played.push(&heard(1, Heard::Playing, 5_000, 300, on(64)), now);
        played.push(&heard(1, Heard::Playing, 5_100, 400, off(64)), now);
        let newest = played.newest(1).unwrap();
        assert_eq!(newest.state, Heard::Playing);
        assert_eq!(newest.messages.len(), 2);
        assert_eq!(newest.messages[0].msg, on(64));
    }

    #[test]
    fn a_phrase_begins_when_the_playhead_goes_back() {
        let mut played = Played::default();
        let now = Instant::now();
        played.push(&heard(1, Heard::Playing, 9_000, 100, on(60)), now);
        played.push(&heard(1, Heard::Playing, 9_100, 150, off(60)), now);
        // A loop wrap or a locate.
        played.push(&heard(1, Heard::Playing, 1_000, 200, on(62)), now);
        played.push(&heard(1, Heard::Playing, 1_100, 250, off(62)), now);
        let newest = played.newest(1).unwrap();
        assert_eq!(newest.messages.len(), 2);
        assert_eq!(newest.messages[0].msg, on(62));
    }

    #[test]
    fn the_newest_phrase_with_a_note_is_taken_for_each_track() {
        let mut played = Played::default();
        let now = Instant::now();
        played.push(&heard(1, Heard::Stopped, 0, 100, on(60)), now);
        played.push(&heard(2, Heard::Stopped, 0, 150, on(70)), now);
        // Only a pedal in the newest phrase of track 1: not a phrase to take.
        played.push(
            &heard(1, Heard::Playing, 5_000, 200, Msg::controller(0, 64, 127)),
            now,
        );
        assert_eq!(played.newest(1).unwrap().messages[0].msg, on(60));
        assert_eq!(played.newest(2).unwrap().messages[0].msg, on(70));
        assert_eq!(played.last_track(), Some(1));
        assert!(played.has_notes(2));
        assert!(played.newest(3).is_none());
    }

    #[test]
    fn it_forgets_what_is_old_and_what_is_past_the_most() {
        let mut played = Played::default();
        let start = Instant::now();
        played.push(&heard(1, Heard::Stopped, 0, 1, on(60)), start);
        played.push(
            &heard(1, Heard::Stopped, 0, 2, on(62)),
            start + KEPT_FOR / 2,
        );
        played.push(
            &heard(1, Heard::Stopped, 0, 3, on(64)),
            start + KEPT_FOR + Duration::from_secs(1),
        );
        assert_eq!(played.len(), 2, "the first is over ten minutes old");
        let mut played = Played::default();
        for n in 0..(MOST_MESSAGES + 5) {
            played.push(&heard(1, Heard::Stopped, 0, n as u64, on(60)), start);
        }
        assert_eq!(played.len(), MOST_MESSAGES);
        played.clear();
        assert!(played.is_empty());
    }

    #[test]
    fn a_phrase_played_while_the_transport_moved_keeps_its_time() {
        let phrase = Phrase {
            id: 1,
            track: 1,
            state: Heard::Playing,
            messages: vec![
                heard(1, Heard::Playing, BAR + 100, 10, on(60)),
                heard(1, Heard::Playing, BAR + 100 + PPQ, 20, off(60)),
            ],
        };
        let take = take_of(&phrase, 0, 0, &ticks, &meter()).unwrap();
        assert_eq!(take.notes.len(), 1);
        assert_eq!(take.notes[0].start, Ticks(BAR + 100));
        assert_eq!(take.notes[0].length, Ticks(PPQ));
        // Bar 2 to bar 3.
        assert_eq!((take.start, take.end), (Ticks(BAR), Ticks(2 * BAR)));
    }

    #[test]
    fn the_output_latency_is_taken_off_a_moving_phrase() {
        let phrase = Phrase {
            id: 1,
            track: 1,
            state: Heard::Playing,
            messages: vec![
                heard(1, Heard::Playing, BAR + 100, 10, on(60)),
                heard(1, Heard::Playing, BAR + 600, 20, off(60)),
            ],
        };
        let take = take_of(&phrase, 0, 40, &ticks, &meter()).unwrap();
        assert_eq!(take.notes[0].start, Ticks(BAR + 60));
    }

    #[test]
    fn a_phrase_played_while_stopped_begins_on_the_playhead() {
        // Every message shares one timeline sample; the device frames differ.
        let phrase = Phrase {
            id: 1,
            track: 1,
            state: Heard::Stopped,
            messages: vec![
                heard(1, Heard::Stopped, 7, 1_000, on(60)),
                heard(1, Heard::Stopped, 7, 1_000 + PPQ as u64, off(60)),
                heard(1, Heard::Stopped, 7, 1_000 + 2 * PPQ as u64, on(64)),
                heard(1, Heard::Stopped, 7, 1_000 + 3 * PPQ as u64, off(64)),
            ],
        };
        let playhead = 3 * BAR + 50;
        let take = take_of(&phrase, playhead, 99, &ticks, &meter()).unwrap();
        let starts: Vec<_> = take.notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, [Ticks(playhead), Ticks(playhead + 2 * PPQ)]);
        // The latency is not taken off a stopped phrase; the clip begins on
        // the bar line at or before the playhead.
        assert_eq!((take.start, take.end), (Ticks(3 * BAR), Ticks(4 * BAR)));
    }

    #[test]
    fn a_note_held_at_the_end_ends_with_the_phrase_and_a_phrase_without_notes_is_nothing() {
        let phrase = Phrase {
            id: 1,
            track: 1,
            state: Heard::Playing,
            messages: vec![
                heard(1, Heard::Playing, 100, 1, on(60)),
                heard(1, Heard::Playing, 300, 2, on(64)),
            ],
        };
        let take = take_of(&phrase, 0, 0, &ticks, &meter()).unwrap();
        assert_eq!(take.notes.len(), 2);
        assert!(take.notes.iter().all(|n| n.length >= Ticks(1)));
        let none = Phrase {
            id: 1,
            track: 1,
            state: Heard::Playing,
            messages: vec![heard(
                1,
                Heard::Playing,
                100,
                1,
                Msg::controller(0, 64, 127),
            )],
        };
        assert!(take_of(&none, 0, 0, &ticks, &meter()).is_none());
    }
}

#[cfg(test)]
mod remove_tests {
    use lumen_core::midi::Msg;

    use super::*;

    #[test]
    fn a_captured_phrase_is_forgotten_and_the_one_before_is_next() {
        let mut played = Played::default();
        let now = Instant::now();
        let note = |state, at, frame| Captured {
            track: 1,
            source: 0,
            at,
            pass: 0,
            frame,
            state,
            msg: Msg::note_on(0, 60, 90),
        };
        played.push(&note(Heard::Stopped, 0, 1), now);
        played.push(&note(Heard::Playing, 500, 2), now);
        let newest = played.newest(1).unwrap();
        assert_eq!(newest.state, Heard::Playing);
        played.remove(&newest);
        assert_eq!(played.newest(1).unwrap().state, Heard::Stopped);
    }
}
