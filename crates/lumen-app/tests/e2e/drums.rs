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

use eframe::egui::{Key, Modifiers};
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
    project_of(
        "Drums",
        vec![midi_track(
            1,
            "Drums",
            "pluck",
            (0, 16),
            Some(1),
            vec![note(0, 1, 36)],
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
fn the_palette_attaches_a_kit_the_track_plays_it_and_a_muted_sound_is_heard_gone() {
    let mut lumen = launch("drums-attach-mute", &kick_track());
    lumen.harness.get_by_label_contains("Track Drums,").click();
    lumen.frames(3);
    play(&mut lumen);
    let before = loudest(&mut lumen);
    assert!(before > 0.01, "the mapped track sounds: {before}");
    let notes = notes_of(&lumen);

    // The palette's "Drum map…" opens the chooser; its General MIDI row gives
    // the track a copy of that kit.
    palette(&mut lumen, "Drum map");
    assert!(lumen.shows("General MIDI"), "the chooser lists the kits");
    lumen.click("General MIDI");
    lumen.frames(5);
    let project = lumen.app().project();
    let map = project.drum_maps[0].clone();
    assert_eq!(map.name, "General MIDI");
    assert_eq!(project.tracks[0].drum_map, Some(map.id));
    assert_eq!(map.sounds.len(), 47);
    assert_eq!(notes_of(&lumen), notes, "the notes never changed");
    assert_eq!(
        lumen.app().undo_label(),
        Some("Attach drum map"),
        "one undo step"
    );

    // Muting the kick (D1's editor row, driven through the document) silences
    // the track: nothing new sounds, and the note that played is let go.
    lumen.app_mut().mute_drum_sound(map.id, 36, true);
    lumen.frames(30);
    let after = loudest(&mut lumen);
    assert!(after < 1e-4, "the muted sound is gone: {after}");

    // Undo brings it back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(30);
    let back = loudest(&mut lumen);
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
