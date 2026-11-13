//! Iris, the parametric equaliser, in the engine (M8 slice B1): a lane moves a
//! band's frequency and the sound follows on the exact frames at any block
//! size, the render is pinned by its samples, a full table of lanes fills one
//! queue each, and nothing here allocates.

use std::sync::Arc;

use lumen_core::automation::Shape;
use lumen_core::time::Ticks;
use lumen_devices::eq::{self, freq_of, gain_of, on_of, q_of};
use lumen_devices::params;
use lumen_devices::{Effect, Eq, Key, Stereo};
use lumen_engine::{
    AutoLane, AutoPoint, Clip, ClipAudio, ClipPlayer, ClipsHandle, Command, Destination, Device,
    Engine, EngineHandle, Mixer, MixerHandle, NodeAutomation, NodeSpec, ParamLane, Routing, Slot,
    Timeline, clip_player, engine, mixer,
};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
const DEVICE: u64 = 7;
const TRACK: u64 = 1;

/// The normalised value of `id` at `value` in its own units.
fn at(id: u32, value: f32) -> f32 {
    params::find(eq::TABLE, id)
        .expect("a parameter of the table")
        .normalised(value)
}

/// The values a project holds for Iris with `set`'s own-units values.
fn values(set: &[(u32, f32)]) -> Vec<(u32, f32)> {
    eq::TABLE
        .iter()
        .map(|info| {
            let normalised = set.iter().find(|(id, _)| *id == info.id).map_or_else(
                || info.normalised(info.default),
                |(id, value)| at(*id, *value),
            );
            (info.id, normalised)
        })
        .collect()
}

/// A clip of `frames` frames of a tone at `hz`, on `TRACK`.
fn tone(hz: f32, frames: usize) -> Clip {
    let samples: Vec<f32> = (0..frames)
        .map(|k| 0.3 * (std::f32::consts::TAU * hz * k as f32 / RATE as f32).sin())
        .collect();
    Clip {
        envelope: Default::default(),
        id: 1,
        track: TRACK,
        start: Ticks::ZERO,
        offset: 0,
        length: frames as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![samples.clone(), samples],
        }),
        shape: Default::default(),
    }
}

/// The routing: a track with Iris on it, into the master.
fn routing(values: &[(u32, f32)], lanes: Vec<(u32, AutoLane)>) -> Routing {
    let mut track = NodeSpec::unity(TRACK, Destination::Node(MASTER));
    track.devices = vec![Device::Builtin {
        device: DEVICE,
        latency: 0,
        bypass: false,
    }];
    track.values = vec![(DEVICE, values.to_vec())];
    if !lanes.is_empty() {
        track.automation = Some(NodeAutomation {
            plugins: lanes
                .into_iter()
                .map(|(param, lane)| (DEVICE, ParamLane { param, lane }))
                .collect(),
            ..Default::default()
        });
    }
    Routing {
        nodes: vec![NodeSpec::unity(MASTER, Destination::Hardware(0)), track],
        cue: None,
        measure: None,
    }
}

struct Rig {
    _clips: ClipsHandle,
    transport: EngineHandle,
    routing: MixerHandle,
    engine: Engine<Mixer<ClipPlayer>>,
}

fn rig(clips: Vec<Clip>, routing: &Routing) -> Rig {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing_handle, source) = mixer(player);
    assert!(routing_handle.set(routing).unwrap());
    let (transport, engine) = engine(Timeline::default(), source, RATE);
    Rig {
        _clips: clips_handle,
        transport,
        routing: routing_handle,
        engine,
    }
}

impl Rig {
    fn attach(&mut self, effect: Box<dyn Effect>) {
        self.routing
            .attach_builtin(DEVICE, Slot::new(effect))
            .unwrap_or_else(|_| panic!("room in the pool"));
    }

    fn play(&mut self) {
        self.transport.send(Command::Play).unwrap();
    }

    /// Renders `frames` in blocks of `block`; returns the left channel.
    fn run(&mut self, frames: usize, block: usize) -> Vec<f32> {
        let mut data = vec![0.0; 2 * block];
        let mut out = Vec::with_capacity(frames);
        let mut done = 0;
        while done < frames {
            let n = block.min(frames - done);
            self.engine.process(RATE, &mut data[..2 * n], 2, n);
            out.extend_from_slice(&data[..n]);
            done += n;
        }
        out
    }
}

/// A lane of `id` from `from` to `to` in its own units, over `frames`.
fn sweep(id: u32, from: f32, to: f32, frames: usize, shape: Shape) -> (u32, AutoLane) {
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: at(id, from),
            shape,
        },
        AutoPoint {
            at: frames as i64,
            value: at(id, to),
            shape: Shape::Linear,
        },
    ])
    .unwrap();
    (id, lane)
}

/// The brief's block-size test, through the plan: 64, 128, 512, 1024 and an
/// odd 97 give the same sound.
#[test]
fn a_swept_band_is_the_same_at_every_block_size() {
    let frames = 5000;
    let lanes = vec![
        sweep(freq_of(3), 200.0, 600.0, frames, Shape::Linear),
        sweep(
            gain_of(3),
            -6.0,
            12.0,
            frames,
            Shape::Curve(lumen_core::curve::Curve::S_CURVE),
        ),
        sweep(q_of(3), 0.7, 2.0, frames, Shape::Linear),
    ];
    let values = values(&[
        (on_of(3), 1.0),
        (on_of(5), 1.0),
        (gain_of(5), -6.0),
        (freq_of(5), 6_000.0),
    ]);
    let render = |block: usize| {
        let routing = routing(&values, lanes.clone());
        let mut rig = rig(vec![tone(600.0, 90_000)], &routing);
        rig.attach(Box::new(Eq::new()));
        rig.play();
        rig.run(frames, block)
    };
    let whole = render(64);
    for block in [97usize, 128, 512, 1024] {
        let parts = render(block);
        assert_eq!(parts.len(), whole.len());
        for (frame, (a, b)) in whole.iter().zip(&parts).enumerate() {
            // The same curves, interpolated along stretches of different
            // lengths: the two agree to a ten-thousandth of the signal's
            // loudest. A resonant band magnifies the last bit of a lane's
            // interpolation, and the difference does not grow (the last fifth
            // of the render agrees to four millionths); a band settled on a
            // different frame would move the sound by a tenth, not a
            // ten-thousandth.
            assert!(
                (a - b).abs() < 5e-4,
                "block {block} frame {frame}: {a} against {b}"
            );
        }
    }
    // The lanes are heard: the bell comes up to the 600 Hz tone with 12 dB on
    // it.
    let early: f32 = whole[100..400]
        .iter()
        .fold(0.0, |most, x| most.max(x.abs()));
    let late: f32 = whole[4600..4900]
        .iter()
        .fold(0.0, |most, x| most.max(x.abs()));
    assert!(late > early * 1.5, "early {early}, late {late}");
}

/// The golden render: a band's gain automated, pinned by twenty of its samples,
/// its level and its energy (the platforms' maths libraries round a last bit
/// apart, so a golden is not pinned by its bits).
#[test]
fn a_golden_render_of_an_automated_iris_is_pinned_to_its_samples() {
    let frames = 5000;
    let lanes = vec![
        sweep(freq_of(3), 800.0, 1_200.0, frames, Shape::Linear),
        sweep(
            gain_of(3),
            -6.0,
            9.0,
            frames,
            Shape::Curve(lumen_core::curve::Curve::FAST),
        ),
        sweep(freq_of(1), 120.0, 40.0, frames, Shape::Linear),
    ];
    let values = values(&[(on_of(3), 1.0), (on_of(1), 1.0), (gain_of(1), 24.0)]);
    let routing = routing(&values, lanes);
    let mut rig = rig(vec![tone(700.0, 90_000)], &routing);
    rig.attach(Box::new(Eq::new()));
    rig.play();
    let heard = rig.run(frames, 128);
    assert_eq!(heard.len(), frames);
    // Pinned within a millionth rather than by its bits: the platforms' maths
    // libraries round a last bit apart (CI runs three).
    const PINS: [f32; 20] = [
        0.0,
        -1.371_150_2e-1,
        1.950_389_4e-1,
        -4.168_852_4e-2,
        -2.076_471_3e-1,
        3.032_362_8e-1,
        -1.334_345_6e-1,
        -1.959_39e-1,
        4.006_603_4e-1,
        -2.701_320_4e-1,
        -1.171_502_6e-1,
        4.351_011_5e-1,
        -4.012_750_4e-1,
        2.331_514_7e-2,
        3.830_263_6e-1,
        -4.848_712_7e-1,
        1.962_542e-1,
        2.498_054_7e-1,
        -4.881_357_6e-1,
        3.407_895_6e-1,
    ];

    let close = |got: f64, want: f64| (got - want).abs() <= 1e-6 * want.abs().max(1e-6);
    for (k, want) in PINS.iter().enumerate() {
        let got = heard[k * 250];
        assert!(
            close(f64::from(got), f64::from(*want)),
            "the golden render moved at frame {}: {got:e}, pinned {want:e}",
            k * 250
        );
    }
    let level: f64 = heard.iter().map(|x| f64::from(x.abs())).sum();
    let energy: f64 = heard.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    assert!(
        close(level, 1.255_782_645_500_712_5e3),
        "the golden level moved: {level:e}"
    );
    assert!(
        close(energy, 4.224_196_798_249_165e2),
        "the golden energy moved: {energy:e}"
    );
}

/// B0's review N7: each automated parameter used to take two of a block's
/// queues, its value and its lane. Iris has thirty-eight parameters; a lane on
/// every one of them must fill one queue each, or the last ones would be cut.
#[test]
fn a_full_table_of_lanes_fills_one_queue_each() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counts the queues and the cut ones of each block.
    struct Counting {
        most: Arc<AtomicUsize>,
        cut: Arc<AtomicUsize>,
        params: usize,
    }

    impl Effect for Counting {
        fn process(&mut self, block: &lumen_devices::Block<'_>, _: Stereo<'_>, _: Option<Key<'_>>) {
            self.most.fetch_max(block.events.len(), Ordering::Relaxed);
            self.cut
                .fetch_add(block.events.cut() as usize, Ordering::Relaxed);
            self.params = block.events.len();
        }
    }

    let (most, cut) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let lanes: Vec<(u32, AutoLane)> = eq::TABLE
        .iter()
        .map(|info| {
            let lane = AutoLane::new(vec![
                AutoPoint {
                    at: 0,
                    value: 0.3,
                    shape: Shape::Linear,
                },
                AutoPoint {
                    at: 200,
                    value: 0.7,
                    shape: Shape::Linear,
                },
            ])
            .unwrap();
            (info.id, lane)
        })
        .collect();
    assert_eq!(lanes.len(), eq::TABLE.len());
    let routing = routing(&values(&[]), lanes);
    let mut rig = rig(vec![tone(440.0, 48_000)], &routing);
    rig.attach(Box::new(Counting {
        most: Arc::clone(&most),
        cut: Arc::clone(&cut),
        params: 0,
    }));
    rig.play();
    rig.run(1024, 128);
    assert_eq!(cut.load(Ordering::Relaxed), 0, "no lane was cut");
    assert_eq!(
        most.load(Ordering::Relaxed),
        eq::TABLE.len(),
        "every parameter arrives as one queue"
    );
}

/// The audio thread's rules, through the plan: a block with Iris and a lane on
/// its band allocates nothing.
#[test]
fn a_block_with_an_eq_never_touches_the_heap() {
    assert!(guard_is_active(), "the guarded allocator is installed");
    let lanes = vec![sweep(freq_of(3), 200.0, 4_000.0, 4000, Shape::Linear)];
    let routing = routing(&values(&[(on_of(3), 1.0)]), lanes);
    let mut rig = rig(vec![tone(600.0, 48_000)], &routing);
    rig.attach(Box::new(Eq::new()));
    rig.play();
    // Past the attach's fade, so the plan's own path is the one measured.
    rig.run(2048, 128);
    let mut data = vec![0.0; 2 * 128];
    let ((), heap) = count_rt_violations(|| {
        rig.engine.process(RATE, &mut data, 2, 128);
    });
    assert_eq!(heap, 0, "the audio thread allocated");
}

/// The power button works on Iris as on Prism: bypassed, the tone comes back.
#[test]
fn bypassing_iris_gives_the_tone_back() {
    let values = values(&[
        (on_of(3), 1.0),
        (gain_of(3), 12.0),
        (freq_of(3), 700.0),
        (q_of(3), 1.0),
    ]);
    let heard = |bypass: bool| {
        let mut routing = routing(&values, Vec::new());
        routing.nodes[1].devices[0] = Device::Builtin {
            device: DEVICE,
            latency: 0,
            bypass,
        };
        let mut rig = rig(vec![tone(700.0, 48_000)], &routing);
        rig.attach(Box::new(Eq::new()));
        rig.play();
        rig.run(4096, 128);
        let tail = rig.run(2048, 128);
        tail[1000..]
            .iter()
            .fold(0.0f32, |most, x| most.max(x.abs()))
    };
    let boosted = heard(false);
    let dry = heard(true);
    assert!(boosted > dry * 2.0, "boosted {boosted}, dry {dry}");
    assert!(
        (dry - 0.3).abs() < 0.02,
        "the dry tone is the clip's: {dry}"
    );
}
