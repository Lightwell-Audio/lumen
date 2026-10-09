//! Mute and solo (M4 slice 4): the rules the owner agreed, driven from the
//! headers, the transport's S chip and the palette, and heard.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use lumen_core::time::Ticks;

use super::{Lumen, wav};

/// A playing project: a track holding a tone, then two empty audio tracks.
/// Returns the three names in order.
fn playing(name: &str) -> (Lumen, [String; 3]) {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND, Key::T);
    let names = lumen.track_names();
    assert_eq!(names.len(), 3);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    (
        lumen,
        [names[0].clone(), names[1].clone(), names[2].clone()],
    )
}

/// The loudest left sample over a stretch, after letting changes glide.
fn loudest(lumen: &mut Lumen) -> f32 {
    std::thread::sleep(Duration::from_millis(150));
    lumen.frames(1);
    lumen.app_mut().heard();
    std::thread::sleep(Duration::from_millis(150));
    lumen.frames(1);
    let heard = lumen.app_mut().heard();
    assert!(heard.len() > 1_000, "heard {} frames", heard.len());
    heard.iter().fold(0.0f32, |m, s| m.max(s.0.abs()))
}

/// Clicks `label` holding Ctrl.
fn ctrl_click(lumen: &mut Lumen, label: &str) {
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::COMMAND));
    lumen.frames(1);
    lumen.click(label);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(1);
}

fn soloed(lumen: &Lumen) -> Vec<bool> {
    let tracks = &lumen.app().project().tracks;
    tracks.iter().map(|t| t.solo).collect()
}

/// Opens the palette, types `query` and runs the first row.
fn palette(lumen: &mut Lumen, query: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text(query.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

#[test]
fn another_tracks_solo_silences_a_track_shows_why_and_the_chip_clears_it() {
    let (mut lumen, [tone, other, third]) = playing("solo-chip");
    assert!(loudest(&mut lumen) > 0.01);

    lumen.click(&format!("Solo {other}"));
    assert!(loudest(&mut lumen) < 1e-4, "silenced by the other's solo");
    // The silenced tracks say so (a hollow M), the soloed one does not.
    assert!(lumen.shows(&format!("Mute {tone} (silenced)")));
    assert!(lumen.shows(&format!("Mute {third} (silenced)")));
    assert!(lumen.shows(&format!("Mute {other}")));
    assert!(!lumen.app().project().tracks[0].mute, "nothing was muted");

    // A plain click on another Solo moves the solo; Ctrl+click adds.
    lumen.click(&format!("Solo {third}"));
    assert_eq!(soloed(&lumen), [false, false, true]);
    ctrl_click(&mut lumen, &format!("Solo {tone}"));
    assert_eq!(soloed(&lumen), [true, false, true]);
    assert!(loudest(&mut lumen) > 0.01, "soloed with the other");

    // The transport's chip clears every solo, as one undo step.
    lumen.click("Clear all solos");
    assert_eq!(soloed(&lumen), [false, false, false]);
    assert_eq!(lumen.app().undo_label(), Some("Clear solos"));
    assert!(loudest(&mut lumen) > 0.01);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(soloed(&lumen), [true, false, true]);
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn mute_beats_solo_and_a_soloed_group_shows_its_tracks_soloed_through_it() {
    let (mut lumen, [tone, _, _]) = playing("solo-group");
    // The tone's track into a group, then solo the group.
    lumen.key(Modifiers::NONE, Key::Space);
    // The new tracks were added last: the tone's track is two up.
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.key(Modifiers::COMMAND, Key::G);
    let group = lumen.track_names()[0].clone();
    lumen.click(&format!("Solo {group}"));
    assert!(lumen.shows(&format!("Solo {tone} (through its group)")));
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    assert!(loudest(&mut lumen) > 0.01, "heard through the soloed group");
    // Muting the track inside wins over the group's solo.
    lumen.click(&format!("Mute {tone}"));
    assert!(loudest(&mut lumen) < 1e-4, "mute beats solo");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn the_palette_solos_and_mutes_the_selection_and_additive_solo_swaps_the_click() {
    let (mut lumen, [tone, _, third]) = playing("solo-palette");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    palette(&mut lumen, "Solo selected tracks");
    assert_eq!(soloed(&lumen), [false, true, false]);
    palette(&mut lumen, "Solo selected tracks");
    assert_eq!(soloed(&lumen), [false, false, false], "again unsolos");
    palette(&mut lumen, "Mute selected tracks");
    assert!(lumen.app().project().tracks[1].mute);

    // Additive solo: a plain click adds, Ctrl+click solos alone.
    palette(&mut lumen, "Additive solo");
    lumen.click(&format!("Solo {tone}"));
    lumen.click(&format!("Solo {third}"));
    assert_eq!(soloed(&lumen), [true, false, true]);
    ctrl_click(&mut lumen, &format!("Solo {third}"));
    // Ctrl+click on a lit Solo unsolos it alone and clears the others.
    assert_eq!(soloed(&lumen), [false, false, false]);
    ctrl_click(&mut lumen, &format!("Solo {tone}"));
    assert_eq!(soloed(&lumen), [true, false, false]);
    palette(&mut lumen, "Clear all solos");
    assert_eq!(soloed(&lumen), [false, false, false]);
}
