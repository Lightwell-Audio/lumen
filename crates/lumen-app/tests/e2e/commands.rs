use eframe::egui::{Event, Key, Modifiers};

use super::Lumen;
use super::properties::{clip, select, with_two_clips};

#[test]
fn shortcuts_match_their_modifiers_exactly() {
    let (mut lumen, kick, _) = with_two_clips("exact-chords");
    select(&mut lumen, kick);
    lumen.key(Modifiers::ALT, Key::Num0);
    assert!(!clip(&lumen, kick).muted, "Alt+0 is not 0");
    lumen.key(Modifiers::SHIFT, Key::Delete);
    assert!(
        lumen.app().selected_clips() == [kick] && lumen.app().undo_label() != Some("Delete"),
        "Shift+Delete is not Delete"
    );
    lumen.key(Modifiers::NONE, Key::Num0);
    assert!(clip(&lumen, kick).muted, "0 mutes");
}

#[test]
fn ctrl_r_renames_the_selected_track_as_f2_does() {
    let mut lumen = Lumen::start("rename-command");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND, Key::R);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("Bass".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.track_names(), ["Bass"]);
    assert_eq!(lumen.app().undo_label(), Some("Rename track"));
}
