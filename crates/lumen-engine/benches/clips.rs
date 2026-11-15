use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_engine::testing::routed;
use lumen_engine::{
    Clip, ClipAudio, ClipPlayer, Command, Destination, Engine, Mixer, NodeSpec, Routing, Timeline,
    clip_player, engine, mixer,
};

fn main() {
    divan::main();
}

fn playing(clips: usize, routed_mix: bool) -> Engine<Mixer<ClipPlayer>> {
    let audio = Arc::new(ClipAudio::Memory {
        sample_rate: 48_000,
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
        .unwrap();
    let (mut routing, source) = mixer(player);
    let plan = if routed_mix {
        routed(clips as u64, 0)
    } else {
        Routing {
            nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
            cue: None,
            measure: None,
        }
    };
    assert!(routing.set(&plan).unwrap());
    let (mut transport, mut engine) = engine(Timeline::default(), source, 48_000);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 256];
    for _ in 0..10 {
        engine.process(48_000, &mut data, 2, 128);
    }
    engine
}

#[divan::bench(args = [1, 16, 64])]
fn stereo_clips_at_128_frames(bencher: divan::Bencher<'_, '_>, clips: usize) {
    let mut engine = playing(clips, false);
    let mut data = vec![0.0; 256];
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}

#[divan::bench(args = [16, 64, 200])]
fn routed_tracks_at_128_frames(bencher: divan::Bencher<'_, '_>, tracks: usize) {
    let mut engine = playing(tracks, true);
    let mut data = vec![0.0; 256];
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}

#[divan::bench]
fn engine_builtin_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::utility::{self, Utility};
    let values: Vec<(u32, f32)> = utility::TABLE
        .iter()
        .map(|p| {
            (
                p.id,
                match p.id {
                    utility::GAIN => 0.6,
                    utility::PAN => 0.55,
                    utility::WIDTH => 0.75,
                    _ => p.normalised(p.default),
                },
            )
        })
        .collect();
    builtin_64(bencher, values, 0, || Box::new(Utility::new()));
}

#[divan::bench]
fn engine_builtin_delay_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::delay::{self, Delay};
    let values: Vec<(u32, f32)> = delay::TABLE
        .iter()
        .map(|p| {
            (
                p.id,
                match p.id {
                    delay::FEEDBACK => 0.45,
                    delay::NOTE => 4.0 / 6.0,
                    delay::PING => 1.0,
                    _ => p.normalised(p.default),
                },
            )
        })
        .collect();
    builtin_64(bencher, values, 0, || Box::new(Delay::new()));
}

#[divan::bench]
fn engine_builtin_reverb_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::reverb::{self, Reverb};
    let values: Vec<(u32, f32)> = reverb::TABLE
        .iter()
        .map(|p| (p.id, p.normalised(p.default)))
        .collect();
    builtin_64(bencher, values, 0, || Box::new(Reverb::new()));
}

fn builtin_64(
    bencher: divan::Bencher<'_, '_>,
    values: Vec<(u32, f32)>,
    latency: u32,
    make: impl Fn() -> Box<dyn lumen_devices::Effect>,
) {
    use lumen_engine::{Device, Slot};
    const TRACKS: u64 = 64;
    let audio = Arc::new(ClipAudio::Memory {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 480_000], vec![-0.1; 480_000]],
    });
    let (mut handle, player) = clip_player();
    handle
        .set(
            (0..TRACKS)
                .map(|i| Clip {
                    envelope: Default::default(),
                    id: i,
                    start: Ticks::ZERO,
                    offset: 0,
                    length: 480_000,
                    gain: 0.5,
                    track: i,
                    audio: Arc::clone(&audio),
                    shape: Default::default(),
                })
                .collect(),
        )
        .unwrap();
    let (mut routing, source) = mixer(player);
    let mut plan = routed(TRACKS, 0);
    for node in plan.nodes.iter_mut().filter(|n| n.id < TRACKS) {
        node.devices = vec![Device::Builtin {
            device: 1_000 + node.id,
            latency,
            bypass: false,
        }];
        node.values = vec![(1_000 + node.id, values.clone())];
    }
    for id in 0..TRACKS {
        assert!(
            routing
                .attach_builtin(1_000 + id, Slot::new(make()))
                .is_ok()
        );
    }
    assert!(routing.set(&plan).unwrap());
    let (mut transport, mut engine) = engine(Timeline::default(), source, 48_000);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 256];
    for _ in 0..400 {
        engine.process(48_000, &mut data, 2, 128);
    }
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}

#[divan::bench]
fn engine_flare_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::{Preset, Synth, synth};
    use lumen_engine::{
        InstrumentSpec, Layered, MidiClip, MidiNote, MidiTrack, Target, midi_player,
    };
    const TRACKS: u64 = 64;
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    let tracks: Vec<MidiTrack> = (0..TRACKS)
        .map(|track| {
            let notes: Vec<MidiNote> = [48u8, 55, 60, 64]
                .into_iter()
                .enumerate()
                .map(|(k, pitch)| MidiNote {
                    start: Ticks(k as i64 * 10),
                    length: Ticks(1_920_000 - k as i64 * 10),
                    pitch,
                    velocity: 100,
                    channel: None,
                })
                .collect();
            MidiTrack::new(
                track,
                Target::Instrument,
                Some(InstrumentSpec {
                    kind: synth::KIND,
                    preset: Preset::Poly.number(),
                    values: Preset::Poly.values(),
                    instrument: Box::new(Synth::new(Preset::Poly)),
                }),
                vec![MidiClip {
                    id: 1_000 + track,
                    start: Ticks::ZERO,
                    length: Ticks(1_920_000),
                    period: None,
                    notes: notes.into(),
                    controls: Default::default(),
                    seams: Default::default(),
                }],
            )
        })
        .collect();
    midi.set(tracks).ok().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    assert!(routing.set(&routed(TRACKS, 0)).unwrap());
    let (mut transport, mut engine) = engine(Timeline::default(), source, 48_000);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 256];
    for _ in 0..100 {
        engine.process(48_000, &mut data, 2, 128);
    }
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}

#[divan::bench]
fn engine_eq_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::eq::{self, Eq};
    use lumen_engine::{Device, Slot};
    const TRACKS: u64 = 64;
    let audio = Arc::new(ClipAudio::Memory {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 480_000], vec![-0.1; 480_000]],
    });
    let (mut handle, player) = clip_player();
    handle
        .set(
            (0..TRACKS)
                .map(|i| Clip {
                    envelope: Default::default(),
                    id: i,
                    start: Ticks::ZERO,
                    offset: 0,
                    length: 480_000,
                    gain: 0.5,
                    track: i,
                    audio: Arc::clone(&audio),
                    shape: Default::default(),
                })
                .collect(),
        )
        .unwrap();
    let (mut routing, source) = mixer(player);
    let mut plan = routed(TRACKS, 0);
    let band = |number: u32| {
        let mut values = Vec::new();
        let mut set = |id: u32, units: f32| {
            let info = eq::TABLE.iter().find(|info| info.id == id).expect("an id");
            values.push((id, info.normalised(units)));
        };
        set(eq::freq_of(number as usize), 100.0 * number as f32);
        set(eq::on_of(number as usize), 1.0);
        if number == 1 || number == 8 {
            set(eq::gain_of(number as usize), 24.0);
        } else {
            set(eq::gain_of(number as usize), 6.0);
        }
        values
    };
    for node in plan.nodes.iter_mut().filter(|n| n.id < TRACKS) {
        node.devices = vec![Device::Builtin {
            device: 1_000 + node.id,
            latency: 0,
            bypass: false,
        }];
        let mut values = vec![(eq::OUTPUT, 0.5)];
        for number in 1..=8 {
            values.extend(band(number));
        }
        node.values = vec![(1_000 + node.id, values)];
    }
    for id in 0..TRACKS {
        assert!(
            routing
                .attach_builtin(1_000 + id, Slot::new(Box::new(Eq::new())))
                .is_ok()
        );
    }
    assert!(routing.set(&plan).unwrap());
    let (mut transport, mut engine) = engine(Timeline::default(), source, 48_000);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 256];
    for _ in 0..400 {
        engine.process(48_000, &mut data, 2, 128);
    }
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}

#[divan::bench]
fn engine_gravity_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::dynamics::compressor::{self, Compressor};
    let values = vec![
        (compressor::THRESHOLD, 0.55),
        (compressor::RATIO, 0.3),
        (compressor::ATTACK, 0.4),
        (compressor::RELEASE, 0.5),
        (compressor::KNEE, 0.25),
    ];
    builtin_64(bencher, values, 0, || Box::new(Compressor::new()));
}

#[divan::bench]
fn engine_horizon_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::dynamics::limiter::{self, Limiter};
    let look = lumen_devices::params::find(limiter::TABLE, limiter::LOOK)
        .expect("a parameter")
        .normalised(1.5);
    let latency =
        lumen_devices::effect_latency(limiter::NAME, &[(limiter::LOOK, look)]).expect("a device");
    let values = vec![
        (limiter::CEILING, 0.7),
        (limiter::RELEASE, 0.4),
        (limiter::LOOK, look),
        (limiter::DETECT, 1.0),
    ];
    builtin_64(bencher, values, latency, || Box::new(Limiter::new()));
}
