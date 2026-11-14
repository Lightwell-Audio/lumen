//! Drum maps (M8 slice D0): a MIDI track takes a kit and plays through it,
//! a muted sound is heard gone, the export of a mapped track is what the test
//! device played, and converting a clip's notes to another map keeps every
//! hit's sound.
//!
//! D0 ships no map editor: muting a sound, and the second map the conversion
//! goes to, are made through the document the way D1's editor will, and the
//! rest is driven as a user does (the palette, the menus and their rows).

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
/// Frames listened to and exported: four seconds of the song.
const LISTEN: usize = 192_000;

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

fn note(start: i64, length: i64, pitch: u8) -> Note {
    Note::new(q(start), q(length), pitch, 100)
}

/// Starts Lumen with `project` open.
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

/// A track with a kick on every beat, on the built-in synth.
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

/// The loudest sample heard over a stretch, after letting it settle.
fn loudest(lumen: &mut Lumen) -> f32 {
    let heard = lumen.listen(Duration::from_millis(100), 2_000);
    heard
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

/// Waits until what the device plays is under `quiet`, and gives the level it
/// reached: a note that ends and whose tail dies, waited for as a condition,
/// never a count of frames (M8 D0 review L10).
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

/// Waits until what the device plays is over `loud`, and gives the level it
/// reached.
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

/// The clip's notes as the project holds them.
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

    // The palette's "Drum map…" opens the map editor; a track without a map
    // gets the list of kits first, and the 808-style kit is picked there.
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

    // The map list offers the kits, this project's maps and the user's
    // files; the map the track reads is in it.
    lumen.click("Map: 808 style");
    lumen.frames(3);
    assert!(
        lumen.shows("808 style (16 sounds)"),
        "the project's map is listed"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);

    // Muting the kick in its row takes the track quiet.
    lumen.click("Mute Kick");
    let after = until_quiet(&mut lumen, 1e-4, "the muted sound to go");
    assert!(after < 1e-4, "the muted sound is gone: {after}");
    assert_eq!(lumen.app().undo_label(), Some("Mute drum sound"));

    // Undo brings it back: the kick is heard again as the loop plays on.
    lumen.key(Modifiers::COMMAND, Key::Z);
    let back = until_loud(&mut lumen, 0.01, "the undone sound");
    assert!(back > 0.01, "undone, it sounds again: {back}");
    stop(&mut lumen);
}

/// A track of four mapped notes, with General MIDI attached in the saved file.
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

/// Where in the left channel a hit starts: the first frame over 0.1 after at
/// least a quarter of a second of quiet.
fn onsets(left: impl IntoIterator<Item = f32>) -> Vec<usize> {
    let mut found: Vec<usize> = Vec::new();
    for (i, s) in left.into_iter().enumerate() {
        if s.abs() > 0.1 && found.last().is_none_or(|last| i - last > 12_000) {
            found.push(i);
        }
    }
    found
}

/// The seconds between each hit and the next.
fn gaps(onsets: &[usize]) -> Vec<f64> {
    onsets
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 / RATE)
        .collect()
}

/// Plays from the start for [`LISTEN`] frames and gives the left channel.
fn hear(lumen: &mut Lumen) -> Vec<f32> {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);
    let heard = lumen.listen_from(|l| l.key(Modifiers::NONE, Key::Space), LISTEN);
    stop(lumen);
    lumen.frames(5);
    heard.iter().map(|f| f[0]).collect()
}

/// Exports the whole project to `path` and gives its left channel.
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
    // The project holds General MIDI and a copy of it an octave up, as a user
    // map would be; the clips play through the first.
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

/// Types `text` into whatever field has the keyboard, as a person does.
fn type_text(lumen: &mut Lumen, text: &str) {
    lumen.harness.event(Event::Text(text.to_owned()));
    lumen.frames(2);
}

/// Selects all in the focused field and replaces it with `text`.
fn replace_text(lumen: &mut Lumen, text: &str) {
    lumen.key(Modifiers::COMMAND, Key::A);
    type_text(lumen, text);
}

/// The map the project holds under `id`.
fn map_of(lumen: &Lumen, id: Id) -> lumen_project::model::DrumMap {
    lumen
        .app()
        .project()
        .drum_map(id)
        .cloned()
        .expect("the map is in the project")
}

/// The drum map editor of a mapped project, open on its track's map.
fn editor(name: &str) -> (Lumen, Id) {
    let mut lumen = launch(name, &mapped_song());
    lumen.harness.get_by_label_contains("Track Drums,").click();
    lumen.frames(3);
    palette(&mut lumen, "Drum map");
    assert!(lumen.app().drum_map_editor_open());
    (lumen, Id(500))
}

/// A row of the track header's Drum map menu, on the "Drums" track.
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
    // The editor opens with the first row's name field focused: the kick's.
    let focused = lumen.harness.ctx.memory(|m| m.focused());
    assert!(focused.is_some(), "a field has the keyboard when it opens");
    replace_text(&mut lumen, "Bass drum");
    // Enter commits the name and moves to the next field (the input note).
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

    // Type the new note and Enter: the sound answers there.
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
    // Undo gives the note back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(map_of(&lumen, map).sound(35).is_some());
    assert!(map_of(&lumen, map).sound(90).is_none());
}

#[test]
fn the_icon_grid_opens_and_chooses_from_the_keyboard() {
    let (mut lumen, map) = editor("drums-icons");
    lumen.frames(5);
    // The first row's icon button, focused from the keyboard, opens the grid
    // with Space; the arrows move and Enter chooses.
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
    // A name only this project's map has, then the file.
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

    // Undo the rename, so only the file has it; then import it back.
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
    // The window closes, and the kick's clip opens in the clip editor.
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
    // M8 D2/D3: a clip on a mapped track opens in the drum editor, as the
    // step grid when it fits it; the lanes and the piano roll are the other
    // two views.
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
    // The editor's Hide unmapped keys button folds the column to the kit's
    // sounds.
    lumen.click("Hide unmapped keys");
    lumen.frames(4);
    assert!(lumen.app().hiding_unmapped_keys());
    assert_eq!(
        lumen.app().note_editor_rows(),
        Some(47),
        "the General MIDI kit's rows"
    );
    // And again shows every key.
    lumen.click("Hide unmapped keys");
    lumen.frames(4);
    assert!(!lumen.app().hiding_unmapped_keys());
    assert_eq!(lumen.app().note_editor_rows(), Some(128));
}
