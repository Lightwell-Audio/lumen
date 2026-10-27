//! The Project media tab (M9 slice R4b, ADR-0106), end to end: a project
//! opened with its audio moved says so and finds the files again by
//! searching a folder; entries nothing plays are removed as one undo step;
//! files that belong to no project are listed, and clearing asks twice.
//!
//! Nothing here moves a file to the real trash: that step is tested in the
//! unit tests with a mover that only records.

use std::path::PathBuf;

use eframe::egui::{Key, Modifiers};
use lumen_app::testing::project_file;
use lumen_project::model::{Color, Id, Media, Track, TrackKind};
use lumen_project::testing::{TempDir, project_of};

use super::copies::{command, with_outside_audio};
use super::{Lumen, wav};

/// A saved project whose one clip plays `kick.wav`, which is then moved
/// into a folder inside `Moved`. Returns the project file and the new place.
fn project_with_moved_audio(name: &str) -> (TempDir, PathBuf, PathBuf) {
    let (mut lumen, kick) = with_outside_audio(name);
    lumen.app_mut().set_copy_imported(false);
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let moved = lumen
        .dir()
        .join("Moved")
        .join("Old samples")
        .join("kick.wav");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&kick, &moved).unwrap();
    (lumen.crash(), path, moved)
}

#[test]
fn a_project_opened_with_its_audio_moved_finds_it_again_in_a_folder() {
    let (dir, project, moved) = project_with_moved_audio("pool-relink");
    let mut lumen = Lumen::start_with(dir, |l| l.opening(project));
    lumen.wait_for("the project", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
    lumen.frames(10);

    // The banner says what is wrong and offers the way out.
    assert_eq!(lumen.app().missing_media_count(), Some(1));
    assert_eq!(
        lumen.app().file_notice(),
        None,
        "its own banner, not a toast"
    );
    assert!(lumen.shows("Relink\u{2026}"));
    lumen.click("Relink\u{2026}");
    assert!(lumen.app().media_tab_shown());
    assert_eq!(
        lumen.app().missing_media_count(),
        None,
        "the banner is done"
    );
    let rows = lumen.app().media_rows();
    assert_eq!(rows, [("kick".to_owned(), true, 1)]);

    // Searching a folder finds it by name, deep inside, and changes nothing
    // until the person says so.
    let folder = moved.ancestors().nth(2).unwrap().to_path_buf();
    lumen.app_mut().answer_next_dialog(vec![folder]);
    lumen.click("Search a folder\u{2026}");
    lumen.wait_for("the search", |app| {
        !app.media_searching() && app.media_found().is_some()
    });
    lumen.frames(3);
    assert_eq!(lumen.app().media_found(), Some(1));
    assert!(
        lumen.app().media_rows()[0].1,
        "still missing until it is applied"
    );
    assert!(!lumen.app().is_dirty());

    lumen.click("Relink 1");
    assert!(!lumen.app().media_rows()[0].1, "found");
    assert!(lumen.app().is_dirty(), "the next save writes the new place");
    assert!(
        lumen
            .app()
            .media_note()
            .is_some_and(|n| n.starts_with("Relinked 1 file")),
        "{:?}",
        lumen.app().media_note()
    );
    let id = lumen.app().media_id("kick").unwrap();
    lumen.wait_for("the clip to sound again", |app| app.has_waveform(id));
    assert_eq!(lumen.app().undo_label(), None, "relinking is not an edit");

    // Saved, it opens whole next time.
    lumen.app_mut().set_copy_imported(false);
    lumen.key(Modifiers::COMMAND, Key::S);
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let dir = lumen.crash();
    let project = project_file(dir.path(), "Song");
    let mut again = Lumen::start_with(dir, |l| l.opening(project));
    again.wait_for("the project", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
    again.frames(10);
    assert_eq!(again.app().missing_media_count(), None);
}

#[test]
fn a_file_chosen_for_a_missing_one_is_checked_and_a_wrong_one_is_refused() {
    let (dir, project, moved) = project_with_moved_audio("pool-locate");
    let mut lumen = Lumen::start_with(dir, |l| l.opening(project));
    lumen.wait_for("the project", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
    lumen.frames(10);
    command(&mut lumen, "Project media");
    lumen.click("kick, missing");

    // A mono file at another rate cannot stand in for a stereo one.
    let other = mono_wav(&lumen.dir().join("other.wav"));
    lumen.app_mut().answer_next_dialog(vec![other]);
    command(&mut lumen, "Locate missing media");
    lumen.wait_for("the answer", |app| app.media_note().is_some());
    assert!(
        lumen
            .app()
            .media_note()
            .is_some_and(|n| n.starts_with("That file will not do")),
        "{:?}",
        lumen.app().media_note()
    );
    assert!(lumen.app().media_rows()[0].1, "still missing");

    // The same file, where it went, is taken at once.
    lumen.app_mut().answer_next_dialog(vec![moved]);
    command(&mut lumen, "Locate missing media");
    lumen.wait_for("the relink", |app| !app.media_rows()[0].1);
    assert!(lumen.app().is_dirty());
}

#[test]
fn unused_entries_are_removed_as_one_step_and_undo_brings_them_back() {
    let dir = TempDir::new("e2e-pool-unused");
    let mut project = project_of(
        "Song",
        vec![Track::new(
            Id(1),
            "Drums",
            TrackKind::Audio,
            Color(0x4F81BD),
        )],
    );
    project.media = [900u64, 901]
        .into_iter()
        .map(|id| Media {
            id: Id(id),
            path: format!("Samples/Imported/old {id}.wav"),
            original_path: None,
            frames: 48_000,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 1,
        })
        .collect();
    let path = project_file(dir.path(), "Song");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    lumen_project::save(&project, &path, &Default::default()).unwrap();
    let mut lumen = Lumen::start_with(dir, |l| l.opening(path));
    lumen.wait_for("the project", |app| {
        !app.is_busy() && app.project().media.len() == 2
    });
    lumen.frames(10);
    command(&mut lumen, "Project media");
    assert_eq!(lumen.app().media_rows().len(), 2);

    lumen.click("Remove 2 unused");
    assert!(lumen.app().project().media.is_empty());
    assert_eq!(lumen.app().undo_label(), Some("Remove unused files"));
    assert!(
        lumen
            .app()
            .media_note()
            .is_some_and(|n| n.contains("Undo brings them back")),
        "{:?}",
        lumen.app().media_note()
    );

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert_eq!(lumen.app().project().media.len(), 2);
    assert_eq!(lumen.app().media_rows().len(), 2, "the tab follows");
}

#[test]
fn leftover_files_are_listed_and_clearing_asks_twice() {
    let dir = TempDir::new("e2e-pool-strays");
    let project = project_of(
        "Song",
        vec![Track::new(
            Id(1),
            "Drums",
            TrackKind::Audio,
            Color(0x4F81BD),
        )],
    );
    let path = project_file(dir.path(), "Song");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    lumen_project::save(&project, &path, &Default::default()).unwrap();
    let samples = path.parent().unwrap().join("Samples").join("Imported");
    std::fs::create_dir_all(&samples).unwrap();
    let samples = path.parent().unwrap().join("Samples").join("Imported");
    std::fs::create_dir_all(&samples).unwrap();
    let orphan = wav(&samples.join("orphan.wav"), 1.0);
    let mut lumen = Lumen::start_with(dir, |l| l.opening(path));
    lumen.wait_for("the project", |app| !app.is_busy());
    command(&mut lumen, "Project media");
    lumen.wait_for("the scan", |app| app.media_strays() == 1);
    lumen.frames(5);

    assert!(lumen.shows("Clear\u{2026}"));
    lumen.click("Clear\u{2026}");
    assert!(lumen.app().media_clear_armed(), "the first press only asks");
    assert!(lumen.shows("Move 1 file to the trash"));
    assert!(orphan.is_file(), "nothing moved yet");

    lumen.click("Cancel");
    assert!(!lumen.app().media_clear_armed());
    assert!(lumen.shows("Clear\u{2026}"));
    assert!(orphan.is_file());
}

/// A mono WAV of a second at 44.1 kHz, which no stereo 48 kHz file's
/// place can be taken by.
fn mono_wav(path: &std::path::Path) -> PathBuf {
    let data = vec![0u8; 44_100 * 2];
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(44_100_u32.to_le_bytes());
    out.extend((44_100_u32 * 2).to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the mono WAV");
    path.to_path_buf()
}
