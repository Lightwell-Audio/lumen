use std::time::Duration;

use eframe::egui::{Key, Modifiers};

use lumen_core::time::Ticks;
use lumen_project::model::{Color, Id, Note, PluginOutput, Project, Track, TrackKind};
use lumen_project::testing::{midi_track, project_of};

use super::Lumen;
use super::hosting::{instrument_device, launch, play, wait_for_state};

const KEYS: Id = Id(1);
const PLUGIN: Id = Id(20);
const KICK: Id = Id(30);
const SNARE: Id = Id(31);

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
    let mut lumen = start("main", &song(&[62], &[(KICK, 1)]));
    let keys = loudest(&mut lumen, KEYS);
    assert!(keys > -20.0, "the main output has it: {keys} dB");
    let kick = lumen.app().meter(KICK).peak_db;
    assert!(
        kick < -60.0,
        "the track of another output is silent: {kick} dB"
    );
}

#[test]
fn the_outputs_menu_keeps_its_width_when_an_output_is_switched_on() {
    use egui_kittest::kittest::{NodeT, Queryable};

    let mut lumen = launch("menu", &song(&[61], &[]), &["Multi"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen.app_mut().select_tracks(&[KEYS]);
    lumen.show_whole_cards();

    let switches = |lumen: &Lumen| -> Vec<(String, f32)> {
        let Some(caption) = lumen.harness.query_by_label_contains("Outputs of") else {
            return Vec::new();
        };
        let below = caption.rect().bottom();
        lumen
            .harness
            .query_all_by_role(eframe::egui::accesskit::Role::CheckBox)
            .filter(|n| n.rect().top() >= below)
            .map(|n| {
                (
                    n.accesskit_node().label().unwrap_or_default(),
                    n.rect().width(),
                )
            })
            .collect()
    };
    let widest = |lumen: &Lumen| switches(lumen).iter().fold(0.0_f32, |m, (_, w)| m.max(*w));
    lumen.click("Outputs, Main only");
    lumen.frames(10);
    let before = widest(&lumen);
    assert!(before > 100.0, "the menu is open: {before}");
    let first = switches(&lumen)[0].0.clone();
    lumen.click(&first);
    lumen.frames(60);
    assert_eq!(
        lumen
            .app()
            .project()
            .tracks
            .iter()
            .filter(|t| t.plugin_output.is_some())
            .count(),
        1,
        "switching an output on makes its track"
    );
    let after = widest(&lumen);
    assert!(
        (after - before).abs() < 1.0,
        "the menu stays {before} wide, it is {after}"
    );
}

#[test]
fn an_output_without_a_track_is_heard_on_the_instruments_own_not_on_another_output() {
    let mut lumen = start("fold", &song(&[61], &[(SNARE, 2)]));
    let keys = loudest(&mut lumen, KEYS);
    assert!(
        keys > -20.0,
        "output 1 plays on the instrument's own: {keys} dB"
    );
    let snare = lumen.app().meter(SNARE).peak_db;
    assert!(snare < -60.0, "the next output is silent: {snare} dB");
}

fn open_card(name: &str, project: &Project) -> Lumen {
    let mut lumen = launch(name, project, &["Multi"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen.app_mut().select_tracks(&[KEYS]);
    lumen.show_whole_cards();
    lumen
}

fn menu_switches(lumen: &Lumen) -> Vec<String> {
    use egui_kittest::kittest::{NodeT, Queryable};
    let Some(caption) = lumen.harness.query_by_label_contains("Outputs of") else {
        return Vec::new();
    };
    let below = caption.rect().bottom();
    lumen
        .harness
        .query_all_by_role(eframe::egui::accesskit::Role::CheckBox)
        .filter(|n| n.rect().top() >= below)
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect()
}

fn output_tracks(lumen: &Lumen) -> Vec<(Id, u32)> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .filter_map(|t| t.plugin_output.map(|o| (t.id, o.bus)))
        .collect()
}

#[test]
fn a_switch_makes_the_output_a_track_in_one_undo_step_and_the_row_says_how_many() {
    let mut lumen = open_card("row", &song(&[61], &[]));
    assert!(lumen.shows("Outputs, Main only"), "the card's row");
    lumen.click("Outputs, Main only");
    lumen.frames(10);
    let names = menu_switches(&lumen);
    assert!(
        names.len() >= 2,
        "the plugin's other outputs are listed: {names:?}"
    );

    lumen.click(&names[0]);
    lumen.frames(10);
    assert_eq!(output_tracks(&lumen).len(), 1, "a track for the output");
    assert!(lumen.shows("Outputs, Main + 1"), "the row counts it");
    let after = lumen.app().project().tracks.len();

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(10);
    assert!(output_tracks(&lumen).is_empty(), "one undo takes it away");
    assert_eq!(lumen.app().project().tracks.len(), after - 1);
    assert!(lumen.shows("Outputs, Main only"));
}

#[test]
fn switching_off_an_output_whose_track_holds_work_asks_and_remove_takes_it_away() {
    let mut project = song(&[61], &[(KICK, 1)]);
    let held = project
        .tracks
        .iter_mut()
        .find(|t| t.id == KICK)
        .map(std::sync::Arc::make_mut)
        .expect("the kick track");
    held.devices = vec![super::hosting::effect_device(Id(60))];
    let mut lumen = open_card("ask", &project);
    lumen.click("Outputs, Main + 1");
    lumen.frames(10);
    let names = menu_switches(&lumen);
    lumen.click(&names[0]);
    lumen.frames(10);
    assert_eq!(output_tracks(&lumen).len(), 1, "nothing is removed yet");
    assert!(lumen.shows("Remove track"), "it asks");

    lumen.click("Remove track");
    lumen.frames(10);
    assert!(output_tracks(&lumen).is_empty(), "removed once answered");
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(10);
    assert_eq!(output_tracks(&lumen).len(), 1, "undo brings it back");
}

#[test]
fn keeping_the_track_leaves_it_with_no_input() {
    let mut project = song(&[61], &[(KICK, 1)]);
    let held = project
        .tracks
        .iter_mut()
        .find(|t| t.id == KICK)
        .map(std::sync::Arc::make_mut)
        .expect("the kick track");
    held.devices = vec![super::hosting::effect_device(Id(60))];
    let mut lumen = open_card("keep", &project);
    lumen.click("Outputs, Main + 1");
    lumen.frames(10);
    let names = menu_switches(&lumen);
    lumen.click(&names[0]);
    lumen.frames(10);
    lumen.click("Keep track");
    lumen.frames(10);
    assert!(output_tracks(&lumen).is_empty(), "it plays no output now");
    assert!(
        lumen.app().project().tracks.iter().any(|t| t.id == KICK),
        "but the track stays"
    );
}
