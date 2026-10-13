//! Which notes sound on a track, and the window-by-window reading of MIDI
//! clips that starts and ends them (M5 slice 1, ADR-0045).
//!
//! One [`TrackState`] per track is the **note tracker**: every note-on that
//! was sent is held here until its note-off is, so any event that must let
//! go (stop, a jump, a loop wrap, an edit, a change of instrument, a lost
//! port, Panic) can release exactly what sounds. Nothing here allocates:
//! the vectors are made at their full size on the UI side and never grow;
//! a full one drops the new event and counts it.
//!
//! Positions are worked out from ticks every block, through the timeline,
//! so tempo changes move notes with the music. An event belongs to the
//! block that holds `sample_at(tick)`; the window of ticks searched is
//! widened by two ticks either side so rounding cannot lose one.

use lumen_core::midi::{Event, Kind, Msg, cc};
use lumen_core::time::Ticks;

use super::live::LiveState;
use super::{MidiClip, MidiNote};
use crate::timeline::Timeline;

/// Most notes one track holds at once; past it, new notes are dropped.
pub(super) const MAX_HELD: usize = 128;
/// Most events one track queues for one part; past it, they are dropped.
pub(super) const MAX_EVENTS: usize = 1_024;
/// Ticks of margin around a block's window of ticks.
const MARGIN: i64 = 2;
/// How many repeats back a chase looks for a controller.
const CHASE_REPEATS: i64 = 64;

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
        }
    }

    /// Notes a key of the recording input going down or up.
    pub fn press(&mut self, source: u8, msg: Msg) {
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
    /// sounds under it nor ends it.
    pub fn yields(&self, key: u8) -> bool {
        self.recording && self.down.iter().any(|d| d.key == key)
    }

    /// Ends the clip notes sounding on `key` at `offset`: the recording
    /// input has just played it.
    pub fn give_way(&mut self, key: u8, offset: u32) {
        let mut i = 0;
        while i < self.held.len() {
            let h = self.held[i];
            if h.key == key {
                self.push(Event {
                    offset,
                    msg: Msg::note_off(h.channel, h.key),
                });
                self.held.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn push(&mut self, event: Event) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(event);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// Sends a note-on at `offset` and holds the note. A note already held
    /// on the key is released first.
    pub fn start_note(&mut self, held: Held, velocity: u8, offset: u32) {
        if let Some(i) = self
            .held
            .iter()
            .position(|h| h.channel == held.channel && h.key == held.key)
        {
            let old = self.held.swap_remove(i);
            self.push(Event {
                offset,
                msg: Msg::note_off(old.channel, old.key),
            });
        }
        if self.held.len() >= MAX_HELD {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.push(Event {
            offset,
            msg: Msg::note_on(held.channel, held.key, velocity),
        });
        self.held.push(held);
    }

    /// Sends a controller-like message, remembering the pedal and bend.
    pub fn send(&mut self, offset: u32, msg: Msg) {
        use lumen_core::midi::Kind;
        match msg.kind() {
            Kind::Controller { number, value } if number == cc::SUSTAIN => {
                self.sustain = value >= 64;
            }
            Kind::PitchBend { value } => self.bend = value != 0,
            _ => {}
        }
        self.push(Event { offset, msg });
    }

    /// Lets go of the live notes, pedal and bend at the start of the next
    /// part.
    pub fn release_live(&mut self) {
        let mut live = std::mem::replace(&mut self.live, LiveState::empty());
        live.release_all(|event| self.push(event));
        self.live = live;
    }

    /// Lets go of everything held, and of the pedal and the bend, at the
    /// start of the next part.
    pub fn release_all(&mut self, channel: u8) {
        for i in 0..self.held.len() {
            let h = self.held[i];
            self.push(Event {
                offset: 0,
                msg: Msg::note_off(h.channel, h.key),
            });
        }
        self.held.clear();
        if self.sustain {
            self.send(0, Msg::controller(channel, cc::SUSTAIN, 0));
        }
        if self.bend {
            self.send(0, Msg::pitch_bend(channel, 0));
        }
    }
}

/// Tick and sample conversions at the current tempo map and rate.
#[derive(Clone, Copy)]
pub(super) struct Clock<'a> {
    pub timeline: &'a Timeline,
    pub rate: u32,
}

impl Clock<'_> {
    fn sample(&self, tick: i64) -> i64 {
        self.timeline.sample_at(Ticks(tick), self.rate)
    }

    fn tick(&self, sample: i64) -> i64 {
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
/// `state.events` at their frame within the part.
pub(super) fn gather(
    clips: &[MidiClip],
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    (from, to): (i64, i64),
) {
    let lo = clock.tick(from) - MARGIN;
    let hi = clock.tick(to) + MARGIN;
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
                if (from..to).contains(&at) && !state.yields(note.pitch) {
                    state.start_note(
                        Held {
                            channel,
                            key: note.pitch,
                            start: Ticks(base + note.start.0),
                            on_sample: at,
                            off_tick: Ticks(off_tick(base, note, span)),
                        },
                        note.velocity,
                        (at - from) as u32,
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
            state.push(Event {
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
    clips: &[MidiClip],
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    pos: i64,
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
                state.push(Event {
                    offset: 0,
                    msg: Msg::note_off(h.channel, h.key),
                });
                state.held.swap_remove(i);
            }
        }
    }
    chase_notes(clips, state, channel, clock, pos);
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
    clips: &[MidiClip],
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    pos: i64,
) {
    let at = clock.tick(pos);
    for clip in clips {
        if clip.start.0 > at + MARGIN {
            break;
        }
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
            for note in &clip.notes[first..end] {
                if note.start.0 >= span {
                    break;
                }
                let on = clock.sample(base + note.start.0);
                let off_t = off_tick(base, note, span);
                let off = clock.sample(off_t).max(on + 1);
                let start = Ticks(base + note.start.0);
                if on < pos
                    && off > pos
                    && !state.yields(note.pitch)
                    && !state
                        .held
                        .iter()
                        .any(|h| h.start == start && h.key == note.pitch)
                {
                    state.start_note(
                        Held {
                            channel,
                            key: note.pitch,
                            start,
                            on_sample: on,
                            off_tick: Ticks(off_t),
                        },
                        note.velocity,
                        0,
                    );
                }
            }
        }
    }
}

/// Slots in the controller table: 128 controllers, bend, pressure, program.
const SLOTS: usize = 131;

fn slot(msg: Msg) -> Option<usize> {
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
    clips: &[MidiClip],
    state: &mut TrackState,
    channel: u8,
    clock: Clock<'_>,
    pos: i64,
) {
    let mut latest: [Option<(i64, Msg)>; SLOTS] = [None; SLOTS];
    let at = clock.tick(pos);
    for clip in clips {
        if clip.start.0 > at {
            break;
        }
        let period = period(clip);
        // A clip that has ended still leaves its controllers as they were.
        let count = ((clip.length.0 + period - 1) / period).max(1);
        let last = ((at - clip.start.0) / period).min(count - 1);
        for rep in (last - CHASE_REPEATS).max(0)..=last {
            let base = clip.start.0 + rep * period;
            let span = span(clip, period, rep);
            for control in clip.controls.iter() {
                if control.at.0 >= span {
                    break;
                }
                let tick = base + control.at.0;
                let Some(slot) = slot(control.msg) else {
                    continue;
                };
                if clock.sample(tick) < pos && latest[slot].is_none_or(|(t, _)| tick >= t) {
                    latest[slot] = Some((tick, control.msg));
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
/// sort: no allocation, and short lists.
pub(super) fn sort_events(events: &mut [Event]) {
    fn rank(msg: Msg) -> u8 {
        use lumen_core::midi::Kind;
        match msg.kind() {
            Kind::NoteOff { .. } => 0,
            Kind::NoteOn { .. } => 2,
            _ => 1,
        }
    }
    for i in 1..events.len() {
        let mut j = i;
        while j > 0
            && (events[j - 1].offset, rank(events[j - 1].msg))
                > (events[j].offset, rank(events[j].msg))
        {
            events.swap(j - 1, j);
            j -= 1;
        }
    }
}
