//! The Monitor badge on MIDI track headers (M5 slice 2 fix, the owner's
//! MA): shown only for In and Off, never for Auto and never on an audio
//! track; a click cycles In, Off, Auto, one undo step each; it sits in the
//! Tab order before M.

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_project::model::{Color, Id, Monitor, Project, Track, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

fn project() -> Project {
    let mut lead = midi_track(31, "Lead", "poly", (0, 8), None, vec![], vec![]);
    lead.monitor = Monitor::In;
    let mut pad = midi_track(51, "Pad", "poly", (0, 8), None, vec![], vec![]);
    pad.monitor = Monitor::Off;
    let mut vox = Track::new(Id(21), "Vox", TrackKind::Audio, Color(0x4F81BD));
    // Monitor is kept on every kind of track, but only MIDI tracks show it.
    vox.monitor = Monitor::In;
    project_of(
        "Badge",
        vec![
            midi_track(1, "Keys", "poly", (0, 8), None, vec![], vec![]),
            lead,
            pad,
            vox,
        ],
    )
}

fn launch(name: &str) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(), &path, &Default::default()).expect("save");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 4);
    lumen.frames(30);
    lumen
}

fn monitor_of(lumen: &Lumen, id: Id) -> Monitor {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == id)
        .expect("the track")
        .monitor
}

/// The name of the control with the keyboard focus.
fn focused(lumen: &Lumen) -> String {
    lumen
        .harness
        .query_all(egui_kittest::kittest::by().predicate(|n| n.is_focused()))
        .next()
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .unwrap_or_default()
}

#[test]
fn the_badge_shows_for_in_and_off_on_midi_tracks_only() {
    let lumen = launch("badge-shows");
    assert!(lumen.shows("Monitor Lead: In"));
    assert!(lumen.shows("Monitor Pad: Off"));
    for none in ["Monitor Keys", "Monitor Vox"] {
        assert!(
            lumen
                .harness
                .query_all_by_label_contains(none)
                .next()
                .is_none(),
            "{none} shows no badge"
        );
    }
}

#[test]
fn clicking_cycles_in_off_auto_and_undo_restores() {
    let mut lumen = launch("badge-cycle");
    lumen.click("Monitor Lead: In");
    assert_eq!(monitor_of(&lumen, Id(31)), Monitor::Off);
    assert_eq!(lumen.app().undo_label(), Some("Monitor: Off"));
    assert!(lumen.shows("Monitor Lead: Off"));
    lumen.click("Monitor Lead: Off");
    assert_eq!(monitor_of(&lumen, Id(31)), Monitor::Auto);
    assert!(!lumen.shows("Monitor Lead: Off") && !lumen.shows("Monitor Lead: In"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(monitor_of(&lumen, Id(31)), Monitor::Off);
    assert!(lumen.shows("Monitor Lead: Off"));
}

#[test]
fn the_badge_is_in_the_tab_order_before_mute_and_space_cycles_it() {
    let mut lumen = launch("badge-tab");
    let mut order = Vec::new();
    // Tab until the mute button: how far that is varies, because the bar can
    // gain a stop (the dropout chip appears on a machine too busy to keep the
    // audio fed).
    for _ in 0..300 {
        lumen.key(Modifiers::NONE, Key::Tab);
        order.push(focused(&lumen));
        if order.last().is_some_and(|n| n == "Mute Lead") {
            break;
        }
    }
    let badge = order.iter().position(|n| n == "Monitor Lead: In");
    let mute = order.iter().position(|n| n == "Mute Lead");
    assert!(
        badge.is_some() && mute.is_some() && badge < mute,
        "badge before M: {order:#?}"
    );
    for _ in 0..300 {
        if focused(&lumen) == "Monitor Lead: In" {
            break;
        }
        lumen.key(Modifiers::NONE, Key::Tab);
    }
    assert_eq!(focused(&lumen), "Monitor Lead: In");
    lumen.key(Modifiers::NONE, Key::Space);
    assert_eq!(monitor_of(&lumen, Id(31)), Monitor::Off);
}
