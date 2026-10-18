//! An instrument's other outputs on tracks of their own (M6 slice 6d): a
//! track that plays an output of the plugin instrument hears the notes that
//! go to that output and nothing else, on the same samples as the
//! instrument's own track would. The plugins and the host program are the
//! real ones, built by the test.

use std::time::Duration;

use lumen_core::time::Ticks;
use lumen_project::model::{Color, Id, Note, PluginOutput, Project, Track, TrackKind};
use lumen_project::testing::{midi_track, project_of};

use super::Lumen;
use super::hosting::{instrument_device, launch, play, wait_for_state};

const KEYS: Id = Id(1);
const PLUGIN: Id = Id(20);
const KICK: Id = Id(30);
const SNARE: Id = Id(31);

/// The instrument on a MIDI track playing `pitches` together for a long
/// time, and an audio track for each of the instrument's outputs in `tracks`
/// (`(track, bus)`).
fn song(pitches: &[u8], tracks: &[(Id, u32)]) -> Project {
    let notes = pitches
        .iter()
        .map(|&p| Note::new(Ticks::ZERO, Ticks::quarters(60), p, 100))
        .collect();
    let mut keys = midi_track(KEYS.0, "Keys", "poly", (0, 64), None, notes, vec![]);
    let mut device = instrument_device(PLUGIN);
    device.params.insert(lumen_test_plugin::params::LEVEL, 1.0);
    keys.devices = vec![device];
    let mut all = vec![keys];
    for &(id, bus) in tracks {
        let mut track = Track::new(id, format!("Out {bus}"), TrackKind::Audio, Color(0x5FA89A));
        track.plugin_output = Some(PluginOutput {
            device: PLUGIN,
            bus,
        });
        all.push(track);
    }
    project_of("Outputs", all)
}

/// How loud each track plays, after the plugin has been set up for its
/// outputs: the loudest of a few windows.
fn loudest(lumen: &mut Lumen, track: Id) -> f32 {
    let mut best = f32::MIN;
    for _ in 0..30 {
        lumen.listen(Duration::from_millis(100), 4_800);
        best = best.max(lumen.app().meter(track).peak_db);
        if best > -20.0 {
            break;
        }
    }
    best
}

fn start(name: &str, project: &Project) -> Lumen {
    let mut lumen = launch(name, project, &["Multi"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    lumen
}

#[test]
fn a_track_plays_the_output_its_notes_go_to_and_nothing_else() {
    // Pitch 61 goes to output 1 (61 mod 3), which the kick track plays; the
    // keys and the snare track hear nothing.
    let mut lumen = start("kick", &song(&[61], &[(KICK, 1), (SNARE, 2)]));
    let kick = loudest(&mut lumen, KICK);
    assert!(kick > -20.0, "the output's own track plays it: {kick} dB");
    let snare = lumen.app().meter(SNARE).peak_db;
    let keys = lumen.app().meter(KEYS).peak_db;
    assert!(snare < -60.0, "the other output is silent: {snare} dB");
    assert!(keys < -60.0, "so is the instrument's own track: {keys} dB");
}

#[test]
fn an_output_with_no_track_plays_on_the_instruments_own() {
    // Pitch 62 goes to output 2, which has no track: the keys play it.
    let mut lumen = start("main", &song(&[62], &[(KICK, 1)]));
    let keys = loudest(&mut lumen, KEYS);
    assert!(keys > -20.0, "the main output has it: {keys} dB");
    let kick = lumen.app().meter(KICK).peak_db;
    assert!(
        kick < -60.0,
        "the track of another output is silent: {kick} dB"
    );
}
