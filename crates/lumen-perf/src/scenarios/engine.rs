//! The engine scenarios: playing clips, crossfades, routed and latent mixes,
//! synths, fake plugin hosts, the transport and disk streams.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_engine::testing::{automated, latent, routed};
use lumen_engine::{
    ClickSound, Clip, ClipAudio, Command, Destination, EngineHandle, MetronomeSettings,
    MixerHandle, NodeSpec, PAGE, PagePool, PoolWriter, Region, Routing, StreamAudio, Timeline,
    clip_player, engine, mixer,
};

use super::{FRAMES, Mix, Player, RATE, SynthPlayer};

pub(crate) fn crossfading_clips() -> Player {
    let audio = Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![vec![0.01; 480_000]; 2],
    });
    let (mut clips, player) = clip_player();
    let mut list = Vec::new();
    for track in 0..64 {
        let mut left = Clip {
            id: track * 2,
            track,
            start: Ticks::ZERO,
            offset: 0,
            length: 264_000,
            gain: 0.5,
            audio: audio.clone(),
            shape: Default::default(),
            envelope: Default::default(),
        };
        let mut right = left.clone();
        right.id += 1;
        right.start = Ticks::quarters(1);
        right.length = 240_000;
        right.shape.fade_in = lumen_engine::FadeFrames {
            frames: 240_000,
            blend: true,
            ..Default::default()
        };
        left.shape.fade_out = right.shape.fade_in;
        list.extend([left, right]);
    }
    clips.set(list).expect("room for a clip list");
    let (mut routing, source) = mixer(player);
    routing
        .set(&Routing {
            nodes: (0..64)
                .map(|id| NodeSpec::unity(id, Destination::Hardware(0)))
                .collect(),
            cue: None,
            measure: None,
        })
        .expect("valid crossfade routing");
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport
        .send(Command::Locate(Ticks::quarters(1)))
        .expect("room for locate");
    transport.send(Command::Play).expect("room for play");
    let mut data = [0.0; 2 * FRAMES];
    for _ in 0..10 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    engine
}

/// `clips` clips playing, mixed as `mix`.
pub(crate) fn playing_clips(clips: usize, mix: Mix) -> Player {
    playing_clips_routed(clips, mix).1
}

/// As [`playing_clips`], with the handle that sends it new routings.
pub(crate) fn playing_clips_routed(clips: usize, mix: Mix) -> (MixerHandle, Player) {
    let routed_mix = mix != Mix::Direct;
    let audio = Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![vec![0.1; 480_000], vec![-0.1; 480_000]],
    });
    let (mut handle, player) = clip_player();
    handle
        .set(
            (0..clips)
                .map(|i| Clip {
                    envelope: Default::default(),
                    id: i as u64,
                    start: Ticks::ZERO,
                    offset: 0,
                    length: 480_000,
                    gain: 0.5,
                    track: if routed_mix { i as u64 } else { 0 },
                    audio: Arc::clone(&audio),
                    shape: Default::default(),
                })
                .collect(),
        )
        .expect("room for one clip list");
    let (mut routing, source) = mixer(player);
    let plan = match mix {
        Mix::Routed => routed(clips as u64, 0),
        Mix::Latent => latent(clips as u64, 0),
        Mix::Automated => automated(clips as u64, 0),
        Mix::Direct => Routing {
            nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
            cue: None,
            measure: None,
        },
    };
    assert!(routing.set(&plan).expect("a valid routing"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).expect("room for a command");
    let mut data = vec![0.0; 2 * FRAMES];
    // Past the fade-in, into steady playback.
    for _ in 0..10 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    (routing, engine)
}

/// `tracks` MIDI tracks, each a four-note chord on the synth that repeats every
/// beat and rings on past it (so voices overlap while they release), mixed as
/// in [`routed`], playing.
pub(crate) fn playing_synths(tracks: u64) -> SynthPlayer {
    use lumen_devices::{Preset, Synth, synth};
    use lumen_engine::{InstrumentSpec, Layered, MidiClip, MidiNote, MidiTrack, Target};
    let (_clips, players) = clip_player();
    let (mut midi, player) = lumen_engine::midi_player();
    let q = Ticks::quarters;
    let list = (0..tracks)
        .map(|id| {
            let notes: Vec<MidiNote> = [0_u8, 4, 7, 11]
                .iter()
                .map(|&interval| MidiNote {
                    start: Ticks(id as i64 * 17),
                    length: Ticks(q(1).0 * 3 / 4),
                    pitch: 48 + (id % 12) as u8 + interval,
                    velocity: 70 + (id % 40) as u8,
                    channel: None,
                })
                .collect();
            MidiTrack::new(
                id,
                Target::Instrument,
                Some(InstrumentSpec {
                    kind: synth::KIND,
                    preset: Preset::Poly.number(),
                    values: Vec::new(),
                    instrument: Box::new(Synth::new(Preset::Poly)),
                }),
                vec![MidiClip {
                    id,
                    start: Ticks::ZERO,
                    length: q(64),
                    period: Some(q(1)),
                    seams: Default::default(),
                    notes: lumen_engine::note_list(notes),
                    controls: lumen_engine::control_list(Vec::new()),
                }],
            )
        })
        .collect();
    midi.set(list).ok().expect("room for one track list");
    let (mut routing, source) = mixer(Layered(players, player));
    assert!(routing.set(&routed(tracks, 0)).expect("a valid routing"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).expect("room for a command");
    let mut data = vec![0.0; 2 * FRAMES];
    // Past the fade-in, into steady playback.
    for _ in 0..10 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    engine
}

/// A host that is not one: it answers inside `submit`, with no thread (the
/// scenarios may not use any). An instrument rings a level that each note-on
/// resets and each block lets fall; an effect halves its input.
struct Echo {
    effect: bool,
    level: f32,
    out: [[f32; FRAMES]; 2],
    frames: usize,
}

impl Echo {
    fn boxed(effect: bool) -> Box<Self> {
        Box::new(Self {
            effect,
            level: 0.0,
            out: [[0.0; FRAMES]; 2],
            frames: 0,
        })
    }
}

impl lumen_devices::Hosted for Echo {
    fn submit(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        events: &[lumen_core::midi::Event],
        _: &lumen_devices::ParamBlock,
    ) -> Result<(), lumen_devices::Refused> {
        self.frames = frames;
        if self.effect {
            for (out, from) in self.out.iter_mut().zip(input) {
                for (to, sample) in out[..frames].iter_mut().zip(*from) {
                    *to = sample * 0.5;
                }
            }
        } else {
            if !events.is_empty() {
                self.level = 1.0;
            }
            for out in &mut self.out {
                out[..frames].fill(self.level * 0.1);
            }
            self.level *= 0.99;
        }
        Ok(())
    }

    fn collect(
        &mut self,
        _: std::time::Instant,
        left: &mut [f32],
        right: &mut [f32],
    ) -> lumen_devices::Reply {
        left.copy_from_slice(&self.out[0][..self.frames]);
        right.copy_from_slice(&self.out[1][..self.frames]);
        lumen_devices::Reply::Ready {
            latency: 0,
            tail: 0,
            restart: false,
            work_ns: 0,
        }
    }

    fn ended(&self) -> bool {
        false
    }
}

/// [`playing_synths`], with every instrument and a hosted effect on every
/// track in plugin hosts (fake ones): the engine's own cost of hosting.
pub(crate) fn playing_hosted(tracks: u64, lanes: bool) -> (MixerHandle, SynthPlayer) {
    use lumen_devices::{Guard, HostedInstrument};
    use lumen_engine::{Device, InstrumentSpec, Layered, MidiClip, MidiNote, MidiTrack, Target};
    let (_clips, players) = clip_player();
    let (mut midi, player) = lumen_engine::midi_player();
    let q = Ticks::quarters;
    let list = (0..tracks)
        .map(|id| {
            let notes: Vec<MidiNote> = [0_u8, 4, 7, 11]
                .iter()
                .map(|&interval| MidiNote {
                    start: Ticks(id as i64 * 17),
                    length: Ticks(q(1).0 * 3 / 4),
                    pitch: 48 + (id % 12) as u8 + interval,
                    velocity: 70 + (id % 40) as u8,
                    channel: None,
                })
                .collect();
            let guard = Guard::new(Echo::boxed(false), 0);
            MidiTrack::new(
                id,
                Target::Instrument,
                Some(InstrumentSpec {
                    kind: 77,
                    preset: 0,
                    values: Vec::new(),
                    instrument: Box::new(HostedInstrument::new(guard)),
                }),
                vec![MidiClip {
                    id,
                    start: Ticks::ZERO,
                    length: q(64),
                    period: Some(q(1)),
                    seams: Default::default(),
                    notes: lumen_engine::note_list(notes),
                    controls: lumen_engine::control_list(Vec::new()),
                }],
            )
        })
        .collect();
    midi.set(list).ok().expect("room for one track list");
    let (mut routing, source) = mixer(Layered(players, player));
    let mut plan = routed(tracks, 0);
    for node in plan.nodes.iter_mut().filter(|n| n.id < tracks) {
        node.devices = vec![Device::Hosted {
            host: 1_000 + node.id,
            latency: 0,
        }];
        if lanes {
            node.automation = Some(plugin_lanes(1_000 + node.id));
        }
    }
    let mut statuses = Vec::new();
    for id in 0..tracks {
        let guard = Guard::new(Echo::boxed(true), 0);
        statuses.push(guard.status());
        assert!(routing.attach_host(1_000 + id, guard).is_ok());
    }
    assert!(routing.set(&plan).expect("a valid routing"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).expect("room for a command");
    let mut data = vec![0.0; 2 * FRAMES];
    // Past the fade-in and the glide from each fallback to its host, into
    // steady playback.
    for _ in 0..400 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // The scenario must really use its hosts: none failed or missed a block.
    assert!(statuses.iter().all(|s| !s.failed() && s.late_blocks() == 0));
    (routing, engine)
}

/// A curved and a stepped lane on the plugin `host`, long enough that
/// every block of the scenario is inside them.
fn plugin_lanes(host: u64) -> lumen_engine::NodeAutomation {
    use lumen_core::automation::Shape;
    use lumen_engine::{AutoLane, AutoPoint, NodeAutomation, ParamLane};
    let lane = |param: u32, shape: Shape, step: i64| ParamLane {
        param,
        lane: AutoLane::new(
            (0..40)
                .map(|k| AutoPoint {
                    at: k * step,
                    value: (k % 2) as f32 * 0.6 + 0.2,
                    shape,
                })
                .collect(),
        )
        .expect("points"),
    };
    NodeAutomation {
        plugins: vec![
            (
                host,
                lane(1, Shape::Curve(lumen_core::curve::Curve::S_CURVE), 4_000),
            ),
            (host, lane(2, Shape::Step, 5_000)),
        ],
        ..Default::default()
    }
}

pub(crate) fn transport_only() -> (EngineHandle, Player) {
    let (_clips, player) = clip_player();
    let (_routing, source) = mixer(player);
    let (mut handle, engine) = engine(Timeline::default(), source, RATE);
    handle
        .send(Command::SetMetronome(MetronomeSettings {
            enabled: true,
            volume: 0.5,
            sound: ClickSound::Wood,
            channel: 0,
        }))
        .expect("room for a command");
    // Two quarters at 120 BPM: 48,000 samples, so 2,000 blocks wrap it
    // about five times.
    handle
        .send(Command::SetLoop(Region {
            start: Ticks::quarters(4),
            end: Ticks::quarters(6),
            enabled: true,
        }))
        .expect("room for a command");
    handle.send(Command::Play).expect("room for a command");
    (handle, engine)
}

/// Pages of each streamed file: 163,840 frames, more than the 1,010 blocks
/// play.
const STREAM_PAGES: usize = 40;

/// 64 streamed clips on one track, every page in memory: the audio
/// thread's page copy (M9 slice R6), beside `engine_clips_64`'s memory.
pub(crate) fn playing_streams() -> (Arc<PagePool>, Player) {
    let pool = PagePool::new(64 * STREAM_PAGES as u32);
    let mut writer = PoolWriter::new(&pool);
    let (left, right) = (vec![0.1; PAGE], vec![-0.1; PAGE]);
    let frames = (STREAM_PAGES * PAGE) as u64;
    let clips = (0..64u64)
        .map(|i| {
            let audio = StreamAudio::new(&pool, i + 1, RATE, 2, frames);
            for page in 0..STREAM_PAGES {
                assert!(writer.load(&audio, page, &left, &right), "room in the pool");
            }
            Clip {
                envelope: Default::default(),
                id: i,
                start: Ticks::ZERO,
                offset: 0,
                length: frames as i64,
                gain: 0.5,
                track: 0,
                audio: Arc::new(ClipAudio::Stream(audio)),
                shape: Default::default(),
            }
        })
        .collect();
    let (mut handle, player) = clip_player();
    handle.set(clips).expect("room for one clip list");
    let (mut routing, source) = mixer(player);
    assert!(
        routing
            .set(&Routing {
                nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
                cue: None,
                measure: None,
            })
            .expect("a valid routing")
    );
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).expect("room for a command");
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..10 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    (pool, engine)
}
