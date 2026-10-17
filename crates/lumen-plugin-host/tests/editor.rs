//! The plugin's editor window through a real host process. Windows has real
//! windows; here the host's stand-in (`LUMEN_HOST_HEADLESS_EDITOR`) records
//! where a window would be, so the requests, the remembered place, a window
//! the user closes and a main thread that stops answering are all exercised.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use lumen_plugin_host::client::{HostError, HostProcess};
use lumen_plugin_host::editor::HEADLESS_VAR;
use lumen_plugin_host::protocol::{EditorBounds, FailureKind};
use support::{FAULT, HOST, SLOW, plugin};

fn host_with(headless: &str) -> HostProcess {
    let mut host = HostProcess::spawn(Path::new(HOST), &[(HEADLESS_VAR, headless), (FAULT, "")])
        .expect("start the host");
    host.load(plugin(), None, SLOW).expect("load");
    host.configure(48_000.0, 512, 0, 2, SLOW)
        .expect("configure");
    host
}

const AT: EditorBounds = EditorBounds {
    x: -1200,
    y: 80,
    width: 640,
    height: 480,
    dpi: 144,
};

#[test]
fn an_editor_opens_where_it_was_and_closes() {
    let mut host = host_with("on");
    assert!(!host.editor_status(SLOW).unwrap().open);
    let opened = host.open_editor(Some(AT), SLOW).unwrap();
    assert!(opened.open);
    assert_eq!(opened.bounds, Some(AT));
    assert_eq!(host.editor_status(SLOW).unwrap(), opened);
    // The control path goes on working while the window is up.
    assert!(host.param_text(0, 0.5, SLOW).is_ok());
    let closed = host.close_editor(SLOW).unwrap();
    assert!(!closed.open);
    assert_eq!(closed.bounds, Some(AT), "where it was, to be remembered");
    // And it opens again, now at the system's place.
    assert!(host.open_editor(None, SLOW).unwrap().open);
}

#[test]
fn a_second_editor_is_refused_with_a_reason() {
    let mut host = host_with("on");
    host.open_editor(None, SLOW).unwrap();
    match host.open_editor(None, SLOW) {
        Err(HostError::Failed { kind, message }) => {
            assert_eq!(kind, FailureKind::NoEditor);
            assert!(message.contains("already open"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_window_the_user_closed_reads_as_closed() {
    let mut host = host_with("on,close=60");
    assert!(host.open_editor(None, SLOW).unwrap().open);
    let end = Instant::now() + Duration::from_secs(5);
    while host.editor_status(SLOW).unwrap().open {
        assert!(Instant::now() < end, "the window never closed");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(windows))]
#[test]
fn without_windows_the_host_says_there_is_no_editor_here() {
    let mut host = HostProcess::spawn(Path::new(HOST), &[(FAULT, "")]).expect("start the host");
    host.load(plugin(), None, SLOW).expect("load");
    match host.open_editor(None, SLOW) {
        Err(HostError::Failed { kind, message }) => {
            assert_eq!(kind, FailureKind::NoEditor);
            assert!(message.contains("platform"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!host.editor_status(SLOW).unwrap().open);
}

#[test]
fn an_editor_that_stops_answering_is_reported_not_waited_for_and_recovers() {
    let mut host = host_with("on,hang=1500");
    let begun = Instant::now();
    match host.open_editor(None, SLOW) {
        Err(HostError::Failed { kind, .. }) => assert_eq!(kind, FailureKind::Unresponsive),
        other => panic!("{other:?}"),
    }
    let waited = begun.elapsed();
    assert!(
        waited < Duration::from_millis(1400),
        "answered after {waited:?}: the host waited for the window"
    );
    // While the main thread is stuck every request says so at once.
    let began = Instant::now();
    match host.editor_status(SLOW) {
        Err(HostError::Failed { kind, .. }) => assert_eq!(kind, FailureKind::Unresponsive),
        other => panic!("{other:?}"),
    }
    assert!(began.elapsed() < Duration::from_millis(300));
    // Then the window code returns, the late answer is dropped, and the
    // host answers again; the window it was making is there.
    let end = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match host.editor_status(SLOW) {
            Ok(status) => break status,
            Err(HostError::Failed {
                kind: FailureKind::Unresponsive,
                ..
            }) => {
                assert!(Instant::now() < end, "still not responding");
                std::thread::sleep(Duration::from_millis(50));
            }
            other => panic!("{other:?}"),
        }
    };
    assert!(status.open);
    host.close_editor(SLOW).unwrap();
}

#[test]
fn a_host_whose_window_is_stuck_still_quits_when_asked() {
    let mut host = host_with("on,hang=30000");
    let _ = host.open_editor(None, SLOW);
    let begun = Instant::now();
    host.quit(Duration::from_secs(5));
    assert!(
        begun.elapsed() < Duration::from_secs(4),
        "the quit waited for the stuck window"
    );
}
