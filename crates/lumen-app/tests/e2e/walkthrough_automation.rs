//! The M7 exit flow (`docs/milestones/M7.md`, slice A8): a track with a plugin
//! instrument and a plugin effect gets a volume lane and a lane for the
//! effect's gain, both drawn with the pointer and both heard; a move of the
//! fader is recorded in Touch; a controller is mapped to the fader and its
//! sweep is recorded too; a tempo and a time-signature change are added; then
//! undo, save and reopen keep all of it, and the reopened song still plays
//! its lanes. The plugins and the host program are the real ones, built by the
//! test (the Lumen-owned test plugin); real third-party plugins and a hardware
//! controller are the owner's MT-AUTO-10.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::automation::Target;
use lumen_project::model::Id;

use super::Lumen;
use super::controllers::{cc, let_go as release_controller};
use super::hosting::{
    PLUGIN, effect_device, faults, instrument_device, launch_with, play, project, timing_sensitive,
    wait_for_state,
};
use super::lane_record::{grab, let_go};
use super::lane_view::{click_at, points, run_command, spot};
use super::midi_from::switch_on_for_tracks;
use super::plugins::built;
use super::song_lanes::{click_meter, click_tempo, type_in};

const KEYS: Id = Id(1);
const FX: Id = Id(21);
/// The effect's gain, which both test plugins give parameter 0.
const GAIN: Target = Target::Device {
    device: FX,
    param: 0,
};
/// The effect's gain at 0 dB, normalized: 60 of its 72 dB.
const UNITY: f32 = 60.0 / 72.0;
/// The controller the fader is mapped to.
const CHANNEL: u8 = 0;
const NUMBER: u8 = 7;

fn peak(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(100), 4_800)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

/// The quietest of a few windows: an effect whose host answers late is passed
/// through for that block, so what a lane takes away shows in the lowest.
fn quietest(lumen: &mut Lumen) -> f32 {
    (0..4).map(|_| peak(lumen)).fold(f32::MAX, f32::min)
}

fn loudest(lumen: &mut Lumen) -> f32 {
    (0..4).map(|_| peak(lumen)).fold(0.0, f32::max)
}

/// Plays from the start, up to the quarter `to`.
fn play_to(lumen: &mut Lumen, to: i64) {
    play(lumen);
    lumen.wait_for("the song to reach the quarter", move |app| {
        app.playhead() > Ticks::quarters(to)
    });
}

fn stop_and_rewind(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the stop", |app| !app.is_playing());
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);
}

/// The volume lane, as a list of points.
fn volume(lumen: &Lumen) -> Vec<(Ticks, f32)> {
    points(lumen, KEYS, Target::Volume)
}

/// Draws the volume lane: quiet at the start, opening to 0 dB.
fn draw_volume(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    assert_eq!(lumen.app().automation_shown(KEYS), Some(Target::Volume));
    for (quarter, db) in [(1.0, -24.0), (6.0, 0.0)] {
        let at = spot(lumen, KEYS, quarter, db);
        click_at(lumen, at);
    }
    let drawn = volume(lumen);
    assert_eq!(drawn.len(), 2, "two points drawn: {drawn:?}");
    assert!(drawn[0].0 < drawn[1].0 && drawn[0].1 < drawn[1].1, "{drawn:?}");
}

/// Chooses the effect's gain from the chip's plugin list and draws it: shut
/// until the second quarter, open by the third.
fn draw_gain(lumen: &mut Lumen) {
    lumen.click("Automation lane of Keys: Volume");
    lumen.click("Parameters of Deterministic Test Effect\u{2026}");
    lumen.frames(5);
    lumen.click("Automate Gain");
    lumen.frames(5);
    assert_eq!(lumen.app().automation_shown(KEYS), Some(GAIN));
    for (quarter, value) in [(2.0, 0.0), (3.0, UNITY)] {
        let at = spot(lumen, KEYS, quarter, value);
        click_at(lumen, at);
    }
    let drawn = points(lumen, KEYS, GAIN);
    assert_eq!(drawn.len(), 2, "two points drawn: {drawn:?}");
}

/// Shows the volume lane again through the chip.
fn show_volume(lumen: &mut Lumen) {
    lumen.click("Automation lane of Keys: Deterministic Test Effect \u{00B7} Gain");
    lumen.click("Volume  \u{00B7}  2 points");
    lumen.frames(5);
    assert_eq!(lumen.app().automation_shown(KEYS), Some(Target::Volume));
}

/// The lane is heard shut, then open: at the start the gain lane holds the
/// plugin at its bottom, and once it has opened the volume lane has reached
/// its top.
#[track_caller]
fn lanes_are_heard(lumen: &mut Lumen, why: &str) {
    play_to(lumen, 1);
    let shut = quietest(lumen);
    assert!(shut < 0.005, "{why}: the gain lane holds it shut: {shut}");
    lumen.wait_for("the lanes to open", |app| {
        app.playhead() > Ticks::quarters(6)
    });
    let open = loudest(lumen);
    assert!(open > 0.01, "{why}: and opens it: {open}");
    stop_and_rewind(lumen);
}

#[test]
fn lanes_drawn_recorded_and_mapped_with_a_tempo_and_meter_change_save_reopen_and_play() {
    let _turn = timing_sensitive();
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Pad");
    let song = project(|d| *d = vec![instrument_device(PLUGIN), effect_device(FX)]);
    let with = fake.clone();
    let mut lumen = launch_with("walkthrough-automation", &song, &["Plug"], move |l| {
        l.with_midi(with)
    });
    wait_for_state(&mut lumen, PLUGIN, "ready");
    wait_for_state(&mut lumen, FX, "ready");
    switch_on_for_tracks(&mut lumen, &fake, "Pad");
    lumen.app_mut().select_tracks(&[KEYS]);
    lumen.frames(10);

    // A volume lane drawn with the pointer is heard: quiet near the start,
    // louder once it has opened.
    draw_volume(&mut lumen);
    play_to(&mut lumen, 1);
    let early = loudest(&mut lumen);
    lumen.wait_for("the lane to open", |app| {
        app.playhead() > Ticks::quarters(6)
    });
    let late = loudest(&mut lumen);
    assert!(
        late > early * 4.0,
        "the volume lane opens the sound: {early} then {late}"
    );
    stop_and_rewind(&mut lumen);

    // A lane for the effect's gain, chosen from the plugin's own list: shut
    // at the start, open by the third quarter.
    draw_gain(&mut lumen);
    lanes_are_heard(&mut lumen, "both lanes");

    // Touch: a fader held while the song plays is written into its lane, and
    // the whole pass is one undo step.
    show_volume(&mut lumen);
    lumen.app_mut().set_automation_mode(KEYS, "touch");
    lumen.click("Fold Keys");
    lumen.frames(40);
    let drawn = volume(&lumen);
    play(&mut lumen);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let slider = lumen.harness.get_by_label_contains("Volume Keys:").rect();
    let at = grab(&mut lumen, slider);
    assert!(lumen.app().recording_automation(KEYS, Target::Volume));
    let_go(&mut lumen, at);
    lumen.frames(5);
    assert!(!lumen.app().recording_automation(KEYS, Target::Volume));
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the stop", |app| !app.is_playing());
    let touched = volume(&lumen);
    assert!(
        touched.len() > drawn.len(),
        "the move was written: {drawn:?} then {touched:?}"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(volume(&lumen), drawn, "the pass is one undo step");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    lumen.frames(3);
    assert_eq!(volume(&lumen), touched, "and redo brings it back");
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);

    // Learn: the next controller moved is mapped to the shown lane's fader,
    // and its sweep is written by Touch like a hand's.
    run_command(&mut lumen, "Learn a controller for the shown lane");
    assert_eq!(
        lumen.app().controller_learning(),
        Some((KEYS, Target::Volume))
    );
    cc(&fake, CHANNEL, NUMBER, 100);
    lumen.wait_for("the mapping", |app| app.project().mappings.len() == 1);
    let mapped = lumen.app().project().mappings[0];
    assert_eq!((mapped.track, mapped.target), (KEYS, Target::Volume));
    assert_eq!((mapped.channel, mapped.number), (CHANNEL, NUMBER));
    let before_sweep = volume(&lumen);
    play(&mut lumen);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    for value in [20_u8, 50, 80, 110] {
        cc(&fake, CHANNEL, NUMBER, value);
        std::thread::sleep(Duration::from_millis(40));
        lumen.frames(3);
    }
    release_controller(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the stop", |app| !app.is_playing());
    let swept = volume(&lumen);
    assert_ne!(swept, before_sweep, "the controller's sweep was written");
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);

    // The tempo slows at the eighth quarter, and the meter becomes 3/4 there.
    lumen.key(Modifiers::NONE, Key::T);
    lumen.frames(40);
    click_tempo(&mut lumen, 8.0, 100.0);
    click_meter(&mut lumen, 8.0);
    type_in(&mut lumen, "3/4");
    assert_eq!(lumen.app().project().tempo.len(), 2);
    assert_eq!(lumen.app().project().meter.len(), 2);

    // Save, close, reopen: the same project, and the lanes still play.
    let path = project_file(lumen.dir(), "Walk");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen.app().project_as_saved();
    assert_eq!(points(&lumen, KEYS, GAIN).len(), 2);
    let dir = lumen.crash();
    let root = dir.path().join("plugins");
    let cache = dir.path().join("plugin-cache.json");
    let with = fake.clone();
    let mut again = Lumen::start_with(dir, move |launch| {
        launch
            .opening(path)
            .with_audio_buffer(1024)
            .with_plugins(vec![root], &built().1, cache)
            .with_plugin_faults(faults)
            .with_midi(with)
    });
    again.wait_for("the project to open", |app| !app.is_busy());
    wait_for_state(&mut again, PLUGIN, "ready");
    wait_for_state(&mut again, FX, "ready");
    let opened = again.app().project_as_saved();
    assert_eq!(opened.tracks, saved.tracks);
    assert_eq!(opened.master, saved.master);
    assert_eq!(opened.tempo, saved.tempo);
    assert_eq!(opened.meter, saved.meter);
    assert_eq!(opened.mappings, saved.mappings);
    assert_eq!(points(&again, KEYS, Target::Volume), swept);
    assert_eq!(points(&again, KEYS, GAIN).len(), 2);
    lanes_are_heard(&mut again, "reopened");
}
