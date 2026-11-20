use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::drums::presets::Preset;
use lumen_project::model::{ClipContent, Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::midi_play::palette;

const RATE: f64 = 48_000.0;

const LISTEN: usize = 192_000;

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

fn note(start: i64, length: i64, pitch: u8) -> Note {
    Note::new(q(start), q(length), pitch, 100)
}

fn launch(name: &str, project: &Project) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("save the test project");
    let tracks = project.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    lumen.frames(30);
    lumen
}

fn kick_track() -> Project {
    let beats: Vec<Note> = (0..16).map(|beat| note(beat, 1, 36)).collect();
    project_of(
        "Drums",
        vec![midi_track(
            1,
            "Drums",
            "pluck",
            (0, 16),
            Some(1),
            beats,
            vec![],
        )],
    )
}

fn loudest(lumen: &mut Lumen) -> f32 {
    let heard = lumen.listen(Duration::from_millis(100), 2_000);
    heard
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

#[track_caller]
fn until_quiet(lumen: &mut Lumen, quiet: f32, what: &str) -> f32 {
    let deadline = lumen_testkit::Deadline::new();
    loop {
        let heard = loudest(lumen);
        if heard < quiet {
            return heard;
        }
        deadline.check(format_args!("{what}; still heard at {heard}"));
        std::thread::sleep(lumen_testkit::POLL);
    }
}

#[track_caller]
fn until_loud(lumen: &mut Lumen, loud: f32, what: &str) -> f32 {
    let deadline = lumen_testkit::Deadline::new();
    loop {
        let heard = loudest(lumen);
        if heard > loud {
            return heard;
        }
        deadline.check(format_args!("{what}; only {heard} heard"));
        std::thread::sleep(lumen_testkit::POLL);
    }
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
}

fn stop(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("stop", |app| !app.is_playing());
}

fn notes_of(lumen: &Lumen) -> Vec<Note> {
    let clip = &lumen.app().project().tracks[0].clips[0];
    match &clip.content {
        ClipContent::Midi { notes, .. } => notes.clone(),
        ClipContent::Audio { .. } => panic!("a MIDI clip"),
    }
}

#[test]
fn the_palette_opens_the_map_editor_and_a_kit_chosen_there_plays_the_track() {
    let mut lumen = launch("drums-attach-mute", &kick_track());
    lumen.harness.get_by_label_contains("Track Drums,").click();
    lumen.frames(3);
    play(&mut lumen);
    let before = loudest(&mut lumen);
    assert!(before > 0.01, "the track sounds: {before}");
    let notes = notes_of(&lumen);

    palette(&mut lumen, "Drum map");
    assert!(lumen.app().drum_map_editor_open(), "the editor is drawn");
    lumen.click("808 style (16 sounds)");
    lumen.frames(5);
    let project = lumen.app().project();
    let map = project.drum_maps[0].clone();
    assert_eq!(map.name, "808 style");
    assert_eq!(project.tracks[0].drum_map, Some(map.id));
    assert_eq!(map.sounds.len(), 16);
    assert_eq!(
        lumen.app().drum_map_editor_map(),
        Some(map.id),
        "the editor shows the map it attached"
    );
    assert_eq!(notes_of(&lumen), notes, "the notes never changed");
    assert_eq!(
        lumen.app().undo_label(),
        Some("Attach drum map"),
        "one undo step"
    );

    lumen.click("Map: 808 style");
    lumen.frames(3);
    assert!(
        lumen.shows("808 style (16 sounds)"),
        "the project's map is listed"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);

    lumen.click("Mute Kick");
    let after = until_quiet(&mut lumen, 1e-4, "the muted sound to go");
    assert!(after < 1e-4, "the muted sound is gone: {after}");
    assert_eq!(lumen.app().undo_label(), Some("Mute drum sound"));

    lumen.key(Modifiers::COMMAND, Key::Z);
    let back = until_loud(&mut lumen, 0.01, "the undone sound");
    assert!(back > 0.01, "undone, it sounds again: {back}");
    stop(&mut lumen);
}

fn mapped_song() -> Project {
    let mut project = project_of(
        "Mapped",
        vec![midi_track(
            1,
            "Drums",
            "pluck",
            (0, 16),
            None,
            vec![
                note(0, 1, 36),
                note(1, 1, 38),
                note(2, 1, 42),
                note(3, 1, 36),
            ],
            vec![],
        )],
    );
    let map = Preset::GENERAL_MIDI.map(Id(500));
    project.drum_maps = vec![map.clone()];
    let track = std::sync::Arc::make_mut(&mut project.tracks[0]);
    track.drum_map = Some(map.id);
    project
}

fn onsets(left: impl IntoIterator<Item = f32>) -> Vec<usize> {
    let mut found: Vec<usize> = Vec::new();
    for (i, s) in left.into_iter().enumerate() {
        if s.abs() > 0.1 && found.last().is_none_or(|last| i - last > 12_000) {
            found.push(i);
        }
    }
    found
}

fn gaps(onsets: &[usize]) -> Vec<f64> {
    onsets
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 / RATE)
        .collect()
}

fn hear(lumen: &mut Lumen) -> Vec<f32> {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);
    let heard = lumen.listen_from(|l| l.key(Modifiers::NONE, Key::Space), LISTEN);
    stop(lumen);
    lumen.frames(5);
    heard.iter().map(|f| f[0]).collect()
}

fn export(lumen: &mut Lumen, path: &Path) -> Vec<f32> {
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    lumen.frames(5);
    assert!(lumen.app().audio_export_open());
    lumen.app_mut().answer_next_dialog(vec![path.to_owned()]);
    lumen.click("Export audio…");
    lumen.wait_for("the export", |app| !app.audio_export_busy());
    assert_eq!(lumen.app().audio_export_error(), None);
    let audio = lumen_media::decode(path, &AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|e| panic!("{} does not decode: {e}", path.display()));
    assert_eq!(audio.sample_rate, 48_000);
    audio.channel(0).to_vec()
}

#[test]
fn the_export_of_a_mapped_track_is_what_the_test_device_played() {
    let mut lumen = launch("drums-export", &mapped_song());
    lumen.frames(30);
    let heard = onsets(hear(&mut lumen));
    assert_eq!(heard.len(), 4, "four hits heard: {heard:?}");
    for gap in gaps(&heard) {
        assert!((gap - 0.5).abs() < 0.02, "on the beat: {:?}", gaps(&heard));
    }
    let song = lumen.dir().join("song.wav");
    let exported = onsets(export(&mut lumen, &song).into_iter().take(LISTEN));
    assert_eq!(exported.len(), heard.len(), "the same hits: {exported:?}");
    for (heard, exported) in gaps(&heard).into_iter().zip(gaps(&exported)) {
        assert!(
            (heard - exported).abs() < 0.002,
            "a gap of {heard} s heard and {exported} s exported"
        );
    }
}

#[test]
fn a_clip_converts_to_another_map_from_its_menu_and_keeps_every_hits_sound() {
    let mut project = mapped_song();
    let mut shifted = Preset::GENERAL_MIDI.map(Id(501));
    shifted.name = "GM up an octave".into();
    for sound in &mut shifted.sounds {
        sound.input += 12;
        sound.output += 12;
    }
    project.drum_maps.push(shifted);
    let mut lumen = launch("drums-convert", &project);
    lumen.frames(30);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    lumen.frames(3);
    let ctx = lumen.harness.ctx.clone();
    let rect = lumen
        .app()
        .clip_rect(&ctx, clip)
        .expect("the clip is in view");
    lumen.right_click(rect.center());
    assert!(
        lumen.shows("Convert drum map…"),
        "the clip's menu offers it"
    );
    lumen.click("Convert drum map…");
    assert!(
        lumen.shows("GM up an octave (47 sounds)"),
        "the chooser lists the maps"
    );
    lumen.click("GM up an octave (47 sounds)");
    lumen.frames(5);
    let project = lumen.app().project();
    assert_eq!(project.tracks[0].drum_map, Some(Id(501)));
    let notes = notes_of(&lumen);
    assert_eq!(
        notes.iter().map(|n| n.pitch()).collect::<Vec<_>>(),
        vec![48, 50, 54, 48],
        "each hit is the same sound, an octave up"
    );
    assert_eq!(lumen.app().undo_label(), Some("Convert drum map"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(
        notes_of(&lumen)
            .iter()
            .map(|n| n.pitch())
            .collect::<Vec<_>>(),
        vec![36, 38, 42, 36],
        "one undo gives the notes back"
    );
    assert_eq!(lumen.app().project().tracks[0].drum_map, Some(Id(500)));
}

fn type_text(lumen: &mut Lumen, text: &str) {
    lumen.harness.event(Event::Text(text.to_owned()));
    lumen.frames(2);
}

fn replace_text(lumen: &mut Lumen, text: &str) {
    lumen.key(Modifiers::COMMAND, Key::A);
    type_text(lumen, text);
}

fn map_of(lumen: &Lumen, id: Id) -> lumen_project::model::DrumMap {
    lumen
        .app()
        .project()
        .drum_map(id)
        .cloned()
        .expect("the map is in the project")
}

fn editor(name: &str) -> (Lumen, Id) {
    let mut lumen = launch(name, &mapped_song());
    lumen.harness.get_by_label_contains("Track Drums,").click();
    lumen.frames(3);
    palette(&mut lumen, "Drum map");
    assert!(lumen.app().drum_map_editor_open());
    (lumen, Id(500))
}

fn drum_map_menu(lumen: &mut Lumen, row: &str) {
    lumen
        .harness
        .get_by_label_contains("Track Drums,")
        .click_secondary();
    lumen.frames(3);
    lumen.click("Drum map");
    lumen.frames(2);
    lumen.click(row);
    lumen.frames(3);
}

#[test]
fn a_row_is_edited_from_the_keyboard_alone() {
    let (mut lumen, map) = editor("drums-keyboard");
    lumen.frames(5);

    let focused = lumen.harness.ctx.memory(|m| m.focused());
    assert!(focused.is_some(), "a field has the keyboard when it opens");
    replace_text(&mut lumen, "Bass drum");

    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert_eq!(
        map_of(&lumen, map).sound(35).map(|s| s.name.clone()),
        Some("Bass drum".to_owned()),
        "the name changed"
    );
    assert_eq!(
        lumen.app().undo_label(),
        Some("Rename the drum sound"),
        "one undo step"
    );

    replace_text(&mut lumen, "90");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert!(map_of(&lumen, map).sound(35).is_none(), "it left note 35");
    assert_eq!(
        map_of(&lumen, map).sound(90).map(|s| s.name.clone()),
        Some("Bass drum".to_owned())
    );
    assert_eq!(
        lumen.app().undo_label(),
        Some("Change the drum sound's note"),
        "one undo step"
    );

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(map_of(&lumen, map).sound(35).is_some());
    assert!(map_of(&lumen, map).sound(90).is_none());
}

#[test]
fn the_icon_grid_opens_and_chooses_from_the_keyboard() {
    let (mut lumen, map) = editor("drums-icons");
    lumen.frames(5);

    lumen.harness.get_by_label("Bass Drum 1 icon: Kick").focus();
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(3);
    assert!(
        lumen.shows("Closed hat"),
        "the grid is open with every glyph named"
    );
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert_eq!(
        map_of(&lumen, map).sound(36).map(|s| s.icon),
        Some(lumen_project::model::DrumIcon::Snare),
        "one step right of the kick is the snare"
    );
    assert_eq!(
        lumen.app().undo_label(),
        Some("Change the drum sound's icon")
    );
}

#[test]
fn an_exported_map_imports_back_into_the_project() {
    let (mut lumen, map) = editor("drums-export-import");
    lumen.frames(5);

    replace_text(&mut lumen, "Kit on the desk");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    let path = lumen.dir().join("Desk kit.lumendrums");
    lumen.app_mut().answer_next_dialog(vec![path.clone()]);
    drum_map_menu(&mut lumen, "Export the drum map…");
    lumen.wait_for("the export", |app| {
        app.drum_map_notice()
            .is_some_and(|notice| notice.contains("Exported to"))
    });
    let bytes = std::fs::read(&path).expect("the file was written");
    let text = String::from_utf8(bytes).expect("text");
    assert!(text.contains("\"version\": 1"), "versioned: {text}");
    assert!(text.contains("Kit on the desk"), "the name is in it");

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(
        map_of(&lumen, map).sound(35).map(|s| s.name.clone()),
        Some("Acoustic Bass Drum".to_owned())
    );
    lumen.app_mut().answer_next_dialog(vec![path]);
    drum_map_menu(&mut lumen, "Import a drum map…");
    lumen.wait_for("the import", |app| {
        app.drum_map_notice()
            .is_some_and(|notice| notice.contains("imported from"))
    });
    let project = lumen.app().project();
    assert_eq!(project.drum_maps.len(), 2, "the import joined the project");
    let imported = project.drum_maps[1].clone();
    assert_eq!(
        imported.sound(35).map(|s| s.name.clone()),
        Some("Kit on the desk".to_owned()),
        "the file's map came back as it was saved"
    );
    assert_eq!(
        project.tracks[0].drum_map,
        Some(imported.id),
        "the selected track took it"
    );
    assert_eq!(lumen.app().undo_label(), Some("Import a drum map"));
}

#[test]
fn a_damaged_file_is_refused_and_its_message_is_read_in_full() {
    let (mut lumen, map) = editor("drums-damaged");
    lumen.frames(5);
    let path = lumen.dir().join("Broken.lumendrums");
    std::fs::write(&path, b"{ \"version\": 1, \"name\": ").expect("the damaged file");
    lumen.app_mut().answer_next_dialog(vec![path]);
    drum_map_menu(&mut lumen, "Import a drum map…");
    lumen.wait_for("the refusal", |app| {
        app.drum_map_notice()
            .is_some_and(|notice| notice.contains("is not a drum map"))
    });
    let notice = lumen.app().drum_map_notice().unwrap().to_owned();
    assert!(
        notice.starts_with("Broken.lumendrums is not a drum map"),
        "{notice}"
    );
    assert!(
        notice.contains("can read:"),
        "the reader's words follow: {notice}"
    );
    assert!(!notice.ends_with('\u{2026}'), "in full: {notice}");
    assert!(lumen.shows(&notice), "the window shows it whole");
    assert_eq!(
        lumen.app().project().drum_maps.len(),
        1,
        "nothing was added"
    );
    assert_eq!(lumen.app().project().tracks[0].drum_map, Some(map));
}

#[test]
fn hide_unmapped_keys_folds_the_roll_to_the_maps_sounds() {
    let (mut lumen, _map) = editor("drums-fold");
    lumen.frames(5);

    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    lumen.frames(3);
    let ctx = lumen.harness.ctx.clone();
    let rect = lumen
        .app()
        .clip_rect(&ctx, clip)
        .expect("the clip is in view");
    lumen.double_click(rect.center());
    lumen.frames(6);

    assert_eq!(lumen.app().drum_view_shown(), Some("steps"));
    lumen.click("Lanes view");
    lumen.frames(6);
    assert_eq!(lumen.app().drum_view_shown(), Some("lanes"));
    assert_eq!(
        lumen.app().note_editor_rows(),
        Some(47),
        "the kit's lanes first"
    );
    lumen.click("Keys view");
    lumen.frames(6);
    assert_eq!(
        lumen.app().note_editor_rows(),
        Some(128),
        "the keyboard in the piano roll"
    );

    lumen.click("Hide unmapped keys");
    lumen.frames(4);
    assert!(lumen.app().hiding_unmapped_keys());
    assert_eq!(
        lumen.app().note_editor_rows(),
        Some(47),
        "the General MIDI kit's rows"
    );

    lumen.click("Hide unmapped keys");
    lumen.frames(4);
    assert!(!lumen.app().hiding_unmapped_keys());
    assert_eq!(lumen.app().note_editor_rows(), Some(128));
}

#[test]
fn learn_hears_a_note_from_the_computer_keyboard() {
    let (mut lumen, map) = editor("drums-learn-keys");
    lumen.frames(5);
    lumen.click("Computer keyboard plays notes");
    lumen.frames(3);
    assert!(lumen.app().computer_keyboard_on());
    lumen
        .harness
        .query_all_by_label_contains("Learn the key you play")
        .next()
        .expect("a Learn button")
        .click();
    lumen.frames(3);
    assert_eq!(lumen.app().drum_map_learning(), Some(35));
    assert_eq!(lumen.app().drum_map_learning_column(), Some("input"));
    assert!(
        !lumen.app().live_notes_play_as_they_are(),
        "input Learn leaves the map in place"
    );
    lumen.key(Modifiers::NONE, Key::X);
    lumen.key(Modifiers::NONE, Key::X);
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(3);
    assert_eq!(lumen.app().drum_map_learning(), None, "learning ended");
    assert!(map_of(&lumen, map).sound(35).is_none(), "it left note 35");
    assert!(
        map_of(&lumen, map).sound(84).is_some(),
        "the sound answers to the note the keyboard played"
    );
}

fn learn_song() -> Project {
    let mut project = mapped_song();
    let mut map = Preset::GENERAL_MIDI.map(Id(500));
    for sound in &mut map.sounds {
        match sound.input {
            35 => sound.output = 48,
            60 => sound.output = 72,
            _ => {}
        }
    }
    project.drum_maps = vec![map];
    project
}

fn learn_editor(name: &str) -> (Lumen, Id) {
    let mut lumen = launch(name, &learn_song());
    lumen.harness.get_by_label_contains("Track Drums,").click();
    lumen.frames(3);
    palette(&mut lumen, "Drum map");
    assert!(lumen.app().drum_map_editor_open());
    lumen.frames(5);
    lumen.click("Computer keyboard plays notes");
    lumen.frames(3);
    assert!(lumen.app().computer_keyboard_on());
    (lumen, Id(500))
}

fn press_learn(lumen: &mut Lumen, which: &str) {
    lumen
        .harness
        .query_all_by_label_contains(which)
        .next()
        .expect("a Learn button")
        .click();
    lumen.frames(3);
}

fn learn_output(lumen: &mut Lumen) {
    press_learn(lumen, "Learn the instrument's note");
}

fn learn_input(lumen: &mut Lumen) {
    press_learn(lumen, "Learn the key you play");
}

const PITCH_WINDOW: usize = 4_800;

fn onset(left: &[f32]) -> Option<usize> {
    left.iter().position(|s| s.abs() > 0.02)
}

fn press_and_hold(lumen: &mut Lumen, key: Key, pressed: bool) {
    lumen.harness.event(Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    lumen.harness.step();
}

fn played_pitch(lumen: &mut Lumen, key: Key) -> f64 {
    let heard = lumen.listen_until(
        |l| press_and_hold(l, key, true),
        |heard| {
            let left: Vec<f32> = heard.iter().map(|f| f[0]).collect();
            onset(&left).is_some_and(|at| left.len() >= at + PITCH_WINDOW)
        },
    );
    press_and_hold(lumen, key, false);
    lumen.frames(3);
    let left: Vec<f32> = heard.iter().map(|f| f[0]).collect();
    let onset = onset(&left).expect("the note is heard");
    let window = &left[onset..onset + PITCH_WINDOW];
    let crossings = window
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    crossings as f64 / (window.len() as f64 / RATE)
}

fn octave_apart(low: f64, high: f64) -> bool {
    let ratio = high / low;
    (1.8..2.2).contains(&ratio)
}

fn same_pitch(a: f64, b: f64) -> bool {
    (a / b - 1.0).abs() < 0.08
}

#[test]
fn output_learn_from_the_computer_keyboard_sets_the_output_and_leaves_the_input() {
    let (mut lumen, map) = learn_editor("drums-learn-out");
    learn_output(&mut lumen);
    assert_eq!(lumen.app().drum_map_learning(), Some(35));
    assert_eq!(lumen.app().drum_map_learning_column(), Some("output"));
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(3);
    assert_eq!(lumen.app().drum_map_learning(), None, "learning ended");
    let sound = map_of(&lumen, map).sound(35).cloned().expect("still on 35");
    assert_eq!(
        sound.output, 60,
        "the output is the key the keyboard played"
    );
    assert_eq!(sound.input, 35, "the input did not move");
    assert_eq!(
        lumen.app().undo_label(),
        Some("Learn the drum sound's output note"),
        "one undo step"
    );
    let note = lumen.app().note().expect("a toast says what changed");
    assert!(
        note.contains("now sends note 60") && note.contains("instead of 48"),
        "{note}"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(
        map_of(&lumen, map).sound(35).map(|s| (s.input, s.output)),
        Some((35, 48)),
        "one undo gave the output back"
    );
}

#[test]
fn while_output_learn_listens_the_instrument_gets_the_key_as_played() {
    let (mut lumen, _map) = learn_editor("drums-learn-raw");
    let mapped = played_pitch(&mut lumen, Key::A);
    learn_output(&mut lumen);
    lumen.wait_for("the bypass", |app| app.live_notes_play_as_they_are());
    let raw = played_pitch(&mut lumen, Key::A);
    assert!(
        octave_apart(raw, mapped),
        "the map sends 60 to note 72, an octave above the note as played: {mapped} Hz mapped, {raw} Hz raw"
    );
    assert!(
        lumen.app().computer_keyboard_held().is_empty(),
        "no note is left held"
    );
    lumen.wait_for("the map back", |app| !app.live_notes_play_as_they_are());
    let after = played_pitch(&mut lumen, Key::A);
    assert!(
        same_pitch(after, mapped),
        "learning ended, so the map answers again: {after} Hz against {mapped} Hz"
    );
}

#[test]
fn a_key_that_already_is_the_output_keeps_output_learn_listening() {
    let (mut lumen, map) = learn_editor("drums-learn-same");
    learn_output(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Z);
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(3);
    assert_eq!(
        lumen.app().drum_map_learning_column(),
        Some("output"),
        "note 48 is already the output, so it keeps listening"
    );
    let note = lumen.app().note().expect("a toast says why");
    assert!(note.contains("already sends note 48"), "{note}");
    assert_eq!(map_of(&lumen, map).sound(35).map(|s| s.output), Some(48));
    assert_eq!(lumen.app().undo_label(), None, "nothing was changed");
}

#[test]
fn the_two_learns_switch_and_a_second_press_of_the_same_one_stops_it() {
    let (mut lumen, map) = learn_editor("drums-learn-switch");
    learn_input(&mut lumen);
    assert_eq!(lumen.app().drum_map_learning_column(), Some("input"));
    assert!(!lumen.app().live_notes_play_as_they_are());
    learn_output(&mut lumen);
    assert_eq!(
        lumen.app().drum_map_learning_column(),
        Some("output"),
        "the output Learn took over from the input"
    );
    lumen.wait_for("the bypass", |app| app.live_notes_play_as_they_are());
    learn_input(&mut lumen);
    assert_eq!(lumen.app().drum_map_learning_column(), Some("input"));
    lumen.wait_for("the map back", |app| !app.live_notes_play_as_they_are());
    learn_input(&mut lumen);
    assert_eq!(
        lumen.app().drum_map_learning(),
        None,
        "the same press stops it"
    );
    learn_output(&mut lumen);
    learn_output(&mut lumen);
    assert_eq!(lumen.app().drum_map_learning(), None);
    assert_eq!(map_of(&lumen, map).sound(35).map(|s| s.output), Some(48));
    assert_eq!(
        lumen.app().undo_label(),
        None,
        "learning alone changes nothing"
    );
}

#[test]
fn escape_stops_either_learn_and_leaves_the_window_and_the_keyboard() {
    let (mut lumen, map) = learn_editor("drums-learn-esc");
    for learn in [learn_input as fn(&mut Lumen), learn_output] {
        learn(&mut lumen);
        assert!(lumen.app().drum_map_learning().is_some());
        lumen.key(Modifiers::NONE, Key::Escape);
        lumen.frames(3);
        assert_eq!(lumen.app().drum_map_learning(), None, "Esc stopped it");
        assert!(lumen.app().drum_map_editor_open(), "the window stays");
        assert!(lumen.app().computer_keyboard_on(), "the keyboard stays on");
        lumen.wait_for("the map back", |app| !app.live_notes_play_as_they_are());
        assert_eq!(map_of(&lumen, map).sound(35).map(|s| s.output), Some(48));
    }
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(
        !lumen.app().computer_keyboard_on(),
        "Esc with nothing to stop leaves the keyboard as before"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(
        !lumen.app().drum_map_editor_open(),
        "and the next Esc closes the window as before"
    );
}
