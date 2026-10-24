//! M7b E5: Freeze renders a track into a file it plays instead of its clips
//! and devices, makes it read-only with a way back (the Frozen badge), and
//! Flatten turns it into a plain audio track. Each is one undo step.

use std::sync::atomic::AtomicBool;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::model::{ClipContent, Id, Note, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::clip_commands::{audio, command};

fn freeze_selected(lumen: &mut Lumen, track: Id) {
    lumen.app_mut().select_tracks(&[track]);
    command(lumen, "Freeze track");
    lumen.wait_for("the freeze", |app| app.undo_label() == Some("Freeze"));
}

fn keys_project(name: &str) -> (Lumen, std::path::PathBuf) {
    let dir = TempDir::new(name);
    let path = dir.path().join("song.lumen");
    let notes = vec![Note::new(Ticks::ZERO, Ticks::quarters(2), 60, 100)];
    let project = project_of(
        "Keys",
        vec![midi_track(1, "Keys", "poly", (0, 4), None, notes, vec![])],
    );
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let opened = path.clone();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(opened));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    (lumen, path)
}

#[test]
fn freezing_an_audio_track_shows_the_badge_and_undo_gives_it_back() {
    let mut lumen = audio("freeze-audio");
    let before = lumen.app().project().clone();
    let track = before.tracks[0].id;
    let name = before.tracks[0].name.clone();
    freeze_selected(&mut lumen, track);

    let after = lumen.app().project();
    let frozen = after.tracks[0]
        .frozen
        .as_ref()
        .expect("the track is frozen");
    assert_eq!(after.tracks[0].clips, before.tracks[0].clips, "clips kept");
    let file = after
        .media
        .iter()
        .find(|m| m.id == frozen.media)
        .expect("the frozen file is in the project");
    assert!(
        std::path::Path::new(&file.path).starts_with(lumen.dir().join("Renders")),
        "{}",
        file.path
    );
    assert!(frozen.length > Ticks::ZERO);
    lumen.frames(3);
    assert!(
        lumen.shows(&format!("{name} is frozen: unfreeze")),
        "the header says Frozen"
    );

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0], before.tracks[0]);
    assert_eq!(lumen.app().project().media, before.media);
    lumen.frames(3);
    assert!(!lumen.shows(&format!("{name} is frozen: unfreeze")));
}

#[test]
fn a_frozen_track_refuses_edits_and_says_why_and_the_badge_unfreezes_it() {
    let mut lumen = audio("freeze-refuse");
    let track = lumen.app().project().tracks[0].id;
    let name = lumen.app().project().tracks[0].name.clone();
    freeze_selected(&mut lumen, track);
    let frozen = lumen.app().project().clone();

    let clip = frozen.tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(3);
    assert_eq!(lumen.app().project(), &frozen, "nothing was changed");
    let note = lumen.app().note().unwrap_or_default().to_owned();
    assert!(note.contains(&format!("{name} is frozen")), "{note}");
    assert!(note.contains("unfreeze"), "{note}");
    assert_eq!(lumen.app().undo_label(), Some("Freeze"), "no undo step");

    lumen.click(&format!("{name} is frozen: unfreeze"));
    assert!(lumen.app().project().tracks[0].frozen.is_none());
    assert_eq!(lumen.app().undo_label(), Some("Unfreeze"));
    assert_eq!(
        lumen.app().project().tracks[0].clips,
        frozen.tracks[0].clips,
        "the clips are exactly as they were"
    );
    // Free again: the same delete now works.
    lumen.app_mut().select_clip(clip);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert!(lumen.app().project().tracks[0].clips.is_empty());
}

#[test]
fn a_midi_track_freezes_to_sound_and_flattens_to_an_audio_track() {
    let (mut lumen, _) = keys_project("freeze-midi");
    let before = lumen.app().project().clone();
    let track = before.tracks[0].id;
    freeze_selected(&mut lumen, track);

    let media = {
        let project = lumen.app().project();
        let frozen = project.tracks[0].frozen.as_ref().expect("frozen");
        project
            .media
            .iter()
            .find(|m| m.id == frozen.media)
            .expect("the file")
            .clone()
    };
    let audio = lumen_media::decode(
        std::path::Path::new(&media.path),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert!(
        audio.channel(0).iter().any(|v| v.abs() > 0.001),
        "the synth sounds in the file"
    );
    assert_eq!(
        lumen.app().project().tracks[0].devices,
        before.tracks[0].devices,
        "the instrument is kept for unfreezing"
    );

    command(&mut lumen, "Flatten track");
    let flat = lumen.app().project().tracks[0].clone();
    assert_eq!(lumen.app().undo_label(), Some("Flatten"));
    assert!(flat.frozen.is_none());
    assert_eq!(flat.kind, TrackKind::Audio);
    assert!(flat.devices.is_empty());
    assert_eq!(flat.clips.len(), 1);
    let ClipContent::Audio { media: played, .. } = flat.clips[0].content else {
        panic!("an audio clip")
    };
    assert_eq!(played, media.id);

    lumen.key(Modifiers::COMMAND, Key::Z);
    let back = &lumen.app().project().tracks[0];
    assert_eq!(back.kind, TrackKind::Midi, "undo brings the notes back");
    assert!(back.frozen.is_some());
}

#[test]
fn a_frozen_track_is_still_frozen_after_save_and_reopen() {
    use lumen_app::testing::project_file;
    let mut lumen = audio("freeze-save");
    let track = lumen.app().project().tracks[0].id;
    freeze_selected(&mut lumen, track);
    let path = project_file(lumen.dir(), "Frozen");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let folder = path.parent().unwrap().join("Samples").join("Rendered");
    assert!(
        std::fs::read_dir(&folder).is_ok_and(|mut d| d.next().is_some()),
        "the render was copied into {folder:?}"
    );

    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    assert_eq!(again.app().file_notice(), None, "nothing is missing");
    let project = again.app().project();
    let frozen = project.tracks[0].frozen.as_ref().expect("still frozen");
    let file = project
        .media
        .iter()
        .find(|m| m.id == frozen.media)
        .expect("the file is a media entry");
    assert!(file.path.starts_with("Samples/Rendered/"), "{}", file.path);
    assert_eq!(file.original_path, None, "a render is not an import");
}

#[test]
fn nothing_selected_means_nothing_to_freeze() {
    let mut lumen = audio("freeze-nothing");
    let before = lumen.app().project().clone();
    lumen.app_mut().select_tracks(&[]);
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Freeze track".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(5);
    assert_eq!(lumen.app().project(), &before);
}

/// The point of freezing: the plugin's process ends while the track is
/// frozen, and unfreezing starts it again from the state it had.
#[test]
fn a_frozen_plugin_track_runs_no_plugin_and_unfreezing_starts_it_again() {
    use super::hosting::{PLUGIN, instrument_device, launch, timing_sensitive, wait_for_state};
    let _turn = timing_sensitive();
    let mut track = midi_track(
        1,
        "Keys",
        "poly",
        (0, 2),
        None,
        vec![Note::new(Ticks::ZERO, Ticks::quarters(1), 69, 100)],
        vec![],
    );
    track.devices = vec![instrument_device(PLUGIN)];
    let project = project_of("Rings", vec![track]);
    let mut lumen = launch("freeze-plugin", &project, &["Rings"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    let id: Id = lumen.app().project().tracks[0].id;

    freeze_selected(&mut lumen, id);
    lumen.wait_for("the plugin to end", |app| {
        app.device_state(PLUGIN).is_none()
    });
    let project = lumen.app().project();
    let frozen = project.tracks[0].frozen.as_ref().expect("frozen");
    let file = project
        .media
        .iter()
        .find(|m| m.id == frozen.media)
        .expect("the file");
    assert!(
        file.frames > u64::from(file.sample_rate) * 2,
        "the plugin's ring is in the file: {} frames",
        file.frames
    );

    command(&mut lumen, "Unfreeze track");
    wait_for_state(&mut lumen, PLUGIN, "ready");
    assert!(lumen.app().project().tracks[0].frozen.is_none());
}
