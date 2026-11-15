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

fn count(
    instance: &mut Instance,
    buffers: &mut Buffers,
    frames: u32,
    blocks: u32,
    what: &str,
) -> u64 {
    let events = [note_on(3, 60), note_off(50, 60)];
    let params = level();
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
