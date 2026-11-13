//! Parallax and Afterglow in the app (M8 slice B3): their commands put each
//! device on a track with its personality, a delay's echoes are still heard
//! after Stop, and an export keeps a reverb's tail — read from the devices'
//! own tails rather than a flat ten seconds.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_devices::params;
use lumen_project::model::{Id, PluginRef};

use super::lane_view::{run_command, tone};
use super::{Lumen, wav};

/// The built-in device of `name` on the project's first track that has one.
fn builtin(lumen: &Lumen, name: &str) -> Option<Id> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|track| track.devices.iter())
        .find(|d| matches!(&d.plugin, PluginRef::Builtin { name: known } if known == name))
        .map(|d| d.id)
}

/// The normalised value of `id` at `value` in its own units.
fn at(table: &'static [params::ParamInfo], id: u32, value: f32) -> f64 {
    f64::from(
        params::find(table, id)
            .expect("a parameter of the table")
            .normalised(value),
    )
}

#[test]
fn the_two_commands_add_their_device_with_its_personality_and_one_undo_step() {
    let (mut lumen, _) = tone("b3-add");
    run_command(&mut lumen, "Add Parallax");
    lumen.wait_for("Parallax's card", |app| app.project().tracks.iter().any(|track| track.devices.iter().any(|d| matches!(&d.plugin, PluginRef::Builtin { name } if name == lumen_devices::delay::NAME))));
    lumen.frames(20);
    assert_eq!(lumen.app().undo_label(), Some("Add Parallax"));
    lumen.show_whole_cards();
    let delay = builtin(&lumen, lumen_devices::delay::NAME).expect("Parallax is on the track");
    assert!(lumen.shows("Parallax on tone"), "the card's spoken name");
    // The eight knobs are the table's, in its order.
    let ids: Vec<u32> = lumen_devices::delay::TABLE.iter().map(|p| p.id).collect();
    assert_eq!(lumen.app().strip_parameters(delay), ids);
    assert!(lumen.shows("Time, parameter: 375 ms"));
    assert!(lumen.shows("Note, choice: 1/4"));
    assert!(
        lumen.harness.query_by_label("All parameters (8)").is_some(),
        "every parameter is one button away"
    );

    run_command(&mut lumen, "Add Afterglow");
    lumen.wait_for("Afterglow's card", |app| {
        app.project().tracks.iter().any(|track| {
            track.devices.iter().any(|d| {
                matches!(&d.plugin, PluginRef::Builtin { name } if name == lumen_devices::reverb::NAME)
            })
        })
    });
    lumen.frames(20);
    assert_eq!(lumen.app().undo_label(), Some("Add Afterglow"));
    let reverb = builtin(&lumen, lumen_devices::reverb::NAME).expect("Afterglow is on the track");
    assert!(lumen.shows("Afterglow on tone"));
    let ids: Vec<u32> = lumen_devices::reverb::TABLE.iter().map(|p| p.id).collect();
    assert_eq!(lumen.app().strip_parameters(reverb), ids);
    assert!(lumen.shows("Decay, parameter: 2.00 s"));
    assert!(
        lumen.harness.query_by_label("All parameters (7)").is_some(),
        "the reverb's seven are all there"
    );
    // Each is one undo step.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("Afterglow undone", |app| {
        !app.project().tracks.iter().any(|track| {
            track.devices.iter().any(|d| {
                matches!(&d.plugin, PluginRef::Builtin { name } if name == lumen_devices::reverb::NAME)
            })
        })
    });
    assert_eq!(lumen.app().undo_label(), Some("Add Parallax"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("Parallax undone", |app| {
        app.project()
            .tracks
            .iter()
            .all(|track| track.devices.is_empty())
    });
}

#[test]
fn a_delay_s_echoes_are_heard_after_stop() {
    let (mut lumen, _) = tone("b3-delay-tail");
    run_command(&mut lumen, "Add Parallax");
    lumen.wait_for("Parallax's card", |app| app.project().tracks.iter().any(|track| track.devices.iter().any(|d| matches!(&d.plugin, PluginRef::Builtin { name } if name == lumen_devices::delay::NAME))));
    lumen.frames(20);
    let device = builtin(&lumen, lumen_devices::delay::NAME).expect("Parallax is on the track");
    // Wet, with a long feedback and a free 200 ms time, so the echoes are
    // still there a moment after the transport stops.
    lumen
        .app_mut()
        .set_device_param(device, lumen_devices::delay::NOTE, 0.0);
    lumen.app_mut().set_device_param(
        device,
        lumen_devices::delay::TIME,
        at(
            lumen_devices::delay::TABLE,
            lumen_devices::delay::TIME,
            200.0,
        ),
    );
    lumen.app_mut().set_device_param(
        device,
        lumen_devices::delay::MIX,
        at(
            lumen_devices::delay::TABLE,
            lumen_devices::delay::MIX,
            100.0,
        ),
    );
    lumen.app_mut().set_device_param(
        device,
        lumen_devices::delay::FEEDBACK,
        at(
            lumen_devices::delay::TABLE,
            lumen_devices::delay::FEEDBACK,
            85.0,
        ),
    );
    lumen.wait_for("the audio thread to have it", |app| {
        app.builtin_attached(device)
    });
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| {
        app.playhead() > lumen_core::time::Ticks::quarters(2)
    });
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(10);
    // The project's automatic tail is the delay's own (what the engine keeps
    // after Stop and an export adds).
    let tail = lumen.app().project_tail();
    assert!(tail > 48_000, "the delay rings on: {tail}");
    let heard = lumen.listen(Duration::from_millis(300), 3_000);
    let peak = heard
        .iter()
        .fold(0.0f32, |m, frame| m.max(frame[0].abs().max(frame[1].abs())));
    assert!(peak > 1e-3, "the echoes ring on after Stop: {peak}");
}

#[test]
fn an_export_keeps_a_reverb_s_tail_and_no_more() {
    // A fifth of a second of tone, a reverb whose decay is a second: the
    // export runs past the span for a tail of about a second, not the flat
    // ten it used to add whatever the project held.
    let mut lumen = Lumen::start("b3-export-tail");
    let input = wav(&lumen.dir().join("take.wav"), 0.2);
    lumen.drop_files(&[input]);
    lumen.wait_for("import", |app| {
        !app.is_busy() && !app.project().tracks.is_empty()
    });
    lumen.frames(10);
    let track = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[track]);
    run_command(&mut lumen, "Add Afterglow");
    lumen.wait_for("Afterglow's card", |app| {
        app.project().tracks.iter().any(|track| {
            track.devices.iter().any(|d| {
                matches!(&d.plugin, PluginRef::Builtin { name } if name == lumen_devices::reverb::NAME)
            })
        })
    });
    lumen.frames(10);
    let device = builtin(&lumen, lumen_devices::reverb::NAME).expect("Afterglow is on the track");
    lumen.app_mut().set_device_param(
        device,
        lumen_devices::reverb::DECAY,
        at(
            lumen_devices::reverb::TABLE,
            lumen_devices::reverb::DECAY,
            1.0,
        ),
    );
    lumen.app_mut().set_device_param(
        device,
        lumen_devices::reverb::MIX,
        at(
            lumen_devices::reverb::TABLE,
            lumen_devices::reverb::MIX,
            100.0,
        ),
    );
    lumen.wait_for("the audio thread to have it", |app| {
        app.builtin_attached(device)
    });
    // The export window, with its default (tails on).
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    lumen.frames(5);
    assert!(lumen.app().audio_export_open());
    let tail = lumen.app().project_tail();
    assert!(
        tail > 48_000 && tail < 3 * 48_000,
        "the reverb's own tail is the automatic one: {tail}"
    );
    let output = lumen.dir().join("tail.wav");
    lumen.app_mut().answer_next_dialog(vec![output.clone()]);
    lumen.click("Export audio…");
    lumen.wait_for("export completion", |app| !app.audio_export_busy());
    assert_eq!(lumen.app().audio_export_error(), None);
    let audio = decode(&output);
    let span = 0.2 * 48_000.0;
    let frames = audio.frames() as f64;
    assert!(
        frames > span + 0.4 * 48_000.0,
        "the reverb's tail is in the file: {frames}"
    );
    assert!(
        frames < span + 3.0 * 48_000.0,
        "and not ten seconds of it: {frames}"
    );
    let tail_loud = audio.channel(0)[(span as usize) + 480..]
        .iter()
        .any(|s| s.abs() > 1e-4);
    assert!(tail_loud, "there is sound after the span ends");
}

fn decode(path: &Path) -> lumen_media::Audio {
    lumen_media::decode(path, &AtomicBool::new(false), &mut |_| {}).unwrap()
}
