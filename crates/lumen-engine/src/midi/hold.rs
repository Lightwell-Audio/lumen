use lumen_core::midi::{Event, Kind, Msg, cc};
use lumen_core::time::Ticks;

use super::index::{ClipIndex, SLOTS};
use super::kit::DrumKit;
use super::live::LiveState;
use super::{MidiClip, MidiControl, MidiNote};
use crate::timeline::Timeline;

pub(super) const MAX_HELD: usize = 128;
pub(super) const MAX_EVENTS: usize = 1_024;
pub(super) const KEPT_FOR_RELEASES: usize = 2 * MAX_HELD + 4;
const MARGIN: i64 = 2;
const CHASE_REPEATS: i64 = 64;
pub(super) const ALWAYS: (Ticks, Ticks) = (Ticks(i64::MIN), Ticks(i64::MAX));

#[derive(Debug, Clone, Copy)]
pub(super) struct Held {
    pub channel: u8,
    pub key: u8,
    pub start: Ticks,
    pub on_sample: i64,
    pub off_tick: Ticks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Down {
    pub source: u8,
    pub channel: u8,
    pub key: u8,
    pub played: u8,
    pub velocity: u8,
    pub wins: bool,
}

pub(super) struct TrackState {
    pub held: Vec<Held>,
    pub events: Vec<Event>,
    pub sustain: bool,
    pub bend: bool,
    pub dropped: u32,
    pub live: LiveState,
    pub down: Vec<Down>,
    pub recording: bool,
    pub record_pass: u32,
    pub window: (Ticks, Ticks),
}

impl TrackState {
    pub fn new() -> Self {
        Self {
            held: Vec::with_capacity(MAX_HELD),
            events: Vec::with_capacity(MAX_EVENTS),
            sustain: false,
            bend: false,
            dropped: 0,
            live: LiveState::new(),
            down: Vec::with_capacity(MAX_HELD),
            recording: false,
            record_pass: 0,
            window: ALWAYS,
        }
    }

    pub fn shows(&self, at: Ticks) -> bool {
        self.recording && self.window_has(at)
    }

    pub fn window_has(&self, at: Ticks) -> bool {
        (self.window.0..self.window.1).contains(&at)
    }

    pub fn press(&mut self, source: u8, msg: Msg, wins: bool, kit: Option<&DrumKit>) {
        match msg.kind() {
            Kind::NoteOn { key, velocity } => {
                let channel = msg.channel();
                let played = kit.map_or(key, |kit| kit.key(key).key);
                self.down
                    .retain(|d| !(d.source == source && d.channel == channel && d.key == key));
                if self.down.len() < MAX_HELD {
                    self.down.push(Down {
                        source,
                        channel,
                        key,
                        played,
                        velocity,
                        wins,
                    });
                }
            }
            Kind::NoteOff { key, .. } => {
                let channel = msg.channel();
                self.down
                    .retain(|d| !(d.source == source && d.channel == channel && d.key == key));
            }
            _ => {}
        }
    }

    pub fn yields(&self, key: u8, at: Ticks) -> bool {
        self.shows(at) && self.down.iter().any(|d| d.wins && d.played == key)
    }

    pub fn give_way(&mut self, key: u8, offset: u32) {
        let mut i = 0;
        while i < self.held.len() {
            let h = self.held[i];
            if h.key == key {
                self.end_before_it_sounded(h.channel, h.key, offset);
                self.held.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn push(&mut self, event: Event) -> bool {
        self.push_within(event, MAX_EVENTS - KEPT_FOR_RELEASES)
    }

    pub(super) fn push_release(&mut self, event: Event) -> bool {
        self.push_within(event, MAX_EVENTS)
    }

    fn push_within(&mut self, event: Event, room: usize) -> bool {
        if self.events.len() < room {
            self.events.push(event);
            true
        } else {
            self.dropped = self.dropped.saturating_add(1);
            false
        }
    }

    pub fn start_note(&mut self, held: Held, velocity: u8, offset: u32, kit: Option<&DrumKit>) {
        if let Some(i) = self
            .held
            .iter()
            .position(|h| h.channel == held.channel && h.key == held.key)
        {
            let old = self.held.swap_remove(i);
            self.end_before_it_sounded(old.channel, old.key, offset);
        }
        self.choke(kit, held.key, offset);
        if self.held.len() >= MAX_HELD {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        if self.push(Event {
            offset,
            msg: Msg::note_on(held.channel, held.key, velocity),
        }) {
            self.held.push(held);
        }
    }

    fn end_before_it_sounded(&mut self, channel: u8, key: u8, offset: u32) {
        if !self.cancel_queued_on(channel, key, offset) {
            self.push_release(Event {
                offset,
                msg: Msg::note_off(channel, key),
            });
        }
    }

    fn cancel_queued_on(&mut self, channel: u8, key: u8, offset: u32) -> bool {
        let found = self.events.iter().rposition(|e| {
            e.offset == offset
                && e.msg.channel() == channel
                && matches!(e.msg.kind(), Kind::NoteOn { key: k, .. } if k == key)
        });
        match found {
            Some(i) => {
                self.events.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn choke(&mut self, kit: Option<&DrumKit>, key: u8, offset: u32) {
        let Some(kit) = kit.filter(|k| k.chokes()) else {
            return;
        };
        let group = kit.choke_group(key);
        if group == 0 {
            return;
        }
        let mut i = 0;
        while i < self.held.len() {
            let h = self.held[i];
            if h.key != key && kit.choke_group(h.key) == group {
                self.end_before_it_sounded(h.channel, h.key, offset);
                self.held.swap_remove(i);
            } else {
                i += 1;
            }
        }
        let mut live = std::mem::replace(&mut self.live, LiveState::empty());
        live.choke(
            |out_key| out_key != key && kit.choke_group(out_key) == group,
            |channel, out_key| self.end_before_it_sounded(channel, out_key, offset),
        );
        self.live = live;
    }

    pub fn send(&mut self, offset: u32, msg: Msg) {
        if self.push(Event { offset, msg }) {
            self.remember(msg);
        }
    }

    fn send_rest(&mut self, msg: Msg) {
        if self.push_release(Event { offset: 0, msg }) {
            self.remember(msg);
        }
    }

    fn remember(&mut self, msg: Msg) {
        match msg.kind() {
            Kind::Controller { number, value } if number == cc::SUSTAIN => {
                self.sustain = value >= 64;
            }
            Kind::PitchBend { value } => self.bend = value != 0,
            _ => {}
        }
    }

    pub fn release_live(&mut self) {
        let mut live = std::mem::replace(&mut self.live, LiveState::empty());
        live.release_all(|event| {
            self.push_release(event);
        });
        self.live = live;
    }

    pub fn release_all(&mut self, channel: u8) {
        for i in 0..self.held.len() {
            let h = self.held[i];
            self.push_release(Event {
                offset: 0,
                msg: Msg::note_off(h.channel, h.key),
            });
        }
        self.held.clear();
        if self.sustain {
            self.send_rest(Msg::controller(channel, cc::SUSTAIN, 0));
        }
        if self.bend {
            self.send_rest(Msg::pitch_bend(channel, 0));
        }
    }
}

pub(super) fn ends_a_held_note(msg: Msg) -> bool {
    matches!(msg.kind(), Kind::NoteOff { .. })
}

#[derive(Clone, Copy)]
pub(super) struct Clock<'a> {
    pub timeline: &'a Timeline,
    pub rate: u32,
}

impl Clock<'_> {
    pub(super) fn sample(&self, tick: i64) -> i64 {
        self.timeline.sample_at(Ticks(tick), self.rate)
    }

    pub(super) fn tick(&self, sample: i64) -> i64 {
        self.timeline.ticks_at(sample, self.rate).0
    }
}

fn period(clip: &MidiClip) -> i64 {
    clip.period
        .map(|p| p.0)
        .filter(|&p| p > 0)
        .unwrap_or(clip.length.0)
        .max(1)
}

fn repeats(clip: &MidiClip, period: i64, lo: i64, hi: i64) -> (i64, i64) {
    let (start, length) = (clip.start.0, clip.length.0);
    if length <= 0 || hi < start || lo >= start + length {
        return (1, 0);
    }
    let count = (length + period - 1) / period;
    let first = ((lo - start).max(0)) / period;
    let last = ((hi - start) / period).min(count - 1);
    (first, last)
}

fn span(clip: &MidiClip, period: i64, rep: i64) -> i64 {
    period.min(clip.length.0 - rep * period)
}

fn off_tick(base: i64, note: &MidiNote, span: i64) -> i64 {
    (base + (note.start.0 + note.length.0).min(span)).max(base + note.start.0 + 1)
}

pub(super) fn gather(
    clips: &[MidiClip],
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    (from, to): (i64, i64),
    kit: Option<&DrumKit>,
) {
    let lo = clock.tick(from.saturating_sub(1)) - MARGIN;
    let hi = clock.tick(to.saturating_add(1)) + MARGIN;
    for clip in clips {
        if clip.start.0 > hi {
            break;
        }
        let period = period(clip);
        let (first, last) = repeats(clip, period, lo, hi);
        for rep in first..=last {
            let base = clip.start.0 + rep * period;
            let span = span(clip, period, rep);
            let (l, h) = (lo - base, hi - base);
            let i = clip.notes.partition_point(|n| n.start.0 < l);
            for note in &clip.notes[i..] {
                if note.start.0 > h || note.start.0 >= span {
                    break;
                }
                let at = clock.sample(base + note.start.0);
                if (from..to).contains(&at) && !state.yields(note.pitch, Ticks(base + note.start.0))
                {
                    state.start_note(
                        Held {
                            channel: note.channel.unwrap_or(channel) & 15,
                            key: note.pitch,
                            start: Ticks(base + note.start.0),
                            on_sample: at,
                            off_tick: Ticks(off_tick(base, note, span)),
                        },
                        note.velocity,
                        (at - from) as u32,
                        kit,
                    );
                }
            }
            let i = clip.controls.partition_point(|c| c.at.0 < l);
            for control in &clip.controls[i..] {
                if control.at.0 > h || control.at.0 >= span {
                    break;
                }
                let at = clock.sample(base + control.at.0);
                if (from..to).contains(&at) {
                    state.send((at - from) as u32, with_channel(control.msg, channel));
                }
            }
        }
    }
    let mut i = 0;
    while i < state.held.len() {
        let h = state.held[i];
        let off = clock.sample(h.off_tick.0).max(h.on_sample + 1);
        if off < to {
            state.push_release(Event {
                offset: (off - from).max(0) as u32,
                msg: Msg::note_off(h.channel, h.key),
            });
            state.held.swap_remove(i);
        } else {
            i += 1;
        }
    }
}

fn with_channel(msg: Msg, channel: u8) -> Msg {
    Msg([(msg.0[0] & 0xF0) | (channel & 15), msg.0[1], msg.0[2]])
}

pub(super) fn sync(
    (clips, index): (&[MidiClip], &[ClipIndex]),
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    pos: i64,
    kit: Option<&DrumKit>,
) {
    let mut i = 0;
    while i < state.held.len() {
        let h = state.held[i];
        match renewed(clips, &h) {
            Some(off) => {
                state.held[i].off_tick = Ticks(off);
                i += 1;
            }
            None => {
                state.push_release(Event {
                    offset: 0,
                    msg: Msg::note_off(h.channel, h.key),
                });
                state.held.swap_remove(i);
            }
        }
    }
    chase_notes((clips, index), state, channel, clock, pos, kit);
}

fn renewed(clips: &[MidiClip], held: &Held) -> Option<i64> {
    let at = held.start.0;
    let mut off: Option<i64> = None;
    for clip in clips {
        if clip.start.0 > at {
            break;
        }
        let period = period(clip);
        let (rep, last) = repeats(clip, period, at, at);
        if rep > last {
            continue;
        }
        let base = clip.start.0 + rep * period;
        let span = span(clip, period, rep);
        let within = at - base;
        if within >= span {
            continue;
        }
        let i = clip.notes.partition_point(|n| n.start.0 < within);
        for note in clip.notes[i..].iter().take_while(|n| n.start.0 == within) {
            if note.pitch == held.key {
                let end = off_tick(base, note, span);
                off = Some(off.map_or(end, |o| o.max(end)));
            }
        }
    }
    off
}

fn owning_start(clip: &MidiClip, clock: Clock<'_>, pos: i64) -> i64 {
    clip.seams
        .iter()
        .rev()
        .find(|s| clock.sample(clip.start.0 + s.0) <= pos)
        .map_or(0, |s| s.0)
}

pub(super) fn chase_notes(
    (clips, index): (&[MidiClip], &[ClipIndex]),
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    pos: i64,
    kit: Option<&DrumKit>,
) {
    let at = clock.tick(pos);
    let over = clock.tick(pos.saturating_sub(4));
    for (k, clip) in clips.iter().enumerate() {
        if clip.start.0 > at + MARGIN {
            break;
        }
        let notes = index.get(k).and_then(|i| i.notes_for(clip));
        let period = period(clip);
        let (first, last) = repeats(clip, period, at, at);
        for rep in first..=last {
            let base = clip.start.0 + rep * period;
            let span = span(clip, period, rep);
            let limit = at - base + MARGIN;
            let end = clip.notes.partition_point(|n| n.start.0 <= limit);
            let owned = match clip.period {
                Some(_) => 0,
                None => owning_start(clip, clock, pos),
            };
            let first = clip.notes[..end].partition_point(|n| n.start.0 < owned);
            let mut visit = |i: usize| -> bool {
                let note = &clip.notes[i];
                if note.start.0 >= span {
                    return false;
                }
                #[cfg(test)]
                VISITED.with(|v| v.set(v.get() + 1));
                let on = clock.sample(base + note.start.0);
                let off_t = off_tick(base, note, span);
                let off = clock.sample(off_t).max(on + 1);
                let start = Ticks(base + note.start.0);
                if on < pos
                    && off > pos
                    && !state.yields(note.pitch, Ticks(at))
                    && !state
                        .held
                        .iter()
                        .any(|h| h.start == start && h.key == note.pitch)
                {
                    state.start_note(
                        Held {
                            channel: note.channel.unwrap_or(channel) & 15,
                            key: note.pitch,
                            start,
                            on_sample: on,
                            off_tick: Ticks(off_t),
                        },
                        note.velocity,
                        0,
                        kit,
                    );
                }
                true
            };
            match notes {
                Some(notes) => {
                    notes.each_ending_after(&clip.notes, (first, end), over - base, &mut visit);
                }
                None => {
                    for i in first..end {
                        if !visit(i) {
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn slot(msg: Msg) -> Option<usize> {
    use lumen_core::midi::Kind;
    match msg.kind() {
        Kind::Controller { number, .. } => Some(usize::from(number)),
        Kind::PitchBend { .. } => Some(128),
        Kind::Pressure { .. } => Some(129),
        Kind::Program { .. } => Some(130),
        _ => None,
    }
}

pub(super) fn chase_controls(
    (clips, index): (&[MidiClip], &[ClipIndex]),
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    pos: i64,
) {
    let mut latest: [Option<(i64, Msg)>; SLOTS] = [None; SLOTS];
    let at = clock.tick(pos);
    for (k, clip) in clips.iter().enumerate() {
        if clip.start.0 > at {
            break;
        }
        let controls = index.get(k).and_then(|i| i.controls_for(clip));
        let period = period(clip);
        let count = ((clip.length.0 + period - 1) / period).max(1);
        let last = ((at - clip.start.0) / period).min(count - 1);
        for rep in (last - CHASE_REPEATS).max(0)..=last {
            let base = clip.start.0 + rep * period;
            let span = span(clip, period, rep);
            let mut keep = |slot: usize, control: &MidiControl| {
                let tick = base + control.at.0;
                if latest[slot].is_none_or(|(t, _)| tick >= t) {
                    latest[slot] = Some((tick, control.msg));
                }
            };
            match controls {
                Some(controls) => {
                    for (slot, list) in controls.slots() {
                        let before = list.partition_point(|&j| {
                            let c = &clip.controls[j as usize];
                            c.at.0 < span && clock.sample(base + c.at.0) < pos
                        });
                        if before > 0 {
                            keep(slot, &clip.controls[list[before - 1] as usize]);
                        }
                    }
                }
                None => {
                    for control in clip.controls.iter() {
                        if control.at.0 >= span {
                            break;
                        }
                        let Some(slot) = slot(control.msg) else {
                            continue;
                        };
                        if clock.sample(base + control.at.0) < pos {
                            keep(slot, control);
                        }
                    }
                }
            }
        }
    }
    for (_, msg) in latest.into_iter().flatten() {
        state.send(0, with_channel(msg, channel));
    }
}

pub(super) fn sort_events(events: &mut [Event]) {
    fn key(event: &Event) -> (u32, u8) {
        use lumen_core::midi::Kind;
        let rank = match event.msg.kind() {
            Kind::NoteOff { .. } => 0,
            Kind::NoteOn { .. } => 2,
            _ => 1,
        };
        (event.offset, rank)
    }
    for i in 1..events.len() {
        let this = key(&events[i]);
        if key(&events[i - 1]) <= this {
            continue;
        }
        let at = events[..i].partition_point(|e| key(e) <= this);
        events[at..=i].rotate_right(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{control_list, note_list};
    use lumen_core::meter::Meter;

    fn clip(start: Ticks, note: Ticks) -> MidiClip {
        MidiClip {
            id: 1,
            start,
            length: Ticks::quarters(8),
            period: None,
            seams: Default::default(),
            notes: note_list(vec![MidiNote {
                start: note,
                length: Ticks::quarters(1),
                pitch: 76,
                velocity: 110,
                channel: None,
            }]),
            controls: control_list(vec![super::super::MidiControl {
                at: note,
                msg: Msg::controller(0, 11, 90),
            }]),
        }
    }

    fn rng(mut seed: u64) -> impl FnMut() -> u64 {
        move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        }
    }

    fn busy_clip(next: &mut impl FnMut() -> u64, start: i64, repeats: bool) -> MidiClip {
        let q = Ticks::quarters(1).0;
        let length = q * 400;
        let notes = (0..2_000)
            .map(|_| {
                let at = (next() % (length as u64 - 1)) as i64;
                let long = next().is_multiple_of(40);
                MidiNote {
                    start: Ticks(at),
                    length: Ticks(if long {
                        (next() % (q as u64 * 120)) as i64
                    } else {
                        (next() % (q as u64 / 2)) as i64
                    }),
                    pitch: 30 + (next() % 60) as u8,
                    velocity: 100,
                    channel: None,
                }
            })
            .collect();
        let controls = (0..500)
            .map(|_| {
                let at = Ticks((next() % (length as u64 - 1)) as i64);
                let msg = match next() % 4 {
                    0 => Msg::controller(0, 1, (next() % 128) as u8),
                    1 => Msg::controller(0, 74, (next() % 128) as u8),
                    2 => Msg::pitch_bend(0, (next() % 16_000) as i16),
                    _ => Msg::program(0, (next() % 100) as u8),
                };
                super::super::MidiControl { at, msg }
            })
            .collect();
        MidiClip {
            id: 1,
            start: Ticks(start),
            length: Ticks(length),
            period: repeats.then_some(Ticks(q * 64)),
            seams: Default::default(),
            notes: note_list(notes),
            controls: control_list(controls),
        }
    }

    #[test]
    fn events_sort_by_frame_then_offs_controllers_and_ons_and_equal_ones_keep_their_order() {
        let mut next = rng(0x1234_5678);
        for len in [0, 1, 2, 3, 17, 100, 1_024] {
            let events: Vec<Event> = (0..len)
                .map(|i| Event {
                    offset: (next() % 24) as u32,
                    msg: match next() % 3 {
                        0 => Msg::note_on(0, (next() % 128) as u8, 100),
                        1 => Msg::note_off(0, (next() % 128) as u8),
                        _ => Msg::controller(0, 1, (i % 128) as u8),
                    },
                })
                .collect();
            let rank = |e: &Event| match e.msg.kind() {
                Kind::NoteOff { .. } => 0,
                Kind::NoteOn { .. } => 2,
                _ => 1,
            };
            let mut want = events.clone();
            for i in 1..want.len() {
                let mut j = i;
                while j > 0
                    && (want[j - 1].offset, rank(&want[j - 1])) > (want[j].offset, rank(&want[j]))
                {
                    want.swap(j - 1, j);
                    j -= 1;
                }
            }
            let mut got = events;
            sort_events(&mut got);
            assert_eq!(got, want, "{len} events");
        }
    }

    #[test]
    fn the_indexed_chase_starts_the_same_notes_and_controllers_as_reading_every_note() {
        let mut next = rng(0x9e37_79b9);
        for (bpm, rate) in [
            (120.0, 48_000),
            (20.0, 192_000),
            (999.0, 44_100),
            (97.3, 48_000),
        ] {
            let timeline = Timeline::constant(bpm, Meter::FOUR_FOUR);
            let clock = Clock {
                timeline: &timeline,
                rate,
            };
            for repeats in [false, true] {
                let clips = vec![
                    busy_clip(&mut next, 0, repeats),
                    busy_clip(&mut next, Ticks::quarters(300).0, repeats),
                ];
                let index: Vec<ClipIndex> = clips.iter().map(ClipIndex::new).collect();
                for _ in 0..60 {
                    let tick = (next() % (Ticks::quarters(800).0 as u64)) as i64;
                    let pos = clock.sample(tick);
                    let mut plain = TrackState::new();
                    let mut fast = TrackState::new();
                    chase_notes((&clips, &[]), &mut plain, 0, clock, pos, None);
                    chase_controls((&clips, &[]), &mut plain, 0, clock, pos);
                    chase_notes((&clips, &index), &mut fast, 0, clock, pos, None);
                    chase_controls((&clips, &index), &mut fast, 0, clock, pos);
                    let words = |s: &TrackState| format!("{:?} {:?}", s.held, s.events);
                    assert_eq!(
                        words(&fast),
                        words(&plain),
                        "{bpm} BPM, {rate} Hz, tick {tick}, repeating {repeats}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_chase_late_in_a_big_clip_reads_the_notes_that_can_sound_not_the_song_so_far() {
        let timeline = Timeline::constant(120.0, Meter::FOUR_FOUR);
        let clock = Clock {
            timeline: &timeline,
            rate: 48_000,
        };
        let step = Ticks::quarters(1).0 / 4;
        let notes: Vec<MidiNote> = (0..100_000)
            .map(|i| MidiNote {
                start: Ticks(i * step),
                length: Ticks(step),
                pitch: 36 + (i % 48) as u8,
                velocity: 100,
                channel: None,
            })
            .collect();
        let mut c = clip(Ticks::ZERO, Ticks::ZERO);
        c.length = Ticks(100_001 * step);
        c.notes = note_list(notes);
        let clips = [c];
        let index: Vec<ClipIndex> = clips.iter().map(ClipIndex::new).collect();
        let pos = clock.sample(99_000 * step + step / 2);
        let looked = |index: &[ClipIndex]| {
            VISITED.with(|v| v.set(0));
            let mut state = TrackState::new();
            chase_notes((&clips, index), &mut state, 0, clock, pos, None);
            (VISITED.with(|v| v.get()), state.held.len())
        };
        let (visited, held) = looked(&index);
        assert_eq!(held, 1, "the one note that sounds is started");
        assert!(
            visited <= 130,
            "{visited} notes were read to find one that sounds"
        );
        let (every, held_plain) = looked(&[]);
        assert_eq!(held_plain, 1);
        assert!(every > 90_000, "{every}");
    }

    #[test]
    fn events_rounding_to_a_blocks_first_sample_are_never_skipped_or_repeated() {
        for bpm in [20.0, 97.3, 120.0, 150.0, 999.0] {
            for rate in [22_050, 44_100, 48_000, 192_000] {
                let timeline = Timeline::constant(bpm, Meter::FOUR_FOUR);
                let clock = Clock {
                    timeline: &timeline,
                    rate,
                };
                let sample = 1_000;
                for tick in clock.tick(sample - 1)..=clock.tick(sample + 1) {
                    if clock.sample(tick) != sample {
                        continue;
                    }
                    for start in [Ticks::ZERO, Ticks(tick)] {
                        let clips = [clip(start, Ticks(tick) - start)];
                        let mut state = TrackState::new();
                        gather(&clips, &mut state, 0, clock, (sample - 128, sample), None);
                        assert!(state.events.is_empty());
                        gather(&clips, &mut state, 0, clock, (sample, sample + 128), None);
                        assert_eq!(
                            state.events.len(),
                            2,
                            "{bpm} BPM, {rate} Hz, tick {tick}, clip {start:?}"
                        );
                        assert!(state.events.iter().all(|e| e.offset == 0));
                        state.events.clear();
                        gather(
                            &clips,
                            &mut state,
                            0,
                            clock,
                            (sample + 128, sample + 256),
                            None,
                        );
                        assert!(state.events.is_empty(), "no duplicate event");
                    }
                }
            }
        }
    }

    #[test]
    fn a_note_at_the_reported_ocarina_onset_sounds_on_play_and_at_a_block_boundary() {
        use lumen_core::time::{TempoMap, TempoPoint};

        let timeline = Timeline::new(
            TempoMap::new(
                [
                    (0, 400_000),
                    (52, 428_571),
                    (54, 500_000),
                    (60, 500_000),
                    (62, 428_571),
                    (64, 400_000),
                ]
                .into_iter()
                .map(|(beat, micros)| TempoPoint {
                    at: Ticks::quarters(beat),
                    bpm: 60_000_000.0 / f64::from(micros),
                    ramp: false,
                })
                .collect(),
            )
            .unwrap(),
            Timeline::default().meter,
        );
        let clock = Clock {
            timeline: &timeline,
            rate: 48_000,
        };
        let tick = Ticks::quarters(66);
        let sample = clock.sample(tick.0);
        assert_eq!(sample, 1_311_086);
        assert_eq!(clock.tick(sample) - tick.0, 27);
        let mut c = clip(Ticks::ZERO, tick);
        c.length = Ticks::quarters(80);
        let clips = [c];
        for starting in [false, true] {
            let mut state = TrackState::new();
            if starting {
                chase_notes((&clips, &[]), &mut state, 0, clock, sample, None);
            } else {
                gather(&clips, &mut state, 0, clock, (sample - 128, sample), None);
            }
            assert!(state.events.is_empty());
            gather(&clips, &mut state, 0, clock, (sample, sample + 128), None);
            assert_eq!(state.held.len(), 1, "the E5 sounds (starting={starting})");
            assert_eq!(
                state
                    .events
                    .iter()
                    .filter(|e| matches!(e.msg.kind(), Kind::NoteOn { .. }))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn a_full_queue_refuses_new_notes_and_never_a_note_off() {
        let mut state = TrackState::new();
        let held = |key| Held {
            channel: 0,
            key,
            start: Ticks::ZERO,
            on_sample: 0,
            off_tick: Ticks(10),
        };
        state.start_note(held(60), 100, 0, None);
        state.release_live();
        let mut refused = 0;
        for i in 0..2 * MAX_EVENTS {
            if !state.push(Event {
                offset: 1,
                msg: Msg::controller(0, 1, (i % 128) as u8),
            }) {
                refused += 1;
            }
        }
        assert_eq!(state.events.len(), MAX_EVENTS - KEPT_FOR_RELEASES);
        assert_eq!(refused, 2 * MAX_EVENTS - state.events.len() + 1);

        state.start_note(held(62), 100, 2, None);
        assert!(state.held.iter().all(|h| h.key != 62), "not held");
        let ons = |state: &TrackState| {
            state
                .events
                .iter()
                .filter(|e| matches!(e.msg.kind(), Kind::NoteOn { .. }))
                .count()
        };
        assert_eq!(ons(&state), 1, "only the first note went out");

        state.sustain = true;
        state.bend = true;
        state.release_all(0);
        let offs = state
            .events
            .iter()
            .filter(|e| matches!(e.msg.kind(), Kind::NoteOff { key: 60, .. }))
            .count();
        assert_eq!(offs, 1, "the first note ends");
        assert!(
            state
                .events
                .iter()
                .any(|e| e.msg == Msg::controller(0, cc::SUSTAIN, 0))
        );
        assert!(state.events.iter().any(|e| e.msg == Msg::pitch_bend(0, 0)));
        assert!(state.events.len() <= MAX_EVENTS);
    }

    #[test]
    fn a_flood_of_pedal_ups_and_centred_bends_cannot_take_a_note_offs_room() {
        let mut state = TrackState::new();
        let held = |key| Held {
            channel: 0,
            key,
            start: Ticks::ZERO,
            on_sample: 0,
            off_tick: Ticks(10),
        };
        state.start_note(held(60), 100, 0, None);
        for i in 0..2 * MAX_EVENTS {
            let msg = if i % 2 == 0 {
                Msg::controller(0, cc::SUSTAIN, 0)
            } else {
                Msg::pitch_bend(0, 0)
            };
            state.send(1, msg);
        }
        assert_eq!(state.events.len(), MAX_EVENTS - KEPT_FOR_RELEASES);
        assert!(!state.sustain && !state.bend);

        state.send(2, Msg::controller(0, cc::SUSTAIN, 127));
        assert!(!state.sustain, "a refused pedal-down changed nothing");

        state.release_all(0);
        let offs = state
            .events
            .iter()
            .filter(|e| matches!(e.msg.kind(), Kind::NoteOff { key: 60, .. }))
            .count();
        assert_eq!(offs, 1, "the held note ends");
        assert!(state.held.is_empty());
    }

    fn hat_kit() -> DrumKit {
        let mut kit = DrumKit::new();
        for key in [42, 46, 36] {
            kit.set(
                key,
                crate::midi::KitKey {
                    key,
                    channel: None,
                    silent: false,
                },
            );
        }
        kit.set_choke(42, 1);
        kit.set_choke(46, 1);
        kit
    }

    fn hold_key_at(state: &mut TrackState, key: u8, at: Ticks, offset: u32, kit: &DrumKit) {
        state.start_note(
            Held {
                channel: 0,
                key,
                start: at,
                on_sample: at.0,
                off_tick: Ticks(at.0 + 10_000),
            },
            100,
            offset,
            Some(kit),
        );
    }

    fn hold_key(state: &mut TrackState, key: u8, at: Ticks, kit: &DrumKit) {
        hold_key_at(state, key, at, 0, kit);
    }

    #[test]
    fn a_hit_of_a_choke_group_ends_the_others_at_the_same_offset_and_keeps_the_tracker_true() {
        let kit = hat_kit();
        let mut state = TrackState::new();
        hold_key(&mut state, 46, Ticks(10), &kit);
        hold_key(&mut state, 36, Ticks(20), &kit);
        assert_eq!(state.held.len(), 2);
        assert_eq!(state.events.len(), 2, "both note-ons went out");
        hold_key_at(&mut state, 42, Ticks(30), 100, &kit);
        let kinds: Vec<(u32, u8, Msg)> = state
            .events
            .iter()
            .skip(2)
            .map(|e| (e.offset, e.msg.channel(), e.msg))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (100, 0, Msg::note_off(0, 46)),
                (100, 0, Msg::note_on(0, 42, 100))
            ],
            "the open hat ends where the closed one starts"
        );
        assert!(
            state.held.iter().all(|h| h.key != 46),
            "the open hat is no longer held"
        );
        state.release_all(0);
        let offs = |key: u8| {
            state
                .events
                .iter()
                .filter(|e| e.msg == Msg::note_off(0, key))
                .count()
        };
        assert_eq!((offs(46), offs(42), offs(36)), (1, 1, 1));
        assert!(state.held.is_empty());
    }

    #[test]
    fn a_note_choked_before_its_part_plays_never_reaches_the_instrument() {
        let kit = hat_kit();
        let mut state = TrackState::new();
        hold_key(&mut state, 46, Ticks(10), &kit);
        hold_key(&mut state, 42, Ticks(10), &kit);
        assert_eq!(
            state.events.iter().map(|e| e.msg).collect::<Vec<_>>(),
            vec![Msg::note_on(0, 42, 100)],
            "only the sound that is heard went out"
        );
        assert_eq!(
            state.held.iter().map(|h| h.key).collect::<Vec<_>>(),
            vec![42],
            "the open hat is not held: it never sounded"
        );
        state.release_all(0);
        let offs = |key: u8| {
            state
                .events
                .iter()
                .filter(|e| e.msg == Msg::note_off(0, key))
                .count()
        };
        assert_eq!((offs(46), offs(42)), (0, 1), "no stray note-off");
    }

    #[test]
    fn a_clip_note_chased_in_the_pass_a_pad_wins_it_never_reaches_the_instrument() {
        let kit = hat_kit();
        let mut state = TrackState::new();
        hold_key(&mut state, 44, Ticks(0), &kit);
        state.give_way(44, 0);
        assert!(
            state.events.is_empty(),
            "nothing reached the instrument: {:?}",
            state.events
        );
        assert!(state.held.is_empty());
        hold_key(&mut state, 44, Ticks(0), &kit);
        state.events.clear();
        state.give_way(44, 0);
        assert_eq!(
            state.events.iter().map(|e| e.msg).collect::<Vec<_>>(),
            vec![Msg::note_off(0, 44)]
        );
    }

    #[test]
    fn a_choked_note_is_not_released_again_when_its_own_end_comes() {
        use lumen_core::meter::Meter;
        let kit = hat_kit();
        let mut state = TrackState::new();
        hold_key(&mut state, 46, Ticks(0), &kit);
        hold_key_at(&mut state, 42, Ticks(5), 100, &kit);
        state.events.clear();
        let timeline = Timeline::constant(120.0, Meter::FOUR_FOUR);
        let clock = Clock {
            timeline: &timeline,
            rate: 48_000,
        };
        let clips: [MidiClip; 0] = [];
        gather(
            &clips,
            &mut state,
            0,
            clock,
            (0, i64::from(u32::MAX)),
            Some(&kit),
        );
        assert!(
            state.events.iter().all(|e| e.msg != Msg::note_off(0, 46)),
            "the open hat's end sent nothing"
        );
        assert!(state.events.iter().any(|e| e.msg == Msg::note_off(0, 42)));
    }

    #[test]
    fn a_hit_of_one_group_leaves_another_groups_sound_alone() {
        let mut kit = hat_kit();
        kit.set_choke(36, 2);
        let mut state = TrackState::new();
        hold_key(&mut state, 36, Ticks(0), &kit);
        hold_key(&mut state, 46, Ticks(1), &kit);
        hold_key_at(&mut state, 42, Ticks(2), 100, &kit);
        assert_eq!(
            state.held.iter().map(|h| h.key).collect::<Vec<_>>(),
            vec![36, 42],
            "the kick of group 2 plays on"
        );
    }
}
