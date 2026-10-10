//! Returns and the master pinned at the bottom (M4 slice 4, the owner's
//! A1): lettered returns under a divider and a caption that folds them,
//! the master last, the area resized at its divider and saved.

use eframe::egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;

use super::Lumen;

/// The header of track `name`, on screen.
fn header(lumen: &Lumen, name: &str) -> eframe::egui::Rect {
    lumen
        .harness
        .get_by_label_contains(&format!("Track {name},"))
        .rect()
}

fn divider(lumen: &Lumen) -> eframe::egui::Rect {
    lumen
        .harness
        .get_by_label("Returns and master divider")
        .rect()
}

#[test]
fn returns_and_the_master_sit_pinned_under_the_divider_and_fold_away() {
    let mut lumen = Lumen::start("returns-pinned");
    lumen.key(Modifiers::COMMAND, Key::T);
    // Without returns, the master alone sits under the divider.
    assert!(lumen.shows("Master track"));
    assert!(!lumen.shows("Fold returns"));
    let master = lumen.harness.get_by_label("Master track").rect();
    assert!(master.top() > divider(&lumen).bottom());

    lumen.click("Add track");
    lumen.click("Add return track");
    lumen.click("Add track");
    lumen.click("Add return track");
    // A track added afterwards goes among the tracks, above the divider.
    lumen.key(Modifiers::COMMAND, Key::T);
    let line = divider(&lumen).bottom();
    let (a, b) = (header(&lumen, "Return 1"), header(&lumen, "Return 2"));
    assert!(
        a.top() >= line && b.top() >= a.bottom() - 1.0,
        "returns under it"
    );
    assert!(header(&lumen, "Audio 2").bottom() <= divider(&lumen).top() + 1.0);
    let master = lumen.harness.get_by_label("Master track").rect();
    assert!(master.top() >= b.bottom() - 1.0, "the master last");
    // Returns record nothing: no arm button.
    assert!(!lumen.shows("Arm Return 1"));
    assert!(lumen.shows("Arm Audio 1"));

    // The caption folds them away; Down skips them then.
    lumen.click("Fold returns");
    assert!(lumen.is_on("Fold returns"));
    assert!(!lumen.shows("Mute Return 1"));
    assert!(lumen.app().project_as_saved().view.returns_folded);
    lumen.click("Fold returns");
    assert!(lumen.shows("Mute Return 1"));
}

#[test]
fn the_divider_resizes_the_area_and_double_click_restores_its_natural_size() {
    let mut lumen = Lumen::start("returns-divider");
    for _ in 0..3 {
        lumen.click("Add track");
        lumen.click("Add return track");
    }
    // New tracks grow in and moved rows glide (M4.5): let them settle.
    lumen.frames(30);
    let saved = |lumen: &Lumen| lumen.app().project_as_saved().view.returns_height;
    assert_eq!(saved(&lumen), None, "its natural size");
    let line = divider(&lumen).center();
    // Dragged down by a return's height: the last return scrolls inside.
    lumen.drag(line, line + vec2(0.0, 44.0), Modifiers::NONE);
    let height = saved(&lumen).expect("a dragged size is saved");
    let master = lumen.harness.get_by_label("Master track").rect();
    assert!(master.top() > divider(&lumen).bottom(), "the master stays");
    assert!((divider(&lumen).center().y - (line.y + 44.0)).abs() < 2.0);
    // Not an edit.
    assert_eq!(lumen.app().undo_label(), Some("Add return track"));
    // Dragged back past its natural size, it fits its rows again.
    let now = divider(&lumen).center();
    lumen.drag(now, now - vec2(0.0, 200.0), Modifiers::NONE);
    assert_eq!(saved(&lumen), None, "{height} then back to natural");
    assert!((divider(&lumen).center().y - line.y).abs() < 2.0);
    let now = divider(&lumen).center();
    lumen.drag(now, now + vec2(0.0, 30.0), Modifiers::NONE);
    assert!(saved(&lumen).is_some());
    let now = divider(&lumen).center();
    lumen.double_click(now);
    assert_eq!(saved(&lumen), None, "double-click: natural size");
}

#[test]
fn the_masters_volume_is_set_from_its_unfolded_row() {
    let mut lumen = Lumen::start("returns-master");
    lumen.click("Fold Master");
    for _ in 0..100 {
        if lumen
            .harness
            .query_by_label_contains("Volume Master")
            .is_some()
        {
            break;
        }
        lumen.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let volume = lumen.harness.get_by_label_contains("Volume Master").rect();
    lumen.drag(
        volume.center(),
        volume.center() - vec2(60.0, 0.0),
        Modifiers::NONE,
    );
    assert!(lumen.app().project().master.gain_db < 0.0);
    assert_eq!(lumen.app().undo_label(), Some("Change volume"));
}
