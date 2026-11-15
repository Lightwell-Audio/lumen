use std::sync::Arc;

use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_devices::utility::{self, GAIN};
use lumen_devices::{Compressor, Effect, Key, Limiter, Stereo, Utility};
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

fn clip(value: f32) -> Clip {
    Clip {
        envelope: Default::default(),
        id: 1,
        track: TRACK,
        start: Ticks::ZERO,
        offset: 0,
        length: 480_000,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![vec![value; 480_000], vec![value; 480_000]],
        }),
        shape: Default::default(),
    }
}

fn routing(values: &[(u32, f32)], latency: u32, lane: Option<AutoLane>) -> Routing {
    let mut track = NodeSpec::unity(TRACK, Destination::Node(MASTER));
    track.devices = vec![Device::Builtin {
        device: DEVICE,
        latency,
        bypass: false,
    }];
    track.values = vec![(DEVICE, values.to_vec())];
    track.automation = lane.map(|lane| NodeAutomation {
        plugins: vec![(DEVICE, ParamLane { param: GAIN, lane })],
        ..Default::default()
    });
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

fn gain(db: f32) -> f32 {
    lumen_devices::params::find(utility::TABLE, GAIN)
        .unwrap()
        .normalised(db)
}

#[test]
fn a_built_in_plays_the_values_the_project_holds() {
    let routing = routing(&[(GAIN, gain(-6.0))], 0, None);
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.attach(Box::new(Utility::new()));
    rig.play();
    rig.run(4096, 128);
    let heard = rig.run(512, 128);
    let want = 0.25 * 10f32.powf(-6.0 / 20.0);
    assert!(
        (heard[500] - want).abs() < 1e-3,
        "{} against {want}",
        heard[500]
    );
}

#[test]
fn an_unattached_built_in_plays_the_audio_through() {
    let routing = routing(&[(GAIN, gain(-60.0))], 0, None);
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.play();
    let heard = rig.run(2048, 128);
    assert!((heard[2000] - 0.25).abs() < 1e-3, "{}", heard[2000]);
}

#[test]
fn a_built_ins_latency_is_compensated() {
    use lumen_engine::compile_routing;
    let routing = routing(&[], 240, None);
    let plan = compile_routing(&routing).unwrap();
    assert_eq!(plan.compensation().frames, 240);
    assert_eq!(plan.compensation().path, [(TRACK, 240)]);
}

#[test]
fn automating_the_utility_s_gain_is_the_same_at_every_block_size() {
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: gain(0.0),
            shape: Shape::Linear,
        },
        AutoPoint {
            at: 2000,
            value: gain(-12.0),
            shape: Shape::Curve(lumen_core::curve::Curve::FAST),
        },
        AutoPoint {
            at: 3000,
            value: gain(-40.0),
            shape: Shape::Step,
        },
        AutoPoint {
            at: 3500,
            value: gain(6.0),
            shape: Shape::Linear,
        },
    ])
    .unwrap();
    let render = |block: usize| {
        let routing = routing(&[(GAIN, gain(0.0))], 0, Some(lane.clone()));
        let mut rig = rig(vec![clip(0.25)], &routing);
        rig.attach(Box::new(Utility::new()));
        rig.play();
        rig.run(5000, block)
    };
    let whole = render(64);
    for block in [128usize, 512, 1024, 97] {
        let parts = render(block);
        assert_eq!(parts.len(), whole.len());
        for (frame, (a, b)) in whole.iter().zip(&parts).enumerate() {
            assert!(
                (a - b).abs() < 1e-4,
                "block {block} frame {frame}: {a} against {b}"
            );
        }
    }
    let quiet = whole[2900];
    assert!(
        (quiet - 0.25 * 10f32.powf(-40.0 / 20.0)).abs() < 2e-4,
        "{quiet}"
    );
    assert!(whole[4999] > whole[2900] * 10.0);
}

#[test]
fn a_block_with_automation_never_touches_the_heap() {
    assert!(guard_is_active(), "the guarded allocator is installed");
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: 0.1,
            shape: Shape::Linear,
        },
        AutoPoint {
            at: 4000,
            value: 0.9,
            shape: Shape::Curve(lumen_core::curve::Curve::S_CURVE),
        },
        AutoPoint {
            at: 8000,
            value: 0.2,
            shape: Shape::Step,
        },
    ])
    .unwrap();
    let routing = routing(&[(GAIN, 0.5)], 0, Some(lane));
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.attach(Box::new(Utility::new()));
    rig.play();
    let mut data = vec![0.0; 2 * 128];
    let ((), heap) = count_rt_violations(|| {
        rig.engine.process(RATE, &mut data, 2, 128);
    });
    assert_eq!(heap, 0, "the audio thread allocated");
}

#[test]
fn a_lane_point_is_heard_on_its_own_sample() {
    let points = [(0, gain(0.0)), (1000, gain(-20.0))];
    let lane = Lane::new(
        points
            .iter()
            .map(|&(q, v)| Point::new(Ticks(q), v).with_shape(Shape::Linear))
            .collect(),
    )
    .unwrap();
    let baked = lumen_engine::AutoLane::new(
        points
            .iter()
            .map(|&(at, value)| AutoPoint {
                at,
                value,
                shape: Shape::Linear,
            })
            .collect(),
    )
    .unwrap();
    assert!(lane.points().len() == 2);
    let routing = routing(&[(GAIN, gain(0.0))], 0, Some(baked));
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.attach(Box::new(Utility::new()));
    rig.play();
    let heard = rig.run(1024, 64);
    let want = 0.25 * 10f32.powf(-20.0 / 20.0);
    assert!(
        (heard[1023] - want).abs() < 1e-3,
        "{} against {want}",
        heard[1023]
    );
}

#[test]
fn an_offline_render_plays_what_playback_plays() {
    use lumen_engine::{Ended, SourceRender, render_source};
    use std::sync::atomic::AtomicBool;
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: gain(0.0),
            shape: Shape::Linear,
        },
        AutoPoint {
            at: 3000,
            value: gain(-18.0),
            shape: Shape::Curve(lumen_core::curve::Curve::S_CURVE),
        },
        AutoPoint {
            at: 4000,
            value: gain(-3.0),
            shape: Shape::Linear,
        },
    ])
    .unwrap();
    let routing = routing(&[(GAIN, gain(0.0))], 0, Some(lane));
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.attach(Box::new(Utility::new()));
    rig.play();
    let live = rig.run(6000, 128);
    let (mut clips, player) = clip_player();
    clips.set(vec![clip(0.25)]).unwrap();
    let (mut handle, mut source) = mixer(player);
    assert!(handle.set(&routing).unwrap());
    handle
        .attach_builtin(DEVICE, Slot::new(Box::new(Utility::new())))
        .unwrap_or_else(|_| panic!("room in the pool"));
    let timeline = Timeline::default();
    let cancel = AtomicBool::new(false);
    let mut rendered: Vec<f32> = Vec::new();
    let ended = render_source(
        &mut source,
        SourceRender {
            timeline: &timeline,
            sample_rate: RATE,
            from: 0,
            frames: 6000,
            most_tail: 0,
            block: 128,
            wait: std::time::Duration::from_secs(1),
        },
        &cancel,
        &mut || false,
        &mut |_| {},
        &mut |left, _right| rendered.extend_from_slice(left),
    );
    assert_eq!(ended, Ended::Done);
    assert_eq!(rendered.len(), 6000);
    let mut worst = 0.0f32;
    for (frame, (a, b)) in live.iter().zip(&rendered).enumerate().skip(300) {
        let difference = (a - b).abs();
        if a.to_bits() != b.to_bits() {
            panic!("frame {frame}: {a} live against {b} rendered");
        }
        worst = worst.max(difference);
    }
    assert_eq!(worst, 0.0, "the render differs by {worst}");
}

struct Timed {
    rings: [Vec<f32>; 2],
    at: usize,
    value: f32,
}

fn timed_latency(values: &[(u32, f32)]) -> u32 {
    let up = lumen_devices::params::normalised_of(utility::TABLE, GAIN, values);
    240 + (up * 240.0) as u32
}

impl Timed {
    fn new() -> Self {
        Self {
            rings: [vec![0.0; 1024], vec![0.0; 1024]],
            at: 0,
            value: 0.0,
        }
    }
}

fn timed_value(block: &lumen_devices::Block<'_>, value: &mut f32) {
    for i in 0..block.events.len() {
        let (id, points) = block.events.queue(i);
        if id == GAIN
            && let Some(point) = points.last()
        {
            *value = point.1 as f32;
        }
    }
}

impl Effect for Timed {
    fn process(&mut self, block: &lumen_devices::Block<'_>, io: Stereo<'_>, _: Option<Key<'_>>) {
        timed_value(block, &mut self.value);
        let delay = timed_latency(&[(GAIN, self.value)]) as usize;
        let n = io.left.len();
        for (ring, samples) in self.rings.iter_mut().zip([io.left, io.right]) {
            for (k, sample) in samples.iter_mut().enumerate() {
                let at = (self.at + k) % ring.len();
                ring[at] = *sample;
                *sample = ring[(at + ring.len() - delay) % ring.len()];
            }
        }
        self.at = (self.at + n) % self.rings[0].len();
    }
}

fn impulse_clip(track: u64, at: usize, value: f32) -> Clip {
    let mut left = vec![0.0; 480_000];
    left[at] = value;
    Clip {
        envelope: Default::default(),
        id: track,
        track,
        start: Ticks::ZERO,
        offset: 0,
        length: 480_000,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![left.clone(), left],
        }),
        shape: Default::default(),
    }
}

#[test]
fn a_latency_that_follows_a_parameter_keeps_a_parallel_path_in_line() {
    const DRY: u64 = 2;
    const AT: usize = 5_000;
    let routing = |latency: u32, gain: f32| Routing {
        nodes: vec![
            NodeSpec::unity(MASTER, Destination::Hardware(0)),
            {
                let mut wet = NodeSpec::unity(TRACK, Destination::Node(MASTER));
                wet.devices = vec![Device::Builtin {
                    device: DEVICE,
                    latency,
                    bypass: false,
                }];
                wet.values = vec![(DEVICE, vec![(GAIN, gain)])];
                wet
            },
            NodeSpec::unity(DRY, Destination::Node(MASTER)),
        ],
        cue: None,
        measure: None,
    };
    let render = |latency: u32, gain: f32| {
        let (mut clips, player) = clip_player();
        clips
            .set(vec![
                impulse_clip(TRACK, AT, 0.5),
                impulse_clip(DRY, AT, 0.5),
            ])
            .unwrap();
        let (mut handle, source) = mixer(player);
        assert!(handle.set(&routing(latency, gain)).unwrap());
        handle
            .attach_builtin(DEVICE, Slot::new(Box::new(Timed::new())))
            .unwrap_or_else(|_| panic!("room in the pool"));
        let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
        transport.send(Command::Play).unwrap();
        let mut data = vec![0.0; 2 * 128];
        let mut left = Vec::new();
        for _ in 0..(AT + 2_000) / 128 {
            engine.process(RATE, &mut data, 2, 128);
            left.extend_from_slice(&data[..128]);
        }
        left
    };
    for (latency, gain) in [(240u32, 0.0f32), (480, 1.0)] {
        let loud = render(latency, gain);
        let placed: Vec<(usize, f32)> = loud
            .iter()
            .enumerate()
            .skip(4_096)
            .filter(|(_, s)| s.abs() > 1e-6)
            .map(|(k, s)| (k, *s))
            .collect();
        assert_eq!(placed.len(), 1, "latency {latency}: {placed:?}");
        let (at, value) = placed[0];
        assert_eq!(
            at,
            AT + latency as usize,
            "latency {latency}: the impulse is at {at}"
        );
        assert!((value - 1.0).abs() < 1e-6, "both paths: {value}");
    }
}

struct Counting(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Effect for Counting {
    fn process(&mut self, _: &lumen_devices::Block<'_>, io: Stereo<'_>, _: Option<Key<'_>>) {
        io.left.fill(0.0);
        io.right.fill(0.0);
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn the_power_button_glides_a_built_in_to_its_dry_signal() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let asked = Arc::new(AtomicUsize::new(0));
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(vec![clip(0.25)]).unwrap();
    let (mut routing_handle, source) = mixer(player);
    let mut on = routing(&[(GAIN, gain(-6.0))], 0, None);
    assert!(routing_handle.set(&on).unwrap());
    routing_handle
        .attach_builtin(DEVICE, Slot::new(Box::new(Counting(Arc::clone(&asked)))))
        .unwrap_or_else(|_| panic!("room in the pool"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    let render = |engine: &mut Engine<Mixer<ClipPlayer>>, blocks: usize| {
        let mut data = vec![0.0; 2 * 128];
        let mut out = Vec::new();
        for _ in 0..blocks {
            engine.process(RATE, &mut data, 2, 128);
            out.extend_from_slice(&data[..128]);
        }
        out
    };
    transport.send(Command::Play).unwrap();
    let heard = render(&mut engine, 4);
    assert_eq!(asked.load(Ordering::Relaxed), 4, "asked while on");
    assert!(heard.iter().all(|s| s.abs() < 1e-6), "it silences while on");
    on.nodes[1].devices[0] = Device::Builtin {
        device: DEVICE,
        latency: 0,
        bypass: true,
    };
    assert!(routing_handle.set(&on).unwrap());
    let before = asked.load(Ordering::Relaxed);
    let heard = render(&mut engine, 20);
    let fading = asked.load(Ordering::Relaxed) - before;
    assert!(fading > 0 && fading < 20, "asked while fading: {fading}");
    let worst = heard
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 0.01, "the bypass stepped by {worst}");
    assert!(
        (heard[heard.len() - 1] - 0.25).abs() < 1e-3,
        "dry again: {}",
        heard[heard.len() - 1]
    );
    let before = asked.load(Ordering::Relaxed);
    render(&mut engine, 40);
    assert_eq!(asked.load(Ordering::Relaxed), before, "not asked when off");
    on.nodes[1].devices[0] = Device::Builtin {
        device: DEVICE,
        latency: 0,
        bypass: false,
    };
    assert!(routing_handle.set(&on).unwrap());
    render(&mut engine, 20);
    assert!(
        asked.load(Ordering::Relaxed) > before,
        "asked again when on"
    );
}

#[test]
fn a_plan_swap_with_a_device_to_attach_and_detach_never_touches_the_heap() {
    assert!(guard_is_active(), "the guarded allocator is installed");
    let before = routing(&[(GAIN, gain(0.0))], 0, None);
    let mut after = routing(&[(GAIN, gain(-6.0))], 0, None);
    after.nodes[1].devices[0] = Device::Builtin {
        device: DEVICE + 1,
        latency: 0,
        bypass: false,
    };
    let mut rig = rig(vec![clip(0.25)], &before);
    rig.attach(Box::new(Utility::new()));
    rig.play();
    rig.run(1024, 128);
    assert!(rig.routing.set(&after).unwrap());
    rig.routing
        .attach_builtin(DEVICE + 1, Slot::new(Box::new(Utility::new())))
        .unwrap_or_else(|_| panic!("room in the pool"));
    rig.routing.detach_builtin(DEVICE);
    let mut data = vec![0.0; 2 * 128];
    let ((), heap) = count_rt_violations(|| {
        rig.engine.process(RATE, &mut data, 2, 128);
    });
    assert_eq!(heap, 0, "the audio thread allocated");
}

#[test]
fn a_still_built_in_is_told_nothing_after_the_first_block() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Watching {
        calls: Arc<AtomicUsize>,
        first: Arc<AtomicUsize>,
        later: Arc<AtomicUsize>,
    }

    impl Effect for Watching {
        fn process(&mut self, block: &lumen_devices::Block<'_>, _: Stereo<'_>, _: Option<Key<'_>>) {
            let n = block.events.len();
            if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
                self.first.store(n, Ordering::Relaxed);
            } else {
                self.later.fetch_max(n, Ordering::Relaxed);
            }
        }
    }

    let (calls, first, later) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let routing = routing(&[(GAIN, gain(-6.0))], 0, None);
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.attach(Box::new(Watching {
        calls: Arc::clone(&calls),
        first: Arc::clone(&first),
        later: Arc::clone(&later),
    }));
    rig.play();
    rig.run(1024, 128);
    assert!(calls.load(Ordering::Relaxed) >= 8, "it ran");
    assert!(
        first.load(Ordering::Relaxed) >= 1,
        "the first block carries the value the project holds"
    );
    assert_eq!(
        later.load(Ordering::Relaxed),
        0,
        "nothing is told again while the values are still"
    );
}

fn burst_clip(track: u64, from: usize, value: f32, frames: usize) -> Clip {
    let mut left = vec![0.0; frames];
    left[from..].fill(value);
    Clip {
        envelope: Default::default(),
        id: track,
        track,
        start: Ticks::ZERO,
        offset: 0,
        length: frames as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![left.clone(), left],
        }),
        shape: Default::default(),
    }
}

#[test]
fn a_key_from_another_track_ducks_on_the_sample_it_arrives() {
    use lumen_devices::dynamics::compressor as gravity;
    use lumen_engine::{SidechainSpec, Tap, compile_routing};
    const KEY: u64 = 2;
    const FROM: usize = 5_000;
    const HELD: u32 = 240;
    let table = gravity::TABLE;
    let normalised = |id: u32, value: f32| {
        lumen_devices::params::find(table, id)
            .expect("a parameter")
            .normalised(value)
    };
    let mut key = NodeSpec::unity(KEY, Destination::Node(MASTER));
    key.audible = false;
    key.devices = vec![Device::Delay {
        frames: HELD,
        listen: false,
    }];
    let mut wet = NodeSpec::unity(TRACK, Destination::Node(MASTER));
    wet.devices = vec![Device::Builtin {
        device: DEVICE,
        latency: 0,
        bypass: false,
    }];
    wet.values = vec![(
        DEVICE,
        vec![
            (gravity::THRESHOLD, normalised(gravity::THRESHOLD, -40.0)),
            (gravity::RATIO, normalised(gravity::RATIO, 20.0)),
            (gravity::KNEE, normalised(gravity::KNEE, 0.0)),
            (gravity::ATTACK, normalised(gravity::ATTACK, 0.05)),
            (gravity::RELEASE, normalised(gravity::RELEASE, 2_000.0)),
            (gravity::MAKEUP, 0.0),
        ],
    )];
    wet.sidechains = vec![SidechainSpec {
        from: KEY,
        tap: Tap::PostFx,
    }];
    let routing = Routing {
        nodes: vec![NodeSpec::unity(MASTER, Destination::Hardware(0)), key, wet],
        cue: None,
        measure: None,
    };
    let plan = compile_routing(&routing).expect("a valid routing");
    assert_eq!(plan.compensation().frames, HELD);
    assert_eq!(plan.compensation().path, [(KEY, HELD)]);

    let frames = FROM + 4_000;
    let (mut clips, player) = clip_player();
    clips
        .set(vec![
            burst_clip(KEY, FROM, 1.0, frames),
            burst_clip(TRACK, FROM, 0.5, frames),
        ])
        .unwrap();
    let (mut handle, source) = mixer(player);
    assert!(handle.set(&routing).unwrap());
    handle
        .attach_builtin(DEVICE, Slot::new(Box::new(Compressor::new())))
        .unwrap_or_else(|_| panic!("room in the pool"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    let mut left = Vec::new();
    let blocks = (frames + 2_000) / 128;
    for _ in 0..blocks {
        engine.process(RATE, &mut data, 2, 128);
        left.extend_from_slice(&data[..128]);
    }
    let heard = |k: usize| left.get(k).copied().unwrap_or(0.0).abs();
    let starts = FROM + HELD as usize;
    assert!(
        heard(starts - 8) < 1e-4,
        "nothing before it: {}",
        heard(starts - 8)
    );
    let first = (starts + 8..starts + 400).fold(0.0f32, |m, k| m.max(heard(k)));
    assert!(
        first < 0.02,
        "the key ducks it from its first sample: {first}"
    );
    let later = (starts + 1_000..starts + 2_000).fold(0.0f32, |m, k| m.max(heard(k)));
    assert!(later < 0.02, "still ducked: {later}");
}

#[test]
fn a_limiter_s_lookahead_keeps_a_parallel_path_in_line() {
    use lumen_devices::dynamics::limiter as horizon;
    use lumen_engine::compile_routing;
    const DRY: u64 = 2;
    const AT: usize = 5_000;
    let values = vec![(
        horizon::LOOK,
        lumen_devices::params::find(horizon::TABLE, horizon::LOOK)
            .expect("a parameter")
            .normalised(1.5),
    )];
    let latency = lumen_devices::effect_latency(horizon::NAME, &values).expect("a device");
    assert_eq!(latency, 72);
    let routing = Routing {
        nodes: vec![
            NodeSpec::unity(MASTER, Destination::Hardware(0)),
            {
                let mut wet = NodeSpec::unity(TRACK, Destination::Node(MASTER));
                wet.devices = vec![Device::Builtin {
                    device: DEVICE,
                    latency,
                    bypass: false,
                }];
                wet.values = vec![(DEVICE, values)];
                wet
            },
            NodeSpec::unity(DRY, Destination::Node(MASTER)),
        ],
        cue: None,
        measure: None,
    };
    assert_eq!(
        compile_routing(&routing).unwrap().compensation().frames,
        latency
    );
    let (mut clips, player) = clip_player();
    clips
        .set(vec![
            impulse_clip(TRACK, AT, 0.25),
            impulse_clip(DRY, AT, 0.25),
        ])
        .unwrap();
    let (mut handle, source) = mixer(player);
    assert!(handle.set(&routing).unwrap());
    handle
        .attach_builtin(DEVICE, Slot::new(Box::new(Limiter::new())))
        .unwrap_or_else(|_| panic!("room in the pool"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    let mut left = Vec::new();
    for _ in 0..(AT + 2_000) / 128 {
        engine.process(RATE, &mut data, 2, 128);
        left.extend_from_slice(&data[..128]);
    }
    let placed: Vec<(usize, f32)> = left
        .iter()
        .enumerate()
        .skip(4_096)
        .filter(|(_, s)| s.abs() > 1e-6)
        .map(|(k, s)| (k, *s))
        .collect();
    assert_eq!(placed.len(), 1, "both paths in one sample: {placed:?}");
    let (at, value) = placed[0];
    assert_eq!(at, AT + latency as usize, "the impulse is at {at}");
    assert!((value - 0.5).abs() < 1e-6, "both paths: {value}");
}

#[test]
fn a_bypassed_limiter_does_not_play_what_it_held() {
    use lumen_devices::dynamics::limiter as horizon;
    const AT: usize = 480;
    const OFF: usize = 2_400;
    const ON: usize = 8_000;
    const BURST_END: usize = 6_000;
    let values = vec![
        (
            horizon::CEILING,
            lumen_devices::params::find(horizon::TABLE, horizon::CEILING)
                .expect("a parameter")
                .normalised(-6.0),
        ),
        (
            horizon::LOOK,
            lumen_devices::params::find(horizon::TABLE, horizon::LOOK)
                .expect("a parameter")
                .normalised(1.5),
        ),
    ];
    let mut burst = vec![0.0f32; 48_000];
    burst[AT..BURST_END].fill(1.5);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        track: TRACK,
        start: Ticks::ZERO,
        offset: 0,
        length: burst.len() as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![burst.clone(), burst],
        }),
        shape: Default::default(),
    };
    let routing = |bypass: bool| {
        let mut track = NodeSpec::unity(TRACK, Destination::Node(MASTER));
        track.devices = vec![Device::Builtin {
            device: DEVICE,
            latency: 72,
            bypass,
        }];
        track.values = vec![(DEVICE, values.clone())];
        Routing {
            nodes: vec![NodeSpec::unity(MASTER, Destination::Hardware(0)), track],
            cue: None,
            measure: None,
        }
    };
    let (mut clips, player) = clip_player();
    clips.set(vec![clip]).unwrap();
    let (mut handle, source) = mixer(player);
    assert!(handle.set(&routing(false)).unwrap());
    handle
        .attach_builtin(DEVICE, Slot::new(Box::new(Limiter::new())))
        .unwrap_or_else(|_| panic!("room in the pool"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    let mut left = Vec::new();
    for block in 0..(ON + 2_000) / 128 {
        if block == OFF / 128 {
            assert!(handle.set(&routing(true)).unwrap());
        }
        if block == ON / 128 {
            assert!(handle.set(&routing(false)).unwrap());
        }
        engine.process(RATE, &mut data, 2, 128);
        left.extend_from_slice(&data[..128]);
    }
    let after = (ON..ON + 1_000).fold(0.0f32, |m, k| m.max(left[k].abs()));
    assert!(
        after < 1e-4,
        "the bypassed limiter played what it held: {after}"
    );
    assert!(
        left[AT..OFF].iter().any(|x| x.abs() > 0.01),
        "the limiter ran before it was bypassed"
    );
}

#[test]
fn a_block_names_a_lane_driven_parameter_once_as_the_lane() {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    type Told = (usize, Vec<(u32, f64)>, Vec<(u32, f64)>);

    struct Queues(Arc<Mutex<Vec<Told>>>);

    impl Effect for Queues {
        fn process(&mut self, block: &lumen_devices::Block<'_>, _: Stereo<'_>, _: Option<Key<'_>>) {
            let mut found = Vec::new();
            for i in 0..block.events.len() {
                let (id, points) = block.events.queue(i);
                if id == GAIN {
                    found.push(points.to_vec());
                }
            }
            if found.is_empty() {
                return;
            }
            let first = found.first().cloned().unwrap_or_default();
            let last = found.last().cloned().unwrap_or_default();
            self.0.lock().unwrap().push((found.len(), first, last));
        }
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: gain(-6.0),
            shape: Shape::Linear,
        },
        AutoPoint {
            at: 400,
            value: gain(-30.0),
            shape: Shape::Linear,
        },
    ])
    .unwrap();
    let routing = routing(&[(GAIN, gain(-12.0))], 0, Some(lane));
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.attach(Box::new(Queues(Arc::clone(&seen))));
    rig.play();
    rig.run(1_024, 128);
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty(), "the device ran");
    calls.store(seen.len(), Ordering::Relaxed);
    for (named, first, _) in seen.iter() {
        assert_eq!(*named, 1, "the lane's queue and no value beside it");
        assert_eq!(first[0].0, 0, "on the block's first frame");
        assert!(
            (first[0].1 - f64::from(gain(-12.0))).abs() > 1e-3,
            "the queue is the lane, not the value the project holds: {:?}",
            first[0]
        );
    }
}

#[test]
fn a_golden_render_of_an_automated_prism_is_pinned_to_its_samples() {
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: gain(0.0),
            shape: Shape::Linear,
        },
        AutoPoint {
            at: 2000,
            value: gain(-12.0),
            shape: Shape::Curve(lumen_core::curve::Curve::FAST),
        },
        AutoPoint {
            at: 3000,
            value: gain(-40.0),
            shape: Shape::Step,
        },
        AutoPoint {
            at: 3500,
            value: gain(6.0),
            shape: Shape::Linear,
        },
    ])
    .unwrap();
    let routing = routing(&[(GAIN, gain(0.0))], 0, Some(lane));
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.attach(Box::new(Utility::new()));
    rig.play();
    let heard = rig.run(5000, 128);
    const PINS: [f32; 20] = [
        2.712_673_8e-5,
        2.103_488_7e-1,
        1.769_865_2e-1,
        1.489_155_4e-1,
        1.252_968_6e-1,
        1.054_241_66e-1,
        8.870_335e-2,
        7.463_457e-2,
        6.279_719e-2,
        1.533_410_5e-2,
        5.601_135_5e-3,
        3.060_502_5e-3,
        2.5e-3,
        2.5e-3,
        3.058_017_7e-3,
        4.988_156e-1,
        4.988_156e-1,
        4.988_156e-1,
        4.988_156e-1,
        4.988_156e-1,
    ];
    let close = |got: f64, want: f64| (got - want).abs() <= 1e-6 * want.abs().max(1e-6);
    assert_eq!(heard.len(), 5000);
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
        close(level, 1.006_878_125_277_191_4e3),
        "the golden level moved: {level:e}"
    );
    assert!(
        close(energy, 4.056_644_738_546_932e2),
        "the golden energy moved: {energy:e}"
    );
}
