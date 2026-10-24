//! M7b E6: the editing family end to end, as one producer would use it.
//! A take is split, the halves are crossfaded, joined into a new file,
//! bounced to a new track, the first track is frozen, and the project is
//! saved, closed and reopened. At every step the state is the model's, and
//! at the end both tracks still play.

use std::time::Duration;

use eframe::egui::{Key, Modifiers, vec2};
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;
use lumen_project::model::ClipContent;

use super::Lumen;
use super::clip_commands::{audio, command, cut_at};

fn peak(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(100), 4_800)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

/// Plays a moment from the start and reports the loudest sample heard.
fn heard(lumen: &mut Lumen) -> f32 {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the play", |app| app.is_playing());
    let loudest = (0..3).map(|_| peak(lumen)).fold(0.0, f32::max);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the stop", |app| !app.is_playing());
    loudest
}

fn media_exists(lumen: &Lumen, clip: &lumen_project::model::Clip) -> bool {
    let ClipContent::Audio { media, .. } = clip.content else {
        return false;
    };
    lumen.app().project().media.iter().any(|m| m.id == media)
}

#[test]
fn split_crossfade_join_bounce_freeze_save_reopen_and_play() {
    let mut lumen = audio("walkthrough-editing");
    for _ in 0..3 {
        lumen.key(Modifiers::NONE, Key::Equals);
    }
    lumen.frames(15);
    let take = lumen.app().project().tracks[0].clips[0].clone();

    // Split at the second quarter: two clips that meet.
    cut_at(&mut lumen, 2);
    assert_eq!(lumen.app().undo_label(), Some("Split clips"));
    let (left, right) = {
        let clips = &lumen.app().project().tracks[0].clips;
        assert_eq!(clips.len(), 2, "split in two");
        assert_eq!(clips[0].start + clips[0].length, clips[1].start);
        (clips[0].id, clips[1].id)
    };

    // Crossfade: the left clip's fade-out handle pulled over the right clip.
    let handle = lumen
        .app()
        .clip_handles(&lumen.harness.ctx, left)
        .expect("the left clip's handles");
    let right_width = lumen
        .app()
        .clip_rect(&lumen.harness.ctx, right)
        .expect("the right clip is in view")
        .width();
    lumen.drag(
        handle.fade_out,
        handle.fade_out + vec2(right_width * 0.2, 0.0),
        Modifiers::NONE,
    );
    {
        let clips = &lumen.app().project().tracks[0].clips;
        let right = clips.iter().find(|c| c.id == right).unwrap();
        assert!(right.crossfade, "the halves are crossfaded");
        assert!(right.fade_in.length > Ticks::ZERO);
    }

    // Join both into one new file, one undo step.
    lumen.key(Modifiers::COMMAND, Key::A);
    command(&mut lumen, "Join into new file");
    lumen.wait_for("the join", |app| {
        app.undo_label() == Some("Join into new file")
    });
    let joined = {
        let clips = &lumen.app().project().tracks[0].clips;
        assert_eq!(clips.len(), 1, "joined into one clip");
        clips[0].clone()
    };
    assert!(
        media_exists(&lumen, &joined),
        "the join's file is in the project"
    );
    assert_ne!(joined.content, take.content, "it plays the new file");

    // Bounce the joined clip to a new track after it.
    lumen.app_mut().select_clip(joined.id);
    command(&mut lumen, "Bounce");
    lumen.frames(3);
    assert!(lumen.shows("Include effects"), "the popover is open");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the bounce", |app| app.undo_label() == Some("Bounce"));
    assert_eq!(lumen.app().project().tracks.len(), 2, "a bounced track");
    let bounced = lumen.app().project().tracks[1].clips[0].clone();
    assert!(
        media_exists(&lumen, &bounced),
        "the bounce's file is in the project"
    );

    // Freeze the first track: it plays its file and refuses edits.
    let first = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[first]);
    command(&mut lumen, "Freeze track");
    lumen.wait_for("the freeze", |app| app.undo_label() == Some("Freeze"));
    assert!(lumen.app().project().tracks[0].frozen.is_some());
    lumen.app_mut().select_clip(joined.id);
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(3);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1, "read-only");
    assert_eq!(lumen.app().undo_label(), Some("Freeze"), "no undo step");
    let loud = heard(&mut lumen);
    assert!(loud > 0.01, "both tracks play: {loud}");

    // Save, close, reopen: the same project, no missing media, still audible.
    let path = project_file(lumen.dir(), "Edited");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen.app().project_as_saved();
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, move |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    assert_eq!(again.app().file_notice(), None, "nothing is missing");
    let opened = again.app().project_as_saved();
    assert_eq!(opened.tracks, saved.tracks);
    assert!(opened.tracks[0].frozen.is_some(), "still frozen");
    assert_eq!(opened.tracks[1].clips.len(), 1);
    let loud = heard(&mut again);
    assert!(loud > 0.01, "reopened, both tracks play: {loud}");

    // Unfreeze is one step, and the clips are as they were frozen.
    let frozen = again.app().project().tracks[0].clips.clone();
    again.app_mut().select_tracks(&[opened.tracks[0].id]);
    command(&mut again, "Unfreeze track");
    again.wait_for("the unfreeze", |app| {
        app.project().tracks[0].frozen.is_none()
    });
    assert_eq!(again.app().project().tracks[0].clips, frozen);
}
