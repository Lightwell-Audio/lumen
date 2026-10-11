//! MIDI through the engine (M5 slice 1): note starts to the sample, tempo
//! changes, loop wraps, every way a note can be let go, chase, and the
//! hardware queue. A probe instrument writes an impulse for every note-on
//! (+1) and note-off (-1) at its exact frame, and logs what it is sent.

use std::sync::{Arc, Mutex};

use lumen_core::meter::Meter;
use lumen_core::midi::{Event, Kind, Msg};
use lumen_core::time::{TempoMap, TempoPoint, Ticks};
use lumen_devices::Instrument;
use lumen_engine::{
    Command, Destination, Engine, EngineHandle, InstrumentSpec, Layered, MidiClip, MidiControl,
    MidiHandle, MidiNote, MidiOut, MidiPlayer, MidiTrack, Mixer, MixerHandle, NodeSpec, Region,
    Routing, Target, Timeline, clip_player, control_list, engine, midi_player, mixer, note_list,
};

const MASTER: u64 = 100;
const TRACK: u64 = 1;

type Log = Arc<Mutex<Vec<Msg>>>;

struct Probe {
    log: Log,
    held: i32,
}

impl Instrument for Probe {
    fn render(&mut self, _rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]) {
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
                _ => {
                    if !right.is_empty() {
                        right[at] += 0.5;
                    }
                }
            }
            self.log.lock().unwrap().push(e.msg);
        }
    }

    fn active(&self) -> bool {
        self.held > 0
    }

    fn silence(&mut self) {
        self.held = 0;
    }
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
        notes: note_list(notes),
        controls: control_list(Vec::new()),
    }
}

struct Rig {
    transport: EngineHandle,
    midi: MidiHandle,
    #[allow(dead_code)]
    routing: MixerHandle,
    engine: Engine<Mixer<Layered<lumen_engine::ClipPlayer, MidiPlayer>>>,
    log: Log,
    rate: u32,
    output: rtrb::Consumer<MidiOut>,
}

fn probe(log: &Log) -> Option<InstrumentSpec> {
    Some(InstrumentSpec {
        kind: 1,
        preset: 0,
        instrument: Box::new(Probe {
            log: Arc::clone(log),
            held: 0,
        }),
    })
}

fn rig(timeline: Timeline, rate: u32, tracks: Vec<MidiTrack>, log: &Log) -> Rig {
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    midi.set(tracks).ok().unwrap();
    let output = midi.take_output().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    let nodes = vec![
        NodeSpec::unity(MASTER, Destination::Hardware(0)),
        NodeSpec::unity(TRACK, Destination::Node(MASTER)),
    ];
    assert!(
        routing
            .set(&Routing {
                nodes,
                cue: None,
                measure: None
            })
            .unwrap()
    );
    let (transport, engine) = engine(timeline, source, rate);
    Rig {
        transport,
        midi,
        routing,
        engine,
        log: Arc::clone(log),
        rate,
        output,
    }
}

impl Rig {
    /// Renders `frames` in blocks of `block`; returns the left channel and
    /// the frames of every impulse (+1 on, -1 off) from the first frame
    /// rendered by this call, counted from `base`.
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

    fn log(&self) -> Vec<Msg> {
        self.log.lock().unwrap().clone()
    }
}

/// (frame, +1 or -1) of every impulse.
fn impulses(out: &[f32]) -> Vec<(usize, i32)> {
    out.iter()
        .enumerate()
        .filter(|(_, v)| v.abs() > 0.5)
        .map(|(i, v)| (i, v.signum() as i32))
        .collect()
}

fn ons(log: &[Msg]) -> usize {
    log.iter()
        .filter(|m| matches!(m.kind(), Kind::NoteOn { .. }))
        .count()
}

fn offs(log: &[Msg]) -> usize {
    log.iter()
        .filter(|m| matches!(m.kind(), Kind::NoteOff { .. }))
        .count()
}

#[test]
fn notes_start_and_end_on_their_exact_sample() {
    let q = Ticks::quarters;
    // Odd tick positions, so nothing falls on a convenient sample.
    let notes = vec![
        note(q(2) + Ticks(12_345), q(1), 60),
        note(q(4) + Ticks(777), Ticks(q(1).0 / 3), 64),
        note(q(7) + Ticks(1), Ticks(1_000), 67),
    ];
    for (bpm, rate, block) in [
        (120.0, 48_000, 128),
        (97.3, 44_100, 100),
        (140.0, 96_000, 480),
        (999.0, 48_000, 64),
        (20.0, 44_100, 333),
    ] {
        let timeline = Timeline::constant(bpm, Meter::FOUR_FOUR);
        let log = Log::default();
        let track = MidiTrack::new(
            TRACK,
            Target::Instrument,
            probe(&log),
            vec![clip(1, Ticks::ZERO, q(16), notes.clone())],
        );
        let mut rig = rig(timeline.clone(), rate, vec![track], &log);
        rig.play();
        let end = timeline.sample_at(q(9), rate) as usize;
        let found = impulses(&rig.run(end, block));
        let mut expected = Vec::new();
        for n in &notes {
            let on = timeline.sample_at(n.start, rate);
            let off = timeline.sample_at(n.start + n.length, rate).max(on + 1);
            expected.push((on as usize, 1));
            expected.push((off as usize, -1));
        }
        expected.sort();
        assert_eq!(
            found, expected,
            "{bpm} bpm at {rate} Hz in blocks of {block}"
        );
    }
}

#[test]
fn notes_follow_a_tempo_change() {
    let tempo = TempoMap::new(vec![
        TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
        },
        TempoPoint {
            at: Ticks::quarters(4),
            bpm: 75.0,
        },
    ])
    .unwrap();
    let timeline = Timeline::new(tempo, Timeline::default().meter);
    let q = Ticks::quarters;
    let log = Log::default();
    let track = MidiTrack::new(
        TRACK,
        Target::Instrument,
        probe(&log),
        vec![clip(
            1,
            Ticks::ZERO,
            q(16),
            vec![note(q(6) + Ticks(5), q(1), 60), note(q(3), q(2), 62)],
        )],
    );
    let mut rig = rig(timeline.clone(), 48_000, vec![track], &log);
    rig.play();
    let found = impulses(&rig.run(timeline.sample_at(q(9), 48_000) as usize, 128));
    let s = |t: Ticks| timeline.sample_at(t, 48_000) as usize;
    assert_eq!(
        found,
        vec![
            (s(q(3)), 1),
            (s(q(5)), -1),
            (s(q(6) + Ticks(5)), 1),
            (s(q(7) + Ticks(5)), -1)
        ]
    );
}

#[test]
fn a_loop_wrap_releases_the_note_it_cuts_and_starts_the_loop_again() {
    let q = Ticks::quarters;
    let log = Log::default();
    let track = MidiTrack::new(
        TRACK,
        Target::Instrument,
        probe(&log),
        vec![clip(
            1,
            Ticks::ZERO,
            q(32),
            vec![note(q(1), q(6), 60), note(q(2), q(1), 64)],
        )],
    );
    let mut rig = rig(Timeline::default(), 48_000, vec![track], &log);
    rig.transport
        .send(Command::SetLoop(Region::new(Ticks::ZERO, q(4), true)))
        .unwrap();
    rig.play();
    // 120 bpm: a beat is 24,000 samples. Three passes of the loop.
    let found = impulses(&rig.run(48_000 * 6, 128));
    let wrap = 96_000;
    for pass in 0..3 {
        let base = wrap * pass;
        assert!(found.contains(&(base + 24_000, 1)), "pass {pass} start");
        // The long note is cut at the wrap and the short one ended before.
        assert!(
            found.contains(&(base + 72_000, -1)),
            "pass {pass} short off"
        );
    }
    let log = rig.log();
    // Each pass: the long note on, the short one on and off, and the long
    // note released by the wrap (the 'off' after the last wrap waits for
    // the next block).
    let keys: Vec<(bool, u8)> = log
        .iter()
        .filter_map(|m| match m.kind() {
            Kind::NoteOn { key, .. } => Some((true, key)),
            Kind::NoteOff { key, .. } => Some((false, key)),
            _ => None,
        })
        .collect();
    let pass = [(true, 60), (true, 64), (false, 64), (false, 60)];
    assert_eq!(keys[..8], [pass, pass].concat()[..]);
    assert_eq!(
        ons(&log),
        offs(&log) + 1,
        "only the note sounding now is held"
    );
}

#[test]
fn stopping_releases_notes_and_lets_the_instrument_ring_out() {
    let q = Ticks::quarters;
    let log = Log::default();
    let track = MidiTrack::new(
        TRACK,
        Target::Instrument,
        probe(&log),
        vec![clip(1, Ticks::ZERO, q(16), vec![note(q(1), q(8), 60)])],
    );
    let mut rig = rig(Timeline::default(), 48_000, vec![track], &log);
    rig.play();
    rig.run(30_000, 128);
    assert_eq!(rig.engine.source().tracks().1.held(TRACK), 1);
    rig.transport.send(Command::Stop).unwrap();
    let after = impulses(&rig.run(512, 128));
    assert_eq!(after.len(), 1, "one note-off, rendered while stopped");
    assert_eq!(after[0].1, -1);
    assert_eq!(rig.engine.source().tracks().1.held(TRACK), 0);
    let log = rig.log();
    assert_eq!(ons(&log), offs(&log));
    // Nothing more happens once everything is let go.
    assert!(impulses(&rig.run(512, 128)).is_empty());
}

#[test]
fn a_jump_releases_and_chases_the_note_under_the_new_position() {
    let q = Ticks::quarters;
    let log = Log::default();
    let track = MidiTrack::new(
        TRACK,
        Target::Instrument,
        probe(&log),
        vec![clip(
            1,
            Ticks::ZERO,
            q(16),
            vec![note(q(1), q(1), 60), note(q(4), q(4), 64)],
        )],
    );
    let mut rig = rig(Timeline::default(), 48_000, vec![track], &log);
    rig.play();
    rig.run(30_000, 128);
    rig.transport.send(Command::Locate(q(6))).unwrap();
    rig.run(1_000, 128);
    let sent = rig.log();
    // On 60, (ended), then at the locate the long note starts at once.
    let keys: Vec<(bool, u8)> = sent
        .iter()
        .filter_map(|m| match m.kind() {
            Kind::NoteOn { key, .. } => Some((true, key)),
            Kind::NoteOff { key, .. } => Some((false, key)),
            _ => None,
        })
        .collect();
    assert_eq!(keys, vec![(true, 60), (false, 60), (true, 64)]);
    // Leaving the note by jumping out of it releases it.
    rig.transport.send(Command::Locate(q(12))).unwrap();
    rig.run(1_000, 128);
    let sent = rig.log();
    assert_eq!(ons(&sent), offs(&sent));
}

#[test]
fn play_in_the_middle_of_a_note_starts_it_and_applies_the_controllers_before() {
    let q = Ticks::quarters;
    let log = Log::default();
    let mut c = clip(1, Ticks::ZERO, q(16), vec![note(q(1), q(8), 60)]);
    c.controls = control_list(vec![
        MidiControl {
            at: q(1),
            msg: Msg::controller(0, 64, 127),
        },
        MidiControl {
            at: q(2),
            msg: Msg::controller(0, 1, 90),
        },
        MidiControl {
            at: q(3),
            msg: Msg::pitch_bend(0, 2_000),
        },
        MidiControl {
            at: q(3),
            msg: Msg::program(0, 3),
        },
        MidiControl {
            at: q(12),
            msg: Msg::controller(0, 1, 5),
        },
    ]);
    let track = MidiTrack::new(TRACK, Target::Instrument, probe(&log), vec![c]);
    let mut rig = rig(Timeline::default(), 48_000, vec![track], &log);
    rig.transport.send(Command::Locate(q(4))).unwrap();
    rig.play();
    rig.run(2_000, 128);
    let sent = rig.log();
    let kinds: Vec<Kind> = sent.iter().map(|m| m.kind()).collect();
    assert!(kinds.contains(&Kind::Controller {
        number: 64,
        value: 127
    }));
    assert!(kinds.contains(&Kind::Controller {
        number: 1,
        value: 90
    }));
    assert!(kinds.contains(&Kind::PitchBend { value: 2_000 }));
    assert!(kinds.contains(&Kind::Program { program: 3 }));
    assert!(!kinds.contains(&Kind::Controller {
        number: 1,
        value: 5
    }));
    // The controllers come before the note.
    let first_note = kinds
        .iter()
        .position(|k| matches!(k, Kind::NoteOn { .. }))
        .unwrap();
    let last_control = kinds
        .iter()
        .rposition(|k| !matches!(k, Kind::NoteOn { .. } | Kind::NoteOff { .. }))
        .unwrap();
    assert!(last_control < first_note);
    // Stopping lets go of the pedal and the bend too.
    rig.transport.send(Command::Stop).unwrap();
    rig.run(512, 128);
    let kinds: Vec<Kind> = rig.log().iter().map(|m| m.kind()).collect();
    assert!(kinds.ends_with(&[
        Kind::NoteOff {
            key: 60,
            velocity: 64
        },
        Kind::Controller {
            number: 64,
            value: 0
        },
        Kind::PitchBend { value: 0 },
    ]));
}

#[test]
fn an_edit_that_removes_or_moves_a_sounding_note_releases_it() {
    let q = Ticks::quarters;
    let log = Log::default();
    let make = |notes: Vec<MidiNote>| {
        MidiTrack::new(
            TRACK,
            Target::Instrument,
            probe(&log),
            vec![clip(1, Ticks::ZERO, q(16), notes)],
        )
    };
    let long = note(q(1), q(8), 60);
    let mut rig = rig(Timeline::default(), 48_000, vec![make(vec![long])], &log);
    rig.play();
    rig.run(30_000, 128);
    assert_eq!(rig.engine.source().tracks().1.held(TRACK), 1);
    // The same clip with the note gone.
    rig.midi.collect_garbage();
    rig.midi.set(vec![make(vec![])]).ok().unwrap();
    rig.run(512, 128);
    assert_eq!(rig.engine.source().tracks().1.held(TRACK), 0);
    let sent = rig.log();
    assert_eq!((ons(&sent), offs(&sent)), (1, 1));
    // An edit that leaves it alone does not retrigger it.
    rig.midi.collect_garbage();
    let long_again = vec![note(q(1), q(8), 60), note(q(12), q(1), 72)];
    rig.midi.set(vec![make(long_again.clone())]).ok().unwrap();
    rig.run(512, 128);
    // The note is added while sounding (it began 6 beats ago): it starts.
    let sent = rig.log();
    assert_eq!(ons(&sent), 2);
    rig.midi.collect_garbage();
    rig.midi.set(vec![make(long_again)]).ok().unwrap();
    rig.run(512, 128);
    assert_eq!(ons(&rig.log()), 2, "an unchanged track is left alone");
    // Moving the note under the playhead releases then re-chases it.
    rig.midi.collect_garbage();
    rig.midi
        .set(vec![make(vec![
            note(q(1) + Ticks(10), q(8), 60),
            note(q(12), q(1), 72),
        ])])
        .ok()
        .unwrap();
    rig.run(512, 128);
    let sent = rig.log();
    assert_eq!(ons(&sent), 3);
    assert_eq!(offs(&sent), 2);
}

#[test]
fn a_changed_target_lets_go_on_the_old_output() {
    let q = Ticks::quarters;
    let log = Log::default();
    let notes = vec![note(q(1), q(8), 60)];
    let make = |target: Target| {
        MidiTrack::new(
            TRACK,
            target,
            probe(&log),
            vec![clip(1, Ticks::ZERO, q(16), notes.clone())],
        )
    };
    let mut rig = rig(
        Timeline::default(),
        48_000,
        vec![make(Target::Port {
            port: 2,
            channel: 5,
        })],
        &log,
    );
    rig.play();
    rig.run(30_000, 128);
    let mut sent = Vec::new();
    while let Ok(m) = rig.output.pop() {
        sent.push(m);
    }
    assert_eq!(
        sent,
        vec![MidiOut::At {
            frame: 24_000,
            port: 2,
            msg: Msg::note_on(5, 60, 100)
        }]
    );
    // Back to the instrument: the port is let go, the instrument chases.
    rig.midi.collect_garbage();
    rig.midi.set(vec![make(Target::Instrument)]).ok().unwrap();
    rig.run(512, 128);
    let mut sent = Vec::new();
    while let Ok(m) = rig.output.pop() {
        sent.push(m);
    }
    assert!(
        matches!(
            sent.as_slice(),
            [MidiOut::At { port: 2, msg, .. }] if msg.kind() == (Kind::NoteOff { key: 60, velocity: 64 })
        ),
        "{sent:?}"
    );
    assert_eq!(ons(&rig.log()), 1, "the instrument chased the note");
}

#[test]
fn hardware_events_are_stamped_with_the_device_frame_they_are_heard_at() {
    let q = Ticks::quarters;
    let log = Log::default();
    let track = MidiTrack::new(
        TRACK,
        Target::Port {
            port: 0,
            channel: 9,
        },
        None,
        vec![clip(
            1,
            Ticks::ZERO,
            q(16),
            vec![note(q(1) + Ticks(33), q(1), 36), note(q(3), q(1), 38)],
        )],
    );
    let timeline = Timeline::default();
    let mut rig = rig(timeline.clone(), 48_000, vec![track], &log);
    rig.run(100, 100);
    rig.play();
    rig.run(200_000, 128);
    let mut sent = Vec::new();
    while let Ok(m) = rig.output.pop() {
        sent.push(m);
    }
    // Playback began at device frame 100.
    let at = |t: Ticks| 100 + timeline.sample_at(t, 48_000) as u64;
    assert_eq!(
        sent,
        vec![
            MidiOut::At {
                frame: at(q(1) + Ticks(33)),
                port: 0,
                msg: Msg::note_on(9, 36, 100)
            },
            MidiOut::At {
                frame: at(q(2) + Ticks(33)),
                port: 0,
                msg: Msg::note_off(9, 36)
            },
            MidiOut::At {
                frame: at(q(3)),
                port: 0,
                msg: Msg::note_on(9, 38, 100)
            },
            MidiOut::At {
                frame: at(q(4)),
                port: 0,
                msg: Msg::note_off(9, 38)
            },
        ]
    );
}

#[test]
fn panic_and_stop_flush_hardware_and_panic_sends_all_notes_off_everywhere() {
    let q = Ticks::quarters;
    let log = Log::default();
    let track = MidiTrack::new(
        TRACK,
        Target::Port {
            port: 1,
            channel: 0,
        },
        None,
        vec![clip(1, Ticks::ZERO, q(16), vec![note(q(1), q(8), 60)])],
    );
    let mut rig = rig(Timeline::default(), 48_000, vec![track], &log);
    rig.play();
    rig.run(30_000, 128);
    while rig.output.pop().is_ok() {}
    rig.transport.send(Command::Stop).unwrap();
    rig.run(256, 128);
    let mut sent = Vec::new();
    while let Ok(m) = rig.output.pop() {
        sent.push(m);
    }
    assert_eq!(sent.first(), Some(&MidiOut::Flush));
    assert!(sent.iter().any(|m| matches!(m, MidiOut::At { msg, .. } if msg.kind() == (Kind::NoteOff { key: 60, velocity: 64 }))));
    rig.transport.send(Command::Panic).unwrap();
    rig.run(256, 128);
    let mut panic = Vec::new();
    while let Ok(m) = rig.output.pop() {
        panic.push(m);
    }
    assert_eq!(panic[0], MidiOut::Flush);
    for channel in 0..16 {
        for number in [123, 120] {
            assert!(
                panic.iter().any(|m| matches!(m, MidiOut::At { port: 1, msg, .. }
                    if msg.kind() == (Kind::Controller { number, value: 0 }) && msg.channel() == channel)),
                "CC {number} on channel {channel}"
            );
        }
    }
}

#[test]
fn every_note_on_has_its_off_through_loops_jumps_stops_and_edits() {
    let q = Ticks::quarters;
    let log = Log::default();
    // Overlapping notes on one key, long notes, clips that repeat.
    let mut notes = Vec::new();
    for i in 0..40 {
        notes.push(note(Ticks(q(1).0 * i / 2), q(3), 48 + (i % 5) as u8));
    }
    let mut looped = clip(
        2,
        q(8),
        q(24),
        vec![note(Ticks::ZERO, q(1), 70), note(q(1), q(3), 71)],
    );
    looped.period = Some(q(2));
    let make = |extra: bool| {
        let mut clips = vec![clip(1, Ticks::ZERO, q(32), notes.clone()), looped.clone()];
        if extra {
            clips.push(clip(3, q(5), q(10), vec![note(Ticks::ZERO, q(20), 90)]));
        }
        MidiTrack::new(TRACK, Target::Instrument, probe(&log), clips)
    };
    let mut rig = rig(Timeline::default(), 48_000, vec![make(false)], &log);
    rig.transport
        .send(Command::SetLoop(Region::new(q(2), q(9), true)))
        .unwrap();
    rig.play();
    let mut seed = 12_345_u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    for step in 0..400 {
        match next() % 9 {
            0 => rig
                .transport
                .send(Command::Locate(Ticks(q(1).0 * i64::from(next() % 40) / 2)))
                .unwrap(),
            1 => rig.transport.send(Command::Pause).unwrap(),
            2 => rig.transport.send(Command::Continue).unwrap(),
            3 => {
                rig.midi.collect_garbage();
                rig.midi.set(vec![make(step % 2 == 0)]).ok().unwrap();
            }
            4 => rig
                .transport
                .send(Command::SetLoop(Region::new(
                    q(1),
                    q(1) + Ticks(q(1).0 * i64::from(next() % 8 + 1)),
                    next() % 2 == 0,
                )))
                .unwrap(),
            _ => {}
        }
        rig.run(
            1_000 + (next() % 9_000) as usize,
            96 + (next() % 200) as usize,
        );
    }
    rig.transport.send(Command::Stop).unwrap();
    rig.run(1_024, 128);
    let sent = rig.log();
    assert!(ons(&sent) > 100);
    assert_eq!(ons(&sent), offs(&sent), "every note-on has its note-off");
    assert_eq!(rig.engine.source().tracks().1.dropped(), 0);
}

fn synth_rig(volume: f32, audible: bool) -> (Rig, Timeline) {
    let q = Ticks::quarters;
    let log = Log::default();
    let synth = InstrumentSpec {
        kind: lumen_devices::synth::KIND,
        preset: 0,
        instrument: Box::new(lumen_devices::Synth::new(lumen_devices::Preset::Poly)),
    };
    let track = MidiTrack::new(
        TRACK,
        Target::Instrument,
        Some(synth),
        vec![clip(
            1,
            Ticks::ZERO,
            q(8),
            vec![note(q(1) + Ticks(77), q(1), 60)],
        )],
    );
    let timeline = Timeline::default();
    let mut rig = rig(timeline.clone(), 48_000, vec![track], &log);
    rig.routing
        .set(&Routing {
            nodes: vec![
                NodeSpec::unity(MASTER, Destination::Hardware(0)),
                NodeSpec {
                    volume,
                    audible,
                    ..NodeSpec::unity(TRACK, Destination::Node(MASTER))
                },
            ],
            cue: None,
            measure: None,
        })
        .unwrap();
    (rig, timeline)
}

#[test]
fn the_synth_plays_into_the_track_bus_from_its_exact_sample_and_the_mixer_applies() {
    let (mut loud, timeline) = synth_rig(1.0, true);
    loud.play();
    let start = timeline.sample_at(Ticks::quarters(1) + Ticks(77), 48_000) as usize;
    let out = loud.run(start + 24_000, 100);
    let first = out.iter().position(|v| v.abs() > 0.0).expect("it sounds");
    assert_eq!(first, start, "the first sample of sound is the note's");
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let loud_peak = peak(&out[start..]);
    assert!(loud_peak > 0.05 && loud_peak < 1.0, "{loud_peak}");

    let (mut half, _) = synth_rig(0.5, true);
    half.play();
    let out = half.run(start + 24_000, 100);
    assert!((peak(&out[start..]) - loud_peak * 0.5).abs() < 1e-4);

    // Muted: the instrument keeps receiving notes, the mixer silences it.
    let (mut muted, _) = synth_rig(1.0, false);
    muted.play();
    let out = muted.run(start + 24_000, 100);
    assert!(out.iter().all(|v| *v == 0.0));
    assert_eq!(muted.engine.source().tracks().1.held(TRACK), 1);
}

#[test]
fn stopping_lets_the_synth_ring_out_its_release_and_then_goes_quiet() {
    let (mut rig, timeline) = synth_rig(1.0, true);
    rig.play();
    let start = timeline.sample_at(Ticks::quarters(1) + Ticks(77), 48_000) as usize;
    rig.run(start + 6_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    let after = rig.run(4_800, 128);
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    assert!(rms(&after[..480]) > 0.001, "the release tail rings");
    assert!(rms(&after[4_000..]) < rms(&after[..480]), "and fades");
    rig.run(48_000, 128);
    assert!(
        !lumen_engine::Source::ringing(rig.engine.source()),
        "silent and idle again"
    );
}

#[test]
fn an_edit_elsewhere_does_not_cut_the_synth_that_is_sounding() {
    let (mut rig, timeline) = synth_rig(1.0, true);
    rig.play();
    let start = timeline.sample_at(Ticks::quarters(1) + Ticks(77), 48_000) as usize;
    rig.run(start + 3_000, 128);
    let q = Ticks::quarters;
    // The same track again, with a fresh synth inside: the sounding one
    // carries over, and so does the note.
    let fresh = MidiTrack::new(
        TRACK,
        Target::Instrument,
        Some(InstrumentSpec {
            kind: lumen_devices::synth::KIND,
            preset: 0,
            instrument: Box::new(lumen_devices::Synth::new(lumen_devices::Preset::Poly)),
        }),
        vec![clip(
            1,
            Ticks::ZERO,
            q(8),
            vec![note(q(1) + Ticks(77), q(1), 60)],
        )],
    );
    rig.midi.set(vec![fresh]).ok().unwrap();
    let out = rig.run(2_000, 128);
    let steepest = out
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    assert!(
        steepest < 0.05,
        "no step where the lists swapped: {steepest}"
    );
    assert!(out[..500].iter().any(|v| v.abs() > 0.01), "still sounding");
    assert_eq!(rig.engine.source().tracks().1.held(TRACK), 1);
}

/// A synth track on `preset` with one note at beat 1 (+77 ticks).
fn preset_track(preset: lumen_devices::Preset) -> MidiTrack {
    let q = Ticks::quarters;
    MidiTrack::new(
        TRACK,
        Target::Instrument,
        Some(InstrumentSpec {
            kind: lumen_devices::synth::KIND,
            preset: preset.number(),
            instrument: Box::new(lumen_devices::Synth::new(preset)),
        }),
        vec![clip(
            1,
            Ticks::ZERO,
            q(8),
            vec![note(q(1) + Ticks(77), q(1), 60)],
        )],
    )
}

/// Pins the latency of a preset change (the owner's MT-MIDI-2 finding):
/// the engine takes the new sound on the very next block, so the next
/// note-on is the new preset's, bit for bit, even when the list arrives
/// one block before the note. A note already sounding finishes as it was.
#[test]
fn a_preset_change_is_in_effect_on_the_next_block() {
    use lumen_devices::Preset;
    let block = 128;
    let timeline = Timeline::default();
    let start = timeline.sample_at(Ticks::quarters(1) + Ticks(77), 48_000) as usize;
    let render = |from: Preset, to: Preset, change_at: usize| {
        let log = Log::default();
        let mut rig = rig(timeline.clone(), 48_000, vec![preset_track(from)], &log);
        rig.play();
        let mut out = rig.run(change_at, block);
        rig.midi.set(vec![preset_track(to)]).ok().unwrap();
        out.extend(rig.run(start + 12_000 - change_at, block));
        out
    };
    // The list lands in the block before the one holding the note-on.
    let change_at = (start / block - 1) * block;
    let changed = render(Preset::Poly, Preset::Pad, change_at);
    let reference = render(Preset::Pad, Preset::Pad, change_at);
    assert_eq!(changed, reference, "the note-on already has the new sound");
    let old = render(Preset::Poly, Preset::Poly, change_at);
    assert_ne!(changed, old, "and it is not the old sound");

    // A change after the note started leaves that note as it was.
    let late = (start / block + 2) * block;
    let late_change = render(Preset::Poly, Preset::Pad, late);
    let late_old = render(Preset::Poly, Preset::Poly, late);
    assert_eq!(late_change[..late + block], late_old[..late + block]);
}
