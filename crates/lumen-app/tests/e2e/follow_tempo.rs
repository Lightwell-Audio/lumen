//! New audio clips follow the song's tempo (M9.5 S4d, the owner's W A).
//!
//! A file that says it is a loop, by a BPM in its name, comes in as a clip
//! as long in beats as the file is at that tempo, made at it, so it fits the
//! song whatever the song's tempo is. Any other file comes in at its own
//! speed. The Editing page's "Follow the tempo" decides, and recordings are
//! in `audio_record.rs`.

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, WarpMode};

use super::{Lumen, wav};

fn first_clip(lumen: &Lumen, track: usize) -> std::sync::Arc<Clip> {
    std::sync::Arc::clone(&lumen.app().project().tracks[track].clips[0])
}

fn dropped(name: &str, files: &[(&str, f32)]) -> Lumen {
    let mut lumen = Lumen::start(name);
    let paths: Vec<_> = files
        .iter()
        .map(|(file, seconds)| wav(&lumen.dir().join(file), *seconds))
        .collect();
    lumen.drop_files(&paths);
    lumen.wait_for("the files", |app| {
        app.project().tracks.len() == files.len()
            && app.project().tracks.iter().all(|t| !t.clips.is_empty())
    });
    lumen.frames(30);
    lumen
}

#[test]
fn a_file_with_a_tempo_in_its_name_comes_in_as_a_loop_that_follows_the_song() {
    let lumen = dropped(
        "follow-tempo-loop",
        &[("Funk Loop 90 BPM.wav", 2.0), ("Kick.wav", 2.0)],
    );
    let names = lumen.track_names();
    let at = |name: &str| names.iter().position(|n| n == name).expect("the track");
    let funk = first_clip(&lumen, at("Funk Loop 90 BPM"));
    let warp = funk.warp.as_ref().expect("a file that says 90 BPM follows");
    assert_eq!((warp.mode, warp.original_bpm), (WarpMode::FullMix, 90.0));
    // Two seconds at 90 BPM are three beats, though the song is at 120.
    assert_eq!(funk.length, Ticks::quarters(3));

    let kick = first_clip(&lumen, at("Kick"));
    assert!(
        kick.warp.is_none(),
        "a file that says nothing plays as it is"
    );
    assert_eq!(kick.length, Ticks::quarters(4), "two seconds at 120 BPM");
}

#[test]
fn the_setting_makes_every_file_follow_or_none() {
    let mut lumen = Lumen::start("follow-tempo-setting");
    lumen.open_preferences("Editing");
    lumen.click("New audio clips: always follow the tempo");
    lumen.frames(2);
    assert!(lumen.is_on("New audio clips: always follow the tempo"));
    lumen.key(Modifiers::NONE, Key::Escape);
    let kick = wav(&lumen.dir().join("Kick.wav"), 2.0);
    lumen.drop_files(&[kick]);
    lumen.wait_for("the file", |app| {
        app.project()
            .tracks
            .first()
            .is_some_and(|t| !t.clips.is_empty())
    });
    let always = first_clip(&lumen, 0);
    let warp = always.warp.as_ref().expect("Always follows");
    assert_eq!(warp.original_bpm, 120.0, "made at the song's tempo");

    lumen.open_preferences("Editing");
    lumen.click("New audio clips: never follow the tempo");
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Escape);
    let funk = wav(&lumen.dir().join("Funk 90 BPM.wav"), 2.0);
    lumen.drop_files(&[funk]);
    lumen.wait_for("the second file", |app| app.project().tracks.len() == 2);
    lumen.frames(30);
    let never = first_clip(&lumen, 1);
    assert!(never.warp.is_none(), "Never leaves it at its own speed");
    assert_eq!(never.length, Ticks::quarters(4));
}
