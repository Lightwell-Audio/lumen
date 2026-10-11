//! Auditioning and editing notes while playing (M5 slice 3d, the owner's
//! Live A): the piano roll's preview sounds on the edited clip's track
//! whether or not it listens, has notes of its own (a preview on a pitch
//! that a clip is playing never ends the clip's note, and the clip's end
//! never ends the preview), and every way of letting go works. Notes
//! committed while the song plays reach the engine through the clip list
//! and never hang. Watched where hanging is felt: a fake hardware port,
//! which must get a note-off for every note-on.

use std::time::Duration;

use eframe::egui::{Key, Modifiers, Pos2};
use lumen_app::TimeView;
use lumen_core::time::{PPQ, Ticks};
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

/// Half a beat in: the middle of beat `n`, where a note's body is grabbed.
fn half(n: i64) -> Ticks {
    Ticks(q(n).0 + PPQ / 2)
}

/// An eight-beat clip holding `notes`, on the port "Synth" with its track
/// silent, and the piano roll open on it.
fn roll(name: &str, notes: Vec<Note>, fake: &FakeControl) -> Lumen {
    let mut lumen = launch(name, &roll_project(notes), fake);
    lumen.app_mut().set_detail_size(460.0);
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: 60.0,
    });
    lumen.frames(40);
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, clip(&lumen))
        .expect("the clip is in view");
    lumen.double_click(r.center());
    lumen.frames(20);
    assert_eq!(lumen.app().editor_open_clip(), Some(clip(&lumen)));
    lumen
}

/// [`project`] with a clip of eight beats, the length of the piano roll's
/// test clips.
fn roll_project(notes: Vec<Note>) -> Project {
    let mut track = midi_track(1, "Keys", "poly", (0, 8), None, notes, vec![]);
    track.midi_to = Some(MidiTo {
        port: "Synth".into(),
        channel: 0,
    });
    track.monitor = Monitor::Off;
    let mut project = project_of("Audition", vec![track]);
    project.tempo[0].bpm = 480.0;
    project
}

/// The editor's point for beat `start` (a clip time) on `pitch`.
fn at(lumen: &Lumen, start: Ticks, pitch: u8) -> Pos2 {
    lumen
        .app()
        .editor_point(start, pitch)
        .expect("the editor has drawn")
}

fn hover(lumen: &mut Lumen, p: Pos2) {
    lumen.harness.hover_at(p);
    lumen.frames(2);
}

/// Picks every note of the open clip (Ctrl+A in the editor).
fn pick_all(lumen: &mut Lumen, notes: usize) {
    let p = at(lumen, half(0), 64);
    hover(lumen, p);
    lumen.key(Modifiers::COMMAND, Key::A);
    assert_eq!(lumen.app().editor_picked(), (0..notes).collect::<Vec<_>>());
}

/// The keys of the note-ons the port got, in order.
fn keys_on(fake: &FakeControl) -> Vec<u8> {
    sent(fake)
        .iter()
        .filter(|m| m[0] & 0xF0 == 0x90)
        .map(|m| m[1])
        .collect()
}

/// Every note-on is answered by a note-off of its key before the key is
/// struck again, and with `quiet` nothing is left sounding.
#[track_caller]
fn check_balance(fake: &FakeControl, quiet: bool) {
    let mut held = std::collections::BTreeSet::new();
    for m in sent(fake) {
        match m[0] & 0xF0 {
            0x90 => assert!(held.insert(m[1]), "key {} struck while sounding", m[1]),
            0x80 => assert!(held.remove(&m[1]), "key {} let go while silent", m[1]),
            _ => {}
        }
    }
    if quiet {
        assert!(held.is_empty(), "keys left sounding: {held:?}");
    }
}

#[test]
fn moving_three_selected_notes_auditions_all_three_and_lets_them_go() {
    let fake = fake_with_output();
    let notes = vec![note(0, 1, 60), note(1, 1, 64), note(2, 1, 67)];
    let mut lumen = roll("audition-chord", notes, &fake);
    pick_all(&mut lumen, 3);
    let from = at(&lumen, half(0), 60);
    let to = at(&lumen, half(0), 63);
    lumen.harness.hover_at(from);
    lumen.frames(2);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    // The press previews the three picked notes as a chord.
    until(&mut lumen, &fake, "the chord sounds", |b| b.0 == 3);
    assert_eq!(&keys_on(&fake)[..3], &[60, 64, 67]);
    // Up three rows: each note sounds at its new pitch.
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(from + (to - from) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.harness.drop_at(to);
    lumen.frames(3);
    settle(&mut lumen);
    until(&mut lumen, &fake, "all three let go", |b| b.0 == b.1);
    let heard = keys_on(&fake);
    for key in [63, 67, 70] {
        assert!(heard.contains(&key), "{key} sounded: {heard:?}");
    }
    check_balance(&fake, true);
    assert!(!lumen.app().audition_sounding());
}

#[test]
fn moving_notes_in_time_alone_strikes_each_note_once() {
    let fake = fake_with_output();
    let notes = vec![note(0, 1, 60), note(1, 1, 64), note(2, 1, 67)];
    let mut lumen = roll("audition-time", notes, &fake);
    pick_all(&mut lumen, 3);
    let from = at(&lumen, half(0), 60);
    let to = at(&lumen, half(1), 60);
    lumen.harness.hover_at(from);
    lumen.frames(2);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    until(&mut lumen, &fake, "the chord sounds", |b| b.0 == 3);
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(from + (to - from) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.harness.drop_at(to);
    lumen.frames(3);
    settle(&mut lumen);
    until(&mut lumen, &fake, "all three let go", |b| b.0 == b.1);
    // No pitch changed, so nothing was struck again (the time may have run
    // out during the drag: then the same three go once, not more).
    assert_eq!(keys_on(&fake), vec![60, 64, 67]);
    check_balance(&fake, true);
}

#[test]
fn escape_during_a_chord_lets_go_of_every_note_of_it() {
    let fake = fake_with_output();
    let notes = vec![note(0, 1, 60), note(1, 1, 64), note(2, 1, 67)];
    let mut lumen = roll("audition-chord-esc", notes, &fake);
    pick_all(&mut lumen, 3);
    let from = at(&lumen, half(0), 60);
    let to = at(&lumen, half(0), 65);
    lumen.harness.hover_at(from);
    lumen.frames(2);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    until(&mut lumen, &fake, "the chord sounds", |b| b.0 == 3);
    for step in 1..=4 {
        lumen
            .harness
            .hover_at(from + (to - from) * (step as f32 / 4.0));
        lumen.frames(1);
    }
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(2);
    assert!(!lumen.app().editor_gesture(), "Esc ended the drag");
    settle(&mut lumen);
    until(&mut lumen, &fake, "every note let go", |b| b.0 == b.1);
    check_balance(&fake, true);
    lumen.harness.drop_at(from);
    lumen.frames(3);
}

#[test]
fn pressing_an_unpicked_note_auditions_only_that_note() {
    let fake = fake_with_output();
    let notes = vec![note(0, 1, 60), note(1, 1, 64), note(2, 1, 67)];
    let mut lumen = roll("audition-one", notes, &fake);
    pick_all(&mut lumen, 3);
    // A click on empty space picks nothing; then the middle note is pressed.
    let empty = at(&lumen, half(6), 62);
    hover(&mut lumen, empty);
    lumen.harness.drag_at(empty);
    lumen.frames(2);
    lumen.harness.drop_at(empty);
    lumen.frames(3);
    assert!(lumen.app().editor_picked().is_empty());
    let p = at(&lumen, half(1), 64);
    hover(&mut lumen, p);
    lumen.harness.drag_at(p);
    lumen.frames(2);
    until(&mut lumen, &fake, "the one note sounds", |b| b.0 == 1);
    assert_eq!(keys_on(&fake), vec![64]);
    lumen.harness.drop_at(p);
    settle(&mut lumen);
    until(&mut lumen, &fake, "let go", |b| b.0 == b.1);
    check_balance(&fake, true);
}
