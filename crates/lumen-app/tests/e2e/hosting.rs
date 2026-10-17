//! Plugin devices on tracks (M6 slice 2c): a project that holds the test
//! plugin's instrument or effect starts a host process for it in the
//! background, plays through it once it has loaded, keeps its time while it
//! loads or is missing, and survives its host crashing. The plugins and the
//! host program are the real ones, built by the test.
//!
//! The interface that adds devices arrives in slice 2d: these projects are
//! written with the devices already on their tracks, as a saved project
//! would hold them.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_plugin_catalog::catalog::class_id;
use lumen_project::model::{Device, DeviceKind, Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::plugins::{built, plugin_root};

const KEYS: Id = Id(1);
const PLUGIN: Id = Id(20);

fn instrument_class() -> String {
    class_id(&lumen_test_plugin::INSTRUMENT.map(|c| c as u8))
}

fn effect_class() -> String {
    class_id(&lumen_test_plugin::EFFECT.map(|c| c as u8))
}

/// A MIDI track holding one long note, with its devices changed by `devices`
/// (it starts with the synth).
fn project(devices: impl FnOnce(&mut Vec<Device>)) -> Project {
    let mut track = midi_track(
        KEYS.0,
        "Keys",
        "poly",
        (0, 64),
        None,
        vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
        vec![],
    );
    devices(&mut track.devices);
    project_of("Hosted", vec![track])
}

fn instrument_device(id: Id) -> Device {
    Device::vst3(
        id,
        &instrument_class(),
        "Deterministic Test Instrument",
        "Lumen",
        DeviceKind::Instrument,
    )
}

/// The effect with its gain at the bottom (-60 dB) and no latency: it is
/// heard only by what it takes away. The test plugin's saved state is its
/// tag, its kind (1 for the effect) and its two parameters' values.
fn quiet_effect(id: Id) -> Device {
    let mut device = effect_device(id);
    device.state = [
        b"LTP1".as_slice(),
        &[1],
        &0.0_f64.to_le_bytes(),
        &0.0_f64.to_le_bytes(),
    ]
    .concat();
    device
}

fn effect_device(id: Id) -> Device {
    Device::vst3(
        id,
        &effect_class(),
        "Deterministic Test Effect",
        "Lumen",
        DeviceKind::Effect,
    )
}

/// The fault a module's host is given, by the module's name.
fn faults(stem: &str) -> Option<&'static str> {
    match stem {
        // The host dies on its 40th block.
        "Dies" => Some("process_crash@40"),
        _ => None,
    }
}

/// Lumen with `project` open and the modules named installed.
fn launch(name: &str, project: &Project, modules: &[&str]) -> Lumen {
    let dir = TempDir::new(&format!("e2e-hosting-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("save the test project");
    let root = plugin_root(dir.path(), modules);
    let cache = dir.path().join("plugin-cache.json");
    let tracks = project.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |launch| {
        launch
            .opening(path)
            // Long blocks (21 ms, so a 16 ms wait for the host): a host that
            // misses its deadline is silenced, and on a busy CI machine a
            // few milliseconds are easily lost. The deadline's own precision
            // is MT-VST-2's, not these scenarios'.
            .with_audio_buffer(1024)
            .with_plugins(vec![root], &built().1, cache)
            .with_plugin_faults(faults)
    });
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    lumen.frames(30);
    lumen
}

fn state(lumen: &Lumen, device: Id) -> String {
    lumen.app().device_state(device).unwrap_or_default()
}

#[track_caller]
fn wait_for_state(lumen: &mut Lumen, device: Id, want: &str) {
    let want = want.to_owned();
    lumen.wait_for(&format!("device {} to be {want}", device.0), move |app| {
        app.device_state(device)
            .is_some_and(|s| s.starts_with(&want))
    });
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
}

fn peak(heard: &[[f32; 4]]) -> f32 {
    heard
        .iter()
        .fold(0.0_f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

/// The quietest of a few windows of audio, after letting it settle. An effect
/// whose host answers a block late is bypassed for that block (its input goes
/// through, by design), and on a loaded CI machine one block in a window is
/// enough to read a "taken down 60 dB" signal as loud: what an effect takes
/// away is what its quietest window shows.
fn quietest(lumen: &mut Lumen) -> f32 {
    let first = peak(&lumen.listen(Duration::from_millis(600), 4_800));
    (0..4).fold(first, |least, _| {
        least.min(peak(&lumen.listen(Duration::from_millis(100), 4_800)))
    })
}

#[test]
fn a_plugin_instrument_loads_in_the_background_and_is_heard() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("instrument", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    let heard = lumen.listen(Duration::from_millis(100), 4_800);
    assert!(peak(&heard) > 0.01, "the plugin instrument is silent");
}

#[test]
fn a_plugin_that_is_not_installed_is_missing_silent_and_keeps_its_state_through_a_save() {
    let mut device = instrument_device(PLUGIN);
    device.state = vec![1, 2, 3, 4];
    device.reported = Some(lumen_project::model::Reported {
        latency: 64,
        tail: 0,
        rate: 48_000,
    });
    let project = project(|d| *d = vec![device.clone()]);
    let mut lumen = launch("missing", &project, &[]);
    wait_for_state(&mut lumen, PLUGIN, "missing");
    play(&mut lumen);
    let heard = lumen.listen(Duration::from_millis(100), 4_800);
    assert_eq!(peak(&heard), 0.0, "a missing instrument plays nothing");
    // Nothing of the device is lost to a save.
    let saved = lumen.app().project_as_saved();
    assert_eq!(saved.tracks[0].devices[0], device);
}

#[test]
fn an_effect_on_a_track_is_heard_and_reports_its_latency_for_the_save() {
    let project = project(|d| d.push(quiet_effect(PLUGIN)));
    let mut lumen = launch("effect", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    // Past the synth's attack: a note that has not sounded yet proves nothing.
    let quiet = quietest(&mut lumen);
    assert!(
        quiet < 0.005,
        "the synth is not heard through an effect that takes it down 60 dB: {quiet}"
    );
    let reported = lumen.app().project_as_saved().tracks[0].devices[1]
        .reported
        .expect("a running plugin's latency is saved");
    assert_eq!(Some(reported.latency), lumen.app().device_latency(PLUGIN));
    assert_eq!(reported.rate, 48_000);
    // The project itself is not changed by hearing it.
    assert!(!lumen.app().is_dirty());
}

#[test]
fn bypassing_removing_and_undoing_a_plugin_device() {
    let project = project(|d| d.push(quiet_effect(PLUGIN)));
    let mut lumen = launch("edits", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    let level = |lumen: &mut Lumen| peak(&lumen.listen(Duration::from_millis(600), 4_800));
    assert!(
        quietest(&mut lumen) < 0.005,
        "the effect takes the synth down"
    );

    lumen.app_mut().set_device_bypass(KEYS, PLUGIN, true);
    lumen.frames(5);
    assert!(
        level(&mut lumen) > 0.01,
        "a bypassed effect lets it through"
    );
    assert_eq!(state(&lumen, PLUGIN), "ready", "bypass keeps the host");

    lumen.app_mut().remove_device(KEYS, PLUGIN);
    lumen.frames(5);
    assert_eq!(lumen.app().device_state(PLUGIN), None, "removed");
    assert!(level(&mut lumen) > 0.01, "the synth plays on without it");

    // Undo brings it back, still bypassed, in a new host.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    assert!(level(&mut lumen) > 0.01, "back, and still bypassed");

    lumen.app_mut().set_device_bypass(KEYS, PLUGIN, false);
    lumen.frames(5);
    assert!(
        quietest(&mut lumen) < 0.005,
        "on again, taking the synth down"
    );
}

#[test]
fn a_plugin_whose_host_dies_is_flagged_and_the_rest_of_the_song_plays_on() {
    // The effect's host crashes on its 40th block.
    let project = project(|d| d.push(effect_device(PLUGIN)));
    let mut lumen = launch("dies", &project, &["Dies"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    wait_for_state(&mut lumen, PLUGIN, "failed");
    // The synth on the track keeps playing through the effect's fallback.
    let heard = lumen.listen(Duration::from_millis(100), 4_800);
    assert!(peak(&heard) > 0.01, "the track went silent with its effect");
    assert!(lumen.app().is_playing());
}

#[test]
fn reload_starts_a_new_host_for_a_device_whose_host_died() {
    let project = project(|d| d.push(effect_device(PLUGIN)));
    let mut lumen = launch("reload", &project, &["Dies"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    wait_for_state(&mut lumen, PLUGIN, "failed");
    // Stopped, the new host is not asked for blocks, so it lives until it
    // is played again.
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("stop", |app| !app.is_playing());
    lumen.app_mut().reload_device(PLUGIN);
    lumen.frames(1);
    assert_eq!(state(&lumen, PLUGIN), "loading");
    wait_for_state(&mut lumen, PLUGIN, "ready");
}

// Slice 3b: chains edited while the song plays. Devices keep their hosts when
// they are reordered or moved to another track, a copy has a host of its own,
// and an instrument that moves starts again where it now plays.

const PAD: Id = Id(10);
const SECOND: Id = Id(21);

/// `KEYS` with its long note, then `PAD` (which holds `pad_notes`), each with
/// the devices `keys` and `pad` leave on it (both start with the synth).
fn two_tracks(
    keys: impl FnOnce(&mut Vec<Device>),
    pad: impl FnOnce(&mut Vec<Device>),
    pad_notes: Vec<Note>,
) -> Project {
    let mut project = project(keys);
    let mut track = midi_track(PAD.0, "Pad", "pad", (0, 64), None, pad_notes, vec![]);
    pad(&mut track.devices);
    project.tracks.push(track.into());
    project
}

fn level(lumen: &mut Lumen) -> f32 {
    peak(&lumen.listen(Duration::from_millis(600), 4_800))
}

#[test]
fn an_effect_moved_to_another_track_keeps_its_host_and_the_sound_follows_it() {
    let project = two_tracks(|d| d.push(quiet_effect(PLUGIN)), |_| {}, vec![]);
    let mut lumen = launch("move-effect", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    assert!(
        quietest(&mut lumen) < 0.005,
        "the keys sit behind the effect"
    );

    lumen.app_mut().move_device(KEYS, PLUGIN, PAD, None);
    lumen.frames(1);
    assert_eq!(
        state(&lumen, PLUGIN),
        "ready",
        "the same host, never reloaded"
    );
    lumen.frames(5);
    assert!(level(&mut lumen) > 0.01, "the keys are heard without it");
    assert_eq!(lumen.app().project().tracks[1].devices.len(), 2);

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert_eq!(state(&lumen, PLUGIN), "ready");
    assert!(
        quietest(&mut lumen) < 0.005,
        "undo put it back in front of the keys"
    );
}

#[test]
fn a_copied_effect_has_a_host_of_its_own_and_the_original_is_not_disturbed() {
    let project = two_tracks(|d| d.push(quiet_effect(PLUGIN)), |_| {}, vec![]);
    let mut lumen = launch("copy-effect", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);

    let copy = lumen
        .app_mut()
        .copy_device(KEYS, PLUGIN, PAD, None)
        .expect("the copy");
    assert_ne!(copy, PLUGIN);
    lumen.frames(1);
    assert_eq!(state(&lumen, copy), "loading", "a new host starts for it");
    assert_eq!(state(&lumen, PLUGIN), "ready", "the original keeps running");
    wait_for_state(&mut lumen, copy, "ready");
    assert!(
        quietest(&mut lumen) < 0.005,
        "the keys are still behind the original"
    );

    // Taking the original away leaves the copy running on the other track.
    lumen.app_mut().remove_device(KEYS, PLUGIN);
    lumen.frames(5);
    assert_eq!(lumen.app().device_state(PLUGIN), None);
    assert_eq!(state(&lumen, copy), "ready");
    assert!(level(&mut lumen) > 0.01, "the keys are heard now");
}

#[test]
fn reordering_effects_while_playing_keeps_every_host_and_the_sound_continuous() {
    let project = project(|d| {
        d.push(quiet_effect(PLUGIN));
        d.push(effect_device(SECOND));
    });
    let mut lumen = launch("reorder", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    wait_for_state(&mut lumen, SECOND, "ready");
    play(&mut lumen);
    assert!(quietest(&mut lumen) < 0.005);

    let synth = lumen.app().project().tracks[0].devices[0].id;
    lumen.app_mut().move_device(KEYS, SECOND, KEYS, Some(synth));
    lumen.frames(1);
    let order: Vec<Id> = lumen.app().project().tracks[0]
        .devices
        .iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(
        order[1..],
        [SECOND, PLUGIN],
        "the second effect is first now"
    );
    assert_eq!(state(&lumen, PLUGIN), "ready");
    assert_eq!(state(&lumen, SECOND), "ready");
    lumen.frames(5);
    assert!(
        quietest(&mut lumen) < 0.005,
        "still taken down in the other order"
    );
    assert!(lumen.app().is_playing());
}

#[test]
fn an_instrument_moved_to_another_track_starts_there_and_nothing_hangs() {
    // A note on every beat, back to back: a note held while the instrument
    // loads is not picked up when it arrives (KNOWN_ISSUES), the next is.
    let notes: Vec<Note> = (0..120)
        .map(|i| {
            Note::new(
                Ticks::quarters(i),
                Ticks::quarters(1),
                60 + (i % 2) as u8,
                100,
            )
        })
        .collect();
    let project = two_tracks(|d| *d = vec![instrument_device(PLUGIN)], |_| {}, notes);
    let mut lumen = launch("move-instrument", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    assert!(level(&mut lumen) > 0.01);

    lumen.app_mut().move_device(KEYS, PLUGIN, PAD, None);
    lumen.frames(1);
    assert_eq!(
        state(&lumen, PLUGIN),
        "loading",
        "it starts where it plays now"
    );
    assert!(lumen.app().project().tracks[0].devices.is_empty());
    assert_eq!(lumen.app().project().tracks[1].devices[0].id, PLUGIN);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    assert!(level(&mut lumen) > 0.01, "the pad's note sounds on it");

    // Stopped, nothing keeps sounding.
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("stop", |app| !app.is_playing());
    lumen.frames(30);
    assert!(
        level(&mut lumen) < 0.001,
        "a note is left sounding on the old track or the new"
    );
}
