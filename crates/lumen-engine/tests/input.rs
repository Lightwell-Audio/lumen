//! Audio input through the engine (M9 slice R1): the device's inputs are
//! metered whatever the transport does, an armed track is heard stopped, while
//! counting in and while recording and not in plain playback, the route's
//! track chain hears it, and none of it touches the heap.

use lumen_engine::{
    Command, Destination, Engine, EngineHandle, InputHandle, InputPlayer, InputRoute, Mixer,
    MixerHandle, Monitoring, NodeSpec, Routing, Timeline, engine, input_player, mixer,
};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

const MASTER: u64 = 100;
const VOCAL: u64 = 1;
const RATE: u32 = 48_000;
const BLOCK: usize = 128;
const INPUT: f32 = 0.25;

struct Rig {
    transport: EngineHandle,
    inputs: InputHandle,
    routing: MixerHandle,
    engine: Engine<Mixer<InputPlayer>>,
    /// Frames fed so far.
    fed: usize,
}

fn rig(monitoring: Monitoring) -> Rig {
    let (mut inputs, player) = input_player();
    assert!(inputs.set_routes(&[InputRoute {
        track: VOCAL,
        channel: 1,
        stereo: false,
        monitoring,
    }]));
    let (mut routing, source) = mixer(player);
    let nodes = vec![
        NodeSpec::unity(MASTER, Destination::Hardware(0)),
        NodeSpec::unity(VOCAL, Destination::Node(MASTER)),
    ];
    assert!(
        routing
            .set(&Routing {
                nodes,
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    let (transport, engine) = engine(Timeline::default(), source, RATE);
    Rig {
        transport,
        inputs,
        routing,
        engine,
        fed: 0,
    }
}

impl Rig {
    /// Runs `blocks` blocks of two input channels (silence, then `level`) and
    /// returns the last block's left output.
    fn run(&mut self, blocks: usize, level: f32) -> Vec<f32> {
        let mut out = vec![0.0; 2 * BLOCK];
        let mut input = vec![0.0; 2 * BLOCK];
        input[BLOCK..].fill(level);
        for _ in 0..blocks {
            self.engine
                .process_with_input(RATE, &input, 2, &mut out, 2, BLOCK);
            self.fed += BLOCK;
        }
        out[..BLOCK].to_vec()
    }
}

fn heard(block: &[f32]) -> bool {
    block.iter().any(|&x| x.abs() > 0.01)
}

#[test]
fn an_armed_track_is_heard_stopped_through_its_chain_to_the_output() {
    let mut rig = rig(Monitoring::Armed);
    let out = rig.run(8, INPUT);
    assert!(
        out.iter().all(|&x| (x - INPUT).abs() < 1e-5),
        "unity through the track and the master: {:?}",
        &out[..4]
    );
}

#[test]
fn auto_monitoring_is_silent_in_plain_playback_and_back_while_recording() {
    let mut rig = rig(Monitoring::Armed);
    rig.transport.send(Command::Play).unwrap();
    assert!(!heard(&rig.run(8, INPUT)), "playing back");
    rig.transport.send(Command::Stop).unwrap();
    assert!(heard(&rig.run(8, INPUT)), "stopped again");
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    assert!(heard(&rig.run(8, INPUT)), "recording");
    rig.transport.send(Command::EndRecord).unwrap();
    assert!(!heard(&rig.run(8, INPUT)), "playing on after the take");
}

#[test]
fn a_count_in_is_monitored_and_input_in_always_plays_under_playback() {
    let mut rig = rig(Monitoring::Armed);
    rig.transport.send(Command::Record { count_in: 1 }).unwrap();
    assert!(heard(&rig.run(8, INPUT)), "counting in");
    let mut always = self::rig(Monitoring::Always);
    always.transport.send(Command::Play).unwrap();
    assert!(heard(&always.run(8, INPUT)), "monitor In");
}

#[test]
fn the_monitor_is_off_for_never_and_the_meters_run_anyway() {
    let mut rig = rig(Monitoring::Never);
    assert!(!heard(&rig.run(8, INPUT)));
    let quiet = rig.inputs.take_level(0);
    let level = rig.inputs.take_level(1);
    assert_eq!(quiet.peak, 0.0);
    assert!((level.peak - INPUT).abs() < 1e-6 && level.rms > 0.0 && !level.clipped);
}

#[test]
fn an_input_at_full_scale_lights_the_clip_indicator() {
    let mut rig = rig(Monitoring::Never);
    let _ = rig.run(2, 1.0);
    assert!(rig.inputs.take_level(1).clipped);
    let _ = rig.run(2, 0.5);
    assert!(!rig.inputs.take_level(1).clipped, "taken once");
}

#[test]
fn a_stream_without_inputs_plays_as_before() {
    let mut rig = rig(Monitoring::Always);
    let mut out = vec![0.0; 2 * BLOCK];
    rig.engine.process(RATE, &mut out, 2, BLOCK);
    assert!(!heard(&out[..BLOCK]));
    let _ = (&rig.routing, rig.fed);
}

#[test]
fn input_never_touches_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let mut rig = rig(Monitoring::Armed);
    let mut out = vec![0.0; 2 * BLOCK];
    let mut input = vec![0.0; 2 * BLOCK];
    input[BLOCK..].fill(INPUT);
    for block in 0..3_000_u32 {
        match block {
            0 => rig.transport.send(Command::Play).unwrap(),
            500 => rig.transport.send(Command::Stop).unwrap(),
            900 => rig.transport.send(Command::Record { count_in: 1 }).unwrap(),
            1_500 => rig.transport.send(Command::Stop).unwrap(),
            _ => {}
        }
        // New route lists arrive while it runs, some for tracks the routing
        // does not have, one with no inputs at all.
        if block % 250 == 100 {
            let monitoring = if block % 500 == 100 {
                Monitoring::Always
            } else {
                Monitoring::Never
            };
            let routes = [
                InputRoute {
                    track: VOCAL,
                    channel: 1,
                    stereo: block % 3 == 0,
                    monitoring,
                },
                InputRoute {
                    track: 77,
                    channel: 0,
                    stereo: false,
                    monitoring: Monitoring::Armed,
                },
            ];
            assert!(
                rig.inputs
                    .set_routes(if block % 750 == 100 { &[] } else { &routes })
            );
        }
        let channels = if block % 400 == 399 { 0 } else { 2 };
        let ((), heap) = count_rt_violations(|| {
            rig.engine
                .process_with_input(RATE, &input, channels, &mut out, 2, BLOCK);
        });
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        let _ = rig.transport.transport();
        rig.inputs.collect_garbage();
        rig.routing.collect_garbage();
    }
}
