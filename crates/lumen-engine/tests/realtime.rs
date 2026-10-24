//! The engine's audio side never touches the heap: not while playing,
//! looping, locating, swapping timelines or following a sample-rate change.
//! Built with `RTSAN_ENABLE=1` on Linux, RealtimeSanitizer also checks for
//! locks and blocking calls in the same sections (ADR-0010).

use lumen_core::meter::Meter;
use lumen_core::time::Ticks;
use lumen_engine::{
    Command, Jump, Output, RecordTiming, Region, RenderContext, ScrubStyle, Source, Timeline,
    engine,
};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

/// A source with some per-sample work and state, like a real one.
struct Tone {
    phase: f32,
}

impl Source for Tone {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
        let step = 440.0 / cx.sample_rate as f32;
        for channel in 0..out.channels() {
            let mut phase = self.phase;
            for sample in out.channel(channel) {
                *sample += (phase * std::f32::consts::TAU).sin() * 0.1;
                phase = (phase + step).fract();
            }
            if channel + 1 == out.channels() {
                self.phase = phase;
            }
        }
    }

    fn locate(&mut self, _: &Timeline, _: u32, _: i64, _: Jump) {
        self.phase = 0.0;
    }
}

#[test]
fn process_never_touches_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let (mut handle, mut engine) = engine(Timeline::default(), Tone { phase: 0.0 }, 48_000);
    let mut data = vec![0.0; 2 * 128];
    let bar = |n: i64| Ticks::quarters(4 * n);

    for block in 0..5_000_u32 {
        // UI-side work happens outside the real-time section, as in the app.
        match block {
            0 => handle.send(Command::Play).unwrap(),
            10 => handle
                .send(Command::SetLoop(Region::new(bar(0), bar(1), true)))
                .unwrap(),
            20 => handle
                .send(Command::SetPunch(Region::new(bar(0), bar(1), true)))
                .unwrap(),
            _ if block % 500 == 250 => handle
                .set_timeline(Timeline::constant(
                    80.0 + f64::from(block / 50),
                    Meter::FOUR_FOUR,
                ))
                .unwrap(),
            _ if block % 700 == 300 => handle.send(Command::Locate(bar(3))).unwrap(),
            2_000 => handle.send(Command::Pause).unwrap(),
            2_100 => handle.send(Command::Play).unwrap(),
            3_000 => handle.send(Command::Stop).unwrap(),
            3_050 => handle.send(Command::Play).unwrap(),
            _ => {}
        }
        // The device changes rate halfway through.
        let rate = if block < 2_500 { 48_000 } else { 44_100 };
        let ((), heap) = count_rt_violations(|| engine.process(rate, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        let _ = handle.transport();
        handle.collect_garbage();
    }
}

#[test]
fn split_clip_seam_resolution_never_touches_the_heap() {
    use std::sync::Arc;

    use lumen_engine::{Clip, ClipAudio, Destination, NodeSpec, Routing, clip_player, mixer};

    assert!(guard_is_active(), "guarded allocator not installed");
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 48_000], vec![0.1; 48_000]],
    });
    let make = |id, start, offset| Clip {
        envelope: Default::default(),
        id,
        track: 0,
        start,
        offset,
        length: 24_000,
        gain: 1.0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut clips, player) = clip_player();
    clips
        .set(vec![
            make(1, Ticks::ZERO, 0),
            make(2, Ticks::quarters(1), 24_000),
        ])
        .unwrap();
    let (mut routing, source) = mixer(player);
    assert!(
        routing
            .set(&Routing {
                nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    let (mut handle, mut engine) = engine(Timeline::default(), source, 48_000);
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    for _ in 0..2_000 {
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation while resolving split seams");
        let _ = handle.transport();
        clips.collect_garbage();
        routing.collect_garbage();
    }
}

#[test]
fn scrubbing_never_touches_the_heap() {
    let (mut handle, mut engine) = engine(Timeline::default(), Tone { phase: 0.0 }, 48_000);
    let mut data = vec![0.0; 2 * 128];
    for block in 0..3_000_i64 {
        // Both styles, moving like a hand at 60 frames a second, released
        // and grabbed again; then Play over a scrub.
        let at = Ticks(Ticks::quarters(1).0 * ((block / 8) % 40));
        let style = if block < 1_500 {
            ScrubStyle::Loop {
                length: Ticks(Ticks::quarters(1).0 / 4),
            }
        } else {
            ScrubStyle::Tape
        };
        match block % 700 {
            0..=599 if block % 6 == 0 => handle.send(Command::Scrub { at, style }).unwrap(),
            600 => handle.send(Command::EndScrub).unwrap(),
            _ => {}
        }
        if block == 2_900 {
            handle.send(Command::Play).unwrap();
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
    }
}

#[test]
fn delay_compensation_never_touches_the_heap() {
    use std::sync::Arc;

    use lumen_engine::testing::latent;
    use lumen_engine::{Clip, ClipAudio, clip_player, mixer};

    assert!(guard_is_active(), "guarded allocator not installed");
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 96_000], vec![0.1; 96_000]],
    });
    let clips: Vec<Clip> = (0..64)
        .map(|i| Clip {
            envelope: Default::default(),
            id: i,
            start: Ticks(i as i64 * 997),
            offset: 0,
            length: 96_000,
            gain: 0.5,
            track: i,
            audio: Arc::clone(&audio),
            shape: Default::default(),
        })
        .collect();
    let (mut clips_handle, player) = clip_player();
    let (mut routing, source) = mixer(player);
    let (mut handle, mut engine) = engine(Timeline::default(), source, 48_000);
    clips_handle.set(clips).unwrap();
    assert!(routing.set(&latent(64, 0)).unwrap());
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(2),
            true,
        )))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    for block in 0..3_000_i64 {
        // Latencies change while playing: plans of a new shape carry what
        // is in flight in their delay lines, and the engine follows.
        if block % 200 == 100 {
            let _ = routing.set(&latent(64, (block / 200) as u32));
        }
        match block % 700 {
            350 => handle.send(Command::Locate(Ticks::quarters(1))).unwrap(),
            500 => handle.send(Command::Pause).unwrap(),
            520 => handle.send(Command::Continue).unwrap(),
            600 => handle.send(Command::Stop).unwrap(),
            610 => handle.send(Command::Play).unwrap(),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        let _ = handle.transport();
        routing.collect_garbage();
        handle.collect_garbage();
    }
}

#[test]
fn automated_lanes_never_touch_the_heap() {
    use std::sync::Arc;

    use lumen_engine::testing::automated;
    use lumen_engine::{Clip, ClipAudio, clip_player, mixer};

    assert!(guard_is_active(), "guarded allocator not installed");
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 96_000], vec![0.1; 96_000]],
    });
    let clips: Vec<Clip> = (0..64)
        .map(|i| Clip {
            envelope: Default::default(),
            id: i,
            start: Ticks(i as i64 * 997),
            offset: 0,
            length: 96_000,
            gain: 0.5,
            track: i,
            audio: Arc::clone(&audio),
            shape: Default::default(),
        })
        .collect();
    let (mut clips_handle, player) = clip_player();
    let (mut routing, source) = mixer(player);
    let (mut handle, mut engine) = engine(Timeline::default(), source, 48_000);
    clips_handle.set(clips).unwrap();
    assert!(routing.set(&automated(64, 0)).unwrap());
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(8),
            true,
        )))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    for block in 0..3_000_i64 {
        // Lanes are edited while playing: a new plan of the same shape takes
        // over and every lane glides in; an odd variant moves a track and
        // crossfades.
        if block % 200 == 100 {
            let _ = routing.set(&automated(64, (block / 200) as u32));
        }
        match block % 700 {
            350 => handle.send(Command::Locate(Ticks::quarters(5))).unwrap(),
            500 => handle.send(Command::Pause).unwrap(),
            520 => handle.send(Command::Continue).unwrap(),
            600 => handle.send(Command::Stop).unwrap(),
            610 => handle.send(Command::Play).unwrap(),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        let _ = handle.transport();
        routing.collect_garbage();
        handle.collect_garbage();
    }
}

#[test]
fn routed_clips_metronome_and_tone_never_touch_the_heap() {
    use std::sync::Arc;

    use lumen_engine::testing::routed;
    use lumen_engine::{Clip, ClipAudio, clip_player, mixer};

    assert!(guard_is_active(), "guarded allocator not installed");
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 96_000], vec![0.1; 96_000]],
    });
    let clips = |offset: i64| -> Vec<Clip> {
        (0..64)
            .map(|i| Clip {
                envelope: Default::default(),
                id: i as u64,
                start: Ticks((i * 7_919 + offset) % (lumen_core::time::PPQ * 8)),
                offset: 0,
                length: 96_000,
                gain: 0.5,
                track: i as u64,
                audio: Arc::clone(&audio),
                // A third fade in and out along curves (read from tables).
                shape: if i % 3 == 0 {
                    lumen_engine::ClipShape {
                        fade_in: lumen_engine::FadeFrames {
                            frames: 4_800,
                            curve: lumen_core::curve::Curve::SLOW,
                            ..Default::default()
                        },
                        fade_out: lumen_engine::FadeFrames {
                            frames: 9_600,
                            curve: lumen_core::curve::Curve::S_CURVE,
                            ..Default::default()
                        },
                        period: None,
                        phase: 0,
                    }
                } else {
                    Default::default()
                },
            })
            .collect()
    };
    // With a cue bus on channels 3 and 4, a few tracks cued (M4 slice 4).
    let routed = |tracks: u64, variant: u32| {
        let mut r = routed(tracks, variant);
        r.cue = Some(lumen_engine::CueOut {
            channel: 2,
            volume: 0.8,
        });
        for node in r
            .nodes
            .iter_mut()
            .filter(|n| n.id % 9 == u64::from(variant) % 9)
        {
            node.cue = true;
        }
        r
    };
    let (mut clips_handle, player) = clip_player();
    // 64 tracks in groups, with sends to two returns (M4 slice 1).
    let (mut routing, source) = mixer(player);
    let (mut handle, mut engine) = engine(Timeline::default(), source, 48_000);
    clips_handle.set(clips(0)).unwrap();
    assert!(routing.set(&routed(64, 0)).unwrap());
    handle.send(Command::Play).unwrap();
    // The metronome and the test tone run on the audio thread too.
    handle
        .send(Command::SetMetronome(lumen_engine::MetronomeSettings {
            enabled: true,
            volume: 0.5,
            sound: lumen_engine::ClickSound::Wood,
            channel: 2,
        }))
        .unwrap();
    handle.send(Command::TestTone(true)).unwrap();
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(8),
            true,
        )))
        .unwrap();
    let mut data = vec![0.0; 4 * 128];
    for block in 0..3_000_i64 {
        // Now and then, and as fast as a drag (moved clips crossfade).
        let dragging = (1_000..1_400).contains(&block) && block % 6 == 0;
        if block % 300 == 150 || dragging {
            // Built and sent from the UI side, outside the real-time section.
            let _ = clips_handle.set(clips(block));
        }
        // Levels change as fast as a fader drag; every other one reroutes
        // a track, which crossfades between plans.
        if (1_500..2_000).contains(&block) && block % 6 == 0 {
            let _ = routing.set(&routed(64, (block / 6) as u32));
        }
        if block % 700 == 350 {
            handle.send(Command::Locate(Ticks::quarters(3))).unwrap();
        }
        // Stopped for a while, with edits arriving: taken while idle (M4
        // slice 7).
        match block {
            2_200 => handle.send(Command::Stop).unwrap(),
            2_600 => handle.send(Command::Play).unwrap(),
            2_201..2_600 if block % 10 == 0 => {
                let _ = clips_handle.set(clips(block));
                let _ = routing.set(&routed(64, (block / 10) as u32));
            }
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 4, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        clips_handle.collect_garbage();
        routing.collect_garbage();
        handle.collect_garbage();
    }
}

#[test]
fn overlapping_crossfade_never_touches_the_heap() {
    use std::sync::Arc;

    use lumen_engine::testing::routed;
    use lumen_engine::{Clip, ClipAudio, FadeFrames, clip_player, mixer};

    assert!(guard_is_active(), "guarded allocator not installed");
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.2; 30_000], vec![0.2; 30_000]],
    });
    let blend = FadeFrames {
        frames: 1_000,
        blend: true,
        ..Default::default()
    };
    let left = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 25_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: lumen_engine::ClipShape {
            fade_out: blend,
            ..Default::default()
        },
    };
    let right = Clip {
        envelope: Default::default(),
        id: 2,
        start: Ticks::quarters(1),
        offset: 25_000,
        length: 2_000,
        gain: 1.0,
        track: 0,
        audio,
        shape: lumen_engine::ClipShape {
            fade_in: blend,
            ..Default::default()
        },
    };
    let (mut clips, player) = clip_player();
    let (mut routing, source) = mixer(player);
    let (mut handle, mut engine) = engine(Timeline::default(), source, 48_000);
    clips.set(vec![left, right]).unwrap();
    assert!(routing.set(&routed(1, 0)).unwrap());
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    for block in 0..210 {
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in crossfade block {block}");
    }
}

#[test]
fn midi_never_touches_the_heap() {
    use lumen_core::midi::{Event, Kind};
    use lumen_devices::Instrument;
    use lumen_engine::testing::routed;
    use lumen_engine::{
        InstrumentSpec, Layered, Listen, LiveIn, LiveInput, MidiClip, MidiControl, MidiNote,
        MidiTrack, Target, clip_player, control_list, midi_player, mixer, note_list,
    };

    /// Plays a chord on and off every few hundred frames, from its own
    /// counter: live input that allocates nothing.
    struct Player {
        next: u64,
        on: bool,
    }
    impl LiveInput for Player {
        fn pull(&mut self, _start: u64, end: u64, take: &mut dyn FnMut(LiveIn)) {
            while self.next < end {
                for key in [60, 64, 67] {
                    let msg = if self.on {
                        lumen_core::midi::Msg::note_off(0, key)
                    } else {
                        lumen_core::midi::Msg::note_on(0, key, 100)
                    };
                    take(LiveIn {
                        frame: self.next,
                        played: self.next,
                        source: 3,
                        track: None,
                        msg,
                    });
                    // The piano roll's preview, on an instrument track and
                    // on a hardware one.
                    for track in [5, 4] {
                        take(LiveIn {
                            frame: self.next,
                            played: self.next,
                            source: lumen_engine::AUDITION,
                            track: Some(track),
                            msg,
                        });
                    }
                }
                self.on = !self.on;
                self.next += 333;
            }
        }
        fn waiting(&self) -> bool {
            false
        }
    }

    /// Does per-sample work for every sounding note, with state, like a
    /// real instrument, and allocates nothing.
    struct Counter {
        held: [bool; 128],
        phase: f32,
    }
    impl Instrument for Counter {
        fn render(&mut self, _rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]) {
            let mut next = 0;
            for i in 0..left.len() {
                while next < events.len() && events[next].offset as usize <= i {
                    match events[next].msg.kind() {
                        Kind::NoteOn { key, .. } => self.held[usize::from(key)] = true,
                        Kind::NoteOff { key, .. } => self.held[usize::from(key)] = false,
                        _ => {}
                    }
                    next += 1;
                }
                let voices = self.held.iter().filter(|h| **h).count() as f32;
                self.phase = (self.phase + 0.01).fract();
                left[i] += self.phase * 0.01 * voices;
                right[i] += self.phase * 0.01 * voices;
            }
        }
        fn active(&self) -> bool {
            self.held.iter().any(|h| *h)
        }
        fn silence(&mut self) {
            self.held = [false; 128];
        }
    }

    assert!(guard_is_active(), "guarded allocator not installed");
    let tracks = |variant: i64| -> Vec<MidiTrack> {
        (0..32_u64)
            .map(|id| {
                let notes: Vec<MidiNote> = (0..24)
                    .map(|n| MidiNote {
                        start: Ticks(n * 97_001 + id as i64 * 31 + variant),
                        length: Ticks::quarters(1 + n % 3),
                        pitch: 40 + ((n * 7 + id as i64) % 40) as u8,
                        velocity: 90,
                    })
                    .collect();
                let controls = vec![MidiControl {
                    at: Ticks::quarters(1),
                    msg: lumen_core::midi::Msg::controller(0, 64, 127),
                }];
                let clip = MidiClip {
                    id,
                    start: Ticks::ZERO,
                    length: Ticks::quarters(16),
                    period: Some(Ticks::quarters(3)),
                    seams: Default::default(),
                    notes: note_list(notes),
                    controls: control_list(controls),
                };
                // Every fourth track goes to hardware.
                let target = if id % 4 == 0 {
                    Target::Port {
                        port: (id % 3) as u16,
                        channel: 2,
                    }
                } else {
                    Target::Instrument
                };
                MidiTrack::new(
                    id,
                    target,
                    Some(InstrumentSpec {
                        kind: 1,
                        preset: (variant % 3) as u32,
                        instrument: Box::new(Counter {
                            held: [false; 128],
                            phase: 0.0,
                        }),
                    }),
                    vec![clip],
                )
                // Half the edits change what the tracks listen to.
                .listening(Listen {
                    sources: 1 << 3,
                    channel: (variant % 2 == 1).then_some(0),
                })
                // And every other track records what it is sent.
                .recording(if id % 2 == 0 {
                    Listen {
                        sources: 1 << 3,
                        channel: None,
                    }
                } else {
                    Listen::OFF
                })
                .recording_timing((id % 2 == 0).then_some(RecordTiming {
                    start: Ticks::ZERO,
                    latency_samples: 128,
                    looped: Some((Ticks::ZERO, Ticks::quarters(5))),
                    punch: Some((Ticks::quarters(1), Ticks::quarters(3))),
                }))
            })
            .collect()
    };
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    let mut output = midi.take_output().unwrap();
    let mut captured = midi.take_captured().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    let (mut handle, mut engine) = engine(Timeline::default(), source, 48_000);
    midi.set(tracks(0)).ok().unwrap();
    // Two ports get MIDI clock (M5 slice 7).
    midi.set_sync(0b101);
    midi.set_live(Box::new(Player { next: 0, on: false }))
        .ok()
        .unwrap();
    assert!(routing.set(&routed(32, 0)).unwrap());
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(5),
            true,
        )))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    for block in 0..4_000_i64 {
        // UI-side work happens outside the real-time section.
        if block % 150 == 75 {
            midi.collect_garbage();
            let _ = midi.set(tracks(block % 5));
        }
        if block % 2_000 == 1_000 {
            // Sync switched on and off while playing.
            midi.set_sync(if block % 4_000 == 1_000 { 0 } else { 0b101 });
        }
        if block % 1_000 == 999 {
            // A new live input (the app gives one when MIDI restarts).
            let _ = midi.set_live(Box::new(Player {
                next: block as u64 * 128,
                on: false,
            }));
        }
        match block % 900 {
            100 => handle.send(Command::Record { count_in: 0 }).unwrap(),
            300 => handle.send(Command::Locate(Ticks::quarters(2))).unwrap(),
            400 => handle.send(Command::EndRecord).unwrap(),
            // Stopped (after 700), a count-in begins and Play is ignored
            // until Panic ends it.
            705 => handle.send(Command::Record { count_in: 1 }).unwrap(),
            500 => handle.send(Command::Pause).unwrap(),
            520 => handle.send(Command::Continue).unwrap(),
            700 => handle.send(Command::Stop).unwrap(),
            720 => handle.send(Command::Play).unwrap(),
            850 => handle.send(Command::Panic).unwrap(),
            880 => handle.send(Command::Play).unwrap(),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        while output.pop().is_ok() {}
        while captured.pop().is_ok() {}
        let _ = handle.transport();
        routing.collect_garbage();
    }
}

/// Review M5 H1: with the queue that takes old lists back full (the UI late
/// to collect), two lists arriving in one block must still leave every old
/// one to be freed off the audio thread, for clips, MIDI and the mixer.
#[test]
fn lists_queued_behind_a_full_retire_queue_are_never_freed_here() {
    use lumen_engine::testing::routed;
    use lumen_engine::{Layered, clip_player, midi_player, mixer};

    assert!(guard_is_active(), "guarded allocator not installed");
    let (mut clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    let (mut routing, source) = mixer(Layered(players, player));
    let (_handle, mut engine) = engine(Timeline::default(), source, 48_000);
    let mut data = vec![0.0; 2 * 128];
    let mut variant = 0;
    // Nothing is collected: one list a block fills each retire queue, then
    // two in one block meet it full, then the stashes drain one a block.
    for block in 0..12_u32 {
        let lists = match block {
            0..5 => 1,
            5 | 6 => 2,
            _ => 0,
        };
        for _ in 0..lists {
            variant += 1;
            let _ = clips.set(Vec::new());
            let _ = midi.set(Vec::new());
            let _ = routing.set(&routed(2, variant));
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
    }
}

/// Review M5 M2: the built-in synth renders, steals voices, glides between
/// presets, bends, holds the pedal and silences without touching the heap.
#[test]
fn the_synth_never_touches_the_heap() {
    use lumen_core::midi::{Event, Msg};
    use lumen_devices::{Instrument, Preset, Synth};

    assert!(guard_is_active(), "guarded allocator not installed");
    let mut synth = Synth::new(Preset::Poly);
    let (mut left, mut right) = (vec![0.0; 128], vec![0.0; 128]);
    let mut events = Vec::with_capacity(64);
    for block in 0..3_000_u32 {
        events.clear();
        let key = 36 + (block % 48) as u8;
        // More notes than voices, so voices are stolen.
        for (n, offset) in [0, 31, 64, 97].into_iter().enumerate() {
            events.push(Event {
                offset,
                msg: Msg::note_on((n % 2) as u8, key + n as u8 * 4, 100),
            });
        }
        if block >= 8 {
            events.push(Event {
                offset: 100,
                msg: Msg::note_off(0, key - 8),
            });
        }
        match block % 500 {
            50 => events.push(Event {
                offset: 10,
                msg: Msg::program(0, ((block / 500) % 5) as u8),
            }),
            100 => events.push(Event {
                offset: 0,
                msg: Msg::controller(0, 64, 127),
            }),
            200 => events.push(Event {
                offset: 0,
                msg: Msg::controller(0, 64, 0),
            }),
            300 => events.push(Event {
                offset: 5,
                msg: Msg::pitch_bend(0, 4_000),
            }),
            400 => events.push(Event {
                offset: 0,
                msg: Msg::controller(0, 123, 0),
            }),
            _ => {}
        }
        events.sort_by_key(|e| e.offset);
        left.fill(0.0);
        right.fill(0.0);
        let ((), heap) = count_rt_violations(|| {
            synth.render(48_000, &events, &mut left, &mut right);
            if block % 1_000 == 999 {
                synth.silence();
            }
        });
        assert_eq!(heap, 0, "heap operation in the synth at block {block}");
    }
    assert!(synth.voices() > 0 || !synth.active());
}
