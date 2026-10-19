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
    MidiNote, MidiPlayer, MidiTrack, Mixer, NodeSpec, Region, Routing, ScrubStyle, Target,
    Timeline, clip_player, control_list, engine, midi_player, mixer, note_list,
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
    /// What the plugin's latency delays its output by: the last `shift`
    /// frames of the left channel, carried into the next block.
    ring: Vec<f32>,
    ring_at: usize,
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
            ring: Vec::new(),
            ring_at: 0,
        })
    }

    /// A host whose output trails what it was given by `frames`, as a plugin
    /// that reports that latency does.
    fn shifted(mut self: Box<Self>, frames: usize) -> Box<Self> {
        self.ring = vec![0.0; frames];
        self
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
        if !self.ring.is_empty() {
            for sample in left.iter_mut() {
                std::mem::swap(sample, &mut self.ring[self.ring_at]);
                self.ring_at = (self.ring_at + 1) % self.ring.len();
            }
        }
        Reply::Ready {
            latency: 0,
            tail: 0,
            restart: false,
            work_ns: 0,
        }
    }

    fn ended(&self) -> bool {
        false
    }

    /// One auxiliary output (pair 1): the same impulses, as the second bus of
    /// a plugin that plays its notes there too.
    fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        if pair != 1 || self.shared.done.load(Ordering::Acquire) != self.seq {
            return false;
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
        true
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
    rig_with_sources(timeline, rate, tracks, &[])
}

/// As [`rig`], with `(track, frames)` of source latency on some tracks.
fn rig_with_sources(
    timeline: Timeline,
    rate: u32,
    tracks: Vec<MidiTrack>,
    sources: &[(u64, u32)],
) -> Rig {
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    let mut nodes = vec![NodeSpec::unity(MASTER, Destination::Hardware(0))];
    nodes.extend(tracks.iter().map(|t| {
        NodeSpec {
            source: sources
                .iter()
                .find(|(id, _)| *id == t.id)
                .map_or(0, |&(_, frames)| frames),
            ..NodeSpec::unity(t.id, Destination::Node(MASTER))
        }
    }));
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
fn a_plugin_instruments_latency_is_compensated_so_it_lines_up_with_a_plain_track() {
    let notes = vec![note(Q(2) + Ticks(12_345), Q(1), 60), note(Q(5), Q(1), 64)];
    let timeline = Timeline::constant(120.0, Meter::FOUR_FOUR);
    for (latency, block) in [(100, 128), (1_024, 128), (333, 100), (4_096, 64)] {
        let play = |compensated: bool| {
            let (spec, _) = hosted(FakeHost::new(Duration::ZERO, false, None).shifted(latency));
            let late = MidiTrack::new(
                1,
                Target::Instrument,
                spec,
                vec![clip(1, Ticks::ZERO, Q(16), notes.clone())],
            );
            let plain = MidiTrack::new(
                2,
                Target::Instrument,
                builtin(),
                vec![clip(2, Ticks::ZERO, Q(16), notes.clone())],
            );
            let sources: &[(u64, u32)] = if compensated {
                &[(1, latency as u32)]
            } else {
                &[]
            };
            let mut rig = rig_with_sources(timeline.clone(), 48_000, vec![late, plain], sources);
            rig.play();
            impulses(&rig.run(timeline.sample_at(Q(8), 48_000) as usize, block))
        };
        let together = play(true);
        assert_eq!(
            together.len(),
            4,
            "two notes, on and off, from both tracks on the same samples (latency {latency}): {together:?}"
        );
        assert_eq!(
            play(false).len(),
            8,
            "without the compensation the plugin is heard {latency} frames late"
        );
    }
}

#[test]
fn an_instruments_other_output_plays_on_its_own_track_on_the_same_samples() {
    // Track 1 is the instrument (muted, so only its other output is heard);
    // track 2 is the audio track that plays the instrument's output pair 1.
    let notes = vec![note(Q(2) + Ticks(12_345), Q(1), 60), note(Q(4), Q(1), 64)];
    let timeline = Timeline::constant(120.0, Meter::FOUR_FOUR);
    for (rate, block) in [(48_000, 128), (44_100, 100), (96_000, 480)] {
        let play = |outputs: Vec<(u32, u64)>, target_in_routing: bool| {
            let (spec, _) = hosted(FakeHost::new(Duration::ZERO, false, None));
            let track = MidiTrack::new(
                1,
                Target::Instrument,
                spec,
                vec![clip(1, Ticks::ZERO, Q(16), notes.clone())],
            )
            .with_outputs(outputs);
            let (_clips, players) = clip_player();
            let (mut midi, player) = midi_player();
            midi.set(vec![track]).ok().unwrap();
            let (mut routing, source) = mixer(Layered(players, player));
            let mut nodes = vec![
                NodeSpec::unity(MASTER, Destination::Hardware(0)),
                NodeSpec {
                    audible: false,
                    sends_audible: false,
                    ..NodeSpec::unity(1, Destination::Node(MASTER))
                },
            ];
            if target_in_routing {
                nodes.push(NodeSpec::unity(2, Destination::Node(MASTER)));
            }
            routing
                .set(&Routing {
                    nodes,
                    cue: None,
                    measure: None,
                })
                .unwrap();
            let (transport, engine) = engine(timeline.clone(), source, rate);
            let mut rig = Rig {
                transport,
                midi,
                engine,
                rate,
            };
            rig.play();
            impulses(&rig.run(timeline.sample_at(Q(6), rate) as usize, block))
        };
        let heard = play(vec![(1, 2)], true);
        assert_eq!(
            heard.len(),
            4,
            "two notes, on and off ({rate} Hz): {heard:?}"
        );
        assert!(
            play(vec![], true).is_empty(),
            "with no output turned on the muted instrument is silent"
        );
        assert!(
            play(vec![(1, 2)], false).is_empty(),
            "a track the routing does not have hears nothing"
        );
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
fn a_host_that_misses_is_silent_and_fails_after_a_second_and_the_other_tracks_play_on() {
    let reference = {
        let track = MidiTrack::new(
            2,
            Target::Instrument,
            builtin(),
            vec![clip(2, Ticks::ZERO, Q(16), vec![note(Q(1), Q(1), 60)])],
        );
        let mut rig = rig(Timeline::default(), 48_000, vec![track]);
        rig.play();
        impulses(&rig.run(144_000, 128))
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
    let found = impulses(&rig.run(144_000, 128));
    assert_eq!(
        found, reference,
        "only the built-in track is heard, exactly"
    );
    assert!(status.failed(), "a second of missed blocks in a row");
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

#[test]
fn a_plugin_with_latency_and_another_output_never_touches_the_heap_through_every_transport_move() {
    // Track 1: a hosted instrument that reports latency (its source is
    // compensated), playing its other output on track 2; track 3 is a busy
    // host that is refused and tried again; track 4 is a plain instrument.
    let rate = 48_000;
    let notes = vec![
        note(Q(1), Q(6), 60),
        note(Q(2), Q(1), 64),
        note(Q(9), Q(3), 67),
    ];
    let (one, _) = hosted(FakeHost::new(Duration::ZERO, false, None).shifted(333));
    let (busy, busy_status) = hosted(FakeHost::new(Duration::from_millis(30), false, None));
    let tracks = vec![
        MidiTrack::new(
            1,
            Target::Instrument,
            one,
            vec![clip(1, Ticks::ZERO, Q(64), notes.clone())],
        )
        .with_outputs(vec![(1, 2)]),
        MidiTrack::new(
            3,
            Target::Instrument,
            busy,
            vec![clip(3, Ticks::ZERO, Q(64), notes.clone())],
        ),
        MidiTrack::new(
            4,
            Target::Instrument,
            builtin(),
            vec![clip(4, Ticks::ZERO, Q(64), notes)],
        ),
    ];
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    midi.set(tracks).ok().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    let mut nodes = vec![NodeSpec::unity(MASTER, Destination::Hardware(0))];
    nodes.extend([1, 2, 3, 4].map(|id| NodeSpec {
        source: if id == 1 { 333 } else { 0 },
        ..NodeSpec::unity(id, Destination::Node(MASTER))
    }));
    routing
        .set(&Routing {
            nodes,
            cue: None,
            measure: None,
        })
        .unwrap();
    let (transport, engine) = engine(Timeline::default(), source, rate);
    let mut rig = Rig {
        transport,
        midi,
        engine,
        rate,
    };
    let mut data = vec![0.0; 2 * 128];
    for block in 0..4_000_u32 {
        match block {
            0 => rig.play(),
            10 => rig
                .transport
                .send(Command::SetLoop(Region::new(Ticks::ZERO, Q(4), true)))
                .unwrap(),
            1_000 => rig.transport.send(Command::Stop).unwrap(),
            1_100 => rig
                .transport
                .send(Command::Scrub {
                    at: Q(2),
                    style: ScrubStyle::Loop { length: Q(1) },
                })
                .unwrap(),
            1_300 => rig
                .transport
                .send(Command::Scrub {
                    at: Q(3),
                    style: ScrubStyle::Tape,
                })
                .unwrap(),
            1_500 => rig.transport.send(Command::EndScrub).unwrap(),
            1_700 => rig.play(),
            2_500 => rig.transport.send(Command::Locate(Q(8))).unwrap(),
            3_000 => rig.transport.send(Command::Pause).unwrap(),
            3_100 => rig.play(),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| rig.engine.process(rate, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        rig.transport.collect_garbage();
    }
    assert!(busy_status.failed(), "the slow host was given up on");
}

/// What the app puts in every track list after the one that attached the host.
fn standin() -> Option<InstrumentSpec> {
    Some(InstrumentSpec {
        kind: 2,
        preset: 0,
        instrument: Box::new(HostedInstrument::placeholder()),
    })
}

fn one_note_track(instrument: Option<InstrumentSpec>) -> MidiTrack {
    MidiTrack::new(
        1,
        Target::Instrument,
        instrument,
        vec![clip(1, Ticks::ZERO, Q(16), vec![note(Q(1), Q(1), 60)])],
    )
}

fn reference_impulses() -> Vec<(usize, i32)> {
    let mut rig = rig(Timeline::default(), 48_000, vec![one_note_track(builtin())]);
    rig.play();
    impulses(&rig.run(48_000, 128))
}

#[test]
fn a_host_attached_after_a_stand_in_list_is_the_one_that_plays() {
    // The track has a stand-in while its plugin loads; the list that
    // attaches the host comes later and must not give the host away.
    let mut rig = rig(Timeline::default(), 48_000, vec![one_note_track(standin())]);
    rig.run(256, 128);
    let (spec, status) = hosted(FakeHost::new(Duration::ZERO, false, None));
    assert!(rig.midi.set(vec![one_note_track(spec)]).is_ok());
    rig.play();
    let found = impulses(&rig.run(48_000, 128));
    assert_eq!(found, reference_impulses());
    assert!(!status.failed());
}

#[test]
fn a_list_skipped_before_it_plays_hands_its_host_to_the_list_that_replaces_it() {
    let mut rig = rig(Timeline::default(), 48_000, vec![one_note_track(standin())]);
    rig.run(256, 128);
    let (spec, status) = hosted(FakeHost::new(Duration::ZERO, false, None));
    // Two lists wait for one block: the engine plays only the newer, and
    // the older, with the only handle to the host, is freed unplayed.
    assert!(rig.midi.set(vec![one_note_track(spec)]).is_ok());
    assert!(rig.midi.set(vec![one_note_track(standin())]).is_ok());
    rig.play();
    let found = impulses(&rig.run(48_000, 128));
    assert_eq!(found, reference_impulses());
    assert!(!status.failed());
}
