//! Cost of playing audio clips: 64 stereo clips all sounding at once, the
//! M2 slice 6 target ("well inside the CPU budget" at 128 frames); and the
//! same through a production-sized routing (M4 slice 1): a track each, in
//! groups, with sends to two returns. The quality bar: under half of the
//! 2.67 ms a 128-frame block lasts at 48 kHz.

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

/// `clips` clips playing; through the routed mix, each on its own track,
/// or else all on one track straight to the device.
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
    // Past the fade-in, into steady playback.
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

/// 64 tracks, each with Prism, the utility device, on it (M8 slice B0): what a
/// built-in effect costs the audio thread, beside `routed_tracks_at_128_frames`
/// without one.
#[divan::bench]
fn engine_builtin_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::utility::{self, Utility};
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
            latency: 0,
            bypass: false,
        }];
        // A gain a little under unity, and a pan off centre: the device works.
        node.values = vec![(
            1_000 + node.id,
            vec![
                (utility::GAIN, 0.6),
                (utility::PAN, 0.55),
                (utility::WIDTH, 0.75),
            ],
        )];
    }
    for id in 0..TRACKS {
        assert!(
            routing
                .attach_builtin(1_000 + id, Slot::new(Box::new(Utility::new())))
                .is_ok()
        );
    }
    assert!(routing.set(&plan).unwrap());
    let (mut transport, mut engine) = engine(Timeline::default(), source, 48_000);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 256];
    // Past the fade-in and the 20 ms glide to the values.
    for _ in 0..400 {
        engine.process(48_000, &mut data, 2, 128);
    }
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}

/// 64 tracks, each with a built-in device on it (M8 slices B0 and B2): `make`
/// builds the device, `values` its parameters and `latency` the frames it
/// holds back.
fn builtin_64(
    bencher: divan::Bencher<'_, '_>,
    make: &dyn Fn() -> Box<dyn lumen_devices::Effect>,
    values: &dyn Fn(u64) -> Vec<(u32, f32)>,
    latency: u32,
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
        node.values = vec![(1_000 + node.id, values(node.id))];
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
    // Past the fade-in and the 20 ms glide to the values.
    for _ in 0..400 {
        engine.process(48_000, &mut data, 2, 128);
    }
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}

/// 64 tracks, each with Gravity, the compressor, on it (M8 slice B2), keyed
/// from nothing: what a compressor costs the audio bus per block, beside
/// `engine_builtin_64` for the utility.
#[divan::bench]
fn engine_gravity_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::dynamics::compressor::{self, Compressor};
    builtin_64(
        bencher,
        &|| Box::new(Compressor::new()),
        &|_| {
            vec![
                (compressor::THRESHOLD, 0.55),
                (compressor::RATIO, 0.3),
                (compressor::ATTACK, 0.4),
                (compressor::RELEASE, 0.5),
                (compressor::KNEE, 0.25),
            ]
        },
        0,
    );
}

/// 64 tracks, each with Horizon, the limiter, in true-peak mode (M8 slice B2):
/// its 1.5 ms lookahead, which the plan compensates.
#[divan::bench]
fn engine_horizon_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::dynamics::limiter::{self, Limiter};
    let look = lumen_devices::params::find(limiter::TABLE, limiter::LOOK)
        .expect("a parameter")
        .normalised(1.5);
    let latency =
        lumen_devices::effect_latency(limiter::NAME, &[(limiter::LOOK, look)]).expect("a device");
    builtin_64(
        bencher,
        &|| Box::new(Limiter::new()),
        &move |_| {
            vec![
                (limiter::CEILING, 0.7),
                (limiter::RELEASE, 0.4),
                (limiter::LOOK, look),
                (limiter::DETECT, 1.0),
            ]
        },
        latency,
    );
}
