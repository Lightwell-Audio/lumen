//! Locators: set at the playhead (M and the Set button), jumped between,
//! dragged (snapped), renamed, deleted, undone and saved.

use eframe::egui::{Event, Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;

use super::Lumen;

const RULER: &str = "Ruler: click to move the playhead";
const BAR: i64 = Ticks::quarters(4).0;

/// Clicks the bar ruler at `quarters` (the view starts at the song's start).
fn locate(lumen: &mut Lumen, quarters: f64) {
    let ruler = lumen.harness.get_by_label(RULER).rect();
    let x = ruler.left() + (quarters * lumen.app().view().zoom) as f32;
    let at = Pos2::new(x, ruler.center().y);
    lumen.harness.hover_at(at);
    lumen.harness.drag_at(at);
    lumen.frames(1);
    lumen.harness.drop_at(at);
    let target = Ticks::quarters(quarters as i64);
    lumen.wait_for("the playhead", |app| app.playhead() == target);
}

fn locator_times(lumen: &Lumen) -> Vec<(String, i64)> {
    let markers = &lumen.app().project().markers;
    markers.iter().map(|m| (m.name.clone(), m.at.0)).collect()
}

#[test]
fn locators_are_set_at_the_playhead_and_jumped_between() {
    let mut lumen = Lumen::start("locators-set");
    lumen.key(Modifiers::NONE, Key::M);
    assert!(lumen.shows("Locator Locator 1"));
    locate(&mut lumen, 8.0);
    lumen.click("Set locator");
    assert_eq!(
        locator_times(&lumen),
        [("Locator 1".into(), 0), ("Locator 2".into(), 2 * BAR)]
    );

    // Jump back and forth, by key and by button.
    lumen.key(Modifiers::COMMAND, Key::ArrowLeft);
    lumen.wait_for("the previous locator", |app| app.playhead() == Ticks::ZERO);
    lumen.click("Next locator");
    lumen.wait_for("the next locator", |app| app.playhead() == Ticks(2 * BAR));

    // M again where one is removes it; undo brings it back.
    lumen.key(Modifiers::NONE, Key::M);
    assert_eq!(locator_times(&lumen).len(), 1);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(locator_times(&lumen).len(), 2);

    // Clicking a flag jumps there.
    lumen.click("Locator Locator 1");
    lumen.wait_for("the clicked locator", |app| app.playhead() == Ticks::ZERO);
}

#[test]
fn a_locator_is_dragged_renamed_deleted_and_saved() {
    let mut lumen = Lumen::start("locators-edit");
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    lumen.key(Modifiers::NONE, Key::M);

    // Drag the flag to a little past bar 3: it snaps to bar 3, in one step.
    let flag = lumen.harness.get_by_label("Locator Locator 1").rect();
    let from = flag.left_center() + eframe::egui::vec2(3.0, 0.0);
    let zoom = lumen.app().view().zoom as f32;
    let to = from + eframe::egui::vec2(8.3 * zoom, 0.0);
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(from + (to - from) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.harness.drop_at(to);
    lumen.frames(3);
    assert_eq!(locator_times(&lumen), [("Locator 1".into(), 2 * BAR)]);
    assert_eq!(lumen.app().undo_label(), Some("Move locator"));

    // The dragged locator is selected: F2 renames it.
    lumen.key(Modifiers::NONE, Key::F2);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("Chorus".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(locator_times(&lumen), [("Chorus".into(), 2 * BAR)]);

    // Saved and opened again.
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(3);
    assert_eq!(locator_times(&again), [("Chorus".into(), 2 * BAR)]);

    // Selected by a click, Delete removes it and leaves the tracks alone.
    let tracks = again.track_names();
    again.click("Locator Chorus");
    again.key(Modifiers::NONE, Key::Delete);
    assert!(locator_times(&again).is_empty());
    assert_eq!(again.track_names(), tracks);
}
