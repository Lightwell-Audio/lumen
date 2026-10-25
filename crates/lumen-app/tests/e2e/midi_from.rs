//! Which tracks listen (M5 slice 2b, format v13): a MIDI track plays what is
//! played live when its Monitor is In, or Auto while it is armed or
//! selected; never when Off. MIDI From chooses the inputs (all, the
//! computer keyboard, one port and channel), and the track menu, the mixer
//! strip and the palette set it and Monitor, each one undo step. The
//! settings survive saving and opening.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::midi::Msg;
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::model::{Color, FromSource, Id, MidiFrom, Monitor, Project, Track, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

/// Two MIDI tracks (no notes) and an audio track.
fn project() -> Project {
    project_of(
        "Listening",
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

fn track<'a>(lumen: &'a Lumen, name: &str) -> &'a Track {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.name == name)
        .unwrap()
}

/// The loudest sample heard while a note is held, for `play`, which
/// starts it. The note is released and its tail let go before returning.
fn level_of(lumen: &mut Lumen, play: impl FnOnce(&mut Lumen)) -> f32 {
    // Let anything left ring out first.
    lumen.listen(Duration::from_millis(600), 4_000);
    play(lumen);
    let heard = lumen.listen(Duration::from_millis(150), 3_000);
    let loudest = heard
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
    assert!(lumen.app_mut().play_live(Msg::note_off(0, 60)));
    loudest
}

fn keyboard_note(lumen: &mut Lumen) -> f32 {
    level_of(lumen, |l| {
        assert!(l.app_mut().play_live(Msg::note_on(0, 60, 100)));
    })
}

const HEARD: f32 = 0.005;

/// The level of a note that is expected to be heard: up to three tries, the
/// first that is heard counts. These scenarios are about which tracks listen,
/// not about every note arriving; on a loaded macOS runner a first note was
/// once not heard (not reproduced on Linux, even with every core busy). A
/// silent expectation is measured once: a retry could only make it pass.
fn eventually(lumen: &mut Lumen, mut measure: impl FnMut(&mut Lumen) -> f32) -> f32 {
    let mut loudest = 0.0f32;
    for _ in 0..3 {
        loudest = loudest.max(measure(lumen));
        if loudest > HEARD {
            break;
        }
    }
    loudest
}

/// Opens the settings' MIDI page and switches the input on for tracks.
pub(super) fn switch_on_for_tracks(lumen: &mut Lumen, fake: &FakeControl, port: &str) {
    lumen.open_midi_settings();
    let label = format!("Track {port} input");
    lumen.run_until(&format!("{label:?} showing"), |l| l.shows(&label));
    lumen.click(&label);
    lumen.run_until("the input opening", |_| fake.open_inputs() == 1);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(10);
}

fn input_note(lumen: &mut Lumen, fake: &FakeControl, port: &str, channel: u8) -> f32 {
    level_of(lumen, |_| {
        let on = [0x90 | channel, 60, 100];
        assert!(fake.send_to_input(port, 1_000, &on));
    })
}

fn input_off(fake: &FakeControl, port: &str, channel: u8) {
    fake.send_to_input(port, 2_000, &[0x80 | channel, 60, 0]);
}

#[test]
fn a_selected_midi_track_plays_what_is_played_and_nothing_selected_is_silent() {
    let fake = FakeControl::new();
    let mut lumen = launch("from-selected", &fake);
    // Nothing is selected: nobody listens.
    assert!(keyboard_note(&mut lumen) < HEARD);
    select(&mut lumen, "Keys");
    assert!(
        eventually(&mut lumen, keyboard_note) > HEARD,
        "the selected track plays"
    );
    // An audio track selected instead: silent.
    select(&mut lumen, "Vox");
    assert!(keyboard_note(&mut lumen) < HEARD);
}

#[test]
fn monitor_in_always_listens_and_off_never_does() {
    let fake = FakeControl::new();
    let mut lumen = launch("from-monitor", &fake);
    select(&mut lumen, "Keys");
    super::midi_play::palette(&mut lumen, "Monitor: Off");
    assert_eq!(track(&lumen, "Keys").monitor, Monitor::Off);
    assert!(
        keyboard_note(&mut lumen) < HEARD,
        "Off is silent even selected"
    );
    super::midi_play::palette(&mut lumen, "Monitor: In");
    assert_eq!(track(&lumen, "Keys").monitor, Monitor::In);
    select(&mut lumen, "Vox");
    assert!(
        eventually(&mut lumen, keyboard_note) > HEARD,
        "In plays with the track not selected"
    );
    // One undo step each.
    assert_eq!(lumen.app().undo_label(), Some("Monitor: In"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(track(&lumen, "Keys").monitor, Monitor::Off);
    select(&mut lumen, "Keys");
    super::midi_play::palette(&mut lumen, "Monitor: Auto");
    assert_eq!(track(&lumen, "Keys").monitor, Monitor::Auto);
}

#[test]
fn arming_a_second_track_layers_it() {
    let fake = FakeControl::new();
    let mut lumen = launch("from-layer", &fake);
    select(&mut lumen, "Keys");
    let one = keyboard_note(&mut lumen);
    assert!(one > HEARD);
    lumen.click("Arm Lead");
    assert!(track(&lumen, "Lead").armed);
    // Armed, it listens while Keys is still selected: twice the sound.
    select(&mut lumen, "Keys");
    let two = keyboard_note(&mut lumen);
    assert!(two > one * 1.5, "layered: {one} then {two}");
    // Selecting the audio track leaves only the armed one.
    select(&mut lumen, "Vox");
    let armed = eventually(&mut lumen, keyboard_note);
    assert!(armed > HEARD && armed < two * 0.75, "{armed} vs {two}");
}

#[test]
fn from_chooses_between_the_keyboard_and_a_port() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Pad");
    let mut lumen = launch("from-ports", &fake);
    switch_on_for_tracks(&mut lumen, &fake, "Pad");
    select(&mut lumen, "Keys");
    // All inputs: the keyboard and the port both play.
    assert!(eventually(&mut lumen, keyboard_note) > HEARD);
    assert!(eventually(&mut lumen, |l| input_note(l, &fake, "Pad", 0)) > HEARD);
    input_off(&fake, "Pad", 0);

    // The computer keyboard only: the port is ignored.
    super::midi_play::palette(&mut lumen, "MIDI From: Computer keyboard");
    assert_eq!(track(&lumen, "Keys").midi_from.source, FromSource::Keyboard);
    assert!(eventually(&mut lumen, keyboard_note) > HEARD);
    assert!(input_note(&mut lumen, &fake, "Pad", 0) < HEARD);
    input_off(&fake, "Pad", 0);

    // The port on channel 2 only.
    lumen.app_mut().set_midi_from(MidiFrom {
        source: FromSource::Port("Pad".into()),
        channel: Some(1),
    });
    lumen.frames(5);
    assert!(keyboard_note(&mut lumen) < HEARD);
    assert!(
        input_note(&mut lumen, &fake, "Pad", 0) < HEARD,
        "wrong channel"
    );
    input_off(&fake, "Pad", 0);
    assert!(eventually(&mut lumen, |l| input_note(l, &fake, "Pad", 1)) > HEARD);
    input_off(&fake, "Pad", 1);
}

#[test]
fn the_track_menu_sets_midi_from_and_monitor_and_undo_restores_them() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Pad");
    let mut lumen = launch("from-menu", &fake);
    let header = lumen.harness.get_by_label_contains("Track Keys,").rect();

    lumen.right_click(header.center());
    lumen.click("MIDI From");
    lumen.click("Computer keyboard");
    assert_eq!(track(&lumen, "Keys").midi_from.source, FromSource::Keyboard);
    assert_eq!(
        lumen.app().undo_label(),
        Some("MIDI From: Computer keyboard")
    );

    lumen.right_click(header.center());
    lumen.click("MIDI From");
    lumen.click("Pad");
    lumen.click("Channel 2");
    assert_eq!(
        track(&lumen, "Keys").midi_from,
        MidiFrom {
            source: FromSource::Port("Pad".into()),
            channel: Some(1)
        }
    );
    assert_eq!(lumen.app().undo_label(), Some("MIDI From: Pad \u{b7} Ch 2"));

    lumen.right_click(header.center());
    lumen.click("In");
    assert_eq!(track(&lumen, "Keys").monitor, Monitor::In);
    assert_eq!(lumen.app().undo_label(), Some("Monitor: In"));

    // Undo, one step at a time.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(track(&lumen, "Keys").monitor, Monitor::Auto);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(track(&lumen, "Keys").midi_from.source, FromSource::Keyboard);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(track(&lumen, "Keys").midi_from.is_default());

    // A port the track names that is unplugged is listed, dimmed.
    select(&mut lumen, "Keys");
    lumen.app_mut().set_midi_from(MidiFrom {
        source: FromSource::Port("Gone".into()),
        channel: None,
    });
    lumen.frames(5);
    lumen.right_click(header.center());
    lumen.click("MIDI From");
    assert!(lumen.shows("Not connected"));
    assert!(lumen.shows("Gone"));
}

#[test]
fn the_mixer_strip_shows_and_sets_from_and_to() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Pad");
    let mut lumen = launch("from-strip", &fake);
    lumen.app_mut().set_detail_size(480.0);
    lumen.frames(5);
    assert!(lumen.shows("Keys MIDI From in the mixer: All ins"));
    assert!(lumen.shows("Keys MIDI To in the mixer: Poly"));
    lumen.click("Keys MIDI From in the mixer: All ins");
    lumen.click("Computer keyboard");
    assert_eq!(track(&lumen, "Keys").midi_from.source, FromSource::Keyboard);
    lumen.frames(3);
    assert!(lumen.shows("Keys MIDI From in the mixer: Keyboard"));
}

#[test]
fn midi_from_and_monitor_survive_saving_and_opening() {
    let fake = FakeControl::new();
    let mut lumen = launch("from-save", &fake);
    select(&mut lumen, "Keys");
    lumen.app_mut().set_midi_from(MidiFrom {
        source: FromSource::Port("Pad".into()),
        channel: Some(3),
    });
    super::midi_play::palette(&mut lumen, "Monitor: In");
    let path = lumen.dir().join("kept.lumen");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    let keys = track(&again, "Keys");
    assert_eq!(
        keys.midi_from,
        MidiFrom {
            source: FromSource::Port("Pad".into()),
            channel: Some(3)
        }
    );
    assert_eq!(keys.monitor, Monitor::In);
    assert!(track(&again, "Lead").midi_from.is_default());
}
