//! The pair that plays MIDI: [`MidiHandle`] on the UI side replaces the
//! track list, [`MidiPlayer`] on the audio thread reads it (a
//! [`TrackSource`], played beside the audio clips).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::capture::{self, Captured};
use super::clock::ClockOut;
use super::hold::{self, Clock, MAX_EVENTS};
use super::live::{AUDITION, LiveIn, LiveInput};
use super::recording::{live_position, record_window};
use super::tracks::{gather_track, play_track, preview_instrument, start_host};
use super::{MidiOut, MidiTrack, Target};
use crate::mixer::{Buses, TrackSource};
use crate::source::{Jump, Mode, RenderContext};
use crate::timeline::Timeline;
use lumen_core::midi::{Event, Kind, Msg, cc};

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

    /// Moves each plugin host this list holds into the same track's
    /// stand-in in `newer` (same id, same kind, an instrument that has no
    /// host), for a list that is skipped and freed unplayed. Real-time safe.
    fn pass_hosts_to(&mut self, newer: &mut MidiSet) {
        for track in &mut self.tracks {
            let Some(mine) = track.instrument.as_mut().filter(|s| s.instrument.hosted()) else {
                continue;
            };
            let Ok(k) = newer.by_id.binary_search_by_key(&track.id, |&(id, _)| id) else {
                continue;
            };
            let at = newer.by_id[k].1 as usize;
            if let Some(theirs) = newer.tracks[at].instrument.as_mut()
                && theirs.kind == mine.kind
                && !theirs.instrument.hosted()
            {
                std::mem::swap(&mut mine.instrument, &mut theirs.instrument);
            }
        }
    }

    /// Some track's instrument runs in a plugin host.
    fn hosted(&self) -> bool {
        self.tracks.iter().any(|t| {
            t.instrument
                .as_ref()
                .is_some_and(|spec| spec.instrument.hosted())
        })
    }
}

/// A live input on its way to the audio thread, or back to be freed.
type Feed = Box<dyn LiveInput>;

/// UI side: replaces the tracks and frees old lists. May allocate.
pub struct MidiHandle {
    incoming: rtrb::Producer<Box<MidiSet>>,
    retired: rtrb::Consumer<Box<MidiSet>>,
    output: Option<rtrb::Consumer<MidiOut>>,
    feeds: rtrb::Producer<Feed>,
    old_feeds: rtrb::Consumer<Feed>,
    captured: Option<rtrb::Consumer<Captured>>,
    sync: Arc<AtomicU64>,
    /// A list with a hosted instrument waits for the audio side.
    hosted_pending: Arc<AtomicBool>,
}

impl MidiHandle {
    /// The ports that get MIDI clock (M5 slice 7): bit `n` for port `n`,
    /// ports 0 to 63.
    pub fn set_sync(&self, ports: u64) {
        self.sync.store(ports, Ordering::Relaxed);
    }

    /// Sends a new track list. Returns it if the queue is full (call again
    /// next frame).
    pub fn set(&mut self, tracks: Vec<MidiTrack>) -> Result<(), Vec<MidiTrack>> {
        let set = MidiSet::new(tracks);
        let hosted = set.hosted();
        self.incoming
            .push(Box::new(set))
            .map(|()| {
                if hosted {
                    self.hosted_pending.store(true, Ordering::Release);
                }
            })
            .map_err(|rtrb::PushError::Full(set)| set.tracks)
    }

    /// Frees lists the audio side has replaced. Call regularly.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        while self.old_feeds.pop().is_ok() {
            freed += 1;
        }
        freed
    }

    /// Gives the audio side the live input to play (replacing any before
    /// it). Returns it if the queue is full (call again later).
    pub fn set_live(&mut self, feed: Box<dyn LiveInput>) -> Result<(), Box<dyn LiveInput>> {
        self.feeds
            .push(feed)
            .map_err(|rtrb::PushError::Full(feed)| feed)
    }

    /// The messages recording tracks kept, once: the app's recorder takes
    /// them (M5 slice 6).
    pub fn take_captured(&mut self) -> Option<rtrb::Consumer<Captured>> {
        self.captured.take()
    }

    /// The messages for hardware, once: the app's sender takes them.
    pub fn take_output(&mut self) -> Option<rtrb::Consumer<MidiOut>> {
        self.output.take()
    }
}

/// Where messages for hardware go.
pub(super) struct Out {
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
    pub(super) fn forward(&mut self, track: &mut MidiTrack, frame: u64) {
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
    feeds: rtrb::Consumer<Feed>,
    old_feeds: rtrb::Producer<Feed>,
    /// The live input, if the app gave one.
    live: Option<Feed>,
    /// A replaced live input that did not fit in its queue back yet.
    old_feed: Option<Feed>,
    /// Where recorded messages go.
    capture: rtrb::Producer<Captured>,
    /// Messages lost because the app was not reading (a count, for tests).
    lost: u32,
    /// How many times the loop has wrapped: the pass messages are kept in.
    pass: u32,
    /// A loop has wrapped since the last play, locate, or reposition.
    /// Delayed live input before the loop start belongs to the previous pass
    /// only while this is true.
    wrapped_since_locate: bool,
    /// The last part rendered had the transport recording.
    recording: bool,
    /// The ports that get MIDI clock, as [`MidiHandle::set_sync`] set them.
    sync: Arc<AtomicU64>,
    /// The ports the clock was last sent to.
    synced: u64,
    clock_out: ClockOut,
    /// Some track's instrument runs in a plugin host.
    hosted: bool,
    /// A list with a hosted instrument is waiting (the handle sets it).
    hosted_pending: Arc<AtomicBool>,
    /// [`TrackSource::begin`] has done the part's first half: the next
    /// `render` plays the tracks it gathered.
    prepared: bool,
}

/// Creates the MIDI pair, starting with no tracks. Allocates, so call it
/// off the audio thread.
pub fn midi_player() -> (MidiHandle, MidiPlayer) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    let (out_tx, out_rx) = rtrb::RingBuffer::new(OUT_QUEUE);
    let (feeds_tx, feeds_rx) = rtrb::RingBuffer::new(2);
    let (old_feeds_tx, old_feeds_rx) = rtrb::RingBuffer::new(3);
    let (capture_tx, capture_rx) = rtrb::RingBuffer::new(capture::QUEUE);
    let sync = Arc::new(AtomicU64::new(0));
    let hosted_pending = Arc::new(AtomicBool::new(false));
    (
        MidiHandle {
            incoming: incoming_tx,
            retired: retired_rx,
            output: Some(out_rx),
            feeds: feeds_tx,
            old_feeds: old_feeds_rx,
            captured: Some(capture_rx),
            sync: Arc::clone(&sync),
            hosted_pending: Arc::clone(&hosted_pending),
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
            feeds: feeds_rx,
            old_feeds: old_feeds_tx,
            live: None,
            old_feed: None,
            capture: capture_tx,
            lost: 0,
            pass: 0,
            wrapped_since_locate: false,
            recording: false,
            sync,
            synced: 0,
            clock_out: ClockOut::default(),
            hosted: false,
            hosted_pending,
            prepared: false,
        },
    )
}

impl MidiPlayer {
    /// Events and notes dropped for lack of room so far (a full track
    /// queue, a full output queue). Zero in any sane project.
    pub fn dropped(&self) -> u32 {
        let tracks: u32 = self.current.tracks.iter().map(|t| t.state.dropped).sum();
        tracks
            .saturating_add(self.out.dropped)
            .saturating_add(self.lost)
    }

    /// Notes held on a track now (tests).
    pub fn held(&self, track: u64) -> usize {
        self.current
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map_or(0, |t| t.state.held.len())
    }

    /// Live notes held on a track now (tests).
    pub fn held_live(&self, track: u64) -> usize {
        self.current
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map_or(0, |t| t.state.live.len())
    }

    /// Takes a new live input if one arrived, handing the old one back.
    /// Real-time safe.
    fn take_feed(&mut self) {
        if let Some(old) = self.old_feed.take()
            && let Err(rtrb::PushError::Full(old)) = self.old_feeds.push(old)
        {
            self.old_feed = Some(old);
            return;
        }
        if let Ok(feed) = self.feeds.pop()
            && let Some(old) = self.live.replace(feed)
            && let Err(rtrb::PushError::Full(old)) = self.old_feeds.push(old)
        {
            self.old_feed = Some(old);
        }
    }

    /// Hands each track that listens the live messages for the part from
    /// device frame `start`, `frames` long.
    fn play_live(&mut self, cx: &RenderContext<'_>, frames: usize) {
        self.recording = cx.recording;
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let start = cx.device;
        let pass = self.pass;
        let tracks = &mut self.current.tracks;
        let (capture, lost) = (&mut self.capture, &mut self.lost);
        // A recording that begins takes the keys already held down, as
        // played at its start, so a held note is not lost.
        let clock = Clock {
            timeline: cx.timeline,
            rate: cx.sample_rate,
        };
        for track in tracks.iter_mut() {
            let on = cx.recording && track.record.is_on();
            if on && !track.state.recording {
                track.state.record_pass = pass;
            }
            track.state.window = record_window(track, pass);
            if on && !track.state.recording {
                let at = capture::heard(cx, start, self.wrapped_since_locate).0;
                let (position, position_pass) = live_position(track, clock, at, pass, false);
                // Keys already down win their pitch only where the take
                // shows them.
                let window = record_window(track, position_pass);
                let wins = on && (window.0..window.1).contains(&position);
                for down in &mut track.state.down {
                    down.wins = wins;
                    let msg = Msg::note_on(down.channel, down.key, down.velocity);
                    let kept = capture.push(Captured {
                        track: track.id,
                        source: down.source,
                        at,
                        pass,
                        msg,
                    });
                    if kept.is_err() {
                        *lost = lost.saturating_add(1);
                    }
                }
                // And the clip's notes on those keys give way to them.
                for i in 0..track.state.down.len() {
                    let key = track.state.down[i].key;
                    if wins {
                        track.state.give_way(key, 0);
                    }
                }
            }
            track.state.recording = on;
        }
        let wrapped = self.wrapped_since_locate;
        let last = frames.saturating_sub(1) as u64;
        let last32 = last as u32;
        // The frame the previous live message ended up on: messages taken in
        // one block go no earlier than it, so their order is kept. A
        // note-off is placed after its note-on (`apart_from_its_on`), and a
        // later note-on left on the frame it came with would pass it.
        let mut floor = 0_u32;
        live.pull(start, start + frames as u64, &mut |m: LiveIn| {
            let offset = (m.frame.saturating_sub(start).min(last) as u32).max(floor);
            for track in tracks.iter_mut() {
                if m.source != AUDITION && track.record.takes(m.source, m.msg) {
                    let recording = cx.recording && track.record.is_on();
                    let (at, back) = capture::heard(cx, m.played, wrapped);
                    // A key played while recording, where the take shows
                    // it, wins its pitch: a clip note sounding on it ends
                    // here (`TrackState::yields`). Outside the window the
                    // take keeps it hidden, and the clips play on.
                    let (position, position_pass) = live_position(track, clock, at, pass, back);
                    let window = record_window(track, position_pass);
                    let wins = recording && (window.0..window.1).contains(&position);
                    track.state.press(m.source, m.msg, wins);
                    if let Kind::NoteOn { key, .. } = m.msg.kind()
                        && wins
                    {
                        track.state.give_way(key, offset);
                    }
                    // Recording keeps what the track takes, heard or not.
                    if cx.recording {
                        let kept = capture.push(Captured {
                            track: track.id,
                            source: m.source,
                            at,
                            pass: pass.wrapping_sub(u32::from(back)),
                            msg: m.msg,
                        });
                        if kept.is_err() {
                            *lost = lost.saturating_add(1);
                        }
                    }
                }
                if !track.sounds() {
                    continue;
                }
                let heard = if m.source == AUDITION {
                    m.track == Some(track.id) && m.msg.0[0] & 0xF0 != 0xF0
                } else {
                    track.listen.takes(m.source, m.msg)
                };
                if !heard {
                    continue;
                }
                let out = match track.target {
                    Target::Port { channel, .. } => Some(channel),
                    Target::Instrument => None,
                };
                if let Some(msg) = track.state.live.take(m.source, m.msg, out) {
                    let offset = apart_from_its_on(&mut track.state.events, msg, offset, last32);
                    floor = floor.max(offset);
                    track.state.push(Event { offset, msg });
                }
            }
        });
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
        // Skipping a list needs room for it and for the one it replaces
        // in the end, or the stash would be overwritten and a list freed
        // here; newer lists then wait a block.
        while self.retired.slots() >= 2
            && let Ok(mut newer) = self.incoming.pop()
        {
            // A host the skipped list brought is the only handle to a
            // running process: it goes to the newer list's stand-in.
            new.pass_hosts_to(&mut newer);
            self.retire(std::mem::replace(&mut new, newer));
        }
        self.carry(&mut new, at);
        let old = std::mem::replace(&mut self.current, new);
        self.hosted = self.current.hosted();
        if self.incoming.is_empty() {
            self.hosted_pending.store(false, Ordering::Relaxed);
        }
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
    /// voices carry by track id, notes that an edit removed or moved are
    /// released (`hold::sync`),
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
                track.state.release_live();
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
            // Live notes let go when the track stops listening or listens
            // to something else (here, or on the old output if rerouted).
            if rerouted || track.listen != before.listen {
                before.state.release_live();
            }
            if rerouted {
                // What sounded on the old output lets go there.
                let channel = before.channel();
                before.state.release_all(channel);
                match before.target {
                    Target::Port { .. } => self.out.forward(before, clock),
                    Target::Instrument => {
                        if same_kind && let Some(spec) = before.instrument.as_mut() {
                            let events = std::mem::take(&mut before.state.events);
                            spec.instrument.render(0, &events, &mut [], &mut []);
                            before.state.events = events;
                        }
                    }
                }
                before.state.events.clear();
            }
            std::mem::swap(&mut track.state, &mut before.state);
            // The instrument the old list had plays on, except a host the
            // new list brings: that one is attached now, and the old list's
            // (a stand-in, or the host it replaces) goes with the old list.
            if same_kind
                && let (Some(a), Some(b)) = (track.instrument.as_mut(), before.instrument.as_mut())
                && !a.instrument.hosted()
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
                    hold::sync(
                        (&track.clips, &track.index),
                        &mut track.state,
                        channel,
                        clock,
                        pos,
                    );
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

impl MidiPlayer {
    /// A part's first half: takes new lists and live input, places the live
    /// and clock messages, reads every track's clips and, with `hosts`, hands
    /// each hosted instrument its block. False for a scrub, which plays no
    /// MIDI.
    fn prepare(
        &mut self,
        cx: &RenderContext<'_>,
        frames: usize,
        clock: Clock<'_>,
        hosts: bool,
    ) -> bool {
        self.take_new((cx.mode == Mode::Play).then_some((clock, cx.start)));
        self.take_feed();
        self.clock = cx.stream + frames as u64;
        if cx.mode == Mode::Scrub {
            // Live input waits until the scrub ends.
            return false;
        }
        self.play_live(cx, frames);
        if cx.mode == Mode::Play {
            self.play_clock(cx, frames);
        }
        for track in &mut self.current.tracks {
            gather_track(track, cx, frames, clock);
            if hosts {
                start_host(track, cx, frames);
            }
        }
        true
    }

    /// The loop wrapped: the next messages are of a new pass, and each
    /// recording track is told so.
    fn wrapped(&mut self) {
        self.pass = self.pass.wrapping_add(1);
        if !self.recording {
            return;
        }
        for track in &self.current.tracks {
            if !track.record.is_on() {
                continue;
            }
            let marker = Captured {
                track: track.id,
                source: capture::WRAP,
                at: 0,
                pass: self.pass,
                msg: Msg([0, 0, 0]),
            };
            if self.capture.push(marker).is_err() {
                self.lost = self.lost.saturating_add(1);
            }
        }
    }

    /// Tells the sender to forget what it has not sent yet, if any track
    /// goes to hardware or any port gets the clock.
    fn flush_hardware(&mut self) {
        let ports = self
            .current
            .tracks
            .iter()
            .any(|t| matches!(t.target, Target::Port { .. }));
        if ports || self.synced != 0 {
            self.out.push(MidiOut::Flush);
        }
    }

    /// Sends `msg` to every port in `ports` at device frame `frame`.
    fn to_synced(out: &mut Out, ports: u64, frame: u64, msg: Msg) {
        let mut left = ports;
        while left != 0 {
            let port = left.trailing_zeros() as u16;
            out.at(frame, port, msg);
            left &= left - 1;
        }
    }

    /// The clock's messages for a part being played (M5 slice 7). Ports
    /// that turn Sync on while playing pick the song up from here.
    fn play_clock(&mut self, cx: &RenderContext<'_>, frames: usize) {
        let ports = self.sync.load(Ordering::Relaxed);
        if ports != self.synced {
            if ports == 0 {
                self.clock_out.reset();
            } else if self.clock_out.running() || self.synced == 0 {
                self.clock_out.locate(Jump::Play);
            }
            self.synced = ports;
        }
        if ports == 0 {
            return;
        }
        let (out, stream) = (&mut self.out, cx.stream);
        self.clock_out.part(
            cx.timeline,
            cx.sample_rate,
            cx.start,
            frames,
            |offset, msg| {
                Self::to_synced(out, ports, stream + u64::from(offset), msg);
            },
        );
    }
}

/// Where a live note-off goes. Events on one frame are ordered offs before
/// ons (so a retriggered note ends before it starts), which would leave a
/// note that was played and let go within one frame sounding: its off is
/// placed one frame after its on (or the on one frame before, at the end of
/// the part).
fn apart_from_its_on(events: &mut [Event], msg: Msg, offset: u32, last: u32) -> u32 {
    let lumen_core::midi::Kind::NoteOff { key, .. } = msg.kind() else {
        return offset;
    };
    let on = events.iter_mut().rev().find(|e| {
        e.offset >= offset
            && e.msg.channel() == msg.channel()
            && matches!(e.msg.kind(), lumen_core::midi::Kind::NoteOn { key: k, .. } if k == key)
    });
    let Some(on) = on else {
        return offset;
    };
    if on.offset < last {
        on.offset + 1
    } else {
        on.offset = last.saturating_sub(1);
        last
    }
}

impl MidiTrack {
    /// Whether anything is listening: an instrument, or hardware.
    pub(super) fn sounds(&self) -> bool {
        matches!(self.target, Target::Port { .. }) || self.instrument.is_some()
    }
}

impl TrackSource for MidiPlayer {
    fn render(&mut self, cx: &RenderContext<'_>, buses: &mut Buses<'_>) {
        if cx.tail {
            if !cx.stopping {
                // Ahead of a jump: what rings goes on, faded out by the
                // engine while the new position fades in.
                let frames = buses.frames();
                for track in &mut self.current.tracks {
                    preview_instrument(track, track.id, (frames, cx.sample_rate), buses);
                }
            }
            return;
        }
        let clock = Clock {
            timeline: cx.timeline,
            rate: cx.sample_rate,
        };
        // `begin` may have done the first half already.
        if !std::mem::take(&mut self.prepared) && !self.prepare(cx, buses.frames(), clock, false) {
            return;
        }
        for track in &mut self.current.tracks {
            play_track(track, cx, buses, &mut self.out);
        }
    }

    fn hosted(&self) -> bool {
        // A list waiting to be taken counts, so the callback that takes it
        // already has its deadline.
        self.hosted || self.hosted_pending.load(Ordering::Relaxed)
    }

    fn begin(&mut self, cx: &RenderContext<'_>, frames: usize) {
        if !self.hosted || cx.tail || cx.mode == Mode::Scrub {
            return;
        }
        let clock = Clock {
            timeline: cx.timeline,
            rate: cx.sample_rate,
        };
        self.prepared = self.prepare(cx, frames, clock, true);
    }

    fn locate(&mut self, timeline: &Timeline, sample_rate: u32, sample: i64, jump: Jump) {
        self.take_new(None);
        if jump == Jump::LoopWrap {
            self.wrapped();
        }
        self.wrapped_since_locate = jump == Jump::LoopWrap;
        self.clock_out.locate(jump);
        let clock = Clock {
            timeline,
            rate: sample_rate,
        };
        for track in &mut self.current.tracks {
            let channel = track.channel();
            track.state.release_all(channel);
            if track.sounds() {
                hold::chase_controls(
                    (&track.clips, &track.index),
                    &mut track.state,
                    channel,
                    clock,
                    sample,
                );
                hold::chase_notes(
                    (&track.clips, &track.index),
                    &mut track.state,
                    channel,
                    clock,
                    sample,
                );
            }
        }
    }

    fn idle(&mut self) {
        self.take_new(None);
        self.take_feed();
    }

    fn stopped(&mut self) {
        self.flush_hardware();
        if self.clock_out.stop() {
            Self::to_synced(&mut self.out, self.synced, self.clock, Msg::STOP);
        }
        for track in &mut self.current.tracks {
            let channel = track.channel();
            track.state.release_all(channel);
        }
    }

    fn panic(&mut self) {
        self.flush_hardware();
        let clock = self.clock;
        let mut seen = [u16::MAX; 32];
        let mut count = 0;
        for track in &mut self.current.tracks {
            let state = &mut track.state;
            state.held.clear();
            state.events.clear();
            state.sustain = false;
            state.bend = false;
            state.live.clear();
            state.down.clear();
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
        self.live.as_ref().is_some_and(|l| l.waiting())
            || self.current.tracks.iter().any(|t| {
                !t.state.events.is_empty()
                    || t.instrument.as_ref().is_some_and(|i| i.instrument.active())
            })
    }
}

const _: () = assert!(MAX_EVENTS >= 512, "a panic and a chase need room");
