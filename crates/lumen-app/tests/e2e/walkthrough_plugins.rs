//! The M6 exit flow (`docs/milestones/M6.md`, slice 7): plugin instruments
//! and effects on tracks, two of the instruments' sounds grouped, one
//! instrument's other output on a track of its own, an effect keyed from
//! another track, a plugin effect on a return that a send feeds; then undo,
//! save and reopen, and a plugin that crashes and is reloaded while the rest
//! play on. The plugins and the host program are the real ones, built by
//! the test (the Lumen-owned test plugin); real third-party plugins are the
//! owner's MT-VST-7.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;
use lumen_project::model::{
    Color, Id, Level, Note, PluginOutput, Project, Send, Sidechain, SidechainTap, Track, TrackKind,
};
use lumen_project::testing::{midi_track, project_of};

use super::Lumen;
use super::hosting::{
    effect_device, faults, instrument_device, launch_with, play, timing_sensitive, wait_for_state,
};
use super::plugins::built;

const KICK: Id = Id(1);
const PERC: Id = Id(2);
const PERC_OUT: Id = Id(3);
const BASS: Id = Id(4);
const VERB: Id = Id(5);
const KICK_PLUGIN: Id = Id(10);
const PERC_PLUGIN: Id = Id(11);
const BASS_PLUGIN: Id = Id(12);
const KEYED: Id = Id(13);
const VERB_PLUGIN: Id = Id(14);
/// Frames in a block of the test device. The hosts get three quarters of a
/// block to answer; at the other scenarios' 1024 (21 ms) a stall of the
/// machine's scheduler a little over 16 ms makes the keyed effect late, and
/// a late effect passes its input dry. A sidechain that is meant to hold the
/// bass at silence then shows a burst, and the meter needs seconds of quiet to
/// fall back. 2048 (43 ms, a 32 ms wait, the test device's largest) rides such stalls out.
const BUFFER: u32 = 2048;
const PLUGINS: [Id; 5] = [KICK_PLUGIN, PERC_PLUGIN, BASS_PLUGIN, KEYED, VERB_PLUGIN];
// These notes sustain through the whole scenario, including slow UI work and
// every bounded audio probe. The old 60-quarter notes ended after 30 seconds,
// so a loaded runner could reach silence before removing the sidechain.
const HELD_QUARTERS: i64 = 7_200;

/// A kick and a percussion instrument, the percussion's second output on a
/// track of its own, a bass whose effect is keyed from the kick and which
/// sends to a return holding an effect. Every instrument holds one long
/// note; the test instrument with three outputs plays a note on the output
/// its pitch picks (pitch mod 3): the kick's 69 and the percussion's 61 are
/// outputs 0 and 1.
fn song() -> Project {
    let held = |pitch| {
        vec![Note::new(
            Ticks::ZERO,
            Ticks::quarters(HELD_QUARTERS),
            pitch,
            100,
        )]
    };
    let loud = |id| {
        let mut device = instrument_device(id);
        device.params.insert(lumen_test_plugin::params::LEVEL, 1.0);
        device
    };
    let mut kick = midi_track(
        KICK.0,
        "Kick",
        "poly",
        (0, HELD_QUARTERS),
        None,
        held(69),
        vec![],
    );
    kick.devices = vec![loud(KICK_PLUGIN)];
    let mut perc = midi_track(
        PERC.0,
        "Perc",
        "poly",
        (0, HELD_QUARTERS),
        None,
        held(61),
        vec![],
    );
    perc.devices = vec![loud(PERC_PLUGIN)];
    let mut perc_out = Track::new(PERC_OUT, "Perc out", TrackKind::Audio, Color(0x5FA89A));
    perc_out.plugin_output = Some(PluginOutput {
        device: PERC_PLUGIN,
        bus: 1,
    });
    let mut bass = midi_track(
        BASS.0,
        "Bass",
        "poly",
        (0, HELD_QUARTERS),
        None,
        held(40),
        vec![],
    );
    let mut keyed = effect_device(KEYED);
    keyed.sidechain = Some(Sidechain {
        from: KICK,
        tap: SidechainTap::PostFx,
    });
    bass.devices = vec![loud(BASS_PLUGIN), keyed];
    bass.sends = vec![Send {
        to: VERB,
        level: Level(0.0),
        pre_fader: false,
    }];
    let mut verb = Track::new(VERB, "Verb", TrackKind::Return, Color(0x9B7EC8));
    verb.devices = vec![effect_device(VERB_PLUGIN)];
    project_of("Walk", vec![kick, perc, perc_out, bass, verb])
}

fn diagnostics(lumen: &Lumen) -> String {
    let hosts = PLUGINS.map(|id| (id, lumen.app().device_state(id)));
    let meters = [KICK, PERC_OUT, BASS, VERB].map(|id| (id, lumen.app().meter(id).peak_db));
    format!(
        "transport: {:?}; hosts: {hosts:?}; meters: {meters:?}",
        lumen.app().transport_state(),
    )
}

fn listen(lumen: &mut Lumen) {
    lumen.listen(Duration::from_millis(100), 4_800);
    assert!(
        lumen.app().is_playing() && lumen.app().playhead() < Ticks::quarters(HELD_QUARTERS),
        "the fixture must still be sustaining its notes: {}",
        diagnostics(lumen)
    );
}

/// The loudest a track's meter reads over a few windows, stopping early once
/// it is above `db`. A plugin that is set up again after an edit is silent
/// until it has run, which takes longer on a slow runner: the limit is
/// generous, and a run that is not slow leaves early.
fn loudest(lumen: &mut Lumen, track: Id, db: f32) -> f32 {
    let mut best = f32::MIN;
    for _ in 0..150 {
        listen(lumen);
        best = best.max(lumen.app().meter(track).peak_db);
        if best > db {
            break;
        }
    }
    if best <= db {
        eprintln!(
            "track {track:?} never rose above {db} dB: {}",
            diagnostics(lumen)
        );
    }
    best
}

/// A track's meter once it has fallen below `db` (it has not heard the key
/// at first, or the plugin has to be set up for its sidechain), or the
/// lowest reading in the end. The meter falls 20 dB a second, so a stall that
/// lets a block through sends it back up and it needs a few quiet seconds
/// again: the limit is generous, and a run that is not stalled leaves early.
fn quietest(lumen: &mut Lumen, track: Id, db: f32) -> f32 {
    let mut least = f32::MAX;
    for _ in 0..150 {
        listen(lumen);
        least = least.min(lumen.app().meter(track).peak_db);
        if least < db {
            break;
        }
    }
    if least >= db {
        eprintln!(
            "track {track:?} never fell below {db} dB: {}",
            diagnostics(lumen)
        );
    }
    least
}

#[track_caller]
fn sounds_as_it_should(lumen: &mut Lumen, why: &str) {
    let kick = loudest(lumen, KICK, -20.0);
    assert!(kick > -20.0, "{why}: the kick plays: {kick} dB");
    let out = loudest(lumen, PERC_OUT, -20.0);
    assert!(
        out > -20.0,
        "{why}: the percussion's own output plays: {out} dB"
    );
    let own = lumen.app().meter(PERC).peak_db;
    assert!(
        own < -60.0,
        "{why}: the percussion track itself is silent: {own} dB"
    );
    let bass = quietest(lumen, BASS, -60.0);
    assert!(
        bass < -60.0,
        "{why}: the kick keys the bass down: {bass} dB"
    );
}

fn start(project: &Project) -> Lumen {
    let mut lumen = launch_with("walkthrough-plugins", project, &["Walk"], |l| {
        l.with_audio_buffer(BUFFER)
    });
    for device in PLUGINS {
        wait_for_state(&mut lumen, device, "ready");
    }
    lumen
}

fn remove_sidechain(lumen: &mut Lumen) {
    lumen.app_mut().select_tracks(&[BASS]);
    lumen
        .harness
        .get_all_by_label("Devices tab")
        .last()
        .expect("the detail panel's tabs")
        .click();
    lumen.frames(20);
    lumen.click("Sidechain source, Kick, Post FX");
    lumen.frames(5);
    lumen.click("None");
    lumen.frames(5);
    let keyed = lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|track| &track.devices)
        .find(|device| device.id == KEYED)
        .expect("the bass's keyed effect");
    assert!(keyed.sidechain.is_none(), "the UI removed the sidechain");
    let bass = loudest(lumen, BASS, -30.0);
    assert!(bass > -30.0, "unkeyed, the bass plays: {bass} dB");
    let verb = loudest(lumen, VERB, -40.0);
    assert!(verb > -40.0, "and feeds the return: {verb} dB");
}

#[test]
fn a_held_note_survives_sidechain_removal_past_the_old_thirty_second_fixture() {
    let _turn = timing_sensitive();
    let mut lumen = start(&song());
    // Locate instead of sleeping: this is beyond both the old note's end
    // (60 quarters) and its clip's end (64). The sound assertions must still
    // have held notes to measure, however long earlier UI work took.
    lumen.harness.get_by_label_contains("Position ").click();
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("17.1.1".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the later position", |app| {
        (app.playhead().0 - Ticks::quarters(64).0).abs() < 100
    });
    play(&mut lumen);
    sounds_as_it_should(&mut lumen, "past the old fixture's end");
    remove_sidechain(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn plugins_in_a_group_with_a_return_a_sidechain_and_outputs_save_reopen_and_survive_a_crash() {
    let _turn = timing_sensitive();
    let mut lumen = start(&song());

    // Group the kick and the percussion: one step, and the plugins keep
    // running through the new routing.
    lumen
        .harness
        .get_by_label("Kick: double-click to rename")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::SHIFT, Key::ArrowDown);
    lumen.key(Modifiers::COMMAND, Key::G);
    lumen.frames(20);
    assert_eq!(
        lumen.track_names(),
        ["Group 1", "Kick", "Perc", "Perc out", "Bass", "Verb"]
    );
    play(&mut lumen);
    sounds_as_it_should(&mut lumen, "grouped");
    // The bass is keyed down, so the return it feeds hears nothing either.
    let verb = quietest(&mut lumen, VERB, -60.0);
    assert!(verb < -60.0, "nothing reaches the return: {verb} dB");

    // Take the sidechain away on the bass's card: the bass plays, and its
    // send reaches the return, until Undo gives the key back.
    remove_sidechain(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    let bass = quietest(&mut lumen, BASS, -60.0);
    assert!(bass < -60.0, "undone, the kick keys it again: {bass} dB");
    lumen.key(Modifiers::NONE, Key::Space);

    // Save, close, reopen: the same project, the same sound.
    let path = project_file(lumen.dir(), "Walk");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen.app().project_as_saved();
    let dir = lumen.crash();
    let root = dir.path().join("plugins");
    let cache = dir.path().join("plugin-cache.json");
    let mut again = Lumen::start_with(dir, move |launch| {
        launch
            .opening(path)
            .with_audio_buffer(BUFFER)
            .with_plugins(vec![root], &built().1, cache)
            .with_plugin_faults(faults)
    });
    again.wait_for("the project to open", |app| !app.is_busy());
    for device in PLUGINS {
        wait_for_state(&mut again, device, "ready");
    }
    let opened = again.app().project_as_saved();
    assert_eq!(opened.tracks, saved.tracks);
    assert_eq!(opened.master, saved.master);
    play(&mut again);
    sounds_as_it_should(&mut again, "reopened");

    // The percussion plugin crashes: its output goes quiet; the others play
    // on; Reload brings it back.
    again.app_mut().end_device_host(PERC_PLUGIN);
    wait_for_state(&mut again, PERC_PLUGIN, "failed");
    let kick = loudest(&mut again, KICK, -20.0);
    assert!(kick > -20.0, "the kick plays on: {kick} dB");
    let out = quietest(&mut again, PERC_OUT, -60.0);
    assert!(
        out < -60.0,
        "the crashed plugin's output is silent: {out} dB"
    );
    again.app_mut().reload_device(PERC_PLUGIN);
    wait_for_state(&mut again, PERC_PLUGIN, "ready");
    // A plugin that finishes loading while a note is held does not sound it
    // until the next note (KNOWN_ISSUES): play again from the start.
    again.key(Modifiers::NONE, Key::Space);
    again.wait_for("the stop", |app| !app.is_playing());
    again.key(Modifiers::NONE, Key::Home);
    play(&mut again);
    let out = loudest(&mut again, PERC_OUT, -20.0);
    assert!(out > -20.0, "reloaded, its output plays again: {out} dB");
    let bass = quietest(&mut again, BASS, -60.0);
    assert!(bass < -60.0, "and the bass is still keyed: {bass} dB");
    again.key(Modifiers::NONE, Key::Space);
}
