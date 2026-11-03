//! The browser's Samples, Projects and Places tabs (M9 R7c, ADR-0116), end
//! to end on the test device: sample folders are walked and searched,
//! choosing a sound plays it (Auto, the owner's P A) on the main pair when
//! there is no cue pair (O A), leaving the file tabs stops it, Enter and a
//! drag add the file to the arrangement, and Places opens folders and goes
//! back up. Nothing here reads the machine's own Music folder: every
//! scenario names its sample folder.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_project::testing::TempDir;

use super::copies::command;
use super::cue::choose_cue_pair;
use super::{Lumen, wav};

/// A sample folder: `Kicks/kick.wav` (1 s), `snare.wav` (2 s), and a text
/// file that is not listed.
fn samples(dir: &Path) -> PathBuf {
    let root = dir.join("My Samples");
    std::fs::create_dir_all(root.join("Kicks")).unwrap();
    wav(&root.join("Kicks").join("kick.wav"), 1.0);
    wav(&root.join("snare.wav"), 2.0);
    std::fs::write(root.join("notes.txt"), "not a sound").unwrap();
    root
}

/// Starts with `samples` as the sample folder and the Samples tab open,
/// once its walk has ended.
fn on_samples(name: &str, adjust: impl FnOnce(lumen_app::Launch) -> lumen_app::Launch) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let root = samples(dir.path());
    let mut lumen = Lumen::start_with(dir, |l| adjust(l.with_sample_folder(&root)));
    command(&mut lumen, "Browse samples");
    assert_eq!(lumen.app().browser_tab_name(), "Samples");
    lumen.run_until("the samples to be listed", |l| {
        !l.app().browser_files_reading() && shows_row(l, "kick, audio file")
    });
    lumen
}

/// Whether a file row whose name starts with `start` is drawn.
fn shows_row(lumen: &Lumen, start: &str) -> bool {
    lumen
        .harness
        .query_all_by_label_contains(start)
        .next()
        .is_some()
}

/// Clicks the file row whose name starts with `start`.
fn click_row(lumen: &mut Lumen, start: &str) {
    lumen.harness.get_by_label_contains(start).click();
    lumen.frames(3);
}

#[test]
fn samples_lists_the_sounds_in_its_folders_and_a_click_plays_one() {
    let mut lumen = on_samples("browse-samples", |l| l);
    assert!(shows_row(&lumen, "snare, audio file"));
    assert!(!shows_row(&lumen, "notes, "), "a text file is not listed");
    assert!(lumen.shows("2 files"), "the count says what is listed");

    // Auto is on: a click plays the sound, on the main pair (no cue pair).
    click_row(&mut lumen, "kick, audio file");
    let kick = lumen
        .app()
        .browser_selected_file()
        .expect("a row is chosen");
    assert!(kick.ends_with("kick.wav"));
    lumen.wait_for("the preview to start", |app| {
        app.previewing_file().as_deref() == Some(kick.as_path())
    });
    let heard = lumen.listen(Duration::from_millis(60), 2_000);
    let loudest = heard.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
    assert!(loudest > 0.01, "the preview is heard, at {loudest}");
    assert!(lumen.app().preview_started_in().is_some());

    // The song's Play does not stop it (O A).
    command(&mut lumen, "Play / Stop");
    lumen.wait_for("the song to play", lumen_app::App::is_playing);
    assert!(lumen.app().previewing_file().is_some());
    command(&mut lumen, "Play / Stop");

    // Leaving the file tabs stops it.
    command(&mut lumen, "Browse devices");
    lumen.wait_for("the preview to stop", |app| app.previewing_file().is_none());
}

#[test]
fn a_preview_ends_by_itself_and_stop_preview_ends_it_sooner() {
    let mut lumen = on_samples("browse-stop", |l| l);
    click_row(&mut lumen, "kick, audio file");
    lumen.wait_for("the preview to start", |app| {
        app.previewing_file().is_some()
    });
    command(&mut lumen, "Stop preview");
    lumen.wait_for("the preview to stop", |app| app.previewing_file().is_none());
    // Played to its end (1 s), it ends by itself.
    command(&mut lumen, "Preview file");
    lumen.wait_for("the preview to start", |app| {
        app.previewing_file().is_some()
    });
    lumen.run_until("the file to end", |l| {
        std::thread::sleep(Duration::from_millis(5));
        l.app().previewing_file().is_none()
    });
    assert_eq!(lumen.app().preview_failure(), None);
}

#[test]
fn with_auto_off_a_click_only_chooses() {
    let mut lumen = on_samples("browse-auto-off", lumen_app::Launch::without_auto_preview);
    assert!(!lumen.app().auto_preview());
    click_row(&mut lumen, "kick, audio file");
    assert!(lumen.app().browser_selected_file().is_some());
    lumen.frames(10);
    assert_eq!(lumen.app().previewing_file(), None);
    // Shift+Enter in the list plays it.
    lumen.key(Modifiers::SHIFT, Key::Enter);
    lumen.wait_for("the preview to start", |app| {
        app.previewing_file().is_some()
    });
}

#[test]
fn search_leaves_the_files_whose_names_hold_every_word() {
    let mut lumen = on_samples("browse-search", |l| l);
    lumen.harness.get_by_label("Search samples").focus();
    lumen.frames(2);
    lumen.harness.event(Event::Text("sna".into()));
    lumen.frames(3);
    assert!(shows_row(&lumen, "snare, audio file"));
    assert!(!shows_row(&lumen, "kick, audio file"));
    assert!(lumen.shows("1 file"));
    lumen.harness.event(Event::Text("zz".into()));
    lumen.frames(3);
    assert!(shows_row(&lumen, "No files match."));
    // Esc clears the words, and every file shows again.
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(shows_row(&lumen, "kick, audio file"));
}

#[test]
fn enter_adds_the_chosen_file_to_the_arrangement() {
    let mut lumen = on_samples("browse-enter", lumen_app::Launch::without_auto_preview);
    click_row(&mut lumen, "snare, audio file");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the file to be added", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1);
    // One undo step takes it away.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the undo", |app| app.project().tracks.is_empty());
}

#[test]
fn a_file_dragged_onto_the_lanes_is_added_there() {
    let mut lumen = on_samples("browse-drag", lumen_app::Launch::without_auto_preview);
    let from = lumen
        .harness
        .get_by_label_contains("kick, audio file")
        .rect()
        .center();
    let to = eframe::egui::pos2(1_100.0, 300.0);
    lumen.drag(from, to, Modifiers::NONE);
    lumen.wait_for("the file to be added", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
}

#[test]
fn places_opens_a_folder_and_goes_back_up() {
    let dir = TempDir::new("e2e-browse-places");
    let root = samples(dir.path());
    let mut lumen = Lumen::start_with(dir, |l| l.with_place(&root).without_auto_preview());
    command(&mut lumen, "Browse places");
    lumen.frames(5);
    assert!(shows_row(&lumen, "My Samples, folder"));
    assert!(shows_row(&lumen, "Computer, folder"));

    let place = lumen
        .harness
        .get_by_label_contains("My Samples, folder")
        .rect();
    lumen.double_click(place.center());
    lumen.run_until("the folder to be read", |l| {
        !l.app().browser_files_reading() && shows_row(l, "snare, audio file")
    });
    // Folders first, then files.
    assert!(shows_row(&lumen, "Kicks, folder"));
    let kicks = lumen.harness.get_by_label_contains("Kicks, folder").rect();
    lumen.double_click(kicks.center());
    lumen.run_until("the inner folder", |l| shows_row(l, "kick, audio file"));

    // Backspace goes up, to the folder came from, then to the top.
    lumen.key(Modifiers::NONE, Key::Backspace);
    lumen.run_until("the place again", |l| shows_row(l, "snare, audio file"));
    lumen.key(Modifiers::NONE, Key::Backspace);
    lumen.run_until("the top", |l| shows_row(l, "Computer, folder"));
}

#[test]
fn add_a_place_keeps_the_chosen_folder() {
    let mut lumen = Lumen::start("browse-add-place");
    let folder = lumen.dir().join("Loops");
    std::fs::create_dir_all(&folder).unwrap();
    lumen.app_mut().answer_next_dialog(vec![folder.clone()]);
    command(&mut lumen, "Add a place");
    lumen.wait_for("the place", |app| app.browser_places().len() == 1);
    assert_eq!(lumen.app().browser_tab_name(), "Places");
    let (name, path) = lumen.app().browser_places().remove(0);
    assert_eq!(name, "Loops");
    assert_eq!(PathBuf::from(path), folder);
    lumen.frames(5);
    assert!(shows_row(&lumen, "Loops, folder"));
}

#[test]
fn add_a_sample_folder_lists_its_sounds() {
    let mut lumen = Lumen::start("browse-add-samples");
    let root = samples(lumen.dir());
    lumen.app_mut().answer_next_dialog(vec![root.clone()]);
    command(&mut lumen, "Add a sample folder");
    lumen.wait_for("the folder", |app| app.browser_sample_folders().len() == 1);
    lumen.run_until("the samples to be listed", |l| {
        !l.app().browser_files_reading() && shows_row(l, "kick, audio file")
    });
}

#[test]
fn with_a_cue_pair_the_preview_plays_there_and_not_on_the_main_pair() {
    let mut lumen = on_samples("browse-cue", |l| l);
    choose_cue_pair(&mut lumen);
    command(&mut lumen, "Browse samples");
    click_row(&mut lumen, "snare, audio file");
    lumen.wait_for("the preview to start", |app| {
        app.previewing_file().is_some()
    });
    let heard = lumen.listen(Duration::from_millis(60), 2_000);
    let (main, cue) = heard.iter().fold((0.0f32, 0.0f32), |(m, c), f| {
        (m.max(f[0].abs()), c.max(f[2].abs()))
    });
    assert!(cue > 0.01, "heard on the cue pair, at {cue}");
    assert!(main < 1e-6, "not on the main pair, at {main}");
}

#[test]
fn a_place_that_is_not_there_says_so_and_a_file_that_cannot_be_read_is_not_played() {
    let dir = TempDir::new("e2e-browse-missing");
    let gone = dir.path().join("Unplugged drive");
    let root = samples(dir.path());
    std::fs::write(root.join("broken.wav"), b"RIFF but not a sound").unwrap();
    let mut lumen = Lumen::start_with(dir, |l| l.with_place(&gone).with_sample_folder(&root));
    command(&mut lumen, "Browse places");
    lumen.run_until("the place checked", |l| {
        shows_row(l, "Unplugged drive, place, not there")
    });

    command(&mut lumen, "Browse samples");
    lumen.run_until("the broken file's picture", |l| {
        shows_row(l, "broken, audio file, cannot be read")
    });
    click_row(&mut lumen, "broken, audio file");
    lumen.frames(10);
    assert_eq!(lumen.app().previewing_file(), None, "it is not offered");
}

#[test]
fn the_keyboard_alone_chooses_previews_stops_and_adds() {
    let mut lumen = on_samples("browse-keys", |l| l);
    // The list takes the keys once the pane has them (a click on a row),
    // then only keys are used.
    click_row(&mut lumen, "kick, audio file");
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    lumen.wait_for("the next sound to play", |app| {
        app.previewing_file()
            .is_some_and(|p| p.ends_with("snare.wav"))
    });
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.wait_for("the preview to stop", |app| app.previewing_file().is_none());
    lumen.key(Modifiers::SHIFT, Key::Enter);
    lumen.wait_for("the preview again", |app| app.previewing_file().is_some());
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the file to be added", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
}

#[test]
fn projects_lists_its_folder_says_what_a_project_holds_and_opens_it_after_asking() {
    use lumen_project::model::{Color, Id, Track, TrackKind};
    use lumen_project::testing::project_of;

    let dir = TempDir::new("e2e-browse-projects");
    let folder = dir.path().join("Projects");
    std::fs::create_dir_all(folder.join("Old")).unwrap();
    let mut ballad = project_of(
        "Ballad",
        vec![
            Track::new(Id(1), "Keys", TrackKind::Audio, Color(0)),
            Track::new(Id(2), "Bass", TrackKind::Audio, Color(0)),
        ],
    );
    ballad.tempo[0].bpm = 92.0;
    ballad.meter[0].meter.numerator = 3;
    let ballad_path = folder.join("Ballad.lumen");
    lumen_project::save(&ballad, &ballad_path, &Default::default()).expect("save");
    let beat = project_of("Beat One", Vec::new());
    let beat_path = folder.join("Old").join("Beat One.lumen");
    lumen_project::save(&beat, &beat_path, &Default::default()).expect("save");
    let mut lumen = Lumen::start_with(dir, |l| l.with_projects_folder(&folder));

    // An unsaved change, so opening another project asks first.
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.frames(30);
    assert!(lumen.app().is_dirty());

    // Projects walks the projects folder, a folder down too.
    command(&mut lumen, "Browse projects");
    assert_eq!(lumen.app().browser_tab_name(), "Projects");
    lumen.run_until("the projects to be listed", |l| {
        !l.app().browser_files_reading()
            && shows_row(l, "Ballad, project")
            && shows_row(l, "Beat One, project")
    });

    // Search leaves the projects whose names hold the words.
    lumen.harness.get_by_label("Search projects").focus();
    lumen.frames(2);
    lumen.harness.event(Event::Text("beat".into()));
    lumen.frames(3);
    assert!(shows_row(&lumen, "Beat One, project"));
    assert!(!shows_row(&lumen, "Ballad, project"));
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(shows_row(&lumen, "Ballad, project"));

    // Choosing one says its tempo, metre and tracks, read off the UI thread.
    click_row(&mut lumen, "Ballad, project");
    lumen.run_until("the project's details", |l| {
        l.shows("92 BPM \u{b7} 3/4 \u{b7} 2 tracks")
    });

    // Enter opens it, after the unsaved-changes question.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(2);
    assert!(lumen.shows("Don't save"), "the question");
    lumen.click("Don't save");
    lumen.wait_for("the project to open", |app| {
        !app.is_busy() && app.project_path() == Some(ballad_path.as_path())
    });
    assert_eq!(lumen.app().project().tracks.len(), 2);
}

#[test]
fn a_sample_folder_that_is_not_there_is_named_above_what_the_others_hold() {
    let dir = TempDir::new("e2e-browse-partial");
    let gone = dir.path().join("Unplugged samples");
    let gone_text = gone.display().to_string();
    let root = samples(dir.path());
    let mut lumen = Lumen::start_with(dir, |l| {
        l.with_sample_folder(&root).also_sample_folder(&gone)
    });
    command(&mut lumen, "Browse samples");
    lumen.run_until("the walk to end", |l| {
        !l.app().browser_files_reading() && shows_row(l, "kick, audio file")
    });
    assert!(shows_row(&lumen, "snare, audio file"));
    assert!(
        shows_row(&lumen, &format!("{} is not there.", gone_text)),
        "the missing folder is said in full beside the files found"
    );
}

#[test]
fn an_empty_folder_still_goes_up_and_a_search_of_a_missing_place_says_why() {
    let dir = TempDir::new("e2e-browse-empty");
    let root = samples(dir.path());
    std::fs::create_dir_all(root.join("Empty")).unwrap();
    let gone = dir.path().join("Unplugged drive");
    let gone_text = gone.display().to_string();
    let mut lumen = Lumen::start_with(dir, |l| {
        l.with_place(&root).with_place(&gone).without_auto_preview()
    });
    command(&mut lumen, "Browse places");
    lumen.frames(5);
    let place = lumen
        .harness
        .get_by_label_contains("My Samples, folder")
        .rect();
    lumen.double_click(place.center());
    lumen.run_until("the place", |l| {
        !l.app().browser_files_reading() && shows_row(l, "Empty, folder")
    });
    let empty = lumen.harness.get_by_label_contains("Empty, folder").rect();
    lumen.double_click(empty.center());
    lumen.run_until("the empty folder", |l| {
        !l.app().browser_files_reading() && shows_row(l, "Nothing here")
    });
    // No rows, and Backspace still goes up.
    lumen.key(Modifiers::NONE, Key::Backspace);
    lumen.run_until("the place again", |l| shows_row(l, "snare, audio file"));
    lumen.key(Modifiers::NONE, Key::Backspace);
    lumen.run_until("the top", |l| shows_row(l, "Computer, folder"));

    // A search that meets the missing place says why, not "No files match".
    lumen.harness.get_by_label("Search places").focus();
    lumen.frames(2);
    lumen.harness.event(Event::Text("zz".into()));
    lumen.run_until("the search to end", |l| {
        !l.app().browser_files_reading() && shows_row(l, "This folder cannot be read")
    });
    assert!(shows_row(&lumen, &format!("{gone_text} is not there.")));
    assert!(!shows_row(&lumen, "No files match"));
}
