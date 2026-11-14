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
    AUDITION, Command, Destination, DrumKit, Engine, EngineHandle, InstrumentSpec, KEYBOARD,
    KitKey, Layered, Listen, LiveIn, LiveInput, MidiClip, MidiHandle, MidiNote, MidiOut,
    MidiPlayer, MidiTrack, Mixer, MixerHandle, NodeSpec, Region, Routing, Target, Timeline,
    clip_player, control_list, engine, midi_player, mixer, note_list,
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
            played: frame,
            source,
            track: None,
            msg,
        });
    }

    fn audition(&self, frame: u64, track: u64, msg: Msg) {
        self.0.lock().unwrap().push_back(LiveIn {
            frame,
            played: frame,
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
        values: Vec::new(),
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
    clips: lumen_engine::ClipsHandle,
}

fn rig(tracks: Vec<MidiTrack>, log: &Log) -> Rig {
    let (clips, players) = clip_player();
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
        clips,
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
        seams: Default::default(),
        notes: note_list(vec![MidiNote {
            start: q(1),
            length: q(1),
            pitch: 60,
            velocity: 100,
            channel: None,
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
        seams: Default::default(),
        notes: note_list(vec![MidiNote {
            start: q(1),
            length: q(1),
            pitch: 60,
            velocity: 100,
            channel: None,
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
        seams: Default::default(),
        notes: note_list(vec![MidiNote {
            start: q(1),
            length: q(7),
            pitch: 60,
            velocity: 100,
            channel: None,
        }]),
        controls: control_list(Vec::new()),
    };
    let voices = Arc::new(Mutex::new(0));
    let synth = InstrumentSpec {
        kind: 1,
        preset: Preset::Poly.number(),
        values: Vec::new(),
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
fn messages_landing_on_one_frame_keep_the_order_they_came_in() {
    let log = Log::default();
    let mut rig = rig(vec![track(A, &log, Listen::OFF, Vec::new())], &log);
    // A slow device takes several of the app's messages in one block, all
    // on one frame: a key struck, let go, struck and let go, and a chord
    // swapped for another that shares a key. Each note-on must follow the
    // note-off before it, whatever the sort of one frame's events.
    rig.run(256, 128);
    let key = |on: bool, k: u8| {
        if on {
            Msg::note_on(15, k, 80)
        } else {
            Msg::note_off(15, k)
        }
    };
    let sent = [
        (true, 69),
        (false, 69),
        (true, 69),
        (false, 69),
        (true, 62),
        (true, 69),
        (false, 62),
        (false, 69),
        (true, 64),
        (true, 69),
        (false, 64),
        (false, 69),
    ];
    for (on, k) in sent {
        rig.live.audition(300, A, key(on, k));
    }
    rig.run(512, 128);
    let heard: Vec<(bool, u8)> = notes_of(&rig.log(), A)
        .into_iter()
        .map(|(on, _, k)| (on, k))
        .collect();
    // Per key, the order sent: on, off, on, off, ...
    for k in [62, 64, 69] {
        let sent: Vec<bool> = sent
            .iter()
            .filter(|(_, n)| *n == k)
            .map(|(o, _)| *o)
            .collect();
        let got: Vec<bool> = heard
            .iter()
            .filter(|(_, n)| *n == k)
            .map(|(o, _)| *o)
            .collect();
        assert_eq!(got, sent, "key {k}: {heard:?}");
    }
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

#[test]
fn clip_lists_sent_while_a_live_note_rings_are_still_taken() {
    // Stopped with a note sounding, the engine renders its tail instead of
    // idling. The lists the interface sends meanwhile (every edit sends one)
    // must still be taken, or the queue fills, the interface holds its routing
    // back, and a volume change does nothing until the note ends.
    let log: Log = Arc::default();
    let mut rig = rig(vec![track(A, &log, keys_on(None), Vec::new())], &log);
    rig.live.send(0, KEYS, Msg::note_on(0, 60, 100));
    rig.run(512, 128);
    assert_eq!(rig.held_live(A), 1, "the note is held and rings");
    let mut refused = 0;
    for _ in 0..40 {
        if rig.clips.set(Vec::new()).is_err() {
            refused += 1;
        }
        rig.run(128, 128);
        rig.clips.collect_garbage();
    }
    assert_eq!(refused, 0, "{refused} lists found the queue full");
    assert_eq!(rig.held_live(A), 1, "and the note still rings");
}

// ---------------------------------------------------------------- choke (M8 D0)

/// A kit whose two hats choke each other (group 1), with a kick in none.
fn hat_kit() -> Box<DrumKit> {
    let mut kit = DrumKit::new();
    for key in [42, 46, 36] {
        kit.set(
            key,
            KitKey {
                key,
                channel: None,
                silent: false,
            },
        );
    }
    kit.set_choke(42, 1);
    kit.set_choke(46, 1);
    Box::new(kit)
}

/// A track sent to MIDI output port 1, playing `kit` (its notes and frames
/// are read back from the output queue).
fn port_track(id: u64, clips: Vec<MidiClip>, kit: Box<DrumKit>) -> MidiTrack {
    MidiTrack::new(
        id,
        Target::Port {
            port: 0,
            channel: 3,
        },
        None,
        clips,
    )
    .listening(keys_on(None))
    .playing_drums(Some(kit))
}

/// The notes the port was sent: `(device frame, note-on, key)`.
fn sent(rig: &mut Rig) -> Vec<(u64, bool, u8)> {
    let mut out = Vec::new();
    while let Ok(message) = rig.output.pop() {
        if let MidiOut::At { frame, msg, .. } = message {
            match msg.kind() {
                Kind::NoteOn { key, .. } => out.push((frame, true, key)),
                Kind::NoteOff { key, .. } => out.push((frame, false, key)),
                _ => {}
            }
        }
    }
    out
}

/// A one-track clip of `(start, length, pitch)` notes, from tick zero.
fn drum_clip(notes: &[(Ticks, Ticks, u8)]) -> MidiClip {
    MidiClip {
        id: 9,
        start: Ticks::ZERO,
        length: Ticks::quarters(8),
        period: None,
        seams: Default::default(),
        notes: note_list(
            notes
                .iter()
                .map(|&(start, length, pitch)| MidiNote {
                    start,
                    length,
                    pitch,
                    velocity: 100,
                    channel: None,
                })
                .collect(),
        ),
        controls: control_list(Vec::new()),
    }
}

#[test]
fn a_closed_hat_in_a_clip_ends_the_open_hat_at_the_same_sample() {
    // 120 BPM at 48 kHz: a beat is 24,000 frames.
    let q = Ticks::quarters;
    let log = Log::default();
    let clip = drum_clip(&[(q(0), q(4), 46), (q(1), q(1), 42), (q(2), q(1), 36)]);
    let mut rig = rig(vec![port_track(A, vec![clip], hat_kit())], &log);
    rig.transport.send(Command::Play).unwrap();
    rig.run(4 * 24_000 + 128, 128);
    assert_eq!(
        sent(&mut rig),
        vec![
            (0, true, 46),
            (24_000, false, 46),
            (24_000, true, 42),
            (48_000, false, 42),
            (48_000, true, 36),
            (72_000, false, 36),
        ],
        "the open hat's note-off lands on the closed hat's sample"
    );
}

#[test]
fn a_live_hat_chokes_a_clip_hat_and_a_clip_hat_chokes_a_live_one() {
    let q = Ticks::quarters;
    let log = Log::default();
    // The clip's open hat runs from beat 1 to beat 5 (0 to 96,000). A played
    // closed hat at 30,000 ends it and sounds itself.
    let clip = drum_clip(&[(q(0), q(4), 46)]);
    let mut rig = rig(vec![port_track(A, vec![clip], hat_kit())], &log);
    rig.transport.send(Command::Play).unwrap();
    rig.live.send(30_000, KEYS, Msg::note_on(0, 42, 100));
    rig.live.send(40_000, KEYS, Msg::note_off(0, 42));
    rig.run(50_000, 128);
    assert_eq!(
        sent(&mut rig),
        vec![
            (0, true, 46),
            (30_000, false, 46),
            (30_000, true, 42),
            (40_000, false, 42),
        ],
        "the clip's open hat stops where the played closed hat starts"
    );
    assert_eq!(rig.held_live(A), 0, "the played hat was let go");
}

#[test]
fn a_played_closed_hat_ends_a_played_open_hat_at_the_same_sample() {
    let log = Log::default();
    let mut rig = rig(vec![port_track(A, Vec::new(), hat_kit())], &log);
    rig.run(128, 128);
    rig.live.send(1_000, KEYS, Msg::note_on(0, 46, 100));
    rig.live.send(2_000, KEYS, Msg::note_on(0, 42, 100));
    rig.live.send(3_000, KEYS, Msg::note_off(0, 42));
    rig.live.send(3_100, KEYS, Msg::note_off(0, 46));
    rig.run(4_096, 128);
    assert_eq!(
        sent(&mut rig),
        vec![
            (1_000, true, 46),
            (2_000, false, 46),
            (2_000, true, 42),
            (3_000, false, 42),
        ],
        "the open hat ends when the closed one is struck; its own off is a stray"
    );
    assert_eq!(rig.held_live(A), 0);
}

#[test]
fn a_sound_the_map_silences_sounds_nothing_from_a_player() {
    // A muted sound: the played note never reaches the instrument, and its
    // note-off is not a stray either.
    let log = Log::default();
    let mut kit = DrumKit::new();
    kit.set(
        38,
        KitKey {
            key: 38,
            channel: None,
            silent: true,
        },
    );
    kit.set(
        36,
        KitKey {
            key: 36,
            channel: None,
            silent: false,
        },
    );
    let mut rig = rig(vec![port_track(A, Vec::new(), Box::new(kit))], &log);
    rig.live.send(0, KEYS, Msg::note_on(0, 38, 100));
    rig.live.send(1_000, KEYS, Msg::note_on(0, 36, 100));
    rig.live.send(2_000, KEYS, Msg::note_off(0, 38));
    rig.live.send(3_000, KEYS, Msg::note_off(0, 36));
    rig.run(4_096, 128);
    assert_eq!(
        sent(&mut rig),
        vec![(1_000, true, 36), (3_000, false, 36)],
        "only the heard note went out"
    );
    assert_eq!(rig.held_live(A), 0);
}

#[test]
fn every_note_on_a_choking_track_is_answered_while_playing() {
    // A short pattern where hats cut each other and a kick plays through:
    // every note-on is answered by a note-off, at the end and at every stop.
    // The hits are a 32nd apart, not a tick: two sounds on one *frame* are
    // one pass's tug of war (the first never sounds, its note-on taken back
    // with the second's start), which the choke tests cover.
    let q = Ticks::quarters;
    let after = Ticks(q(1).0 / 8);
    let log = Log::default();
    let clip = drum_clip(&[
        (q(0), q(4), 46),
        (q(1), q(1), 42),
        (q(1) + after, q(1), 46),
        (q(2), q(1), 36),
        (q(2) + after, q(4), 42),
        (q(3), q(1), 46),
    ]);
    let mut rig = rig(vec![port_track(A, vec![clip], hat_kit())], &log);
    rig.transport.send(Command::Play).unwrap();
    rig.run(6 * 24_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(4_096, 128);
    let notes = sent(&mut rig);
    let ons = notes.iter().filter(|(_, on, _)| *on).count();
    let offs = notes.iter().filter(|(_, on, _)| !*on).count();
    assert_eq!(ons, offs, "{notes:?}");
    assert!(ons >= 6, "the pattern played: {notes:?}");
}

/// The notes the port was sent, with their channel: `(device frame, on,
/// channel, key)`.
fn sent_channels(rig: &mut Rig) -> Vec<(u64, bool, u8, u8)> {
    let mut out = Vec::new();
    while let Ok(message) = rig.output.pop() {
        if let MidiOut::At { frame, msg, .. } = message {
            match msg.kind() {
                Kind::NoteOn { key, .. } => out.push((frame, true, msg.channel(), key)),
                Kind::NoteOff { key, .. } => out.push((frame, false, msg.channel(), key)),
                _ => {}
            }
        }
    }
    out
}

/// Asserts every note-on was answered once: per channel and key a note-off
/// never comes before the note-on it answers, and nothing is left sounding at
/// the end (M8 D0 review H2).
#[track_caller]
fn assert_every_note_answered(notes: &[(u64, bool, u8, u8)]) {
    let mut sounding: Vec<(u8, u8)> = Vec::new();
    for &(frame, on, channel, key) in notes {
        if on {
            sounding.push((channel, key));
            continue;
        }
        let found = sounding.iter().position(|&(c, k)| c == channel && k == key);
        assert!(
            found.is_some(),
            "note {key} on channel {channel} was let go at frame {frame} before it sounded: {notes:?}"
        );
        if let Some(at) = found {
            sounding.swap_remove(at);
        }
    }
    assert!(
        sounding.is_empty(),
        "still sounding at the end: {sounding:?} in {notes:?}"
    );
}

/// One hat of each kind overlapping in time, hit at one tick, or struck on one
/// frame: the choke lands on the other's note-on. Every one of these starts
/// both in one pass, where a note-off for a note the instrument has not been
/// sent yet used to ring on for ever (M8 D0 review H1).
fn answers_after(rig: &mut Rig, run: impl FnOnce(&mut Rig)) -> Vec<(u64, bool, u8, u8)> {
    run(rig);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(4_096, 128);
    let notes = sent_channels(rig);
    assert_every_note_answered(&notes);
    notes
}

#[test]
fn a_locate_into_two_overlapping_hats_answers_every_note() {
    // 120 BPM at 48 kHz: a beat is 24,000 frames. An open hat holds beats 0 to
    // 4 and a closed one 1 to 5; a locate to beat 1½ starts both in one pass.
    let q = Ticks::quarters;
    let log = Log::default();
    let clip = drum_clip(&[(q(0), q(4), 46), (q(1), q(4), 42)]);
    let mut rig = rig(vec![port_track(A, vec![clip], hat_kit())], &log);
    rig.transport.send(Command::Play).unwrap();
    rig.run(24_000, 128);
    let notes = answers_after(&mut rig, |rig| {
        rig.transport
            .send(Command::Locate(Ticks(q(1).0 * 3 / 2)))
            .unwrap();
        rig.run(96_000, 128);
    });
    let ons = |key: u8| {
        notes
            .iter()
            .filter(|(_, on, _, k)| *on && *k == key)
            .count()
    };
    assert_eq!(
        ons(42),
        1,
        "the closed hat sounds after the locate: {notes:?}"
    );
    assert_eq!(
        ons(46),
        1,
        "the open hat sounds only before the locate: {notes:?}"
    );
}

#[test]
fn a_loop_wrap_into_two_overlapping_hats_answers_every_note() {
    // The open hat runs from beat 0 to 9 and a closed one from 1 to 3: the
    // loop's wrap to beat 2 starts both in one pass.
    let q = Ticks::quarters;
    let log = Log::default();
    let clip = drum_clip(&[(q(0), q(9), 46), (q(1), q(2), 42)]);
    let mut rig = rig(vec![port_track(A, vec![clip], hat_kit())], &log);
    rig.transport
        .send(Command::SetLoop(Region::new(q(2), q(5), true)))
        .unwrap();
    rig.transport.send(Command::Play).unwrap();
    answers_after(&mut rig, |rig| {
        // Past the wrap at beat 5, into the loop's second pass.
        rig.run(5 * 24_000 + 12_000, 128);
    });
    assert_eq!(rig.held_live(A), 0);
}

#[test]
fn two_notes_of_one_group_on_one_tick_answer_every_note() {
    let q = Ticks::quarters;
    let log = Log::default();
    let clip = drum_clip(&[(q(1), q(1), 46), (q(1), q(1), 42)]);
    let mut rig = rig(vec![port_track(A, vec![clip], hat_kit())], &log);
    rig.transport.send(Command::Play).unwrap();
    let notes = answers_after(&mut rig, |rig| {
        rig.run(3 * 24_000, 128);
    });
    assert_eq!(
        notes.iter().filter(|(_, on, _, _)| *on).count(),
        1,
        "one of the two sounds is heard: {notes:?}"
    );
}

#[test]
fn two_pads_of_one_group_on_one_frame_answer_every_note() {
    let log = Log::default();
    let mut rig = rig(vec![port_track(A, Vec::new(), hat_kit())], &log);
    rig.run(128, 128);
    let notes = answers_after(&mut rig, |rig| {
        rig.live.send(1_000, KEYS, Msg::note_on(0, 46, 100));
        rig.live.send(1_000, KEYS, Msg::note_on(0, 42, 100));
        rig.live.send(2_000, KEYS, Msg::note_off(0, 42));
        rig.live.send(2_100, KEYS, Msg::note_off(0, 46));
        rig.run(4_096, 128);
    });
    assert!(
        notes.iter().all(|(_, on, _, key)| !*on || *key != 46),
        "the choked pad never sounds: {notes:?}"
    );
    assert_eq!(rig.held_live(A), 0);
}

/// A clip of one note, as the app hands it to the engine: the sound's note and
/// its channel.
fn one_note_clip(start: Ticks, length: Ticks, pitch: u8, channel: u8) -> MidiClip {
    let mut clip = drum_clip(&[]);
    clip.length = Ticks::quarters(8);
    clip.notes = note_list(vec![MidiNote {
        start,
        length,
        pitch,
        velocity: 100,
        channel: Some(channel),
    }]);
    clip
}

#[test]
fn a_clip_note_ends_on_the_channel_it_played_on_even_after_an_edit() {
    // A sound may name a channel (M8 D0): the note goes out there, and its
    // note-off does too, whatever the map says when the note ends (review
    // L4).
    let q = Ticks::quarters;
    let log = Log::default();
    let first = one_note_clip(Ticks::ZERO, q(4), 36, 5);
    let mut rig = rig(vec![port_track(A, vec![first.clone()], hat_kit())], &log);
    rig.transport.send(Command::Play).unwrap();
    rig.run(24_000, 128);
    // A map edit that changes the sound's note under it, as the app's rebuilt
    // clip would: the old note is let go where it played, and the new one
    // starts on the same channel.
    let edited = one_note_clip(Ticks::ZERO, q(4), 37, 5);
    rig.midi
        .set(vec![port_track(A, vec![edited], hat_kit())])
        .ok()
        .unwrap();
    rig.run(48_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(4_096, 128);
    let notes = sent_channels(&mut rig);
    assert_eq!(notes[0], (0, true, 5, 36), "the note plays on its channel");
    assert_eq!(
        notes
            .iter()
            .filter(|(_, on, _, key)| !*on && *key == 36)
            .map(|(frame, _, channel, _)| (*frame, *channel))
            .collect::<Vec<_>>(),
        vec![(24_000, 5)],
        "its note-off goes out on the channel it played on: {notes:?}"
    );
    assert_eq!(
        notes
            .iter()
            .filter(|(frame, on, _, key)| *frame >= 24_000 && *on && *key == 37)
            .map(|(_, _, channel, _)| *channel)
            .collect::<Vec<_>>(),
        vec![5],
        "the note after the edit plays on the same channel: {notes:?}"
    );
}

#[test]
fn a_live_pad_ends_on_the_channel_it_played_on_even_after_an_edit() {
    let log = Log::default();
    // The kit's snare plays on channel 9, not the track's channel 3.
    let mut kit = DrumKit::new();
    kit.set(
        38,
        KitKey {
            key: 38,
            channel: Some(9),
            silent: false,
        },
    );
    let mut rig = rig(vec![port_track(A, Vec::new(), Box::new(kit))], &log);
    rig.transport.send(Command::Play).unwrap();
    rig.run(128, 128);
    // A pad played and let go: the note and its end are both on channel 9.
    rig.live.send(1_000, KEYS, Msg::note_on(0, 38, 100));
    rig.live.send(2_000, KEYS, Msg::note_off(0, 38));
    rig.run(2_000, 128);
    // A second pad, held while the map changes under it (the sound now plays
    // on channel 11): its note-off still goes out where its note-on did
    // (review L4).
    rig.live.send(3_000, KEYS, Msg::note_on(0, 38, 100));
    rig.run(1_000, 128);
    let mut edited = DrumKit::new();
    edited.set(
        38,
        KitKey {
            key: 38,
            channel: Some(11),
            silent: false,
        },
    );
    rig.midi
        .set(vec![port_track(A, Vec::new(), Box::new(edited))])
        .ok()
        .unwrap();
    rig.run(2_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(4_096, 128);
    let notes = sent_channels(&mut rig);
    assert_eq!(
        notes[..3],
        [
            (1_000, true, 9, 38),
            (2_000, false, 9, 38),
            (3_000, true, 9, 38)
        ],
        "the sound's channel, not the track's: {notes:?}"
    );
    assert_eq!(notes.len(), 4, "{notes:?}");
    assert_eq!(
        (notes[3].1, notes[3].2, notes[3].3),
        (false, 9, 38),
        "the edit's release goes out on the channel the note played on: {notes:?}"
    );
    assert_eq!(rig.held_live(A), 0);
}
