//! Tap tempo (M9 R7): the Tap piece after the tempo counts a run's taps and
//! sets the tempo from the third, as one undo step; a key held down taps
//! once. The times are given (`App::next_tap_at`), so the tempo is exact.

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};

use super::Lumen;

/// A tap on the Tap piece at `time` seconds.
fn tap(lumen: &mut Lumen, time: f64) {
    lumen.app_mut().next_tap_at(time);
    lumen.click("Tap tempo");
    lumen.frames(2);
}

fn bpm(lumen: &Lumen) -> f64 {
    lumen.app().project().tempo[0].bpm
}

/// The count the Tap piece shows, as its accessible value.
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
    // 0.6 s apart is 100 BPM.
    for time in [11.2, 11.8, 12.4] {
        tap(&mut lumen, time);
        assert_eq!(bpm(&lumen), 100.0);
    }
    assert_eq!(count(&lumen), Some(5.0));
    assert_eq!(lumen.app().undo_label(), Some("Tap tempo"));

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(bpm(&lumen), 120.0, "the run was one step");
    // Undo was another edit: the run ended, and the piece says Tap again.
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
    // The repeats of the held key are not taps.
    for _ in 0..5 {
        lumen.harness.event(press(true));
        lumen.frames(1);
    }
    assert_eq!(count(&lumen), Some(1.0));
}
