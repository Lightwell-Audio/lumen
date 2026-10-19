//! The host's side of the audio path: once a plugin is set up and its first
//! blocks have grown the host's scratch space, `Instance::process_into` (what
//! the block thread calls for every block) takes no heap operation, with
//! events, parameter points, a block longer than the plugin was promised
//! (given in pieces) and plugin edits in the queue (M6 review M3).
//!
//! The plugin's own allocations are its own: the test plugin is a separate
//! module with an allocator of its own. This does not run the host's block
//! thread (its locks and the wait are covered by `shm_rt` and the soak).

mod support;

use lumen_plugin_host::instance::{Buffers, Format};
use lumen_plugin_host::protocol::ParamQueue;
use lumen_plugin_host::{Instance, Module};
use lumen_test_plugin::params::LEVEL;
use support::{effect_class, note_off, note_on, plugin};

lumen_rt::install_guarded_allocator!();

const FRAMES: u32 = 128;

fn level() -> Vec<ParamQueue> {
    vec![ParamQueue {
        id: LEVEL,
        points: vec![(0, 0.5), (32, 1.0)],
    }]
}

/// Runs `blocks` blocks of `frames` frames through `instance` inside the
/// real-time section and returns the heap operations counted.
fn count(
    instance: &mut Instance,
    buffers: &mut Buffers,
    frames: u32,
    blocks: u32,
    what: &str,
) -> u64 {
    let events = [note_on(3, 60), note_off(50, 60)];
    let params = level();
    // The first blocks grow what the host keeps; they are not counted.
    for _ in 0..4 {
        instance
            .process_into(frames, buffers, &events, &params)
            .unwrap_or_else(|e| panic!("{what}: warm-up: {e:?}"));
    }
    let (ok, heap) = lumen_rt::count_rt_violations(|| {
        let mut ok = 0_u32;
        for _ in 0..blocks {
            if instance
                .process_into(frames, buffers, &events, &params)
                .is_ok()
            {
                ok += 1;
            }
        }
        ok
    });
    assert_eq!(ok, blocks, "{what}: every block was processed");
    heap
}

#[test]
fn an_instrument_is_processed_without_a_heap_operation_once_it_is_warm() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is not installed"
    );
    let module = Module::load(plugin()).expect("load");
    let mut instance = Instance::create(&module, None).expect("create");
    let format = Format {
        sample_rate: 48_000.0,
        max_frames: 256,
        inputs: 0,
        outputs: 2,
    };
    instance.configure(format).expect("configure");
    let mut buffers = Buffers {
        input: vec![],
        output: vec![vec![0.0; 1024]; 2],
    };
    let heap = count(&mut instance, &mut buffers, FRAMES, 500, "instrument");
    assert_eq!(heap, 0, "heap operations while processing an instrument");
    // A block longer than the plugin was promised is given in pieces.
    let heap = count(&mut instance, &mut buffers, 600, 100, "long blocks");
    assert_eq!(heap, 0, "heap operations while splitting long blocks");
}

#[test]
fn an_effect_is_processed_without_a_heap_operation_once_it_is_warm() {
    let module = Module::load(plugin()).expect("load");
    let mut instance = Instance::create(&module, Some(effect_class())).expect("create");
    let format = Format {
        sample_rate: 48_000.0,
        max_frames: 256,
        inputs: 2,
        outputs: 2,
    };
    instance.configure(format).expect("configure");
    let mut buffers = Buffers {
        input: vec![vec![0.25; 1024]; 2],
        output: vec![vec![0.0; 1024]; 2],
    };
    let heap = count(&mut instance, &mut buffers, FRAMES, 500, "effect");
    assert_eq!(heap, 0, "heap operations while processing an effect");
}
