//! The transport, view, mixer and track commands of M9 R7 part 1: each runs
//! from the palette, and End goes to the end of the song except while a
//! browser list has the keyboard, which keeps End for its last row.

use eframe::egui::{Key, Modifiers};
use lumen_project::model::TrackKind;

use super::Lumen;
use super::lane_view::run_command;
use super::properties::with_two_clips;

fn kinds(lumen: &Lumen, kind: TrackKind) -> usize {
    let tracks = &lumen.app().project().tracks;
    tracks.iter().filter(|t| t.kind == kind).count()
}

#[test]
fn the_transport_toggles_run_from_the_palette() {
    let mut lumen = Lumen::start("commands-transport");
    let transport = |lumen: &Lumen| {
        let t = &lumen.app().project().transport;
        (t.loop_enabled, t.punch_in, t.punch_out)
    };
    assert_eq!(transport(&lumen), (false, false, false));

    run_command(&mut lumen, "Loop on or off");
    assert_eq!(transport(&lumen), (true, false, false));
    assert!(lumen.is_on("Loop"), "the bar's button follows");
    run_command(&mut lumen, "Punch in at the loop start");
    run_command(&mut lumen, "Punch out at the loop end");
    assert_eq!(transport(&lumen), (true, true, true));
    run_command(&mut lumen, "Punch in at the loop start");
    assert_eq!(transport(&lumen), (true, false, true), "one flag at a time");
    run_command(&mut lumen, "Loop on or off");
    assert!(!transport(&lumen).0);
    assert!(!lumen.is_on("Loop"));

    let metronome = lumen.is_on("Metronome");
    run_command(&mut lumen, "Metronome on or off");
    assert_eq!(lumen.is_on("Metronome"), !metronome);
    run_command(&mut lumen, "Metronome on or off");
    assert_eq!(lumen.is_on("Metronome"), metronome);
}

#[test]
fn go_to_end_moves_to_the_last_clip_or_a_later_loop_end() {
    let (mut lumen, _, _) = with_two_clips("commands-end");
    let project = lumen.app().project();
    let end = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.start + c.length)
        .max()
        .expect("two clips");
    assert_ne!(lumen.app().playhead(), end);

    run_command(&mut lumen, "Go to end");
    lumen.wait_for("the playhead at the last clip's end", |app| {
        app.playhead() == end
    });

    // The plain key does the same.
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the playhead at the start", |app| app.playhead().0 == 0);
    lumen.key(Modifiers::NONE, Key::End);
    lumen.wait_for("End at the last clip's end", |app| app.playhead() == end);

    // The default loop runs past the two short clips.
    let loop_end = lumen.app().project().transport.loop_end;
    assert!(loop_end > end, "the loop ends after the clips");
    run_command(&mut lumen, "Loop on or off");
    lumen.key(Modifiers::NONE, Key::End);
    lumen.wait_for("End at the loop's end", |app| app.playhead() == loop_end);
}

#[test]
fn go_to_end_says_why_it_cannot_run_in_an_empty_song() {
    let mut lumen = Lumen::start("commands-end-empty");
    let ctx = lumen.harness.ctx.clone();
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Go to end".into()));
    lumen.frames(2);
    let shown = lumen.app().palette_shows(&ctx);
    assert!(
        shown.iter().any(|(name, on)| name == "Go to end" && !on),
        "listed, greyed: {shown:?}"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
}

#[test]
fn end_in_the_browser_list_goes_to_its_last_row_and_leaves_the_playhead() {
    let (mut lumen, _, _) = with_two_clips("commands-end-browser");
    // F6 goes to the arrangement, the detail panel, then the browser.
    for _ in 0..3 {
        lumen.key(Modifiers::NONE, Key::F6);
    }
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(lumen.app().focused_pane(&ctx).as_deref(), Some("Browser"));
    lumen.key(Modifiers::NONE, Key::Home);
    let first = lumen.app().selected_device();
    let playhead = lumen.app().playhead();

    lumen.key(Modifiers::NONE, Key::End);
    lumen.frames(5);
    let last = lumen.app().selected_device();
    assert!(last.is_some(), "the last row is a device");
    assert_ne!(last, first, "the selection moved to the end of the list");
    assert_eq!(lumen.app().playhead(), playhead, "the list kept End");
}

#[test]
fn the_view_commands_run_from_the_palette() {
    let mut lumen = Lumen::start("commands-view");
    assert!(lumen.shows("Time format: Minutes and seconds"));
    run_command(&mut lumen, "Next time format");
    assert!(lumen.shows("Time format: Samples"));

    let shown = lumen.app().browser_shown();
    run_command(&mut lumen, "Show or hide the browser");
    assert_eq!(lumen.app().browser_shown(), !shown);
    run_command(&mut lumen, "Show or hide the browser");
    assert_eq!(lumen.app().browser_shown(), shown);
}

#[test]
fn audio_on_or_off_runs_from_the_palette() {
    let mut lumen = Lumen::start("commands-audio");
    lumen.wait_for("audio to run", |app| app.audio_running());
    run_command(&mut lumen, "Audio on or off");
    lumen.wait_for("audio to stop", |app| !app.audio_running());
    run_command(&mut lumen, "Audio on or off");
    lumen.wait_for("audio to run again", |app| app.audio_running());
}

#[test]
fn add_return_and_group_tracks_run_from_the_palette() {
    let mut lumen = Lumen::start("commands-tracks");
    let (returns, groups) = (
        kinds(&lumen, TrackKind::Return),
        kinds(&lumen, TrackKind::Group),
    );
    run_command(&mut lumen, "Add return track");
    run_command(&mut lumen, "Add group track");
    lumen.frames(30);
    assert_eq!(kinds(&lumen, TrackKind::Return), returns + 1);
    assert_eq!(kinds(&lumen, TrackKind::Group), groups + 1);
    assert_eq!(lumen.app().undo_label(), Some("Add group track"));
}

#[test]
fn add_group_track_goes_after_the_selected_track_as_the_menu_does() {
    let (mut lumen, _, _) = with_two_clips("commands-group-place");
    let first = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[first]);
    run_command(&mut lumen, "Add group track");
    lumen.frames(30);
    let tracks = &lumen.app().project().tracks;
    assert_eq!(tracks[1].kind, TrackKind::Group, "after the selected track");
    assert_eq!(
        lumen.app().selected_tracks(),
        [tracks[1].id],
        "and selected"
    );

    // The Add track button's menu, the same path: after the selection.
    lumen.click("Add track");
    lumen.click("Add group track");
    lumen.frames(30);
    let tracks = &lumen.app().project().tracks;
    assert_eq!(tracks[2].kind, TrackKind::Group);
    assert_eq!(lumen.track_names()[3], "snare");
}
