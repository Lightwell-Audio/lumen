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

use lumen_core::midi::{Event, Msg, cc};
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

/// Where a sounding note comes from, so an edit can tell whether the note
/// still exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Owner {
    pub clip: u64,
    pub clip_start: Ticks,
    pub rep: u32,
    pub start: Ticks,
    pub length: Ticks,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Held {
    pub channel: u8,
    pub key: u8,
    pub owner: Owner,
    pub on_sample: i64,
    /// Absolute tick of the note-off.
    pub off_tick: Ticks,
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

fn owner(clip: &MidiClip, rep: i64, note: &MidiNote) -> Owner {
    Owner {
        clip: clip.id,
        clip_start: clip.start,
        rep: rep as u32,
        start: note.start,
        length: note.length,
    }
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
                if (from..to).contains(&at) {
                    state.start_note(
                        Held {
                            channel,
                            key: note.pitch,
                            owner: owner(clip, rep, note),
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

/// Where `held` ends now, if its note is still in `clips`.
fn renewed(clips: &[MidiClip], held: &Held) -> Option<i64> {
    let o = held.owner;
    let clip = clips
        .iter()
        .find(|c| c.id == o.clip && c.start == o.clip_start)?;
    let period = period(clip);
    let count = (clip.length.0 + period - 1) / period;
    if i64::from(o.rep) >= count {
        return None;
    }
    let note = clip
        .notes
        .iter()
        .find(|n| n.start == o.start && n.length == o.length && n.pitch == held.key)?;
    let base = clip.start.0 + i64::from(o.rep) * period;
    let span = span(clip, period, i64::from(o.rep));
    (note.start.0 < span).then(|| off_tick(base, note, span))
}

/// Starts the notes that sound at `pos` (began before it, end after it)
/// and are not held.
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
            for note in &clip.notes[..end] {
                if note.start.0 >= span {
                    break;
                }
                let on = clock.sample(base + note.start.0);
                let off_t = off_tick(base, note, span);
                let off = clock.sample(off_t).max(on + 1);
                let owner = owner(clip, rep, note);
                if on < pos
                    && off > pos
                    && !state
                        .held
                        .iter()
                        .any(|h| h.owner == owner && h.key == note.pitch)
                {
                    state.start_note(
                        Held {
                            channel,
                            key: note.pitch,
                            owner,
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
