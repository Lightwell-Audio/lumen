//! Audio buses through a real host process (M6 slice 6b): the list a plugin
//! declares, the auxiliary buses turned on at configuration and their
//! channels in a block, the sidechain input, and a plugin that changes its
//! outputs while it runs.

mod support;

use lumen_plugin_host::client::HostProcess;
use lumen_plugin_host::protocol::{Aux, ParamQueue, RESTART_RECONFIGURE};
use lumen_test_plugin::controller::TRIGGER;
use lumen_test_plugin::params::LEVEL;
use support::{SLOW, effect_class, note_on, plugin, spawn};

const FRAMES: u32 = 64;

/// A host with the instrument loaded and not configured.
fn loaded_instrument(fault: &str) -> HostProcess {
    let mut host = spawn(fault);
    host.load(plugin(), None, SLOW).expect("load");
    host
}

fn loaded_effect(fault: &str) -> HostProcess {
    let mut host = spawn(fault);
    host.load(plugin(), Some(effect_class()), SLOW)
        .expect("load");
    host
}

/// The plugin's level at full, so a note is heard at full scale.
fn full() -> Vec<ParamQueue> {
    vec![ParamQueue {
        id: LEVEL,
        points: vec![(0, 1.0)],
    }]
}

/// One block of held notes at `pitches`, the channels it carries.
fn play(host: &mut HostProcess, pitches: &[i16]) -> Vec<Vec<f32>> {
    let events = pitches.iter().map(|&p| note_on(0, p)).collect();
    host.process(FRAMES, vec![], events, full(), SLOW)
        .expect("process")
        .output
}

/// What one note sounds like on the main bus of a plain instrument.
fn alone(pitch: i16) -> Vec<f32> {
    let mut host = loaded_instrument("");
    host.configure(48_000.0, 512, 0, 2, SLOW).unwrap();
    play(&mut host, &[pitch]).swap_remove(0)
}

#[test]
fn a_plugin_lists_the_buses_it_declares() {
    let mut host = loaded_instrument("multi_out=3");
    let (inputs, outputs) = host.buses(SLOW).unwrap();
    assert!(inputs.is_empty());
    assert_eq!(outputs.len(), 3);
    assert_eq!(
        (
            outputs[0].name.as_str(),
            outputs[0].main,
            outputs[0].default_active
        ),
        ("Output", true, true)
    );
    assert_eq!(
        (
            outputs[1].name.as_str(),
            outputs[1].main,
            outputs[1].default_active
        ),
        ("Output 2", false, false)
    );
    assert_eq!(outputs[2].channels, 2);

    let mut host = loaded_effect("sidechain");
    let (inputs, outputs) = host.buses(SLOW).unwrap();
    assert_eq!(outputs.len(), 1);
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[1].name, "Sidechain");
    assert!(!inputs[1].main && !inputs[1].default_active);

    let mut host = loaded_instrument("");
    let (_, outputs) = host.buses(SLOW).unwrap();
    assert_eq!(outputs.len(), 1, "a plain instrument has one output");
}

#[test]
fn each_output_bus_turned_on_carries_its_own_notes() {
    let mut host = loaded_instrument("multi_out=3");
    let done = host
        .configure_buses(
            48_000.0,
            512,
            (0, 2),
            Aux {
                inputs: 0,
                outputs: 0b110,
            },
            SLOW,
        )
        .unwrap();
    assert_eq!((done.inputs, done.outputs), (0, 6), "three stereo buses");
    // Pitches 60, 61 and 62 pick buses 0, 1 and 2.
    let out = play(&mut host, &[60, 61, 62]);
    assert_eq!(out.len(), 6);
    for (bus, pitch) in [(0, 60), (1, 61), (2, 62)] {
        let want = alone(pitch);
        assert_eq!(out[bus * 2], want, "bus {bus} left");
        assert_eq!(out[bus * 2 + 1], want, "bus {bus} right");
    }
}

#[test]
fn a_bus_that_is_off_plays_on_the_main_bus_instead() {
    let mut host = loaded_instrument("multi_out=3");
    // Only bus 2 is on: pitch 61 (bus 1) has nowhere else to go.
    let done = host
        .configure_buses(
            48_000.0,
            512,
            (0, 2),
            Aux {
                inputs: 0,
                outputs: 0b100,
            },
            SLOW,
        )
        .unwrap();
    assert_eq!(done.outputs, 4);
    let out = play(&mut host, &[60, 61, 62]);
    let main: Vec<f32> = alone(60)
        .iter()
        .zip(alone(61))
        .map(|(a, b)| a + b)
        .collect();
    assert_eq!(
        out[0], main,
        "the main bus has the notes of the bus that is off"
    );
    assert_eq!(out[2], alone(62), "the bus that is on has its own");
}

#[test]
fn a_plugin_configured_without_the_auxiliary_buses_plays_everything_on_the_main_one() {
    let mut host = loaded_instrument("multi_out=3");
    let done = host
        .configure_buses(48_000.0, 512, (0, 2), Aux::default(), SLOW)
        .unwrap();
    assert_eq!(done.outputs, 2);
    let out = play(&mut host, &[60, 61, 62]);
    assert_eq!(out.len(), 2);
    let sum: Vec<f32> = (0..FRAMES as usize)
        .map(|i| alone(60)[i] + alone(61)[i] + alone(62)[i])
        .collect();
    assert_eq!(out[0], sum);
}

#[test]
fn a_bus_the_plugin_does_not_have_is_refused_with_the_bus_in_the_reason() {
    let mut host = loaded_instrument("multi_out=2");
    let err = host
        .configure_buses(
            48_000.0,
            512,
            (0, 2),
            Aux {
                inputs: 0,
                outputs: 1 << 5,
            },
            SLOW,
        )
        .unwrap_err();
    assert!(err.to_string().contains("output bus 5"), "{err}");
}

#[test]
fn a_sidechain_input_reaches_the_effect_on_the_same_samples() {
    let mut host = loaded_effect("sidechain");
    let done = host
        .configure_buses(
            48_000.0,
            512,
            (2, 2),
            Aux {
                inputs: 0b10,
                outputs: 0,
            },
            SLOW,
        )
        .unwrap();
    assert_eq!((done.inputs, done.outputs), (4, 2));
    let n = FRAMES as usize;
    let steady = vec![0.5_f32; n];
    let mut impulse = vec![0.0_f32; n];
    impulse[10] = 1.0;
    let block = host
        .process(
            FRAMES,
            vec![steady.clone(), steady.clone(), impulse, vec![0.0; n]],
            vec![],
            vec![],
            SLOW,
        )
        .unwrap();
    let left = &block.output[0];
    // The effect follows the key's peak: nothing before the loud sample is
    // touched, and from it the effect is held silent (the key falls away over
    // a tenth of a second, far longer than this block).
    assert_eq!(left[10], 0.0, "the loud sidechain sample is silenced");
    let kept = left[9];
    assert!(kept != 0.0);
    assert!(
        left.iter()
            .enumerate()
            .all(|(i, &s)| if i >= 10 { s == 0.0 } else { s == kept }),
        "the samples before it are untouched and the rest held down: {left:?}"
    );
}

#[test]
fn an_effect_with_its_sidechain_off_is_not_ducked() {
    let mut host = loaded_effect("sidechain");
    let done = host
        .configure_buses(48_000.0, 512, (2, 2), Aux::default(), SLOW)
        .unwrap();
    assert_eq!(done.inputs, 2);
    let n = FRAMES as usize;
    let steady = vec![0.5_f32; n];
    let block = host
        .process(FRAMES, vec![steady.clone(), steady], vec![], vec![], SLOW)
        .unwrap();
    assert!(block.output[0].iter().all(|&s| s == block.output[0][0]));
}

#[test]
fn a_plugin_that_changes_its_outputs_asks_to_be_set_up_again_and_lists_the_new_ones() {
    let mut host = loaded_instrument("multi_out=2,bus_change");
    host.configure_buses(48_000.0, 512, (0, 2), Aux::default(), SLOW)
        .unwrap();
    assert_eq!(host.buses(SLOW).unwrap().1.len(), 2);
    host.set_param(TRIGGER, 1.0, SLOW).unwrap();
    let block = host
        .process(FRAMES, vec![], vec![], vec![], SLOW)
        .expect("process");
    assert!(
        block.restart & RESTART_RECONFIGURE != 0,
        "the plugin asked for a restart: {:#x}",
        block.restart
    );
    assert_eq!(
        host.buses(SLOW).unwrap().1.len(),
        3,
        "and now has a third output"
    );
    // Set up again with the new bus on.
    let done = host
        .configure_buses(
            48_000.0,
            512,
            (0, 2),
            Aux {
                inputs: 0,
                outputs: 0b100,
            },
            SLOW,
        )
        .unwrap();
    assert_eq!(done.outputs, 4);
}
