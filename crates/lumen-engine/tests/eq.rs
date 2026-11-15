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

fn at(id: u32, value: f32) -> f32 {
    params::find(eq::TABLE, id)
        .expect("a parameter of the table")
        .normalised(value)
}

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

fn tone(hz: f32, frames: usize) -> Clip {
    let samples: Vec<f32> = (0..frames)
        .map(|k| {
            (0.3 * (std::f64::consts::TAU * f64::from(hz) * k as f64 / f64::from(RATE)).sin())
                as f32
        })
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
            assert!(
                (a - b).abs() < 5e-4,
                "block {block} frame {frame}: {a} against {b}"
            );
        }
    }
    let early: f32 = whole[100..400]
        .iter()
        .fold(0.0, |most, x| most.max(x.abs()));
    let late: f32 = whole[4600..4900]
        .iter()
        .fold(0.0, |most, x| most.max(x.abs()));
    assert!(late > early * 1.5, "early {early}, late {late}");
}

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
    const PINS: [f32; 20] = [
        0.0,
        -1.3711277e-1,
        1.9503886e-1,
        -4.1689515e-2,
        -2.0763436e-1,
        3.0325797e-1,
        -1.3342762e-1,
        -1.9594619e-1,
        4.006971e-1,
        -2.7014214e-1,
        -1.1716747e-1,
        4.3509215e-1,
        -4.012807e-1,
        2.3335539e-2,
        3.830033e-1,
        -4.8495448e-1,
        1.9625153e-1,
        2.4981107e-1,
        -4.8815334e-1,
        3.40705e-1,
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
        close(level, 1.2557834914260702e3),
        "the golden level moved: {level:e}"
    );
    assert!(
        close(energy, 4.2242044370250557e2),
        "the golden energy moved: {energy:e}"
    );
}

#[test]
fn a_full_table_of_lanes_fills_one_queue_each() {
    use std::sync::atomic::{AtomicUsize, Ordering};

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

#[test]
fn a_block_with_an_eq_never_touches_the_heap() {
    assert!(guard_is_active(), "the guarded allocator is installed");
    let lanes = vec![sweep(freq_of(3), 200.0, 4_000.0, 4000, Shape::Linear)];
    let routing = routing(&values(&[(on_of(3), 1.0)]), lanes);
    let mut rig = rig(vec![tone(600.0, 48_000)], &routing);
    rig.attach(Box::new(Eq::new()));
    rig.play();
    rig.run(2048, 128);
    let mut data = vec![0.0; 2 * 128];
    let ((), heap) = count_rt_violations(|| {
        rig.engine.process(RATE, &mut data, 2, 128);
    });
    assert_eq!(heap, 0, "the audio thread allocated");
}

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
