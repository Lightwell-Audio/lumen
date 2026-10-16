//! Hosted instruments through the engine (M6 slice 2b, ADR-0069). A fake host
//! stands for the plugin's process: it answers at once, or after a delay on a
//! worker thread. The engine hands every host its block before the clips
//! render and collects them against one deadline; a host that misses is
//! silence, and three misses fail it. Every path runs inside the allocation
//! guard (and under RealtimeSanitizer in Linux CI).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use lumen_core::meter::Meter;
use lumen_core::midi::{Event, Kind, Msg};
use lumen_core::time::Ticks;
use lumen_devices::{Guard, Hosted, HostedInstrument, Instrument, Refused, Reply, Status};
use lumen_engine::{
    Command, Destination, Engine, EngineHandle, InstrumentSpec, Layered, MidiClip, MidiHandle,
    MidiNote, MidiPlayer, MidiTrack, Mixer, NodeSpec, Region, Routing, Target, Timeline,
    clip_player, control_list, engine, midi_player, mixer, note_list,
};
use lumen_rt::count_rt_violations;

lumen_rt::install_guarded_allocator!();

const MASTER: u64 = 100;
type Log = Arc<Mutex<Vec<Msg>>>;

struct Shared {
    go: AtomicU64,
    done: AtomicU64,
    stop: AtomicBool,
}

/// A plugin host that is not one: with no delay it answers inside `submit`;
/// with one, a worker thread answers after it. It writes +1 at a note-on's
/// frame and -1 at a note-off's, like the built-in probe.
struct FakeHost {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    wake: Option<Thread>,
    seq: u64,
    delay: Duration,
    events: Vec<Event>,
    log: Option<Log>,
    overlap: Option<Arc<Overlap>>,
}

/// Whether the engine hands every fake host its block before it waits for
/// any: how many blocks had been handed over when the first host was collected.
#[derive(Default)]
struct Overlap {
    submitted: AtomicU32,
    before_first_collect: AtomicU32,
}

impl FakeHost {
    /// `unpark`: wake the worker on submit (an OS call, so not under the
    /// real-time sanitizer); otherwise it notices within a millisecond.
    fn new(delay: Duration, unpark: bool, log: Option<Log>) -> Box<Self> {
        Self::build(delay, unpark, log, None)
    }

    fn build(
        delay: Duration,
        unpark: bool,
        log: Option<Log>,
        overlap: Option<Arc<Overlap>>,
    ) -> Box<Self> {
        let shared = Arc::new(Shared {
            go: AtomicU64::new(0),
            done: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let worker = (!delay.is_zero()).then(|| {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                while !shared.stop.load(Ordering::Acquire) {
                    let go = shared.go.load(Ordering::Acquire);
                    if go != shared.done.load(Ordering::Relaxed) {
                        thread::sleep(delay);
                        shared.done.store(go, Ordering::Release);
                    } else {
                        thread::park_timeout(Duration::from_millis(1));
                    }
                }
            })
        });
        let wake = worker
            .as_ref()
            .filter(|_| unpark)
            .map(|w| w.thread().clone());
        Box::new(Self {
            shared,
            worker,
            wake,
            seq: 0,
            delay,
            events: Vec::with_capacity(4096),
            log,
            overlap,
        })
    }
}

impl Drop for FakeHost {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

impl Hosted for FakeHost {
    fn submit(&mut self, _: usize, _: &[&[f32]], events: &[Event]) -> Result<(), Refused> {
        if self.shared.done.load(Ordering::Acquire) != self.seq {
            return Err(Refused::Busy);
        }
        self.events.clear();
        self.events.extend_from_slice(events);
        if let Some(log) = &self.log {
            log.lock().unwrap().extend(events.iter().map(|e| e.msg));
        }
        self.seq += 1;
        if self.delay.is_zero() {
            self.shared.done.store(self.seq, Ordering::Release);
        } else {
            if let Some(overlap) = &self.overlap {
                overlap.submitted.fetch_add(1, Ordering::AcqRel);
            }
            self.shared.go.store(self.seq, Ordering::Release);
            if let Some(wake) = &self.wake {
                wake.unpark();
            }
        }
        Ok(())
    }

    fn collect(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Reply {
        if let Some(overlap) = &self.overlap {
            let submitted = overlap.submitted.load(Ordering::Acquire);
            let _ = overlap.before_first_collect.compare_exchange(
                0,
                submitted,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        loop {
            if self.shared.done.load(Ordering::Acquire) == self.seq {
                break;
            }
            if Instant::now() >= deadline {
                return Reply::Late;
            }
            std::hint::spin_loop();
        }
        left.fill(0.0);
        right.fill(0.0);
        for e in &self.events {
            let at = (e.offset as usize).min(left.len().saturating_sub(1));
            match e.msg.kind() {
                Kind::NoteOn { .. } => left[at] += 1.0,
                Kind::NoteOff { .. } => left[at] -= 1.0,
                _ => {}
            }
        }
        Reply::Ready {
            latency: 0,
            tail: 0,
            restart: false,
        }
    }

    fn ended(&self) -> bool {
        false
    }
}

/// The built-in reference: the same impulses, in the same call.
struct Probe {
    held: i32,
}

impl Instrument for Probe {
    fn render(&mut self, _: u32, events: &[Event], left: &mut [f32], _: &mut [f32]) {
        for e in events {
            let at = (e.offset as usize).min(left.len().saturating_sub(1));
            match e.msg.kind() {
                Kind::NoteOn { .. } => {
                    self.held += 1;
                    if !left.is_empty() {
                        left[at] += 1.0;
                    }
                }
                Kind::NoteOff { .. } => {
                    self.held -= 1;
                    if !left.is_empty() {
                        left[at] -= 1.0;
                    }
                }
                _ => {}
            }
        }
    }

    fn active(&self) -> bool {
        self.held > 0
    }

    fn silence(&mut self) {
        self.held = 0;
    }
}

fn builtin() -> Option<InstrumentSpec> {
    Some(InstrumentSpec {
        kind: 1,
        preset: 0,
        instrument: Box::new(Probe { held: 0 }),
    })
}

/// A hosted instrument and what the interface may read about its host.
fn hosted(host: Box<FakeHost>) -> (Option<InstrumentSpec>, Arc<Status>) {
    let guard = Guard::new(host, 0);
    let status = guard.status();
    let spec = InstrumentSpec {
        kind: 2,
        preset: 0,
        instrument: Box::new(HostedInstrument::new(guard)),
    };
    (Some(spec), status)
}

fn note(start: Ticks, length: Ticks, pitch: u8) -> MidiNote {
    MidiNote {
        start,
        length,
        pitch,
        velocity: 100,
    }
}

fn clip(id: u64, start: Ticks, length: Ticks, notes: Vec<MidiNote>) -> MidiClip {
    MidiClip {
        id,
        start,
        length,
        period: None,
        seams: Default::default(),
        notes: note_list(notes),
        controls: control_list(Vec::new()),
    }
}

struct Rig {
    transport: EngineHandle,
    #[allow(dead_code)]
    midi: MidiHandle,
    engine: Engine<Mixer<Layered<lumen_engine::ClipPlayer, MidiPlayer>>>,
    rate: u32,
}

/// Tracks `1..=tracks.len()`, one MIDI track each, all into the master.
fn rig(timeline: Timeline, rate: u32, tracks: Vec<MidiTrack>) -> Rig {
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    let mut nodes = vec![NodeSpec::unity(MASTER, Destination::Hardware(0))];
    nodes.extend(
        tracks
            .iter()
            .map(|t| NodeSpec::unity(t.id, Destination::Node(MASTER))),
    );
    midi.set(tracks).ok().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    routing
        .set(&Routing {
            nodes,
            cue: None,
            measure: None,
        })
        .unwrap();
    let (transport, engine) = engine(timeline, source, rate);
    Rig {
        transport,
        midi,
        engine,
        rate,
    }
}

impl Rig {
    fn run(&mut self, frames: usize, block: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames);
        let mut data = vec![0.0; 2 * block];
        let mut done = 0;
        while done < frames {
            let n = block.min(frames - done);
            self.engine.process(self.rate, &mut data[..2 * n], 2, n);
            out.extend_from_slice(&data[..n]);
            done += n;
        }
        out
    }

    fn play(&mut self) {
        self.transport.send(Command::Play).unwrap();
    }
}

fn impulses(out: &[f32]) -> Vec<(usize, i32)> {
    out.iter()
        .enumerate()
        .filter(|(_, v)| v.abs() > 0.5)
        .map(|(i, v)| (i, v.signum() as i32))
        .collect()
}

const Q: fn(i64) -> Ticks = Ticks::quarters;

#[test]
fn a_hosted_instrument_plays_notes_on_the_same_samples_as_a_built_in_one() {
    let notes = vec![
        note(Q(2) + Ticks(12_345), Q(1), 60),
        note(Q(4) + Ticks(777), Ticks(Q(1).0 / 3), 64),
        note(Q(7) + Ticks(1), Ticks(1_000), 67),
    ];
    for (bpm, rate, block) in [
        (120.0, 48_000, 128),
        (97.3, 44_100, 100),
        (140.0, 96_000, 480),
        (999.0, 48_000, 64),
        (20.0, 44_100, 333),
    ] {
        let timeline = Timeline::constant(bpm, Meter::FOUR_FOUR);
        let play = |instrument| {
            let track = MidiTrack::new(
                1,
                Target::Instrument,
                instrument,
                vec![clip(1, Ticks::ZERO, Q(16), notes.clone())],
            );
            let mut rig = rig(timeline.clone(), rate, vec![track]);
            rig.play();
            impulses(&rig.run(timeline.sample_at(Q(9), rate) as usize, block))
        };
        let reference = play(builtin());
        let (spec, status) = hosted(FakeHost::new(Duration::ZERO, false, None));
        let found = play(spec);
        assert_eq!(reference.len(), 6, "three notes, on and off");
        assert_eq!(
            found, reference,
            "{bpm} bpm at {rate} Hz, blocks of {block}"
        );
        assert!(!status.failed());
        assert_eq!(status.late_blocks(), 0);
    }
}

#[test]
fn stop_and_a_loop_wrap_let_go_of_what_the_host_is_holding() {
    let log = Log::default();
    let (spec, _) = hosted(FakeHost::new(Duration::ZERO, false, Some(Arc::clone(&log))));
    let track = MidiTrack::new(
        1,
        Target::Instrument,
        spec,
        vec![clip(
            1,
            Ticks::ZERO,
            Q(32),
            vec![note(Q(1), Q(6), 60), note(Q(2), Q(1), 64)],
        )],
    );
    let mut rig = rig(Timeline::default(), 48_000, vec![track]);
    rig.transport
        .send(Command::SetLoop(Region::new(Ticks::ZERO, Q(4), true)))
        .unwrap();
    rig.play();
    // Three passes of the loop, the long note cut at each wrap.
    rig.run(48_000 * 6, 128);
    // Stop in the middle of a note, then let the host's messages through.
    rig.run(30_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(2_000, 128);
    let log = log.lock().unwrap();
    let count = |want: fn(Kind) -> bool| log.iter().filter(|m| want(m.kind())).count();
    let ons = count(|k| matches!(k, Kind::NoteOn { .. }));
    let offs = count(|k| matches!(k, Kind::NoteOff { .. }));
    assert!(ons >= 6, "the notes were played: {ons}");
    assert_eq!(ons, offs, "every note the host started was let go");
}

#[test]
fn a_host_that_misses_is_silent_and_fails_after_three_and_the_other_tracks_play_on() {
    let reference = {
        let track = MidiTrack::new(
            2,
            Target::Instrument,
            builtin(),
            vec![clip(2, Ticks::ZERO, Q(16), vec![note(Q(1), Q(1), 60)])],
        );
        let mut rig = rig(Timeline::default(), 48_000, vec![track]);
        rig.play();
        impulses(&rig.run(48_000, 128))
    };
    // A host that takes 200 ms to answer a block of 128 frames.
    let (slow, status) = hosted(FakeHost::new(Duration::from_millis(200), false, None));
    let slow_track = MidiTrack::new(
        1,
        Target::Instrument,
        slow,
        vec![clip(1, Ticks::ZERO, Q(16), vec![note(Q(1), Q(1), 62)])],
    );
    let fast_track = MidiTrack::new(
        2,
        Target::Instrument,
        builtin(),
        vec![clip(2, Ticks::ZERO, Q(16), vec![note(Q(1), Q(1), 60)])],
    );
    let mut rig = rig(Timeline::default(), 48_000, vec![slow_track, fast_track]);
    rig.play();
    let found = impulses(&rig.run(48_000, 128));
    assert_eq!(
        found, reference,
        "only the built-in track is heard, exactly"
    );
    assert!(status.failed(), "three missed blocks in a row");
    assert!(status.late_blocks() >= 3);
}

#[test]
fn sixty_four_hosts_have_their_blocks_in_flight_together_not_one_after_another() {
    const HOSTS: u64 = 64;
    let overlap = Arc::new(Overlap::default());
    let tracks: Vec<MidiTrack> = (1..=HOSTS)
        .map(|id| {
            let host = FakeHost::build(Duration::from_millis(5), true, None, Some(overlap.clone()));
            let (spec, _) = hosted(host);
            MidiTrack::new(
                id,
                Target::Instrument,
                spec,
                vec![clip(id, Ticks::ZERO, Q(16), vec![note(Q(0), Q(8), 60)])],
            )
        })
        .collect();
    // One part of 256 frames at 8 kHz: the callback's deadline is 24 ms, so a
    // 5 ms host is in time, and the engine submits to every host (a few
    // microseconds each) before the first is collected. Had it waited for each
    // in turn, one block would have been handed over by then.
    let mut rig = rig(Timeline::default(), 8_000, tracks);
    rig.play();
    rig.run(256, 256);
    assert_eq!(
        overlap.before_first_collect.load(Ordering::Acquire),
        HOSTS as u32,
        "every host had its block before the first was waited for"
    );
}

#[test]
fn hosted_instruments_never_touch_the_heap() {
    let log: Option<Log> = None;
    let mut statuses = Vec::new();
    let mut tracks = Vec::new();
    for id in 1..=4u64 {
        // Three answer at once; the last never answers in time and fails.
        let delay = if id == 4 {
            Duration::from_millis(30)
        } else {
            Duration::ZERO
        };
        let (spec, status) = hosted(FakeHost::new(delay, false, log.clone()));
        statuses.push(status);
        tracks.push(MidiTrack::new(
            id,
            Target::Instrument,
            spec,
            vec![clip(
                id,
                Ticks::ZERO,
                Q(64),
                vec![
                    note(Q(1), Q(6), 60),
                    note(Q(2), Q(1), 64),
                    note(Q(9), Q(3), 67),
                ],
            )],
        ));
    }
    let mut rig = rig(Timeline::default(), 48_000, tracks);
    let mut data = vec![0.0; 2 * 128];
    for block in 0..4_000_u32 {
        match block {
            0 => rig.play(),
            10 => rig
                .transport
                .send(Command::SetLoop(Region::new(Ticks::ZERO, Q(4), true)))
                .unwrap(),
            1_500 => rig.transport.send(Command::Stop).unwrap(),
            1_600 => rig.play(),
            2_500 => rig.transport.send(Command::Locate(Q(8))).unwrap(),
            3_000 => rig.transport.send(Command::Pause).unwrap(),
            3_100 => rig.play(),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| rig.engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        rig.transport.collect_garbage();
    }
    assert!(statuses[3].failed(), "the slow host was given up on");
    assert!(statuses[..3].iter().all(|s| !s.failed()));
}
