//! Parallax and Afterglow through the engine (M8 slice B3): a synced echo
//! follows the song's own tempo map to the sample, both devices play the same
//! audio at every block size and in an offline render as in playback, their
//! golden renders are pinned, and their tails ring on after a Stop.

use std::sync::Arc;

use lumen_core::meter::{Meter, MeterMap};
use lumen_core::time::{TempoMap, TempoPoint, Ticks};
use lumen_devices::Effect;
use lumen_devices::delay;
use lumen_devices::params;
use lumen_devices::reverb;
use lumen_engine::{
    AutoLane, AutoPoint, Clip, ClipAudio, ClipsHandle, Command, Destination, Device, Engine,
    EngineHandle, Mixer, NodeAutomation, NodeSpec, ParamLane, Routing, Slot, Source, Timeline,
    clip_player, engine, mixer,
};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
const TRACK: u64 = 1;
const DEVICE: u64 = 7;

/// The normalised value of `id` at `value` in its own units.
fn value(table: &'static [params::ParamInfo], id: u32, value: f32) -> f32 {
    params::find(table, id)
        .expect("a parameter of the table")
        .normalised(value)
}

fn note(index: usize) -> f32 {
    index as f32 / 6.0
}

/// A clip on `track` that is silence with one sample of `value` at each of
/// `at`, long enough for any render here.
fn impulses(track: u64, at: &[(usize, f32)]) -> Clip {
    let mut left = vec![0.0; 480_000];
    for &(frame, value) in at {
        left[frame] = value;
    }
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

/// A clip of a steady tone on `track`.
fn tone(track: u64, level: f32) -> Clip {
    let left = vec![level; 480_000];
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

/// A clip of two tones and a spike: what a golden render is pinned from.
fn rich(track: u64) -> Clip {
    let mut left = vec![0.0; 480_000];
    for (k, sample) in left.iter_mut().enumerate() {
        let t = k as f32 / RATE as f32;
        *sample = (std::f32::consts::TAU * 220.0 * t).sin() * 0.2
            + (std::f32::consts::TAU * 1_234.0 * t).sin() * 0.08;
    }
    left[1_000] += 0.5;
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

/// The routing: a track with one built-in device on it, into the master.
fn routing(values: Vec<(u32, f32)>, lane: Option<ParamLane>) -> Routing {
    let mut track = NodeSpec::unity(TRACK, Destination::Node(MASTER));
    track.devices = vec![Device::Builtin {
        device: DEVICE,
        latency: 0,
        bypass: false,
    }];
    track.values = vec![(DEVICE, values)];
    track.automation = lane.map(|lane| NodeAutomation {
        plugins: vec![(DEVICE, lane)],
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
    engine: Engine<Mixer<lumen_engine::ClipPlayer>>,
}

fn rig(timeline: &Timeline, clip: Clip, routing: &Routing, effect: Box<dyn Effect>) -> Rig {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(vec![clip]).unwrap();
    let (mut routing_handle, source) = mixer(player);
    assert!(routing_handle.set(routing).unwrap());
    routing_handle
        .attach_builtin(DEVICE, Slot::new(effect))
        .unwrap_or_else(|_| panic!("room in the pool"));
    let (transport, engine) = engine(timeline.clone(), source, RATE);
    Rig {
        _clips: clips_handle,
        transport,
        engine,
    }
}

impl Rig {
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

/// The delay's values for a quarter-note echo at full wet, no feedback, with
/// `extra` on top.
fn delay_values(extra: &[(u32, f32)]) -> Vec<(u32, f32)> {
    delay::TABLE
        .iter()
        .map(|info| {
            let set = match info.id {
                delay::NOTE => Some(note(4)),
                delay::MIX => Some(value(delay::TABLE, delay::MIX, 100.0)),
                delay::FEEDBACK => Some(0.0),
                _ => None,
            };
            let extra = extra.iter().find(|(id, _)| *id == info.id).map(|&(_, v)| v);
            (
                info.id,
                extra
                    .or(set)
                    .unwrap_or_else(|| info.normalised(info.default)),
            )
        })
        .collect()
}

/// Where and how loud the loudest frame of `samples[from..]` is.
fn loudest(samples: &[f32], from: usize) -> (usize, f32) {
    let mut at = from;
    let mut level = 0.0f32;
    for (k, sample) in samples.iter().enumerate().skip(from) {
        if sample.abs() > level {
            level = sample.abs();
            at = k;
        }
    }
    (at, level)
}

/// 120 BPM until sample 96000 (four quarters in), then 60: the tempo map a
/// delay follows.
fn changing_tempo() -> Timeline {
    let tempo = TempoMap::new(vec![
        TempoPoint::step(Ticks::ZERO, 120.0),
        TempoPoint::step(Ticks::quarters(4), 60.0),
    ])
    .expect("a valid tempo map");
    Timeline::new(tempo, MeterMap::constant(Meter::FOUR_FOUR).expect("4/4"))
}

#[test]
fn a_synced_echo_follows_the_song_s_tempo_map_to_the_sample() {
    // An impulse a millisecond in (past the play fade), and another at the
    // tempo change: the first echoes at 25000 (a quarter at 120), the second
    // at 144000 (a quarter at 60, from 96000).
    let clip = impulses(TRACK, &[(1_000, 1.0), (96_000, 1.0)]);
    let routing = routing(delay_values(&[]), None);
    let mut rig = rig(
        &changing_tempo(),
        clip,
        &routing,
        Box::new(delay::Delay::new()),
    );
    rig.play();
    let heard = rig.run(160_000, 128);
    let (at, level) = loudest(&heard, 1_001);
    assert_eq!(at, 25_000, "the echo at 120 BPM: {level}");
    assert!((level - 1.0).abs() < 1e-3, "{level}");
    let (at, level) = loudest(&heard, 96_001);
    assert_eq!(at, 144_000, "the echo after the change: {level}");
    assert!((level - 1.0).abs() < 1e-3, "{level}");
}

#[test]
fn both_devices_play_the_same_audio_at_every_block_size_in_the_engine() {
    // The same feedback lane, drawn in seconds, at 64, 97, 128, 512 and 1024.
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: 0.3,
            shape: lumen_core::automation::Shape::Linear,
        },
        AutoPoint {
            at: 3_000,
            value: 0.8,
            shape: lumen_core::automation::Shape::Curve(lumen_core::curve::Curve::FAST),
        },
        AutoPoint {
            at: 6_000,
            value: 0.1,
            shape: lumen_core::automation::Shape::Step,
        },
    ])
    .unwrap();
    let lane = ParamLane {
        param: delay::FEEDBACK,
        lane: lane.clone(),
    };
    let render = |block: usize| {
        let routing = routing(delay_values(&[(delay::FEEDBACK, 0.3)]), Some(lane.clone()));
        let mut rig = rig(
            &Timeline::default(),
            impulses(TRACK, &[(1_000, 1.0)]),
            &routing,
            Box::new(delay::Delay::new()),
        );
        rig.play();
        rig.run(80_000, block)
    };
    let whole = render(64);
    for block in [97usize, 128, 512, 1_024] {
        let parts = render(block);
        for (frame, (a, b)) in whole.iter().zip(&parts).enumerate() {
            assert!(
                (a - b).abs() < 1e-5,
                "block {block}, frame {frame}: {a} against {b}"
            );
        }
    }
    // The lane is really heard: the first echo (the feedback still at 0.3) is
    // louder than the train after the lane has fallen to 0.1.
    let loud = whole[24_000..26_000]
        .iter()
        .fold(0.0f32, |m, s| m.max(s.abs()));
    let quiet = whole[40_000..80_000]
        .iter()
        .fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(loud > 0.05, "the first echo: {loud}");
    assert!(quiet < loud * 0.9, "{loud} then {quiet}");
}

#[test]
fn an_offline_render_plays_what_playback_plays_with_a_delay() {
    use lumen_engine::{Ended, SourceRender, render_source};
    use std::sync::atomic::AtomicBool;
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: 0.4,
            shape: lumen_core::automation::Shape::Linear,
        },
        AutoPoint {
            at: 2_000,
            value: 0.85,
            shape: lumen_core::automation::Shape::Linear,
        },
    ])
    .unwrap();
    let lane = ParamLane {
        param: delay::FEEDBACK,
        lane,
    };
    let routing = routing(delay_values(&[(delay::FEEDBACK, 0.4)]), Some(lane.clone()));
    let mut rig = rig(
        &Timeline::default(),
        impulses(TRACK, &[(1_000, 0.8)]),
        &routing,
        Box::new(delay::Delay::new()),
    );
    rig.play();
    let live = rig.run(12_000, 128);

    let (mut clips, player) = clip_player();
    clips.set(vec![impulses(TRACK, &[(1_000, 0.8)])]).unwrap();
    let (mut handle, mut source) = mixer(player);
    assert!(handle.set(&routing).unwrap());
    handle
        .attach_builtin(DEVICE, Slot::new(Box::new(delay::Delay::new())))
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
            frames: 12_000,
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
    assert_eq!(rendered.len(), 12_000);
    for (frame, (a, b)) in live.iter().zip(&rendered).enumerate().skip(300) {
        assert!(
            a.to_bits() == b.to_bits(),
            "frame {frame}: {a} live against {b} rendered"
        );
    }
}

#[test]
fn a_delay_s_tail_rings_on_after_a_stop() {
    // A tone heard, then Stop: the echoes carry on, and the engine says it is
    // still sounding (the mixer's answer is the effect's tail).
    let routing = routing(
        delay_values(&[(delay::FEEDBACK, 0.7), (delay::MIX, 0.5)]),
        None,
    );
    let mut rig = rig(
        &Timeline::default(),
        tone(TRACK, 0.3),
        &routing,
        Box::new(delay::Delay::new()),
    );
    rig.play();
    rig.run(6_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    assert!(
        rig.engine.source().ringing(),
        "the delay still has echoes to play"
    );
    let heard = rig.run(6_000, 128);
    let after = heard.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(after > 1e-3, "the tail is heard: {after}");
}

#[test]
fn a_reverb_s_tail_rings_on_after_a_stop() {
    let values: Vec<(u32, f32)> = reverb::TABLE
        .iter()
        .map(|info| (info.id, info.normalised(info.default)))
        .collect();
    let routing = routing(values, None);
    let mut rig = rig(
        &Timeline::default(),
        tone(TRACK, 0.3),
        &routing,
        Box::new(reverb::Reverb::new()),
    );
    rig.play();
    rig.run(6_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    assert!(rig.engine.source().ringing(), "the reverb still rings");
    let heard = rig.run(6_000, 128);
    let after = heard[..4_000].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(after > 1e-3, "the tail is heard: {after}");
}

/// What a golden render is pinned by: twenty samples, its level (the sum of
/// |x|) and its energy (the sum of x²). A golden is frozen: one that differs
/// from its pin was changed on purpose, and the pin moves in the same commit.
struct Golden {
    pins: [f32; 20],
    level: f64,
    energy: f64,
}

fn check_golden(heard: &[f32], golden: &Golden, what: &str) {
    let close = |got: f64, want: f64| (got - want).abs() <= 1e-6 * want.abs().max(1e-6);
    assert_eq!(heard.len(), 5_000, "{what}");
    for (k, want) in golden.pins.iter().enumerate() {
        let got = heard[k * 250];
        assert!(
            close(f64::from(got), f64::from(*want)),
            "{what}: frame {} is {got:e}, pinned {want:e}",
            k * 250
        );
    }
    let level: f64 = heard.iter().map(|x| f64::from(x.abs())).sum();
    let energy: f64 = heard.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    assert!(
        close(level, golden.level),
        "{what}: the level moved: {level:e}"
    );
    assert!(
        close(energy, golden.energy),
        "{what}: the energy moved: {energy:e}"
    );
}

/// A delay with a feedback lane, rendered for the golden.
fn delay_golden_render() -> Vec<f32> {
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: 0.3,
            shape: lumen_core::automation::Shape::Linear,
        },
        AutoPoint {
            at: 2_000,
            value: 0.8,
            shape: lumen_core::automation::Shape::Curve(lumen_core::curve::Curve::FAST),
        },
        AutoPoint {
            at: 3_500,
            value: 0.35,
            shape: lumen_core::automation::Shape::Step,
        },
    ])
    .unwrap();
    let lane = ParamLane {
        param: delay::FEEDBACK,
        lane,
    };
    let routing = routing(
        delay_values(&[(delay::FEEDBACK, 0.3), (delay::MIX, 0.6)]),
        Some(lane),
    );
    let mut rig = rig(
        &Timeline::default(),
        rich(TRACK),
        &routing,
        Box::new(delay::Delay::new()),
    );
    rig.play();
    rig.run(5_000, 128)
}

/// A reverb with a decay lane, rendered for the golden.
fn reverb_golden_render() -> Vec<f32> {
    let lane = AutoLane::new(vec![
        AutoPoint {
            at: 0,
            value: value(reverb::TABLE, reverb::DECAY, 0.4),
            shape: lumen_core::automation::Shape::Linear,
        },
        AutoPoint {
            at: 2_000,
            value: value(reverb::TABLE, reverb::DECAY, 2.5),
            shape: lumen_core::automation::Shape::Linear,
        },
    ])
    .unwrap();
    let lane = ParamLane {
        param: reverb::DECAY,
        lane,
    };
    let values: Vec<(u32, f32)> = reverb::TABLE
        .iter()
        .map(|info| (info.id, info.normalised(info.default)))
        .collect();
    let routing = routing(values, Some(lane));
    let mut rig = rig(
        &Timeline::default(),
        rich(TRACK),
        &routing,
        Box::new(reverb::Reverb::new()),
    );
    rig.play();
    rig.run(5_000, 128)
}

#[test]
fn a_golden_render_of_parallax_is_pinned_to_its_samples() {
    let heard = delay_golden_render();
    let golden = Golden {
        pins: [
            0.0,
            0.120_616_16,
            0.070_492_61,
            0.072_122_35,
            0.129_090_25,
            -0.055_256_628,
            -0.068_814_4,
            0.008_349_069,
            0.085_281_81,
            0.047_303_192,
            0.052_432_068,
            -0.079_003_24,
            -0.057_372_56,
            -0.058_988_098,
            0.016_528_439,
            0.091_688_41,
            0.041_569_41,
            0.042_373_74,
            -0.086_132_58,
            -0.058_216_427,
        ],
        level: 278.095_261_032_916_8,
        energy: 21.007_810_760_790_775,
    };
    check_golden(&heard, &golden, "parallax");
}

#[test]
fn a_golden_render_of_afterglow_is_pinned_to_its_samples() {
    let heard = reverb_golden_render();
    let golden = Golden {
        pins: [
            0.0,
            0.145_540_28,
            0.097_287_744,
            0.116_249_6,
            0.242_044_24,
            -0.103_606_18,
            -0.129_027_01,
            0.015_654_506,
            0.154_351_32,
            0.096_447_56,
            0.103_963_31,
            -0.171_617_34,
            -0.122_880_295,
            -0.123_902_13,
            0.045_193_6,
            0.184_000_94,
            0.087_535_17,
            0.081_752_63,
            -0.177_344_32,
            -0.120_058_134,
        ],
        level: 520.287_903_960_329_9,
        energy: 72.267_926_613_704_26,
    };
    check_golden(&heard, &golden, "afterglow");
}
