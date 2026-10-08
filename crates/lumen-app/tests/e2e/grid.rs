//! The editing grid: Ableton's keys, the grid menu, snapping on the ruler
//! (and Alt to place freely), and the grid saved with the project.

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;

use super::Lumen;

const RULER: &str = "Ruler: click to move the playhead";

#[test]
fn the_keys_change_the_grid_and_the_button_shows_it() {
    let mut lumen = Lumen::start("grid-keys");
    // The default: adaptive, Narrow; eighths at the default zoom.
    assert!(lumen.shows("Grid: 1/8"));
    lumen.key(Modifiers::COMMAND, Key::Num1);
    assert!(lumen.shows("Grid: 1/16"), "Ctrl+1 narrower");
    lumen.key(Modifiers::COMMAND, Key::Num2);
    lumen.key(Modifiers::COMMAND, Key::Num2);
    assert!(lumen.shows("Grid: 1/4"), "Ctrl+2 wider, twice");
    lumen.key(Modifiers::COMMAND, Key::Num3);
    assert!(lumen.shows("Grid: 1/2T"), "Ctrl+3 triplet");
    lumen.key(Modifiers::COMMAND, Key::Num4);
    assert!(lumen.shows("Grid: 1/2T, snap off"), "Ctrl+4 snap off");
    lumen.key(Modifiers::COMMAND, Key::Num4);

    // Ctrl+5 fixes the step showing now; zooming no longer changes it.
    lumen.key(Modifiers::COMMAND, Key::Num5);
    lumen.key(Modifiers::NONE, Key::Equals);
    lumen.frames(30);
    assert!(lumen.shows("Grid: 1/2T"), "fixed through a zoom");
    lumen.key(Modifiers::COMMAND, Key::Num5);
    assert!(
        !lumen.shows("Grid: 1/2T"),
        "adaptive again: finer when zoomed in"
    );
}

#[test]
fn the_ruler_snaps_to_the_grid_chosen_in_the_menu() {
    let mut lumen = Lumen::start("grid-menu");
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    assert!(lumen.shows("Grid: 1 bar"));

    let bar = Ticks::quarters(4).0;
    lumen.click(RULER);
    lumen.wait_for("the playhead to land", |app| app.playhead() > Ticks::ZERO);
    assert_eq!(lumen.app().playhead().0 % bar, 0, "on a bar line");

    // Alt places it freely: the middle of the ruler is not on a bar line.
    lumen
        .harness
        .get_by_label(RULER)
        .click_modifiers(Modifiers::ALT);
    lumen.frames(3);
    lumen.wait_for("the playhead to move", |app| app.playhead().0 % bar != 0);
}

#[test]
fn the_grid_is_saved_with_the_project() {
    let mut lumen = Lumen::start("grid-save");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.click("Grid: 1/8");
    lumen.click("1/16");
    lumen.key(Modifiers::COMMAND, Key::Num3);
    assert!(lumen.shows("Grid: 1/16T"));

    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path));
    again.wait_for("the project to open", |app| !app.is_busy());
    assert!(again.shows("Grid: 1/16T"));
}
