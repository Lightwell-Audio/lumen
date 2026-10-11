//! The MIDI chip in the transport bar (M5 slice 2d): its lights flash with
//! each port's own traffic (not with the clock), its popover switches ports
//! the way the settings page does, remembered ports that are away keep
//! their switches, and "MIDI settings…" opens the MIDI page.

use std::time::Duration;

use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;

use super::Lumen;

const NOTE_ON: [u8; 3] = [0x90, 60, 100];

fn start(name: &str, fake: &FakeControl) -> Lumen {
    let fake = fake.clone();
    Lumen::start_with(
        lumen_project::testing::TempDir::new(&format!("e2e-{name}")),
        |l| l.with_midi(fake),
    )
}

/// Runs frames until the chip carries `name`.
#[track_caller]
fn until_named(lumen: &mut Lumen, name: &str) {
    for _ in 0..400 {
        lumen.frames(1);
        if lumen.shows(name) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the chip never said {name:?}");
}

/// Plugs `Keystation` in and `Synth` out, opens the popover and switches
/// both on for tracks; waits for the ports to open.
fn with_ports_on(name: &str) -> (Lumen, FakeControl) {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Keystation");
    fake.plug(Direction::Output, "Synth");
    let mut lumen = start(name, &fake);
    until_named(&mut lumen, "MIDI: 0 inputs, 0 outputs");
    lumen.click("MIDI: 0 inputs, 0 outputs");
    until_named(&mut lumen, "Track Keystation input in the MIDI panel");
    lumen.click("Track Keystation input in the MIDI panel");
    lumen.click("Track Synth output in the MIDI panel");
    for _ in 0..400 {
        if fake.open_inputs() == 1 && fake.opens(Direction::Output, "Synth") == 1 {
            break;
        }
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fake.open_inputs(), 1);
    (lumen, fake)
}

#[test]
fn the_chip_names_how_many_ports_are_on() {
    let (mut lumen, _fake) = with_ports_on("chip-name");
    until_named(&mut lumen, "MIDI: 1 input, 1 output");
}

#[test]
fn a_note_flashes_the_in_light_and_its_row_and_the_clock_does_not() {
    let (mut lumen, fake) = with_ports_on("chip-flash");
    lumen.frames(5);
    assert_eq!(lumen.app().midi_chip_lights().0, 0.0, "quiet at rest");

    // The clock, active sensing: not playing.
    assert!(fake.send_to_input("Keystation", 1, &[0xF8]));
    assert!(fake.send_to_input("Keystation", 2, &[0xFE]));
    lumen.frames(4);
    assert_eq!(lumen.app().midi_chip_lights().0, 0.0);
    assert_eq!(
        lumen.app().midi_row_light(Direction::Input, "Keystation"),
        Some(0.0)
    );

    assert!(fake.send_to_input("Keystation", 3, &NOTE_ON));
    lumen.frames(2);
    let (input, output) = lumen.app().midi_chip_lights();
    assert!(input > 0.5, "the IN light flashed: {input}");
    assert_eq!(output, 0.0, "the OUT light did not");
    let row = lumen
        .app()
        .midi_row_light(Direction::Input, "Keystation")
        .expect("the popover shows the port");
    assert!(row > 0.5, "the port's own light flashed: {row}");
    assert_eq!(
        lumen.app().midi_row_light(Direction::Output, "Synth"),
        Some(0.0)
    );

    // It fades.
    lumen.frames(120);
    assert!(lumen.app().midi_chip_lights().0 < input / 2.0);
}

#[test]
fn a_pill_changes_the_saved_switches_as_the_settings_page_does() {
    let (mut lumen, _fake) = with_ports_on("chip-pills");
    assert_eq!(
        lumen.app().saved_midi_ports(),
        [
            ("input".to_owned(), "Keystation".to_owned(), true, false),
            ("output".to_owned(), "Synth".to_owned(), true, false),
        ]
    );
    assert!(lumen.is_on("Track Keystation input in the MIDI panel"));
    lumen.click("Remote Keystation input in the MIDI panel");
    assert_eq!(
        lumen.app().saved_midi_ports(),
        [
            ("input".to_owned(), "Keystation".to_owned(), true, true),
            ("output".to_owned(), "Synth".to_owned(), true, false),
        ]
    );
    // Off everywhere: forgotten, as on the settings page.
    lumen.click("Remote Keystation input in the MIDI panel");
    lumen.click("Track Keystation input in the MIDI panel");
    assert_eq!(
        lumen.app().saved_midi_ports(),
        [("output".to_owned(), "Synth".to_owned(), true, false)]
    );
}

#[test]
fn a_remembered_port_that_is_away_is_listed_with_its_switches() {
    let (mut lumen, fake) = with_ports_on("chip-away");
    fake.unplug(Direction::Input, "Keystation");
    for _ in 0..400 {
        lumen.frames(1);
        if lumen.shows("not connected") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(lumen.shows("not connected"));
    assert!(lumen.shows("Keystation"));
    assert!(lumen.is_on("Track Keystation input in the MIDI panel"));
    assert!(lumen.shows("MIDI: 0 inputs, 1 output"));
    // Its switches still work.
    lumen.click("Remote Keystation input in the MIDI panel");
    assert!(lumen.app().saved_midi_ports().contains(&(
        "input".to_owned(),
        "Keystation".to_owned(),
        true,
        true
    )));
}

#[test]
fn midi_settings_opens_the_window_on_the_midi_page() {
    let (mut lumen, _fake) = with_ports_on("chip-settings");
    assert!(!lumen.shows("No MIDI inputs found"));
    lumen.click("MIDI settings\u{2026}");
    lumen.frames(5);
    assert!(lumen.shows("MIDI tab"));
    assert!(lumen.shows("Track Keystation input"), "the MIDI page");
    assert!(!lumen.shows("Track Keystation input in the MIDI panel"));
}

#[test]
fn the_computer_keyboard_flashes_the_in_light() {
    let (mut lumen, _fake) = with_ports_on("chip-keys");
    lumen.frames(3);
    assert_eq!(lumen.app().midi_chip_lights().0, 0.0);
    // No engine is listening yet in some runs: wait until a key goes.
    let msg = lumen_core::midi::Msg::note_on(0, 60, 100);
    lumen.wait_for("the keyboard to send", |_| true);
    let mut sent = false;
    for _ in 0..200 {
        lumen.frames(1);
        if lumen.app_mut().play_live(msg) {
            sent = true;
            break;
        }
    }
    assert!(sent);
    lumen.frames(2);
    assert!(lumen.app().midi_chip_lights().0 > 0.5);
    assert!(lumen.shows("Computer keyboard"));
}

#[test]
fn without_midi_the_popover_says_why() {
    let mut lumen = Lumen::start_with(lumen_project::testing::TempDir::new("e2e-chip-off"), |l| {
        l.with_midi_off()
    });
    lumen.frames(3);
    assert!(lumen.shows("MIDI: unavailable. MIDI is switched off for this run"));
    lumen.click("MIDI: unavailable. MIDI is switched off for this run");
    assert!(lumen.shows("MIDI is switched off for this run"));
}
