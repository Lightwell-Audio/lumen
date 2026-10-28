//! Playing audio from disk (M9 slice R6): with "Play audio from disk" on, a
//! long file plays from its stream file and is heard as it was from memory;
//! a short one stays in memory; turning the switch off loads them back.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;

use super::{Lumen, TempDir, wav};

/// The loudest left sample of a stretch of playback.
fn loudest(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(100), 4_800)
        .iter()
        .fold(0.0f32, |peak, f| peak.max(f[0].abs()))
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| {
        app.is_playing() && app.playhead() > Ticks::quarters(1)
    });
}

fn stop(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback to stop", |app| !app.is_playing());
}

#[test]
fn a_long_file_plays_from_disk_and_a_short_one_from_memory() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-streaming"), |launch| {
        launch.playing_from_disk()
    });
    let long = wav(&lumen.dir().join("long.wav"), 40.0);
    let short = wav(&lumen.dir().join("short.wav"), 2.0);
    lumen.drop_files(&[long, short]);
    let mut from_disk = lumen.app().media_from_disk();
    from_disk.sort();
    assert_eq!(from_disk, [("long".into(), true), ("short".into(), false)]);

    play(&mut lumen);
    let peak = loudest(&mut lumen);
    // Both tones at 3,000 of 32,767, on two tracks at unity.
    assert!(peak > 0.15, "the streamed tone is heard: {peak}");
    let reading = lumen.app().disk_reading().expect("the disk thread runs");
    assert_eq!(reading.failed, 0);
    stop(&mut lumen);
}

#[test]
fn the_switch_loads_files_again_in_memory_or_from_disk() {
    let mut lumen = Lumen::start("streaming-switch");
    let long = wav(&lumen.dir().join("long.wav"), 40.0);
    lumen.drop_files(&[long]);
    assert_eq!(lumen.app().media_from_disk(), [("long".into(), false)]);
    assert_eq!(lumen.app().disk_reading(), None, "no disk thread while off");

    lumen.app_mut().set_playing_from_disk(true);
    lumen.wait_for("the file to play from disk", |app| {
        app.media_from_disk() == [("long".into(), true)]
    });
    play(&mut lumen);
    let streamed = loudest(&mut lumen);
    stop(&mut lumen);

    lumen.app_mut().set_playing_from_disk(false);
    lumen.wait_for("the file to be in memory again", |app| {
        app.media_from_disk() == [("long".into(), false)] && app.disk_reading().is_none()
    });
    play(&mut lumen);
    let in_memory = loudest(&mut lumen);
    stop(&mut lumen);
    assert!(streamed > 0.05, "heard from disk: {streamed}");
    assert!(
        (streamed - in_memory).abs() < 1e-3,
        "the same from disk ({streamed}) as from memory ({in_memory})"
    );
}
