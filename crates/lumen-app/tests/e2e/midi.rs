//! MIDI devices (M5 slice 0): ports appear and disappear with hot-plug in
//! the settings' MIDI page, the switches open them, are saved, and come
//! back with the device; a port that will not open says so.

use std::time::{Duration, Instant};

use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;

use super::{Lumen, PATIENCE};

fn start(name: &str, fake: &FakeControl) -> Lumen {
    let fake = fake.clone();
    Lumen::start_with(
        lumen_project::testing::TempDir::new(&format!("e2e-{name}")),
        |l| l.with_midi(fake),
    )
}

fn open_midi_page(lumen: &mut Lumen) {
    lumen.open_midi_settings();
}

/// Runs frames until `done` holds, for as long as background work may take
/// (`PATIENCE`), never a count of frames: the MIDI worker answers on its own
/// thread, and on a loaded machine that can take far longer than a few
/// hundred frames.
#[track_caller]
fn until(lumen: &mut Lumen, what: &str, done: impl Fn(&Lumen) -> bool) {
    let started = Instant::now();
    loop {
        lumen.frames(1);
        if done(lumen) {
            return;
        }
        assert!(started.elapsed() < PATIENCE, "{what} never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs frames until the page shows (or no longer shows) `label`.
#[track_caller]
fn until_shown(lumen: &mut Lumen, label: &str, shown: bool) {
    let what = format!("{label:?} {}", if shown { "appearing" } else { "leaving" });
    until(lumen, &what, |l| l.shows(label) == shown);
}

/// Runs frames until the switch named `label` is (or is no longer) on. A
/// switch shows the MIDI worker's published state, which follows a click
/// only after a round trip to that thread: never read it straight after one.
#[track_caller]
fn until_on(lumen: &mut Lumen, label: &str, on: bool) {
    let what = format!("{label:?} turning {}", if on { "on" } else { "off" });
    until(lumen, &what, |l| l.is_on(label) == on);
}

/// Runs frames until `open` input ports are open on the fake system.
#[track_caller]
fn until_open_inputs(lumen: &mut Lumen, fake: &FakeControl, open: usize) {
    until(lumen, "the input ports opening", |_| {
        fake.open_inputs() == open
    });
}

#[test]
fn with_no_devices_the_page_says_so() {
    let fake = FakeControl::new();
    let mut lumen = start("midi-empty", &fake);
    open_midi_page(&mut lumen);
    assert!(lumen.shows("No MIDI inputs found"));
    assert!(lumen.shows("No MIDI outputs found"));
}

#[test]
fn devices_appear_and_leave_with_hot_plug() {
    let fake = FakeControl::new();
    let mut lumen = start("midi-hotplug", &fake);
    open_midi_page(&mut lumen);
    fake.plug(Direction::Input, "Keystation");
    fake.plug(Direction::Output, "Synth");
    until_shown(&mut lumen, "Keystation", true);
    assert!(lumen.shows("Synth"));
    assert!(!lumen.shows("No MIDI inputs found"));
    fake.unplug(Direction::Input, "Keystation");
    until_shown(&mut lumen, "Keystation", false);
    assert!(lumen.shows("No MIDI inputs found"));
}

#[test]
fn a_switch_opens_the_port_and_is_remembered_with_the_device() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Keystation");
    let mut lumen = start("midi-switch", &fake);
    open_midi_page(&mut lumen);
    until_shown(&mut lumen, "Track Keystation input", true);
    assert_eq!(fake.open_inputs(), 0, "off until switched on");
    lumen.click("Track Keystation input");
    until_on(&mut lumen, "Track Keystation input", true);
    until_open_inputs(&mut lumen, &fake, 1);
    assert_eq!(fake.open_inputs(), 1);
    assert_eq!(
        lumen.app().saved_midi_ports(),
        [("input".to_owned(), "Keystation".to_owned(), true, false)]
    );

    // Unplugged and plugged back: it opens again without another click.
    fake.unplug(Direction::Input, "Keystation");
    until_shown(&mut lumen, "Track Keystation input", false);
    assert_eq!(fake.open_inputs(), 0);
    fake.plug(Direction::Input, "Keystation");
    until_shown(&mut lumen, "Track Keystation input", true);
    until_open_inputs(&mut lumen, &fake, 1);
    assert_eq!(fake.open_inputs(), 1);
    until_on(&mut lumen, "Track Keystation input", true);
}

#[test]
fn remote_and_track_are_separate_switches() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Pad");
    let mut lumen = start("midi-remote", &fake);
    open_midi_page(&mut lumen);
    until_shown(&mut lumen, "Remote Pad input", true);
    lumen.click("Remote Pad input");
    until_on(&mut lumen, "Remote Pad input", true);
    assert!(!lumen.is_on("Track Pad input"));
    assert_eq!(
        lumen.app().saved_midi_ports(),
        [("input".to_owned(), "Pad".to_owned(), false, true)]
    );
    // The second click reads the switch as shown, so it must show on first.
    lumen.click("Remote Pad input");
    until_on(&mut lumen, "Remote Pad input", false);
    assert!(
        lumen.app().saved_midi_ports().is_empty(),
        "off everywhere: forgotten"
    );
}

#[test]
fn a_busy_port_says_it_could_not_open() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Busy");
    fake.refuse(Direction::Input, "Busy", true);
    let mut lumen = start("midi-busy", &fake);
    open_midi_page(&mut lumen);
    until_shown(&mut lumen, "Track Busy input", true);
    lumen.click("Track Busy input");
    until_shown(&mut lumen, "Could not open: Busy is in use", true);
}
