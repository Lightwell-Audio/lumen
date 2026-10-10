//! The pair that plays MIDI: [`MidiHandle`] on the UI side replaces the
//! track list, [`MidiPlayer`] on the audio thread reads it (a
//! [`TrackSource`], played beside the audio clips).

use lumen_core::midi::{Event, Msg, cc};

use super::hold::{self, Clock, MAX_EVENTS};
use super::{MidiOut, MidiTrack, Target};
use crate::mixer::{Buses, CHUNK, TrackSource};
use crate::source::{Jump, Mode, RenderContext};
use crate::timeline::Timeline;

const QUEUE: usize = 4;
/// Messages waiting for the app's sender.
const OUT_QUEUE: usize = 8_192;

/// A track list, sorted by id for carrying state into the next.
struct MidiSet {
    tracks: Vec<MidiTrack>,
    /// `(id, index)` sorted by id.
    by_id: Vec<(u64, u32)>,
}

impl MidiSet {
    fn new(tracks: Vec<MidiTrack>) -> Self {
        let mut by_id: Vec<(u64, u32)> = tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i as u32))
            .collect();
        by_id.sort_unstable();
        Self { tracks, by_id }
    }
}

/// UI side: replaces the tracks and frees old lists. May allocate.
pub struct MidiHandle {
    incoming: rtrb::Producer<Box<MidiSet>>,
    retired: rtrb::Consumer<Box<MidiSet>>,
    output: Option<rtrb::Consumer<MidiOut>>,
}

impl MidiHandle {
    /// Sends a new track list. Returns it if the queue is full (call again
    /// next frame).
    pub fn set(&mut self, tracks: Vec<MidiTrack>) -> Result<(), Vec<MidiTrack>> {
        self.incoming
            .push(Box::new(MidiSet::new(tracks)))
            .map_err(|rtrb::PushError::Full(set)| set.tracks)
    }

    /// Frees lists the audio side has replaced. Call regularly.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }

    /// The messages for hardware, once: the app's sender takes them.
    pub fn take_output(&mut self) -> Option<rtrb::Consumer<MidiOut>> {
        self.output.take()
    }
}

/// Where messages for hardware go.
struct Out {
    tx: rtrb::Producer<MidiOut>,
    dropped: u32,
}

impl Out {
    fn push(&mut self, message: MidiOut) {
        if self.tx.push(message).is_err() {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    fn at(&mut self, frame: u64, port: u16, msg: Msg) {
        self.push(MidiOut::At { frame, port, msg });
    }

    /// Sends a hardware track's queued events from device frame `frame`.
    fn forward(&mut self, track: &mut MidiTrack, frame: u64) {
        if let Target::Port { port, .. } = track.target {
            for event in track.state.events.drain(..) {
                self.at(frame + u64::from(event.offset), port, event.msg);
            }
        }
    }
}

/// Audio side: plays the MIDI clips. A [`TrackSource`].
pub struct MidiPlayer {
    incoming: rtrb::Consumer<Box<MidiSet>>,
    retired: rtrb::Producer<Box<MidiSet>>,
    /// A replaced list that did not fit in the retired queue yet.
    stash: Option<Box<MidiSet>>,
    current: Box<MidiSet>,
    out: Out,
    /// The device frame at the end of the last part rendered: when to
    /// stamp messages sent outside a part.
    clock: u64,
}

/// Creates the MIDI pair, starting with no tracks. Allocates, so call it
/// off the audio thread.
pub fn midi_player() -> (MidiHandle, MidiPlayer) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    let (out_tx, out_rx) = rtrb::RingBuffer::new(OUT_QUEUE);
    (
        MidiHandle {
            incoming: incoming_tx,
            retired: retired_rx,
            output: Some(out_rx),
        },
        MidiPlayer {
            incoming: incoming_rx,
            retired: retired_tx,
            stash: None,
            current: Box::new(MidiSet::new(Vec::new())),
            out: Out {
                tx: out_tx,
                dropped: 0,
            },
            clock: 0,
        },
    )
}

impl MidiPlayer {
    /// Events and notes dropped for lack of room so far (a full track
    /// queue, a full output queue). Zero in any sane project.
    pub fn dropped(&self) -> u32 {
        let tracks: u32 = self.current.tracks.iter().map(|t| t.state.dropped).sum();
        tracks.saturating_add(self.out.dropped)
    }

    /// Notes held on a track now (tests).
    pub fn held(&self, track: u64) -> usize {
        self.current
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map_or(0, |t| t.state.held.len())
    }

    /// Takes the newest track list if one arrived. With `at` (the clock and
    /// the position being played), notes are brought in line with it.
    /// Real-time safe.
    fn take_new(&mut self, at: Option<(Clock<'_>, i64)>) {
        if let Some(old) = self.stash.take() {
            self.retire(old);
            if self.stash.is_some() {
                return;
            }
        }
        let Ok(mut new) = self.incoming.pop() else {
            return;
        };
        // Only the newest list matters.
        while self.stash.is_none()
            && let Ok(newer) = self.incoming.pop()
        {
            self.retire(std::mem::replace(&mut new, newer));
        }
        self.carry(&mut new, at);
        let old = std::mem::replace(&mut self.current, new);
        self.retire(old);
    }

    /// Hands `old` back to be freed, keeping it if the queue is full. Only
    /// called while the stash is empty.
    fn retire(&mut self, old: Box<MidiSet>) {
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
    }

    /// Moves what sounds from the old list into the new: trackers and
    /// voices carry by track id, notes that an edit changed are released,
    /// and a track that left lets go of its notes.
    fn carry(&mut self, new: &mut MidiSet, at: Option<(Clock<'_>, i64)>) {
        let clock = self.clock;
        let old = &mut *self.current;
        // Tracks that went away let go of their hardware notes.
        for track in &mut old.tracks {
            if new
                .by_id
                .binary_search_by_key(&track.id, |&(id, _)| id)
                .is_err()
            {
                let channel = track.channel();
                track.state.release_all(channel);
                self.out.forward(track, clock);
            }
        }
        for track in new.tracks.iter_mut() {
            let found = old
                .by_id
                .binary_search_by_key(&track.id, |&(id, _)| id)
                .ok()
                .map(|k| old.by_id[k].1 as usize);
            let Some(j) = found else {
                track.dirty = true;
                continue;
            };
            let before = &mut old.tracks[j];
            let same_kind = matches!(
                (&track.instrument, &before.instrument),
                (Some(a), Some(b)) if a.kind == b.kind
            );
            let rerouted = track.target != before.target || !same_kind;
            if rerouted {
                // What sounded on the old output lets go there.
                let channel = before.channel();
                before.state.release_all(channel);
                match before.target {
                    Target::Port { .. } => self.out.forward(before, clock),
                    Target::Instrument => {
                        if same_kind && let Some(spec) = before.instrument.as_mut() {
                            let events = std::mem::take(&mut before.state.events);
                            spec.instrument.render(&events, &mut [], &mut []);
                            before.state.events = events;
                        }
                    }
                }
                before.state.events.clear();
            }
            std::mem::swap(&mut track.state, &mut before.state);
            if same_kind
                && let (Some(a), Some(b)) = (track.instrument.as_mut(), before.instrument.as_mut())
            {
                std::mem::swap(&mut a.instrument, &mut b.instrument);
                if a.preset != b.preset {
                    a.instrument.configure(a.preset);
                }
            }
            track.dirty = !track.same_as(before);
        }
        // Notes in line with the new data, at the position being played.
        if let Some((clock, pos)) = at {
            for track in &mut new.tracks {
                if track.dirty && track.sounds() {
                    let channel = track.channel();
                    hold::sync(&track.clips, &mut track.state, channel, clock, pos);
                }
                track.dirty = false;
            }
        } else {
            for track in &mut new.tracks {
                track.dirty = false;
            }
        }
    }
}

impl MidiTrack {
    /// Whether anything is listening: an instrument, or hardware.
    fn sounds(&self) -> bool {
        matches!(self.target, Target::Port { .. }) || self.instrument.is_some()
    }
}

/// Plays one track's part: reads its clips, sends hardware events out, or
/// renders the instrument into the track's bus.
fn run_track(
    track: &mut MidiTrack,
    cx: &RenderContext<'_>,
    buses: &mut Buses<'_>,
    out: &mut Out,
    clock: Clock<'_>,
) {
    let frames = buses.frames();
    let channel = track.channel();
    let id = track.id;
    if cx.mode == Mode::Play && track.sounds() {
        let from = cx.start;
        hold::gather(
            &track.clips,
            &mut track.state,
            channel,
            clock,
            (from, from + frames as i64),
        );
    }
    hold::sort_events(&mut track.state.events);
    match track.target {
        Target::Port { .. } => {
            out.forward(track, cx.stream);
            render_instrument(track, id, frames, buses, false);
        }
        Target::Instrument => render_instrument(track, id, frames, buses, true),
    }
}

/// Renders the track's instrument into its bus: with the queued events when
/// `events`, otherwise only what still rings.
fn render_instrument(
    track: &mut MidiTrack,
    id: u64,
    frames: usize,
    buses: &mut Buses<'_>,
    events: bool,
) {
    let queued: &[Event] = if events { &track.state.events } else { &[] };
    if let Some(spec) = track.instrument.as_mut()
        && (!queued.is_empty() || spec.instrument.active())
    {
        let mut left = [0.0f32; CHUNK];
        let mut right = [0.0f32; CHUNK];
        spec.instrument
            .render(queued, &mut left[..frames], &mut right[..frames]);
        if let Some(mut bus) = buses.track(id) {
            for (side, samples) in [(0, &left), (1, &right)] {
                for (to, from) in bus.channel(side).iter_mut().zip(samples) {
                    *to += from;
                }
            }
        }
    }
    if events {
        track.state.events.clear();
    }
}

impl TrackSource for MidiPlayer {
    fn render(&mut self, cx: &RenderContext<'_>, buses: &mut Buses<'_>) {
        if cx.tail {
            return;
        }
        let clock = Clock {
            timeline: cx.timeline,
            rate: cx.sample_rate,
        };
        self.take_new((cx.mode == Mode::Play).then_some((clock, cx.start)));
        self.clock = cx.stream + buses.frames() as u64;
        if cx.mode == Mode::Scrub {
            return;
        }
        for track in &mut self.current.tracks {
            run_track(track, cx, buses, &mut self.out, clock);
        }
    }

    fn locate(&mut self, timeline: &Timeline, sample_rate: u32, sample: i64, _jump: Jump) {
        self.take_new(None);
        let clock = Clock {
            timeline,
            rate: sample_rate,
        };
        for track in &mut self.current.tracks {
            let channel = track.channel();
            track.state.release_all(channel);
            if track.sounds() {
                hold::chase_controls(&track.clips, &mut track.state, channel, clock, sample);
                hold::chase_notes(&track.clips, &mut track.state, channel, clock, sample);
            }
        }
    }

    fn idle(&mut self) {
        self.take_new(None);
    }

    fn stopped(&mut self) {
        self.out.push(MidiOut::Flush);
        for track in &mut self.current.tracks {
            let channel = track.channel();
            track.state.release_all(channel);
        }
    }

    fn panic(&mut self) {
        self.out.push(MidiOut::Flush);
        let clock = self.clock;
        let mut seen = [u16::MAX; 32];
        let mut count = 0;
        for track in &mut self.current.tracks {
            let state = &mut track.state;
            state.held.clear();
            state.events.clear();
            state.sustain = false;
            state.bend = false;
            if let Some(spec) = track.instrument.as_mut() {
                spec.instrument.silence();
            }
            if let Target::Port { port, .. } = track.target
                && !seen[..count].contains(&port)
            {
                if count < seen.len() {
                    seen[count] = port;
                    count += 1;
                }
                for channel in 0..16 {
                    self.out
                        .at(clock, port, Msg::controller(channel, cc::ALL_NOTES_OFF, 0));
                    self.out
                        .at(clock, port, Msg::controller(channel, cc::ALL_SOUND_OFF, 0));
                }
            }
        }
    }

    fn ringing(&self) -> bool {
        self.current.tracks.iter().any(|t| {
            !t.state.events.is_empty()
                || t.instrument.as_ref().is_some_and(|i| i.instrument.active())
        })
    }
}

const _: () = assert!(MAX_EVENTS >= 512, "a panic and a chase need room");
