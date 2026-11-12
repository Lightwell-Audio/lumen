//! Built-in effects in the engine (M8 slice B0, ADR-XXXX): a device runs where
//! the plan reaches it, the values the project holds reach it with the block,
//! a lane moves its parameters on the exact frames at any block size, nothing
//! it does allocates, its latency is compensated and its tail rings on after
//! a Stop.

use std::sync::Arc;

use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_devices::utility::{self, GAIN};
use lumen_devices::{Effect, Utility};
use lumen_engine::{
    AutoLane, AutoPoint, Clip, ClipAudio, ClipPlayer, ClipsHandle, Command, Destination, Device,
    Engine, EngineHandle, Mixer, MixerHandle, NodeAutomation, NodeSpec, ParamLane, Routing, Slot,
    Timeline, clip_player, engine, mixer,
};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
/// The device's id in the plan, and in the mixer's built-in pool.
const DEVICE: u64 = 7;
const TRACK: u64 = 1;

/// A clip of `value` on `TRACK`, long enough for any render here.
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

/// The routing: a track with a built-in device on it, into the master.
fn routing(values: &[(u32, f32)], latency: u32, lane: Option<AutoLane>) -> Routing {
    let mut track = NodeSpec::unity(TRACK, Destination::Node(MASTER));
    track.devices = vec![Device::Builtin {
        device: DEVICE,
        latency,
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

/// The normalised value of a gain in decibels.
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
    // Past the glide from the device's default to the project's value.
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
    // Nothing is attached: the node's fallback plays, so the track sounds as
    // if it had no device at all (never silence, never late).
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

/// The golden render: a lane moves the utility's gain, and the same audio comes
/// out at every block size a device is given.
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
            // The same curve, interpolated along stretches of different
            // lengths: the two agree to float rounding, not bit for bit.
            assert!(
                (a - b).abs() < 1e-4,
                "block {block} frame {frame}: {a} against {b}"
            );
        }
    }
    // And the lane is really heard: the gain falls and comes back.
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

/// A lane's own point is played on its own frame: the value at the point's
/// sample is the value the lane gives, not one smoothing step behind it.
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

/// An offline render of the same project plays what playback plays: the same
/// mixer, the same device, the same values and lane, at the same samples.
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
    // Playback.
    let routing = routing(&[(GAIN, gain(0.0))], 0, Some(lane));
    let mut rig = rig(vec![clip(0.25)], &routing);
    rig.attach(Box::new(Utility::new()));
    rig.play();
    let live = rig.run(6000, 128);
    // A render, with its own mixer over the same routing.
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
    // Past the play fade-in (2 ms) and the start of the first glide. The two
    // cut the blocks differently (the render pre-rolls 100 ms, so its blocks
    // are not the engine's), and the same lane curve interpolated along
    // different stretches differs by float rounding: within a thousandth of
    // the level, far under what a 16-bit file holds.
    let mut worst = 0.0f32;
    for (frame, (a, b)) in live.iter().zip(&rendered).enumerate().skip(300) {
        let difference = (a - b).abs();
        let allowed = 1e-3 * b.abs() + 1e-5;
        if difference > allowed {
            panic!("frame {frame}: {a} live against {b} rendered");
        }
        worst = worst.max(difference);
    }
    assert!(worst < 1e-3, "the render differs by {worst}");
}
