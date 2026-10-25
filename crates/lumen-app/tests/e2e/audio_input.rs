//! Audio input (M9 slice R1, ADR-0100): arming an unrouted audio track picks
//! the first free input, the In chip chooses the input from a menu of
//! the device's pairs and mono inputs, the input meters move and their clip
//! light stays until clicked, and an armed track is heard stopped (Auto
//! monitoring) and a disarmed one is not. They share the process-wide input
//! test device, so they take turns.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use eframe::egui::{Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_project::model::{AudioInput, Color, Id, Monitor, Project, Track, TrackKind};
use lumen_project::testing::{TempDir, project_of};

use super::Lumen;
use super::midi_play::palette;

/// The scenarios take turns on the one input device.
static DEVICE: Mutex<()> = Mutex::new(());

const VOX: Id = Id(21);
const GTR: Id = Id(22);

fn project() -> Project {
    project_of(
        "Input",
        vec![
            Track::new(VOX, "Vox", TrackKind::Audio, Color(0x4F81BD)),
            Track::new(GTR, "Gtr", TrackKind::Audio, Color(0xC0504D)),
        ],
    )
}

/// Starts on the input device with both tracks, and nothing on its inputs.
fn launch(name: &str) -> (Lumen, MutexGuard<'static, ()>) {
    let turn = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    lumen_app::testing::input_device().set(|d| d.input_levels = Vec::new());
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(), &path, &Default::default()).expect("save");
    let mut lumen = Lumen::start_with(dir, move |l| l.with_input_device().opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 2);
    lumen.frames(30);
    (lumen, turn)
}

fn input_of(lumen: &Lumen, id: Id) -> Option<AudioInput> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == id)
        .expect("the track")
        .input
}

/// Unfolds `track`'s header and waits for its second row.
fn unfold(lumen: &mut Lumen, track: &str) {
    lumen.click(&format!("Fold {track}"));
    for _ in 0..100 {
        if lumen.shows(&format!("{track} input: None"))
            || lumen.shows(&format!("Volume {track}: 0.0 dB"))
        {
            break;
        }
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    lumen.frames(5);
}

fn play_input(levels: &[f32]) {
    lumen_app::testing::input_device().set(|d| d.input_levels = levels.to_vec());
}

/// Lets the device run for `ms`, drawing frames as the app would.
fn listen(lumen: &mut Lumen, ms: u64) {
    for _ in 0..ms / 50 {
        std::thread::sleep(Duration::from_millis(50));
        lumen.frames(1);
    }
}

/// Clicks the top edge of `name`'s header, clear of its buttons and the In chip.
fn select(lumen: &mut Lumen, name: &str) {
    let header = lumen
        .harness
        .get_by_label_contains(&format!("Track {name},"))
        .rect();
    let at = Pos2::new(header.center().x, header.top() + 4.0);
    lumen.harness.hover_at(at);
    lumen.harness.drag_at(at);
    lumen.frames(1);
    lumen.harness.drop_at(at);
    lumen.frames(3);
}

#[test]
fn arming_picks_the_first_free_input_and_one_undo_takes_both_back() {
    let (mut lumen, _turn) = launch("input-arm");
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    assert_eq!(input_of(&lumen, VOX), None);
    lumen.click("Arm Vox");
    assert_eq!(input_of(&lumen, VOX), Some(AudioInput::mono(0)));
    // The second track's arm (Ctrl keeps the first armed) takes the next one.
    lumen.click("Arm Gtr");
    assert_eq!(input_of(&lumen, GTR), Some(AudioInput::mono(1)));
    assert_eq!(lumen.app().undo_label(), Some("Arm for recording"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(input_of(&lumen, GTR), None);
    assert_eq!(input_of(&lumen, VOX), Some(AudioInput::mono(0)));
}

#[test]
fn the_in_chip_chooses_a_stereo_pair_or_a_mono_input_and_none() {
    let (mut lumen, _turn) = launch("input-chip");
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    // Unarmed, the chip is on the unfolded second row.
    assert!(!lumen.shows("Vox input: None"));
    unfold(&mut lumen, "Vox");
    assert!(lumen.shows("Vox input: None"));
    lumen.click("Vox input: None");
    assert!(lumen.shows("1+2  In 1 + In 2"));
    lumen.click("1+2  In 1 + In 2");
    assert_eq!(input_of(&lumen, VOX), Some(AudioInput::pair(0)));
    assert_eq!(lumen.app().undo_label(), Some("Input: 1+2"));
    assert!(lumen.shows("Vox input: In 1 + In 2"));

    lumen.click("Vox input: In 1 + In 2");
    lumen.click("2  In 2");
    assert_eq!(input_of(&lumen, VOX), Some(AudioInput::mono(1)));
    lumen.click("Vox input: In 2");
    lumen.click("None");
    assert_eq!(input_of(&lumen, VOX), None);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(input_of(&lumen, VOX), Some(AudioInput::mono(1)));
}

#[test]
fn the_palette_steps_a_selected_audio_track_through_the_inputs_and_monitor_modes() {
    let (mut lumen, _turn) = launch("input-palette");
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    select(&mut lumen, "Vox");
    palette(&mut lumen, "Next track input");
    assert_eq!(input_of(&lumen, VOX), Some(AudioInput::pair(0)));
    palette(&mut lumen, "Next track input");
    assert_eq!(input_of(&lumen, VOX), Some(AudioInput::mono(0)));
    palette(&mut lumen, "Next monitor mode");
    let monitor = lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == VOX)
        .map(|t| t.monitor);
    assert_eq!(monitor, Some(Monitor::In));
    assert!(lumen.shows("Monitor Vox: In"));
}

#[test]
fn an_armed_track_is_heard_and_metered_and_a_disarmed_one_is_only_metered() {
    let (mut lumen, _turn) = launch("input-heard");
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    play_input(&[0.5, 0.0]);
    // Metered whatever the tracks do: nothing is armed or routed yet.
    lumen.wait_for("the input meter", |app| {
        app.input_meter(0).peak_db > -7.0 && app.input_meter(0).peak_db < -5.0
    });
    assert!(lumen.app().meter(VOX).peak_db.is_infinite());

    // Armed, the track takes input 1 and plays it through its chain.
    lumen.click("Arm Vox");
    lumen.wait_for("the armed track to play its input", |app| {
        let heard = app.meter(VOX).peak_db;
        heard > -7.0 && heard < -5.0
    });
    // Monitor Off silences it, and the meter rests.
    select(&mut lumen, "Vox");
    palette(&mut lumen, "Next monitor mode");
    palette(&mut lumen, "Next monitor mode");
    lumen.wait_for("the monitor to go quiet", |app| {
        app.meter(VOX).peak_db.is_infinite()
    });
    // The input is still metered.
    assert!(lumen.app().input_meter(0).peak_db > -7.0);
}

#[test]
fn an_input_clip_light_stays_until_it_is_clicked() {
    let (mut lumen, _turn) = launch("input-clip");
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    lumen.click("Arm Vox");
    play_input(&[1.0, 0.0]);
    lumen.wait_for("the clip light", |app| app.input_meter(0).clipped);
    play_input(&[0.0, 0.0]);
    listen(&mut lumen, 400);
    assert!(lumen.app().input_meter(0).clipped, "the light stays");
    lumen.click("Clear the input clip light of Vox");
    assert!(!lumen.app().input_meter(0).clipped);
    assert!(!lumen.shows("Clear the input clip light of Vox"));
}

#[test]
fn a_track_whose_input_the_device_lacks_says_so() {
    let (mut lumen, _turn) = launch("input-lacks");
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    lumen.app_mut().set_audio_input(VOX, AudioInput::mono(9));
    lumen.frames(5);
    unfold(&mut lumen, "Vox");
    lumen.click("Vox input: Input 10");
    assert!(lumen.shows("Input 10 (not on this device)"));
}
