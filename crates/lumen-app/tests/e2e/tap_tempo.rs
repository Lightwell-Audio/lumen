use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};

use super::Lumen;

fn tap(lumen: &mut Lumen, time: f64) {
    lumen.app_mut().next_tap_at(time);
    lumen.click("Tap tempo");
    lumen.frames(2);
}

fn bpm(lumen: &Lumen) -> f64 {
    lumen.app().project().tempo[0].bpm
}

fn count(lumen: &Lumen) -> Option<f64> {
    lumen
        .harness
        .get_by_label("Tap tempo")
        .accesskit_node()
        .numeric_value()
}

#[test]
fn taps_set_the_tempo_from_the_third_and_one_undo_takes_the_run_back() {
    let mut lumen = Lumen::start("tap-tempo");
    assert_eq!(bpm(&lumen), 120.0);
    assert_eq!(count(&lumen), None, "no run: the piece says Tap");
    tap(&mut lumen, 10.0);
    assert_eq!(count(&lumen), Some(1.0));
    tap(&mut lumen, 10.6);
    assert_eq!(bpm(&lumen), 120.0, "two taps change nothing");
    for time in [11.2, 11.8, 12.4] {
        tap(&mut lumen, time);
        assert_eq!(bpm(&lumen), 100.0);
    }
    assert_eq!(count(&lumen), Some(5.0));
    assert_eq!(lumen.app().undo_label(), Some("Tap tempo"));

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(bpm(&lumen), 120.0, "the run was one step");
    assert_eq!(count(&lumen), None);
}

#[test]
fn a_key_held_down_taps_once() {
    let dir = lumen_project::testing::TempDir::new("e2e-tap-key");
    let file = dir.path().join("settings.json");
    std::fs::write(
        &file,
        r#"{"version": 1, "commands": {"shortcuts": {"transport.tap_tempo": ["Shift+T"]}}}"#,
    )
    .unwrap();
    let mut lumen = Lumen::start_with(dir, |l| l.with_settings_file(&file));
    assert_eq!(lumen.app().shortcuts_of("Tap tempo"), ["Shift+T"]);
    let press = |repeat: bool| Event::Key {
        key: Key::T,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Modifiers::SHIFT,
    };
    lumen.app_mut().next_tap_at(1.0);
    lumen.harness.event(press(false));
    lumen.frames(2);
    assert_eq!(count(&lumen), Some(1.0));
    for _ in 0..5 {
        lumen.harness.event(press(true));
        lumen.frames(1);
    }
    assert_eq!(count(&lumen), Some(1.0));
}

#[test]
fn a_new_project_ends_the_run() {
    let mut lumen = Lumen::start("tap-tempo-new");
    tap(&mut lumen, 10.0);
    tap(&mut lumen, 10.6);
    assert_eq!(count(&lumen), Some(2.0));
    super::copies::command(&mut lumen, "New project");
    lumen.frames(5);
    assert_eq!(count(&lumen), None, "the piece says Tap again");
    tap(&mut lumen, 11.2);
    assert_eq!(count(&lumen), Some(1.0), "a run of its own");
    assert_eq!(bpm(&lumen), 120.0, "one tap sets nothing");
}
