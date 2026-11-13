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
    builtin_64(bencher, values, || Box::new(Utility::new()));
}

/// 64 tracks, each with Parallax, the stereo delay, on it (M8 slice B3): what
/// a tempo-synced delay with its loop filters costs the audio thread.
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
    builtin_64(bencher, values, || Box::new(Delay::new()));
}

/// 64 tracks, each with Afterglow, the reverb, on it (M8 slice B3): eight
/// feedback lines, their damping and the early reflections, a block at a time.
#[divan::bench]
fn engine_builtin_reverb_64(bencher: divan::Bencher<'_, '_>) {
    use lumen_devices::reverb::{self, Reverb};
    let values: Vec<(u32, f32)> = reverb::TABLE
        .iter()
        .map(|p| (p.id, p.normalised(p.default)))
        .collect();
    builtin_64(bencher, values, || Box::new(Reverb::new()));
}

/// 64 tracks of steady sound, each with one built-in effect holding `values`:
/// the body the three `engine_builtin_*` benches share (M8 slices B0 and B3).
fn builtin_64(
    bencher: divan::Bencher<'_, '_>,
    values: Vec<(u32, f32)>,
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
            latency: 0,
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
    // Past the fade-in and the 20 ms glide to the values.
    for _ in 0..400 {
        engine.process(48_000, &mut data, 2, 128);
    }
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}
