//! Large declared-bus counts stay bounded while unused outputs remain safe.

mod support;

use lumen_plugin_host::instance::{Buffers, Format};
use lumen_plugin_host::protocol::{Aux, Event, EventKind, ParamQueue};
use lumen_plugin_host::{Instance, InstanceError, Module};
use lumen_test_plugin::params::LEVEL;
use support::plugin;

lumen_rt::install_guarded_allocator!();

#[test]
fn declared_buses_configure_with_low_aux_and_warm_processing_allocates_nothing() {
    let fault = lumen_test_plugin::faults::ENV;
    // This test runs in its own integration-test process and is its only test;
    // the plugin reads the value when each component is created.
    // SAFETY: no concurrent tests or plugin threads read this environment.
    unsafe { std::env::set_var(fault, "multi_out=59") };
    let module = Module::load(plugin()).expect("load");
    let mut instance = Instance::create(&module, None).expect("create instrument");

    // The public bus list and routable mask remain bounded to the first 32.
    let (_, outputs) = instance.buses();
    assert_eq!(outputs.len(), 32);
    assert_eq!(outputs[31].name, "Output 32");

    instance
        .configure_with(
            Format {
                sample_rate: 48_000.0,
                max_frames: 64,
                inputs: 0,
                outputs: 2,
            },
            Aux {
                inputs: 0,
                outputs: 1 << 1,
            },
        )
        .expect("configure all declared buses with one low auxiliary bus on");
    let mut buffers = Buffers {
        input: Vec::new(),
        output: vec![vec![0.0; 64]; 4],
    };
    let events = [Event {
        offset: 0,
        kind: EventKind::NoteOn {
            channel: 0,
            pitch: 1,
            velocity: 1.0,
        },
    }];
    let params = [ParamQueue {
        id: LEVEL,
        points: vec![(0, 1.0)],
    }];

    for _ in 0..4 {
        instance
            .process_into(64, &mut buffers, &events, &params)
            .expect("warm process buffers");
    }
    let (done, heap_operations) = lumen_rt::count_rt_violations(|| {
        (0..64).try_for_each(|_| {
            instance
                .process_into(64, &mut buffers, &events, &params)
                .map(|_| ())
        })
    });
    assert!(done.is_ok(), "all warmed blocks process: {done:?}");
    assert_eq!(heap_operations, 0, "warmed processing allocates nothing");
    assert!(
        buffers.output[2].iter().any(|sample| *sample != 0.0),
        "the enabled low auxiliary bus receives audio"
    );

    // The declared-bus limit prevents excessive plugin-controlled stack and
    // vector sizes before any bus arrangement is passed back to the plugin.
    // SAFETY: the deterministic fixture starts no threads; this is the sole test.
    unsafe { std::env::set_var(fault, "multi_out=129") };
    let mut too_many = Instance::create(&module, None).expect("create oversized fixture");
    let error = too_many
        .configure_with(
            Format {
                sample_rate: 48_000.0,
                max_frames: 64,
                inputs: 0,
                outputs: 2,
            },
            Aux::default(),
        )
        .expect_err("more than 128 declared buses are refused");
    assert!(matches!(error, InstanceError::Plugin(_)));
    assert!(error.to_string().contains("129"), "{error}");
}
