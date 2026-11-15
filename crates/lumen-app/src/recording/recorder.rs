use std::collections::BTreeMap;

use lumen_core::midi::Kind;
use lumen_core::time::Ticks;
use lumen_engine::{Captured, WRAP, record_position};
use lumen_project::edit::record::{PassTake, Take, TakeEvent, TakeNote};
use lumen_project::model::EventKind;

const MOST_PASSES_HELD: i32 = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held {
    start: Ticks,
    velocity: u8,
    pass: u32,
}

#[derive(Debug, Clone, Copy)]
struct Piece {
    pass: u32,
    note: TakeNote,
    extra: bool,
}

#[derive(Debug, Default)]
struct Lane {
    notes: Vec<Piece>,
    events: Vec<(u32, TakeEvent)>,
    held: BTreeMap<(u8, u8), Held>,
    pass: u32,
}

impl Lane {
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.events.is_empty() && self.held.is_empty()
    }

    fn see(&mut self, pass: u32) {
        if pass.wrapping_sub(self.pass) as i32 > 0 {
            self.pass = pass;
        }
    }

    fn finish(
        &mut self,
        pitch: u8,
        open: Held,
        (end, pass): (Ticks, u32),
        looped: Option<(Ticks, Ticks)>,
    ) {
        let across = pass.wrapping_sub(open.pass) as i32;
        let Some((from, to)) = looped.filter(|_| across > 0) else {
            self.note(open.pass, pitch, (open.start, end), open.velocity, false);
            return;
        };
        self.note(open.pass, pitch, (open.start, to), open.velocity, false);
        for between in 1..across.min(MOST_PASSES_HELD) {
            let layered = between == 1;
            self.note(
                open.pass.wrapping_add(between as u32),
                pitch,
                (from, to),
                open.velocity,
                !layered,
            );
        }
        if end > from {
            self.note(pass, pitch, (from, end), open.velocity, false);
        }
    }

    fn event(&mut self, pass: u32, at: Ticks, kind: EventKind) {
        self.events.push((pass, TakeEvent { at, kind }));
    }

    fn note(
        &mut self,
        pass: u32,
        pitch: u8,
        (start, end): (Ticks, Ticks),
        velocity: u8,
        extra: bool,
    ) {
        self.notes.push(Piece {
            pass,
            note: TakeNote {
                start,
                length: (end - start).max(Ticks(1)),
                pitch,
                velocity,
            },
            extra,
        });
    }
}

#[derive(Debug, Default)]
pub struct Capture {
    lanes: BTreeMap<u64, Lane>,
    looped: Option<(Ticks, Ticks)>,
    start: Option<Ticks>,
    first_pass: Option<u32>,
    store: BTreeMap<u64, [u8; 128]>,
}

impl Capture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_store_map(&mut self, track: u64, map: Option<&lumen_project::model::DrumMap>) {
        match map {
            Some(map) => {
                self.store
                    .insert(track, lumen_project::drums::store_table(map));
            }
            None => {
                self.store.remove(&track);
            }
        }
    }

    #[cfg(test)]
    pub fn looping(looped: Option<(Ticks, Ticks)>) -> Self {
        Self {
            looped,
            ..Self::default()
        }
    }

    pub fn recording(looped: Option<(Ticks, Ticks)>, start: Ticks) -> Self {
        Self {
            looped,
            start: Some(start),
            ..Self::default()
        }
    }

    pub fn push(&mut self, c: &Captured, latency: i64, ticks: &dyn Fn(i64) -> Ticks) {
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
        let heard = ticks(c.at.max(0));
        let compensated = ticks((c.at - latency).max(0));
        let mut pass = c.pass;
        let (at, mapped_pass) = record_position(
            heard,
            compensated,
            pass,
            first_pass,
            self.start.unwrap_or(Ticks(i64::MIN)),
            self.looped,
        );
        pass = mapped_pass;
        let channel = c.msg.channel();
        let looped = self.looped;
        let stored = |key: u8| {
            self.store
                .get(&c.track)
                .map_or(key, |table| table[usize::from(key)])
        };
        let lane = self.lanes.entry(c.track).or_default();
        lane.see(pass);
        match kind {
            Kind::NoteOn { key, velocity } => {
                let key = stored(key);
                let open = Held {
                    start: at,
                    velocity,
                    pass,
                };
                if let Some(previous) = lane.held.insert((channel, key), open) {
                    lane.finish(key, previous, (at, pass), looped);
                }
            }
            Kind::NoteOff { key, .. } => {
                let key = stored(key);
                if let Some(open) = lane.held.remove(&(channel, key)) {
                    lane.finish(key, open, (at, pass), looped);
                }
            }
            Kind::Controller { number, value } => {
                lane.event(pass, at, EventKind::Controller { number, value });
            }
            Kind::PitchBend { value } => lane.event(pass, at, EventKind::PitchBend { value }),
            Kind::Pressure { value } => lane.event(pass, at, EventKind::Pressure { value }),
            Kind::Program { program } => lane.event(
                pass,
                at,
                EventKind::Program {
                    program,
                    bank: None,
                },
            ),
            Kind::KeyPressure { .. } | Kind::Other => {}
        }
    }

    pub fn take(&self, track: u64, range: (Ticks, Ticks)) -> Take {
        self.snapshot(track, range, None)
    }

    pub fn so_far(&self, track: u64, range: (Ticks, Ticks), now: Ticks) -> Take {
        self.snapshot(track, range, Some(now))
    }

    fn snapshot(&self, track: u64, range: (Ticks, Ticks), now: Option<Ticks>) -> Take {
        let mut pieces = Vec::new();
        let mut events = Vec::new();
        let mut newest = 0;
        if let Some(lane) = self.lanes.get(&track) {
            pieces.clone_from(&lane.notes);
            events.clone_from(&lane.events);
            newest = lane.pass;
            if let Some(now) = now {
                let mut open = Lane::default();
                for (&(_, pitch), &held) in &lane.held {
                    open.finish(pitch, held, (now, lane.pass), self.looped);
                }
                pieces.extend(open.notes);
            }
        }
        let passes = match (self.looped, self.first_pass) {
            (Some(_), Some(first)) if newest.wrapping_sub(first) as i32 > 0 => {
                let count = newest.wrapping_sub(first) as usize + 1;
                let mut passes = vec![PassTake::default(); count.min(MOST_PASSES_HELD as usize)];
                let slot = |pass: u32| {
                    let n = pass.wrapping_sub(first) as i32;
                    (n.max(0) as usize).min(passes.len() - 1)
                };
                let places: Vec<_> = pieces.iter().map(|p| slot(p.pass)).collect();
                let event_places: Vec<_> = events.iter().map(|(p, _)| slot(*p)).collect();
                for (piece, n) in pieces.iter().zip(places) {
                    passes[n].notes.push(piece.note);
                }
                for ((_, event), n) in events.iter().zip(event_places) {
                    passes[n].events.push(*event);
                }
                for pass in &mut passes {
                    pass.notes.sort_by_key(|n| (n.start, n.pitch));
                }
                passes
            }
            _ => Vec::new(),
        };
        let mut notes: Vec<TakeNote> = pieces.iter().filter(|p| !p.extra).map(|p| p.note).collect();
        notes.sort_by_key(|n| (n.start, n.pitch));
        Take {
            start: range.0,
            end: range.1,
            notes,
            events: events.into_iter().map(|(_, e)| e).collect(),
            passes,
        }
    }

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
    pub fn held(&self) -> usize {
        self.lanes.values().map(|lane| lane.held.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::midi::Msg;
    use lumen_engine::Heard;

    use super::*;

    fn cap(track: u64, at: i64, msg: Msg) -> Captured {
        Captured {
            track,
            source: 0,
            at,
            pass: 0,
            frame: 0,
            state: Heard::Recording,
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
            frame: 0,
            state: Heard::Recording,
            msg: Msg([0, 0, 0]),
        }
    }

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
    fn a_mapped_track_keeps_the_input_note_of_the_note_a_sound_plays() {
        use lumen_project::model::{DrumIcon, DrumMap, Id, Sound};

        let map = DrumMap::new(
            Id(1),
            "Kit",
            vec![Sound::new("Kick", 36, 40, DrumIcon::Kick)],
        );
        let mut c = Capture::new();
        c.set_store_map(1, Some(&map));
        c.push(&cap(1, 100, Msg::note_on(0, 40, 90)), 0, &ticks);
        c.push(&cap(1, 200, Msg::note_off(0, 40)), 0, &ticks);
        c.push(&cap(1, 300, Msg::note_on(0, 60, 80)), 0, &ticks);
        c.push(&cap(1, 400, Msg::note_off(0, 60)), 0, &ticks);
        assert_eq!(
            c.take(1, range()).notes,
            vec![note(1000, 1000, 36, 90), note(3000, 1000, 60, 80)]
        );
        c.push(&cap(2, 500, Msg::note_on(0, 40, 70)), 0, &ticks);
        c.push(&cap(2, 600, Msg::note_off(0, 40)), 0, &ticks);
        assert_eq!(c.take(2, range()).notes, vec![note(5000, 1000, 40, 70)]);
        c.set_store_map(1, None);
        c.push(&cap(1, 700, Msg::note_on(0, 40, 60)), 0, &ticks);
        c.push(&cap(1, 800, Msg::note_off(0, 40)), 0, &ticks);
        assert_eq!(
            c.take(1, range()).notes.last().copied(),
            Some(note(7000, 1000, 40, 60))
        );
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
                passes: Vec::new(),
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

    fn looped() -> Capture {
        Capture::looping(Some((Ticks(0), Ticks(1_000))))
    }

    #[test]
    fn each_pass_of_a_loop_keeps_what_it_played_besides_the_layered_take() {
        let mut c = looped();
        c.push(&cap(1, 10, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 20, Msg::note_off(0, 60)), 0, &ticks);
        c.push(&wrap(1, 1), 0, &ticks);
        c.push(&in_pass(1, cap(1, 30, Msg::note_on(0, 62, 90))), 0, &ticks);
        c.push(&in_pass(1, cap(1, 35, Msg::controller(0, 1, 5))), 0, &ticks);
        c.push(&in_pass(1, cap(1, 40, Msg::note_off(0, 62))), 0, &ticks);
        c.push(&wrap(1, 2), 0, &ticks);
        let take = c.take(1, (Ticks(0), Ticks(1_000)));
        assert_eq!(take.notes.len(), 2, "layered: both notes");
        assert_eq!(take.passes.len(), 3, "the silent third pass counts");
        assert_eq!(take.passes[0].notes, vec![note(100, 100, 60, 90)]);
        assert_eq!(take.passes[1].notes, vec![note(300, 100, 62, 90)]);
        assert_eq!(take.passes[1].events.len(), 1);
        assert!(take.passes[0].events.is_empty());
        assert!(take.passes[2].notes.is_empty());
    }

    #[test]
    fn one_pass_or_no_loop_is_no_passes() {
        let mut c = looped();
        c.push(&cap(1, 10, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 20, Msg::note_off(0, 60)), 0, &ticks);
        assert!(c.take(1, (Ticks(0), Ticks(1_000))).passes.is_empty());
        let mut c = Capture::new();
        c.push(&cap(1, 10, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&wrap(1, 1), 0, &ticks);
        c.push(&in_pass(1, cap(1, 20, Msg::note_off(0, 60))), 0, &ticks);
        assert!(c.take(1, (Ticks(0), Ticks(1_000))).passes.is_empty());
    }

    #[test]
    fn a_note_held_across_wraps_is_in_every_pass_it_sounds_in() {
        let mut c = looped();
        c.push(&in_pass(0, cap(1, 50, Msg::note_on(0, 60, 90))), 0, &ticks);
        c.push(&wrap(1, 1), 0, &ticks);
        c.push(&wrap(1, 2), 0, &ticks);
        c.push(&in_pass(2, cap(1, 30, Msg::note_off(0, 60))), 0, &ticks);
        let take = c.take(1, (Ticks(0), Ticks(1_000)));
        assert_eq!(take.passes.len(), 3);
        assert_eq!(take.passes[0].notes, vec![note(500, 500, 60, 90)]);
        assert_eq!(take.passes[1].notes, vec![note(0, 1_000, 60, 90)]);
        assert_eq!(take.passes[2].notes, vec![note(0, 300, 60, 90)]);
        assert_eq!(take.notes.len(), 3, "the layered take is as it was");
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
        c.push(&cap(1, 40, Msg::note_on(0, 60, 90)), 0, &ticks);
        c.push(&cap(1, 70, Msg::note_off(0, 60)), 0, &ticks);
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
        let take = c.take(1, (Ticks(1_200), Ticks(2_000)));
        assert_eq!(take.notes, vec![note(1_000, 300, 60, 90)]);
        assert_eq!(take.events[0].at, Ticks(1_000));
    }

    #[test]
    fn compensated_positions_respect_both_punch_edges() {
        let punch = (Ticks(1_000), Ticks(1_500));
        let looped = Some((Ticks(800), Ticks(1_600)));
        let (before_in, _) = record_position(Ticks(1_010), Ticks(990), 0, 0, Ticks(600), looped);
        assert!(before_in < punch.0);
        let (before_out, _) = record_position(Ticks(1_510), Ticks(1_490), 0, 0, Ticks(600), looped);
        assert!((punch.0..punch.1).contains(&before_out));
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
    fn latency_across_a_wrap_moves_a_note_to_the_end_of_the_pass_before() {
        let mut c = Capture::recording(Some((Ticks(800), Ticks(1_600))), Ticks(800));
        c.push(&wrap(1, 1), 100, &Ticks);
        c.push(
            &in_pass(1, cap(1, 850, Msg::note_on(0, 60, 90))),
            100,
            &Ticks,
        );
        c.push(
            &in_pass(1, cap(1, 860, Msg::controller(0, 1, 7))),
            100,
            &Ticks,
        );
        c.push(&in_pass(1, cap(1, 950, Msg::note_off(0, 60))), 100, &Ticks);
        let take = c.take(1, (Ticks(800), Ticks(1_600)));
        assert_eq!(
            take.notes,
            vec![note(800, 50, 60, 90), note(1_550, 50, 60, 90)]
        );
        assert_eq!(take.events[0].at, Ticks(1_560));
    }

    #[test]
    fn pre_loop_capture_and_tails_past_the_loop_keep_their_times() {
        let mut c = Capture::recording(Some((Ticks(800), Ticks(1_600))), Ticks(500));
        c.push(&cap(1, 850, Msg::note_on(0, 60, 90)), 100, &Ticks);
        c.push(&cap(1, 950, Msg::note_off(0, 60)), 100, &Ticks);
        c.push(&cap(1, 1_750, Msg::note_on(0, 62, 90)), 100, &Ticks);
        c.push(&cap(1, 1_850, Msg::note_off(0, 62)), 100, &Ticks);
        assert_eq!(
            c.take(1, (Ticks(800), Ticks(1_600))).notes,
            vec![note(750, 100, 60, 90), note(1_650, 100, 62, 90)]
        );
        let mut c = Capture::recording(Some((Ticks(800), Ticks(1_600))), Ticks(800));
        c.push(&wrap(1, 1), 100, &Ticks);
        c.push(
            &in_pass(1, cap(1, 1_750, Msg::note_on(0, 62, 90))),
            100,
            &Ticks,
        );
        c.push(
            &in_pass(1, cap(1, 1_850, Msg::note_off(0, 62))),
            100,
            &Ticks,
        );
        assert_eq!(
            c.take(1, (Ticks(800), Ticks(1_600))).notes,
            vec![note(1_650, 100, 62, 90)]
        );
    }

    #[test]
    fn a_wrap_before_the_first_message_still_identifies_the_initial_pass() {
        for first in [8_u32, u32::MAX] {
            let next = first.wrapping_add(1);
            let mut c = Capture::recording(Some((Ticks(800), Ticks(1_600))), Ticks(1_200));
            c.push(&wrap(1, next), 100, &Ticks);
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
