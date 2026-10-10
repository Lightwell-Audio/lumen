//! The M3 exit flow (`docs/milestones/M3.md`): create a project, add a
//! track, import audio, arrange clips, edit, save, close and reopen, and
//! get the same project back, which also sounds the same. Driven as a
//! user would: menus, keys, dropped files, drags and the palette.

use eframe::egui::{Event, Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::{Lumen, wav};

fn clip_rect(lumen: &Lumen, id: Id) -> eframe::egui::Rect {
    let ctx = lumen.harness.ctx.clone();
    lumen
        .app()
        .clip_rect(&ctx, id)
        .expect("the clip is in view")
}

/// The ids of every clip, track by track.
fn clip_ids(lumen: &Lumen) -> Vec<Id> {
    let tracks = &lumen.app().project().tracks;
    tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.id))
        .collect()
}

/// Plays from the start past the moved kick (the snare is muted) and
/// returns what was heard from the
/// first sound on.
fn listen(lumen: &mut Lumen) -> Vec<(f32, f32)> {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.frames(3);
    lumen.app_mut().heard();
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the kick", |app| app.playhead() > Ticks::quarters(3));
    lumen.key(Modifiers::NONE, Key::Space);
    let heard = lumen.app_mut().heard();
    let first = heard
        .iter()
        .position(|s| s.0.abs() > 1e-4)
        .expect("something was heard");
    heard[first..].to_vec()
}

/// Opens the palette, types `query` and runs the first row.
fn palette(lumen: &mut Lumen, query: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text(query.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

#[test]
fn create_add_import_arrange_edit_save_close_and_reopen_the_same_song() {
    // Create: a new Lumen is an empty project.
    let mut lumen = Lumen::start("walkthrough");
    assert!(lumen.app().project().tracks.is_empty());

    // Add a track from the menu.
    lumen.click("Add track");
    lumen.click("Add audio track Ctrl+T");
    assert_eq!(lumen.track_names(), ["Audio 1"]);
    // New tracks grow in and moved rows glide (M4.5): let them settle.
    lumen.frames(30);

    // Import: a file onto that track's lane, and one below the last track,
    // which makes a track for it.
    let header = lumen
        .harness
        .get_by_label("Audio 1: double-click to rename")
        .rect();
    // Near the start of the song (the lanes start where the ruler does):
    // files land where they are dropped.
    let lanes = lumen
        .harness
        .get_by_label("Ruler: click to move the playhead")
        .rect();
    let lane_x = lanes.left() + 10.0;
    let kick = wav(&lumen.dir().join("kick.wav"), 1.0);
    lumen.drop_files_at(Pos2::new(lane_x, header.center().y), &[kick]);
    let snare = wav(&lumen.dir().join("snare.wav"), 1.0);
    lumen.drop_files_at(Pos2::new(lane_x, header.bottom() + 120.0), &[snare]);
    lumen.frames(40);
    assert_eq!(lumen.track_names(), ["Audio 1", "snare"]);
    let ids = clip_ids(&lumen);
    let [kick, snare] = ids[..] else {
        panic!("two clips: {ids:?}");
    };

    // Arrange: drag the kick a beat to the right, then duplicate it.
    let zoom = lumen.app().view().zoom as f32;
    let r = clip_rect(&lumen, kick);
    let from = Pos2::new(r.left() + 10.0, r.top() + 6.0);
    lumen.drag(from, Pos2::new(from.x + zoom, from.y), Modifiers::NONE);
    lumen.key(Modifiers::COMMAND, Key::D);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 2);

    // Edit: rename the copy, mute the snare, set the tempo, a locator and
    // a loop round everything.
    lumen.key(Modifiers::COMMAND, Key::R);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("Kick B".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    let r = clip_rect(&lumen, snare);
    let at = Pos2::new(r.left() + 10.0, r.top() + 6.0);
    lumen.harness.hover_at(at);
    lumen.harness.drag_at(at);
    lumen.frames(1);
    lumen.harness.drop_at(at);
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Num0);
    palette(&mut lumen, "128");
    lumen.key(Modifiers::NONE, Key::M);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.key(Modifiers::COMMAND, Key::L);
    let project = lumen.app().project();
    assert!(project.tracks[1].clips[0].muted, "the snare is muted");
    assert!(
        project.tracks[0].clips.iter().any(|c| c.name == "Kick B"),
        "the copy is renamed"
    );
    assert_eq!(project.tempo[0].bpm, 128.0);
    assert_eq!(project.markers.len(), 1);
    assert!(project.transport.loop_enabled);
    let heard = listen(&mut lumen);

    // Save, as the Save dialog would once a path is chosen.
    let path = project_file(lumen.dir(), "Walkthrough");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen.app().project_as_saved();

    // Close, and reopen it in a fresh Lumen.
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    assert_eq!(again.app().file_notice(), None);
    assert!(!again.app().is_dirty());
    // The same song, field for field: tracks, clips and their edits,
    // tempo, meter, locators and the loop, and the view (zoom, scroll,
    // panels, grid, track heights) as it was when saved.
    let opened = again.app().project_as_saved();
    assert_eq!(opened.tracks, saved.tracks);
    assert_eq!(
        (
            &opened.tempo,
            &opened.time_signature,
            &opened.markers,
            &opened.transport
        ),
        (
            &saved.tempo,
            &saved.time_signature,
            &saved.markers,
            &saved.transport
        )
    );
    assert_eq!(opened.view, saved.view);
    // The same audio, now copied into the project folder (the project is
    // saved with "Copy imported audio" on, as it is by default).
    assert_eq!(opened.media.len(), saved.media.len());
    for (o, s) in opened.media.iter().zip(&saved.media) {
        assert_eq!(
            (o.id, o.frames, o.channels, o.sample_rate, o.size_bytes),
            (s.id, s.frames, s.channels, s.sample_rate, s.size_bytes)
        );
        let name = |p: &str| std::path::Path::new(p).file_name().map(|n| n.to_owned());
        assert_eq!(name(&o.path), name(&s.path));
        assert!(o.path.starts_with("Samples/Imported"), "{}", o.path);
    }

    // And it sounds the same, sample for sample, up to the Stop. Which
    // audio block a key press lands in depends on the threads' timing: in
    // the last block or two, one take can already be fading out while the
    // other still plays (seen on Linux, KNOWN_ISSUES), so those are left
    // out.
    const STOP: usize = 2_048;
    let reheard = listen(&mut again);
    let n = heard
        .len()
        .min(reheard.len())
        .min(48_000)
        .saturating_sub(STOP);
    assert!(n > 24_000, "a beat or more heard: {n}");
    let worst = heard[..n]
        .iter()
        .zip(&reheard[..n])
        .map(|(a, b)| (a.0 - b.0).abs().max((a.1 - b.1).abs()))
        .fold(0.0, f32::max);
    assert!(worst < 1e-6, "heard the same: worst difference {worst}");
}
