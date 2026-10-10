//! The Mixer tab (M4 slice 5): a strip per track in the arrangement's order
//! and folding, returns after the tracks and the master pinned at the
//! right, every control by name, routing from a strip, and the loudness
//! column.

use eframe::egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_project::model::Output;

use super::Lumen;

/// Three audio tracks, the first two grouped (Group 1 › Audio 1, Audio 2),
/// Audio 3 outside, and a return; the mixer tall enough for every section.
fn mixed(name: &str) -> Lumen {
    mixed_with(name, 1)
}

/// As [`mixed`], with `returns` returns.
fn mixed_with(name: &str, returns: usize) -> Lumen {
    let mut lumen = Lumen::start(name);
    for _ in 0..3 {
        lumen.key(Modifiers::COMMAND, Key::T);
    }
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.key(Modifiers::SHIFT, Key::ArrowUp);
    lumen.key(Modifiers::COMMAND, Key::G);
    for _ in 0..returns {
        lumen.click("Add track");
        lumen.click("Add return track");
    }
    tall_mixer(&mut lumen);
    lumen
}

/// Drags the detail panel taller, so every strip section shows (a short
/// panel folds them away, the owner's B7).
fn tall_mixer(lumen: &mut Lumen) {
    lumen.app_mut().set_detail_size(480.0);
    lumen.frames(3);
    assert!(
        lumen.app().detail_size() > 470.0,
        "{}",
        lumen.app().detail_size()
    );
}

fn strip(name: &str) -> String {
    format!("Mixer strip: {name},")
}

fn track<'a>(lumen: &'a Lumen, name: &str) -> &'a lumen_project::model::Track {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.name == name)
        .expect("track")
}

/// The strips on screen, left to right.
fn strips(lumen: &Lumen) -> Vec<String> {
    let mut found: Vec<(f32, String)> = lumen
        .harness
        .query_all_by_label_contains("Mixer strip: ")
        .map(|n| {
            let label = n.accesskit_node().label().unwrap_or_default();
            let name = label
                .trim_start_matches("Mixer strip: ")
                .split(',')
                .next()
                .unwrap_or_default()
                .to_owned();
            (n.rect().left(), name)
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, n)| n).collect()
}

/// Clicks once at the middle of the control named like `label`.
fn press(lumen: &mut Lumen, label: &str) {
    let at = lumen.harness.get_by_label_contains(label).rect().center();
    lumen.harness.hover_at(at);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    lumen.frames(3);
}

#[test]
fn strips_follow_the_arrangement_with_returns_after_and_one_fold_for_both() {
    let mut lumen = mixed("mixer-order");
    assert_eq!(
        strips(&lumen),
        [
            "Group 1", "Audio 1", "Audio 2", "Audio 3", "Return 1", "Master"
        ],
        "tracks in order, then the returns, then the master"
    );
    // The master is pinned at the right, with the loudness column.
    assert!(lumen.shows("Mute Master in the mixer"));
    assert!(lumen.shows("Reset integrated loudness"));

    // Folding the group in the arrangement folds it in the mixer (B4)...
    lumen.click("Fold group Group 1");
    assert_eq!(strips(&lumen), ["Group 1", "Audio 3", "Return 1", "Master"]);
    // ...and unfolding it from its strip shows it in both again.
    lumen.click("Fold group Group 1 in the mixer");
    assert_eq!(strips(&lumen).len(), 6);
    assert!(lumen.shows("Audio 1: double-click to rename"));

    // A click on a strip selects its track, as a click on its header does.
    lumen
        .harness
        .get_by_label_contains(&strip("Audio 3"))
        .click();
    lumen.frames(3);
    let id = track(&lumen, "Audio 3").id;
    assert_eq!(lumen.app().selected_tracks(), [id]);
}

#[test]
fn every_strip_control_sets_its_track_in_one_undo_step() {
    let mut lumen = mixed("mixer-controls");

    // The fader: dragged up, one step.
    let fader = lumen
        .harness
        .get_by_label_contains("Audio 3 fader in the mixer:")
        .rect();
    lumen.drag(
        fader.center(),
        fader.center() - vec2(0.0, 30.0),
        Modifiers::NONE,
    );
    let after_drag = track(&lumen, "Audio 3").gain_db;
    assert!(after_drag > 0.5, "{after_drag}");
    assert_eq!(lumen.app().undo_label(), Some("Change volume"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(track(&lumen, "Audio 3").gain_db, 0.0);

    // The volume typed in the field under the fader.
    press(&mut lumen, "Audio 3 volume in the mixer:");
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("-12".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(track(&lumen, "Audio 3").gain_db, -12.0);

    // Pan, dragged hard right.
    let pan = lumen
        .harness
        .get_by_label_contains("Audio 3 pan in the mixer:")
        .rect();
    lumen.drag(
        pan.center(),
        pan.center() + vec2(300.0, 0.0),
        Modifiers::NONE,
    );
    assert_eq!(track(&lumen, "Audio 3").pan, 1.0);

    // Polarity and trim.
    lumen.click("Invert polarity of Audio 3 in the mixer");
    assert!(track(&lumen, "Audio 3").invert);
    let trim = lumen
        .harness
        .get_by_label_contains("Audio 3 trim in the mixer:")
        .rect();
    lumen.drag(
        trim.center(),
        trim.center() + vec2(20.0, 0.0),
        Modifiers::NONE,
    );
    assert!(track(&lumen, "Audio 3").trim_db > 1.0);
    assert_eq!(lumen.app().undo_label(), Some("Change input trim"));

    // The send to the return, turned up from the strip (O2).
    let send = lumen
        .harness
        .get_by_label_contains("Send Audio 3 to A Return 1 in the mixer")
        .rect();
    lumen.drag(
        send.center(),
        send.center() - vec2(0.0, 80.0),
        Modifiers::NONE,
    );
    let level = track(&lumen, "Audio 3").sends[0].level.0;
    assert!(level > -30.0, "{level}");

    // Mute and solo, as on the header.
    lumen.click("Mute Audio 3 in the mixer");
    assert!(track(&lumen, "Audio 3").mute);
    assert!(lumen.is_on("Mute Audio 3"), "the header shows it too");
    lumen.click("Solo Audio 3 in the mixer");
    assert!(track(&lumen, "Audio 3").solo);

    // Renamed from its strip.
    let name = lumen
        .harness
        .get_by_label("Audio 3 in the mixer: double-click to rename")
        .rect();
    lumen.double_click(name.center());
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("Lead".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(lumen.track_names().contains(&"Lead".to_owned()));
}

#[test]
fn the_output_menu_routes_into_a_group_and_never_offers_a_loop() {
    let mut lumen = mixed("mixer-output");
    lumen.click("Audio 3 output in the mixer: Master");
    lumen.click("Group 1");
    let group = track(&lumen, "Group 1").id;
    assert_eq!(track(&lumen, "Audio 3").output, Output::Track(group));
    assert!(lumen.shows("Audio 3 output in the mixer: Group 1"));
    assert_eq!(lumen.app().undo_label(), Some("Change output"));

    // A group is never offered as its own output.
    lumen.click("Group 1 output in the mixer: Master");
    assert!(!lumen.shows("Group 1"), "Group 1 offered as its own output");
    assert!(lumen.shows("Master"));
}

#[test]
fn the_loudness_column_resets_on_click() {
    let mut lumen = mixed("mixer-loudness");
    lumen.click("Reset integrated loudness");
    let (_, _, integrated, true_peak) = lumen.app().loudness();
    assert!(integrated.is_infinite() && true_peak.is_infinite());
    let readout = lumen.harness.get_by_label_contains("Loudness: integrated");
    let said = readout
        .accesskit_node()
        .label()
        .or_else(|| readout.accesskit_node().value())
        .unwrap_or_default();
    assert!(
        said.contains('\u{2014}'),
        "nothing measured reads as a dash: {said}"
    );
}

/// A point on `name`'s strip clear of its controls: beside its meter.
fn grip(lumen: &Lumen, name: &str) -> eframe::egui::Pos2 {
    let rect = lumen.harness.get_by_label_contains(&strip(name)).rect();
    eframe::egui::pos2(rect.right() - 3.0, rect.center().y)
}

#[test]
fn dragging_a_strip_moves_its_track_and_returns_reorder_among_themselves() {
    let mut lumen = mixed_with("mixer-drag", 2);
    // Audio 3 onto the middle of the group's strip: it goes in at the end.
    let to = lumen
        .harness
        .get_by_label_contains(&strip("Group 1"))
        .rect()
        .center();
    let from = grip(&lumen, "Audio 3");
    lumen.drag(from, to, Modifiers::NONE);
    let group = track(&lumen, "Group 1").id;
    assert_eq!(track(&lumen, "Audio 3").parent, Some(group));
    assert_eq!(lumen.app().undo_label(), Some("Move track"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(track(&lumen, "Audio 3").parent, None);

    // The second return, dragged before the first: the letters follow.
    let names = |lumen: &Lumen| -> Vec<String> {
        lumen
            .app()
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == lumen_project::model::TrackKind::Return)
            .map(|t| t.name.clone())
            .collect()
    };
    assert_eq!(names(&lumen), ["Return 1", "Return 2"]);
    let first = lumen
        .harness
        .get_by_label_contains(&strip("Return 1"))
        .rect();
    let from = grip(&lumen, "Return 2");
    lumen.drag(
        from,
        eframe::egui::pos2(first.left() + 6.0, first.center().y),
        Modifiers::NONE,
    );
    assert_eq!(names(&lumen), ["Return 2", "Return 1"]);
    // Returns stay among returns: a track strip takes no return.
    let audio = grip(&lumen, "Audio 3");
    let from = grip(&lumen, "Return 1");
    lumen.drag(from, audio, Modifiers::NONE);
    let last = lumen.app().project().tracks.last().map(|t| t.name.clone());
    assert_ne!(last.as_deref(), Some("Audio 3"));
    assert!(lumen.track_names().iter().any(|n| n == "Return 1"));
}

#[test]
fn the_pan_law_is_chosen_from_the_mixers_menu_and_the_palette() {
    use lumen_core::pan::PanLaw;
    let mut lumen = mixed("mixer-pan-law");
    assert_eq!(lumen.app().project().pan_law, PanLaw::ConstantPower);
    // Right-click the mixer's empty area, left of the master.
    let master = lumen.harness.get_by_label_contains(&strip("Master")).rect();
    lumen.right_click(eframe::egui::pos2(master.left() - 60.0, master.center().y));
    lumen.click("\u{2212}6 dB, linear");
    assert_eq!(lumen.app().project().pan_law, PanLaw::Linear);
    assert_eq!(lumen.app().undo_label(), Some("Change pan law"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().pan_law, PanLaw::ConstantPower);

    // And from the palette.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("Pan law: 0 dB".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.app().project().pan_law, PanLaw::Balance);
}
