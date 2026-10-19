//! Golden renders of automation lanes (M7 slice A0): every automatable value
//! of a node is played where its lane puts it, on the sample, at any block
//! size, with delay compensation at work, and an edited lane glides in
//! instead of clicking.

use std::sync::Arc;

use lumen_core::automation::Shape;
use lumen_core::curve::Curve;
use lumen_core::pan::PanLaw;
use lumen_core::time::Ticks;
use lumen_engine::testing::automated;
use lumen_engine::{
    AutoLane, AutoPoint, Clip, ClipAudio, ClipPlayer, Command, Destination, Device, Engine,
    EngineHandle, Mixer, MixerHandle, NodeAutomation, NodeSpec, Routing, SendSpec, Timeline,
    clip_player, db_gain, engine, mixer,
};

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
const TRACK: u64 = 1;
const RETURN: u64 = 2;
/// The level of the steady audio on both sides.
const LEVEL: f32 = 0.5;

type Player = Engine<Mixer<ClipPlayer>>;

struct Rig {
    _transport: EngineHandle,
    routing: MixerHandle,
    engine: Player,
}

fn steady(level: f32, frames: usize) -> Arc<ClipAudio> {
    Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![vec![level; frames]; 2],
    })
}

fn clip(track: u64, audio: &Arc<ClipAudio>) -> Clip {
    Clip {
        id: track,
        track,
        start: Ticks::ZERO,
        offset: 0,
        length: audio.frames() as i64,
        gain: 1.0,
        audio: Arc::clone(audio),
        shape: Default::default(),
    }
}

fn rig(clips: Vec<Clip>, routing: &Routing) -> Rig {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing_handle, source) = mixer(player);
    assert!(routing_handle.set(routing).unwrap());
    let (mut transport, engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).unwrap();
    Rig {
        _transport: transport,
        routing: routing_handle,
        engine,
    }
}

impl Rig {
    /// Renders `frames` in blocks of `block`; returns the left and right
    /// channels.
    fn run(&mut self, frames: usize, block: usize) -> [Vec<f32>; 2] {
        let mut data = vec![0.0; 2 * block];
        let mut out = [Vec::new(), Vec::new()];
        let mut done = 0;
        while done < frames {
            let n = block.min(frames - done);
            self.engine.process(RATE, &mut data[..2 * n], 2, n);
            out[0].extend_from_slice(&data[..n]);
            out[1].extend_from_slice(&data[n..2 * n]);
            done += n;
        }
        out
    }

    fn reroute(&mut self, routing: &Routing) {
        self.routing.collect_garbage();
        assert!(self.routing.set(routing).unwrap());
    }
}

fn point(at: i64, value: f32, shape: Shape) -> AutoPoint {
    AutoPoint { at, value, shape }
}

fn lane(points: &[(i64, f32, Shape)]) -> Option<AutoLane> {
    AutoLane::new(points.iter().map(|&(a, v, s)| point(a, v, s)).collect())
}

/// A track straight to the master, with `lanes`.
fn routing(lanes: NodeAutomation) -> Routing {
    Routing {
        nodes: vec![
            NodeSpec::unity(MASTER, Destination::Hardware(0)),
            NodeSpec {
                automation: Some(lanes),
                ..NodeSpec::unity(TRACK, Destination::Node(MASTER))
            },
        ],
        cue: None,
        measure: None,
    }
}

fn play(lanes: NodeAutomation, frames: usize) -> [Vec<f32>; 2] {
    let audio = steady(LEVEL, 400_000);
    rig(vec![clip(TRACK, &audio)], &routing(lanes)).run(frames, 128)
}

/// The largest change from one frame to the next.
fn steepest(out: &[f32]) -> f32 {
    out.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn a_fader_lane_plays_its_level_at_every_sample() {
    let fader = lane(&[
        (10_000, 0.0, Shape::Linear),
        (40_000, -20.0, Shape::Curve(Curve::SLOW)),
        (70_000, -6.0, Shape::Linear),
        (90_000, -60.0, Shape::Linear),
    ])
    .unwrap();
    let [left, right] = play(
        NodeAutomation {
            volume: Some(fader.clone()),
            ..Default::default()
        },
        100_000,
    );
    // Past the play fade-in, every frame is the audio at the lane's level.
    for n in 3_000..100_000 {
        let want = LEVEL * db_gain(fader.value_at(n as i64));
        assert!(
            (left[n] - want).abs() < 6e-4,
            "left {n}: {} vs {want}",
            left[n]
        );
        assert_eq!(left[n], right[n]);
    }
}

#[test]
fn a_fader_lane_that_falls_to_the_floor_ends_in_silence() {
    let fader = lane(&[
        (5_000, 0.0, Shape::Linear),
        (30_000, lumen_core::automation::FLOOR_DB, Shape::Linear),
    ]);
    let [left, _] = play(
        NodeAutomation {
            volume: fader,
            ..Default::default()
        },
        40_000,
    );
    assert_eq!(left[30_000], 0.0);
    assert!(left[39_000..].iter().all(|&s| s == 0.0));
    assert!(left[20_000] > 0.0);
}

#[test]
fn a_pan_lane_balances_the_sides_as_the_law_does() {
    let pan = lane(&[(2_000, -1.0, Shape::Linear), (50_000, 1.0, Shape::Linear)]).unwrap();
    let [left, right] = play(
        NodeAutomation {
            pan: Some(pan.clone()),
            ..Default::default()
        },
        60_000,
    );
    for n in 3_000..60_000 {
        let [l, r] = PanLaw::Balance.gains(pan.value_at(n as i64));
        // The balance law bends at the centre, between two knots.
        assert!((left[n] - LEVEL * l).abs() < 0.011, "left {n}");
        assert!((right[n] - LEVEL * r).abs() < 0.011, "right {n}");
    }
    assert!((left[3_000] - LEVEL).abs() < 1e-3 && right[3_000] < 0.03);
    assert!(left[59_000].abs() < 1e-3 && (right[59_000] - LEVEL).abs() < 1e-3);
}

#[test]
fn a_trim_lane_scales_the_input() {
    let trim = lane(&[(0, -6.0, Shape::Step), (20_000, 6.0, Shape::Step)]);
    let [left, _] = play(
        NodeAutomation {
            trim,
            ..Default::default()
        },
        30_000,
    );
    assert!((left[10_000] - LEVEL * db_gain(-6.0)).abs() < 1e-5);
    assert!((left[28_000] - LEVEL * db_gain(6.0)).abs() < 1e-5);
}

#[test]
fn a_mute_lane_steps_on_its_sample_and_ramps_over_a_millisecond() {
    let mute = lane(&[
        (10_000, 0.0, Shape::Step),
        (20_000, 1.0, Shape::Step),
        (30_000, 0.0, Shape::Step),
    ]);
    let [left, _] = play(
        NodeAutomation {
            mute,
            ..Default::default()
        },
        40_000,
    );
    assert_eq!(left[19_999], LEVEL, "heard to the sample before");
    assert_eq!(left[20_000], LEVEL, "the ramp starts at the point");
    assert!(left[20_024] < LEVEL && left[20_024] > 0.0, "half way down");
    assert_eq!(left[20_048], 0.0);
    assert_eq!(left[29_999], 0.0);
    assert!(left[30_024] > 0.0 && left[30_024] < LEVEL);
    assert_eq!(left[30_048], LEVEL);
    // The ramp is what stops the click: no step bigger than one frame of it.
    assert!(steepest(&left[3_000..]) < LEVEL / 40.0);
}

#[test]
fn a_send_lane_scales_what_reaches_the_return() {
    let send = lane(&[(0, -12.0, Shape::Step), (30_000, 0.0, Shape::Step)]);
    let routing = Routing {
        nodes: vec![
            NodeSpec::unity(MASTER, Destination::Hardware(0)),
            NodeSpec::unity(RETURN, Destination::Node(MASTER)),
            NodeSpec {
                // Heard through the return only.
                audible: false,
                sends: vec![SendSpec {
                    to: RETURN,
                    gain: 0.0,
                    pre_fader: false,
                }],
                automation: Some(NodeAutomation {
                    sends: vec![send],
                    ..Default::default()
                }),
                ..NodeSpec::unity(TRACK, Destination::Node(MASTER))
            },
        ],
        cue: None,
        measure: None,
    };
    let audio = steady(LEVEL, 100_000);
    let out = rig(vec![clip(TRACK, &audio)], &routing).run(40_000, 128);
    assert!((out[0][10_000] - LEVEL * db_gain(-12.0)).abs() < 1e-5);
    assert!((out[0][38_000] - LEVEL).abs() < 1e-5);
}

#[test]
fn the_same_frames_at_any_block_size() {
    // Audio whose every sample differs, through the production-sized
    // automated routing: groups, returns and sends.
    let ramp = Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![
            (0..200_000)
                .map(|i| ((i % 997) as f32 - 498.0) / 1e3)
                .collect(),
            (0..200_000)
                .map(|i| ((i % 601) as f32 - 300.0) / 1e3)
                .collect(),
        ],
    });
    let routing = automated(16, 0);
    let clips: Vec<Clip> = (0..16).map(|t| clip(t, &ramp)).collect();
    let whole = rig(clips.clone(), &routing).run(30_000, 256);
    for block in [1, 37, 64, 128, 1000] {
        let parts = rig(clips.clone(), &routing).run(30_000, block);
        for side in 0..2 {
            let worst = whole[side]
                .iter()
                .zip(&parts[side])
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(worst < 1e-5, "block of {block}: differs by {worst}");
        }
    }
}

#[test]
fn a_latent_device_does_not_move_a_lane_in_time() {
    let fader = lane(&[
        (20_000, 0.0, Shape::Step),
        (30_000, -18.0, Shape::Step),
        (40_000, -6.0, Shape::Linear),
        (60_000, 0.0, Shape::Linear),
    ]);
    let mute = lane(&[(25_000, 0.0, Shape::Step), (35_000, 1.0, Shape::Step)]);
    let lanes = NodeAutomation {
        volume: fader,
        mute,
        ..Default::default()
    };
    let plain = play(lanes.clone(), 70_000);
    let mut latent = routing(lanes);
    latent.nodes[1].devices.push(Device::Delay {
        frames: 240,
        listen: false,
    });
    let audio = steady(LEVEL, 400_000);
    let delayed = rig(vec![clip(TRACK, &audio)], &latent).run(70_000, 128);
    // The device delays the node's sound by its 240 frames, and the lane
    // acts on the audio of its own moment: the two renders are the same sound
    // 240 frames apart, every step and ramp on the same sample of it.
    let worst = (4_000..69_000)
        .map(|n| (plain[0][n] - delayed[0][n + 240]).abs())
        .fold(0.0, f32::max);
    assert!(worst < 1e-5, "differs by {worst}");
    assert_eq!(delayed[0][30_239], LEVEL, "the step has not begun");
    assert!(
        (delayed[0][30_240 + 60] - LEVEL * db_gain(-18.0)).abs() < 1e-5,
        "and has finished 60 frames on"
    );
    assert_eq!(delayed[0][35_000 + 240 + 100], 0.0, "muted");
}

#[test]
fn a_lane_edited_while_playing_glides_instead_of_clicking() {
    let steady_lane = |db: f32| NodeAutomation {
        volume: lane(&[(0, db, Shape::Step)]),
        ..Default::default()
    };
    let audio = steady(LEVEL, 400_000);
    let mut rig = rig(vec![clip(TRACK, &audio)], &routing(steady_lane(0.0)));
    let mut out = rig.run(8_000, 128)[0].clone();
    rig.reroute(&routing(steady_lane(-20.0)));
    out.extend(rig.run(8_000, 128)[0].iter());
    assert!((out[7_000] - LEVEL).abs() < 1e-6);
    assert!(
        (out[15_900] - LEVEL * db_gain(-20.0)).abs() < 1e-5,
        "ends on the edit"
    );
    // 20 ms is 960 frames: the fall is gentle, never a step.
    assert!(
        steepest(&out[3_000..]) < 0.001,
        "{}",
        steepest(&out[3_000..])
    );
}

#[test]
fn switching_a_lane_on_crossfades_the_plan() {
    let audio = steady(LEVEL, 400_000);
    let plain = Routing {
        nodes: vec![
            NodeSpec::unity(MASTER, Destination::Hardware(0)),
            NodeSpec::unity(TRACK, Destination::Node(MASTER)),
        ],
        cue: None,
        measure: None,
    };
    let mut rig = rig(vec![clip(TRACK, &audio)], &plain);
    let mut out = rig.run(8_000, 128)[0].clone();
    rig.reroute(&routing(NodeAutomation {
        volume: lane(&[(0, -20.0, Shape::Step)]),
        ..Default::default()
    }));
    out.extend(rig.run(8_000, 128)[0].iter());
    assert!((out[15_900] - LEVEL * db_gain(-20.0)).abs() < 1e-5);
    // A 5 ms crossfade of 240 frames across a 0.45 fall.
    assert!(
        steepest(&out[3_000..]) < 0.003,
        "{}",
        steepest(&out[3_000..])
    );
}

#[test]
fn lanes_with_nothing_in_them_leave_the_node_as_it_was() {
    let a = play(NodeAutomation::default(), 10_000);
    let audio = steady(LEVEL, 400_000);
    let plain = Routing {
        nodes: vec![
            NodeSpec::unity(MASTER, Destination::Hardware(0)),
            NodeSpec::unity(TRACK, Destination::Node(MASTER)),
        ],
        cue: None,
        measure: None,
    };
    let b = rig(vec![clip(TRACK, &audio)], &plain).run(10_000, 128);
    assert_eq!(a, b);
}
