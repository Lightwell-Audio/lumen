//! Recording MIDI, the pure half (M5 slice 6c): turns the live messages a
//! recording track takes (`lumen_engine::Captured`) into the notes and events
//! of a take (`lumen_project::edit::record::Take`), in absolute project ticks.
//!
//! The app pushes what the engine queues each frame, takes each track's take
//! about once a second and places it with `Document::record_take`. Notes that
//! are still held are not in a take until [`Capture::close_all`] ends them at
//! the stop.
//!
//! # What is kept
//!
//! - Notes are paired by channel and key, as the player's notes are. The input
//!   a message came from is not part of the key, so two inputs holding one key
//!   on one channel at once pair as one note.
//! - A clip has no channels: playback sends every message on channel 0
//!   (`midi_play.rs`), so a take drops the channel of each message.
//! - Controllers, pitch bend and channel pressure become the clip events that
//!   playback sends for them, exactly.
//! - A program change becomes a program with no bank. A bank select that came
//!   before it is already a controller event (CC 0 and CC 32), so the bank
//!   still plays back as it was played.
//! - Polyphonic key pressure and system messages have no place in a clip and
//!   are dropped.
//! - With Loop on, every pass through the loop layers into one take. A note
//!   held across the loop's end is cut there and goes on at the loop's start,
//!   as many times as it is held across it (the engine numbers the passes,
//!   `Captured::pass`).
//! - The take's range is only a visible window. Notes and events outside it
//!   keep their original times and lengths in the underlying clip.

use std::collections::BTreeMap;

use lumen_core::midi::Kind;
use lumen_core::time::Ticks;
use lumen_engine::{Captured, WRAP};
use lumen_project::edit::record::{Take, TakeEvent, TakeNote};
use lumen_project::model::EventKind;

/// A note that is sounding: when it began and how hard it was struck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held {
    start: Ticks,
    velocity: u8,
    /// The loop pass it began in.
    pass: u32,
}

/// One track's part of a capture.
#[derive(Debug, Default)]
struct Lane {
    /// Notes that have ended, in the order they ended.
    notes: Vec<TakeNote>,
    /// Events, in the order they arrived.
    events: Vec<TakeEvent>,
    /// Notes still sounding, by (channel, key).
    held: BTreeMap<(u8, u8), Held>,
    /// The newest loop pass this track has seen.
    pass: u32,
}

impl Lane {
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.events.is_empty() && self.held.is_empty()
    }

    /// Notes that `pass` is newer than the newest seen.
    fn see(&mut self, pass: u32) {
        if pass.wrapping_sub(self.pass) as i32 > 0 {
            self.pass = pass;
        }
    }

    /// Ends a note at `end`, in loop pass `pass`. A note is at least one tick
    /// long. A note that began in an earlier pass runs to the loop's end,
    /// fills the loop for each pass between, and goes on from its start.
    fn finish(
        &mut self,
        pitch: u8,
        open: Held,
        (end, pass): (Ticks, u32),
        looped: Option<(Ticks, Ticks)>,
    ) {
        let across = pass.wrapping_sub(open.pass) as i32;
        let Some((from, to)) = looped.filter(|_| across > 0) else {
            self.note(pitch, open.start, end, open.velocity);
            return;
        };
        self.note(pitch, open.start, to, open.velocity);
        if across > 1 {
            self.note(pitch, from, to, open.velocity);
        }
        if end > from {
            self.note(pitch, from, end, open.velocity);
        }
    }

    fn note(&mut self, pitch: u8, start: Ticks, end: Ticks, velocity: u8) {
        self.notes.push(TakeNote {
            start,
            length: (end - start).max(Ticks(1)),
            pitch,
            velocity,
        });
    }
}

/// What the recording has captured so far, per track.
#[derive(Debug, Default)]
pub struct Capture {
    lanes: BTreeMap<u64, Lane>,
    /// The loop region, when the take layers its passes.
    looped: Option<(Ticks, Ticks)>,
    /// The actual Record boundary, independent of the visible loop/punch
    /// window. Only its first pass is floored here after latency correction.
    start: Option<Ticks>,
    first_pass: Option<u32>,
}

impl Capture {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// A capture of a take that loops over `looped` (start and end ticks),
    /// if it does.
    #[cfg(test)]
    pub fn looping(looped: Option<(Ticks, Ticks)>) -> Self {
        Self {
            looped,
            ..Self::default()
        }
    }

    /// A recording that begins at `start`. Compensation may place an
    /// already-held key or the first live event just before that boundary;
    /// it belongs at the start, without clamping later loop passes or the
    /// independently positioned punch window.
    pub fn recording(looped: Option<(Ticks, Ticks)>, start: Ticks) -> Self {
        Self {
            looped,
            start: Some(start),
            ..Self::default()
        }
    }

    /// Keeps `c`. `latency` samples (the output latency) are taken off its
    /// time first, never below 0; `ticks` turns a timeline sample into project
    /// ticks.
    ///
    /// A note-on for a key that is already held closes the held note at this
    /// moment and opens a new one. A note-off with nothing open for its channel
    /// and key is ignored. A note-on with velocity 0 is a note-off, as
    /// `Msg::kind` reads it.
    pub fn push(&mut self, c: &Captured, latency: i64, ticks: &dyn Fn(i64) -> Ticks) {
        // A wrap can arrive before the first played message, for a silent
        // first pass. Keep its number so late messages from it use the same
        // floor; pass counters may wrap around u32::MAX.
        let first_pass = *self
            .first_pass
            .get_or_insert(c.pass.wrapping_sub(u32::from(c.source == WRAP)));
        if c.source == WRAP {
            self.lanes.entry(c.track).or_default().see(c.pass);
            return;
        }
        let kind = c.msg.kind();
        if matches!(kind, Kind::KeyPressure { .. } | Kind::Other) {
            return;
        }
        let mut at = ticks((c.at - latency).max(0));
        if c.pass == first_pass
            && let Some(start) = self.start
        {
            at = at.max(start);
        }
        let channel = c.msg.channel();
        let looped = self.looped;
        let lane = self.lanes.entry(c.track).or_default();
        lane.see(c.pass);
        match kind {
            Kind::NoteOn { key, velocity } => {
                let open = Held {
                    start: at,
                    velocity,
                    pass: c.pass,
                };
                if let Some(previous) = lane.held.insert((channel, key), open) {
                    lane.finish(key, previous, (at, c.pass), looped);
                }
            }
            Kind::NoteOff { key, .. } => {
                if let Some(open) = lane.held.remove(&(channel, key)) {
                    lane.finish(key, open, (at, c.pass), looped);
                }
            }
            Kind::Controller { number, value } => lane.events.push(TakeEvent {
                at,
                kind: EventKind::Controller { number, value },
            }),
            Kind::PitchBend { value } => lane.events.push(TakeEvent {
                at,
                kind: EventKind::PitchBend { value },
            }),
            Kind::Pressure { value } => lane.events.push(TakeEvent {
                at,
                kind: EventKind::Pressure { value },
            }),
            Kind::Program { program } => lane.events.push(TakeEvent {
                at,
                kind: EventKind::Program {
                    program,
                    bank: None,
                },
            }),
            Kind::KeyPressure { .. } | Kind::Other => {}
        }
    }

    /// The take so far for `track` over `range` (start and end ticks): the
    /// notes that have ended and every event, as [`Capture::push`] kept them.
    /// Notes still held are not in it. Notes are sorted by start, then pitch.
    /// The range is carried, not applied: placing the take uses it as a
    /// visible window over the full performance. A track with nothing yet
    /// gives an empty take with that range.
    pub fn take(&self, track: u64, range: (Ticks, Ticks)) -> Take {
        self.snapshot(track, range, None)
    }

    /// The take as [`Capture::take`] has it, with the notes still held in it
    /// as if they ended at `now`: what the arrangement draws as it grows.
    pub fn so_far(&self, track: u64, range: (Ticks, Ticks), now: Ticks) -> Take {
        self.snapshot(track, range, Some(now))
    }

    fn snapshot(&self, track: u64, range: (Ticks, Ticks), now: Option<Ticks>) -> Take {
        let (mut notes, events) = match self.lanes.get(&track) {
            Some(lane) => (lane.notes.clone(), lane.events.clone()),
            None => (Vec::new(), Vec::new()),
        };
        if let (Some(now), Some(lane)) = (now, self.lanes.get(&track)) {
            let mut open = Lane::default();
            for (&(_, pitch), &held) in &lane.held {
                open.finish(pitch, held, (now, lane.pass), self.looped);
            }
            notes.extend(open.notes);
        }
        notes.sort_by_key(|n| (n.start, n.pitch));
        Take {
            start: range.0,
            end: range.1,
            notes,
            events,
        }
    }

    /// Ends every held note at `end` (the stop), in the pass each track has
    /// reached, so [`Capture::take`] has them.
    pub fn close_all(&mut self, end: Ticks) {
        let looped = self.looped;
        for lane in self.lanes.values_mut() {
            let pass = lane.pass;
            for ((_, pitch), open) in std::mem::take(&mut lane.held) {
                lane.finish(pitch, open, (end, pass), looped);
            }
        }
    }

    #[cfg(test)]
    /// The tracks that have anything (finished notes, held notes or events),
    /// sorted.
    pub fn tracks(&self) -> Vec<u64> {
        self.lanes
            .iter()
            .filter(|(_, lane)| !lane.is_empty())
            .map(|(&track, _)| track)
            .collect()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.lanes.values().all(Lane::is_empty)
    }

    #[cfg(test)]
    /// The notes held now, across all tracks.
    pub fn held(&self) -> usize {
        self.lanes.values().map(|lane| lane.held.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::midi::Msg;

    use super::*;

    fn cap(track: u64, at: i64, msg: Msg) -> Captured {
        Captured {
            track,
            source: 0,
            at,
            pass: 0,
            msg,
        }
    }

    fn in_pass(pass: u32, mut c: Captured) -> Captured {
        c.pass = pass;
        c
    }

    fn wrap(track: u64, pass: u32) -> Captured {
        Captured {
            track,
            source: WRAP,
            at: 0,
            pass,
            msg: Msg([0, 0, 0]),
        }
    }

    /// One sample is ten ticks.
    fn ticks(s: i64) -> Ticks {
        Ticks(s * 10)
    }

    fn note(start: i64, length: i64, pitch: u8, velocity: u8) -> TakeNote {
        TakeNote {
            start: Ticks(start),
            length: Ticks(length),
            pitch,
            velocity,
        }
    }

    fn range() -> (Ticks, Ticks) {
        (Ticks(0), Ticks(100_000))
    }

    #[test]
    fn a_note_on_and_its_off_make_one_note() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 250, Msg::note_off(0, 60)), 0, &ticks);
        assert_eq!(c.take(1, range()).notes, vec![note(1000, 1500, 60, 90)]);
        assert_eq!(c.held(), 0);
    }

    #[test]
    fn tracks_keep_their_notes_apart() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(2, 110, Msg::note_on(0, 60, 80)), 0, &ticks);
        c.push(&cap(1, 200, Msg::note_off(0, 60)), 0, &ticks);
        assert_eq!(c.take(1, range()).notes, vec![note(1000, 1000, 60, 90)]);
        assert!(c.take(2, range()).notes.is_empty(), "track 2 is still held");
        assert_eq!(c.held(), 1);
        assert_eq!(c.tracks(), vec![1, 2]);
    }

    #[test]
    fn the_same_key_twice_closes_the_first_note_and_opens_the_next() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 200, Msg::note_on(0, 60, 70)), 0, &ticks);
        assert_eq!(c.held(), 1, "never two open on one key");
        c.push(&cap(1, 300, Msg::note_off(0, 60)), 0, &ticks);
        assert_eq!(
            c.take(1, range()).notes,
            vec![note(1000, 1000, 60, 90), note(2000, 1000, 60, 70)]
        );
        assert_eq!(c.held(), 0);
    }

    #[test]
    fn a_note_off_with_nothing_open_is_ignored() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_off(0, 60)), 0, &ticks);
        assert!(c.is_empty());
        assert!(c.tracks().is_empty());
        assert!(c.take(1, range()).notes.is_empty());
    }

    #[test]
    fn notes_pair_by_channel_and_key() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_on(3, 60, 90)), 0, &ticks);
        c.push(&cap(1, 150, Msg::note_off(4, 60)), 0, &ticks);
        assert_eq!(c.held(), 1, "another channel's off does not close it");
        c.push(&cap(1, 200, Msg::note_off(3, 60)), 0, &ticks);
        assert_eq!(c.take(1, range()).notes, vec![note(1000, 1000, 60, 90)]);
    }

    #[test]
    fn held_notes_are_left_out_until_close_all_ends_them() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 150, Msg::note_on(0, 64, 90)), 0, &ticks);
        c.push(&cap(1, 160, Msg::note_off(0, 64)), 0, &ticks);
        assert_eq!(c.take(1, range()).notes, vec![note(1500, 100, 64, 90)]);
        assert_eq!(c.held(), 1);
        c.close_all(ticks(900));
        assert_eq!(c.held(), 0);
        assert_eq!(
            c.take(1, range()).notes,
            vec![note(1000, 8000, 60, 90), note(1500, 100, 64, 90)]
        );
    }

    #[test]
    fn a_note_lasts_at_least_one_tick() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 100, Msg::note_off(0, 60)), 0, &ticks);
        c.push(&cap(1, 200, Msg::note_on(0, 62, 90)), 0, &ticks);
        c.push(&cap(1, 190, Msg::note_off(0, 62)), 0, &ticks);
        let notes = c.take(1, range()).notes;
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|n| n.length == Ticks(1)));
    }

    #[test]
    fn a_zero_velocity_note_on_is_a_note_off() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_on(0, 60, 100)), 0, &ticks);
        c.push(&cap(1, 200, Msg([0x90, 60, 0])), 0, &ticks);
        assert_eq!(c.take(1, range()).notes, vec![note(1000, 1000, 60, 100)]);
        assert_eq!(c.held(), 0);
    }

    #[test]
    fn each_event_kind_maps_to_the_clip_event_playback_sends() {
        let mut c = Capture::new();
        c.push(&cap(1, 10, Msg::controller(0, 7, 100)), 0, &ticks);
        c.push(&cap(1, 20, Msg::pitch_bend(0, -300)), 0, &ticks);
        c.push(&cap(1, 30, Msg::pressure(0, 33)), 0, &ticks);
        c.push(&cap(1, 40, Msg::program(0, 5)), 0, &ticks);
        assert_eq!(
            c.take(1, range()).events,
            vec![
                TakeEvent {
                    at: Ticks(100),
                    kind: EventKind::Controller {
                        number: 7,
                        value: 100
                    },
                },
                TakeEvent {
                    at: Ticks(200),
                    kind: EventKind::PitchBend { value: -300 },
                },
                TakeEvent {
                    at: Ticks(300),
                    kind: EventKind::Pressure { value: 33 },
                },
                TakeEvent {
                    at: Ticks(400),
                    kind: EventKind::Program {
                        program: 5,
                        bank: None
                    },
                },
            ]
        );
    }

    #[test]
    fn the_channel_of_an_event_is_dropped() {
        let mut c = Capture::new();
        c.push(&cap(1, 10, Msg::controller(9, 7, 100)), 0, &ticks);
        c.push(&cap(1, 20, Msg::program(5, 1)), 0, &ticks);
        assert_eq!(
            c.take(1, range()).events,
            vec![
                TakeEvent {
                    at: Ticks(100),
                    kind: EventKind::Controller {
                        number: 7,
                        value: 100
                    },
                },
                TakeEvent {
                    at: Ticks(200),
                    kind: EventKind::Program {
                        program: 1,
                        bank: None
                    },
                },
            ]
        );
    }

    #[test]
    fn key_pressure_and_system_messages_are_dropped() {
        let mut c = Capture::new();
        c.push(&cap(1, 10, Msg([0xA0, 60, 50])), 0, &ticks);
        c.push(&cap(1, 20, Msg([0xF8, 0, 0])), 0, &ticks);
        assert!(c.is_empty());
        assert!(c.tracks().is_empty());
    }

    #[test]
    fn events_keep_their_arrival_order() {
        let mut c = Capture::new();
        c.push(&cap(1, 50, Msg::controller(0, 1, 1)), 0, &ticks);
        c.push(&cap(1, 10, Msg::controller(0, 1, 2)), 0, &ticks);
        c.push(&cap(1, 30, Msg::pressure(0, 3)), 0, &ticks);
        let at: Vec<(Ticks, EventKind)> = c
            .take(1, range())
            .events
            .into_iter()
            .map(|e| (e.at, e.kind))
            .collect();
        assert_eq!(
            at,
            vec![
                (
                    Ticks(500),
                    EventKind::Controller {
                        number: 1,
                        value: 1
                    }
                ),
                (
                    Ticks(100),
                    EventKind::Controller {
                        number: 1,
                        value: 2
                    }
                ),
                (Ticks(300), EventKind::Pressure { value: 3 }),
            ]
        );
    }

    #[test]
    fn notes_are_sorted_by_start_then_pitch() {
        let mut c = Capture::new();
        c.push(&cap(1, 100, Msg::note_on(0, 67, 100)), 0, &ticks);
        c.push(&cap(1, 200, Msg::note_on(0, 60, 100)), 0, &ticks);
        c.push(&cap(1, 210, Msg::note_off(0, 60)), 0, &ticks);
        c.push(&cap(1, 300, Msg::note_off(0, 67)), 0, &ticks);
        c.push(&cap(1, 500, Msg::note_on(0, 64, 100)), 0, &ticks);
        c.push(&cap(1, 500, Msg::note_on(0, 60, 100)), 0, &ticks);
        c.push(&cap(1, 510, Msg::note_off(0, 64)), 0, &ticks);
        c.push(&cap(1, 520, Msg::note_off(0, 60)), 0, &ticks);
        assert_eq!(
            c.take(1, range()).notes,
            vec![
                note(1000, 2000, 67, 100),
                note(2000, 100, 60, 100),
                note(5000, 200, 60, 100),
                note(5000, 100, 64, 100),
            ]
        );
    }

    #[test]
    fn latency_comes_off_each_time_and_never_goes_below_zero() {
        let mut c = Capture::new();
        c.push(&cap(1, 1000, Msg::note_on(0, 60, 100)), 300, &ticks);
        c.push(&cap(1, 1500, Msg::note_off(0, 60)), 300, &ticks);
        assert_eq!(c.take(1, range()).notes, vec![note(7000, 5000, 60, 100)]);
        // Earlier than the latency: the time is 0, not negative.
        c.push(&cap(1, 100, Msg::controller(0, 7, 1)), 300, &ticks);
        assert_eq!(c.take(1, range()).events[0].at, Ticks(0));
    }

    #[test]
    fn a_take_carries_its_range_and_does_not_cut_it() {
        let mut c = Capture::new();
        c.push(&cap(1, 10, Msg::note_on(0, 60, 100)), 0, &ticks);
        c.push(&cap(1, 20, Msg::note_off(0, 60)), 0, &ticks);
        let t = c.take(1, (Ticks(500), Ticks(900)));
        assert_eq!((t.start, t.end), (Ticks(500), Ticks(900)));
        assert_eq!(t.notes, vec![note(100, 100, 60, 100)]);
    }

    #[test]
    fn an_empty_take_carries_its_range() {
        assert_eq!(
            Capture::new().take(7, (Ticks(5), Ticks(9))),
            Take {
                start: Ticks(5),
                end: Ticks(9),
                notes: Vec::new(),
                events: Vec::new(),
            }
        );
    }

    #[test]
    fn tracks_lists_only_tracks_with_something_in_order() {
        let mut c = Capture::new();
        c.push(&cap(9, 10, Msg::controller(0, 7, 1)), 0, &ticks);
        c.push(&cap(3, 10, Msg::note_on(0, 60, 100)), 0, &ticks);
        c.push(&cap(5, 10, Msg::note_off(0, 60)), 0, &ticks);
        assert_eq!(c.tracks(), vec![3, 9]);
        assert!(!c.is_empty());
    }

    /// The loop is ticks 0 to 1,000 (samples 0 to 100).
    fn looped() -> Capture {
        Capture::looping(Some((Ticks(0), Ticks(1_000))))
    }

    #[test]
    fn a_note_held_across_the_loop_end_is_cut_there_and_goes_on_from_the_start() {
        let mut c = looped();
        c.push(&in_pass(0, cap(1, 95, Msg::note_on(0, 60, 90))), 0, &ticks);
        c.push(&wrap(1, 1), 0, &ticks);
        c.push(&in_pass(1, cap(1, 20, Msg::note_off(0, 60))), 0, &ticks);
        assert_eq!(
            c.take(1, (Ticks(0), Ticks(1_000))).notes,
            vec![note(0, 200, 60, 90), note(950, 50, 60, 90)]
        );
        assert_eq!(c.held(), 0);
    }

    #[test]
    fn a_note_held_across_several_passes_fills_the_loop_between() {
        let mut c = looped();
        c.push(&in_pass(0, cap(1, 50, Msg::note_on(0, 60, 90))), 0, &ticks);
        c.push(&wrap(1, 1), 0, &ticks);
        c.push(&wrap(1, 2), 0, &ticks);
        c.push(&in_pass(2, cap(1, 30, Msg::note_off(0, 60))), 0, &ticks);
        assert_eq!(
            c.take(1, (Ticks(0), Ticks(1_000))).notes,
            vec![
                note(0, 1_000, 60, 90),
                note(0, 300, 60, 90),
                note(500, 500, 60, 90)
            ]
        );
    }

    #[test]
    fn a_note_held_at_the_stop_in_a_later_pass_ends_at_the_stop() {
        let mut c = looped();
        c.push(&in_pass(0, cap(1, 90, Msg::note_on(0, 62, 80))), 0, &ticks);
        // The marker is the only thing that tells the track the pass.
        c.push(&wrap(1, 1), 0, &ticks);
        c.close_all(ticks(40));
        assert_eq!(
            c.take(1, (Ticks(0), Ticks(1_000))).notes,
            vec![note(0, 400, 62, 80), note(900, 100, 62, 80)]
        );
    }

    #[test]
    fn a_note_played_before_a_wrap_and_let_go_before_it_stays_in_its_pass() {
        let mut c = looped();
        // The first pass's note-off comes after the marker in the queue.
        c.push(&in_pass(0, cap(1, 90, Msg::note_on(0, 60, 90))), 0, &ticks);
        c.push(&wrap(1, 1), 0, &ticks);
        c.push(&in_pass(0, cap(1, 99, Msg::note_off(0, 60))), 0, &ticks);
        c.push(&in_pass(0, cap(1, 99, Msg::note_on(0, 64, 90))), 0, &ticks);
        c.push(&in_pass(1, cap(1, 10, Msg::note_off(0, 64))), 0, &ticks);
        assert_eq!(
            c.take(1, (Ticks(0), Ticks(1_000))).notes,
            vec![
                note(0, 100, 64, 90),
                note(900, 90, 60, 90),
                note(990, 10, 64, 90)
            ]
        );
    }

    #[test]
    fn without_a_loop_passes_do_not_cut_notes() {
        let mut c = Capture::new();
        c.push(&in_pass(0, cap(1, 50, Msg::note_on(0, 60, 90))), 0, &ticks);
        c.push(&in_pass(1, cap(1, 80, Msg::note_off(0, 60))), 0, &ticks);
        assert_eq!(c.take(1, range()).notes, vec![note(500, 300, 60, 90)]);
    }

    #[test]
    fn a_wrap_marker_keeps_no_note_and_no_event() {
        let mut c = looped();
        c.push(&wrap(1, 1), 0, &ticks);
        assert!(c.is_empty());
        assert!(c.tracks().is_empty());
    }

    #[test]
    fn so_far_shows_the_held_notes_ending_now_and_keeps_them_held() {
        let mut c = Capture::new();
        c.push(&cap(1, 10, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 20, Msg::note_on(0, 64, 90)), 0, &ticks);
        c.push(&cap(1, 30, Msg::note_off(0, 64)), 0, &ticks);
        let shown = c.so_far(1, range(), ticks(50));
        assert_eq!(
            shown.notes,
            vec![note(100, 400, 60, 90), note(200, 100, 64, 90)]
        );
        assert_eq!(c.take(1, range()).notes, vec![note(200, 100, 64, 90)]);
        assert_eq!(c.held(), 1);
    }

    #[test]
    fn a_note_that_began_before_the_range_keeps_its_full_timing() {
        let mut c = Capture::new();
        // Held when recording began at tick 500 (the latency put it at 400).
        c.push(&cap(1, 40, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 70, Msg::note_off(0, 60)), 0, &ticks);
        // And one that ended before the range: kept as hidden content.
        c.push(&cap(1, 10, Msg::note_on(0, 62, 90)), 0, &ticks);
        c.push(&cap(1, 20, Msg::note_off(0, 62)), 0, &ticks);
        let t = c.take(1, (Ticks(500), Ticks(900)));
        assert_eq!(
            t.notes,
            vec![note(100, 100, 62, 90), note(400, 300, 60, 90)]
        );
    }

    #[test]
    fn latency_cannot_hide_the_first_note_before_the_record_boundary() {
        let mut c = Capture::recording(None, Ticks(1_000));
        c.push(&cap(1, 1_100, Msg::note_on(0, 60, 90)), 200, &Ticks);
        c.push(&cap(1, 1_100, Msg::controller(0, 1, 64)), 200, &Ticks);
        let preview = c.so_far(1, (Ticks(1_000), Ticks(2_000)), Ticks(1_250));
        assert_eq!(preview.notes, vec![note(1_000, 250, 60, 90)]);
        c.push(&cap(1, 1_500, Msg::note_off(0, 60)), 200, &Ticks);
        // The punch window starts later than Record. This independent
        // boundary must still leave the pre-punch note hidden and whole.
        let take = c.take(1, (Ticks(1_200), Ticks(2_000)));
        assert_eq!(take.notes, vec![note(1_000, 300, 60, 90)]);
        assert_eq!(take.events[0].at, Ticks(1_000));
    }

    #[test]
    fn later_loop_passes_can_begin_before_the_original_record_start() {
        let mut c = Capture::recording(Some((Ticks(800), Ticks(1_600))), Ticks(1_200));
        c.push(
            &in_pass(8, cap(1, 1_200, Msg::note_on(0, 60, 90))),
            100,
            &Ticks,
        );
        c.push(
            &in_pass(8, cap(1, 1_400, Msg::note_off(0, 60))),
            100,
            &Ticks,
        );
        c.push(&wrap(1, 9), 100, &Ticks);
        c.push(
            &in_pass(9, cap(1, 1_000, Msg::note_on(0, 64, 80))),
            100,
            &Ticks,
        );
        c.push(
            &in_pass(9, cap(1, 1_100, Msg::note_off(0, 64))),
            100,
            &Ticks,
        );
        assert_eq!(
            c.take(1, (Ticks(800), Ticks(1_600))).notes,
            vec![note(900, 100, 64, 80), note(1_200, 100, 60, 90)]
        );
    }

    #[test]
    fn a_wrap_before_the_first_message_still_identifies_the_initial_pass() {
        for first in [8_u32, u32::MAX] {
            let next = first.wrapping_add(1);
            let mut c = Capture::recording(Some((Ticks(800), Ticks(1_600))), Ticks(1_200));
            c.push(&wrap(1, next), 100, &Ticks);
            // A delayed message from the otherwise silent first pass.
            c.push(
                &in_pass(first, cap(1, 1_200, Msg::note_on(0, 60, 90))),
                100,
                &Ticks,
            );
            c.push(
                &in_pass(first, cap(1, 1_400, Msg::note_off(0, 60))),
                100,
                &Ticks,
            );
            c.push(
                &in_pass(next, cap(1, 1_000, Msg::note_on(0, 64, 80))),
                100,
                &Ticks,
            );
            c.push(
                &in_pass(next, cap(1, 1_100, Msg::note_off(0, 64))),
                100,
                &Ticks,
            );
            assert_eq!(
                c.take(1, (Ticks(800), Ticks(1_600))).notes,
                vec![note(900, 100, 64, 80), note(1_200, 100, 60, 90)]
            );
        }
    }
}
