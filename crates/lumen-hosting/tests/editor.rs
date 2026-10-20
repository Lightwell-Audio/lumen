//! The plugin's editor window through the manager: what it says when the
//! window opens, moves, is closed by the user, stops answering and is ended.
//! The host's stand-in window (`LUMEN_HOST_HEADLESS_EDITOR`) stands in for a
//! window system.

mod support;

use std::time::Duration;

use lumen_hosting::{EditorBounds, EditorEvent, Happened, Manager};
use support::{instrument, linked, wait_for};

fn manager() -> Manager {
    Manager::new(|| {})
}

const HEADLESS: &str = "LUMEN_HOST_HEADLESS_EDITOR";
/// The host says a window is not responding after this many milliseconds
/// rather than the system's five seconds, so the tests stay quick.
const WAIT_MS: &str = "600";
const AT: EditorBounds = EditorBounds {
    x: 40,
    y: 60,
    width: 700,
    height: 500,
    dpi: 96,
};

/// A manager with one instrument whose host has the stand-in window set as
/// `headless` says, linked.
fn with_window(headless: &str) -> Manager {
    let mut manager = manager();
    let mut start = instrument("");
    start.env.push((HEADLESS.into(), headless.into()));
    start.env.push((
        lumen_plugin_host::server::EDITOR_WAIT_VAR.into(),
        WAIT_MS.into(),
    ));
    manager.want(1, start).unwrap();
    let _linked = linked(&mut manager, 1);
    manager
}

/// The next `n` editor events, in order. (Events that arrive together are
/// kept: picking one out of a batch with `wait_for` would drop the rest.)
fn editor_events(manager: &mut Manager, n: usize) -> Vec<EditorEvent> {
    let mut seen = Vec::new();
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while seen.len() < n {
        for event in manager.poll() {
            if let Happened::Editor(e) = event.happened {
                seen.push(e);
            }
        }
        assert!(
            std::time::Instant::now() < end,
            "only {seen:?} within 10 s, wanted {n}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    seen
}

fn editor_event(manager: &mut Manager) -> EditorEvent {
    editor_events(manager, 1).remove(0)
}

#[test]
fn an_editor_opens_where_it_was_and_closes_with_its_place() {
    let mut manager = with_window("on");
    manager.open_editor(1, Some(AT));
    assert_eq!(editor_event(&mut manager), EditorEvent::Opened(AT));
    manager.close_editor(1);
    assert_eq!(editor_event(&mut manager), EditorEvent::Closed(Some(AT)));
}

#[test]
fn a_window_the_user_closes_is_reported_closed() {
    let mut manager = with_window("on,close=150");
    manager.open_editor(1, None);
    assert!(matches!(editor_event(&mut manager), EditorEvent::Opened(_)));
    assert!(matches!(
        editor_event(&mut manager),
        EditorEvent::Closed(Some(_))
    ));
}

#[test]
fn a_plugin_without_an_editor_is_refused_with_a_reason() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let _linked = linked(&mut manager, 1);
    manager.open_editor(1, None);
    match editor_event(&mut manager) {
        EditorEvent::Refused(why) => {
            if cfg!(any(windows, target_os = "macos")) {
                assert!(why.contains("no editor"), "{why}");
            } else {
                assert!(why.contains("platform"), "{why}");
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_window_that_stops_answering_is_flagged_then_answers_again() {
    let mut manager = with_window("on,hang=1500");
    manager.open_editor(1, None);
    let seen = editor_events(&mut manager, 3);
    assert_eq!(seen[0], EditorEvent::NotResponding);
    assert_eq!(seen[1], EditorEvent::Responding);
    assert!(matches!(seen[2], EditorEvent::Opened(_)), "{seen:?}");
    manager.close_editor(1);
    assert!(matches!(editor_event(&mut manager), EditorEvent::Closed(_)));
}

/// The owner's choice (M6 slice 4): the computer keyboard's keys pressed in a
/// plugin's window reach the app.
#[test]
fn keys_taken_from_the_window_reach_the_app_down_then_up() {
    let mut manager = with_window("on,press=75");
    manager.open_editor(1, None);
    assert!(matches!(editor_event(&mut manager), EditorEvent::Opened(_)));
    manager.capture_keys(vec![b'A', b'K']);
    let mut keys = Vec::new();
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while keys.len() < 2 {
        for event in manager.poll() {
            if let Happened::Key { code, down } = event.happened {
                keys.push((event.device, code, down));
            }
        }
        assert!(std::time::Instant::now() < end, "only {keys:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(keys, [(1, b'K', true), (1, b'K', false)]);
}

#[test]
fn a_plugin_whose_window_is_stuck_can_be_ended_and_then_reads_as_failed() {
    let mut manager = with_window("on,hang=60000");
    manager.open_editor(1, None);
    assert_eq!(editor_event(&mut manager), EditorEvent::NotResponding);
    manager.end(1);
    let failure = wait_for(
        &mut manager,
        Duration::from_secs(10),
        |_, happened| match happened {
            Happened::Failed(f) => Ok(f),
            other => Err(other),
        },
    );
    assert!(failure.to_string().contains("ended"), "{failure}");
}
