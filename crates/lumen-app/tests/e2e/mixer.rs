use eframe::egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_project::model::Output;

use super::Lumen;

fn mixed(name: &str) -> Lumen {
    mixed_with(name, 1)
}

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
    assert!(lumen.shows("Mute Master in the mixer"));
    assert!(lumen.shows("Reset integrated loudness"));

    lumen.click("Fold group Group 1");
    assert_eq!(strips(&lumen), ["Group 1", "Audio 3", "Return 1", "Master"]);
    lumen.click("Fold group Group 1 in the mixer");
    assert_eq!(strips(&lumen).len(), 6);
    assert!(lumen.shows("Audio 1: double-click to rename"));

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

    press(&mut lumen, "Audio 3 volume in the mixer:");
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("-12".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(track(&lumen, "Audio 3").gain_db, -12.0);

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

    lumen.click("Mute Audio 3 in the mixer");
    assert!(track(&lumen, "Audio 3").mute);
    assert!(lumen.is_on("Mute Audio 3"), "the header shows it too");
    lumen.click("Solo Audio 3 in the mixer");
    assert!(track(&lumen, "Audio 3").solo);

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
        said.contains("integrated not measured"),
        "nothing measured is said so: {said}"
    );
    assert!(
        said.contains("Play to measure"),
        "the empty history says why"
    );
}

fn grip(lumen: &Lumen, name: &str) -> eframe::egui::Pos2 {
    let rect = lumen.harness.get_by_label_contains(&strip(name)).rect();
    eframe::egui::pos2(rect.right() - 3.0, rect.center().y)
}

#[test]
fn dragging_a_strip_moves_its_track_and_returns_reorder_among_themselves() {
    let mut lumen = mixed_with("mixer-drag", 2);
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
    let master = lumen.harness.get_by_label_contains(&strip("Master")).rect();
    lumen.right_click(eframe::egui::pos2(master.left() - 60.0, master.center().y));
    lumen.click("\u{2212}6 dB, linear");
    assert_eq!(lumen.app().project().pan_law, PanLaw::Linear);
    assert_eq!(lumen.app().undo_label(), Some("Change pan law"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().pan_law, PanLaw::ConstantPower);

    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("Pan law: 0 dB".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.app().project().pan_law, PanLaw::Balance);
}

fn wheel(lumen: &mut Lumen, at: eframe::egui::Pos2, notches: i32, modifiers: Modifiers) {
    lumen.harness.hover_at(at);
    lumen.frames(2);
    for _ in 0..notches.abs() {
        lumen.harness.event(Event::MouseWheel {
            unit: eframe::egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 40.0 * notches.signum() as f32),
            phase: eframe::egui::TouchPhase::Move,
            modifiers,
        });
        lumen.frames(2);
    }
    lumen.frames(30);
}

#[test]
fn the_wheel_sets_a_control_its_gesture_starts_on() {
    let mut lumen = mixed("mixer-wheel");
    let fader = lumen
        .harness
        .get_by_label_contains("Audio 3 fader in the mixer:")
        .rect();
    wheel(&mut lumen, fader.center(), 3, Modifiers::NONE);
    let db = track(&lumen, "Audio 3").gain_db;
    assert!((db - 3.0).abs() < 0.01, "{db}");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(track(&lumen, "Audio 3").gain_db, 0.0);

    let pan = lumen
        .harness
        .get_by_label_contains("Audio 3 pan in the mixer:")
        .rect();
    wheel(&mut lumen, pan.center(), -2, Modifiers::NONE);
    assert!((track(&lumen, "Audio 3").pan + 0.1).abs() < 0.001);
    std::thread::sleep(std::time::Duration::from_millis(400));
    let send = lumen
        .harness
        .get_by_label_contains("Send Audio 3 to A Return 1 in the mixer")
        .rect();
    wheel(&mut lumen, send.center(), 2, Modifiers::NONE);
    let level = track(&lumen, "Audio 3").sends[0].level.0;
    assert!((level + 59.0).abs() < 0.01, "{level}");
}

#[test]
fn a_track_plays_straight_to_another_pair_of_outputs() {
    let mut lumen = Lumen::start("mixer-device-outputs");
    let file = super::wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    tall_mixer(&mut lumen);
    let name = lumen.track_names()[0].clone();
    lumen.click(&format!("{name} output in the mixer: Master"));
    lumen.run_until("the output pairs", |l| l.shows("3/4"));
    assert!(lumen.shows("1/2 Main"), "the main pair is marked");
    lumen.click("3/4");
    assert_eq!(lumen.app().project().tracks[0].output, Output::Device(1));
    lumen.wait_for("the audio to restart", lumen_app::App::audio_running);
    lumen.key(Modifiers::NONE, Key::Space);
    let heard = lumen.listen(std::time::Duration::from_millis(300), 1_000);
    let loudest = |c: usize| heard.iter().fold(0.0f32, |m, f| m.max(f[c].abs()));
    assert!(loudest(0) < 1e-4 && loudest(1) < 1e-4, "main is silent");
    assert!(
        loudest(2) > 0.01 && loudest(3) > 0.01,
        "outputs 3/4 play it"
    );
}

#[test]
fn a_strips_sends_and_buttons_are_wells_as_on_the_header() {
    let mut lumen = mixed_with("mixer-wells", 3);
    let rect = |lumen: &Lumen, name: &str| lumen.harness.get_by_label_contains(name).rect();
    let strip = rect(&lumen, "Mixer strip: Audio 3");
    let slots: Vec<_> = ["A Return 1", "B Return 2", "C Return 3"]
        .iter()
        .map(|to| rect(&lumen, &format!("Send Audio 3 to {to} in the mixer")))
        .collect();
    for pair in slots.windows(2) {
        assert!((pair[0].center().y - pair[1].center().y).abs() < 0.5);
        assert!(pair[1].left() > pair[0].right(), "{pair:?}");
    }
    assert!(strip.contains_rect(slots[0]) && strip.contains_rect(slots[2]));
    let pan = rect(&lumen, "Audio 3 pan in the mixer:");
    assert!(pan.top() > slots[0].bottom(), "the sends come before pan");
    let buttons: Vec<_> = ["Mute Audio 3 in", "Solo Audio 3 in", "Arm Audio 3 in"]
        .iter()
        .map(|name| rect(&lumen, name))
        .collect();
    for pair in buttons.windows(2) {
        assert!((pair[0].center().y - pair[1].center().y).abs() < 0.5);
        assert!(pair[1].left() > pair[0].right(), "{pair:?}");
    }
    assert!(buttons[0].top() > pan.bottom(), "M, S and arm under pan");
    let ret = rect(&lumen, "Mute Return 1 in the mixer");
    assert!((ret.center().y - buttons[0].center().y).abs() < 0.5);
    assert!(!lumen.shows("Arm Return 1 in the mixer"));
    let master = rect(&lumen, "Mute Master in the mixer");
    assert!((master.center().y - buttons[0].center().y).abs() < 0.5);
    let to = track(&lumen, "Return 2").id;
    lumen
        .harness
        .get_by_label_contains("Send Audio 3 to B Return 2 in the mixer")
        .click();
    lumen.frames(2);
    lumen.harness.event(Event::Text("-9".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.run_until("the send is set", |lumen| {
        track(lumen, "Audio 3")
            .sends
            .iter()
            .any(|s| s.to == to && (s.level.0 + 9.0).abs() < 0.01)
    });
}
