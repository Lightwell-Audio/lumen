//! MIDI out to hardware (M5 slice 1, ADR-0045).
//!
//! A MIDI track set to a port sends its notes there instead of playing an
//! instrument. The engine stamps each message with the device frame at which
//! it should be heard (`RenderContext::stream`: the block, the frame in it,
//! and the delay compensation's read ahead), and puts it on a lock-free
//! queue. A thread here turns frames into wall-clock time and sends each
//! message on time through `lumen-midi`:
//!
//! - the audio callback publishes `(frame, time)` at the start of every
//!   block ([`StreamClock`]); callbacks only ever run late, so the thread
//!   takes the earliest `time - frame / rate` seen lately as the time of
//!   frame 0 ([`ClockMap`]) and adds the device's output latency, so the
//!   hardware sounds when the audio of the same frame does;
//! - messages wait in a [`Scheduler`], which also knows which notes it has
//!   sent on, so a stop (a `Flush` from the engine), a port that went away
//!   or a track that changed port can let go of exactly those.
//!
//! - MIDI clock (M5 slice 7) comes from the engine like any message; a
//!   port's Sync delay moves its clock and transport messages (not its
//!   notes) later or earlier ([`Order::Delays`]). An earlier one can only
//!   be as early as the output latency: the thread cannot send before it
//!   is told. A port left running when the stream stands still or the
//!   sender ends gets Stop.
//!
//! Timing against real hardware is by construction, not measured here (no
//! interface with a MIDI port is on the test machine); the scheduling and
//! the clock mapping are tested with injected times.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lumen_core::midi::{Kind, Msg};
use lumen_engine::MidiOut;
use lumen_midi::{Direction, MidiSender, PortId, Short};

/// How many recent callbacks the clock mapping looks back over.
const WINDOW: usize = 256;
/// The thread's wake-up while messages wait or were lately sent, and when
/// nothing has happened for a while.
const BUSY: Duration = Duration::from_millis(1);
const QUIET: Duration = Duration::from_millis(4);
/// "Lately": this long after the last message.
const LATELY: Duration = Duration::from_millis(500);
/// The audio stream standing still this long (the device was lost, or Audio
/// was switched off) lets go of every note sent on.
const STALL: Duration = Duration::from_secs(1);

/// Nanoseconds the frame `frame` lasts from frame 0 at `rate`.
pub(crate) fn frame_ns(frame: u64, rate: u32) -> i64 {
    (u128::from(frame) * 1_000_000_000 / u128::from(rate.max(1))) as i64
}

/// What the audio callback tells the sender: where the stream is, and when
/// that was. Written by one thread, read by another, through a sequence
/// number so a read never mixes two blocks.
pub struct StreamClock {
    epoch: Instant,
    seq: AtomicU64,
    frame: AtomicU64,
    nanos: AtomicU64,
    rate: AtomicU32,
    /// The block's length in frames (live input is heard one block after
    /// it is played, slice 2).
    block: AtomicU32,
}

impl StreamClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            epoch: Instant::now(),
            seq: AtomicU64::new(0),
            frame: AtomicU64::new(0),
            nanos: AtomicU64::new(0),
            rate: AtomicU32::new(48_000),
            block: AtomicU32::new(0),
        })
    }

    /// Nanoseconds since the clock was made.
    pub fn now(&self) -> i64 {
        self.epoch.elapsed().as_nanos() as i64
    }

    /// Nanoseconds from the clock's start to `at` (negative before it).
    pub fn nanos_of(&self, at: Instant) -> i64 {
        match at.checked_duration_since(self.epoch) {
            Some(after) => after.as_nanos() as i64,
            None => -(self.epoch.duration_since(at).as_nanos() as i64),
        }
    }

    /// Called at the start of an audio block of `frames` with the frames
    /// processed so far. Real-time safe: atomics and a clock read.
    pub fn publish(&self, frame: u64, frames: usize, rate: u32) {
        let nanos = self.epoch.elapsed().as_nanos() as i64;
        self.publish_at(frame, frames, rate, nanos);
    }

    /// [`publish`](Self::publish) at a given time (tests inject times).
    pub fn publish_at(&self, frame: u64, frames: usize, rate: u32, nanos: i64) {
        self.seq.fetch_add(1, Ordering::AcqRel);
        self.frame.store(frame, Ordering::Release);
        self.nanos.store(nanos.max(0) as u64, Ordering::Release);
        self.rate.store(rate, Ordering::Release);
        self.block.store(frames as u32, Ordering::Release);
        self.seq.fetch_add(1, Ordering::AcqRel);
    }

    /// The instant `nanos` after the clock's start (tests).
    #[cfg(test)]
    pub fn instant_at(&self, nanos: i64) -> Instant {
        self.epoch + Duration::from_nanos(nanos.max(0) as u64)
    }

    /// The latest block: `(frame, nanos, rate, frames)`.
    pub fn read_block(&self) -> Option<(u64, i64, u32, u32)> {
        for _ in 0..8 {
            let before = self.seq.load(Ordering::Acquire);
            if before == 0 {
                return None;
            }
            if before % 2 == 1 {
                continue;
            }
            let frame = self.frame.load(Ordering::Acquire);
            let nanos = self.nanos.load(Ordering::Acquire) as i64;
            let rate = self.rate.load(Ordering::Acquire);
            let block = self.block.load(Ordering::Acquire);
            if self.seq.load(Ordering::Acquire) == before {
                return Some((frame, nanos, rate, block));
            }
        }
        None
    }

    /// The latest `(frame, nanos, rate)`, or `None` before the first block.
    fn read(&self) -> Option<(u64, i64, u32)> {
        self.read_block().map(|(f, n, r, _)| (f, n, r))
    }
}

/// Maps device frames to times: the earliest `time - frame` lately seen.
#[derive(Default)]
pub struct ClockMap {
    offsets: VecDeque<i64>,
    last: Option<(u64, u32)>,
}

impl ClockMap {
    /// Notes that frame `frame` began at `nanos`.
    pub fn observe(&mut self, frame: u64, nanos: i64, rate: u32) {
        if self.last == Some((frame, rate)) {
            return;
        }
        if self.last.is_some_and(|(_, r)| r != rate) {
            self.offsets.clear();
        }
        self.last = Some((frame, rate));
        self.offsets.push_back(nanos - frame_ns(frame, rate));
        while self.offsets.len() > WINDOW {
            self.offsets.pop_front();
        }
    }

    /// When frame `frame` is heard, `latency_ns` after it is rendered.
    pub fn due(&self, frame: u64, rate: u32, latency_ns: i64) -> Option<i64> {
        let base = *self.offsets.iter().min()?;
        Some(base + frame_ns(frame, rate) + latency_ns)
    }
}

/// Messages waiting for their time, in order, with the notes sent on.
#[derive(Default)]
pub struct Scheduler {
    queue: BTreeMap<(i64, u64), (u16, Msg)>,
    seq: u64,
    /// `(port, channel, key)` of notes sent on and not yet off, with how
    /// many times: two sources (a clip and a live key or the piano roll's
    /// preview) can sound one key, and each note-on is answered by one
    /// note-off.
    sounding: HashMap<(u16, u8, u8), u32>,
    /// Ports sent Start or Continue and not Stop since (bit per port).
    running: u64,
}

impl Scheduler {
    pub fn push(&mut self, due: i64, port: u16, msg: Msg) {
        self.seq += 1;
        self.queue.insert((due, self.seq), (port, msg));
    }

    /// Takes what is due at `now`, in order.
    pub fn due(&mut self, now: i64) -> Vec<(u16, Msg)> {
        let mut out = Vec::new();
        while let Some(entry) = self.queue.first_entry()
            && entry.key().0 <= now
        {
            let (port, msg) = entry.remove();
            if self.note(port, msg) {
                out.push((port, msg));
            }
        }
        out
    }

    /// Records what `msg` does to the notes sent on; false for a note-off
    /// for a note that is not sounding (a flush already let it go, or its
    /// note-on never went out), which is not sent a second time.
    fn note(&mut self, port: u16, msg: Msg) -> bool {
        let bit = 1_u64.checked_shl(u32::from(port)).unwrap_or(0);
        match msg {
            Msg::START | Msg::CONTINUE => self.running |= bit,
            Msg::STOP => self.running &= !bit,
            _ => {}
        }
        match msg.kind() {
            Kind::NoteOn { key, .. } => {
                *self.sounding.entry((port, msg.channel(), key)).or_insert(0) += 1;
                true
            }
            Kind::NoteOff { key, .. } => {
                let which = (port, msg.channel(), key);
                match self.sounding.get_mut(&which) {
                    Some(count) if *count > 1 => {
                        *count -= 1;
                        true
                    }
                    Some(_) => self.sounding.remove(&which).is_some(),
                    None => false,
                }
            }
            _ => true,
        }
    }

    /// Forgets everything waiting and returns the note-offs for the notes
    /// sent on.
    pub fn flush(&mut self) -> Vec<(u16, Msg)> {
        self.queue.clear();
        let mut offs: Vec<(u16, Msg)> = self
            .sounding
            .drain()
            .flat_map(|((port, channel, key), count)| {
                (0..count).map(move |_| (port, Msg::note_off(channel, key)))
            })
            .collect();
        offs.sort_by_key(|(port, msg)| (*port, msg.0));
        offs
    }

    /// Stop for every port whose clock is running (the stream stood
    /// still, or the sender ends).
    pub fn stop_running(&mut self) -> Vec<(u16, Msg)> {
        let mut out = Vec::new();
        let mut left = std::mem::take(&mut self.running);
        while left != 0 {
            out.push((left.trailing_zeros() as u16, Msg::STOP));
            left &= left - 1;
        }
        out
    }

    /// A port went away: its messages and notes are forgotten.
    pub fn forget_port(&mut self, port: u16) {
        self.queue.retain(|_, (p, _)| *p != port);
        self.sounding.retain(|(p, _, _), _| *p != port);
        if let Some(bit) = 1_u64.checked_shl(u32::from(port)) {
            self.running &= !bit;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// A port's clock was started and not stopped.
    pub fn clock_running(&self) -> bool {
        self.running != 0
    }

    /// Notes have been sent on and not yet off.
    pub fn has_sounding(&self) -> bool {
        !self.sounding.is_empty()
    }
}

/// What the app tells the thread.
enum Order {
    /// The port names the engine's port numbers stand for.
    Ports(Vec<String>),
    /// Each port's Sync delay in nanoseconds, by port number (M5 slice 7).
    Delays(Vec<i64>),
}

/// The sender: owns the thread that times and sends MIDI to hardware.
pub struct Output {
    orders: Sender<Order>,
    stop: Arc<AtomicBool>,
    latency: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
    /// Waiting for the first port before the thread starts.
    parts: Option<Parts>,
    sent: Vec<String>,
    delays: Vec<i64>,
}

struct Parts {
    queue: rtrb::Consumer<MidiOut>,
    sender: MidiSender,
    clock: Arc<StreamClock>,
    orders: Receiver<Order>,
}

impl Output {
    /// `queue` is the engine's output; `sender` reaches the ports. The
    /// thread starts when a track first names a port.
    pub fn new(
        queue: rtrb::Consumer<MidiOut>,
        sender: MidiSender,
        clock: Arc<StreamClock>,
    ) -> Self {
        let (orders, receiver) = mpsc::channel();
        Self {
            orders,
            stop: Arc::new(AtomicBool::new(false)),
            latency: Arc::new(AtomicU32::new(0)),
            thread: None,
            parts: Some(Parts {
                queue,
                sender,
                clock,
                orders: receiver,
            }),
            sent: Vec::new(),
            delays: Vec::new(),
        }
    }

    /// Each port's Sync delay in milliseconds, by port number: its clock
    /// and transport messages go that much later (earlier when negative).
    pub fn set_sync_delays(&mut self, ms: &[i32]) {
        let delays: Vec<i64> = ms.iter().map(|&ms| i64::from(ms) * 1_000_000).collect();
        if delays != self.delays {
            self.delays.clone_from(&delays);
            let _ = self.orders.send(Order::Delays(delays));
        }
    }

    /// The device's output latency in frames, which the hardware waits
    /// for so it sounds with the audio.
    pub fn set_latency(&self, frames: u32) {
        self.latency.store(frames, Ordering::Relaxed);
    }

    /// Tells the thread which port each of the engine's port numbers is.
    pub fn set_ports(&mut self, names: &[String]) {
        if names == self.sent.as_slice() {
            return;
        }
        self.sent = names.to_vec();
        let _ = self.orders.send(Order::Ports(names.to_vec()));
        if let Some(parts) = self.parts.take() {
            let stop = Arc::clone(&self.stop);
            let latency = Arc::clone(&self.latency);
            self.thread = thread::Builder::new()
                .name("lumen-midi-out".into())
                .spawn(move || run(parts, &stop, &latency))
                .ok();
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(mut parts: Parts, stop: &AtomicBool, latency: &AtomicU32) {
    let mut ports: Vec<PortId> = Vec::new();
    let mut delays: Vec<i64> = Vec::new();
    let mut map = ClockMap::default();
    let mut scheduler = Scheduler::default();
    let mut active = Instant::now();
    // When the stream last moved, to notice it standing still.
    let mut moved = (0_u64, Instant::now());
    let send = |scheduler: &mut Scheduler, ports: &[PortId], port: u16, msg: Msg| {
        let Some(id) = ports.get(usize::from(port)) else {
            return;
        };
        if !parts.sender.is_open(id) {
            scheduler.forget_port(port);
            return;
        }
        if let Some(short) = Short::from_bytes(msg.bytes()) {
            parts.sender.send(id.clone(), short);
        }
    };
    while !stop.load(Ordering::Relaxed) {
        while let Ok(order) = parts.orders.try_recv() {
            match order {
                Order::Ports(names) => {
                    ports = names
                        .into_iter()
                        .map(|name| PortId {
                            direction: Direction::Output,
                            name,
                        })
                        .collect();
                }
                Order::Delays(by_port) => delays = by_port,
            }
        }
        let clock = parts.clock.read();
        if let Some((frame, nanos, rate)) = clock {
            map.observe(frame, nanos, rate);
        }
        if let Some((frame, _, _)) = clock
            && frame != moved.0
        {
            moved = (frame, Instant::now());
        }
        if (scheduler.has_sounding() || scheduler.clock_running()) && moved.1.elapsed() > STALL {
            let mut lost = scheduler.flush();
            lost.extend(scheduler.stop_running());
            for (port, msg) in lost {
                send(&mut scheduler, &ports, port, msg);
            }
        }
        let now = parts.clock.now();
        while let Ok(message) = parts.queue.pop() {
            active = Instant::now();
            match message {
                MidiOut::At { frame, port, msg } => {
                    let due = clock
                        .and_then(|(_, _, rate)| {
                            let lag = frame_ns(u64::from(latency.load(Ordering::Relaxed)), rate);
                            map.due(frame, rate, lag)
                        })
                        .unwrap_or(now);
                    let delay = match msg.is_system() {
                        true => delays.get(usize::from(port)).copied().unwrap_or(0),
                        false => 0,
                    };
                    scheduler.push(due + delay, port, msg);
                }
                MidiOut::Flush => {
                    for (port, msg) in scheduler.flush() {
                        send(&mut scheduler, &ports, port, msg);
                    }
                }
            }
        }
        for (port, msg) in scheduler.due(parts.clock.now()) {
            send(&mut scheduler, &ports, port, msg);
        }
        let busy = !scheduler.is_empty() || active.elapsed() < LATELY;
        thread::sleep(if busy { BUSY } else { QUIET });
    }
    // Let go of whatever is still sounding, and stop running clocks.
    let mut last = scheduler.flush();
    last.extend(scheduler.stop_running());
    for (port, msg) in last {
        send(&mut scheduler, &ports, port, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(key: u8) -> Msg {
        Msg::note_on(0, key, 100)
    }

    fn off(key: u8) -> Msg {
        Msg::note_off(0, key)
    }

    #[test]
    fn messages_go_out_in_time_order_and_stay_in_order_when_due_together() {
        let mut s = Scheduler::default();
        s.push(2_000, 0, off(60));
        s.push(1_000, 0, on(60));
        s.push(1_000, 0, on(64));
        assert!(s.due(999).is_empty());
        assert_eq!(s.due(1_000), vec![(0, on(60)), (0, on(64))]);
        assert_eq!(s.due(5_000), vec![(0, off(60))]);
        assert!(s.is_empty());
    }

    #[test]
    fn a_flush_forgets_what_waits_and_releases_what_was_sent() {
        let mut s = Scheduler::default();
        s.push(0, 0, on(60));
        s.push(0, 1, Msg::note_on(3, 40, 90));
        s.push(10_000, 0, on(72));
        s.due(100);
        let offs = s.flush();
        assert_eq!(offs, vec![(0, off(60)), (1, Msg::note_off(3, 40))]);
        assert!(s.is_empty());
        assert!(s.due(1_000_000).is_empty(), "the waiting note never goes");
        assert!(s.flush().is_empty());
    }

    #[test]
    fn two_sources_on_one_key_each_get_their_own_note_off() {
        let mut s = Scheduler::default();
        // A clip's note and a preview on one key: on, on, off, off.
        s.push(0, 0, on(60));
        s.push(1, 0, on(60));
        s.push(2, 0, off(60));
        s.push(3, 0, off(60));
        s.push(4, 0, off(60));
        assert_eq!(
            s.due(10),
            vec![(0, on(60)), (0, on(60)), (0, off(60)), (0, off(60))],
            "every note-on is answered, and a third off is not sent"
        );
        // A flush answers every note-on still unanswered.
        s.push(0, 0, on(61));
        s.push(0, 0, on(61));
        s.due(10);
        assert_eq!(s.flush(), vec![(0, off(61)), (0, off(61))]);
    }

    #[test]
    fn a_port_whose_clock_runs_gets_stop_when_the_stream_stands_still() {
        let mut s = Scheduler::default();
        s.push(0, 2, Msg::START);
        s.push(1, 2, Msg::CLOCK);
        s.push(1, 5, Msg::song_position(4));
        s.push(1, 5, Msg::CONTINUE);
        s.due(10);
        assert!(s.clock_running());
        assert_eq!(s.stop_running(), vec![(2, Msg::STOP), (5, Msg::STOP)]);
        assert!(!s.clock_running());
        // Stopped by the engine: nothing more to stop.
        s.push(20, 2, Msg::CONTINUE);
        s.push(30, 2, Msg::STOP);
        s.due(40);
        assert!(s.stop_running().is_empty());
    }

    #[test]
    fn a_note_off_for_a_note_that_is_not_sounding_is_not_sent() {
        let mut s = Scheduler::default();
        s.push(0, 0, on(60));
        s.due(1);
        // A stop lets it go; the engine's own note-off follows.
        assert_eq!(s.flush(), vec![(0, off(60))]);
        s.push(2, 0, off(60));
        assert!(s.due(10).is_empty(), "already released");
        // Nor is the off of a note whose on never went out.
        s.push(3, 0, off(61));
        assert!(s.due(10).is_empty());
    }

    #[test]
    fn a_note_that_ended_is_not_released_again() {
        let mut s = Scheduler::default();
        s.push(0, 0, on(60));
        s.push(1, 0, off(60));
        s.due(10);
        assert!(s.flush().is_empty());
    }

    #[test]
    fn a_port_that_went_away_is_forgotten() {
        let mut s = Scheduler::default();
        s.push(0, 0, on(60));
        s.push(5, 0, off(60));
        s.push(5, 1, on(61));
        s.due(1);
        s.forget_port(0);
        assert_eq!(s.flush(), vec![]);
        s.push(0, 1, on(1));
        s.forget_port(0);
        assert_eq!(s.due(10), vec![(1, on(1))]);
    }

    #[test]
    fn the_clock_map_takes_the_earliest_offset_so_late_callbacks_do_not_shift_time() {
        let mut map = ClockMap::default();
        assert_eq!(map.due(0, 48_000, 0), None);
        // Blocks of 480 frames (10 ms), the third callback 4 ms late.
        map.observe(0, 1_000_000, 48_000);
        map.observe(480, 11_000_000, 48_000);
        map.observe(960, 24_000_000, 48_000);
        map.observe(1_440, 31_000_000, 48_000);
        // Frame 0 was at 1 ms; frame 4,800 (100 ms later) at 101 ms, plus
        // 5 ms of output latency.
        assert_eq!(map.due(4_800, 48_000, 5_000_000), Some(106_000_000));
        // The same frame again changes nothing.
        map.observe(1_440, 1, 48_000);
        assert_eq!(map.due(0, 48_000, 0), Some(1_000_000));
    }

    #[test]
    fn a_new_rate_starts_the_mapping_again() {
        let mut map = ClockMap::default();
        map.observe(0, 5_000_000, 48_000);
        map.observe(0, 9_000_000, 44_100);
        assert_eq!(map.due(0, 44_100, 0), Some(9_000_000));
    }

    #[test]
    fn the_stream_clock_reads_what_was_published() {
        let clock = StreamClock::new();
        assert_eq!(clock.read(), None);
        clock.publish(12_800, 128, 48_000);
        let (frame, nanos, rate) = clock.read().unwrap();
        assert_eq!((frame, rate), (12_800, 48_000));
        assert!(nanos >= 0 && nanos <= clock.now());
    }
}

#[cfg(test)]
mod sender_tests {
    use std::collections::HashMap;

    use lumen_midi::fake::FakeControl;
    use lumen_midi::{MidiSystem, Use};

    use super::*;

    struct Rig {
        fake: FakeControl,
        _system: MidiSystem,
        queue: rtrb::Producer<MidiOut>,
        clock: Arc<StreamClock>,
        output: Output,
    }

    fn id(name: &str) -> PortId {
        FakeControl::id(Direction::Output, name)
    }

    /// A fake output "Synth" switched on for tracks, with the sender
    /// running on port 0.
    fn rig() -> Rig {
        let fake = FakeControl::new();
        fake.plug(Direction::Output, "Synth");
        let uses = HashMap::from([(
            id("Synth"),
            Use {
                track: true,
                remote: false,
                sync: false,
            },
        )]);
        let backend = fake.backend();
        let system = MidiSystem::start(
            move || Ok(Box::new(backend)),
            uses,
            Duration::from_millis(10),
        );
        for _ in 0..200 {
            if system.ports().iter().any(|p| p.open) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let (queue, rx) = rtrb::RingBuffer::new(4_096);
        let clock = StreamClock::new();
        let mut output = Output::new(rx, system.sender(), Arc::clone(&clock));
        output.set_ports(&["Synth".to_owned()]);
        Rig {
            fake,
            _system: system,
            queue,
            clock,
            output,
        }
    }

    fn wait_for(fake: &FakeControl, count: usize) {
        for _ in 0..600 {
            if fake.sent().len() >= count {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn at(frame: u64, msg: Msg) -> MidiOut {
        MidiOut::At {
            frame,
            port: 0,
            msg,
        }
    }

    #[test]
    fn messages_keep_their_order_and_spacing_and_arrive_on_time() {
        let mut rig = rig();
        rig.output.set_latency(480);
        let base = Instant::now();
        // Blocks of 128 frames at the pace of real time, as a device runs.
        let running = Arc::new(AtomicBool::new(true));
        let ticker = {
            let (clock, running) = (Arc::clone(&rig.clock), Arc::clone(&running));
            let start = clock.now();
            thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    let elapsed = (clock.now() - start).max(0) as u128;
                    let frame = elapsed * 48_000 / 1_000_000_000 / 128 * 128;
                    clock.publish(frame as u64, 128, 48_000);
                    thread::sleep(Duration::from_micros(2_666));
                }
            })
        };
        // The engine publishes a block before it sends anything.
        while rig.clock.read().is_none() {
            thread::sleep(Duration::from_millis(1));
        }
        // A note every 100 ms, each held for 50 ms, ten of them.
        for k in 1..=10_u64 {
            let key = 60 + k as u8;
            rig.queue
                .push(at(4_800 * k, Msg::note_on(0, key, 100)))
                .unwrap();
            rig.queue
                .push(at(4_800 * k + 2_400, Msg::note_off(0, key)))
                .unwrap();
        }
        wait_for(&rig.fake, 20);
        let sent = rig.fake.sent_timed();
        assert_eq!(sent.len(), 20);
        let mut lateness = Vec::new();
        for (i, (_, bytes, when)) in sent.iter().enumerate() {
            let k = (i / 2 + 1) as u64;
            let on = i % 2 == 0;
            let key = 60 + k as u8;
            let expected = if on {
                Msg::note_on(0, key, 100)
            } else {
                Msg::note_off(0, key)
            };
            assert_eq!(bytes.as_slice(), expected.bytes(), "message {i}");
            // Due at its frame's time plus the 10 ms of output latency.
            let frame = 4_800 * k + if on { 0 } else { 2_400 };
            let plan = base + Duration::from_micros(frame * 1_000_000 / 48_000 + 10_000);
            lateness.push(when.saturating_duration_since(plan));
            assert!(
                *when + Duration::from_millis(1) >= plan,
                "message {i} went out early"
            );
        }
        running.store(false, Ordering::Relaxed);
        ticker.join().unwrap();
        // The sender keeps time to a few milliseconds; a shared CI runner
        // can stall any thread for longer now and then, so the bound on the
        // worst message is loose and the typical one is held tight.
        lateness.sort();
        let (median, worst) = (lateness[lateness.len() / 2], lateness[lateness.len() - 1]);
        eprintln!("lateness: median {median:?}, worst {worst:?}");
        assert!(
            median < Duration::from_millis(4),
            "median lateness {median:?}"
        );
        assert!(
            worst < Duration::from_millis(25),
            "worst lateness {worst:?}"
        );
        // Spacing: the gaps between notes follow the frames.
        let gap = sent[2].2 - sent[0].2;
        assert!(
            gap > Duration::from_millis(97) && gap < Duration::from_millis(104),
            "{gap:?}"
        );
    }

    #[test]
    fn a_flush_lets_go_of_the_notes_sent_and_forgets_the_rest() {
        let mut rig = rig();
        rig.clock.publish(0, 128, 48_000);
        rig.queue.push(at(0, Msg::note_on(2, 60, 90))).unwrap();
        // Far in the future: never heard.
        rig.queue
            .push(at(480_000, Msg::note_on(2, 72, 90)))
            .unwrap();
        wait_for(&rig.fake, 1);
        rig.queue.push(MidiOut::Flush).unwrap();
        wait_for(&rig.fake, 2);
        let sent = rig.fake.sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].1, Msg::note_on(2, 60, 90).bytes());
        assert_eq!(sent[1].1, Msg::note_off(2, 60).bytes());
        thread::sleep(Duration::from_millis(60));
        assert_eq!(rig.fake.sent().len(), 2, "the waiting note never goes");
    }

    #[test]
    fn a_stream_that_stops_lets_go_of_the_notes_sent_on() {
        let mut rig = rig();
        rig.clock.publish(0, 128, 48_000);
        rig.queue.push(at(0, Msg::note_on(0, 60, 90))).unwrap();
        wait_for(&rig.fake, 1);
        // The device is lost: no more blocks, and no note-off from the engine.
        let started = Instant::now();
        wait_for(&rig.fake, 2);
        let sent = rig.fake.sent();
        assert_eq!(sent.len(), 2, "the note was released");
        assert_eq!(sent[1].1, Msg::note_off(0, 60).bytes());
        assert!(started.elapsed() >= Duration::from_millis(900));
    }

    #[test]
    fn a_port_that_goes_away_drops_its_messages_without_trouble() {
        let mut rig = rig();
        rig.clock.publish(0, 128, 48_000);
        rig.queue.push(at(0, Msg::note_on(0, 60, 90))).unwrap();
        wait_for(&rig.fake, 1);
        rig.fake.unplug(Direction::Output, "Synth");
        for _ in 0..200 {
            if !rig._system.ports().iter().any(|p| p.open) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        rig.queue.push(at(0, Msg::note_off(0, 60))).unwrap();
        rig.queue.push(MidiOut::Flush).unwrap();
        thread::sleep(Duration::from_millis(80));
        assert_eq!(
            rig.fake.sent().len(),
            1,
            "nothing is sent to a missing port"
        );
        // It comes back and works again.
        rig.fake.plug(Direction::Output, "Synth");
        for _ in 0..400 {
            if rig._system.ports().iter().any(|p| p.open) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        rig.queue.push(at(0, Msg::note_on(0, 61, 90))).unwrap();
        wait_for(&rig.fake, 2);
        assert_eq!(rig.fake.sent().len(), 2);
    }

    /// The owner's F4: a port's Sync delay moves its clock and transport
    /// messages, not its notes; and a clock left running gets Stop when the
    /// stream stands still.
    #[test]
    fn the_sync_delay_moves_the_clock_not_the_notes_and_a_stall_stops_it() {
        let mut rig = rig();
        rig.output.set_sync_delays(&[30]);
        // Frame 0 is heard now; the clock goes 30 ms after it.
        rig.clock.publish(0, 128, 48_000);
        rig.queue.push(at(0, Msg::START)).unwrap();
        rig.queue.push(at(0, Msg::note_on(0, 60, 90))).unwrap();
        wait_for(&rig.fake, 2);
        let sent = rig.fake.sent_timed();
        assert_eq!(sent[0].1, Msg::note_on(0, 60, 90).bytes(), "the note first");
        assert_eq!(sent[1].1, Msg::START.bytes());
        let gap = sent[1].2 - sent[0].2;
        assert!(gap >= Duration::from_millis(20), "{gap:?}");
        // The stream stands still: the note lets go and the clock stops.
        wait_for(&rig.fake, 4);
        let sent = rig.fake.sent();
        let tail: Vec<&[u8]> = sent[2..].iter().map(|(_, b)| b.as_slice()).collect();
        assert!(tail.contains(&Msg::note_off(0, 60).bytes()));
        assert!(tail.contains(&Msg::STOP.bytes()));
    }
}
