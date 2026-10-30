//! The R2 spike (M7b E4, ADR-0096): can a plugin in its own host process be
//! rendered without a device, with a result that is repeatable and equal to
//! what real time plays? The test effect runs in a real host process behind
//! the real mixer; the engine is driven as fast as it will go, with and
//! without its offline mode.

mod support;

use std::time::Duration;

use lumen_core::time::Ticks;
use lumen_devices::{Guard, HostedInstrument, Status};
use lumen_engine::{
    Clip, ClipAudio, Command, Destination, Device, InstrumentSpec, Layered, MidiClip, MidiNote,
    MidiTrack, NodeSpec, Routing, Target, Timeline, clip_player, control_list, engine, midi_player,
    mixer, note_list,
};
use lumen_hosting::Manager;
use lumen_test_plugin::params::{Kind, LATENCY, LEVEL};
use std::sync::Arc;
use support::*;

const RATE_HZ: u32 = 48_000;
const MASTER: u64 = 100;
const DEVICE: u64 = 7;

/// What a run left behind.
struct Run {
    out: Vec<f32>,
    /// Blocks the host did not answer in time, and whether it was failed.
    late: u32,
    failed: bool,
    /// Blocks rendered (a run stops at the first late or failed block).
    blocks: usize,
}

/// Audio whose every sample differs, to catch any misplaced frame.
fn audio() -> Arc<ClipAudio> {
    let ramp: Vec<f32> = (0..480_000)
        .map(|i| ((i % 997) as f32 - 498.0) / 1e3)
        .collect();
    Arc::new(ClipAudio::Memory {
        sample_rate: RATE_HZ,
        channels: vec![ramp.clone(), ramp],
    })
}

/// The test effect at -6 dB with 96 frames of latency, with `fault` on.
fn start(fault: &str) -> lumen_hosting::Start {
    let mut start = effect(fault);
    start.values = vec![
        (LEVEL, Kind::Effect.normalized(LEVEL, -6.0)),
        (LATENCY, 96.0 / 4096.0),
    ];
    start
}

/// Renders `frames` of one track through the effect, in blocks of `block`.
/// `offline` is how long the host is waited for per block, or real time.
fn run(fault: &str, offline: Option<Duration>, block: usize, frames: usize) -> Run {
    let mut manager = Manager::new(|| {});
    manager.want(DEVICE, start(fault)).unwrap();
    let linked = *linked(&mut manager, DEVICE);
    let latency = linked.latency;
    let guard = Guard::new(Box::new(linked.link), latency);
    let status: Arc<Status> = guard.status();

    let audio = audio();
    let (mut clips, player) = clip_player();
    clips
        .set(vec![Clip {
            envelope: Default::default(),
            id: 1,
            track: 1,
            start: Ticks::ZERO,
            offset: 0,
            length: audio.frames() as i64,
            gain: 1.0,
            audio: Arc::clone(&audio),
            shape: Default::default(),
        }])
        .unwrap();
    let (mut routing, source) = mixer(player);
    let node = NodeSpec {
        devices: vec![Device::Hosted {
            host: DEVICE,
            latency,
        }],
        ..NodeSpec::unity(1, Destination::Node(MASTER))
    };
    assert!(
        routing
            .set(&Routing {
                nodes: vec![NodeSpec::unity(MASTER, Destination::Hardware(0)), node],
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    assert!(routing.attach_host(DEVICE, guard).is_ok());
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE_HZ);
    engine.set_offline(offline);
    transport.send(Command::Play).unwrap();

    let mut data = vec![0.0; 2 * block];
    let mut out = Vec::with_capacity(frames);
    let (mut done, mut blocks) = (0, 0);
    while done < frames {
        let n = block.min(frames - done);
        engine.process(RATE_HZ, &mut data[..2 * n], 2, n);
        out.extend_from_slice(&data[..n]);
        done += n;
        blocks += 1;
        // An offline render ends at the first block the plugin missed.
        if offline.is_some() && (status.failed() || status.late_blocks() > 0) {
            break;
        }
    }
    Run {
        out,
        late: status.late_blocks(),
        failed: status.failed(),
        blocks,
    }
}

const FAST: Option<Duration> = Some(Duration::from_secs(10));

#[test]
fn an_offline_render_is_repeatable_and_the_block_size_changes_nothing_audible() {
    let a = run("", FAST, 128, 24_000);
    let b = run("", FAST, 128, 24_000);
    let c = run("", FAST, 333, 24_000);
    assert_eq!((a.late, a.failed, b.late, c.late), (0, false, 0, 0));
    assert!(a.out.iter().any(|&s| s.abs() > 0.01), "it is not silence");
    assert_eq!(
        a.out, b.out,
        "a second render is the same, sample for sample"
    );
    // Another block size lands the mixer's level glides on other chunk edges:
    // a rounding of 3e-8 (-150 dB) on a few samples, nothing the plugin does.
    // A render uses one block size, so it is exactly repeatable.
    let worst = a
        .out
        .iter()
        .zip(&c.out)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst < 1e-6, "{worst}");
}

#[test]
fn offline_equals_real_time_while_the_host_keeps_up() {
    // The claim holds while the host keeps up. On a busy CI machine a real
    // time run can miss blocks for reasons outside the plugin, so it is
    // tried a few times for a run that kept up.
    let live = (0..5)
        .map(|_| run("", None, 256, 24_000))
        .find(|r| r.late == 0)
        .expect("the test effect keeps up in real time in one of five runs");
    let offline = run("", FAST, 256, 24_000);
    assert_eq!(live.out, offline.out);
}

#[test]
fn a_plugin_too_slow_for_real_time_still_renders_offline_in_full() {
    // 6 ms a block against the 4 ms (three quarters of 256 frames at 48 kHz)
    // that real time allows.
    let reference = run("", FAST, 256, 24_000);
    let live = run("process_slow=6000", None, 256, 24_000);
    assert!(live.late > 0, "real time missed blocks");
    assert_ne!(live.out, reference.out, "and so played something else");
    let offline = run("process_slow=6000", FAST, 256, 24_000);
    assert_eq!(offline.late, 0);
    assert_eq!(offline.out, reference.out, "offline loses nothing");
}

#[test]
fn a_plugin_that_crashes_mid_render_ends_it_with_a_result() {
    let r = run("process_crash@30", FAST, 256, 48_000);
    assert!(r.failed || r.late > 0, "the failure is seen");
    assert!(r.blocks < 48_000 / 256, "the render stopped at it");
}

#[test]
fn a_plugin_that_hangs_ends_the_render_after_one_wait() {
    let waited = std::time::Instant::now();
    let r = run(
        "process_hang@30",
        Some(Duration::from_millis(500)),
        256,
        48_000,
    );
    assert!(r.late > 0 || r.failed);
    assert!(r.blocks < 48_000 / 256);
    assert!(
        waited.elapsed() < Duration::from_secs(60),
        "one wait, not one per block of a second of audio"
    );
}

/// The test instrument playing three notes, through the MIDI player and the
/// mixer; run as `run` is.
fn run_instrument(fault: &str, offline: Option<Duration>, block: usize, frames: usize) -> Run {
    let mut manager = Manager::new(|| {});
    manager.want(DEVICE, instrument(fault)).unwrap();
    let linked = *linked(&mut manager, DEVICE);
    let latency = linked.latency;
    let guard = Guard::new(Box::new(linked.link), latency);
    let status: Arc<Status> = guard.status();

    let note = |start: i64, pitch: u8| MidiNote {
        start: Ticks::quarters(start),
        length: Ticks::quarters(1),
        pitch,
        velocity: 100,
    };
    let track = MidiTrack::new(
        1,
        Target::Instrument,
        Some(InstrumentSpec {
            kind: 2,
            preset: 0,
            instrument: Box::new(HostedInstrument::new(guard)),
        }),
        vec![MidiClip {
            id: 1,
            start: Ticks::ZERO,
            length: Ticks::quarters(16),
            period: None,
            seams: Default::default(),
            notes: note_list(vec![note(0, 60), note(1, 64), note(2, 67)]),
            controls: control_list(Vec::new()),
        }],
    );
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    midi.set(vec![track]).ok().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    routing
        .set(&Routing {
            nodes: vec![
                NodeSpec::unity(MASTER, Destination::Hardware(0)),
                NodeSpec {
                    source: latency,
                    ..NodeSpec::unity(1, Destination::Node(MASTER))
                },
            ],
            cue: None,
            measure: None,
        })
        .unwrap();
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE_HZ);
    engine.set_offline(offline);
    transport.send(Command::Play).unwrap();

    let mut data = vec![0.0; 2 * block];
    let mut out = Vec::with_capacity(frames);
    let (mut done, mut blocks) = (0, 0);
    while done < frames {
        let n = block.min(frames - done);
        engine.process(RATE_HZ, &mut data[..2 * n], 2, n);
        out.extend_from_slice(&data[..n]);
        done += n;
        blocks += 1;
        if offline.is_some() && (status.failed() || status.late_blocks() > 0) {
            break;
        }
    }
    Run {
        out,
        late: status.late_blocks(),
        failed: status.failed(),
        blocks,
    }
}

#[test]
fn an_instrument_renders_offline_repeatably_and_as_real_time_plays_it() {
    let a = run_instrument("", FAST, 256, 60_000);
    let b = run_instrument("", FAST, 256, 60_000);
    // As in offline_equals_real_time_while_the_host_keeps_up: a real time
    // run that kept up, of up to five on a busy CI machine.
    let live = (0..5)
        .map(|_| run_instrument("", None, 256, 60_000))
        .find(|r| r.late == 0)
        .expect("the instrument keeps up in real time in one of five runs");
    assert_eq!((a.late, a.failed), (0, false));
    assert!(a.out.iter().any(|&s| s.abs() > 0.01), "it is not silence");
    assert_eq!(a.out, b.out);
    assert_eq!(live.out, a.out);
}

#[test]
fn an_instrument_too_slow_for_real_time_still_renders_offline_in_full() {
    let reference = run_instrument("", FAST, 256, 60_000);
    let live = run_instrument("process_slow=6000", None, 256, 60_000);
    assert!(live.late > 0);
    assert_ne!(live.out, reference.out);
    let offline = run_instrument("process_slow=6000", FAST, 256, 60_000);
    assert_eq!(offline.late, 0);
    assert_eq!(offline.out, reference.out);
}

#[test]
fn an_instrument_that_crashes_mid_render_ends_it_with_a_result() {
    let r = run_instrument("process_crash@30", FAST, 256, 60_000);
    assert!(r.failed || r.late > 0);
    assert!(r.blocks < 60_000 / 256);
}
