//! A real host process: the plugin loaded, configured, played and saved
//! through the protocol, as Lumen will drive it.

mod support;

use lumen_plugin_host::client::{ExitKind, HostError};
use lumen_plugin_host::protocol::{FailureKind, ParamQueue};
use lumen_test_plugin::params::{LATENCY, LEVEL, MAX_LATENCY};
use support::{SLOW, effect, instrument, note_off, note_on, plugin, spawn};

fn level(value: f64) -> Vec<ParamQueue> {
    vec![ParamQueue {
        id: LEVEL,
        points: vec![(0, value)],
    }]
}

#[test]
fn a_note_is_played_in_the_host_process() {
    let mut host = instrument("");
    let block = host
        .process(
            64,
            vec![],
            vec![note_on(10, 69), note_off(40, 69)],
            level(1.0),
            SLOW,
        )
        .expect("process");
    assert_eq!(block.status, 0);
    assert_eq!(block.repaired, 0);
    let out = &block.output[0];
    assert!(out[..10].iter().all(|&s| s == 0.0));
    assert!(out[10..40].iter().all(|&s| s == 1.0 || s == -1.0));
    assert!(out[40..].iter().all(|&s| s == 0.0));
    assert_eq!(host.stale(), 0);
    assert_eq!(host.quit(SLOW), ExitKind::Clean);
}

#[test]
fn the_load_reports_the_class_and_the_effect_reports_its_latency() {
    let mut host = spawn("");
    let class = host.load(plugin(), None, SLOW).expect("load");
    assert_eq!(class.name, "Deterministic Test Instrument");
    drop(host);

    let mut host = effect("");
    let mut impulse = vec![0.0_f32; 64];
    impulse[0] = 1.0;
    let block = host
        .process(
            64,
            vec![impulse.clone(), impulse],
            vec![],
            vec![ParamQueue {
                id: LATENCY,
                points: vec![(0, 24.0 / f64::from(MAX_LATENCY))],
            }],
            SLOW,
        )
        .expect("process");
    assert_eq!(block.latency, 24);
    assert_eq!(block.output[0][24], 1.0);
}

#[test]
fn state_saved_in_one_host_restores_in_another() {
    let mut first = instrument("");
    first.process(8, vec![], vec![], level(0.25), SLOW).unwrap();
    let state = first.state(SLOW).expect("state");
    drop(first);

    let mut second = instrument("");
    second.set_state(state.clone(), SLOW).expect("restore");
    assert_eq!(second.state(SLOW).expect("state"), state);
    let err = second.set_state(vec![1, 2, 3], SLOW).unwrap_err();
    assert!(matches!(
        err,
        HostError::Failed {
            kind: FailureKind::Plugin,
            ..
        }
    ));
    // The host survived the refusal.
    assert!(second.state(SLOW).is_ok());
}

#[test]
fn requests_out_of_order_are_answered_not_obeyed() {
    let mut host = spawn("");
    let err = host.process(8, vec![], vec![], vec![], SLOW).unwrap_err();
    assert!(matches!(
        err,
        HostError::Failed {
            kind: FailureKind::Sequence,
            ..
        }
    ));
    assert!(matches!(
        host.state(SLOW),
        Err(HostError::Failed {
            kind: FailureKind::Sequence,
            ..
        })
    ));
    host.load(plugin(), None, SLOW).unwrap();
    // Loading twice is refused; the first plugin is still there.
    assert!(host.load(plugin(), None, SLOW).is_err());
    assert!(host.parameters(SLOW).is_ok());
}

#[test]
fn a_host_does_not_outlive_its_handle() {
    let host = instrument("");
    let pid = host.id();
    drop(host);
    assert!(!support::exists(pid), "host {pid} is still running");
}

/// A host session runs at high priority, so a plugin's own threads are not
/// kept waiting behind ordinary programs while the audio waits on them.
#[cfg(windows)]
#[test]
fn a_host_session_runs_at_high_priority() {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetPriorityClass, HIGH_PRIORITY_CLASS, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // Loaded, so the session is past its start.
    let mut host = instrument("");
    // SAFETY: opening a process this test started, to read its priority.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, host.id()) };
    assert!(!handle.is_null(), "open the host process");
    // SAFETY: a live handle with query rights.
    let class = unsafe { GetPriorityClass(handle) };
    // SAFETY: the handle opened above, closed once.
    unsafe { CloseHandle(handle) };
    assert_eq!(class, HIGH_PRIORITY_CLASS);
    assert_eq!(host.quit(SLOW), ExitKind::Clean);
}
