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
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::time::Ticks;
use lumen_plugin_catalog::catalog::class_id;
use lumen_project::model::{Device, DeviceKind, Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::plugins::{built, plugin_root};

const KEYS: Id = Id(1);
pub(super) const PLUGIN: Id = Id(20);

fn instrument_class() -> String {
    class_id(&lumen_test_plugin::INSTRUMENT.map(|c| c as u8))
}

fn effect_class() -> String {
    class_id(&lumen_test_plugin::EFFECT.map(|c| c as u8))
}

/// A MIDI track holding one long note, with its devices changed by `devices`
/// (it starts with the synth).
pub(super) fn project(devices: impl FnOnce(&mut Vec<Device>)) -> Project {
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

pub(super) fn instrument_device(id: Id) -> Device {
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

pub(super) fn effect_device(id: Id) -> Device {
    Device::vst3(
        id,
        &effect_class(),
        "Deterministic Test Effect",
        "Lumen",
        DeviceKind::Effect,
    )
}

/// The fault a module's host is given, by the module's name.
pub(super) fn faults(stem: &str) -> Option<&'static str> {
    match stem {
        // The host dies on its 40th block.
        "Dies" => Some("process_crash@40"),
        // Released notes fade out over four seconds; no tail is reported.
        "Rings" => Some("release=4000"),
        // Every block takes 3 ms of the plugin's work.
        "Slow" => Some("process_slow=3000"),
        // The instrument has a trigger that makes it edit its wave, as a
        // control moved in its own window would.
        "Edits" => Some("window_edits"),
        // Programs, and a trigger that edits the wave as a control moved in
        // the plugin's window would (the preset menu's dot).
        "PresetEdits" => Some("presets,window_edits"),
        // A plugin still loading its content: it refuses a state twice, then
        // takes it (the preset's retries), or refuses it every time.
        "Reluctant" => Some("presets,state_refused=2"),
        "Refuses" => Some("presets,state_refused=9"),
        // Stepping the trigger makes it tell the host it changed inside
        // (`setDirty`, or a restart that says its values changed), as a patch
        // loaded in its own window does; no parameter shows it.
        "Dirty" => Some("set_dirty"),
        "Values" => Some("values_changed"),
        // Reading the plugin's state never returns.
        "Stuck" => Some("state_hang"),
        // Many parameters, in groups, some hidden, some read-only.
        "Many" => Some("many_params=300"),
        // A program list and a program-change parameter, and a controller state.
        "Presets" => Some("presets"),
        _ => None,
    }
}

/// Lumen with `project` open and the modules named installed.
pub(super) fn launch(name: &str, project: &Project, modules: &[&str]) -> Lumen {
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
pub(super) fn wait_for_state(lumen: &mut Lumen, device: Id, want: &str) {
    let want = want.to_owned();
    lumen.wait_for(&format!("device {} to be {want}", device.0), move |app| {
        app.device_state(device)
            .is_some_and(|s| s.starts_with(&want))
    });
}

pub(super) fn play(lumen: &mut Lumen) {
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
fn a_plugin_instruments_release_is_heard_until_it_ends_not_cut_after_a_second() {
    // The owner's report (2026-10-17): a synth's release, and a delay in it,
    // were cut about a second after the last note, because the plugin was
    // given blocks only for its reported tail (none) or a second. It is now
    // given them until its output goes quiet.
    let mut track = midi_track(
        KEYS.0,
        "Keys",
        "poly",
        (0, 64),
        None,
        vec![Note::new(Ticks::ZERO, Ticks::quarters(1), 69, 100)],
        vec![],
    );
    track.devices = vec![instrument_device(PLUGIN)];
    let project = project_of("Release", vec![track]);
    let mut lumen = launch("release", &project, &["Rings"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    // The note ends half a second in (a beat at 120 BPM); two seconds after
    // that, its four-second release is still about half as loud.
    let heard = lumen.listen(Duration::from_millis(2_500), 4_800);
    assert!(peak(&heard) > 0.01, "the release was cut: {}", peak(&heard));
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

pub(super) fn level(lumen: &mut Lumen) -> f32 {
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

// Slice 3d: the master takes effects like any track.

#[test]
fn an_effect_on_the_master_is_heard_on_everything_and_bypass_lets_it_through() {
    let mut project = project(|_| {});
    std::sync::Arc::make_mut(&mut project.master).devices = vec![quiet_effect(PLUGIN)];
    let mut lumen = launch("master-effect", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    assert!(
        quietest(&mut lumen) < 0.005,
        "the keys are taken down on the master"
    );
    let master = lumen.app().project().master.id;
    lumen.app_mut().set_device_bypass(master, PLUGIN, true);
    lumen.frames(5);
    assert!(level(&mut lumen) > 0.01, "bypassed, the keys come through");
    assert_eq!(state(&lumen, PLUGIN), "ready", "bypass keeps the host");
}

#[test]
fn an_effect_moved_onto_the_master_takes_everything_down_and_undo_restores_it() {
    let project = two_tracks(|d| d.push(quiet_effect(PLUGIN)), |_| {}, vec![]);
    let mut lumen = launch("move-to-master", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    assert!(quietest(&mut lumen) < 0.005);
    let master = lumen.app().project().master.id;
    // Keys is the only track with notes: the effect on the master quiets it
    // the same, and still on the same host.
    lumen.app_mut().move_device(KEYS, PLUGIN, master, None);
    lumen.frames(1);
    assert_eq!(state(&lumen, PLUGIN), "ready", "the same host");
    lumen.frames(5);
    assert!(quietest(&mut lumen) < 0.005, "now on the master");
    assert_eq!(lumen.app().project().master.devices.len(), 1);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert!(lumen.app().project().master.devices.is_empty());
    assert!(quietest(&mut lumen) < 0.005, "and back on the keys");
}

/// A node's accessible name, or its value when the name is empty.
fn text(node: &egui_kittest::Node<'_>) -> String {
    let node = node.accesskit_node();
    let name = node.label().unwrap_or_default();
    if name.is_empty() {
        node.value().unwrap_or_default()
    } else {
        name
    }
}

/// The milliseconds after `what` in an accessible name ("work 3.01 ms").
fn millis_after(label: &str, what: &str) -> f64 {
    let at = label.find(what).expect("the figure is named") + what.len();
    let figure: String = label[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    figure.parse().expect("a figure")
}

#[test]
fn the_audio_panel_shows_each_plugins_work_in_the_block_budget() {
    // Owner (2026-10-17, ADR-0075): with two plugins on a track the CPU
    // readout was high and nothing said where the time went. The audio
    // panel's budget lists each running plugin with its own work and the
    // wait beyond it.
    const EFFECT: Id = Id(21);
    let project = project(|d| *d = vec![instrument_device(PLUGIN), effect_device(EFFECT)]);
    let mut lumen = launch("budget", &project, &["Slow"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    wait_for_state(&mut lumen, EFFECT, "ready");
    play(&mut lumen);
    lumen.harness.get_by_label_contains("Audio: ").click();
    lumen.frames(3);
    // The budget is taken once a second: wait for a stretch with both.
    let rows = [
        "Deterministic Test Instrument, Keys · instrument: ",
        "Deterministic Test Effect, Keys · effect 1: ",
    ];
    let until = std::time::Instant::now() + Duration::from_secs(30);
    while rows.iter().any(|r| {
        lumen
            .harness
            .query_by_label_contains(&format!("{r}work"))
            .is_none_or(|row| millis_after(&text(&row), "work ") == 0.0)
    }) {
        assert!(
            std::time::Instant::now() < until,
            "the budget never listed both plugins"
        );
        std::thread::sleep(Duration::from_millis(20));
        lumen.frames(1);
    }
    assert!(
        lumen
            .harness
            .query_by_label_contains("Block budget: ")
            .is_some()
    );
    for row in rows {
        let label = text(&lumen.harness.get_by_label_contains(row));
        let work = millis_after(&label, "work ");
        // 3 ms a block of the plugin's own work, give or take its timing.
        assert!((2.5..10.0).contains(&work), "{label}");
    }
}

#[test]
fn plugins_are_promised_the_devices_block_and_follow_a_new_buffer_size() {
    // Owner (2026-10-17, ADR-0075, option B): a plugin is told the device's
    // block size, not a ceiling far above it, and is set up again when the
    // buffer size changes. The scenarios run 1024-frame blocks, which the
    // engine hands plugins as 256 at most.
    let project = project(|d| *d = vec![quiet_effect(PLUGIN)]);
    let mut lumen = launch("promised", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen.wait_for("the plugin promised 256 frames", |app| {
        app.device_promised_frames(PLUGIN) == Some(256)
    });
    lumen.click("Lumen");
    lumen.click("Audio and MIDI settings\u{2026}");
    lumen.frames(5);
    lumen.harness.get_by_label_contains("Buffer size: ").click();
    lumen.frames(3);
    lumen.click("64 samples");
    lumen.wait_for("the plugin promised 64 frames", |app| {
        app.device_promised_frames(PLUGIN) == Some(64)
    });
    wait_for_state(&mut lumen, PLUGIN, "ready");
}

// Slice 4: parameters. The project holds the values a plugin is set to over its
// saved state, so setting one, undoing and redoing are heard, and what the
// plugin does inside itself is recorded as an edit.

pub(super) const LEVEL: u32 = 0;
pub(super) const WAVE: u32 = 1;
const TRIGGER: u32 = 90;

#[test]
fn a_parameter_set_in_the_project_is_heard_and_so_are_undo_and_redo() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("param-heard", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    let loud = level(&mut lumen);
    assert!(loud > 0.05, "the instrument is silent: {loud}");

    lumen.app_mut().set_device_param(PLUGIN, LEVEL, 0.1);
    lumen.frames(5);
    lumen.wait_for("the value to reach the plugin", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.1)
    });
    let quiet = level(&mut lumen);
    assert!(quiet < loud * 0.4, "{quiet} is not much below {loud}");

    // Undo puts the plugin back to the value its saved state holds.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the value to go back", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.5)
    });
    let again = level(&mut lumen);
    assert!(again > loud * 0.7, "{again} is not back near {loud}");
    assert!(lumen.app().device_params(PLUGIN).is_empty());

    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    lumen.wait_for("the value to come back", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.1)
    });
    assert!(level(&mut lumen) < loud * 0.4);
}

#[test]
fn values_the_project_holds_are_in_place_when_the_plugin_loads_and_do_not_dirty_it() {
    let mut device = instrument_device(PLUGIN);
    device.params = [(LEVEL, 0.1)].into_iter().collect();
    let project = project(|d| *d = vec![device.clone()]);
    let mut lumen = launch("param-load", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    assert_eq!(lumen.app().device_param_known(PLUGIN, LEVEL), Some(0.1));
    play(&mut lumen);
    let quiet = level(&mut lumen);
    assert!(quiet < 0.2, "the saved value was not in place: {quiet}");
    assert!(!lumen.app().is_dirty());
    // Dropping the held value (an undo of the change that made it) goes back
    // to what the plugin's own saved state says.
    assert!(
        lumen
            .app()
            .device_param_titles(PLUGIN)
            .contains(&"Level".to_owned())
    );
}

#[test]
fn what_a_plugin_does_inside_itself_is_recorded_as_one_undo_step() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("param-inside", &project, &["Edits"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    // The trigger makes the plugin move its wave in a gesture of its own, as a
    // control moved in its window would (and not as an answer to a value of
    // Lumen's, which is not recorded as an edit).
    lumen.app_mut().poke_plugin(PLUGIN, TRIGGER, 1.0);
    lumen.wait_for("the plugin's own edit to be recorded", |app| {
        app.device_params(PLUGIN).iter().any(|&(id, _)| id == WAVE)
    });
    assert_eq!(lumen.app().device_params(PLUGIN), [(WAVE, 1.0)]);
    // The edit is already known to the host, so nothing was sent back.
    assert_eq!(lumen.app().device_param_known(PLUGIN, WAVE), Some(1.0));
    assert!(lumen.app().is_dirty());

    // Undo takes the plugin's own edit back first, as one step, and the
    // plugin goes back to the wave its saved state holds.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the wave to go back", |app| {
        app.device_param_known(PLUGIN, WAVE) == Some(0.0)
    });
    assert!(lumen.app().device_params(PLUGIN).is_empty());
}

// Slice 4g: what a save holds of a plugin that changed.

fn saved_device(lumen: &Lumen) -> Device {
    lumen.app().project_as_saved().tracks[0].devices[0].clone()
}

#[test]
fn a_plugin_that_changed_is_read_once_it_rests_and_the_save_holds_what_it_is_now() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("capture", &project, &["Edits"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    assert!(
        saved_device(&lumen).state.is_empty(),
        "loaded with no state"
    );

    lumen.app_mut().set_device_param(PLUGIN, LEVEL, 0.3);
    // Until it is read, a save keeps the old state and the project's values.
    assert!(saved_device(&lumen).state.is_empty());
    assert_eq!(saved_device(&lumen).params.get(&LEVEL), Some(&0.3));

    lumen.wait_for("the plugin's state to be read", |app| {
        !app.project_as_saved().tracks[0].devices[0].state.is_empty()
    });
    let saved = saved_device(&lumen);
    assert!(saved.state.starts_with(b"LTP1"), "the plugin's own state");
    assert!(saved.params.is_empty(), "the values are in the state now");
    // The project in memory is untouched by it: still the edit, undoable.
    assert_eq!(lumen.app().device_params(PLUGIN), [(LEVEL, 0.3)]);

    // Another change makes the read state old again: the save holds that last
    // good state (never less than it had) with the values that moved since set
    // over it.
    lumen.app_mut().set_device_param(PLUGIN, LEVEL, 0.9);
    lumen.frames(2);
    assert!(saved_device(&lumen).state.starts_with(b"LTP1"));
    assert_eq!(saved_device(&lumen).params.get(&LEVEL), Some(&0.9));
}

/// Steps the plugin's trigger on and off as its own window would, until the
/// project reads as having unsaved changes (what a plugin says in its first
/// seconds is taken as its loading).
#[track_caller]
fn change_inside(lumen: &mut Lumen) {
    const TRIGGER: u32 = 90;
    for i in 0..60 {
        if lumen.app().is_dirty() {
            return;
        }
        lumen
            .app_mut()
            .poke_plugin(PLUGIN, TRIGGER, f64::from(i % 2 == 0));
        lumen.frames(6);
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the project never read as changed");
}

#[test]
fn a_change_inside_a_plugin_marks_the_project_unsaved_with_nothing_to_undo_and_a_save_clears_it() {
    for module in ["Dirty", "Values"] {
        let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
        let mut lumen = launch(&format!("inside-{module}"), &project, &[module]);
        wait_for_state(&mut lumen, PLUGIN, "ready");
        assert!(!lumen.app().is_dirty(), "{module}: opened as saved");
        let undoable = lumen.app().project().clone();
        change_inside(&mut lumen);
        assert_eq!(lumen.app().project(), &undoable, "{module}: no edit");
        // It is read once it rests, and a save holds what it is now.
        let path = lumen.dir().join("saved.lumen");
        lumen.app_mut().save_to(path.clone());
        lumen.wait_for("the save", |app| !app.is_busy());
        assert!(!lumen.app().is_dirty(), "{module}: saved");
        let reopened = lumen_project::load(&path).expect("the saved project loads");
        assert!(
            reopened.tracks[0].devices[0].state.starts_with(b"LTP1"),
            "{module}: the plugin's state is in the file"
        );
    }
}

#[test]
fn a_plugin_saying_it_changed_while_it_loads_is_not_the_users_change() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("inside-loading", &project, &["Dirty"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    // At once, before the host has been up for a few seconds.
    lumen.app_mut().poke_plugin(PLUGIN, 90, 0.0);
    lumen.app_mut().poke_plugin(PLUGIN, 90, 1.0);
    lumen.frames(20);
    assert!(!lumen.app().is_dirty());
}

#[test]
fn a_save_reads_every_running_plugin_first_even_one_that_reported_no_change() {
    // A plugin keeps most of what the user sets (a patch, a preset) without
    // telling the host: nothing marks it changed, so only reading it at the
    // save finds out. The owner's BBC Symphony Orchestra and Ozone lost their
    // settings on reopening because nothing was read.
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("save-reads", &project, &["Edits"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    assert!(saved_device(&lumen).state.is_empty(), "no change reported");
    let path = lumen.dir().join("saved.lumen");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy());
    let reopened = lumen_project::load(&path).expect("the saved project loads");
    assert!(
        reopened.tracks[0].devices[0].state.starts_with(b"LTP1"),
        "the plugin's own state is in the file"
    );
}

#[test]
fn a_save_does_not_wait_for_a_plugin_that_cannot_answer() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("save-stuck", &project, &["Stuck"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    let path = lumen.dir().join("saved.lumen");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy());
    let reopened = lumen_project::load(&path).expect("the saved project loads");
    assert!(
        reopened.tracks[0].devices[0].state.is_empty(),
        "saved with what the project had"
    );
}

#[test]
fn a_plugin_that_cannot_be_read_is_saved_with_the_values_the_project_holds_and_the_save_says_so() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("stuck", &project, &["Stuck"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen.app_mut().set_device_param(PLUGIN, LEVEL, 0.3);
    lumen.wait_for("the read to fail", |app| app.unread_plugins() == 1);
    let saved = saved_device(&lumen);
    assert!(saved.state.is_empty());
    assert_eq!(saved.params.get(&LEVEL), Some(&0.3), "nothing set is lost");
    lumen.key(Modifiers::COMMAND, Key::S);
    // The save waits for the plugin's read to fail, then writes what it has.
    lumen.wait_for("the save", |app| !app.is_busy());
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|t| t.contains("could not be read")),
        "{:?}",
        lumen.app().note()
    );
}
