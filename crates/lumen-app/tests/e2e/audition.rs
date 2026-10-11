//! Auditioning and editing notes while playing (M5 slice 3d, the owner's
//! Live A): the piano roll's preview sounds on the edited clip's track
//! whether or not it listens, has notes of its own (a preview on a pitch
//! that a clip is playing never ends the clip's note, and the clip's end
//! never ends the preview), and every way of letting go works. Notes
//! committed while the song plays reach the engine through the clip list
//! and never hang. Watched where hanging is felt: a fake hardware port,
//! which must get a note-off for every note-on.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::model::{Id, MidiTo, Monitor, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

fn note(start: i64, length: i64, pitch: u8) -> Note {
    Note::new(q(start), q(length), pitch, 100)
}

/// One track sent to the fake port "Synth" on channel 1, monitoring off
/// (so it listens to nothing), at 480 BPM.
fn project(notes: Vec<Note>) -> Project {
    let mut track = midi_track(1, "Drums", "poly", (0, 64), None, notes, vec![]);
    track.midi_to = Some(MidiTo {
        port: "Synth".into(),
        channel: 0,
    });
    track.monitor = Monitor::Off;
    let mut project = project_of("Audition", vec![track]);
    project.tempo[0].bpm = 480.0;
    project
}

fn launch(name: &str, project: &Project, fake: &FakeControl) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("save the test project");
    let owned = fake.clone();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path).with_midi(owned));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.frames(30);
    for _ in 0..1_500 {
        if lumen_open(fake) {
            break;
        }
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    lumen
}

fn lumen_open(fake: &FakeControl) -> bool {
    fake.opens(Direction::Output, "Synth") > 0
}

fn fake_with_output() -> FakeControl {
    let fake = FakeControl::new();
    fake.plug(Direction::Output, "Synth");
    fake
}

fn track(lumen: &Lumen) -> Id {
    lumen.app().project().tracks[0].id
}

fn clip(lumen: &Lumen) -> Id {
    lumen.app().project().tracks[0].clips[0].id
}

fn sent(fake: &FakeControl) -> Vec<Vec<u8>> {
    fake.sent().into_iter().map(|(_, b)| b).collect()
}

/// (note-ons, note-offs) of the port.
fn balance(fake: &FakeControl) -> (usize, usize) {
    let s = sent(fake);
    let count = |status: u8| s.iter().filter(|m| m[0] & 0xF0 == status).count();
    (count(0x90), count(0x80))
}

#[track_caller]
fn until(lumen: &mut Lumen, fake: &FakeControl, what: &str, done: impl Fn((usize, usize)) -> bool) {
    for _ in 0..1_000 {
        lumen.frames(1);
        if done(balance(fake)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("{what}; the port got {:?}", fake.sent());
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
}

fn stop(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("stop", |app| !app.is_playing());
}

/// Runs frames for a while, so anything due has gone.
fn settle(lumen: &mut Lumen) {
    for _ in 0..40 {
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_preview_sounds_on_a_track_that_does_not_listen_and_lets_go() {
    let fake = fake_with_output();
    let mut lumen = launch("audition-basic", &project(vec![note(8, 1, 40)]), &fake);
    let id = track(&lumen);
    assert!(lumen.app_mut().audition(id, 62, 90));
    until(&mut lumen, &fake, "the preview", |b| b == (1, 0));
    assert_eq!(sent(&fake)[0], [0x90, 62, 90]);
    lumen.app_mut().release_audition();
    until(&mut lumen, &fake, "the release", |b| b == (1, 1));
    assert_eq!(sent(&fake)[1][..2], [0x80, 62]);
    // Another preview lets go of the last by itself, and ends alone.
    lumen.app_mut().audition(id, 64, 90);
    lumen.app_mut().audition(id, 65, 90);
    until(&mut lumen, &fake, "the second then third", |b| b.0 == 3);
    until(&mut lumen, &fake, "both let go", |b| b == (3, 3));
    assert!(!lumen.app().audition_sounding());
}

#[test]
fn a_preview_on_a_pitch_the_clip_plays_never_ends_the_clips_note() {
    let fake = fake_with_output();
    // A clip note on pitch 50 that lasts far longer than the test.
    let mut lumen = launch("audition-same", &project(vec![note(0, 60, 50)]), &fake);
    let id = track(&lumen);
    play(&mut lumen);
    until(&mut lumen, &fake, "the clip's note", |b| b == (1, 0));
    lumen.app_mut().audition(id, 50, 80);
    until(&mut lumen, &fake, "the preview", |b| b == (2, 0));
    lumen.app_mut().release_audition();
    until(&mut lumen, &fake, "the preview let go", |b| b == (2, 1));
    // The clip's own note is still sounding: nothing more came off.
    settle(&mut lumen);
    assert_eq!(balance(&fake), (2, 1));
    // ...and the clip's end (stop) does not matter to a later preview.
    stop(&mut lumen);
    until(&mut lumen, &fake, "the clip's note let go", |b| b == (2, 2));
    lumen.app_mut().audition(id, 50, 80);
    until(&mut lumen, &fake, "a preview", |b| b == (3, 2));
    lumen.app_mut().release_audition();
    until(&mut lumen, &fake, "answered", |b| b == (3, 3));
}

#[test]
fn a_held_key_on_the_pitch_is_not_ended_by_the_preview() {
    let fake = fake_with_output();
    // The track listens to the keyboard (monitor In).
    let mut listening = project(vec![note(8, 1, 40)]);
    std::sync::Arc::make_mut(&mut listening.tracks[0]).monitor = Monitor::In;
    let mut lumen = launch("audition-key", &listening, &fake);
    let id = track(&lumen);
    assert_eq!(lumen.app().project().tracks[0].monitor, Monitor::In);
    let key_on = lumen_core::midi::Msg::note_on(0, 55, 100);
    assert!(lumen.app_mut().play_live(key_on));
    until(&mut lumen, &fake, "the key", |b| b == (1, 0));
    lumen.app_mut().audition(id, 55, 70);
    until(&mut lumen, &fake, "the preview", |b| b == (2, 0));
    lumen.app_mut().release_audition();
    until(&mut lumen, &fake, "the preview let go", |b| b == (2, 1));
    settle(&mut lumen);
    assert_eq!(balance(&fake), (2, 1), "the held key still sounds");
    assert!(
        lumen
            .app_mut()
            .play_live(lumen_core::midi::Msg::note_off(0, 55))
    );
    until(&mut lumen, &fake, "the key let go", |b| b == (2, 2));
}

#[test]
fn the_setting_stop_and_panic_let_go() {
    let fake = fake_with_output();
    let mut lumen = launch("audition-ways", &project(vec![note(8, 1, 40)]), &fake);
    let id = track(&lumen);
    // Off in the settings: nothing plays.
    lumen.app_mut().set_audition_setting(false);
    assert!(!lumen.app_mut().audition(id, 60, 90));
    lumen.app_mut().set_audition_setting(true);
    // Stop: a note sounding when playback stops is let go.
    play(&mut lumen);
    lumen.app_mut().audition(id, 61, 90);
    until(&mut lumen, &fake, "a preview", |b| b.0 == 1);
    stop(&mut lumen);
    until(&mut lumen, &fake, "let go at stop", |b| b == (1, 1));
    // Turned off while sounding: the next frame lets go.
    lumen.app_mut().audition(id, 62, 90);
    until(&mut lumen, &fake, "a preview", |b| b.0 == 2);
    lumen.app_mut().set_audition_setting(false);
    until(&mut lumen, &fake, "let go by the setting", |b| b == (2, 2));
    lumen.app_mut().set_audition_setting(true);
    // Panic forgets it: the engine silences the port, and nothing stays
    // owed or sounding in the service.
    lumen.app_mut().audition(id, 63, 90);
    until(&mut lumen, &fake, "a preview", |b| b.0 == 3);
    lumen.key(Modifiers::COMMAND, Key::K);
    super::midi_play::palette(&mut lumen, "Panic");
    settle(&mut lumen);
    assert!(!lumen.app().audition_sounding());
}

/// A note edit committed while the song plays: the note that sounds is let
/// go (or kept, if lengthened), and every note-on is answered.
fn committed(name: &str, edit: lumen_app::testing::NoteEdit, longer_than_expected: bool) {
    let fake = fake_with_output();
    // A long note from the start, and a short one later to keep time.
    let mut lumen = launch(
        name,
        &project(vec![note(0, 30, 50), note(40, 1, 52)]),
        &fake,
    );
    play(&mut lumen);
    until(&mut lumen, &fake, "the note", |b| b == (1, 0));
    let clip = clip(&lumen);
    lumen.app_mut().edit_notes(clip, &[0], edit);
    lumen.frames(5);
    settle(&mut lumen);
    let (ons, offs) = balance(&fake);
    assert!(ons - offs <= 1, "{name}: {ons} on, {offs} off");
    if longer_than_expected {
        // Lengthened: it sounds on (never doubled), or was retriggered
        // with its off sent first.
        assert!(ons == 1 && offs == 0 || ons == 2 && offs == 1, "{name}");
    } else {
        assert!(offs >= 1, "{name}: the changed note is let go");
    }
    stop(&mut lumen);
    until(&mut lumen, &fake, "every note answered", |b| b.0 == b.1);
}

#[test]
fn a_note_moved_deleted_or_muted_while_it_sounds_is_let_go() {
    use lumen_app::testing::NoteEdit;
    committed(
        "committed-move",
        NoteEdit::Move {
            time: q(2),
            pitch: 0,
        },
        false,
    );
    committed(
        "committed-transpose",
        NoteEdit::Move {
            time: Ticks::ZERO,
            pitch: 3,
        },
        false,
    );
    committed("committed-delete", NoteEdit::Delete, false);
    committed("committed-mute", NoteEdit::Mute(true), false);
}

#[test]
fn a_note_lengthened_while_it_sounds_keeps_sounding_without_hanging() {
    committed(
        "committed-lengthen",
        lumen_app::testing::NoteEdit::Lengthen(q(4)),
        true,
    );
}
