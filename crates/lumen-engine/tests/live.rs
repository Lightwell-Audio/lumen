//! Live MIDI through the engine (M5 slice 2): messages from a live input
//! land on their device frame on the tracks that listen, stopped or
//! playing; note-offs end only what this track holds; and every way a
//! track stops listening lets go of its live notes. A probe instrument
//! writes +1 for a note-on and -1 for a note-off at its exact frame.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use lumen_core::midi::{Event, Kind, Msg, cc};
use lumen_core::time::Ticks;
use lumen_devices::{Instrument, Preset, Synth};
use lumen_engine::{
    AUDITION, Command, Destination, Engine, EngineHandle, InstrumentSpec, KEYBOARD, Layered,
    Listen, LiveIn, LiveInput, MidiClip, MidiHandle, MidiNote, MidiOut, MidiPlayer, MidiTrack,
    Mixer, MixerHandle, NodeSpec, Routing, Target, Timeline, clip_player, control_list, engine,
    midi_player, mixer, note_list,
};

const MASTER: u64 = 100;
const A: u64 = 1;
const B: u64 = 2;
const KEYS: u8 = 3;

type Log = Arc<Mutex<Vec<(u64, Msg)>>>;

struct Probe {
    track: u64,
    log: Log,
    held: i32,
}

impl Instrument for Probe {
    fn render(&mut self, _rate: u32, events: &[Event], left: &mut [f32], _right: &mut [f32]) {
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
            self.log.lock().unwrap().push((self.track, e.msg));
        }
    }

    fn active(&self) -> bool {
        self.held > 0
    }

    fn silence(&mut self) {
        self.held = 0;
    }
}

/// A live input the test fills: messages wait until their frame comes.
#[derive(Clone, Default)]
struct Script(Arc<Mutex<VecDeque<LiveIn>>>);

impl Script {
    fn send(&self, frame: u64, source: u8, msg: Msg) {
        self.0.lock().unwrap().push_back(LiveIn {
            frame,
            source,
            track: None,
            msg,
        });
    }

    fn audition(&self, frame: u64, track: u64, msg: Msg) {
        self.0.lock().unwrap().push_back(LiveIn {
            frame,
            source: AUDITION,
            track: Some(track),
            msg,
        });
    }
}

impl LiveInput for Script {
    fn pull(&mut self, _start: u64, end: u64, take: &mut dyn FnMut(LiveIn)) {
        let mut queue = self.0.lock().unwrap();
        while queue.front().is_some_and(|m| m.frame < end) {
            take(queue.pop_front().unwrap());
        }
    }

    fn waiting(&self) -> bool {
        !self.0.lock().unwrap().is_empty()
    }
}

fn probe(track: u64, log: &Log) -> Option<InstrumentSpec> {
    Some(InstrumentSpec {
        kind: 1,
        preset: 0,
        instrument: Box::new(Probe {
            track,
            log: Arc::clone(log),
            held: 0,
        }),
    })
}

fn keys_on(channel: Option<u8>) -> Listen {
    Listen {
        sources: 1 << KEYS,
        channel,
    }
}

fn track(id: u64, log: &Log, listen: Listen, clips: Vec<MidiClip>) -> MidiTrack {
    MidiTrack::new(id, Target::Instrument, probe(id, log), clips).listening(listen)
}

struct Rig {
    transport: EngineHandle,
    midi: MidiHandle,
    #[allow(dead_code)]
    routing: MixerHandle,
    engine: Engine<Mixer<Layered<lumen_engine::ClipPlayer, MidiPlayer>>>,
    log: Log,
    live: Script,
    output: rtrb::Consumer<MidiOut>,
}

fn rig(tracks: Vec<MidiTrack>, log: &Log) -> Rig {
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    midi.set(tracks).ok().unwrap();
    let live = Script::default();
    midi.set_live(Box::new(live.clone())).ok().unwrap();
    let output = midi.take_output().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    let nodes = vec![
        NodeSpec::unity(MASTER, Destination::Hardware(0)),
        NodeSpec::unity(A, Destination::Node(MASTER)),
        NodeSpec::unity(B, Destination::Node(MASTER)),
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
    let (transport, engine) = engine(Timeline::default(), source, 48_000);
    Rig {
        transport,
        midi,
        routing,
        engine,
        log: Arc::clone(log),
        live,
        output,
    }
}

impl Rig {
    /// Renders `frames` in blocks of `block`; returns the left channel.
    fn run(&mut self, frames: usize, block: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames);
        let mut data = vec![0.0; 2 * block];
        let mut done = 0;
        while done < frames {
            let n = block.min(frames - done);
            self.engine.process(48_000, &mut data[..2 * n], 2, n);
            out.extend_from_slice(&data[..n]);
            done += n;
        }
        out
    }

    fn log(&self) -> Vec<(u64, Msg)> {
        self.log.lock().unwrap().clone()
    }

    fn held_live(&self, track: u64) -> usize {
        self.engine.source().tracks().1.held_live(track)
    }
}

fn impulses(out: &[f32]) -> Vec<(usize, i32)> {
    out.iter()
        .enumerate()
        .filter(|(_, v)| v.abs() > 0.5)
        .map(|(i, v)| (i, v.signum() as i32))
        .collect()
}

fn notes_of(log: &[(u64, Msg)], track: u64) -> Vec<(bool, u8, u8)> {
    log.iter()
        .filter(|(t, _)| *t == track)
        .filter_map(|(_, m)| match m.kind() {
            Kind::NoteOn { key, .. } => Some((true, m.channel(), key)),
            Kind::NoteOff { key, .. } => Some((false, m.channel(), key)),
            _ => None,
        })
        .collect()
}

#[test]
fn live_notes_play_on_their_frame_while_stopped_and_while_playing() {
    let log = Log::default();
    let mut rig = rig(vec![track(A, &log, keys_on(None), Vec::new())], &log);
    rig.live.send(1_000, KEYS, Msg::note_on(0, 60, 100));
    rig.live.send(1_777, KEYS, Msg::note_off(0, 60));
    let stopped = impulses(&rig.run(4_096, 128));
    assert_eq!(stopped, vec![(1_000, 1), (1_777, -1)]);

    rig.transport.send(Command::Play).unwrap();
    rig.run(128, 128);
    // Frames count on from the 4,224 already rendered.
    rig.live.send(5_000, KEYS, Msg::note_on(0, 62, 90));
    rig.live.send(6_543, KEYS, Msg::note_off(0, 62));
    let playing = impulses(&rig.run(4_096, 100));
    assert_eq!(playing, vec![(5_000 - 4_224, 1), (6_543 - 4_224, -1)]);
}

#[test]
fn a_late_message_plays_at_the_start_of_the_part() {
    let log = Log::default();
    let mut rig = rig(vec![track(A, &log, keys_on(None), Vec::new())], &log);
    rig.run(512, 128);
    rig.live.send(10, KEYS, Msg::note_on(0, 60, 100));
    assert_eq!(impulses(&rig.run(128, 128)), vec![(0, 1)]);
}

#[test]
fn only_tracks_that_listen_to_the_source_and_channel_play_it() {
    let log = Log::default();
    let mut rig = rig(
        vec![
            track(A, &log, keys_on(Some(1)), Vec::new()),
            track(B, &log, Listen::OFF, Vec::new()),
        ],
        &log,
    );
    rig.live.send(0, KEYS, Msg::note_on(0, 60, 100));
    rig.live.send(0, KEYS, Msg::note_on(1, 61, 100));
    rig.live.send(0, KEYBOARD, Msg::note_on(1, 62, 100));
    rig.run(256, 128);
    let log = rig.log();
    assert_eq!(notes_of(&log, A), vec![(true, 1, 61)]);
    assert!(notes_of(&log, B).is_empty());
}

#[test]
fn a_live_note_and_a_clip_note_on_one_key_each_end_by_their_own_off() {
    let q = Ticks::quarters;
    let log = Log::default();
    let clip = MidiClip {
        id: 9,
        start: Ticks::ZERO,
        length: q(8),
        period: None,
        notes: note_list(vec![MidiNote {
            start: q(1),
            length: q(1),
            pitch: 60,
            velocity: 100,
        }]),
        controls: control_list(Vec::new()),
    };
    let mut rig = rig(vec![track(A, &log, keys_on(None), vec![clip])], &log);
    rig.transport.send(Command::Play).unwrap();
    // The clip's note is held from 24,000 to 48,000; the live one from
    // 30,000 to 40,000.
    rig.live.send(30_000, KEYS, Msg::note_on(0, 60, 100));
    rig.live.send(40_000, KEYS, Msg::note_off(0, 60));
    let found = impulses(&rig.run(50_000, 128));
    assert_eq!(
        found,
        vec![(24_000, 1), (30_000, 1), (40_000, -1), (48_000, -1)]
    );
    assert_eq!(rig.held_live(A), 0);
}

#[test]
fn an_audition_sounds_on_its_track_alone_and_never_ends_another_sources_note() {
    let q = Ticks::quarters;
    let log = Log::default();
    let clip = MidiClip {
        id: 9,
        start: Ticks::ZERO,
        length: q(8),
        period: None,
        notes: note_list(vec![MidiNote {
            start: q(1),
            length: q(1),
            pitch: 60,
            velocity: 100,
        }]),
        controls: control_list(Vec::new()),
    };
    // A listens to the keys and has the clip; B listens to nothing.
    let mut rig = rig(
        vec![
            track(A, &log, keys_on(None), vec![clip]),
            track(B, &log, Listen::OFF, Vec::new()),
        ],
        &log,
    );
    rig.transport.send(Command::Play).unwrap();
    // The clip's note on pitch 60 runs 24,000 to 48,000; a held key on it
    // 26,000 to 46,000. Previews of pitch 60 on A: 30,000 to 32,000, and
    // 47,000 to 50,000 (across the clip's end). One on B, which does not
    // listen: 31,000 to 33,000.
    rig.live.send(26_000, KEYS, Msg::note_on(0, 60, 100));
    // Previews play on channel 15, apart from the clip's channel 0.
    rig.live.audition(30_000, A, Msg::note_on(15, 60, 80));
    rig.live.audition(31_000, B, Msg::note_on(15, 60, 80));
    rig.live.audition(32_000, A, Msg::note_off(15, 60));
    rig.live.audition(33_000, B, Msg::note_off(15, 60));
    rig.live.send(46_000, KEYS, Msg::note_off(0, 60));
    rig.live.audition(47_000, A, Msg::note_on(15, 60, 80));
    rig.live.audition(50_000, A, Msg::note_off(15, 60));
    let found = impulses(&rig.run(52_000, 128));
    // Every source ends its own note, at its own frame.
    assert_eq!(
        found,
        vec![
            (24_000, 1),
            (26_000, 1),
            (30_000, 1),
            (31_000, 1),
            (32_000, -1),
            (33_000, -1),
            (46_000, -1),
            (47_000, 1),
            (48_000, -1),
            (50_000, -1),
        ]
    );
    assert_eq!(
        notes_of(&rig.log(), B),
        vec![(true, 15, 60), (false, 15, 60)]
    );
    assert_eq!(rig.held_live(A), 0);
    assert_eq!(rig.held_live(B), 0);
    // A stray off with nothing sounding sends nothing.
    let before = notes_of(&rig.log(), A).len();
    rig.live.audition(52_100, A, Msg::note_off(15, 60));
    rig.run(512, 128);
    assert_eq!(notes_of(&rig.log(), A).len(), before);
}

/// The real synth, reporting how many of its voices sound after each block.
struct Counted {
    synth: Synth,
    voices: Arc<Mutex<usize>>,
}

impl Instrument for Counted {
    fn render(&mut self, rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]) {
        self.synth.render(rate, events, left, right);
        *self.voices.lock().unwrap() = self.synth.voices();
    }

    fn active(&self) -> bool {
        self.synth.active()
    }

    fn preview(&mut self, rate: u32, left: &mut [f32], right: &mut [f32]) {
        self.synth.preview(rate, left, right);
    }

    fn configure(&mut self, preset: u32) {
        self.synth.configure(preset);
    }

    fn silence(&mut self) {
        self.synth.silence();
    }
}

#[test]
fn a_preview_of_a_clip_notes_key_on_the_audition_channel_leaves_the_clip_note_sounding() {
    let q = Ticks::quarters;
    let clip = MidiClip {
        id: 9,
        start: Ticks::ZERO,
        length: q(8),
        period: None,
        notes: note_list(vec![MidiNote {
            start: q(1),
            length: q(7),
            pitch: 60,
            velocity: 100,
        }]),
        controls: control_list(Vec::new()),
    };
    let voices = Arc::new(Mutex::new(0));
    let synth = InstrumentSpec {
        kind: 1,
        preset: Preset::Poly.number(),
        instrument: Box::new(Counted {
            synth: Synth::new(Preset::Poly),
            voices: Arc::clone(&voices),
        }),
    };
    let track = MidiTrack::new(A, Target::Instrument, Some(synth), vec![clip]);
    let log = Log::default();
    let mut rig = rig(vec![track.listening(keys_on(None))], &log);
    rig.transport.send(Command::Play).unwrap();
    // The clip's note on pitch 60 runs from 24,000 to 192,000. A preview of
    // pitch 60 on the audition channel runs from 30,000 to 32,000.
    rig.live.audition(30_000, A, Msg::note_on(15, 60, 80));
    rig.live.audition(32_000, A, Msg::note_off(15, 60));
    rig.run(31_000, 128);
    assert_eq!(
        *voices.lock().unwrap(),
        2,
        "the clip's note and the preview both sound"
    );
    rig.run(70_000, 128);
    assert_eq!(
        *voices.lock().unwrap(),
        1,
        "the preview has ended and the clip's note still sounds"
    );
}

#[test]
fn a_note_played_and_let_go_within_one_frame_is_not_left_sounding() {
    let log = Log::default();
    let mut rig = rig(vec![track(A, &log, Listen::OFF, Vec::new())], &log);
    // Events on one frame order offs first, so an on and off on one frame
    // (a preview retriggered at once) must keep the off after its on.
    rig.run(256, 128);
    rig.live.audition(300, A, Msg::note_on(0, 60, 80));
    rig.live.audition(300, A, Msg::note_off(0, 60));
    assert_eq!(impulses(&rig.run(512, 128)), vec![(44, 1), (45, -1)]);
    // And on the last frame of a part.
    rig.live.audition(1_279, A, Msg::note_on(0, 61, 80));
    rig.live.audition(1_279, A, Msg::note_off(0, 61));
    assert_eq!(impulses(&rig.run(512, 128)), vec![(510, 1), (511, -1)]);
}

#[test]
fn stop_and_jumps_leave_live_notes_alone() {
    let log = Log::default();
    let mut rig = rig(vec![track(A, &log, keys_on(None), Vec::new())], &log);
    rig.transport.send(Command::Play).unwrap();
    rig.live.send(100, KEYS, Msg::note_on(0, 60, 100));
    rig.run(1_024, 128);
    rig.transport
        .send(Command::Locate(Ticks::quarters(8)))
        .unwrap();
    rig.run(1_024, 128);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(4_096, 128);
    assert_eq!(rig.held_live(A), 1);
    assert_eq!(notes_of(&rig.log(), A), vec![(true, 0, 60)]);
}

#[test]
fn a_track_that_stops_listening_or_leaves_lets_go_of_its_live_notes() {
    let log = Log::default();
    let tracks = || {
        vec![
            track(A, &log, keys_on(None), Vec::new()),
            track(B, &log, keys_on(None), Vec::new()),
        ]
    };
    let mut rig = rig(tracks(), &log);
    rig.live.send(0, KEYS, Msg::note_on(0, 60, 100));
    rig.live.send(0, KEYS, Msg::controller(0, cc::SUSTAIN, 127));
    rig.run(256, 128);
    // A stops listening, and B leaves the list.
    rig.midi
        .set(vec![track(A, &log, Listen::OFF, Vec::new())])
        .ok()
        .unwrap();
    rig.run(256, 128);
    let log = rig.log();
    assert_eq!(notes_of(&log, A), vec![(true, 0, 60), (false, 0, 60)]);
    assert!(
        log.iter()
            .any(|(t, m)| *t == A && *m == Msg::controller(0, cc::SUSTAIN, 0)),
        "the pedal is let up"
    );
    assert_eq!(rig.held_live(A), 0);
    // The note-off from the key now finds nothing to end.
    rig.live.send(600, KEYS, Msg::note_off(0, 60));
    rig.run(256, 128);
    assert_eq!(notes_of(&rig.log(), A).len(), 2);
}

#[test]
fn panic_forgets_live_notes() {
    let log = Log::default();
    let mut rig = rig(vec![track(A, &log, keys_on(None), Vec::new())], &log);
    rig.live.send(0, KEYS, Msg::note_on(0, 60, 100));
    rig.run(256, 128);
    rig.transport.send(Command::Panic).unwrap();
    rig.run(256, 128);
    assert_eq!(rig.held_live(A), 0);
}

#[test]
fn live_input_to_a_port_goes_out_on_the_track_channel_at_its_frame() {
    let log = Log::default();
    let port = MidiTrack::new(
        A,
        Target::Port {
            port: 4,
            channel: 9,
        },
        None,
        Vec::new(),
    )
    .listening(keys_on(None));
    let mut rig = rig(vec![port], &log);
    rig.run(128, 128);
    rig.live.send(300, KEYS, Msg::note_on(2, 60, 100));
    rig.live.send(700, KEYS, Msg::note_off(2, 60));
    rig.run(1_024, 128);
    let mut out = Vec::new();
    while let Ok(m) = rig.output.pop() {
        out.push(m);
    }
    assert_eq!(
        out,
        vec![
            MidiOut::At {
                frame: 300,
                port: 4,
                msg: Msg::note_on(9, 60, 100)
            },
            MidiOut::At {
                frame: 700,
                port: 4,
                msg: Msg::note_off(9, 60)
            },
        ]
    );
}

#[test]
fn a_new_live_input_replaces_the_old_and_the_old_comes_back_to_be_freed() {
    let log = Log::default();
    let mut rig = rig(vec![track(A, &log, keys_on(None), Vec::new())], &log);
    rig.run(128, 128);
    let next = Script::default();
    rig.midi.set_live(Box::new(next.clone())).ok().unwrap();
    rig.run(128, 128);
    assert!(rig.midi.collect_garbage() >= 1);
    next.send(300, KEYS, Msg::note_on(0, 60, 100));
    assert_eq!(impulses(&rig.run(256, 128)), vec![(300 - 256, 1)]);
}
