//! Sends on track headers (M4 slice 4, the owner's K1 comet): a knob per
//! return on an unfolded track; dragged, silenced with a double-click,
//! switched pre-fader from its menu, and heard through a soloed return
//! (wet only).

use std::time::Duration;

use eframe::egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::model::TrackKind;

use super::{Lumen, wav};

/// A project with a tone on a track and a return; the track unfolded, so
/// its sends show. Returns the track's name.
fn with_a_return(name: &str) -> (Lumen, String) {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    lumen.click("Add track");
    lumen.click("Add return track");
    let track = lumen.track_names()[0].clone();
    lumen.click(&format!("Fold {track}"));
    let knob = format!("Send {track} to A Return 1");
    for _ in 0..100 {
        if lumen.harness.query_by_label_contains(&knob).is_some() {
            break;
        }
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    (lumen, track)
}

fn send(lumen: &Lumen) -> Option<(f32, bool)> {
    let project = lumen.app().project();
    let ret = project
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Return)?
        .id;
    let send = project.tracks[0].sends.iter().find(|s| s.to == ret)?;
    Some((send.level.0, send.pre_fader))
}

#[test]
fn a_send_knob_is_dragged_silenced_and_switched_pre_fader() {
    let (mut lumen, track) = with_a_return("send-knob");
    let label = format!("Send {track} to A Return 1");
    let knob = lumen.harness.get_by_label_contains(&label).rect();
    assert_eq!(send(&lumen), None, "no send until it is turned up");

    // Dragged up: one undo step for the whole drag.
    lumen.drag(
        knob.center(),
        knob.center() - vec2(0.0, 90.0),
        Modifiers::NONE,
    );
    let (level, pre) = send(&lumen).expect("a send was made");
    assert!(level > -30.0 && level <= 6.0, "{level}");
    assert!(!pre, "new sends are post-fader");
    assert_eq!(lumen.app().undo_label(), Some("Change send"));

    // Double-click: silence.
    lumen.double_click(knob.center());
    assert_eq!(send(&lumen).map(|s| s.0), Some(f32::NEG_INFINITY));

    // The menu switches it before the fader.
    lumen.right_click(knob.center());
    lumen.click("Pre-fader");
    assert_eq!(send(&lumen).map(|s| s.1), Some(true));
    assert_eq!(lumen.app().undo_label(), Some("Send before the fader"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(send(&lumen).map(|s| s.1), Some(false));
}

#[test]
fn a_typed_level_applies_on_enter() {
    let (mut lumen, track) = with_a_return("send-typed");
    let label = format!("Send {track} to A Return 1");
    lumen.harness.get_by_label_contains(&label).click();
    lumen.frames(3);
    lumen.harness.event(eframe::egui::Event::Text("-12".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(send(&lumen).map(|s| s.0), Some(-12.0));
}

#[test]
fn a_soloed_return_plays_only_what_is_sent_to_it() {
    let (mut lumen, track) = with_a_return("send-wet");
    let label = format!("Send {track} to A Return 1");
    lumen.harness.get_by_label_contains(&label).click();
    lumen.frames(3);
    lumen.harness.event(eframe::egui::Event::Text("0".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(send(&lumen).map(|s| s.0), Some(0.0));
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let loudest = |lumen: &mut Lumen| {
        std::thread::sleep(Duration::from_millis(150));
        lumen.frames(1);
        lumen.app_mut().heard();
        std::thread::sleep(Duration::from_millis(150));
        lumen.frames(1);
        let heard = lumen.app_mut().heard();
        assert!(heard.len() > 1_000, "heard {} frames", heard.len());
        heard.iter().fold(0.0f32, |m, s| m.max(s.0.abs()))
    };
    let both = loudest(&mut lumen);
    // Soloing the return: the send alone, about half of dry plus wet.
    lumen.click("Solo Return 1");
    let wet = loudest(&mut lumen);
    assert!(wet > 0.01, "the send is heard: {wet}");
    assert!(
        (wet - both / 2.0).abs() < both * 0.1,
        "wet {wet}, both {both}"
    );
    // The track's own output is silent now, and says so; muting it
    // silences its send too.
    lumen.click(&format!("Mute {track} (silenced)"));
    assert!(loudest(&mut lumen) < 1e-4);
    lumen.key(Modifiers::NONE, Key::Space);
}
