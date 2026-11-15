use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lumen_core::midi::{Kind, Msg};
use lumen_engine::MidiOut;
use lumen_midi::{Direction, MidiSender, PortId, Short};

const WINDOW: usize = 256;
const BUSY: Duration = Duration::from_millis(1);
const QUIET: Duration = Duration::from_millis(4);
const LATELY: Duration = Duration::from_millis(500);
const STALL: Duration = Duration::from_secs(1);

pub(crate) fn frame_ns(frame: u64, rate: u32) -> i64 {
    (u128::from(frame) * 1_000_000_000 / u128::from(rate.max(1))) as i64
}

pub struct StreamClock {
    epoch: Instant,
    seq: AtomicU64,
    frame: AtomicU64,
    nanos: AtomicU64,
    rate: AtomicU32,
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

    pub fn now(&self) -> i64 {
        self.epoch.elapsed().as_nanos() as i64
    }

    pub fn nanos_of(&self, at: Instant) -> i64 {
        match at.checked_duration_since(self.epoch) {
            Some(after) => after.as_nanos() as i64,
            None => -(self.epoch.duration_since(at).as_nanos() as i64),
        }
    }

    pub fn publish(&self, frame: u64, frames: usize, rate: u32) {
        let nanos = self.epoch.elapsed().as_nanos() as i64;
        self.publish_at(frame, frames, rate, nanos);
    }

    pub fn publish_at(&self, frame: u64, frames: usize, rate: u32, nanos: i64) {
        self.seq.fetch_add(1, Ordering::AcqRel);
        self.frame.store(frame, Ordering::Release);
        self.nanos.store(nanos.max(0) as u64, Ordering::Release);
        self.rate.store(rate, Ordering::Release);
        self.block.store(frames as u32, Ordering::Release);
        self.seq.fetch_add(1, Ordering::AcqRel);
    }

    #[cfg(test)]
    pub fn instant_at(&self, nanos: i64) -> Instant {
        self.epoch + Duration::from_nanos(nanos.max(0) as u64)
    }

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

    fn read(&self) -> Option<(u64, i64, u32)> {
        self.read_block().map(|(f, n, r, _)| (f, n, r))
    }
}

#[derive(Default)]
pub struct ClockMap {
    offsets: VecDeque<i64>,
    last: Option<(u64, u32)>,
}

impl ClockMap {
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

    pub fn due(&self, frame: u64, rate: u32, latency_ns: i64) -> Option<i64> {
        let base = *self.offsets.iter().min()?;
        Some(base + frame_ns(frame, rate) + latency_ns)
    }
}

#[derive(Default)]
pub struct Scheduler {
    queue: BTreeMap<(i64, u64), (u16, Msg)>,
    seq: u64,
    sounding: HashMap<(u16, u8, u8), u32>,
    running: u64,
}

impl Scheduler {
    pub fn push(&mut self, due: i64, port: u16, msg: Msg) {
        self.seq += 1;
        self.queue.insert((due, self.seq), (port, msg));
    }

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

    pub fn stop_running(&mut self) -> Vec<(u16, Msg)> {
        let mut out = Vec::new();
        let mut left = std::mem::take(&mut self.running);
        while left != 0 {
            out.push((left.trailing_zeros() as u16, Msg::STOP));
            left &= left - 1;
        }
        out
    }

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

    pub fn clock_running(&self) -> bool {
        self.running != 0
    }

    pub fn has_sounding(&self) -> bool {
        !self.sounding.is_empty()
    }
}

enum Order {
    Ports(Vec<String>),
    Delays(Vec<i64>),
}

pub struct Output {
    orders: Sender<Order>,
    stop: Arc<AtomicBool>,
    latency: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
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

    pub fn set_sync_delays(&mut self, ms: &[i32]) {
        let delays: Vec<i64> = ms.iter().map(|&ms| i64::from(ms) * 1_000_000).collect();
        if delays != self.delays {
            self.delays.clone_from(&delays);
            let _ = self.orders.send(Order::Delays(delays));
        }
    }

    pub fn set_latency(&self, frames: u32) {
        self.latency.store(frames, Ordering::Relaxed);
    }

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
    let _priority = lumen_audio::priority::ProAudio::join();
    let mut ports: Vec<PortId> = Vec::new();
    let mut delays: Vec<i64> = Vec::new();
    let mut map = ClockMap::default();
    let mut scheduler = Scheduler::default();
    let mut active = Instant::now();
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
        assert_eq!(s.flush(), vec![(0, off(60))]);
        s.push(2, 0, off(60));
        assert!(s.due(10).is_empty(), "already released");
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
        map.observe(0, 1_000_000, 48_000);
        map.observe(480, 11_000_000, 48_000);
        map.observe(960, 24_000_000, 48_000);
        map.observe(1_440, 31_000_000, 48_000);
        assert_eq!(map.due(4_800, 48_000, 5_000_000), Some(106_000_000));
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
        lumen_testkit::wait_until("the port to open", || system.ports().iter().any(|p| p.open));
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

    #[track_caller]
    fn wait_for(fake: &FakeControl, count: usize) {
        lumen_testkit::wait_until(format_args!("{count} messages sent"), || {
            fake.sent().len() >= count
        });
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
        let running = Arc::new(AtomicBool::new(true));
        let ticker = {
            let (clock, running) = (Arc::clone(&rig.clock), Arc::clone(&running));
            let start = clock.nanos_of(base);
            thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    let elapsed = (clock.now() - start).max(0) as u128;
                    let frame = (elapsed * 48_000 / 1_000_000_000 / 128 * 128) as u64;
                    clock.publish_at(frame, 128, 48_000, start + frame_ns(frame, 48_000));
                    thread::sleep(Duration::from_micros(2_666));
                }
            })
        };
        while rig.clock.read().is_none() {
            thread::sleep(Duration::from_millis(1));
        }
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
        lateness.sort();
        let (median, worst) = (lateness[lateness.len() / 2], lateness[lateness.len() - 1]);
        eprintln!("lateness: median {median:?}, worst {worst:?}");
        assert!(
            median < Duration::from_millis(8),
            "median lateness {median:?}"
        );
        let allowed_lateness = Duration::from_millis(25);
        assert!(worst < allowed_lateness, "worst lateness {worst:?}");
        let gap = sent[2].2 - sent[0].2;
        assert!(
            gap.abs_diff(Duration::from_millis(100)) < allowed_lateness + Duration::from_millis(1),
            "{gap:?}"
        );
    }

    #[test]
    fn a_flush_lets_go_of_the_notes_sent_and_forgets_the_rest() {
        let mut rig = rig();
        rig.clock.publish(0, 128, 48_000);
        rig.queue.push(at(0, Msg::note_on(2, 60, 90))).unwrap();
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
        lumen_testkit::wait_until("the port to close", || {
            !rig._system.ports().iter().any(|p| p.open)
        });
        rig.queue.push(at(0, Msg::note_off(0, 60))).unwrap();
        rig.queue.push(MidiOut::Flush).unwrap();
        thread::sleep(Duration::from_millis(80));
        assert_eq!(
            rig.fake.sent().len(),
            1,
            "nothing is sent to a missing port"
        );
        rig.fake.plug(Direction::Output, "Synth");
        lumen_testkit::wait_until("the port to open again", || {
            rig._system.ports().iter().any(|p| p.open)
        });
        rig.queue.push(at(0, Msg::note_on(0, 61, 90))).unwrap();
        wait_for(&rig.fake, 2);
        assert_eq!(rig.fake.sent().len(), 2);
    }

    #[test]
    fn the_sync_delay_moves_the_clock_not_the_notes_and_a_stall_stops_it() {
        let mut rig = rig();
        rig.output.set_sync_delays(&[30]);
        let heard = Instant::now();
        rig.clock.publish(0, 128, 48_000);
        rig.queue.push(at(0, Msg::START)).unwrap();
        rig.queue.push(at(0, Msg::note_on(0, 60, 90))).unwrap();
        wait_for(&rig.fake, 2);
        let sent = rig.fake.sent_timed();
        assert_eq!(sent[0].1, Msg::note_on(0, 60, 90).bytes(), "the note first");
        assert_eq!(sent[1].1, Msg::START.bytes());
        let after = sent[1].2 - heard;
        assert!(after >= Duration::from_millis(25), "{after:?}");
        wait_for(&rig.fake, 4);
        let sent = rig.fake.sent();
        let tail: Vec<&[u8]> = sent[2..].iter().map(|(_, b)| b.as_slice()).collect();
        assert!(tail.contains(&Msg::note_off(0, 60).bytes()));
        assert!(tail.contains(&Msg::STOP.bytes()));
    }
}
