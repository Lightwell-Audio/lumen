//! Seeing and adding an automation lane (M7 slice A1): the A button and key
//! show a lane over a track's clips, a click adds a point, a drag moves it,
//! Ctrl reaches the clip underneath, the chip chooses the parameter, and the
//! keys alone can do all of it. Every edit is one undo step.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, Pos2};
use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::Id;

use super::{Lumen, wav};

/// The test tone's peak (`wav`): 3,000 of 32,768.
const TONE: f32 = 3_000.0 / 32_768.0;

/// A project with one track holding a long tone, selected, on a one-beat
/// grid, with the lane not yet shown.
fn tone(name: &str) -> (Lumen, Id) {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let track = lumen.app().project().tracks[0].id;
    lumen.click("Grid: 1/8");
    lumen.click("1/4");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.app_mut().select_tracks(&[track]);
    lumen.frames(40);
    (lumen, track)
}

/// The lane of `track` for `target`, as the project holds it.
fn points(lumen: &Lumen, track: Id, target: Target) -> Vec<(Ticks, f32)> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.lane(target))
        .map(|l| l.lane.points().iter().map(|p| (p.at, p.value)).collect())
        .unwrap_or_default()
}

fn spot(lumen: &Lumen, track: Id, quarters: f64, value: f32) -> Pos2 {
    let ctx = lumen.harness.ctx.clone();
    lumen
        .app()
        .automation_spot(&ctx, track, quarters, value)
        .expect("the lane is in view")
}

fn point_pos(lumen: &Lumen, track: Id, index: usize) -> Pos2 {
    let ctx = lumen.harness.ctx.clone();
    lumen
        .app()
        .automation_point_pos(&ctx, track, index)
        .expect("the point is in view")
}

fn click_at(lumen: &mut Lumen, p: Pos2) {
    lumen.harness.hover_at(p);
    lumen.frames(2);
    lumen.harness.drag_at(p);
    lumen.frames(1);
    lumen.harness.drop_at(p);
    lumen.frames(3);
}

/// Types `query` into the palette and presses Enter on its first row.
fn run_command(lumen: &mut Lumen, query: &str) {
    if !lumen.app().palette_open() {
        lumen.key(Modifiers::COMMAND, Key::K);
    }
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(query.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
}

fn peak(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(150), 1_000)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()))
}

fn near(got: f32, want: f32) -> bool {
    (got - want).abs() <= want.max(1e-3) * 0.15
}

#[test]
fn the_a_key_shows_a_lane_over_the_clips_and_makes_room_for_it() {
    let (mut lumen, track) = tone("lane-key");
    assert_eq!(lumen.app().automation_shown(track), None);
    let ctx = lumen.harness.ctx.clone();
    assert!(lumen.app().automation_row(&ctx, track).is_none());
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    assert_eq!(lumen.app().automation_shown(track), Some(Target::Volume));
    let row = lumen.app().automation_row(&ctx, track).expect("in view");
    assert!(
        row.height() >= 110.0 - 0.5,
        "a folded track opens for its lane: {}",
        row.height()
    );
    assert!(
        lumen.shows("Automation lane of tone: Volume"),
        "the chip names the parameter"
    );
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(5);
    assert_eq!(
        lumen.app().automation_shown(track),
        None,
        "A again hides it"
    );
    assert!(!lumen.shows("Automation lane of tone: Volume"));
}

#[test]
fn the_a_button_in_an_unfolded_header_shows_and_hides_the_lane() {
    let (mut lumen, track) = tone("lane-button");
    lumen.click("Fold tone");
    lumen.frames(40);
    assert!(lumen.shows("Automation tone"), "unfolded headers have it");
    assert!(!lumen.is_on("Automation tone"));
    lumen.click("Automation tone");
    assert!(lumen.is_on("Automation tone"));
    assert_eq!(lumen.app().automation_shown(track), Some(Target::Volume));
    lumen.click("Automation tone");
    assert_eq!(lumen.app().automation_shown(track), None);
}

#[test]
fn a_click_adds_a_point_on_the_grid_and_it_is_heard() {
    let (mut lumen, track) = tone("lane-click");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let at = spot(&lumen, track, 4.3, -20.0);
    click_at(&mut lumen, at);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.len(), 1, "one point: {held:?}");
    assert_eq!(held[0].0, Ticks::quarters(4), "snapped to the beat");
    assert!(
        (held[0].1 + 20.0).abs() < 1.5,
        "about −20 dB where it was clicked: {}",
        held[0].1
    );
    assert_eq!(lumen.app().automation_picked(), Some((track, vec![0])));
    assert!(lumen.app().is_dirty());
    // The lane plays: a single point holds its value.
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let level = peak(&mut lumen);
    assert!(
        near(level, TONE * 0.1),
        "a tenth of the level at −20 dB: {level}"
    );
    lumen.key(Modifiers::NONE, Key::Space);

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(
        points(&lumen, track, Target::Volume).is_empty(),
        "the click is one undo step and takes the lane with it"
    );
}

#[test]
fn alt_places_a_point_off_the_grid() {
    let (mut lumen, track) = tone("lane-alt");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let at = spot(&lumen, track, 4.3, 0.0);
    lumen.harness.event(Event::ModifiersChanged(Modifiers::ALT));
    click_at(&mut lumen, at);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(2);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.len(), 1);
    let quarters = held[0].0.as_quarters();
    assert!((quarters - 4.3).abs() < 0.1, "{quarters}");
    assert_ne!(held[0].0, Ticks::quarters(4));
}

#[test]
fn dragging_a_point_moves_it_and_is_one_undo_step() {
    let (mut lumen, track) = tone("lane-drag");
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            Lane::new(vec![
                Point::new(Ticks::quarters(2), 0.0),
                Point::new(Ticks::quarters(8), -12.0),
            ])
            .unwrap(),
        )
        .unwrap();
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let from = point_pos(&lumen, track, 0);
    let to = spot(&lumen, track, 5.0, -30.0);
    lumen.drag(from, to, Modifiers::NONE);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.len(), 2, "nothing was added: {held:?}");
    assert_eq!(held[0].0, Ticks::quarters(5));
    assert!((held[0].1 + 30.0).abs() < 1.5, "{}", held[0].1);
    assert_eq!(
        held[1],
        (Ticks::quarters(8), -12.0),
        "the other is as it was"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        points(&lumen, track, Target::Volume)[0],
        (Ticks::quarters(2), 0.0),
        "one undo puts the point back"
    );
}

#[test]
fn a_point_does_not_pass_its_neighbour() {
    let (mut lumen, track) = tone("lane-neighbour");
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            Lane::new(vec![
                Point::new(Ticks::quarters(2), 0.0),
                Point::new(Ticks::quarters(4), 0.0),
            ])
            .unwrap(),
        )
        .unwrap();
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let from = point_pos(&lumen, track, 0);
    let to = spot(&lumen, track, 9.0, 0.0);
    lumen.drag(from, to, Modifiers::NONE);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.len(), 2);
    assert!(held[0].0 < held[1].0, "still in order: {held:?}");
    assert_eq!(held[1].0, Ticks::quarters(4));
}

#[test]
fn pressing_empty_lane_and_dragging_adds_a_point_and_moves_it_in_one_step() {
    let (mut lumen, track) = tone("lane-press");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let from = spot(&lumen, track, 2.0, -6.0);
    let to = spot(&lumen, track, 6.0, -24.0);
    lumen.drag(from, to, Modifiers::NONE);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].0, Ticks::quarters(6));
    assert!((held[0].1 + 24.0).abs() < 1.5, "{}", held[0].1);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(
        points(&lumen, track, Target::Volume).is_empty(),
        "adding and moving it is one step"
    );
}

#[test]
fn ctrl_reaches_the_clip_under_the_lane() {
    let (mut lumen, track) = tone("lane-ctrl");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    let at = spot(&lumen, track, 3.0, -6.0);
    // Plain: the lane takes the click.
    click_at(&mut lumen, at);
    assert!(lumen.app().selected_clips().is_empty());
    assert_eq!(points(&lumen, track, Target::Volume).len(), 1);
    // Ctrl: the clip does, and the lane gets no second point.
    let elsewhere = spot(&lumen, track, 7.0, -6.0);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::COMMAND));
    click_at(&mut lumen, elsewhere);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(2);
    assert_eq!(lumen.app().selected_clips(), [clip]);
    assert_eq!(points(&lumen, track, Target::Volume).len(), 1);
}

#[test]
fn delete_takes_the_picked_point_and_the_last_one_takes_the_lane() {
    let (mut lumen, track) = tone("lane-delete");
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            Lane::new(vec![
                Point::new(Ticks::quarters(2), 0.0),
                Point::new(Ticks::quarters(6), -6.0),
            ])
            .unwrap(),
        )
        .unwrap();
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let first = point_pos(&lumen, track, 0);
    click_at(&mut lumen, first);
    assert_eq!(lumen.app().automation_picked(), Some((track, vec![0])));
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(
        points(&lumen, track, Target::Volume),
        [(Ticks::quarters(6), -6.0)]
    );
    assert_eq!(lumen.app().automation_picked(), None);
    let last = point_pos(&lumen, track, 0);
    click_at(&mut lumen, last);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert!(
        lumen.app().project().tracks[0]
            .lane(Target::Volume)
            .is_none(),
        "no points, no lane: the fader is its own again"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(points(&lumen, track, Target::Volume).len(), 1);
}

#[test]
fn the_chip_chooses_which_parameter_the_lane_draws() {
    let (mut lumen, track) = tone("lane-chip");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    lumen.click("Automation lane of tone: Volume");
    lumen.click("Pan");
    assert_eq!(lumen.app().automation_shown(track), Some(Target::Pan));
    assert!(lumen.shows("Automation lane of tone: Pan"));
    // A click now makes a pan lane, and leaves volume alone.
    let at = spot(&lumen, track, 4.0, 0.5);
    click_at(&mut lumen, at);
    let held = points(&lumen, track, Target::Pan);
    assert_eq!(held.len(), 1);
    assert!(held[0].1 > 0.3 && held[0].1 < 0.7, "{}", held[0].1);
    assert!(points(&lumen, track, Target::Volume).is_empty());
    // The chooser says how many points each lane holds.
    lumen.click("Automation lane of tone: Pan");
    assert!(lumen.shows("Pan  \u{00B7}  1 point"));
}

#[test]
fn the_keys_alone_make_a_lane_and_move_and_delete_its_point() {
    let (mut lumen, track) = tone("lane-keys");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    // The playhead goes to bar 2; the command adds a point there.
    run_command(&mut lumen, "bar 2");
    lumen.wait_for("the playhead at bar 2", |app| {
        (app.playhead() - Ticks::quarters(4)).0.abs() < Ticks::quarters(1).0 / 1000
    });
    run_command(&mut lumen, "Add an automation point at the playhead");
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].0, Ticks::quarters(4));
    assert_eq!(held[0].1, 0.0, "the control's own value, so nothing jumps");
    assert_eq!(lumen.app().automation_picked(), Some((track, vec![0])));
    // Right moves it a grid step later, Up raises it, Delete takes it.
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert_eq!(
        points(&lumen, track, Target::Volume)[0].0,
        Ticks::quarters(5)
    );
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(
        points(&lumen, track, Target::Volume)[0].0,
        Ticks::quarters(3)
    );
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    assert!(points(&lumen, track, Target::Volume)[0].1 < 0.0);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert!(points(&lumen, track, Target::Volume).is_empty());
}

#[test]
fn a_lane_can_be_turned_off_and_removed_from_the_palette() {
    let (mut lumen, track) = tone("lane-palette");
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            Lane::new(vec![Point::new(Ticks::quarters(0), -40.0)]).unwrap(),
        )
        .unwrap();
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    run_command(&mut lumen, "Turn the automation lane on or off");
    let off = lumen.app().project().tracks[0]
        .lane(Target::Volume)
        .map(|l| l.enabled);
    assert_eq!(off, Some(false));
    assert!(lumen.shows("Automation lane of tone: Volume \u{00B7} off"));
    run_command(&mut lumen, "Remove the automation lane");
    assert!(
        lumen.app().project().tracks[0]
            .lane(Target::Volume)
            .is_none()
    );
}

#[test]
fn a_lane_that_cannot_run_says_why_in_the_palette() {
    let mut lumen = Lumen::start("lane-why");
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen
        .harness
        .event(Event::Text("Show or hide automation lanes".into()));
    lumen.frames(2);
    assert!(lumen.shows("Show or hide automation lanes, unavailable: Select a track first"));
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(
        "Add an automation point at the playhead".into(),
    ));
    lumen.frames(2);
    assert!(
        lumen.shows("Add an automation point at the playhead, unavailable: Show a lane first (A)")
    );
}
