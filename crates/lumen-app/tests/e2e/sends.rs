//! Sends on track headers (M4 slice 4, the owner's K1 comet): a knob per
//! return on an unfolded track; dragged, silenced with a double-click,
//! switched pre-fader from its menu, and heard through a soloed return
//! (wet only).

use std::time::Duration;

use eframe::egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
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

#[test]
fn a_send_at_the_bottom_is_silent_and_leaves_the_level_alone() {
    // MT-MIX-4 run 1 (owner): the track seemed louder with its send at the
    // lowest. The bottom of the knob stored −80 dB, not silence.
    let (mut lumen, track) = with_a_return("send-silent");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let loudest = |lumen: &mut Lumen| {
        std::thread::sleep(Duration::from_millis(150));
        lumen.frames(1);
        lumen.app_mut().heard();
        std::thread::sleep(Duration::from_millis(150));
        lumen.frames(1);
        let heard = lumen.app_mut().heard();
        heard.iter().fold(0.0f32, |m, s| m.max(s.0.abs()))
    };
    let before = loudest(&mut lumen);
    let label = format!("Send {track} to A Return 1");
    let knob = lumen.harness.get_by_label_contains(&label).rect();
    lumen.drag(
        knob.center(),
        knob.center() - vec2(0.0, 60.0),
        Modifiers::NONE,
    );
    assert!(
        send(&lumen).is_some_and(|s| s.0 > -60.0),
        "{:?}",
        send(&lumen)
    );
    assert!(
        loudest(&mut lumen) > before * 1.01,
        "an empty return adds a copy"
    );
    lumen.drag(
        knob.center(),
        knob.center() + vec2(0.0, 400.0),
        Modifiers::NONE,
    );
    assert_eq!(
        send(&lumen).map(|s| s.0),
        Some(f32::NEG_INFINITY),
        "silence"
    );
    let after = loudest(&mut lumen);
    assert!(
        (after - before).abs() < before * 0.01,
        "before {before}, after {after}"
    );
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn sends_past_the_headers_width_are_set_from_the_plus_list() {
    // MT-MIX-4 run 1 (owner): the "+N" could not be reached (the owner's O1).
    let (mut lumen, track) = with_a_return("send-more");
    for _ in 0..9 {
        lumen.click("Add track");
        lumen.click("Add return track");
    }
    lumen.frames(20);
    let more = format!("more sends from {track}");
    assert!(lumen.harness.query_by_label_contains(&more).is_some());
    // The tenth return's knob is not on the header.
    assert!(
        lumen
            .harness
            .query_by_label_contains("to J Return 10")
            .is_none()
    );
    lumen.harness.get_by_label_contains(&more).click();
    lumen.frames(3);
    lumen
        .harness
        .get_by_label_contains("to J Return 10")
        .click();
    lumen.frames(2);
    lumen.harness.event(eframe::egui::Event::Text("-9".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    let project = lumen.app().project();
    let ret = project
        .tracks
        .iter()
        .find(|t| t.name == "Return 10")
        .unwrap()
        .id;
    let level = project.tracks[0]
        .sends
        .iter()
        .find(|s| s.to == ret)
        .map(|s| s.level.0);
    assert_eq!(level, Some(-9.0));
}

#[test]
fn an_unfolded_return_lists_what_sends_to_it_and_sets_it_there() {
    // MT-MIX-4 run 1 (owner): a return should say what sends to it (R2).
    let (mut lumen, track) = with_a_return("send-senders");
    lumen.click("Fold Return 1");
    lumen.frames(30);
    let into = format!("{track} into A Return 1");
    assert!(
        lumen.harness.query_by_label_contains(&into).is_none(),
        "nothing sends yet"
    );
    let label = format!("Send {track} to A Return 1");
    lumen.harness.get_by_label_contains(&label).click();
    lumen.frames(2);
    lumen.harness.event(eframe::egui::Event::Text("-6".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(30);
    // Listed on the return, and set from there too.
    lumen.harness.get_by_label_contains(&into).click();
    lumen.frames(2);
    lumen.harness.event(eframe::egui::Event::Text("-15".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(send(&lumen).map(|s| s.0), Some(-15.0));
}

/// Sets the send `label` names to `level` by clicking it and typing.
fn type_into(lumen: &mut Lumen, label: &str, level: &str) {
    lumen.harness.get_by_label_contains(label).click();
    lumen.frames(2);
    lumen.harness.event(eframe::egui::Event::Text(level.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(2);
}

#[test]
fn a_return_lists_its_four_loudest_senders_then_all_of_them_from_more() {
    // MT-MIX-4 run 2 (owner): many senders did not fit (the owner's L4).
    let mut lumen = Lumen::start("send-many");
    for _ in 0..6 {
        lumen.key(Modifiers::COMMAND, Key::T);
    }
    lumen.click("Add track");
    lumen.click("Add return track");
    let names = lumen.track_names();
    let tracks: Vec<String> = names.iter().take(6).cloned().collect();
    for (n, track) in tracks.iter().enumerate() {
        lumen.click(&format!("Fold {track}"));
        lumen.frames(30);
        type_into(
            &mut lumen,
            &format!("Send {track} to A Return 1"),
            &format!("-{}", n * 3),
        );
        lumen.click(&format!("Fold {track}"));
        lumen.frames(30);
    }
    lumen.click("Fold Return 1");
    lumen.frames(40);
    let listed = |lumen: &Lumen| {
        lumen
            .harness
            .query_all_by_label_contains("into A Return 1:")
            .count()
    };
    assert_eq!(listed(&lumen), 4, "the four loudest");
    // The loudest are the first four (0, −3, −6, −9 dB).
    for track in &tracks[..4] {
        let row = format!("{track} into A Return 1:");
        assert!(
            lumen.harness.query_by_label_contains(&row).is_some(),
            "{track}"
        );
    }
    let quiet = format!("{} into A Return 1:", tracks[5]);
    assert!(lumen.harness.query_by_label_contains(&quiet).is_none());
    lumen
        .harness
        .get_by_label_contains("2 more tracks sending into A Return 1")
        .click();
    lumen.frames(3);
    assert_eq!(listed(&lumen), 4 + 6, "the list opens with every sender");
    lumen.key(Modifiers::NONE, Key::Escape);
}

#[test]
fn a_send_turned_off_leaves_the_returns_list_when_let_go() {
    // MT-MIX-4 run 2 (owner): senders turned off stayed listed (S1).
    let (mut lumen, track) = with_a_return("send-off");
    lumen.click("Fold Return 1");
    lumen.frames(30);
    type_into(&mut lumen, &format!("Send {track} to A Return 1"), "-6");
    lumen.frames(30);
    let into = format!("{track} into A Return 1");
    assert!(lumen.harness.query_by_label_contains(&into).is_some());
    // Silenced on the track (double-click): gone from the return's list,
    // pre-fader kept on the track.
    let knob = lumen
        .harness
        .get_by_label_contains(&format!("Send {track} to A Return 1"))
        .rect();
    lumen.right_click(knob.center());
    lumen.click("Pre-fader");
    lumen.double_click(knob.center());
    lumen.frames(30);
    assert!(lumen.harness.query_by_label_contains(&into).is_none());
    assert_eq!(send(&lumen), Some((f32::NEG_INFINITY, true)));
    // Turned down from the return's own list, it stays while held.
    type_into(&mut lumen, &format!("Send {track} to A Return 1"), "-6");
    lumen.frames(30);
    let row = lumen.harness.get_by_label_contains(&into).rect();
    lumen.harness.hover_at(row.center());
    lumen.harness.drag_at(row.center());
    lumen.frames(2);
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(row.center() + vec2(0.0, 50.0 * step as f32));
        lumen.frames(1);
    }
    assert_eq!(send(&lumen).map(|s| s.0), Some(f32::NEG_INFINITY));
    assert!(
        lumen.harness.query_by_label_contains(&into).is_some(),
        "held: still listed"
    );
    lumen.harness.drop_at(row.center() + vec2(0.0, 400.0));
    lumen.frames(30);
    assert!(
        lumen.harness.query_by_label_contains(&into).is_none(),
        "let go: gone"
    );
}

#[test]
fn a_duplicated_tracks_send_is_its_own() {
    // MT-MIX-4 (owner): changing a send on a duplicated track changed the
    // original's too.
    let (mut lumen, track) = with_a_return("send-duplicate");
    type_into(&mut lumen, &format!("Send {track} to A Return 1"), "-6");
    lumen
        .harness
        .get_by_label_contains(&format!("Track {track},"))
        .click();
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    while lumen.app().selected_tracks().first() != Some(&lumen.app().project().tracks[0].id) {
        lumen.key(Modifiers::NONE, Key::ArrowUp);
    }
    lumen.key(Modifiers::COMMAND, Key::D);
    lumen.frames(30);
    let tracks = &lumen.app().project().tracks;
    let (original, copy) = (tracks[0].clone(), tracks[1].clone());
    assert_ne!(original.id, copy.id);
    eprintln!(
        "original {} {:?}\ncopy {} {:?}",
        original.name, original.sends, copy.name, copy.sends
    );
    // The copy is unfolded too: set its send.
    let copy_label = format!("Send {} to A Return 1", copy.name);

    // Unfold the copy (the lower of the two "Fold tone" arrows).
    let folds: Vec<_> = lumen
        .harness
        .query_all_by_label(&format!("Fold {}", copy.name))
        .map(|n| n.rect())
        .collect();
    let lower = *folds
        .iter()
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    lumen.harness.hover_at(lower.center());
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(eframe::egui::Event::PointerButton {
            pos: lower.center(),
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    lumen.frames(40);
    let knobs: Vec<_> = lumen
        .harness
        .query_all_by_label_contains(&copy_label)
        .map(|n| n.rect())
        .collect();
    eprintln!("knobs {knobs:?}");
    let second = *knobs
        .iter()
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    lumen.harness.hover_at(second.center());
    lumen.frames(1);
    lumen.harness.event(eframe::egui::Event::PointerButton {
        pos: second.center(),
        button: eframe::egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    lumen.harness.event(eframe::egui::Event::PointerButton {
        pos: second.center(),
        button: eframe::egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    lumen.frames(2);
    lumen.harness.event(eframe::egui::Event::Text("-20".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(2);
    let tracks = &lumen.app().project().tracks;
    let level = |t: &lumen_project::model::Track| t.sends.first().map(|s| s.level.0);
    eprintln!(
        "after: original {:?} copy {:?}",
        level(&tracks[0]),
        level(&tracks[1])
    );
    assert_eq!(level(&tracks[0]), Some(-6.0), "the original keeps its send");
    assert_eq!(level(&tracks[1]), Some(-20.0));
}

#[test]
fn a_duplicated_tracks_send_is_heard_as_its_own() {
    // MT-MIX-4 (owner): a send change on one of two duplicated tracks was
    // heard on the other.
    let (mut lumen, track) = with_a_return("send-duplicate-heard");
    type_into(&mut lumen, &format!("Send {track} to A Return 1"), "0");
    lumen.key(Modifiers::NONE, Key::Escape);
    while lumen.app().selected_tracks().first() != Some(&lumen.app().project().tracks[0].id) {
        lumen.key(Modifiers::NONE, Key::ArrowUp);
    }
    lumen.key(Modifiers::COMMAND, Key::D);
    lumen.frames(30);
    lumen.click("Solo Return 1");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let loudest = |lumen: &mut Lumen| {
        std::thread::sleep(Duration::from_millis(200));
        lumen.frames(1);
        lumen.app_mut().heard();
        std::thread::sleep(Duration::from_millis(200));
        lumen.frames(1);
        let heard = lumen.app_mut().heard();
        heard.iter().fold(0.0f32, |m, s| m.max(s.0.abs()))
    };
    let both = loudest(&mut lumen);
    // Silence the copy's send only (the lower knob, by double-click).
    let copy = lumen.app().project().tracks[1].id;
    let copy_name = lumen.app().project().tracks[1].name.clone();
    let folds: Vec<_> = lumen
        .harness
        .query_all_by_label(&format!("Fold {copy_name}"))
        .map(|n| n.rect())
        .collect();
    let lower = *folds
        .iter()
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    lumen.harness.hover_at(lower.center());
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(eframe::egui::Event::PointerButton {
            pos: lower.center(),
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    lumen.frames(40);
    let knobs: Vec<_> = lumen
        .harness
        .query_all_by_label_contains(&format!("Send {copy_name} to A Return 1"))
        .map(|n| n.rect())
        .collect();
    let knob = *knobs
        .iter()
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    lumen.double_click(knob.center());
    let tracks = &lumen.app().project().tracks;
    let level = |i: usize| tracks[i].sends.first().map(|s| s.level.0);
    assert_eq!((level(0), level(1)), (Some(0.0), Some(f32::NEG_INFINITY)));
    assert_eq!(tracks[1].id, copy);
    let one = loudest(&mut lumen);
    eprintln!("both {both}, one {one}");
    assert!(
        (one - both / 2.0).abs() < both * 0.05,
        "both {both}, one {one}"
    );
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn turning_a_sender_down_in_a_returns_list_leaves_the_rows_in_place() {
    // MT-MIX-4 run 4 (owner): with a duplicated track, setting one sender
    // seemed to change the other: the list re-sorted by level and the two
    // rows, of the same name, swapped places.
    let (mut lumen, track) = with_a_return("send-rows-stay");
    type_into(&mut lumen, &format!("Send {track} to A Return 1"), "-13");
    lumen.key(Modifiers::NONE, Key::Escape);
    while lumen.app().selected_tracks().first() != Some(&lumen.app().project().tracks[0].id) {
        lumen.key(Modifiers::NONE, Key::ArrowUp);
    }
    lumen.key(Modifiers::COMMAND, Key::D);
    lumen.click("Fold Return 1");
    lumen.frames(40);
    let rows = |lumen: &Lumen| -> Vec<(f32, String)> {
        let mut rows: Vec<(f32, String)> = lumen
            .harness
            .query_all_by_label_contains("into A Return 1:")
            .map(|n| {
                (
                    n.rect().top(),
                    n.accesskit_node().label().unwrap_or_default(),
                )
            })
            .collect();
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        rows
    };
    let before = rows(&lumen);
    assert_eq!(before.len(), 2);
    // Type a lower level into the top row.
    let top = before[0].0;
    let knob = lumen
        .harness
        .query_all_by_label_contains("into A Return 1:")
        .find(|n| n.rect().top() == top)
        .unwrap()
        .rect();
    lumen.harness.hover_at(knob.center());
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(eframe::egui::Event::PointerButton {
            pos: knob.center(),
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    lumen.frames(2);
    lumen.harness.event(eframe::egui::Event::Text("-30".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    let after = rows(&lumen);
    assert!(
        after[0].1.contains("30"),
        "the top row is the one changed: {after:?}"
    );
    assert!(
        after[1].1.contains("13"),
        "the other is untouched: {after:?}"
    );
}

#[test]
fn a_slow_drag_turns_a_silent_send_up() {
    // MT-MIX-4 run 5 (owner): hard to turn a send up from −∞. Each frame
    // the knob started again from its level, and moves of a few points a
    // frame were rounded back to silence: 24 points in steps of 3 stayed
    // at −∞ before the fix.
    let (mut lumen, track) = with_a_return("send-engage");
    let label = format!("Send {track} to A Return 1");
    let knob = lumen.harness.get_by_label_contains(&label).rect();
    lumen.drag(
        knob.center(),
        knob.center() - vec2(0.0, 24.0),
        Modifiers::NONE,
    );
    let level = send(&lumen).map(|s| s.0).expect("a send");
    assert!(level > -40.0 && level < -25.0, "{level}");
    // The same from a send silenced by a double-click.
    lumen.double_click(knob.center());
    assert_eq!(send(&lumen).map(|s| s.0), Some(f32::NEG_INFINITY));
    lumen.drag(
        knob.center(),
        knob.center() - vec2(0.0, 24.0),
        Modifiers::NONE,
    );
    assert!(
        send(&lumen).is_some_and(|s| s.0 > -40.0),
        "{:?}",
        send(&lumen)
    );
}
