//! The tempo and meter lanes (M7 slice A6): shown with T, a click adds a
//! change, a drag moves it, a double-click types it, Delete takes it away;
//! the bar fields follow the playhead through the changes; every edit is one
//! undo step, and the keyboard reaches all of it.

use eframe::egui::{Event, Key, Modifiers, pos2, vec2};
use lumen_core::time::Ticks;

use super::Lumen;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

use super::lane_view::{click_at, run_command};

fn q(quarters: i64) -> Ticks {
    Ticks::quarters(quarters)
}

/// A new project with the lanes shown and settled.
fn start(name: &str) -> Lumen {
    let mut lumen = Lumen::start(name);
    lumen.key(Modifiers::NONE, Key::T);
    lumen.frames(40);
    assert!(lumen.app().song_lanes_shown());
    lumen
}

fn click_tempo(lumen: &mut Lumen, quarters: f64, bpm: f64) {
    // A point at the start sits on the lane's edge: press a little inside it.
    let nudge = if quarters == 0.0 { 3.0 } else { 0.0 };
    let at = lumen.app().song_tempo_spot(quarters, bpm).unwrap() + vec2(nudge, 0.0);
    click_at(lumen, at);
}

fn click_meter(lumen: &mut Lumen, quarters: f64) {
    let at = lumen.app().song_meter_spot(quarters).unwrap();
    click_at(lumen, at);
}

fn click_chip(lumen: &mut Lumen, index: usize) {
    let at = lumen.app().song_chip_spot(index).unwrap();
    click_at(lumen, at);
}

fn tempos(lumen: &Lumen) -> Vec<(Ticks, f64)> {
    lumen
        .app()
        .project()
        .tempo
        .iter()
        .map(|p| (p.at, p.bpm))
        .collect()
}

/// Types `text` into the box that is open and applies it.
fn type_in(lumen: &mut Lumen, text: &str) {
    assert!(lumen.app().song_typing_open(), "a box to type in is open");
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(text.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

#[test]
fn the_lanes_show_and_hide_with_a_key_and_are_named_for_the_access_tree() {
    let mut lumen = Lumen::start("song-show");
    assert!(!lumen.app().song_lanes_shown());
    assert!(!lumen.shows("Tempo lane: click to add a tempo change, drag a point to move it, double-click a point to type its tempo"));
    lumen.key(Modifiers::NONE, Key::T);
    lumen.frames(40);
    assert!(lumen.app().song_lanes_shown());
    assert!(
        lumen
            .harness
            .query_by_label_contains("Tempo lane: click to add")
            .is_some()
    );
    assert!(
        lumen
            .harness
            .query_by_label_contains("Meter lane: click to add")
            .is_some()
    );
    lumen.key(Modifiers::NONE, Key::T);
    lumen.frames(40);
    assert!(!lumen.app().song_lanes_shown());
    assert!(
        lumen
            .harness
            .query_by_label_contains("Tempo lane: click to add")
            .is_none()
    );
}

#[test]
fn a_click_adds_a_tempo_change_where_it_lands_and_undo_takes_it_back() {
    let mut lumen = start("song-add-tempo");
    let at = lumen.app().song_tempo_spot(8.0, 100.0).unwrap();
    click_at(&mut lumen, at);
    assert_eq!(tempos(&lumen), [(Ticks::ZERO, 120.0), (q(8), 100.0)]);
    assert_eq!(
        lumen.app().song_picked(),
        Some(("tempo", 1)),
        "it is picked"
    );
    assert_eq!(lumen.app().undo_label(), Some("Add tempo change"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(tempos(&lumen), [(Ticks::ZERO, 120.0)], "one step");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(tempos(&lumen).len(), 2, "redo brings it back");
}

#[test]
fn a_drag_moves_a_tempo_change_in_time_and_value_as_one_step() {
    let mut lumen = start("song-move-tempo");
    click_tempo(&mut lumen, 8.0, 100.0);
    let from = lumen.app().song_tempo_spot(8.0, 100.0).unwrap();
    let to = lumen.app().song_tempo_spot(16.0, 110.0).unwrap();
    lumen.drag(from, to, Modifiers::NONE);
    assert_eq!(tempos(&lumen), [(Ticks::ZERO, 120.0), (q(16), 110.0)]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        tempos(&lumen),
        [(Ticks::ZERO, 120.0), (q(8), 100.0)],
        "the whole drag was one step"
    );
}

#[test]
fn the_first_tempo_stays_at_zero_and_only_changes_value() {
    let mut lumen = start("song-first-tempo");
    // The first point sits on the lane's left edge: press a little inside it.
    let from = lumen.app().song_tempo_spot(0.0, 120.0).unwrap() + vec2(3.0, 0.0);
    let to = pos2(
        lumen.app().song_tempo_spot(8.0, 120.0).unwrap().x,
        lumen.app().song_tempo_spot(0.0, 130.0).unwrap().y,
    );
    lumen.drag(from, to, Modifiers::NONE);
    let held = tempos(&lumen);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].0, Ticks::ZERO);
    assert!(held[0].1 > 120.0, "{held:?}");
}

#[test]
fn a_double_click_types_a_tempo() {
    let mut lumen = start("song-type-tempo");
    click_tempo(&mut lumen, 8.0, 100.0);
    let point = lumen.app().song_tempo_spot(8.0, 100.0).unwrap();
    lumen.double_click(point);
    type_in(&mut lumen, "93.5");
    assert_eq!(tempos(&lumen)[1], (q(8), 93.5));
    // Not a tempo: nothing changes.
    let point = lumen.app().song_tempo_spot(8.0, 93.5).unwrap();
    lumen.double_click(point);
    type_in(&mut lumen, "fast");
    assert_eq!(tempos(&lumen)[1], (q(8), 93.5));
    // Esc cancels.
    lumen.double_click(point);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("77".into()));
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(tempos(&lumen)[1], (q(8), 93.5));
    assert!(!lumen.app().song_typing_open());
}

#[test]
fn delete_takes_a_picked_change_away_but_never_the_first() {
    let mut lumen = start("song-delete-tempo");
    click_tempo(&mut lumen, 8.0, 100.0);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(tempos(&lumen).len(), 1, "the change is gone");
    click_tempo(&mut lumen, 0.0, 120.0);
    assert_eq!(lumen.app().song_picked(), Some(("tempo", 0)));
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(tempos(&lumen).len(), 1, "the first stays");
}

#[test]
fn a_click_on_the_meter_lane_asks_for_a_signature_on_the_nearest_bar_line() {
    let mut lumen = start("song-add-meter");
    // 9 quarters is nearest the bar line at 8.
    let at = lumen.app().song_meter_spot(9.0).unwrap();
    click_at(&mut lumen, at);
    type_in(&mut lumen, "7/8");
    let meter = lumen.app().project().meter.clone();
    assert_eq!(meter.len(), 2);
    assert_eq!(meter[1].at, q(8));
    assert_eq!(
        (meter[1].meter.numerator, meter[1].meter.denominator),
        (7, 8)
    );
    assert_eq!(lumen.app().undo_label(), Some("Add time signature"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().meter.len(), 1);
    assert_eq!(lumen_project::validate(lumen.app().project()), Ok(()));
}

#[test]
fn a_chip_moves_to_another_bar_line_and_types_its_signature() {
    let mut lumen = start("song-move-meter");
    click_meter(&mut lumen, 8.0);
    type_in(&mut lumen, "3/4");
    let from = lumen.app().song_chip_spot(1).unwrap();
    let step =
        lumen.app().song_meter_spot(16.0).unwrap().x - lumen.app().song_meter_spot(8.0).unwrap().x;
    lumen.drag(from, pos2(from.x + step, from.y), Modifiers::NONE);
    assert_eq!(lumen.app().project().meter[1].at, q(16));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().meter[1].at, q(8), "one step");
    // A double-click on the chip types another signature.
    let chip = lumen.app().song_chip_spot(1).unwrap();
    lumen.double_click(chip);
    type_in(&mut lumen, "5/4");
    let m = lumen.app().project().meter[1].meter;
    assert_eq!((m.numerator, m.denominator), (5, 4));
    // Delete takes it away; the first stays.
    click_chip(&mut lumen, 1);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(lumen.app().project().meter.len(), 1);
}

#[test]
fn the_bar_field_shows_the_tempo_at_the_playhead() {
    let mut lumen = start("song-bar-fields");
    click_tempo(&mut lumen, 8.0, 100.0);
    let shown = |lumen: &Lumen| {
        lumen
            .harness
            .get_by_label("Tempo")
            .accesskit_node()
            .numeric_value()
    };
    assert_eq!(shown(&lumen), Some(120.0), "the playhead is at the start");
    // Click the bar ruler past the change.
    let ruler = lumen
        .harness
        .get_by_label("Ruler: click to move the playhead")
        .rect();
    let x = lumen.app().song_tempo_spot(12.0, 100.0).unwrap().x;
    click_at(&mut lumen, pos2(x, ruler.center().y));
    lumen.frames(5);
    assert_eq!(shown(&lumen), Some(100.0));
}

#[test]
fn the_keyboard_reaches_the_lanes_through_the_palette() {
    let mut lumen = Lumen::start("song-keyboard");
    run_command(&mut lumen, "Add a tempo change at the playhead");
    lumen.frames(40);
    assert!(lumen.app().song_lanes_shown(), "the lane was shown for it");
    // At the start the change replaces the first tempo's place.
    type_in(&mut lumen, "90");
    assert_eq!(tempos(&lumen), [(Ticks::ZERO, 90.0)]);
    run_command(&mut lumen, "Add a time signature change at the playhead");
    type_in(&mut lumen, "6/8");
    let m = lumen.app().project().meter[0].meter;
    assert_eq!(
        (m.numerator, m.denominator),
        (6, 8),
        "at the start it is the first"
    );
}

#[test]
fn the_corner_button_shows_and_hides_the_lanes_for_anyone_who_does_not_know_the_key() {
    let mut lumen = Lumen::start("song-button");
    assert!(!lumen.app().song_lanes_shown());
    assert!(!lumen.is_on("Tempo and meter lanes"));
    lumen.click("Tempo and meter lanes");
    lumen.frames(40);
    assert!(lumen.app().song_lanes_shown());
    assert!(lumen.is_on("Tempo and meter lanes"), "lit while shown");
    lumen.click("Tempo and meter lanes");
    lumen.frames(40);
    assert!(!lumen.app().song_lanes_shown());
    assert!(!lumen.is_on("Tempo and meter lanes"));
}

#[test]
fn the_bar_rulers_menu_shows_and_hides_the_lanes() {
    let mut lumen = Lumen::start("song-ruler-menu");
    let ruler = lumen
        .harness
        .get_by_label("Ruler: click to move the playhead")
        .rect();
    lumen.right_click(ruler.center());
    let entry = |lumen: &Lumen| {
        lumen
            .harness
            .query_all_by_label_contains("Tempo and meter lanes")
            .find(|node| node.rect().left() >= ruler.left())
            .map(|node| node.rect().center())
    };
    let at = entry(&lumen).expect("the menu has an entry for the lanes");
    click_at(&mut lumen, at);
    lumen.frames(40);
    assert!(lumen.app().song_lanes_shown());
    // The entry is on while they are shown.
    lumen.right_click(ruler.center());
    let at = entry(&lumen).expect("the entry is still there");
    click_at(&mut lumen, at);
    lumen.frames(40);
    assert!(!lumen.app().song_lanes_shown());
}

#[test]
fn a_tempo_change_ramps_to_the_next_from_its_menu_and_the_bar_follows_the_glide() {
    let mut lumen = start("song-ramp");
    click_tempo(&mut lumen, 8.0, 100.0);
    assert_eq!(tempos(&lumen).len(), 2);
    // Right-click the first change: it can ramp to the one after it.
    let first = lumen.app().song_tempo_spot(0.0, 120.0).unwrap() + vec2(3.0, 0.0);
    lumen.right_click(first);
    lumen.click("Ramp to the next change");
    assert!(lumen.app().project().tempo[0].ramp);
    assert_eq!(lumen.app().undo_label(), Some("Ramp tempo"));
    // Half way along, the tempo is half way between the two.
    let ruler = lumen
        .harness
        .get_by_label("Ruler: click to move the playhead")
        .rect();
    let x = lumen.app().song_tempo_spot(4.0, 110.0).unwrap().x;
    click_at(&mut lumen, pos2(x, ruler.center().y));
    lumen.frames(5);
    let shown = lumen
        .harness
        .get_by_label("Tempo")
        .accesskit_node()
        .numeric_value()
        .unwrap();
    assert!((shown - 110.0).abs() < 1.0, "{shown}");
    // The last change has no next one to ramp to.
    let last = lumen.app().song_tempo_spot(8.0, 100.0).unwrap();
    lumen.right_click(last);
    lumen.frames(2);
    assert!(lumen.shows("Ramp to the next change"), "it is listed");
    lumen.key(Modifiers::NONE, Key::Escape);
    // One undo step puts the glide away; the file says so too.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!lumen.app().project().tempo[0].ramp);
    assert_eq!(lumen_project::validate(lumen.app().project()), Ok(()));
}

#[test]
fn the_palette_ramps_and_holds_the_picked_change() {
    let mut lumen = start("song-ramp-palette");
    click_tempo(&mut lumen, 8.0, 100.0);
    click_tempo(&mut lumen, 0.0, 120.0);
    run_command(&mut lumen, "Ramp or hold the picked tempo change");
    assert!(lumen.app().project().tempo[0].ramp);
    run_command(&mut lumen, "Ramp or hold the picked tempo change");
    assert!(!lumen.app().project().tempo[0].ramp, "it holds again");
    // The last change cannot ramp: the command says so and does nothing.
    click_tempo(&mut lumen, 8.0, 100.0);
    run_command(&mut lumen, "Ramp or hold the picked tempo change");
    assert!(!lumen.app().project().tempo[1].ramp);
}
