//! The transport: play and stop, the playhead, metronome, loop and Follow.

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;

use super::Lumen;

#[test]
fn space_plays_and_stops_and_the_playhead_moves() {
    let mut lumen = Lumen::start("play");
    assert!(!lumen.app().is_playing());

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the playhead to move", |app| {
        app.is_playing() && app.playhead() > Ticks::quarters(1)
    });
    assert!(lumen.shows("Stop here"), "Play turns into Stop here");

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback to stop", |app| !app.is_playing());
    assert!(lumen.shows("Play from the start marker"));
}

#[test]
fn the_buttons_play_stop_and_return_to_the_start() {
    let mut lumen = Lumen::start("buttons");
    lumen.click("Play from the start marker");
    lumen.wait_for("the playhead to move", |app| {
        app.playhead() > Ticks::quarters(1)
    });
    lumen.click("Stop here");
    lumen.wait_for("playback to stop", |app| !app.is_playing());
    assert!(lumen.app().playhead() > Ticks::ZERO);
    lumen.click("Go to start");
    lumen.wait_for("the playhead to return", |app| {
        app.playhead() == Ticks::ZERO
    });
}

#[test]
fn metronome_loop_and_follow_toggle() {
    let mut lumen = Lumen::start("toggles-transport");
    for label in ["Metronome", "Loop"] {
        let was = lumen.is_on(label);
        lumen.click(label);
        assert_eq!(lumen.is_on(label), !was, "{label}");
    }
    // Follow is on by default (owner decision); Ctrl+Shift+F turns it off.
    let follow = "Follow playhead";
    assert!(lumen.is_on(follow));
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    assert!(!lumen.is_on(follow));
}
