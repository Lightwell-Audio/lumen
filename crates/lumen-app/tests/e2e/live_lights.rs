//! Playing live, end to end (M5 slice 2e): the computer keyboard is heard
//! on the selected MIDI track and on no other, the lights (the track's
//! header light and the chip's IN) flash with it, a fake input's notes
//! reach the track the same way, a track whose Monitor is Off never lights,
//! and an input that is switched on costs no frames while it is quiet.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::model::{Color, Id, Monitor, Project, Track, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

const HEARD: f32 = 0.005;
const KEYS: Id = Id(1);
const LEAD: Id = Id(11);

fn project() -> Project {
    project_of(
        "Live",
        vec![
            midi_track(1, "Keys", "poly", (0, 8), None, vec![], vec![]),
            midi_track(11, "Lead", "poly", (0, 8), None, vec![], vec![]),
            Track::new(Id(21), "Vox", TrackKind::Audio, Color(0x4F81BD)),
        ],
    )
}

fn launch(name: &str, fake: &FakeControl) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(), &path, &Default::default()).expect("save the test project");
    let fake = fake.clone();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path).with_midi(fake));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 3);
    // Rows grow in over a few frames.
    lumen.frames(30);
    lumen
}

fn select(lumen: &mut Lumen, name: &str) {
    lumen
        .harness
        .get_by_label_contains(&format!("Track {name},"))
        .click();
    lumen.frames(5);
}

fn key(lumen: &mut Lumen, key: Key, pressed: bool) {
    lumen.harness.event(Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    lumen.frames(2);
}

fn header_light(lumen: &Lumen, track: Id) -> f32 {
    lumen.app().header_light(&lumen.harness.ctx.clone(), track)
}

/// The loudest sample in the next stretch of audio.
fn loudest(lumen: &mut Lumen, settle: Duration) -> f32 {
    lumen
        .listen(settle, 3_000)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

#[test]
fn the_computer_keyboard_is_heard_on_the_selected_midi_track_and_lights_it() {
    let fake = FakeControl::new();
    let mut lumen = launch("live-keyboard", &fake);
    // Quiet first: let the synth's idle settle.
    lumen.listen(Duration::from_millis(200), 1_000);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
    assert!(lumen.app().computer_keyboard_on());
    select(&mut lumen, "Keys");
    assert_eq!(header_light(&lumen, KEYS), 0.0, "quiet at rest");

    // A is held: the track plays, and its light and the chip's flash.
    key(&mut lumen, Key::A, true);
    let (input, output) = lumen.app().midi_chip_lights();
    assert!(input > 0.5, "the chip's IN light flashed: {input}");
    assert_eq!(output, 0.0);
    assert!(
        header_light(&lumen, KEYS) > 0.5,
        "the selected track's light flashed"
    );
    assert_eq!(
        header_light(&lumen, LEAD),
        0.0,
        "a track that does not listen never lights"
    );
    assert!(
        loudest(&mut lumen, Duration::from_millis(50)) > HEARD,
        "the note is heard while held"
    );

    // Released: it rings out and goes quiet.
    key(&mut lumen, Key::A, false);
    assert!(lumen.app().computer_keyboard_held().is_empty());
    assert!(
        loudest(&mut lumen, Duration::from_millis(3_000)) < HEARD,
        "nothing is left sounding"
    );
    // The light fades.
    lumen.frames(60);
    assert_eq!(header_light(&lumen, KEYS), 0.0);

    // An audio track selected instead: nobody listens, so A is silent.
    select(&mut lumen, "Vox");
    key(&mut lumen, Key::A, true);
    assert_eq!(header_light(&lumen, KEYS), 0.0);
    assert_eq!(header_light(&lumen, LEAD), 0.0);
    assert!(
        loudest(&mut lumen, Duration::from_millis(50)) < HEARD,
        "an audio track does not play MIDI"
    );
    key(&mut lumen, Key::A, false);
}

/// Opens the settings' MIDI page and switches the input on for tracks.
fn switch_on_for_tracks(lumen: &mut Lumen, fake: &FakeControl, port: &str) {
    lumen.click("Lumen");
    lumen.click("Audio and MIDI settings\u{2026}");
    lumen.click("MIDI tab");
    let label = format!("Track {port} input");
    for _ in 0..400 {
        lumen.frames(1);
        if lumen.shows(&label) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    lumen.click(&label);
    for _ in 0..400 {
        lumen.frames(1);
        if fake.open_inputs() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fake.open_inputs(), 1);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(10);
}

#[test]
fn a_note_from_an_input_lights_the_listening_track_and_a_monitor_off_track_never_lights() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Keystation");
    let mut lumen = launch("live-input", &fake);
    switch_on_for_tracks(&mut lumen, &fake, "Keystation");

    // Keys is selected, Lead is not: only Keys listens.
    select(&mut lumen, "Keys");
    lumen.frames(10);
    assert!(fake.send_to_input("Keystation", 1_000, &[0x90, 60, 100]));
    lumen.frames(3);
    assert!(header_light(&lumen, KEYS) > 0.5, "Keys lit");
    assert_eq!(header_light(&lumen, LEAD), 0.0, "Lead does not listen");
    assert!(lumen.app().midi_chip_lights().0 > 0.5);
    fake.send_to_input("Keystation", 2_000, &[0x80, 60, 0]);
    lumen.frames(90);

    // Monitor Off on the selected track: it never lights.
    lumen.app_mut().set_monitor_of_selected(Monitor::Off);
    lumen.frames(10);
    assert!(fake.send_to_input("Keystation", 3_000, &[0x90, 62, 100]));
    lumen.frames(3);
    assert_eq!(header_light(&lumen, KEYS), 0.0, "Monitor Off: never lights");
    assert!(
        lumen.app().midi_chip_lights().0 > 0.5,
        "the chip still shows the traffic"
    );
    fake.send_to_input("Keystation", 4_000, &[0x80, 62, 0]);
}

#[test]
fn an_input_that_is_on_and_quiet_asks_for_no_frames() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Keystation");
    let mut lumen = launch("live-idle", &fake);
    switch_on_for_tracks(&mut lumen, &fake, "Keystation");
    select(&mut lumen, "Keys");
    // Let every glide and fade finish.
    lumen.frames(120);
    for _ in 0..30 {
        lumen.frames(1);
        let causes = lumen.harness.ctx.repaint_causes();
        let chip: Vec<_> = causes
            .iter()
            .filter(|c| c.file.ends_with("midi_chip.rs") || c.file.ends_with("listen_light.rs"))
            .collect();
        assert!(
            chip.is_empty(),
            "the chip and the header lights poll nothing: {chip:?}"
        );
    }
    // A note wakes it: the flash is drawn without the test stepping extra.
    assert!(fake.send_to_input("Keystation", 1_000, &[0x90, 60, 100]));
    let started = std::time::Instant::now();
    let mut woke = false;
    while started.elapsed() < Duration::from_secs(2) {
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(5));
        if lumen.app().midi_chip_lights().0 > 0.5 {
            woke = true;
            break;
        }
    }
    assert!(woke, "the note reached the chip");
}

#[test]
fn a_light_stays_lit_for_as_long_as_a_key_is_held_then_fades() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Keystation");
    let mut lumen = launch("live-held", &fake);
    switch_on_for_tracks(&mut lumen, &fake, "Keystation");
    select(&mut lumen, "Keys");
    lumen.frames(10);

    assert!(fake.send_to_input("Keystation", 1_000, &[0x90, 60, 100]));
    // A flash lasts well under a second: two seconds of frames later the
    // key is still down, and so are the lights.
    lumen.frames(3);
    lumen.frames(120);
    assert!(
        header_light(&lumen, KEYS) > 0.99,
        "the track's light stays lit"
    );
    assert!(
        lumen.app().midi_chip_lights().0 > 0.99,
        "the chip's IN light stays lit"
    );

    // Released: they fade from full, and go out.
    assert!(fake.send_to_input("Keystation", 2_000, &[0x80, 60, 0]));
    let started = std::time::Instant::now();
    while header_light(&lumen, KEYS) > 0.99 && started.elapsed() < Duration::from_secs(2) {
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    lumen.frames(90);
    assert_eq!(header_light(&lumen, KEYS), 0.0);
    assert_eq!(lumen.app().midi_chip_lights().0, 0.0);
}

#[test]
fn the_computer_keyboard_lights_stay_lit_while_a_key_is_held() {
    let fake = FakeControl::new();
    let mut lumen = launch("live-held-keyboard", &fake);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
    select(&mut lumen, "Keys");
    key(&mut lumen, Key::A, true);
    lumen.frames(120);
    assert!(header_light(&lumen, KEYS) > 0.99, "lit while A is down");
    assert!(lumen.app().midi_chip_lights().0 > 0.99);
    key(&mut lumen, Key::A, false);
    lumen.frames(90);
    assert_eq!(header_light(&lumen, KEYS), 0.0);
    assert_eq!(lumen.app().midi_chip_lights().0, 0.0);
}
