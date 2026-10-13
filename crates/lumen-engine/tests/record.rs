//! Recording MIDI in the engine (M5 slice 6, ADR-0055): tracks that record
//! keep the live messages they take, with the timeline sample heard when
//! they were played; the count-in holds the song at the play marker while
//! the metronome clicks; Stop, a locate and Panic end both.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use lumen_core::midi::Msg;
use lumen_core::time::Ticks;
use lumen_engine::{
    Captured, Command, Destination, Engine, EngineHandle, Layered, Listen, LiveIn, LiveInput,
    MidiHandle, MidiPlayer, MidiTrack, Mixer, MixerHandle, NodeSpec, Region, Routing, Target,
    Timeline, clip_player, engine, midi_player, mixer,
};

const MASTER: u64 = 100;
const A: u64 = 1;
const B: u64 = 2;
const KEYS: u8 = 3;
/// One beat at 120 BPM and 48 kHz.
const BEAT: u64 = 24_000;

#[derive(Clone, Default)]
struct Script(Arc<Mutex<VecDeque<LiveIn>>>);

impl Script {
    /// A message played at device frame `played`, heard at `frame`.
    fn send(&self, played: u64, frame: u64, msg: Msg) {
        self.0.lock().unwrap().push_back(LiveIn {
            frame,
            played,
            source: KEYS,
            track: None,
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

struct Rig {
    transport: EngineHandle,
    engine: Engine<Mixer<Layered<lumen_engine::ClipPlayer, MidiPlayer>>>,
    live: Script,
    captured: rtrb::Consumer<Captured>,
    #[allow(dead_code)]
    midi: MidiHandle,
    #[allow(dead_code)]
    routing: MixerHandle,
}

fn keys() -> Listen {
    Listen {
        sources: 1 << KEYS,
        channel: None,
    }
}

fn rig(tracks: Vec<MidiTrack>) -> Rig {
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    midi.set(tracks).ok().unwrap();
    let live = Script::default();
    midi.set_live(Box::new(live.clone())).ok().unwrap();
    let captured = midi.take_captured().unwrap();
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
        engine,
        live,
        captured,
        midi,
        routing,
    }
}

fn recorder(id: u64) -> MidiTrack {
    MidiTrack::new(id, Target::Instrument, None, Vec::new()).recording(keys())
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

    fn taken(&mut self) -> Vec<Captured> {
        let mut out = Vec::new();
        while let Ok(c) = self.captured.pop() {
            out.push(c);
        }
        out
    }
}

#[test]
fn notes_are_kept_at_the_sample_they_were_played_whatever_the_block() {
    for block in [64, 100, 128, 333, 1_024] {
        let mut rig = rig(vec![recorder(A)]);
        rig.transport.send(Command::Record { count_in: 0 }).unwrap();
        rig.run(block, block);
        // Played at 1,000 and heard one block later, as the feed does.
        let heard = 1_000 + block as u64;
        rig.live.send(1_000, heard, Msg::note_on(0, 60, 100));
        rig.live
            .send(7_777, 7_777 + block as u64, Msg::note_off(0, 60));
        rig.run(10_000, block);
        let kept = rig.taken();
        assert_eq!(
            kept.iter()
                .map(|c| (c.track, c.at, c.msg))
                .collect::<Vec<_>>(),
            vec![
                (A, 1_000, Msg::note_on(0, 60, 100)),
                (A, 7_777, Msg::note_off(0, 60)),
            ],
            "block {block}"
        );
    }
}

#[test]
fn only_recording_tracks_keep_input_and_only_while_recording() {
    let mut rig = rig(vec![
        recorder(A),
        MidiTrack::new(B, Target::Instrument, None, Vec::new()).listening(keys()),
    ]);
    // Not recording yet: nothing is kept.
    rig.live.send(100, 100, Msg::note_on(0, 60, 100));
    rig.run(1_000, 128);
    assert!(rig.taken().is_empty());

    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(128, 128);
    assert!(rig.transport.transport().recording);
    rig.live.send(2_000, 2_000, Msg::note_on(0, 62, 100));
    rig.run(1_000, 128);
    let kept = rig.taken();
    assert_eq!(kept.len(), 1, "track B listens but does not record");
    assert_eq!(kept[0].track, A);

    rig.transport.send(Command::EndRecord).unwrap();
    rig.run(128, 128);
    assert!(!rig.transport.transport().recording);
    assert!(rig.transport.transport().playing, "playing carries on");
    rig.live.send(5_000, 5_000, Msg::note_off(0, 62));
    rig.run(1_000, 128);
    assert!(rig.taken().is_empty());
}

#[test]
fn the_count_in_holds_the_song_and_recording_starts_mid_block() {
    let mut rig = rig(vec![recorder(A)]);
    rig.transport.send(Command::Record { count_in: 4 }).unwrap();
    // The first clicks sound with the metronome off.
    let first = rig.run(2_000, 77);
    assert!(
        first.iter().any(|v| v.abs() > 0.05),
        "the count-in clicks without the metronome"
    );
    let snap = rig.transport.transport();
    assert!(snap.recording && !snap.playing);
    assert_eq!(snap.sample, 0, "the song waits");
    assert_eq!(snap.count_in_total, 4 * BEAT as i64);
    assert_eq!(snap.count_in_left, 4 * BEAT as i64 - 2_000);

    // A note played in the count-in is not kept.
    rig.live.send(5_000, 5_000, Msg::note_on(0, 60, 100));
    rig.run(4 * BEAT as usize - 2_000 + 1_000, 77);
    let snap = rig.transport.transport();
    assert!(snap.playing && snap.recording && snap.count_in_left == 0);
    assert!(rig.taken().is_empty(), "nothing is kept before the take");

    // The take begins on the frame after the count: device frame 96,000.
    rig.live
        .send(4 * BEAT + 1_500, 4 * BEAT + 1_500, Msg::note_on(0, 64, 100));
    rig.run(3_000, 77);
    let kept = rig.taken();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].at, 1_500);
}

#[test]
fn the_count_in_accents_the_first_beat_of_a_bar() {
    let mut rig = rig(vec![recorder(A)]);
    rig.transport.send(Command::Record { count_in: 8 }).unwrap();
    let out = rig.run(8 * BEAT as usize, 256);
    let peak = |beat: usize| {
        out[beat * BEAT as usize..beat * BEAT as usize + 2_000]
            .iter()
            .fold(0.0_f32, |m, v| m.max(v.abs()))
    };
    // Downbeats (0 and 4) are louder than the beats between.
    assert!(peak(0) > peak(1) * 1.2, "{} vs {}", peak(0), peak(1));
    assert!(peak(4) > peak(5) * 1.2);
    assert!(peak(7) > 0.05, "the last beat of the count sounds");
}

#[test]
fn stop_and_locate_end_a_count_in_without_moving_the_song() {
    for ending in [
        Command::Stop,
        Command::Pause,
        Command::Locate(Ticks(0)),
        Command::Panic,
    ] {
        let mut rig = rig(vec![recorder(A)]);
        rig.transport.send(Command::Record { count_in: 2 }).unwrap();
        rig.run(10_000, 128);
        assert!(rig.transport.transport().recording);
        rig.transport.send(ending).unwrap();
        rig.run(128, 128);
        let snap = rig.transport.transport();
        assert!(!snap.recording && !snap.playing, "{ending:?}");
        assert_eq!(snap.count_in_total, 0, "{ending:?}");
        rig.run(60_000, 128);
        assert!(!rig.transport.transport().playing, "{ending:?}");
    }
}

#[test]
fn stop_ends_recording_and_the_next_play_does_not_record() {
    let mut rig = rig(vec![recorder(A)]);
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(1_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(128, 128);
    assert!(!rig.transport.transport().recording);
    rig.transport.send(Command::Play).unwrap();
    rig.run(128, 128);
    rig.live.send(2_000, 2_000, Msg::note_on(0, 60, 100));
    rig.run(1_000, 128);
    assert!(rig.taken().is_empty());
}

#[test]
fn a_note_played_just_before_the_loop_wraps_is_kept_at_the_loop_end() {
    let mut rig = rig(vec![recorder(A)]);
    rig.transport
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(2),
            true,
        )))
        .unwrap();
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    // The loop is 48,000 frames; the note was played 50 frames before the
    // wrap and is heard 70 frames after it.
    rig.live.send(47_950, 48_070, Msg::note_on(0, 60, 100));
    rig.live.send(48_500, 48_500, Msg::note_off(0, 60));
    rig.run(50_000, 100);
    let kept = rig.taken();
    assert_eq!(
        kept.iter().map(|c| c.at).collect::<Vec<_>>(),
        vec![47_950, 500]
    );
}

#[test]
fn recording_while_playing_starts_at_once() {
    let mut rig = rig(vec![recorder(A)]);
    rig.transport.send(Command::Play).unwrap();
    rig.run(4_800, 128);
    rig.transport.send(Command::Record { count_in: 4 }).unwrap();
    rig.run(128, 128);
    let snap = rig.transport.transport();
    assert!(snap.recording && snap.playing && snap.count_in_total == 0);
}
