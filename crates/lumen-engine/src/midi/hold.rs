//! Which notes sound on a track, and the window-by-window reading of MIDI
//! clips that starts and ends them (M5 slice 1, ADR-0045).
//!
//! One [`TrackState`] per track is the **note tracker**: every note-on that
//! was sent is held here until its note-off is, so any event that must let
//! go (stop, a jump, a loop wrap, an edit, a change of instrument, a lost
//! port, Panic) can release exactly what sounds. Nothing here allocates:
//! the vectors are made at their full size on the UI side and never grow;
//! a full one drops the new event and counts it. Near the top, room is kept
//! for what the tracker itself lets go ([`TrackState::push_release`]): the
//! note-off of a note it holds, and the pedal and the bend put back at rest.
//! Room is kept by where a message comes from, not by what it is: a clip's or
//! a player's own pedal-up takes ordinary room, so a flood of them cannot use
//! up what a held note's note-off needs, and a note can never stick for lack
//! of room. That holds because a track holds at most [`MAX_HELD`] clip notes
//! and [`MAX_HELD`] live notes, which is what [`KEPT_FOR_RELEASES`] is made
//! from.
//!
//! Positions are worked out from ticks every block, through the timeline,
//! so tempo changes move notes with the music. An event belongs to the
//! block that holds `sample_at(tick)`; the window of ticks searched is
//! widened by one sample either side before converting to ticks, so the
//! nearest-sample rounding cannot lose an event on a block boundary.

use lumen_core::midi::{Event, Kind, Msg, cc};
use lumen_core::time::Ticks;

use super::index::{ClipIndex, SLOTS};
use super::kit::DrumKit;
use super::live::LiveState;
use super::{MidiClip, MidiControl, MidiNote};
use crate::timeline::Timeline;

/// Most notes one track holds at once; past it, new notes are dropped.
pub(super) const MAX_HELD: usize = 128;
/// Most events one track queues for one part; past it, they are dropped.
pub(super) const MAX_EVENTS: usize = 1_024;
/// Room at the top of a track's queue kept for what the tracker lets go: a
/// note-off for every clip note and live note it can hold, and the pedal and
/// the bend back at rest for each. Nothing else may use it, and the tracker
/// holds nothing new once the ordinary room is full, so it is never short.
pub(super) const KEPT_FOR_RELEASES: usize = 2 * MAX_HELD + 4;
/// Ticks of margin around a block's window of ticks.
const MARGIN: i64 = 2;
/// How many repeats back a chase looks for a controller.
const CHASE_REPEATS: i64 = 64;
/// A recording window with no bounds.
pub(super) const ALWAYS: (Ticks, Ticks) = (Ticks(i64::MIN), Ticks(i64::MAX));

#[derive(Debug, Clone, Copy)]
pub(super) struct Held {
    pub channel: u8,
    pub key: u8,
    /// Absolute tick where the note began: with the key, what tells the
    /// note apart through an edit (see [`sync`]).
    pub start: Ticks,
    pub on_sample: i64,
    /// Absolute tick of the note-off.
    pub off_tick: Ticks,
}

/// A key a recording track's input holds down, so a recording that begins
/// while it is held can take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Down {
    pub source: u8,
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
    /// It went down inside the recording window, so the take shows its
    /// note and it wins its pitch (`TrackState::yields`).
    pub wins: bool,
}

/// Per track: the held notes and the events waiting for the next part.
pub(super) struct TrackState {
    pub held: Vec<Held>,
    /// Events for the next part, sorted when it is rendered.
    pub events: Vec<Event>,
    /// A sustain pedal down message went out and no up since.
    pub sustain: bool,
    /// A pitch bend away from the centre went out.
    pub bend: bool,
    /// Events and notes dropped for lack of room.
    pub dropped: u32,
    /// Live notes, apart from the clips' (slice 2).
    pub live: LiveState,
    /// Keys the track's recording input holds down now (slice 6).
    pub down: Vec<Down>,
    /// The last part had the track recording.
    pub recording: bool,
    /// The engine pass on which this recording session began.
    pub record_pass: u32,
    /// Where the take shows what is recorded (punch and loop), in ticks:
    /// only there does a key played win its pitch. Captured outside it
    /// stays hidden in the clip, so the clips play on under it.
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

    /// Whether a key played at tick `at` shows in the take: the track
    /// records and `at` is inside the recording window.
    pub fn shows(&self, at: Ticks) -> bool {
        self.recording && self.window_has(at)
    }

    /// Whether tick `at` is inside the recording window.
    pub fn window_has(&self, at: Ticks) -> bool {
        (self.window.0..self.window.1).contains(&at)
    }

    /// Notes a key of the recording input going down or up. `wins`: it went
    /// down where the take shows it ([`Self::shows`]).
    pub fn press(&mut self, source: u8, msg: Msg, wins: bool) {
        match msg.kind() {
            Kind::NoteOn { key, velocity } => {
                let channel = msg.channel();
                self.down
                    .retain(|d| !(d.source == source && d.channel == channel && d.key == key));
                if self.down.len() < MAX_HELD {
                    self.down.push(Down {
                        source,
                        channel,
                        key,
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

    /// Whether the clips' notes on `key` give way to the recording input:
    /// the track records and its input holds the key down. A note played
    /// over a clip's note of the same pitch wins it, as the take will once
    /// committed (`lumen_project::edit::record`), so the old note neither
    /// sounds under it nor ends it. Only inside the recording window, at
    /// tick `at`, and only for a key that went down there: outside it the
    /// take keeps the played note hidden, so the clips play on.
    pub fn yields(&self, key: u8, at: Ticks) -> bool {
        self.shows(at) && self.down.iter().any(|d| d.wins && d.key == key)
    }

    /// Ends the clip notes sounding on `key` at `offset`: the recording
    /// input has just played it.
    pub fn give_way(&mut self, key: u8, offset: u32) {
        let mut i = 0;
        while i < self.held.len() {
            let h = self.held[i];
            if h.key == key {
                self.push_release(Event {
                    offset,
                    msg: Msg::note_off(h.channel, h.key),
                });
                self.held.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Queues `event` for the next part; false (and counted) when there is
    /// no room. It may not use the room kept at the top of the queue, so a
    /// note-off of a held note always finds a place.
    pub fn push(&mut self, event: Event) -> bool {
        self.push_within(event, MAX_EVENTS - KEPT_FOR_RELEASES)
    }

    /// Queues a release the tracker owes: the note-off of a note it holds
    /// (and no longer holds once this is sent), or the pedal or the bend put
    /// back at rest. It may use the room kept at the top of the queue.
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

    /// Sends a note-on at `offset` and holds the note. A note already held
    /// on the key is released first, and the other sounds of its choke group
    /// end at the same sample (M8 D0).
    pub fn start_note(&mut self, held: Held, velocity: u8, offset: u32, kit: Option<&DrumKit>) {
        if let Some(i) = self
            .held
            .iter()
            .position(|h| h.channel == held.channel && h.key == held.key)
        {
            let old = self.held.swap_remove(i);
            self.push_release(Event {
                offset,
                msg: Msg::note_off(old.channel, old.key),
            });
        }
        self.choke(kit, held.key, offset);
        if self.held.len() >= MAX_HELD {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        // A note-on with no room is not held: its note-off would be a
        // release of nothing.
        if self.push(Event {
            offset,
            msg: Msg::note_on(held.channel, held.key, velocity),
        }) {
            self.held.push(held);
        }
    }

    /// Ends the other sounds of the choke group `key` is in, at `offset`: a
    /// new hit of a group silences the rest of it, the owner's CO A. A note
    /// of the same key is left to [`Self::start_note`], which retriggers it.
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
                self.push_release(Event {
                    offset,
                    msg: Msg::note_off(h.channel, h.key),
                });
                self.held.swap_remove(i);
            } else {
                i += 1;
            }
        }
        // The live notes of the group end too: a played hat chokes a clip's
        // open hat, and the other way round.
        let mut live = std::mem::replace(&mut self.live, LiveState::empty());
        live.choke(
            |out_key| out_key != key && kit.choke_group(out_key) == group,
            |channel, out_key| {
                self.push_release(Event {
                    offset,
                    msg: Msg::note_off(channel, out_key),
                });
            },
        );
        self.live = live;
    }

    /// Sends a controller-like message, remembering the pedal and bend once
    /// it is queued (a refused one changed nothing that must be put back).
    pub fn send(&mut self, offset: u32, msg: Msg) {
        if self.push(Event { offset, msg }) {
            self.remember(msg);
        }
    }

    /// Puts the pedal or the bend back at rest: a release the tracker owes.
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

    /// Lets go of the live notes, pedal and bend at the start of the next
    /// part.
    pub fn release_live(&mut self) {
        let mut live = std::mem::replace(&mut self.live, LiveState::empty());
        live.release_all(|event| {
            self.push_release(event);
        });
        self.live = live;
    }

    /// Lets go of everything held, and of the pedal and the bend, at the
    /// start of the next part.
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

/// Whether a message in a track's queue is the note-off of a note it held.
/// Every note-off there is one: a clip's are sent only for held notes, and
/// a player's only for a live note the track holds ([`LiveState::take`]).
pub(super) fn ends_a_held_note(msg: Msg) -> bool {
    matches!(msg.kind(), Kind::NoteOff { .. })
}

/// Tick and sample conversions at the current tempo map and rate.
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

/// The repeats of `clip` that overlap ticks `lo..=hi`, as `first..=last`
/// (empty when `first > last`).
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

/// How many ticks of repeat `rep` the clip plays.
fn span(clip: &MidiClip, period: i64, rep: i64) -> i64 {
    period.min(clip.length.0 - rep * period)
}

/// Where a note ends: the end of its length, cut at the end of its repeat,
/// and at least a tick after its start.
fn off_tick(base: i64, note: &MidiNote, span: i64) -> i64 {
    (base + (note.start.0 + note.length.0).min(span)).max(base + note.start.0 + 1)
}

/// Reads the part `from..to` (timeline samples): note-ons and controllers
/// that fall in it, then the note-offs that come due. Events land in
/// `state.events` at their frame within the part. `kit` is the track's drum
/// map, for the groups that choke each other.
pub(super) fn gather(
    clips: &[MidiClip],
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    (from, to): (i64, i64),
    kit: Option<&DrumKit>,
) {
    // A sample represents a range of ticks. Its inverse is the centre of
    // that range, so a fixed tick margin can miss events that round to
    // `from` but lie before that centre. Bound the search in samples; the
    // exact sample check below still assigns every event to just one part.
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
    // Note-offs that come due, the new notes' included.
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

/// Brings the held notes in line with `clips` after an edit, at `pos`:
/// notes that no longer exist are released, and notes that should be
/// sounding but are not are started now.
///
/// A held note is known by where it began on the timeline and its key, not
/// by its clip's start or its length: resizing a clip (either edge, which
/// moves its start and cuts its notes' lengths anew) or changing a note's
/// length leaves the note sounding, only ending where it now ends. Only a
/// note that is gone or begins elsewhere is released (and chased again if
/// it sounds at `pos`).
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

/// Where `held` ends now, if a note that began where it did on its key is
/// still in `clips` (the longest, if several).
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

/// Where the clip the position `pos` is in begins, in ticks from the start
/// of `clip`, a run of touching clips (`MidiClip::seams`): zero for a
/// single clip. A seam counts from its own sample, so a position on it is
/// in the clip that begins there.
fn owning_start(clip: &MidiClip, clock: Clock<'_>, pos: i64) -> i64 {
    clip.seams
        .iter()
        .rev()
        .find(|s| clock.sample(clip.start.0 + s.0) <= pos)
        .map_or(0, |s| s.0)
}

/// Starts the notes that sound at `pos` (began before it, end after it)
/// and are not held. In a run of touching clips, only notes that began in
/// the clip `pos` is in: one carried on from the clip before belongs to
/// that clip, so playing from the next one does not strike it again.
pub(super) fn chase_notes(
    (clips, index): (&[MidiClip], &[ClipIndex]),
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    pos: i64,
    kit: Option<&DrumKit>,
) {
    let at = clock.tick(pos);
    // A note that ended at or before this tick had ended by the sample `pos`
    // (with a sample's rounding to spare).
    let over = clock.tick(pos.saturating_sub(4));
    for (k, clip) in clips.iter().enumerate() {
        if clip.start.0 > at + MARGIN {
            break;
        }
        let notes = index.get(k).and_then(|i| i.notes_for(clip));
        let period = period(clip);
        let (first, last) = repeats(clip, period, at, at);
        // Only the repeat the position is in: notes end with their repeat.
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

// Notes the chase on this thread has looked at (a test counts them).
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

/// Sends the latest value of every controller, bend, pressure and program
/// set before `pos`, at the start of the next part (bank selects and
/// controllers first, the program last).
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
        // A clip that has ended still leaves its controllers as they were.
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
                // The last event of each controller before the position.
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

/// Orders events by frame, then note-offs, controllers and note-ons, so a
/// note retriggered on one frame ends before it starts. A stable insertion
/// sort that finds each place by bisection and moves the events with one
/// rotation: no allocation, and a list of up to [`MAX_EVENTS`] out of order
/// costs a few memory moves, not a million comparisons.
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
        // After every event that is not greater: equal keys keep their order.
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

    /// A repeatable stream of numbers.
    fn rng(mut seed: u64) -> impl FnMut() -> u64 {
        move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        }
    }

    /// A clip with many short notes, some long ones, controllers on a few
    /// slots, and optionally repeating.
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
            // The old sort, a bubble of swaps, is the reference.
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
        // Half a sixteenth into the last thousand notes.
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
        // Without the index every note before the position is read.
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
                // Every tick that rounds to this sample belongs to its
                // block, even when it is before that sample's inverse tick.
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

        // Timing geometry from the owner's 192-PPQ file, without its
        // musical content. The second E5 starts at quarter 66.
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
        // A pathological part fills one block's queue: a note started before
        // the flood must still end, so it can never stick (M5 review M4).
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

        // A note-on now is refused and not held: no release of nothing later.
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

        // Every held note, the pedal and the bend still let go.
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
        // A clip or a player sends pedal-ups and centred bends: they look
        // like releases but are not the tracker's, so they take ordinary
        // room and the note-off of a held note still finds its place.
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

        // A pedal-down refused now is not remembered as down.
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

    /// A kit whose hats are one group; the kick is in none.
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

    /// Holds `key` from `at`, as a clip note would.
    fn hold_key(state: &mut TrackState, key: u8, at: Ticks, kit: &DrumKit) {
        state.start_note(
            Held {
                channel: 0,
                key,
                start: at,
                on_sample: at.0,
                off_tick: Ticks(at.0 + 10_000),
            },
            100,
            0,
            Some(kit),
        );
    }

    #[test]
    fn a_hit_of_a_choke_group_ends_the_others_at_the_same_offset_and_keeps_the_tracker_true() {
        let kit = hat_kit();
        let mut state = TrackState::new();
        hold_key(&mut state, 46, Ticks(10), &kit);
        // The kick is in no group: it ends nothing.
        hold_key(&mut state, 36, Ticks(20), &kit);
        assert_eq!(state.held.len(), 2);
        assert_eq!(state.events.len(), 2, "both note-ons went out");
        // The closed hat strikes: the open hat's off precedes its on, both at
        // offset 0 of this part.
        hold_key(&mut state, 42, Ticks(30), &kit);
        let kinds: Vec<(u32, u8, Msg)> = state
            .events
            .iter()
            .skip(2)
            .map(|e| (e.offset, e.msg.channel(), e.msg))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (0, 0, Msg::note_off(0, 46)),
                (0, 0, Msg::note_on(0, 42, 100))
            ],
            "the open hat ends where the closed one starts"
        );
        assert!(
            state.held.iter().all(|h| h.key != 46),
            "the open hat is no longer held"
        );
        // Letting go of everything answers the notes still held, once.
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
    fn a_choked_note_is_not_released_again_when_its_own_end_comes() {
        use lumen_core::meter::Meter;
        let kit = hat_kit();
        let mut state = TrackState::new();
        hold_key(&mut state, 46, Ticks(0), &kit);
        hold_key(&mut state, 42, Ticks(5), &kit);
        state.events.clear();
        // The open hat's own end (tick 10,000) is gone from the tracker: no
        // second note-off for it when the part is read past it.
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
        // The closed hat's own end still comes.
        assert!(state.events.iter().any(|e| e.msg == Msg::note_off(0, 42)));
    }

    #[test]
    fn a_hit_of_one_group_leaves_another_groups_sound_alone() {
        let mut kit = hat_kit();
        kit.set_choke(36, 2);
        let mut state = TrackState::new();
        hold_key(&mut state, 36, Ticks(0), &kit);
        hold_key(&mut state, 46, Ticks(1), &kit);
        hold_key(&mut state, 42, Ticks(2), &kit);
        assert_eq!(
            state.held.iter().map(|h| h.key).collect::<Vec<_>>(),
            vec![36, 42],
            "the kick of group 2 plays on"
        );
    }
}
