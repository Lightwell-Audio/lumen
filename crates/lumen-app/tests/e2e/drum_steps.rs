//! The step sequencer (M8 D3, the owner's SQ A): a clip that fits the grid
//! opens as steps; a click toggles a step, a vertical drag sets its velocity,
//! Shift-click toggles its accent; the count, the resolution and the page are
//! settable; the keyboard reaches every step; and viewing a clip as steps never
//! moves a note.
//!
//! Points come from the grid's own geometry (`App::steps_cell_point`), so a
//! test clicks where a person would.

use eframe::egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::time::{PPQ, Ticks};
use lumen_project::drums::presets::Preset;
use lumen_project::model::{ClipContent, Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::midi_play::palette;

const STEP: i64 = PPQ / 4;
const KICK: u8 = 36;
const SNARE: u8 = 38;
const HAT: u8 = 42;

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

fn step(index: i64) -> Ticks {
    Ticks(index * STEP)
}

/// A kick on 1 and 3, a snare on 2 and 4, hats on the off-beat eighths: two
/// bars, steps 0, 8; 4, 12; 2, 10. Their length and velocity are a player's,
/// so none of them is the plain hit a click makes.
fn kit_notes() -> Vec<Note> {
    vec![
        Note::new(Ticks::ZERO, Ticks(60), KICK, 110),
        Note::new(q(2), Ticks(60), KICK, 100),
        Note::new(q(1), Ticks(60), SNARE, 100),
        Note::new(q(3), Ticks(60), SNARE, 96),
        Note::new(step(2), Ticks(30), HAT, 80),
        Note::new(step(10), Ticks(30), HAT, 72),
    ]
}

/// A project with one clip of `beats` beats on a track that reads the 808-style
/// map.
fn project_with(beats: i64, mut notes: Vec<Note>) -> Project {
    // A clip's notes are kept in start order.
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    let mut project = project_of(
        "Drums",
        vec![midi_track(
            1,
            "Drums",
            "pluck",
            (0, beats),
            None,
            notes,
            vec![],
        )],
    );
    let map = Preset::EIGHT_OH_EIGHT.map(Id(500));
    project.drum_maps = vec![map.clone()];
    let track = std::sync::Arc::make_mut(&mut project.tracks[0]);
    track.drum_map = Some(map.id);
    project
}

/// Starts Lumen on `project` and opens its clip as a double-click does, with
/// the panel the room the grid needs.
fn launch(name: &str, project: Project) -> (Lumen, Id) {
    let clip = project.tracks[0].clips[0].id;
    let dir = TempDir::new(&format!("e2e-steps-{name}"));
    let path = dir.path().join("kit.lumen");
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.frames(30);
    lumen.app_mut().set_detail_size(520.0);
    lumen.app_mut().open_clip_editor(clip);
    lumen.frames(30);
    (lumen, clip)
}

fn notes_of(lumen: &Lumen, clip: Id) -> Vec<Note> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == clip)
        .and_then(|c| match &c.content {
            ClipContent::Midi { notes, .. } => Some(notes.clone()),
            ClipContent::Audio { .. } => None,
        })
        .expect("the clip")
}

fn at(lumen: &Lumen, pitch: u8, index: i64) -> Pos2 {
    lumen
        .app()
        .steps_cell_point(pitch, index)
        .unwrap_or_else(|| panic!("step {index} of sound {pitch} is on the page"))
}

fn on(lumen: &Lumen, pitch: u8, index: i64) -> bool {
    lumen.app().steps_cell(pitch, index).expect("a grid").on
}

fn velocity(lumen: &Lumen, pitch: u8, index: i64) -> u8 {
    lumen
        .app()
        .steps_cell(pitch, index)
        .expect("a grid")
        .velocity
}

/// The grid's accessible name, which says where the keyboard's step is.
fn grid_name(lumen: &Lumen) -> String {
    lumen
        .harness
        .query_by_label_contains("Step grid: Drums")
        .map(|node| node.accesskit_node().label().unwrap_or_default().to_owned())
        .expect("the grid is drawn")
}

#[test]
fn a_clip_that_fits_the_grid_opens_as_steps_and_a_longer_one_as_lanes() {
    let (lumen, _) = launch("open-fit", project_with(8, kit_notes()));
    assert_eq!(lumen.app().drum_view_shown(), Some("steps"));
    assert!(lumen.app().steps_shown());
    // 8 beats of 1/16 steps are 32, and a page is 16.
    let setup = lumen.app().steps_setup().expect("a grid");
    assert_eq!((setup.count, setup.resolution.as_str()), (16, "1/16"));
    assert_eq!((setup.page, setup.pages), (0, 2));
    assert_eq!(lumen.app().steps_page_range(), Some((0, 15)));
    // The kit's pattern is on the grid where it was played.
    for (pitch, index) in [(KICK, 0), (KICK, 8), (SNARE, 4), (SNARE, 12), (HAT, 2)] {
        assert!(on(&lumen, pitch, index), "sound {pitch} step {index}");
    }
    assert!(!on(&lumen, KICK, 1));
    assert_eq!(velocity(&lumen, KICK, 0), 110);
    // The switch, with its names, is in the header.
    assert!(lumen.shows("Lanes view") && lumen.shows("Steps view") && lumen.shows("Keys view"));

    // Five bars do not fit 64 steps: the clip opens as lanes.
    let (long, _) = launch("open-long", project_with(20, kit_notes()));
    assert_eq!(long.app().drum_view_shown(), Some("lanes"));
    assert!(!long.app().steps_shown());
}

#[test]
fn a_click_makes_a_plain_hit_and_a_second_click_takes_it_away_one_undo_step_each() {
    let (mut lumen, clip) = launch("click", project_with(8, kit_notes()));
    let before = notes_of(&lumen, clip);
    let p = at(&lumen, SNARE, 6);
    lumen.click_at(p, Modifiers::NONE);
    assert!(on(&lumen, SNARE, 6));
    let made = notes_of(&lumen, clip)
        .into_iter()
        .find(|n| n.pitch() == SNARE && n.start() == step(6))
        .expect("the hit");
    assert_eq!(made.velocity(), 100, "a click makes the plain hit");
    assert_eq!(made.length(), Ticks(STEP), "one step long");
    assert!(!made.muted());
    assert_eq!(lumen.app().undo_label(), Some("Toggle step"));
    assert_eq!(notes_of(&lumen, clip).len(), before.len() + 1);
    assert_eq!(lumen.app().picked_note_count(), 1, "the new hit is picked");

    // The second click takes the plain hit away: the clip is as it was.
    lumen.click_at(p, Modifiers::NONE);
    assert!(!on(&lumen, SNARE, 6));
    assert_eq!(notes_of(&lumen, clip), before);

    // Each click was one undo step.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(on(&lumen, SNARE, 6), "undo brings the hit back");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(notes_of(&lumen, clip), before);
}

#[test]
fn a_hit_that_is_not_plain_is_muted_by_a_click_and_comes_back_exactly() {
    let (mut lumen, clip) = launch("mute", project_with(8, kit_notes()));
    let before = notes_of(&lumen, clip);
    let p = at(&lumen, KICK, 0);
    lumen.click_at(p, Modifiers::NONE);
    let cell = lumen.app().steps_cell(KICK, 0).expect("a grid");
    assert!(!cell.on && cell.held, "off, with the player's hit kept");
    let muted = notes_of(&lumen, clip);
    assert_eq!(muted.len(), before.len(), "no note was lost");
    let kick = muted
        .iter()
        .find(|n| n.pitch() == KICK && n.start() == Ticks::ZERO);
    assert!(kick.expect("the kick").muted());

    lumen.click_at(p, Modifiers::NONE);
    assert_eq!(
        notes_of(&lumen, clip),
        before,
        "toggling off and on returns the clip's notes exactly"
    );
}

#[test]
fn a_vertical_drag_sets_the_velocity_in_one_undo_step() {
    let (mut lumen, clip) = launch("velocity", project_with(8, kit_notes()));
    let before = notes_of(&lumen, clip);
    assert_eq!(velocity(&lumen, SNARE, 4), 100);
    let p = at(&lumen, SNARE, 4);
    // Down 10 points is quieter, and the note's other facts do not move.
    lumen.drag(p, p + vec2(0.0, 10.0), Modifiers::NONE);
    let quiet = velocity(&lumen, SNARE, 4);
    assert!(quiet < 70, "a drag down lowers the velocity: {quiet}");
    let after = notes_of(&lumen, clip);
    assert_eq!(after.len(), before.len());
    let snare = after
        .iter()
        .find(|n| n.pitch() == SNARE && n.start() == q(1))
        .expect("the snare");
    assert_eq!(snare.length(), Ticks(60), "only the velocity changed");
    assert_eq!(lumen.app().undo_label(), Some("Set step velocity"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(notes_of(&lumen, clip), before, "one undo step");

    // Up is louder, and a drag from an off step makes it.
    let p = at(&lumen, SNARE, 4);
    lumen.drag(p, p - vec2(0.0, 10.0), Modifiers::NONE);
    assert_eq!(velocity(&lumen, SNARE, 4), 127);
    let q = at(&lumen, KICK, 5);
    lumen.drag(q, q - vec2(0.0, 6.0), Modifiers::NONE);
    assert!(on(&lumen, KICK, 5), "a drag on an off step turns it on");
    assert!(velocity(&lumen, KICK, 5) > 100);
}

#[test]
fn a_shift_click_toggles_the_accent_and_the_accent_is_a_velocity_the_clip_remembers() {
    let (mut lumen, clip) = launch("accent", project_with(8, kit_notes()));
    let p = at(&lumen, HAT, 6);
    lumen.click_at(p, Modifiers::SHIFT);
    assert!(on(&lumen, HAT, 6), "Shift-click on an off step turns it on");
    assert_eq!(velocity(&lumen, HAT, 6), 127, "as an accent");
    lumen.click_at(p, Modifiers::SHIFT);
    assert!(on(&lumen, HAT, 6), "and a second Shift-click keeps it");
    assert_eq!(velocity(&lumen, HAT, 6), 100, "as a plain hit");
    // A step that is not an accent becomes one.
    let p = at(&lumen, SNARE, 4);
    lumen.click_at(p, Modifiers::SHIFT);
    assert_eq!(velocity(&lumen, SNARE, 4), 127);
    // The accent is a velocity the clip's notes hold, so it survives the
    // grid: the notes carry it.
    assert!(
        notes_of(&lumen, clip)
            .iter()
            .any(|n| n.pitch() == SNARE && n.velocity() == 127)
    );
    // The accent velocity is settable from the strip.
    lumen.click("Accent 127");
    lumen.click("112");
    assert_eq!(lumen.app().steps_setup().expect("a grid").accent, 112);
    let p = at(&lumen, HAT, 12);
    lumen.click_at(p, Modifiers::SHIFT);
    assert_eq!(velocity(&lumen, HAT, 12), 112);
}

#[test]
fn an_off_grid_hit_shows_at_its_step_and_viewing_it_never_moves_it() {
    let late = Note::new(Ticks(STEP * 4 + 9_000), Ticks(10_000), KICK, 90);
    let mut notes = kit_notes();
    notes.push(late);
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    let (mut lumen, clip) = launch("off-grid", project_with(8, notes));
    let before = notes_of(&lumen, clip);
    let cell = lumen.app().steps_cell(KICK, 4).expect("a grid");
    assert!(cell.on && cell.off_grid, "shown at its step, with a mark");
    assert_eq!(cell.velocity, 90);
    // Looking around changes nothing: the page, the count, the resolution.
    lumen.click("Page 2");
    lumen.click("Page 1");
    lumen.click("32 steps a page");
    lumen.click("16 steps a page");
    assert_eq!(notes_of(&lumen, clip), before, "viewing moves nothing");
    assert!(lumen.app().undo_label().is_none() || lumen.app().undo_label() == Some("Open"));

    // Off and on again returns the notes exactly.
    let p = at(&lumen, KICK, 4);
    lumen.click_at(p, Modifiers::NONE);
    assert!(!on(&lumen, KICK, 4));
    lumen.click_at(p, Modifiers::NONE);
    assert_eq!(notes_of(&lumen, clip), before);
}

#[test]
fn the_count_the_resolution_and_the_page_are_settable() {
    let (mut lumen, _) = launch("setup", project_with(16, kit_notes()));
    // Four bars are 64 sixteenths: four pages of 16.
    assert_eq!(lumen.app().steps_setup().expect("a grid").pages, 4);
    lumen.click("32 steps a page");
    let setup = lumen.app().steps_setup().expect("a grid");
    assert_eq!((setup.count, setup.pages, setup.page), (32, 2, 0));
    lumen.click("Page 2");
    assert_eq!(lumen.app().steps_page_range(), Some((32, 63)));
    // A step on the second page is on the screen, and the first is not.
    assert!(lumen.app().steps_cell_point(KICK, 40).is_some());
    assert!(lumen.app().steps_cell_point(KICK, 4).is_none());
    // A different count starts again at the first page.
    lumen.click("8 steps a page");
    let setup = lumen.app().steps_setup().expect("a grid");
    assert_eq!((setup.count, setup.pages, setup.page), (8, 8, 0));
    lumen.click("16 steps a page");

    // The resolution: 1/8 makes each step two sixteenths, so the four bars
    // are 32 steps: two pages of 16.
    lumen.click("1/16");
    lumen.click("1/8");
    let setup = lumen.app().steps_setup().expect("a grid");
    assert_eq!(setup.resolution, "1/8");
    assert_eq!(setup.pages, 2);
    // A hat at 2 sixteenths is the second 1/8 step.
    assert!(on(&lumen, HAT, 1));
    // Triplets are there too, and the grid is the same clip's.
    lumen.click("1/8");
    lumen.click("1/16T");
    assert_eq!(
        lumen.app().steps_setup().expect("a grid").resolution,
        "1/16T"
    );
}

#[test]
fn the_page_commands_turn_the_page_and_say_when_they_cannot() {
    let (mut lumen, _) = launch("pages", project_with(8, kit_notes()));
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(
        lumen.app().command_reason(&ctx, "Steps: previous page"),
        Some("This is the first page")
    );
    assert_eq!(lumen.app().command_reason(&ctx, "Steps: next page"), None);
    palette(&mut lumen, "Steps: next page");
    lumen.frames(5);
    assert_eq!(lumen.app().steps_page_range(), Some((16, 31)));
    assert_eq!(
        lumen.app().command_reason(&ctx, "Steps: next page"),
        Some("This is the last page")
    );
    palette(&mut lumen, "Steps: previous page");
    lumen.frames(5);
    assert_eq!(lumen.app().steps_page_range(), Some((0, 15)));
}

#[test]
fn the_keyboard_reaches_every_step_and_space_toggles_a_step_not_the_transport() {
    let (mut lumen, clip) = launch("keys", project_with(8, kit_notes()));
    let before = notes_of(&lumen, clip);
    // The grid is a Tab stop with a name that says where the keyboard is.
    lumen
        .harness
        .get_by_label_contains("Step grid: Drums")
        .focus();
    lumen.frames(4);
    assert!(
        lumen.app().steps_has_keyboard(),
        "the grid has the keyboard"
    );
    // The first row, the first step.
    let (pitch, index) = lumen.app().steps_cursor().expect("a place");
    assert_eq!(index, 0);
    let first = grid_name(&lumen);
    assert!(first.contains("step 1 of 32"), "{first}");
    assert!(
        first.contains("on, velocity"),
        "or off: the name says: {first}"
    );

    // Right, Right moves to step 3; Down moves a sound down.
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert_eq!(lumen.app().steps_cursor(), Some((pitch, 2)));
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    let (next_pitch, index) = lumen.app().steps_cursor().expect("a place");
    assert_ne!(next_pitch, pitch);
    assert_eq!(index, 2);
    assert!(grid_name(&lumen).contains("step 3 of 32"));

    // Space toggles the step the keyboard is on, one undo step, and never
    // starts the transport.
    assert!(!lumen.app().is_playing());
    let was = on(&lumen, next_pitch, 2);
    lumen.key(Modifiers::NONE, Key::Space);
    assert!(
        !lumen.app().is_playing(),
        "Space did not start the transport"
    );
    assert_ne!(on(&lumen, next_pitch, 2), was, "Space toggled the step");
    assert_eq!(lumen.app().undo_label(), Some("Toggle step"));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(on(&lumen, next_pitch, 2), was, "Enter toggles it back");

    // Shift+Space toggles the accent.
    lumen.key(Modifiers::SHIFT, Key::Space);
    assert!(on(&lumen, next_pitch, 2));
    assert_eq!(velocity(&lumen, next_pitch, 2), 127);
    assert!(grid_name(&lumen).contains("accent"));

    // End and Home run along the row; Page Down turns the page.
    lumen.key(Modifiers::NONE, Key::End);
    assert_eq!(lumen.app().steps_cursor(), Some((next_pitch, 31)));
    assert_eq!(lumen.app().steps_page_range(), Some((16, 31)));
    lumen.key(Modifiers::NONE, Key::Home);
    assert_eq!(lumen.app().steps_cursor(), Some((next_pitch, 0)));
    assert_eq!(lumen.app().steps_page_range(), Some((0, 15)));
    lumen.key(Modifiers::NONE, Key::PageDown);
    assert_eq!(lumen.app().steps_page_range(), Some((16, 31)));
    assert_eq!(lumen.app().steps_cursor(), Some((next_pitch, 16)));

    // The keyboard is as strong as the mouse: undo everything it did.
    for _ in 0..3 {
        lumen.key(Modifiers::COMMAND, Key::Z);
    }
    assert_eq!(notes_of(&lumen, clip), before);
}

#[test]
fn space_is_the_transports_when_the_grid_does_not_have_the_keyboard() {
    let (mut lumen, clip) = launch("space", project_with(8, kit_notes()));
    let before = notes_of(&lumen, clip);
    assert!(!lumen.app().steps_has_keyboard());
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the transport", |app| app.is_playing());
    assert_eq!(notes_of(&lumen, clip), before, "no step was toggled");
    // The step the playhead is in is lit while it plays.
    lumen.wait_for("a lit step", |app| app.steps_playing_step().is_some());
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the stop", |app| !app.is_playing());
}

#[test]
fn the_view_switch_is_in_the_header_the_editors_menu_and_the_palette() {
    let (mut lumen, _) = launch("switch", project_with(8, kit_notes()));
    assert_eq!(lumen.app().drum_view_shown(), Some("steps"));
    lumen.click("Lanes view");
    assert_eq!(lumen.app().drum_view_shown(), Some("lanes"));
    lumen.click("Steps view");
    assert_eq!(lumen.app().drum_view_shown(), Some("steps"));
    lumen.click("Keys view");
    assert_eq!(lumen.app().drum_view_shown(), Some("keys"));

    palette(&mut lumen, "Show the step sequencer");
    lumen.frames(5);
    assert_eq!(lumen.app().drum_view_shown(), Some("steps"));
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(
        lumen
            .app()
            .command_reason(&ctx, "Show the step sequencer (Steps)"),
        Some("The step sequencer is the view already")
    );

    // The editor's menu offers the other two.
    let body = lumen.app().editor_body().expect("the grid drew");
    let empty = Pos2::new(body.right() - 40.0, body.bottom() - 6.0);
    lumen.right_click(empty);
    assert!(lumen.shows("Show the drum lanes"));
    assert!(lumen.shows("Show the piano roll (Keys)"));
    lumen.click("Show the drum lanes");
    assert_eq!(lumen.app().drum_view_shown(), Some("lanes"));
}

#[test]
fn a_chosen_view_is_kept_for_the_track_and_the_fit_only_decides_before_a_choice() {
    let (mut lumen, clip) = launch("chosen", project_with(8, kit_notes()));
    assert_eq!(lumen.app().drum_view_shown(), Some("steps"));
    lumen.click("Lanes view");
    // Closing and opening the clip again keeps what the user chose.
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    lumen.app_mut().open_clip_editor(clip);
    lumen.frames(10);
    assert_eq!(lumen.app().drum_view_shown(), Some("lanes"));
}

#[test]
fn a_right_click_on_a_step_with_a_hit_offers_the_notes_menu_and_delete_removes_the_hit() {
    let (mut lumen, clip) = launch("menu", project_with(8, kit_notes()));
    let before = notes_of(&lumen, clip);
    let p = at(&lumen, SNARE, 4);
    lumen.right_click(p);
    assert!(
        lumen.shows("Show the drum lanes"),
        "the view rows are there"
    );
    assert!(
        lumen.shows("Mute the picked lane"),
        "the hit under the pointer is picked, so the lane's rows apply"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    // A click picks the step's hit (it was a player's, so it is muted, not
    // deleted), and Delete removes the picked hit like any note.
    lumen.click_at(at(&lumen, KICK, 0), Modifiers::NONE);
    assert_eq!(
        lumen.app().picked_note_count(),
        0,
        "a muted hit is not picked"
    );
    lumen.click_at(at(&lumen, KICK, 0), Modifiers::NONE);
    assert_eq!(lumen.app().picked_note_count(), 1, "unmuted, it is picked");
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(notes_of(&lumen, clip).len(), before.len() - 1);
    assert!(!on(&lumen, KICK, 0));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(notes_of(&lumen, clip).len(), before.len());
}

#[test]
fn a_lanes_sound_with_no_hits_is_still_a_row_and_hide_unused_hides_it() {
    let (mut lumen, _) = launch("rows", project_with(8, kit_notes()));
    let rows = lumen.app().open_drum_lanes().len();
    assert!(rows > 3, "every sound of the map is a row");
    lumen.app_mut().hide_the_unused_lanes();
    lumen.frames(10);
    assert_eq!(
        lumen.app().open_drum_lanes(),
        vec!["Kick", "Snare", "Closed Hat"],
        "the three sounds the clip plays"
    );
    // The first of them is on the page and the grid still reaches it.
    assert!(lumen.app().steps_cell_point(KICK, 0).is_some());
}
