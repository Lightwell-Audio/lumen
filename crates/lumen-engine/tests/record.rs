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
    MidiClip, MidiHandle, MidiNote, MidiOut, MidiPlayer, MidiTrack, Mixer, MixerHandle, NodeSpec,
    RecordTiming, Region, Routing, Target, Timeline, WRAP, clip_player, engine, midi_player, mixer,
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
    output: rtrb::Consumer<MidiOut>,
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
        engine,
        live,
        captured,
        output,
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
    rig.live.send(200, 200, Msg::note_off(0, 60));
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

    // A note played and let go in the count-in is not kept.
    rig.live.send(5_000, 5_000, Msg::note_on(0, 60, 100));
    rig.live.send(5_100, 5_100, Msg::note_off(0, 60));
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
    let kept: Vec<Captured> = rig
        .taken()
        .into_iter()
        .filter(|c| c.source != WRAP)
        .collect();
    assert_eq!(
        kept.iter().map(|c| c.at).collect::<Vec<_>>(),
        vec![47_950, 500]
    );
    // The note began in the pass before the wrap, and ended after it.
    assert_eq!(kept[1].pass, kept[0].pass + 1);
}

#[test]
fn delayed_input_before_the_first_loop_entry_keeps_its_preloop_position() {
    let mut rig = rig(vec![recorder(A)]);
    rig.transport
        .send(Command::SetLoop(Region::new(
            Ticks::quarters(1),
            Ticks::quarters(3),
            true,
        )))
        .unwrap();
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(BEAT as usize + 128, 128);

    // The key went down 50 frames before loop start. Its input stamp reaches
    // the recorder 70 frames later, after the playhead entered the loop but
    // before the first loop wrap.
    rig.live
        .send(BEAT - 50, BEAT + 20, Msg::note_on(0, 60, 100));
    rig.run(512, 128);
    let kept: Vec<Captured> = rig
        .taken()
        .into_iter()
        .filter(|c| c.source != WRAP)
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].at, (BEAT - 50) as i64);
}

#[test]
fn a_previous_playback_wrap_does_not_shift_the_next_sessions_first_loop_entry() {
    let mut rig = rig(vec![recorder(A)]);
    rig.transport
        .send(Command::SetLoop(Region::new(
            Ticks::quarters(1),
            Ticks::quarters(3),
            true,
        )))
        .unwrap();
    rig.transport.send(Command::Play).unwrap();
    rig.run((4 * BEAT) as usize, 128);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(128, 128);
    rig.transport.send(Command::Locate(Ticks::ZERO)).unwrap();
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(BEAT as usize + 128, 128);

    // Input frames are device-relative across transport stops and starts.
    let session_start = 4 * BEAT + 128;
    rig.live.send(
        session_start + BEAT - 50,
        session_start + BEAT + 20,
        Msg::note_on(0, 60, 100),
    );
    rig.run(512, 128);
    let kept: Vec<Captured> = rig
        .taken()
        .into_iter()
        .filter(|c| c.source != WRAP)
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].at, (BEAT - 50) as i64);
}

#[test]
fn each_loop_wrap_marks_a_new_pass_for_every_recording_track() {
    let mut rig = rig(vec![recorder(A), recorder(B)]);
    rig.transport
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(2),
            true,
        )))
        .unwrap();
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    // Two and a half passes of the 48,000 frame loop.
    rig.run(120_000, 128);
    let marks: Vec<(u64, u32)> = rig
        .taken()
        .into_iter()
        .filter(|c| c.source == WRAP)
        .map(|c| (c.track, c.pass))
        .collect();
    assert_eq!(marks.len(), 4, "two wraps, two tracks: {marks:?}");
    let first = marks.iter().map(|m| m.1).min().unwrap();
    for track in [A, B] {
        let passes: Vec<u32> = marks.iter().filter(|m| m.0 == track).map(|m| m.1).collect();
        assert_eq!(passes, vec![first, first + 1]);
    }
}

#[test]
fn a_key_held_when_recording_begins_is_taken_at_the_start() {
    let mut rig = rig(vec![recorder(A)]);
    // Held before Record, and one let go before it.
    rig.live.send(100, 100, Msg::note_on(0, 60, 90));
    rig.live.send(150, 150, Msg::note_on(0, 62, 80));
    rig.live.send(300, 300, Msg::note_off(0, 62));
    rig.run(1_000, 128);
    assert!(rig.taken().is_empty());
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(1_024, 128);
    let kept = rig.taken();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].msg, Msg::note_on(0, 60, 90));
    assert!(kept[0].at < 200, "at the start: {}", kept[0].at);
    // Letting go is kept like any other message.
    rig.live.send(2_500, 2_500, Msg::note_off(0, 60));
    rig.run(1_024, 128);
    let kept = rig.taken();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].msg, Msg::note_off(0, 60));
}

#[test]
fn a_held_key_uses_the_latency_compensated_record_start_for_playback() {
    let q = |beats: f64| Ticks((beats * lumen_core::time::PPQ as f64) as i64);
    let clip = MidiClip {
        id: 92,
        start: Ticks::ZERO,
        length: q(4.0),
        period: None,
        seams: Default::default(),
        notes: vec![MidiNote {
            start: Ticks::ZERO,
            length: q(4.0),
            pitch: 60,
            velocity: 100,
        }]
        .into(),
        controls: Vec::new().into(),
    };
    let start = Timeline::default().ticks_at((BEAT + 256) as i64, 48_000);
    let track = MidiTrack::new(
        A,
        Target::Port {
            port: 4,
            channel: 0,
        },
        None,
        vec![clip],
    )
    .listening(keys())
    .recording(keys())
    .recording_within(Some((start, Ticks(i64::MAX))))
    .recording_timing(Some(RecordTiming {
        start,
        latency_samples: 512,
        looped: None,
        punch: None,
    }));
    let mut rig = rig(vec![track]);
    rig.transport.send(Command::Locate(q(1.0))).unwrap();
    rig.transport.send(Command::Play).unwrap();
    rig.run(128, 128);
    // This key is already held when Record begins. Its compensated position
    // falls before the boundary and must be floored back to Record start.
    rig.live.send(128, 128, Msg::note_on(0, 60, 90));
    rig.run(128, 128);
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(128, 128);
    let mut sent = Vec::new();
    while let Ok(MidiOut::At { frame, msg, .. }) = rig.output.pop() {
        sent.push((frame, msg));
    }
    assert!(
        sent.iter()
            .any(|&(frame, msg)| frame == 256 && msg == Msg::note_off(0, 60)),
        "the held key must take over the clip note at Record start: {sent:?}"
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

/// Overdubbing a key a clip already plays there (owner, after MT-MIDI-7 run
/// 3): the key played wins its pitch, as the take will once committed. The
/// clip's note sounding on it ends as the key goes down, its own note-off
/// later does not cut the played note, and a clip note on that key that
/// would start while it is held does not.
#[test]
fn a_key_played_while_recording_ends_the_clips_note_on_that_key() {
    let q = |beats: f64| Ticks((beats * lumen_core::time::PPQ as f64) as i64);
    let note = |start: f64, length: f64| MidiNote {
        start: q(start),
        length: q(length),
        pitch: 60,
        velocity: 100,
    };
    let clip = MidiClip {
        id: 9,
        start: Ticks::ZERO,
        length: q(8.0),
        period: None,
        seams: Default::default(),
        notes: vec![note(0.0, 4.0), note(1.2, 0.1), note(5.0, 1.0)].into(),
        controls: Vec::new().into(),
    };
    let track = MidiTrack::new(
        A,
        Target::Port {
            port: 4,
            channel: 0,
        },
        None,
        vec![clip],
    )
    .listening(keys())
    .recording(keys());
    let mut rig = rig(vec![track]);
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(128, 128);
    rig.live.send(BEAT, BEAT, Msg::note_on(0, 60, 90));
    rig.live
        .send(BEAT * 3 / 2, BEAT * 3 / 2, Msg::note_off(0, 60));
    rig.run(BEAT as usize * 6, 128);
    let mut sent = Vec::new();
    while let Ok(MidiOut::At { frame, msg, .. }) = rig.output.pop() {
        sent.push((frame, msg));
    }
    let start = sent.first().map_or(0, |&(f, _)| f);
    let at = |(f, m): &(u64, Msg)| (f - start, *m);
    let sent: Vec<(u64, Msg)> = sent.iter().map(at).collect();
    assert_eq!(
        sent,
        vec![
            (0, Msg::note_on(0, 60, 100)),
            // The key goes down: the clip's note ends, the played one starts.
            (BEAT, Msg::note_off(0, 60)),
            (BEAT, Msg::note_on(0, 60, 90)),
            (BEAT * 3 / 2, Msg::note_off(0, 60)),
            // After the key is let go, the clip plays on as before.
            (BEAT * 5, Msg::note_on(0, 60, 100)),
            (BEAT * 6, Msg::note_off(0, 60)),
        ],
        "no clip note at 1.2 beats under the held key, and no off at 4"
    );
}

/// After punch-out, recording goes on and keeps what is played, hidden in
/// the take: a key played there does not win its pitch. With the monitor
/// Off the key sounds nothing, and the clip's notes on it play as before:
/// the one sounding is not cut, and one that starts while it is held is not
/// held back.
#[test]
fn a_key_played_after_punch_out_leaves_the_clips_notes_alone() {
    let q = |beats: f64| Ticks((beats * lumen_core::time::PPQ as f64) as i64);
    let note = |start: f64, length: f64| MidiNote {
        start: q(start),
        length: q(length),
        pitch: 60,
        velocity: 100,
    };
    let clip = MidiClip {
        id: 9,
        start: Ticks::ZERO,
        length: q(8.0),
        period: None,
        seams: Default::default(),
        notes: vec![note(0.0, 4.0), note(5.0, 1.0)].into(),
        controls: Vec::new().into(),
    };
    // Monitor Off: the track records but does not listen.
    let track = MidiTrack::new(
        A,
        Target::Port {
            port: 4,
            channel: 0,
        },
        None,
        vec![clip],
    )
    .recording(keys())
    .recording_within(Some((Ticks::ZERO, q(2.0))));
    let mut rig = rig(vec![track]);
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(128, 128);
    // Played after punch-out (beat 2), held across the next clip note.
    rig.live.send(BEAT * 3, BEAT * 3, Msg::note_on(0, 60, 90));
    rig.live
        .send(BEAT * 11 / 2, BEAT * 11 / 2, Msg::note_off(0, 60));
    rig.run(BEAT as usize * 7, 128);
    let kept = rig.taken();
    assert_eq!(kept.len(), 2, "still captured: {kept:?}");
    let mut sent = Vec::new();
    while let Ok(MidiOut::At { frame, msg, .. }) = rig.output.pop() {
        sent.push((frame, msg));
    }
    let start = sent.first().map_or(0, |&(f, _)| f);
    let sent: Vec<(u64, Msg)> = sent.iter().map(|&(f, m)| (f - start, m)).collect();
    assert_eq!(
        sent,
        vec![
            (0, Msg::note_on(0, 60, 100)),
            (BEAT * 4, Msg::note_off(0, 60)),
            (BEAT * 5, Msg::note_on(0, 60, 100)),
            (BEAT * 6, Msg::note_off(0, 60)),
        ],
        "the clip plays whole under a key played after punch-out"
    );
}

/// Inside the window the key still wins its pitch, as before.
#[test]
fn a_key_played_inside_the_recording_window_still_wins_its_pitch() {
    let q = |beats: f64| Ticks((beats * lumen_core::time::PPQ as f64) as i64);
    let clip = MidiClip {
        id: 9,
        start: Ticks::ZERO,
        length: q(8.0),
        period: None,
        seams: Default::default(),
        notes: vec![MidiNote {
            start: Ticks::ZERO,
            length: q(4.0),
            pitch: 60,
            velocity: 100,
        }]
        .into(),
        controls: Vec::new().into(),
    };
    let track = MidiTrack::new(
        A,
        Target::Port {
            port: 4,
            channel: 0,
        },
        None,
        vec![clip],
    )
    .recording(keys())
    .recording_within(Some((Ticks::ZERO, q(2.0))));
    let mut rig = rig(vec![track]);
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(128, 128);
    rig.live.send(BEAT, BEAT, Msg::note_on(0, 60, 90));
    rig.live.send(BEAT * 3, BEAT * 3, Msg::note_off(0, 60));
    rig.run(BEAT as usize * 5, 128);
    let mut sent = Vec::new();
    while let Ok(MidiOut::At { frame, msg, .. }) = rig.output.pop() {
        sent.push((frame, msg));
    }
    let start = sent.first().map_or(0, |&(f, _)| f);
    let sent: Vec<(u64, Msg)> = sent.iter().map(|&(f, m)| (f - start, m)).collect();
    assert_eq!(
        sent,
        vec![(0, Msg::note_on(0, 60, 100)), (BEAT, Msg::note_off(0, 60))],
        "the clip's note ends where the key goes down"
    );
}

#[test]
fn compensated_live_keys_follow_both_punch_edges() {
    let q = |beats: f64| Ticks((beats * lumen_core::time::PPQ as f64) as i64);
    let clip = MidiClip {
        id: 93,
        start: Ticks::ZERO,
        length: q(4.0),
        period: None,
        seams: Default::default(),
        notes: vec![60, 62]
            .into_iter()
            .map(|pitch| MidiNote {
                start: Ticks::ZERO,
                length: q(4.0),
                pitch,
                velocity: 100,
            })
            .collect::<Vec<_>>()
            .into(),
        controls: Vec::new().into(),
    };
    let track = MidiTrack::new(
        A,
        Target::Port {
            port: 4,
            channel: 0,
        },
        None,
        vec![clip],
    )
    // Monitor Off: the inputs affect playback eligibility without sounding
    // through the live monitor.
    .recording(keys())
    .recording_within(Some((q(1.0), q(2.0))))
    .recording_timing(Some(RecordTiming {
        start: Ticks::ZERO,
        latency_samples: 128,
        looped: None,
        punch: Some((q(1.0), q(2.0))),
    }));
    let mut rig = rig(vec![track]);
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.live.send(BEAT + 64, BEAT + 64, Msg::note_on(0, 60, 90));
    rig.live
        .send(2 * BEAT + 64, 2 * BEAT + 64, Msg::note_on(0, 62, 90));
    rig.run((2 * BEAT + 2_000) as usize, 128);
    let mut sent = Vec::new();
    while let Ok(MidiOut::At { frame, msg, .. }) = rig.output.pop() {
        sent.push((frame, msg));
    }
    assert!(
        !sent
            .iter()
            .any(|&(frame, msg)| frame == BEAT + 64 && msg == Msg::note_off(0, 60)),
        "latency puts the punch-in key before the window, so playback continues: {sent:?}"
    );
    assert!(
        sent.iter()
            .any(|&(frame, msg)| frame == 2 * BEAT + 64 && msg == Msg::note_off(0, 62)),
        "latency puts the punch-out key before the edge, inside the window: {sent:?}"
    );
}

/// Before the app publishes its post-wrap window, a live key at the loop head
/// still uses the saved loop window and cuts the note playing there.
#[test]
fn a_key_after_the_first_wrap_uses_the_saved_loop_window_before_ui_refresh() {
    let q = |beats: f64| Ticks((beats * lumen_core::time::PPQ as f64) as i64);
    let clip = MidiClip {
        id: 91,
        start: Ticks::ZERO,
        length: q(2.0),
        period: None,
        seams: Default::default(),
        notes: vec![MidiNote {
            start: Ticks::ZERO,
            length: q(1.0),
            pitch: 60,
            velocity: 100,
        }]
        .into(),
        controls: Vec::new().into(),
    };
    let track = MidiTrack::new(
        A,
        Target::Port {
            port: 4,
            channel: 0,
        },
        None,
        vec![clip],
    )
    .listening(keys())
    .recording(keys())
    // The UI still holds the initial pass's lower bound after the wrap.
    .recording_within(Some((q(1.0), Ticks(i64::MAX))))
    .recording_timing(Some(RecordTiming {
        start: q(1.0),
        latency_samples: 0,
        looped: Some((Ticks::ZERO, q(2.0))),
        punch: None,
    }));
    let mut rig = rig(vec![track]);
    rig.transport
        .send(Command::SetLoop(Region::new(Ticks::ZERO, q(2.0), true)))
        .unwrap();
    rig.transport.send(Command::Locate(q(1.0))).unwrap();
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(24_128, 128);
    // The device frame keeps increasing; the heard timeline is 200 samples
    // into the new pass, before the old UI window's q(1) lower edge.
    rig.live.send(24_200, 24_200, Msg::note_on(0, 60, 90));
    rig.run(1_000, 128);
    let mut sent = Vec::new();
    while let Ok(MidiOut::At { frame, msg, .. }) = rig.output.pop() {
        sent.push((frame, msg));
    }
    assert!(
        sent.iter()
            .any(|&(frame, msg)| frame == 24_200 && msg == Msg::note_off(0, 60)),
        "the clip note must yield at the post-wrap live key: {sent:?}"
    );
}
