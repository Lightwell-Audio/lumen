//! Editing an automation lane (M7 slice A2): picking several points with a
//! box or Shift, moving them together, giving segments a shape, bending a
//! segment, resting places for values, the pencil, the shape tools, and copy,
//! paste and duplicate. Every edit is one undo step.

use eframe::egui::{Event, Key, Modifiers};
use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::Id;

use super::Lumen;
use super::lane_view::{click_at, point_pos, points, run_command, spot, tone};

fn q(quarters: i64) -> Ticks {
    Ticks::quarters(quarters)
}

/// A volume lane of points at the beats given, each at its own level, with
/// the lane shown.
fn shown(name: &str, held: &[(i64, f32)]) -> (Lumen, Id) {
    let (mut lumen, track) = tone(name);
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            Lane::new(held.iter().map(|&(b, v)| Point::new(q(b), v)).collect()).unwrap(),
        )
        .unwrap();
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    (lumen, track)
}

fn lane(lumen: &Lumen, track: Id, target: Target) -> Lane {
    lumen.app().project().tracks[0]
        .lane(target)
        .map(|l| l.lane.clone())
        .unwrap_or_else(|| {
            let _ = track;
            Lane::default()
        })
}

fn shift_click(lumen: &mut Lumen, at: eframe::egui::Pos2) {
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::SHIFT));
    click_at(lumen, at);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(2);
}

const FOUR: [(i64, f32); 4] = [(2, 0.0), (4, -6.0), (6, -12.0), (8, -18.0)];

#[test]
fn a_shift_drag_picks_the_points_in_the_box() {
    let (mut lumen, track) = shown("lane-box", &FOUR);
    let from = spot(&lumen, track, 3.0, 6.0);
    let to = spot(&lumen, track, 7.0, -30.0);
    lumen.drag(from, to, Modifiers::SHIFT);
    assert_eq!(
        lumen.app().automation_picked(),
        Some((track, vec![1, 2])),
        "the two points inside the box"
    );
    assert_eq!(
        points(&lumen, track, Target::Volume).len(),
        4,
        "nothing added"
    );
    // A box that catches nothing lets go of the picks.
    let from = spot(&lumen, track, 9.0, 6.0);
    let to = spot(&lumen, track, 10.0, -30.0);
    lumen.drag(from, to, Modifiers::SHIFT);
    assert_eq!(lumen.app().automation_picked(), None);
}

#[test]
fn shift_click_adds_a_point_to_the_picks_and_takes_it_away_again() {
    let (mut lumen, track) = shown("lane-shift", &FOUR);
    let first = point_pos(&lumen, track, 0);
    click_at(&mut lumen, first);
    assert_eq!(lumen.app().automation_picked(), Some((track, vec![0])));
    let third = point_pos(&lumen, track, 2);
    shift_click(&mut lumen, third);
    assert_eq!(lumen.app().automation_picked(), Some((track, vec![0, 2])));
    shift_click(&mut lumen, first);
    assert_eq!(lumen.app().automation_picked(), Some((track, vec![2])));
    assert_eq!(points(&lumen, track, Target::Volume).len(), 4);
}

#[test]
fn picked_points_move_together_in_one_undo_step() {
    let (mut lumen, track) = shown("lane-together", &FOUR);
    let from = spot(&lumen, track, 3.0, 6.0);
    let to = spot(&lumen, track, 7.0, -30.0);
    lumen.drag(from, to, Modifiers::SHIFT);
    // Take hold of the second and move it a beat later and 6 dB down.
    let grab = point_pos(&lumen, track, 1);
    let drop = spot(&lumen, track, 5.0, -12.0);
    lumen.drag(grab, drop, Modifiers::NONE);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.len(), 4);
    assert_eq!(held[0], (q(2), 0.0), "unpicked points stay");
    assert_eq!(held[3], (q(8), -18.0));
    assert_eq!(held[1].0, q(5), "a beat later");
    assert_eq!(held[2].0, q(7), "the other moved with it");
    assert!((held[1].1 + 12.0).abs() < 1.5, "{}", held[1].1);
    assert!(
        (held[2].1 - held[1].1 + 6.0).abs() < 1.5,
        "the same height step for both: {held:?}"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        points(&lumen, track, Target::Volume)[1],
        (q(4), -6.0),
        "one undo puts them all back"
    );
    assert_eq!(points(&lumen, track, Target::Volume)[2], (q(6), -12.0));
}

#[test]
fn a_block_of_picks_stops_against_the_point_beside_it() {
    let (mut lumen, track) = shown("lane-block", &FOUR);
    let from = spot(&lumen, track, 3.0, 6.0);
    let to = spot(&lumen, track, 7.0, -30.0);
    lumen.drag(from, to, Modifiers::SHIFT);
    let grab = point_pos(&lumen, track, 1);
    let drop = spot(&lumen, track, 12.0, -6.0);
    lumen.drag(grab, drop, Modifiers::NONE);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.len(), 4);
    assert!(
        held.windows(2).all(|w| w[0].0 < w[1].0),
        "in order: {held:?}"
    );
    assert!(
        held[2].0 < q(8),
        "the last pick stops before the point at 8"
    );
}

#[test]
fn delete_and_the_arrows_act_on_every_pick() {
    let (mut lumen, track) = shown("lane-all-picks", &FOUR);
    let from = spot(&lumen, track, 3.0, 6.0);
    let to = spot(&lumen, track, 7.0, -30.0);
    lumen.drag(from, to, Modifiers::SHIFT);
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    let held = points(&lumen, track, Target::Volume);
    assert!(held[1].1 < -6.0 && held[2].1 < -12.0, "{held:?}");
    assert_eq!(held[0].1, 0.0);
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!((held[1].0, held[2].0), (q(5), q(7)));
    lumen.key(Modifiers::NONE, Key::Delete);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(held.iter().map(|p| p.0).collect::<Vec<_>>(), [q(2), q(8)]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(points(&lumen, track, Target::Volume).len(), 4);
}

#[test]
fn select_all_copy_paste_and_duplicate_repeat_the_points() {
    let (mut lumen, track) = shown("lane-clip", &[(1, -12.0), (3, -6.0), (5, -12.0)]);
    // A press in the lane gives it the keyboard.
    let first = point_pos(&lumen, track, 0);
    click_at(&mut lumen, first);
    lumen.key(Modifiers::COMMAND, Key::A);
    assert_eq!(
        lumen.app().automation_picked(),
        Some((track, vec![0, 1, 2]))
    );
    lumen.key(Modifiers::COMMAND, Key::C);
    run_command(&mut lumen, "bar 3");
    lumen.wait_for("the playhead at bar 3", |app| {
        (app.playhead() - q(8)).0.abs() < q(1).0 / 1000
    });
    lumen.key(Modifiers::COMMAND, Key::V);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(
        held.iter().map(|p| p.0).collect::<Vec<_>>(),
        [q(1), q(3), q(5), q(8), q(10), q(12)]
    );
    assert_eq!(held[4], (q(10), -6.0));
    assert_eq!(
        lumen.app().automation_picked(),
        Some((track, vec![3, 4, 5])),
        "the pasted points are picked"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(points(&lumen, track, Target::Volume).len(), 3, "one step");

    // Duplicate: the picked run again, right after itself. This run starts
    // and ends at one level, so the copy joins on the last point's beat.
    let first = point_pos(&lumen, track, 0);
    click_at(&mut lumen, first);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.key(Modifiers::COMMAND, Key::D);
    let held = points(&lumen, track, Target::Volume);
    assert_eq!(
        held.iter().map(|p| p.0).collect::<Vec<_>>(),
        [q(1), q(3), q(5), q(7), q(9)]
    );
    assert_eq!(held[3].1, -6.0);
    assert_eq!(held[2].1, -12.0, "the copy joined on the last beat");
}

#[test]
fn pasting_points_into_another_kind_of_parameter_is_refused_and_says_so() {
    let (mut lumen, track) = shown("lane-kind", &[(1, -12.0), (3, -6.0)]);
    let first = point_pos(&lumen, track, 0);
    click_at(&mut lumen, first);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.key(Modifiers::COMMAND, Key::C);
    lumen.click("Automation lane of tone: Volume");
    lumen.click("Pan");
    let at = spot(&lumen, track, 5.0, 0.0);
    click_at(&mut lumen, at);
    lumen.key(Modifiers::COMMAND, Key::V);
    assert_eq!(
        points(&lumen, track, Target::Pan).len(),
        1,
        "only the clicked point: the paste was refused"
    );
    assert!(lumen.shows("Those points belong to another kind of parameter"));
}

#[test]
fn a_value_dragged_near_unity_lands_on_it_and_alt_lets_it_go() {
    let (mut lumen, track) = shown("lane-rest", &[(2, -12.0), (8, -12.0)]);
    let grab = point_pos(&lumen, track, 0);
    let near = spot(&lumen, track, 2.0, -0.4);
    lumen.drag(grab, near, Modifiers::NONE);
    assert_eq!(
        points(&lumen, track, Target::Volume)[0].1,
        0.0,
        "exactly 0 dB"
    );
    let grab = point_pos(&lumen, track, 1);
    let near = spot(&lumen, track, 8.0, -0.4);
    lumen.drag(grab, near, Modifiers::ALT);
    let free = points(&lumen, track, Target::Volume)[1].1;
    assert!(free != 0.0 && free.abs() < 1.5, "free of the rest: {free}");
}

#[test]
fn the_pencil_draws_a_thinned_stroke_in_one_undo_step() {
    let (mut lumen, track) = tone("lane-pencil");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    lumen.key(Modifiers::NONE, Key::B);
    assert_eq!(lumen.app().automation_tool(), "Pencil");
    let from = spot(&lumen, track, 2.0, -40.0);
    let to = spot(&lumen, track, 10.0, -10.0);
    lumen.drag(from, to, Modifiers::NONE);
    let held = points(&lumen, track, Target::Volume);
    assert!(held.len() >= 2, "{held:?}");
    assert!(
        held.len() <= 4,
        "a straight stroke is thinned to its ends: {held:?}"
    );
    assert!(held.first().unwrap().0 < q(3), "{held:?}");
    assert!(held.last().unwrap().0 > q(9), "{held:?}");
    assert!(held.first().unwrap().1 < held.last().unwrap().1, "it rises");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(
        points(&lumen, track, Target::Volume).is_empty(),
        "the whole stroke is one undo step"
    );
    lumen.key(Modifiers::NONE, Key::B);
    assert_eq!(lumen.app().automation_tool(), "Pointer", "B again");
}

/// Picks a shape tool and drags it across beats 4 to 12 from a low level up
/// to a high one.
fn draw(lumen: &mut Lumen, track: Id, command: &str) -> Vec<Point> {
    run_command(lumen, command);
    let from = spot(lumen, track, 4.0, -30.0);
    let to = spot(lumen, track, 12.0, -6.0);
    lumen.drag(from, to, Modifiers::NONE);
    lumen.app().project().tracks[0]
        .lane(Target::Volume)
        .map(|l| l.lane.points().to_vec())
        .unwrap_or_default()
}

fn tone_with_lane(name: &str) -> (Lumen, Id) {
    let (mut lumen, track) = tone(name);
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    (lumen, track)
}

#[test]
fn the_ramp_tool_draws_one_straight_run() {
    let (mut lumen, track) = tone_with_lane("lane-ramp");
    let held = draw(&mut lumen, track, "Automation tool: ramp");
    assert_eq!(held.len(), 2, "{held:?}");
    assert_eq!((held[0].at, held[1].at), (q(4), q(12)));
    assert!(held[0].value < held[1].value);
    assert!(held.iter().all(|p| p.shape == Shape::Linear));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(points(&lumen, track, Target::Volume).is_empty(), "one step");
}

#[test]
fn the_sine_tool_draws_plain_points_that_swing_between_the_two_levels() {
    let (mut lumen, track) = tone_with_lane("lane-sine");
    let held = draw(&mut lumen, track, "Automation tool: sine");
    assert!(held.len() > 16, "{} points", held.len());
    assert_eq!(held[0].at, q(4));
    assert_eq!(held.last().unwrap().at, q(12));
    assert!(held.windows(2).all(|w| w[0].at < w[1].at));
    assert!(
        held.iter().all(|p| p.shape == Shape::Linear),
        "plain points"
    );
    let (low, high) = held.iter().fold((f32::MAX, f32::MIN), |(l, h), p| {
        (l.min(p.value), h.max(p.value))
    });
    assert!(
        (low + 30.0).abs() < 1.5 && (high + 6.0).abs() < 1.5,
        "{low} {high}"
    );
    // It starts where the press was and turns back.
    assert!((held[0].value + 30.0).abs() < 1.5);
    assert!(held.iter().any(|p| p.value > held[0].value + 10.0));
}

#[test]
fn the_triangle_tool_alternates_between_the_two_levels() {
    let (mut lumen, track) = tone_with_lane("lane-triangle");
    let held = draw(&mut lumen, track, "Automation tool: triangle");
    assert!(held.len() >= 8, "{} points", held.len());
    assert!(held.iter().all(|p| p.shape == Shape::Linear));
    let ups = held.windows(2).filter(|w| w[1].value > w[0].value).count();
    let downs = held.windows(2).filter(|w| w[1].value < w[0].value).count();
    assert!(ups >= 3 && downs >= 3, "{ups} up, {downs} down");
}

#[test]
fn the_saw_tool_rises_each_step_and_drops_at_its_end() {
    let (mut lumen, track) = tone_with_lane("lane-saw");
    let held = draw(&mut lumen, track, "Automation tool: saw");
    let drops = held.windows(2).filter(|w| w[1].value < w[0].value).count();
    let rises = held.windows(2).filter(|w| w[1].value > w[0].value).count();
    assert!(drops >= 3, "{drops} drops in {held:?}");
    assert!(rises >= drops, "{rises} rises");
    let drop = held
        .windows(2)
        .find(|w| w[1].value < w[0].value)
        .expect("a drop");
    assert!(
        (drop[1].at - drop[0].at).0 <= 2,
        "the drop takes a tick, not a slope"
    );
}

#[test]
fn the_square_tool_writes_steps() {
    let (mut lumen, track) = tone_with_lane("lane-square");
    let held = draw(&mut lumen, track, "Automation tool: square");
    assert!(held.len() >= 8, "{} points", held.len());
    assert!(held.iter().all(|p| p.shape == Shape::Step), "{held:?}");
    let mut levels: Vec<i32> = held
        .iter()
        .map(|p| (p.value * 10.0).round() as i32)
        .collect();
    levels.sort_unstable();
    levels.dedup();
    assert_eq!(levels.len(), 2, "only the two levels: {levels:?}");
}

#[test]
fn the_tool_strip_picks_the_tool_and_names_every_choice() {
    let (mut lumen, _track) = tone_with_lane("lane-strip");
    assert_eq!(lumen.app().automation_tool(), "Pointer");
    assert!(lumen.is_on("Automation pointer tool of tone"));
    lumen.click("Automation pencil tool of tone");
    assert_eq!(lumen.app().automation_tool(), "Pencil");
    lumen.click("Automation shape tool of tone");
    assert_eq!(
        lumen.app().automation_tool(),
        "Ramp",
        "a shape tool is in hand"
    );
    assert!(lumen.shows("Automation shape of tone: Ramp"));
    lumen.click("Automation shape of tone: Ramp");
    lumen.click("Square");
    assert_eq!(lumen.app().automation_tool(), "Square");
    lumen.click("Automation pointer tool of tone");
    assert_eq!(lumen.app().automation_tool(), "Pointer");
}

#[test]
fn a_command_gives_the_picked_segments_a_shape() {
    let (mut lumen, track) = shown("lane-segments", &[(2, -24.0), (6, 0.0), (10, -12.0)]);
    let first = point_pos(&lumen, track, 0);
    click_at(&mut lumen, first);
    run_command(&mut lumen, "Make the automation segments steps");
    assert_eq!(
        lane(&lumen, track, Target::Volume).points()[0].shape,
        Shape::Step
    );
    assert_eq!(
        lane(&lumen, track, Target::Volume).points()[1].shape,
        Shape::Linear,
        "only the picked point's segment"
    );
    run_command(&mut lumen, "Make the automation segments curved");
    assert!(matches!(
        lane(&lumen, track, Target::Volume).points()[0].shape,
        Shape::Curve(_)
    ));
    run_command(&mut lumen, "Make the automation segments straight");
    assert_eq!(
        lane(&lumen, track, Target::Volume).points()[0].shape,
        Shape::Linear
    );
    // Each was one step.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(matches!(
        lane(&lumen, track, Target::Volume).points()[0].shape,
        Shape::Curve(_)
    ));
}

#[test]
fn dragging_the_dot_on_a_segment_bends_it_in_one_undo_step() {
    let (mut lumen, track) = shown("lane-bend", &[(2, -30.0), (10, -6.0)]);
    let ctx = lumen.harness.ctx.clone();
    assert!(
        lumen.app().automation_bend_pos(&ctx, track, 0).is_some(),
        "a dot sits on the segment"
    );
    let first = point_pos(&lumen, track, 0);
    click_at(&mut lumen, first);
    let dot = lumen.app().automation_bend_pos(&ctx, track, 0).unwrap();
    // Pull the dot down and to the right: slow at first, then quick.
    lumen.drag(dot, dot + eframe::egui::vec2(30.0, 14.0), Modifiers::NONE);
    let bent = lane(&lumen, track, Target::Volume);
    assert!(
        matches!(bent.points()[0].shape, Shape::Curve(_)),
        "{:?}",
        bent.points()[0]
    );
    let mid = bent.value_at(q(6)).unwrap();
    assert!(mid < -18.0 - 0.5, "below the straight line's -18 dB: {mid}");
    assert_eq!(bent.len(), 2, "no point was added");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        lane(&lumen, track, Target::Volume).points()[0].shape,
        Shape::Linear
    );
}

#[test]
fn a_tooltip_shows_the_value_while_a_point_moves() {
    let (mut lumen, track) = shown("lane-tip", &[(2, -12.0), (8, -12.0)]);
    let grab = point_pos(&lumen, track, 0);
    lumen.harness.hover_at(grab);
    lumen.harness.drag_at(grab);
    lumen.frames(2);
    lumen.harness.hover_at(spot(&lumen, track, 3.0, -20.0));
    lumen.frames(3);
    let tip = lumen
        .app()
        .automation_tip()
        .expect("a tip while a point moves");
    assert!(tip.contains("dB"), "{tip}");
    assert!(tip.contains('.'), "and where in the song: {tip}");
    lumen.harness.drop_at(spot(&lumen, track, 3.0, -20.0));
    lumen.frames(3);
}

#[test]
fn a_shape_on_a_mute_lane_is_made_of_steps() {
    let (mut lumen, track) = tone_with_lane("lane-mute-shape");
    lumen.click("Automation lane of tone: Volume");
    lumen.click("Mute");
    run_command(&mut lumen, "Automation tool: sine");
    let from = spot(&lumen, track, 4.0, 0.0);
    let to = spot(&lumen, track, 12.0, 1.0);
    lumen.drag(from, to, Modifiers::NONE);
    let held = lane(&lumen, track, Target::Mute);
    assert!(held.len() >= 4, "{} points", held.len());
    assert!(
        held.points().iter().all(|p| p.shape == Shape::Step),
        "a mute lane holds each value: {:?}",
        held.points()
    );
    assert!(
        held.points()
            .iter()
            .all(|p| p.value == 0.0 || p.value == 1.0)
    );
}
