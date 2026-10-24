//! M7b E3: Consolidate renders the selected audio clips of a track into one
//! new file and replaces them with a clip that plays it, as one undo step.

use std::sync::atomic::AtomicBool;

use eframe::egui::{Key, Modifiers};
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;
use lumen_project::model::ClipContent;

use super::Lumen;
use super::clip_commands::{audio, command, cut_at};

/// A track with the clip cut at two quarters, the second part moved one grid
/// step later so a gap lies between the parts, both selected.
fn parts_with_a_gap(name: &str) -> Lumen {
    let mut lumen = audio(name);
    cut_at(&mut lumen, 2);
    let second = lumen.app().project().tracks[0].clips[1].id;
    lumen.app_mut().select_clip(second);
    lumen.key(Modifiers::ALT, Key::ArrowRight);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen
}

fn consolidated(lumen: &mut Lumen) {
    command(lumen, "Join into new file");
    lumen.wait_for("the render to replace the clips", |app| {
        app.undo_label() == Some("Join into new file")
    });
}

fn media_of(lumen: &Lumen, clip: usize) -> lumen_project::model::Media {
    let ClipContent::Audio { media, .. } = lumen.app().project().tracks[0].clips[clip].content
    else {
        panic!("an audio clip")
    };
    lumen
        .app()
        .project()
        .media
        .iter()
        .find(|m| m.id == media)
        .expect("the clip's media is in the project")
        .clone()
}

#[test]
fn consolidate_writes_one_file_holding_the_parts_and_the_gap_and_undo_gives_them_back() {
    let mut lumen = parts_with_a_gap("consolidate");
    let before = lumen.app().project().clone();
    let (first, second) = (
        before.tracks[0].clips[0].clone(),
        before.tracks[0].clips[1].clone(),
    );
    assert!(second.start > first.start + first.length, "a gap");

    consolidated(&mut lumen);
    let after = lumen.app().project();
    assert_eq!(after.tracks[0].clips.len(), 1);
    let clip = &after.tracks[0].clips[0];
    assert_eq!(clip.start, first.start);
    assert_eq!(clip.start + clip.length, second.start + second.length);
    assert!(clip.name.ends_with("(joined)"), "{}", clip.name);
    assert_eq!(lumen.app().selected_clips(), [clip.id]);

    // The file is a render in the renders folder, at the device's rate, and
    // the originals stay in the media pool.
    let media = media_of(&lumen, 0);
    assert!(after.media.len() > 1, "the original media stays");
    let path = std::path::PathBuf::from(&media.path);
    assert!(path.starts_with(lumen.dir().join("Renders")), "{path:?}");
    let audio = lumen_media::decode(&path, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(audio.frames() as u64, media.frames);
    assert_eq!(audio.channels(), 2);
    assert_eq!(audio.sample_rate, media.sample_rate);

    // Sound at both ends, silence in the gap (120 BPM: a quarter is half a
    // second).
    let rate = f64::from(media.sample_rate);
    let at = |t: Ticks| (t.0 as f64 / lumen_core::time::PPQ as f64 * 0.5 * rate) as usize;
    let gap = at(first.length)..at(second.start - first.start);
    let left = audio.channel(0);
    assert!(
        left[..gap.start].iter().any(|v| v.abs() > 0.01),
        "first part"
    );
    assert!(
        left[gap.end..].iter().any(|v| v.abs() > 0.01),
        "second part"
    );
    // A few frames in from each edge, to leave the 2 ms fades out.
    let quiet = gap.start + 200..gap.end - 200;
    assert!(left[quiet].iter().all(|v| *v == 0.0), "the gap is silent");

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project(), &before);
    assert_eq!(lumen.app().undo_label(), Some("Nudge clip"));
}

#[test]
fn one_clip_consolidates_and_a_clip_with_nothing_selected_does_not() {
    let mut lumen = audio("consolidate-one");
    let before = lumen.app().project().clone();
    // Nothing selected: the command is not available.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Join into new file".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(10);
    assert_eq!(lumen.app().project(), &before);

    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(id);
    consolidated(&mut lumen);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1);
    let (old, new) = (
        &before.tracks[0].clips[0],
        &lumen.app().project().tracks[0].clips[0],
    );
    assert_eq!((new.start, new.length), (old.start, old.length));
    assert_ne!(new.id, old.id);
}

#[test]
fn ctrl_j_renders_parts_that_do_not_continue_and_a_second_press_has_nothing_left_to_join() {
    let mut lumen = parts_with_a_gap("consolidate-ctrl-j");
    lumen.key(Modifiers::COMMAND, Key::J);
    lumen.wait_for("the render to replace the clips", |app| {
        app.undo_label() == Some("Join into new file")
    });
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1);
    // One clip is selected: Ctrl+J wants two.
    let now = lumen.app().project().clone();
    lumen.key(Modifiers::COMMAND, Key::J);
    lumen.frames(10);
    assert_eq!(lumen.app().project(), &now);
}

#[test]
fn a_consolidated_clip_survives_save_and_reopen_inside_the_project() {
    let mut lumen = parts_with_a_gap("consolidate-save");
    consolidated(&mut lumen);
    let path = project_file(lumen.dir(), "Rendered");
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
    let media = media_of(&again, 0);
    assert!(
        media.path.starts_with("Samples/Rendered/"),
        "{}",
        media.path
    );
    assert_eq!(media.original_path, None, "a render is not an import");
}

/// MIDI needs no file: the loop is baked into real notes and the track's
/// clips merge, in one undo step, at once.
#[test]
fn a_looping_midi_clip_consolidates_at_once_and_undo_gives_the_loop_back() {
    use lumen_core::time::Ticks;
    use lumen_project::model::Note;
    use lumen_project::testing::{TempDir, midi_track, project_of};
    let dir = TempDir::new("e2e-consolidate-midi");
    let path = dir.path().join("song.lumen");
    let notes = vec![Note::new(Ticks::quarters(0), Ticks::quarters(2), 60, 90)];
    let project = project_of(
        "Loop",
        vec![midi_track(
            1,
            "Keys",
            "poly",
            (0, 7),
            Some(3),
            notes,
            vec![],
        )],
    );
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    let before = lumen.app().project().clone();
    let clip = before.tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    command(&mut lumen, "Join into new file");
    assert_eq!(lumen.app().undo_label(), Some("Join into new file"));
    let after = lumen.app().project();
    assert_eq!(after.tracks[0].clips.len(), 1);
    let ClipContent::Midi { notes, .. } = &after.tracks[0].clips[0].content else {
        panic!("a MIDI clip")
    };
    assert_eq!(notes.len(), 3, "each repeat is real notes now");
    assert_eq!(after.tracks[0].clips[0].loop_length, None);
    assert_eq!(after.media, before.media, "no file");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project(), &before);
}

#[test]
fn consolidating_one_plain_midi_clip_says_there_is_nothing_to_do() {
    use lumen_project::testing::{TempDir, midi_track, project_of};
    let dir = TempDir::new("e2e-consolidate-plain");
    let path = dir.path().join("song.lumen");
    let project = project_of(
        "Plain",
        vec![midi_track(1, "Keys", "poly", (0, 4), None, vec![], vec![])],
    );
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    let before = lumen.app().project().clone();
    lumen.app_mut().select_clip(before.tracks[0].clips[0].id);
    command(&mut lumen, "Join into new file");
    assert_eq!(lumen.app().project(), &before);
    assert_eq!(lumen.app().undo_label(), None);
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("Nothing to join"))
    );
}
