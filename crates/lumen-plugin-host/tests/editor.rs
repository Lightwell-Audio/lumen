//! The plugin's editor window through a real host process. Windows has real
//! windows; here the host's stand-in (`LUMEN_HOST_HEADLESS_EDITOR`) records
//! where a window would be, so the requests, the remembered place, a window
//! the user closes and a main thread that stops answering are all exercised.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use lumen_plugin_host::client::{HostError, HostProcess};
use lumen_plugin_host::editor::HEADLESS_VAR;
use lumen_plugin_host::protocol::{EditorBounds, FailureKind, Response};
use lumen_plugin_host::server::EDITOR_WAIT_VAR;
use support::{FAULT, HOST, SLOW, plugin};

/// A host with the stand-in window, which says a window is not responding
/// after 600 ms rather than the system's five seconds.
fn host_with(headless: &str) -> HostProcess {
    let mut host = HostProcess::spawn(
        Path::new(HOST),
        &[
            (HEADLESS_VAR, headless),
            (FAULT, ""),
            (EDITOR_WAIT_VAR, "600"),
        ],
    )
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
fn a_hidden_editor_stays_open_and_comes_back_where_it_was() {
    let mut host = host_with("on");
    let opened = host.open_editor(Some(AT), SLOW).unwrap();
    assert!(opened.open && opened.visible);
    let hidden = host.show_editor(false, SLOW).unwrap();
    assert!(hidden.open, "hiding is not closing");
    assert!(!hidden.visible);
    assert_eq!(hidden.bounds, Some(AT));
    // A second editor is still refused: the window is there.
    assert!(host.open_editor(None, SLOW).is_err());
    let shown = host.show_editor(true, SLOW).unwrap();
    assert!(shown.open && shown.visible);
    assert_eq!(shown.bounds, Some(AT));
    // With no window there is nothing to show.
    host.close_editor(SLOW).unwrap();
    let none = host.show_editor(true, SLOW).unwrap();
    assert!(!none.open && !none.visible);
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

/// MT-VST-4 run 1 (RP-5): the plugin took more than the window's wait to open
/// its editor, and setting it up again in the meantime was refused as "not
/// responding". Only window requests are answered that way; the rest wait
/// their turn and are done.
#[test]
fn while_the_window_is_stuck_other_requests_wait_and_are_answered() {
    let mut host = host_with("on,hang=1500");
    match host.open_editor(None, SLOW) {
        Err(HostError::Failed { kind, .. }) => assert_eq!(kind, FailureKind::Unresponsive),
        other => panic!("{other:?}"),
    }
    let (latency, _) = host
        .configure(44_100.0, 256, 0, 2, SLOW)
        .expect("setting the plugin up waits for the window, not refused");
    assert_eq!(latency, 0);
    assert!(host.param_text(0, 0.5, SLOW).is_ok());
    assert!(host.editor_status(SLOW).unwrap().open);
}

/// MT-VST-4 run 1 (RP-7): an answer that came after its request was given up on
/// was read as the answer to the next one ("expected Configured"). The host
/// answers in order, so the late answer is dropped.
#[test]
fn a_late_answer_is_never_taken_for_the_next_one() {
    let mut host = host_with("on,hang=800");
    // Given up on long before the window opens.
    assert_eq!(
        host.open_editor(None, Duration::from_millis(100)),
        Err(HostError::Timeout)
    );
    let text = host
        .param_text(0, 0.5, SLOW)
        .expect("the window's late answer is not this one's");
    assert!(!text.is_empty());
    let before = host.stale();
    host.configure(48_000.0, 512, 0, 2, SLOW)
        .expect("configure after a late answer");
    assert_eq!(host.stale(), before, "one late answer, dropped once");
    assert!(before >= 1);
}

/// The owner's choice (M6 slice 4): with the computer keyboard on, its keys
/// pressed in a plugin's window are taken and told to Lumen.
#[test]
fn keys_asked_for_are_told_as_they_are_pressed_in_the_window() {
    let mut host = host_with("on,press=75");
    let (tx, rx) = std::sync::mpsc::channel();
    host.on_notice(Box::new(move |news| {
        let _ = tx.send(news);
    }));
    host.open_editor(None, SLOW).unwrap();
    // Not asked for: the plugin keeps it.
    host.capture_keys(vec![b'A'], SLOW).unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    host.capture_keys(vec![b'A', b'K'], SLOW).unwrap();
    let got: Vec<Response> = (0..2)
        .map(|_| rx.recv_timeout(SLOW).expect("the key"))
        .collect();
    assert_eq!(
        got,
        [
            Response::Key {
                code: b'K',
                down: true
            },
            Response::Key {
                code: b'K',
                down: false
            }
        ]
    );
    // The news never stands for an answer.
    assert!(host.param_text(0, 0.5, SLOW).is_ok());
    host.capture_keys(Vec::new(), SLOW).unwrap();
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
