//! Group tracks (M4 slice 3): grouping and ungrouping by key, nesting,
//! folding, dragging tracks in and out, arming, deleting, files dropped on
//! a group, and what is heard through nested groups.

use std::time::Duration;

use eframe::egui::{Key, Modifiers, pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::model::{Id, TrackKind};

use super::{Lumen, wav};

/// A project with three audio tracks, the first two selected.
fn three_tracks(name: &str) -> Lumen {
    let mut lumen = Lumen::start(name);
    for _ in 0..3 {
        lumen.key(Modifiers::COMMAND, Key::T);
    }
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.key(Modifiers::SHIFT, Key::ArrowUp);
    lumen
}

fn ids(lumen: &Lumen) -> Vec<Id> {
    lumen.app().project().tracks.iter().map(|t| t.id).collect()
}

fn parent_of(lumen: &Lumen, name: &str) -> Option<String> {
    let tracks = &lumen.app().project().tracks;
    let parent = tracks.iter().find(|t| t.name == name)?.parent?;
    tracks
        .iter()
        .find(|t| t.id == parent)
        .map(|t| t.name.clone())
}

/// The header of track `name`, on screen.
fn header(lumen: &Lumen, name: &str) -> eframe::egui::Rect {
    lumen
        .harness
        .get_by_label_contains(&format!("Track {name},"))
        .rect()
}

#[test]
fn ctrl_g_groups_the_selection_and_ctrl_shift_g_ungroups_it() {
    let mut lumen = three_tracks("group-keys");
    lumen.key(Modifiers::COMMAND, Key::G);
    assert_eq!(
        lumen.track_names(),
        ["Group 1", "Audio 1", "Audio 2", "Audio 3"]
    );
    assert_eq!(parent_of(&lumen, "Audio 1").as_deref(), Some("Group 1"));
    assert_eq!(parent_of(&lumen, "Audio 2").as_deref(), Some("Group 1"));
    assert_eq!(parent_of(&lumen, "Audio 3"), None);
    let group = ids(&lumen)[0];
    assert_eq!(lumen.app().selected_tracks(), [group], "the new group");
    assert_eq!(lumen.app().undo_label(), Some("Group tracks"));
    assert!(lumen.shows("Fold group Group 1"));

    // One undo step each way.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.track_names(), ["Audio 1", "Audio 2", "Audio 3"]);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(lumen.track_names()[0], "Group 1");

    // Ctrl+Shift+G with the group selected: its tracks stay, a level up,
    // and become the selection.
    lumen
        .harness
        .get_by_label("Group 1: double-click to rename")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    assert_eq!(lumen.track_names(), ["Audio 1", "Audio 2", "Audio 3"]);
    assert_eq!(parent_of(&lumen, "Audio 1"), None);
    assert_eq!(lumen.app().selected_tracks().len(), 2);
    assert_eq!(lumen.app().undo_label(), Some("Ungroup"));
    assert!(lumen_project::validate(lumen.app().project()).is_ok());
}

#[test]
fn groups_nest_and_a_track_comes_out_of_its_group() {
    let mut lumen = three_tracks("group-nest");
    lumen.key(Modifiers::COMMAND, Key::G);
    // The second track alone, grouped again: a group inside the group.
    lumen
        .harness
        .get_by_label("Audio 2: double-click to rename")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::G);
    assert_eq!(
        lumen.track_names(),
        ["Group 1", "Audio 1", "Group 2", "Audio 2", "Audio 3"]
    );
    assert_eq!(parent_of(&lumen, "Group 2").as_deref(), Some("Group 1"));
    assert_eq!(parent_of(&lumen, "Audio 2").as_deref(), Some("Group 2"));

    // Ctrl+Shift+G on a track inside: it comes out, after its group.
    lumen
        .harness
        .get_by_label("Audio 1: double-click to rename")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    assert_eq!(
        lumen.track_names(),
        ["Group 1", "Group 2", "Audio 2", "Audio 1", "Audio 3"]
    );
    assert_eq!(parent_of(&lumen, "Audio 1"), None);
    assert_eq!(lumen.app().undo_label(), Some("Take track out of group"));
}

#[test]
fn a_folded_group_hides_its_tracks_and_stays_folded_when_saved() {
    let mut lumen = three_tracks("group-fold");
    lumen.key(Modifiers::COMMAND, Key::G);
    assert!(lumen.shows("Mute Audio 1"));
    lumen.click("Fold group Group 1");
    assert!(lumen.is_on("Fold group Group 1"));
    assert!(!lumen.shows("Mute Audio 1") && !lumen.shows("Mute Audio 2"));
    assert!(lumen.shows("Mute Audio 3"));
    // The header says what it hides.
    assert!(
        lumen
            .harness
            .query_by_label_contains("2 tracks inside, folded")
            .is_some()
    );
    // Folding is a view change, not an edit; it is saved with the project.
    assert_eq!(lumen.app().undo_label(), Some("Group tracks"));
    let group = ids(&lumen)[0];
    assert!(
        lumen
            .app()
            .project_as_saved()
            .view
            .collapsed
            .contains(&group)
    );

    // Right, with the group selected, shows its tracks again; Left hides.
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert!(lumen.shows("Mute Audio 1"));
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    assert!(!lumen.shows("Mute Audio 1"));
    // Up and Down step over hidden tracks.
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    assert_eq!(lumen.app().selected_tracks(), [ids(&lumen)[3]]);
    // Unfold all groups, from the palette's table of commands.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Unfold all groups".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(lumen.shows("Mute Audio 1"));
}

#[test]
fn tracks_dragged_onto_a_group_go_in_and_dragged_below_it_come_out() {
    let mut lumen = three_tracks("group-drag");
    // Group only the first track, so Audio 2 and Audio 3 are outside.
    lumen
        .harness
        .get_by_label("Audio 1: double-click to rename")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::G);
    assert_eq!(parent_of(&lumen, "Audio 3"), None);

    // Onto the middle of the group's header: in, at its end.
    let from = header(&lumen, "Audio 3").left_center() + vec2(60.0, 0.0);
    let onto = header(&lumen, "Group 1").left_center() + vec2(60.0, 0.0);
    lumen.drag(from, onto, Modifiers::NONE);
    assert_eq!(
        lumen.track_names(),
        ["Group 1", "Audio 1", "Audio 3", "Audio 2"]
    );
    assert_eq!(parent_of(&lumen, "Audio 3").as_deref(), Some("Group 1"));
    assert_eq!(lumen.app().undo_label(), Some("Move track"));

    // Below the last track: out of the group, at the end.
    let from = header(&lumen, "Audio 3").left_center() + vec2(60.0, 0.0);
    let below = header(&lumen, "Audio 2").left_bottom() + vec2(60.0, -4.0);
    lumen.drag(from, below, Modifiers::NONE);
    assert_eq!(
        lumen.track_names(),
        ["Group 1", "Audio 1", "Audio 2", "Audio 3"]
    );
    assert_eq!(parent_of(&lumen, "Audio 3"), None);

    // The group dragged below Audio 3 takes its track along.
    let from = header(&lumen, "Group 1").left_top() + vec2(60.0, 6.0);
    let below = header(&lumen, "Audio 3").left_bottom() + vec2(60.0, -4.0);
    lumen.drag(from, below, Modifiers::NONE);
    assert_eq!(
        lumen.track_names(),
        ["Audio 2", "Audio 3", "Group 1", "Audio 1"]
    );
    assert!(lumen_project::validate(lumen.app().project()).is_ok());
}

#[test]
fn arming_a_group_arms_its_tracks_and_deleting_it_takes_them() {
    let mut lumen = three_tracks("group-arm-delete");
    lumen.key(Modifiers::COMMAND, Key::G);
    lumen.click("Arm Group 1");
    let armed: Vec<bool> = lumen
        .app()
        .project()
        .tracks
        .iter()
        .map(|t| t.armed)
        .collect();
    assert_eq!(armed, [false, true, true, false], "inside, not the group");
    assert!(lumen.is_on("Arm Group 1"));
    // One disarmed inside: the group reads partly armed (not on).
    lumen.click("Arm Audio 1");
    assert!(!lumen.is_on("Arm Group 1"));
    lumen.click("Arm Group 1");
    assert!(lumen.app().project().tracks[1].armed);

    // Delete with the group selected: its tracks go too, in one step.
    lumen
        .harness
        .get_by_label("Group 1: double-click to rename")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(lumen.track_names(), ["Audio 3"]);
    assert_eq!(lumen.app().undo_label(), Some("Delete 3 tracks"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        lumen.track_names(),
        ["Group 1", "Audio 1", "Audio 2", "Audio 3"]
    );
}

#[test]
fn files_dropped_on_a_groups_lane_make_a_track_inside_it() {
    let mut lumen = three_tracks("group-drop");
    lumen.key(Modifiers::COMMAND, Key::G);
    let file = wav(&lumen.dir().join("clap.wav"), 1.0);
    let group = header(&lumen, "Group 1");
    let lane = pos2(group.right() + 300.0, group.center().y);
    lumen.drop_files_at(lane, &[file]);
    assert_eq!(
        lumen.track_names(),
        ["Group 1", "Audio 1", "Audio 2", "clap", "Audio 3"]
    );
    assert_eq!(parent_of(&lumen, "clap").as_deref(), Some("Group 1"));
    assert!(
        lumen.app().project().tracks[0].clips.is_empty(),
        "a group holds no clips"
    );
}

#[test]
fn a_track_in_nested_groups_is_heard_through_them_and_the_outer_mute_silences_it() {
    let mut lumen = Lumen::start("group-heard");
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    // The tone's track, grouped twice.
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    lumen.key(Modifiers::COMMAND, Key::G);
    lumen.key(Modifiers::COMMAND, Key::G);
    let tracks = &lumen.app().project().tracks;
    let kinds: Vec<TrackKind> = tracks.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [TrackKind::Group, TrackKind::Group, TrackKind::Audio]
    );
    let outer = tracks[0].name.clone();

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let loudest = |lumen: &mut Lumen| {
        std::thread::sleep(Duration::from_millis(150));
        lumen.frames(1);
        lumen.app_mut().heard();
        std::thread::sleep(Duration::from_millis(150));
        lumen.frames(1);
        let heard = lumen.app_mut().heard();
        assert!(heard.len() > 1_000, "heard {} frames", heard.len());
        heard.iter().fold(0.0f32, |m, s| m.max(s.0.abs()))
    };
    // As loud as the track alone (a stereo track at the centre keeps its
    // level) and nothing more from the groups, which stay at unity (owner,
    // after MT-MIX-3 run 1: grouped tracks played quieter, 3 dB a level).
    let alone = 3_000.0 / 32_768.0;
    let level = loudest(&mut lumen);
    assert!(
        (level - alone).abs() < alone * 0.05,
        "heard through both groups at {level}, alone {alone}"
    );
    // Soloing the track keeps its groups open (MT-MIX-3 run 1: it went
    // silent).
    let name = lumen.track_names()[2].clone();
    lumen.click(&format!("Solo {name}"));
    assert!(loudest(&mut lumen) > 0.01, "soloed inside its groups");
    lumen.click(&format!("Solo {name}"));
    lumen.click(&format!("Mute {outer}"));
    assert!(loudest(&mut lumen) < 1e-4, "the outer group's mute");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn groups_go_three_deep_and_ctrl_g_says_so() {
    // MT-MIX-3 run 1 (owner): no fourth level.
    let mut lumen = Lumen::start("group-depth");
    lumen.key(Modifiers::COMMAND, Key::T);
    for _ in 0..3 {
        lumen.key(Modifiers::COMMAND, Key::G);
    }
    assert_eq!(lumen.track_names().len(), 4, "three groups and the track");
    lumen.key(Modifiers::COMMAND, Key::G);
    assert_eq!(lumen.track_names().len(), 4, "no fourth group");
    assert_eq!(lumen.app().note(), Some("Groups go three deep at most"));
    assert_eq!(lumen.app().undo_label(), Some("Group tracks"));
}

#[test]
fn the_header_column_is_dragged_wider_and_double_clicked_back() {
    // MT-MIX-3 run 1 (owner): names were cut short.
    let mut lumen = Lumen::start("header-width");
    lumen.key(Modifiers::COMMAND, Key::T);
    let edge = lumen
        .harness
        .get_by_label("Resize the track headers")
        .rect();
    let width = |lumen: &Lumen| lumen.app().project_as_saved().view.header_width;
    assert_eq!(width(&lumen), 204.0);
    lumen.drag(
        edge.center(),
        edge.center() + vec2(100.0, 0.0),
        Modifiers::NONE,
    );
    assert_eq!(width(&lumen), 304.0, "saved with the project's view");
    let wider = header(&lumen, "Audio 1");
    assert_eq!(wider.width(), 304.0, "the headers follow");
    // Not an edit: nothing to undo for it.
    assert_eq!(lumen.app().undo_label(), Some("Add audio track"));
    lumen.drag(
        edge.center() + vec2(100.0, 0.0),
        edge.center() + vec2(2_000.0, 0.0),
        Modifiers::NONE,
    );
    assert_eq!(width(&lumen), 480.0, "at most 480");
    let edge = lumen
        .harness
        .get_by_label("Resize the track headers")
        .rect();
    lumen.double_click(edge.center());
    assert_eq!(width(&lumen), 204.0, "double-click: back to the default");
}
