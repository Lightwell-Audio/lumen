//! Hearing MIDI (M5 slice 1): a MIDI track plays its instrument, each
//! preset sounds, notes never hang (stop, a jump, a loop wrap, a clip mute,
//! an edit, a change of output, Panic), play in the middle of a note
//! chases it, and MIDI To sends to a port. Hanging is watched where it
//! would be heard or felt: the hardware port (a fake device), which gets
//! every note-on and must get its note-off.
//!
//! The test device plays a block per poll, far slower than real time, so
//! these use a fast tempo to keep notes short in real time.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::model::{
    ClipEvent, EventKind, Id, Marker, MidiTo, Note, Project, Track, TrackKind,
};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::properties::{rect, select};

const BEAT: i64 = Ticks::quarters(1).0;

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

fn note(start: i64, length: i64, pitch: u8) -> Note {
    Note(q(start), q(length), pitch, 100)
}

/// Starts Lumen with `project` open, MIDI from `fake`.
fn launch(name: &str, project: &Project, fake: &FakeControl) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("save the test project");
    let fake = fake.clone();
    let tracks = project.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path).with_midi(fake));
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    // Rows grow in over a few frames.
    lumen.frames(30);
    lumen
}

fn fake_with_output() -> FakeControl {
    let fake = FakeControl::new();
    fake.plug(Direction::Output, "Synth");
    fake
}

/// One MIDI track with the synth on `preset` and one long note.
fn one_note(preset: &str) -> Project {
    project_of(
        "One note",
        vec![midi_track(
            1,
            "Keys",
            preset,
            (0, 64),
            None,
            vec![note(0, 60, 60)],
            vec![],
        )],
    )
}

/// A track sent to the fake port "Synth" on channel 4, with `notes`
/// repeating every `repeat` beats (if given), at 480 BPM.
fn to_port(notes: Vec<Note>, repeat: Option<i64>) -> Project {
    let mut track = midi_track(1, "Drums", "poly", (0, 64), repeat, notes, vec![]);
    track.midi_to = Some(MidiTo {
        port: "Synth".into(),
        channel: 3,
    });
    let mut project = project_of("To port", vec![track]);
    project.tempo[0].bpm = 480.0;
    project
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
}

fn stop(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("stop", |app| !app.is_playing());
}

/// Waits for the fake output to be opened by the app.
fn wait_open(lumen: &mut Lumen, fake: &FakeControl) {
    for _ in 0..1_500 {
        if fake.opens(Direction::Output, "Synth") > 0 {
            return;
        }
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the output was never opened");
}

/// Runs frames until `done` holds for what the port received.
#[track_caller]
fn until_sent(
    lumen: &mut Lumen,
    fake: &FakeControl,
    what: &str,
    done: impl Fn(&[Vec<u8>]) -> bool,
) {
    for _ in 0..1_000 {
        lumen.frames(1);
        let sent: Vec<Vec<u8>> = fake.sent().into_iter().map(|(_, b)| b).collect();
        if done(&sent) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("{what}; the port got {:?}", fake.sent());
}

fn count(sent: &[Vec<u8>], status: u8) -> usize {
    sent.iter().filter(|m| m[0] & 0xF0 == status).count()
}

/// Note-ons and note-offs the port got.
fn balance(fake: &FakeControl) -> (usize, usize) {
    let sent: Vec<Vec<u8>> = fake.sent().into_iter().map(|(_, b)| b).collect();
    (count(&sent, 0x90), count(&sent, 0x80))
}

/// The loudest sample heard over a stretch, after letting it settle.
fn loudest(lumen: &mut Lumen) -> f32 {
    let heard = lumen.listen(Duration::from_millis(100), 2_000);
    heard
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

/// The RMS of the left channel over a stretch of what plays.
fn rms(lumen: &mut Lumen) -> f32 {
    let heard = lumen.listen(Duration::from_millis(250), 6_000);
    (heard.iter().map(|f| f[0] * f[0]).sum::<f32>() / heard.len() as f32).sqrt()
}

fn track_id(lumen: &Lumen) -> Id {
    lumen.app().project().tracks[0].id
}

fn select_track(lumen: &mut Lumen) {
    lumen.harness.get_by_label_contains("Track Keys,").click();
    lumen.frames(3);
}

/// Opens the palette, types `query` and runs the first match.
pub fn palette(lumen: &mut Lumen, query: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(query.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

#[test]
fn a_new_midi_track_has_the_synth_and_a_midi_track_is_heard() {
    let fake = FakeControl::new();
    let mut lumen = launch("midi-heard", &one_note("poly"), &fake);
    let track = &lumen.app().project().tracks[0];
    assert_eq!(track.instrument().unwrap().preset(), Some("poly"));
    play(&mut lumen);
    let level = loudest(&mut lumen);
    assert!(level > 0.01 && level < 1.0, "heard {level}");
    // Stopping lets the release ring out, then it is quiet.
    stop(&mut lumen);
    lumen.frames(5);
    let quiet = lumen.listen(Duration::from_millis(1_500), 2_000);
    let after = quiet.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
    assert!(after < 1e-4, "still sounding after stop: {after}");

    // A track added from the keyboard gets the synth on Poly.
    let mut lumen = Lumen::start("midi-new-track");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    let added = &lumen.app().project().tracks[0];
    assert_eq!(added.kind, TrackKind::Midi);
    assert_eq!(added.instrument().and_then(|d| d.preset()), Some("poly"));
}

#[test]
fn every_preset_is_chosen_from_the_palette_and_sounds() {
    let fake = FakeControl::new();
    // A short note on every beat, so each new sound is heard on the next.
    let mut project = project_of(
        "Repeated",
        vec![midi_track(
            1,
            "Keys",
            "poly",
            (0, 4000),
            Some(1),
            vec![note(0, 1, 60)],
            vec![],
        )],
    );
    project.tempo[0].bpm = 300.0;
    let mut lumen = launch("midi-presets", &project, &fake);
    select_track(&mut lumen);
    play(&mut lumen);
    let mut levels = Vec::new();
    for (name, id) in [
        ("Poly", "poly"),
        ("Keys", "keys"),
        ("Bass", "bass"),
        ("Pluck", "pluck"),
        ("Pad", "pad"),
    ] {
        palette(&mut lumen, &format!("Instrument: {name}"));
        let preset = lumen.app().project().tracks[0]
            .instrument()
            .and_then(|d| d.preset().map(str::to_owned));
        assert_eq!(preset.as_deref(), Some(id));
        levels.push(rms(&mut lumen));
    }
    stop(&mut lumen);
    assert!(
        levels.iter().all(|l| *l > 0.001),
        "each preset sounds: {levels:?}"
    );
    // The track's undo history names each change.
    assert_eq!(lumen.app().undo_label(), Some("Instrument: Pad"));
}

#[test]
fn the_track_menu_offers_the_instruments_and_midi_to() {
    let fake = fake_with_output();
    let mut lumen = launch("midi-menu", &one_note("poly"), &fake);
    let header = lumen.harness.get_by_label_contains("Track Keys,").rect();
    lumen.right_click(header.center());
    lumen.click("Instrument");
    lumen.click("Pad");
    assert_eq!(
        lumen.app().project().tracks[0]
            .instrument()
            .and_then(|d| d.preset()),
        Some("pad")
    );
    // MIDI To lists the output; choosing a channel sets it, and the output
    // is switched on for tracks.
    lumen.right_click(header.center());
    lumen.click("MIDI To");
    lumen.click("Synth");
    lumen.click("Channel 3");
    let out = lumen.app().project().tracks[0].midi_to.clone();
    assert_eq!(
        out,
        Some(MidiTo {
            port: "Synth".into(),
            channel: 2
        })
    );
    wait_open(&mut lumen, &fake);
    assert_eq!(lumen.app().undo_label(), Some("MIDI To: Synth 3"));
    // The palette sends it back to the instrument.
    select_track(&mut lumen);
    palette(&mut lumen, "MIDI To: Instrument");
    assert!(lumen.app().project().tracks[0].midi_to.is_none());
}

#[test]
fn midi_to_sends_the_notes_to_the_port_and_stop_lets_go() {
    let fake = fake_with_output();
    let project = to_port(vec![note(1, 1, 36), note(2, 40, 38)], None);
    let mut lumen = launch("midi-to-port", &project, &fake);
    wait_open(&mut lumen, &fake);
    play(&mut lumen);
    until_sent(&mut lumen, &fake, "two notes", |s| count(s, 0x90) == 2);
    let sent: Vec<Vec<u8>> = fake.sent().into_iter().map(|(_, b)| b).collect();
    // Channel 4 (status low nibble 3): the first note on and off, then the
    // second note on.
    assert_eq!(sent[0], [0x93, 36, 100]);
    assert_eq!(sent[1][0], 0x83);
    assert_eq!(sent[1][1], 36);
    assert_eq!(sent[2], [0x93, 38, 100]);
    // The long note is still sounding: stopping releases it.
    let (ons, offs) = balance(&fake);
    assert_eq!((ons, offs), (2, 1));
    stop(&mut lumen);
    until_sent(&mut lumen, &fake, "the note let go", |s| {
        count(s, 0x80) == 2
    });
    assert_eq!(balance(&fake), (2, 2));
}

#[test]
fn a_loop_wrap_releases_the_note_it_cuts() {
    let fake = fake_with_output();
    // A note far longer than the one-beat loop: every wrap must let it go.
    let mut project = to_port(vec![note(0, 40, 50)], None);
    project.transport.loop_start = Ticks::ZERO;
    project.transport.loop_end = q(1);
    project.transport.loop_enabled = true;
    let mut lumen = launch("midi-loop-wrap", &project, &fake);
    wait_open(&mut lumen, &fake);
    play(&mut lumen);
    // Each pass starts the note and the wrap ends it: after a few passes
    // the port has seen several off-on pairs, while still playing.
    until_sent(&mut lumen, &fake, "several passes", |s| count(s, 0x80) >= 3);
    let (ons, offs) = balance(&fake);
    assert!(ons >= offs && ons - offs <= 1, "{ons} on, {offs} off");
    stop(&mut lumen);
    until_sent(&mut lumen, &fake, "everything let go", |s| {
        count(s, 0x90) == count(s, 0x80)
    });
}

#[test]
fn a_jump_a_clip_mute_and_an_edit_each_let_go_of_what_sounds() {
    // Jumping out of a note, inside the same note, muting the clip and
    // deleting it: each leaves the port with every note-on answered.
    for case in ["jump out", "jump in", "mute", "delete"] {
        eprintln!("case {case}");
        let fake = fake_with_output();
        let mut project = to_port(vec![note(0, 4, 50), note(6, 60, 52)], None);
        project.markers.push(Marker {
            id: Id(900),
            at: q(if case == "jump in" { 8 } else { 40 }),
            name: "Locator 1".into(),
        });
        let mut lumen = launch(
            &format!("midi-let-go-{}", case.replace(' ', "-")),
            &project,
            &fake,
        );
        wait_open(&mut lumen, &fake);
        if case == "jump in" {
            // Start inside the long note (it began at beat 6), then jump
            // elsewhere inside it: released and started again.
            lumen.key(Modifiers::COMMAND, Key::ArrowRight);
            lumen.wait_for("the playhead", |app| app.playhead() == Ticks::quarters(8));
            play(&mut lumen);
            until_sent(&mut lumen, &fake, "the chased note", |s| {
                count(s, 0x90) >= 1
            });
            lumen.key(Modifiers::COMMAND, Key::ArrowLeft);
            until_sent(&mut lumen, &fake, "the note let go", |s| {
                count(s, 0x80) >= 1
            });
        } else {
            play(&mut lumen);
            until_sent(&mut lumen, &fake, "the first note", |s| count(s, 0x90) >= 1);
            match case {
                "jump out" => {
                    // To the locator at beat 40: inside the long note.
                    lumen.key(Modifiers::COMMAND, Key::ArrowRight);
                }
                "mute" => {
                    let clip = lumen.app().project().tracks[0].clips[0].id;
                    select(&mut lumen, clip);
                    lumen.key(Modifiers::NONE, Key::Num0);
                }
                _ => {
                    let clip = lumen.app().project().tracks[0].clips[0].id;
                    select(&mut lumen, clip);
                    lumen.key(Modifiers::NONE, Key::Delete);
                }
            }
            until_sent(&mut lumen, &fake, "the note let go", |s| {
                count(s, 0x80) >= 1
            });
        }
        stop(&mut lumen);
        until_sent(&mut lumen, &fake, "every note answered", |s| {
            count(s, 0x90) == count(s, 0x80)
        });
        let _ = rect;
    }
}

#[test]
fn deleting_the_clip_or_moving_off_the_output_lets_go_and_panic_silences_the_port() {
    let fake = fake_with_output();
    let mut lumen = launch(
        "midi-edit-panic",
        &to_port(vec![note(0, 60, 50)], None),
        &fake,
    );
    wait_open(&mut lumen, &fake);
    play(&mut lumen);
    until_sent(&mut lumen, &fake, "the note", |s| count(s, 0x90) >= 1);

    // Back to the instrument: the port lets go.
    select_track_named(&mut lumen, "Drums");
    palette(&mut lumen, "MIDI To: Instrument");
    until_sent(&mut lumen, &fake, "the port let go", |s| {
        count(s, 0x80) >= 1
    });
    assert_eq!(balance(&fake), (1, 1));

    // Panic: All Notes Off and All Sound Off on every channel of the port.
    palette(&mut lumen, "Panic");
    until_sent(&mut lumen, &fake, "all notes and sound off", |s| {
        let all =
            |number: u8| (0..16_u8).all(|channel| s.contains(&vec![0xB0 | channel, number, 0]));
        all(123) && all(120)
    });
    lumen.wait_for("the engine to stop", |app| !app.is_playing());
}

#[test]
fn play_in_the_middle_of_a_note_starts_it_at_once() {
    let fake = FakeControl::new();
    let mut project = one_note("poly");
    project.markers.push(Marker {
        id: Id(900),
        at: q(16),
        name: "Locator 1".into(),
    });
    let mut lumen = launch("midi-chase", &project, &fake);
    // Ctrl+Right jumps to the locator, bar 5; the note began at bar 1.
    lumen.key(Modifiers::COMMAND, Key::ArrowRight);
    lumen.wait_for("the playhead", |app| app.playhead() == Ticks::quarters(16));
    play(&mut lumen);
    let level = loudest(&mut lumen);
    assert!(
        level > 0.01,
        "the held note sounds from the first block: {level}"
    );
}

#[test]
fn a_muted_track_is_silent_and_a_port_that_is_unplugged_costs_nothing() {
    let fake = fake_with_output();
    let mut lumen = launch("midi-mute", &to_port(vec![note(0, 60, 50)], None), &fake);
    wait_open(&mut lumen, &fake);
    play(&mut lumen);
    until_sent(&mut lumen, &fake, "the note", |s| count(s, 0x90) >= 1);
    // Unplugged mid-note: nothing breaks, and playing on is fine.
    fake.unplug(Direction::Output, "Synth");
    lumen.frames(30);
    std::thread::sleep(Duration::from_millis(200));
    lumen.frames(5);
    assert!(lumen.app().is_playing());
    stop(&mut lumen);
    fake.plug(Direction::Output, "Synth");
    for _ in 0..200 {
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    // Muting a track that sends to a port silences the port: nothing new.
    let before = fake.sent().len();
    select_track_named(&mut lumen, "Drums");
    palette(&mut lumen, "Mute selected tracks");
    play(&mut lumen);
    std::thread::sleep(Duration::from_millis(300));
    lumen.frames(10);
    assert_eq!(fake.sent().len(), before, "a muted track sends nothing");
}

fn select_track_named(lumen: &mut Lumen, name: &str) {
    lumen
        .harness
        .get_by_label_contains(&format!("Track {name},"))
        .click();
    lumen.frames(3);
}

#[test]
fn the_instrument_and_output_are_saved_and_come_back() {
    let fake = fake_with_output();
    let mut lumen = launch("midi-saved", &one_note("poly"), &fake);
    select_track(&mut lumen);
    palette(&mut lumen, "Instrument: Bass");
    let path = lumen.dir().join("saved.lumen");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_dirty());
    let reopened: Project = lumen_project::load(&path).expect("the saved project loads");
    let track: &Track = &reopened.tracks[0];
    assert_eq!(track.instrument().and_then(|d| d.preset()), Some("bass"));
    let _ = (track_id(&lumen), rect as fn(&Lumen, Id) -> _, BEAT);
}

/// A note in a clip with a controller or two, for the controller path.
#[test]
fn controllers_in_a_clip_reach_the_port() {
    let fake = fake_with_output();
    let mut track = midi_track(
        1,
        "Drums",
        "poly",
        (0, 64),
        None,
        vec![note(1, 1, 60)],
        vec![
            ClipEvent {
                at: q(0),
                kind: EventKind::Controller {
                    number: 64,
                    value: 127,
                },
            },
            ClipEvent {
                at: q(1),
                kind: EventKind::PitchBend { value: 4096 },
            },
            ClipEvent {
                at: q(1),
                kind: EventKind::Program {
                    program: 9,
                    bank: Some(130),
                },
            },
        ],
    );
    track.midi_to = Some(MidiTo {
        port: "Synth".into(),
        channel: 1,
    });
    let mut project = project_of("Controllers", vec![track]);
    project.tempo[0].bpm = 480.0;
    let mut lumen = launch("midi-controllers", &project, &fake);
    wait_open(&mut lumen, &fake);
    play(&mut lumen);
    until_sent(&mut lumen, &fake, "the note", |s| count(s, 0x80) >= 1);
    let sent: Vec<Vec<u8>> = fake.sent().into_iter().map(|(_, b)| b).collect();
    assert!(sent.contains(&vec![0xB1, 64, 127]), "sustain pedal down");
    assert!(sent.contains(&vec![0xE1, 0, 96]), "pitch bend");
    assert!(sent.contains(&vec![0xC1, 9]), "program change");
    assert!(sent.contains(&vec![0xB1, 0, 1]), "bank select MSB");
    assert!(sent.contains(&vec![0xB1, 32, 2]), "bank select LSB");
    stop(&mut lumen);
    // Stopping lets go of the pedal and the bend too.
    until_sent(&mut lumen, &fake, "the pedal up", |s| {
        s.contains(&vec![0xB1, 64, 0]) && s.contains(&vec![0xE1, 0, 64])
    });
}

/// Left-channel samples of a stretch of what plays.
fn left(heard: &[[f32; 4]]) -> Vec<f32> {
    heard.iter().map(|f| f[0]).collect()
}

fn steepest(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

fn rms_of(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// How bright a sound is: the size of its steps against its own.
fn brightness(x: &[f32]) -> f32 {
    let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
    rms_of(&d) / rms_of(x)
}

/// Choosing another preset from the track menu reaches a note that is
/// already sounding: it glides to the new sound within about 100 ms, with
/// no retrigger (a Pad note would start from silence again) and no click.
#[test]
fn a_preset_chosen_from_the_menu_changes_the_note_being_held() {
    let fake = FakeControl::new();
    let mut lumen = launch("midi-glide", &one_note("pad"), &fake);
    play(&mut lumen);
    // Past Pad's attack and decay: a steady held note.
    let before = left(&lumen.listen(Duration::from_millis(1_500), 9_600));
    assert!(rms_of(&before) > 0.01, "the pad is held");
    let header = lumen.harness.get_by_label_contains("Track Keys,").rect();
    lumen.right_click(header.center());
    lumen.click("Instrument");
    lumen.click("Poly");
    assert_eq!(
        lumen.app().project().tracks[0]
            .instrument()
            .and_then(|d| d.preset()),
        Some("poly")
    );
    // The glide is heard from the first frame; 100 ms later it has settled.
    let heard = left(&lumen.listen(Duration::ZERO, 14_400));
    let after = heard[4_800..].to_vec();
    let (b0, b1) = (brightness(&before), brightness(&after));
    assert!(b1 > b0 * 1.15, "brighter: {b0} -> {b1}");
    // ...it is the same note, not a new one starting from silence...
    assert!(rms_of(&after) > 0.01, "still sounding at its level");
    // ...and nothing stepped (Poly's own steps are the larger).
    let mut across = before[before.len() - 64..].to_vec();
    across.extend(&heard);
    let level = steepest(&before).max(steepest(&after));
    assert!(
        steepest(&across) < level * 2.2,
        "a click: {} vs {level}",
        steepest(&across),
    );
    // Undo glides back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    let back = left(&lumen.listen(Duration::from_millis(100), 9_600));
    assert!(brightness(&back) < b1 * 0.9, "darker again");
}
