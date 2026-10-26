//! Capture MIDI (M9 slice R3, ADR-0104), end to end on the fake keyboard:
//! notes played on a MIDI track while not recording are kept in memory, and
//! Shift+F9 turns the newest phrase into a clip, as one undo step. Played
//! while the song moved they land where they were heard; played while
//! stopped, the first note lands on the playhead.
//!
//! Timing is checked loosely, for the reason `record.rs` gives: the test
//! device does not pace its blocks to real time. Exact placement is the unit
//! tests of `played.rs`.

use std::thread::sleep;
use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::testing::project_of;

use super::Lumen;
use super::midi_play::palette;
use super::record::{Keys, beats, clips_of, keyboard, launch, placed, reach, record, stop, track};

fn shift_f9(lumen: &mut Lumen) {
    lumen.key(Modifiers::SHIFT, Key::F9);
}

/// Starts with an armed, empty MIDI track and a fake keyboard.
fn start(name: &str) -> (Lumen, Keys) {
    let fake = keyboard();
    let project = project_of("Capture", vec![track(None, true)]);
    let lumen = launch(name, &project, &fake);
    let keys = Keys::new(&fake);
    (lumen, keys)
}

/// Plays a note and lets it go after a pause, so the device frames differ.
fn tap(lumen: &mut Lumen, keys: &Keys, pitch: u8, held: Duration) {
    let before = lumen.app().played_messages();
    keys.on(pitch, 90);
    sleep(held);
    keys.off(pitch);
    lumen.wait_for("the notes to be kept", move |app| {
        app.played_messages() >= before + 2
    });
}

#[test]
fn notes_played_while_stopped_become_a_clip_on_the_playhead_in_one_undo_step() {
    let (mut lumen, keys) = start("stopped");
    tap(&mut lumen, &keys, 60, Duration::from_millis(300));
    tap(&mut lumen, &keys, 64, Duration::from_millis(200));
    assert!(clips_of(&lumen).is_empty(), "nothing is a clip until asked");

    shift_f9(&mut lumen);
    let clips = clips_of(&lumen);
    assert_eq!(clips.len(), 1, "{:?}", lumen.app().note());
    assert_eq!(clips[0].start, Ticks::ZERO, "the playhead's bar");
    let notes = placed(&clips[0]);
    let pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
    assert_eq!(pitches, [60, 64], "in the order played");
    assert_eq!(
        notes[0].start,
        Ticks::ZERO,
        "the first note is on the playhead"
    );
    assert!(
        notes[1].start > notes[0].start,
        "the second comes after the first"
    );
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.starts_with("Captured 2 notes")),
        "{:?}",
        lumen.app().note()
    );
    assert_eq!(lumen.app().undo_label(), Some("Capture MIDI"));

    // Asked again, there is nothing new to capture.
    shift_f9(&mut lumen);
    assert_eq!(clips_of(&lumen).len(), 1, "a phrase is captured once");

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(clips_of(&lumen).is_empty(), "one undo takes it away");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(
        placed(&clips_of(&lumen)[0]).len(),
        2,
        "and redo gives it back"
    );
}

#[test]
fn notes_played_while_the_song_moves_are_captured_with_the_song() {
    let (mut lumen, keys) = start("moving");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    reach(&mut lumen, beats(1.0));
    keys.on(62, 100);
    reach(&mut lumen, beats(1.5));
    keys.off(62);
    lumen.wait_for("the notes to be kept", |app| app.played_messages() >= 2);
    stop(&mut lumen);

    shift_f9(&mut lumen);
    let clips = clips_of(&lumen);
    assert_eq!(clips.len(), 1, "{:?}", lumen.app().note());
    let notes = placed(&clips[0]);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].pitch, 62);
    assert_eq!(notes[0].velocity, 100);
    // Where the song was, not at the playhead it stopped on: after beat 0.
    assert!(
        notes[0].start > beats(0.25) && notes[0].start < beats(2.5),
        "{:?}",
        notes[0].start
    );
    assert_eq!(
        clips[0].start.0 % beats(4.0).0,
        0,
        "the clip begins on a bar"
    );
}

#[test]
fn what_a_take_recorded_is_not_offered_to_capture() {
    let (mut lumen, keys) = start("recorded");
    record(&mut lumen);
    reach(&mut lumen, beats(1.0));
    keys.on(60, 90);
    reach(&mut lumen, beats(1.5));
    keys.off(60);
    reach(&mut lumen, beats(2.0));
    stop(&mut lumen);
    assert_eq!(clips_of(&lumen).len(), 1, "the take");
    lumen.frames(5);

    shift_f9(&mut lumen);
    assert_eq!(
        clips_of(&lumen).len(),
        1,
        "it was recorded, not just played"
    );
    assert_eq!(lumen.app().undo_label(), Some("Record"));
}

#[test]
fn nothing_is_kept_while_the_setting_is_off() {
    let (mut lumen, keys) = start("off");
    tap(&mut lumen, &keys, 60, Duration::from_millis(50));
    assert!(lumen.app().played_messages() >= 2);

    palette(&mut lumen, "Keep played notes");
    assert!(!lumen.app().keeps_played_notes());
    lumen.frames(3);
    assert_eq!(
        lumen.app().played_messages(),
        0,
        "and what was kept is gone"
    );
    keys.on(64, 90);
    keys.off(64);
    lumen.frames(20);
    assert_eq!(lumen.app().played_messages(), 0);
    shift_f9(&mut lumen);
    assert!(clips_of(&lumen).is_empty());
    // The command cannot run, and the palette says so.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Capture MIDI".into()));
    lumen.frames(2);
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(
        lumen.app().palette_shows(&ctx)[0],
        ("Capture MIDI".to_owned(), false)
    );
}

#[test]
fn capture_midi_is_in_the_palette_and_says_why_when_it_cannot_run() {
    let (mut lumen, _keys) = start("palette");
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Capture MIDI".into()));
    lumen.frames(2);
    let ctx = lumen.harness.ctx.clone();
    let rows = lumen.app().palette_shows(&ctx);
    assert_eq!(
        rows[0],
        ("Capture MIDI".to_owned(), false),
        "nothing played"
    );
}
