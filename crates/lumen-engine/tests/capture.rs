//! Recording audio in the engine (M9 slice R2): a route that records keeps the
//! input it takes while the transport records, stamped with where it was
//! heard; the count-in, a stop and plain playback keep nothing; a loop wrap
//! shows as a jump back in `at` with `device` unbroken; a full ring drops a
//! whole chunk and the gap shows in `device`; and none of it touches the heap.

use lumen_core::time::Ticks;
use lumen_engine::{
    CaptureChunk, CaptureReader, Command, Destination, Engine, EngineHandle, InputHandle,
    InputPlayer, InputRoute, Mixer, MixerHandle, Monitoring, NodeSpec, Region, Routing, Timeline,
    engine, input_player, mixer,
};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

const MASTER: u64 = 100;
const TRACK: u64 = 1;
const RATE: u32 = 48_000;
const BLOCK: usize = 128;
/// One beat at 120 BPM and 48 kHz.
const BEAT: u64 = 24_000;

struct Rig {
    transport: EngineHandle,
    inputs: InputHandle,
    routing: MixerHandle,
    engine: Engine<Mixer<InputPlayer>>,
    reader: CaptureReader,
    /// Device frames fed so far.
    fed: u64,
}

/// The sample the input holds at device frame `frame`.
fn at(frame: u64) -> f32 {
    (frame % 100_000) as f32 * 1e-5
}

fn rig(stereo: bool) -> Rig {
    let (mut inputs, player) = input_player();
    assert!(inputs.set_routes(&[InputRoute {
        track: TRACK,
        channel: 0,
        stereo,
        monitoring: Monitoring::Never,
        record: true,
    }]));
    let reader = inputs.take_capture().unwrap();
    let (mut routing, source) = mixer(player);
    let nodes = vec![
        NodeSpec::unity(MASTER, Destination::Hardware(0)),
        NodeSpec::unity(TRACK, Destination::Node(MASTER)),
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
        reader,
        fed: 0,
    }
}

impl Rig {
    /// Runs `blocks` blocks of two input channels: the left holds `at(frame)`,
    /// the right its negative.
    fn run(&mut self, blocks: usize) {
        let mut out = vec![0.0; 2 * BLOCK];
        let mut input = vec![0.0; 2 * BLOCK];
        for _ in 0..blocks {
            for i in 0..BLOCK {
                input[i] = at(self.fed + i as u64);
                input[BLOCK + i] = -at(self.fed + i as u64);
            }
            self.engine
                .process_with_input(RATE, &input, 2, &mut out, 2, BLOCK);
            self.fed += BLOCK as u64;
            self.inputs.collect_garbage();
            self.routing.collect_garbage();
        }
    }

    /// Everything kept so far: chunks and their samples.
    fn kept(&mut self) -> Vec<(CaptureChunk, Vec<f32>)> {
        let mut out = Vec::new();
        let mut buf = Vec::new();
        while let Some(chunk) = self.reader.next(&mut buf) {
            out.push((chunk, buf.clone()));
        }
        out
    }
}

#[test]
fn recording_from_stopped_keeps_the_input_where_it_is_heard() {
    let mut rig = rig(false);
    rig.run(5);
    assert!(rig.kept().is_empty(), "nothing is kept before Record");
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(10);
    let kept = rig.kept();
    assert_eq!(kept.len(), 10);
    let first = kept[0].0.device;
    assert_eq!(first, 5 * BLOCK as u64, "from the block after the command");
    for (index, (chunk, samples)) in kept.iter().enumerate() {
        assert_eq!(chunk.device, first + (index * BLOCK) as u64);
        assert_eq!(chunk.at, (index * BLOCK) as i64, "heard from the start");
        assert_eq!((chunk.frames, chunk.channels), (BLOCK as u32, 1));
        assert_eq!(samples[0], at(chunk.device));
        assert_eq!(samples[BLOCK - 1], at(chunk.device + BLOCK as u64 - 1));
    }
}

#[test]
fn a_pair_is_kept_as_left_then_right() {
    let mut rig = rig(true);
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(1);
    let (chunk, samples) = rig.kept().remove(0);
    assert_eq!(chunk.channels, 2);
    assert_eq!(samples[3], at(3));
    assert_eq!(
        samples[BLOCK + 3],
        -at(3),
        "the right side follows the left"
    );
}

#[test]
fn a_count_in_keeps_nothing_and_the_take_begins_at_the_first_beat_after_it() {
    let mut rig = rig(false);
    rig.transport.send(Command::Record { count_in: 1 }).unwrap();
    rig.run(300);
    let kept = rig.kept();
    let first = &kept[0].0;
    // The beat is 24,000 frames; playback starts inside the block that holds it.
    assert_eq!(first.device, BEAT, "no frame of the count-in is kept");
    assert_eq!(first.at, 0);
    assert_eq!(first.frames as u64, 188 * BLOCK as u64 - BEAT);
    let seen: u64 = kept.iter().map(|(c, _)| u64::from(c.frames)).sum();
    assert_eq!(seen, rig.fed - BEAT, "every frame after it, once");
}

#[test]
fn stopping_ends_the_take_and_plain_playback_keeps_nothing() {
    let mut rig = rig(false);
    rig.transport.send(Command::Play).unwrap();
    rig.run(4);
    assert!(rig.kept().is_empty(), "playing back");
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(4);
    assert_eq!(
        rig.kept().len(),
        4,
        "recording while playing starts at once"
    );
    rig.transport.send(Command::EndRecord).unwrap();
    rig.run(4);
    assert!(rig.kept().is_empty(), "after EndRecord");
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    rig.run(2);
    rig.transport.send(Command::Stop).unwrap();
    rig.run(4);
    assert_eq!(rig.kept().len(), 2, "Stop ends it");
}

#[test]
fn a_loop_wrap_is_a_jump_back_in_the_timeline_and_none_in_the_device() {
    let mut rig = rig(false);
    // A loop of 1,000 frames' worth of beats is not round: use two beats.
    rig.transport
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(2),
            true,
        )))
        .unwrap();
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    // Three passes of 48,000 frames: 375 blocks each.
    rig.run(3 * 375);
    let kept = rig.kept();
    let mut wraps = 0;
    for pair in kept.windows(2) {
        let (a, b) = (&pair[0].0, &pair[1].0);
        assert_eq!(
            b.device,
            a.device + u64::from(a.frames),
            "device frames never skip"
        );
        if b.at != a.at + i64::from(a.frames) {
            wraps += 1;
            assert_eq!(b.at, 0, "the wrap lands on the loop start");
            assert_eq!(a.at + i64::from(a.frames), 2 * BEAT as i64);
        }
    }
    assert_eq!(wraps, 2, "three passes wrap twice");
}

#[test]
fn input_that_the_ring_cannot_hold_is_dropped_whole_and_shows_as_a_gap_in_device() {
    let mut rig = rig(false);
    rig.transport.send(Command::Record { count_in: 0 }).unwrap();
    // Nobody reads: the ring (4 Mi samples) fills in 32,768 blocks of 128.
    rig.run(33_000);
    assert!(rig.reader.dropped_chunks() > 0);
    let kept = rig.kept();
    assert_eq!(kept.len(), 32_768);
    assert_eq!(
        rig.reader.dropped_frames(),
        rig.reader.dropped_chunks() * BLOCK as u64
    );
    // Reading makes room again and the capture goes on, from a later device frame.
    rig.run(10);
    let after = rig.kept();
    assert_eq!(after.len(), 10);
    assert!(after[0].0.device > kept.last().unwrap().0.device + BLOCK as u64);
}

#[test]
fn capturing_never_touches_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let mut rig = rig(true);
    let mut out = vec![0.0; 2 * BLOCK];
    let input = vec![0.1; 2 * BLOCK];
    let mut buf = Vec::new();
    for block in 0..2_000_u32 {
        match block {
            0 => rig.transport.send(Command::Record { count_in: 1 }).unwrap(),
            700 => rig.transport.send(Command::Stop).unwrap(),
            800 => rig.transport.send(Command::Record { count_in: 0 }).unwrap(),
            1_200 => rig.transport.send(Command::EndRecord).unwrap(),
            _ => {}
        }
        if block % 300 == 150 {
            let on = block % 600 == 150;
            assert!(rig.inputs.set_routes(&[InputRoute {
                track: TRACK,
                channel: 0,
                stereo: on,
                monitoring: Monitoring::Armed,
                record: true,
            }]));
        }
        let ((), heap) = count_rt_violations(|| {
            rig.engine
                .process_with_input(RATE, &input, 2, &mut out, 2, BLOCK);
        });
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        let _ = rig.transport.transport();
        rig.inputs.collect_garbage();
        rig.routing.collect_garbage();
        // The worker keeps up.
        while rig.reader.next(&mut buf).is_some() {}
    }
}
