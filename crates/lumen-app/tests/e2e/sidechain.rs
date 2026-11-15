use std::time::Duration;

use lumen_core::time::Ticks;
use lumen_project::model::{Id, Note, Project, Sidechain, SidechainTap};
use lumen_project::testing::{midi_track, project_of};

use egui_kittest::kittest::Queryable;

use super::Lumen;
use super::hosting::{effect_device, instrument_device, launch, play, wait_for_state};

const KEY: Id = Id(1);
const KEY_PLUGIN: Id = Id(20);
const VOICE: Id = Id(30);
const VOICE_PLUGIN: Id = Id(31);
const EFFECT: Id = Id(40);

fn held(velocity: u8) -> Vec<Note> {
    vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, velocity)]
}

fn song(sidechain: Option<SidechainTap>) -> Project {
    let mut key = midi_track(KEY.0, "Key", "poly", (0, 64), None, held(127), vec![]);
    let mut instrument = instrument_device(KEY_PLUGIN);
    instrument
        .params
        .insert(lumen_test_plugin::params::LEVEL, 1.0);
    key.devices = vec![instrument];
    key.mute = true;
    let mut voice = midi_track(VOICE.0, "Voice", "poly", (0, 64), None, held(100), vec![]);
    let mut effect = effect_device(EFFECT);
    effect.sidechain = sidechain.map(|tap| Sidechain { from: KEY, tap });
    voice.devices = vec![instrument_device(VOICE_PLUGIN), effect];
    project_of("Keyed", vec![key, voice])
}

fn peak(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(150), 7_200)
        .iter()
        .fold(0.0_f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

fn ready(lumen: &mut Lumen) {
    for device in [KEY_PLUGIN, VOICE_PLUGIN, EFFECT] {
        wait_for_state(lumen, device, "ready");
    }
}

fn settled(lumen: &mut Lumen, until: impl Fn(f32) -> bool) -> f32 {
    let mut last = f32::MAX;
    for _ in 0..40 {
        last = peak(lumen);
        if until(last) {
            break;
        }
    }
    last
}

#[test]
fn a_loud_key_silences_the_effect_it_is_keyed_to_and_without_one_it_plays() {
    let mut lumen = launch("keyed", &song(Some(SidechainTap::PostFx)), &["Keyed"]);
    ready(&mut lumen);
    play(&mut lumen);
    let keyed = settled(&mut lumen, |p| p < 0.001);
    assert!(keyed < 0.001, "the effect is silenced by the key: {keyed}");

    let mut lumen = launch("unkeyed", &song(None), &["Keyed"]);
    ready(&mut lumen);
    play(&mut lumen);
    let open = settled(&mut lumen, |p| p > 0.05);
    assert!(open > 0.05, "without a sidechain it plays: {open}");
}

#[test]
fn the_tap_decides_whether_the_muted_key_is_heard_by_the_effect() {
    let mut lumen = launch(
        "keyed-mixer",
        &song(Some(SidechainTap::PostMixer)),
        &["Keyed"],
    );
    ready(&mut lumen);
    play(&mut lumen);
    let open = settled(&mut lumen, |p| p > 0.05);
    assert!(
        open > 0.05,
        "a muted track plays nothing to key with: {open}"
    );

    let mut lumen = launch("keyed-pre", &song(Some(SidechainTap::PreFx)), &["Keyed"]);
    ready(&mut lumen);
    play(&mut lumen);
    let keyed = settled(&mut lumen, |p| p < 0.001);
    assert!(
        keyed < 0.001,
        "pre FX still carries the muted track: {keyed}"
    );
}

use eframe::egui::{Key, Modifiers};
use lumen_project::model::{Color, Track, TrackKind};

const SOURCE: &str = "Sidechain source, none";

fn open(name: &str, project: &Project) -> Lumen {
    let mut lumen = launch(name, project, &["Keyed"]);
    ready(&mut lumen);
    lumen.app_mut().select_tracks(&[VOICE]);
    lumen.show_whole_cards();
    lumen
}

fn keyed_from(lumen: &Lumen) -> Option<Sidechain> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.devices)
        .find(|d| d.id == EFFECT)
        .and_then(|d| d.sidechain)
}

#[test]
fn an_effect_with_a_sidechain_input_has_the_row_and_an_instrument_has_none() {
    let mut lumen = open("row", &song(None));
    lumen.wait_for("the row", |_| true);
    assert!(lumen.shows(SOURCE), "the effect's row");
    assert_eq!(lumen.harness.query_all_by_label(SOURCE).count(), 1);
}

#[test]
fn choosing_a_source_keys_the_effect_and_undo_lets_it_through_again() {
    let mut lumen = open("choose", &song(None));
    play(&mut lumen);
    let open_peak = settled(&mut lumen, |p| p > 0.05);
    assert!(open_peak > 0.05, "unkeyed it plays: {open_peak}");

    lumen.click(SOURCE);
    lumen.frames(5);
    lumen.click("Key");
    lumen.frames(5);
    assert_eq!(
        keyed_from(&lumen),
        Some(Sidechain {
            from: KEY,
            tap: SidechainTap::PostFx
        })
    );
    assert_eq!(
        lumen.app().undo_label(),
        Some("Sidechain Deterministic Test Effect")
    );
    let keyed = settled(&mut lumen, |p| p < 0.001);
    assert!(keyed < 0.001, "the key silences it: {keyed}");
    assert!(
        lumen.shows("Sidechain source, Key, Post FX"),
        "the chooser says which track and where"
    );

    assert!(
        lumen.app().sidechain_light(VOICE) > 0.3,
        "the light shows the key arriving: {}",
        lumen.app().sidechain_light(VOICE)
    );

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert_eq!(keyed_from(&lumen), None);
    let open_again = settled(&mut lumen, |p| p > 0.05);
    assert!(open_again > 0.05, "undone, it plays: {open_again}");
}

#[test]
fn the_tap_is_chosen_in_the_same_menu() {
    let mut lumen = open("tap", &song(Some(SidechainTap::PostFx)));
    play(&mut lumen);
    let keyed = settled(&mut lumen, |p| p < 0.001);
    assert!(
        keyed < 0.001,
        "the muted key still keys at Post FX: {keyed}"
    );
    lumen.click("Sidechain source, Key, Post FX");
    lumen.frames(5);
    lumen.click("Post mixer");
    lumen.frames(5);
    assert_eq!(
        keyed_from(&lumen).map(|s| s.tap),
        Some(SidechainTap::PostMixer)
    );
    let open = settled(&mut lumen, |p| p > 0.05);
    assert!(
        open > 0.05,
        "post mixer of a muted track keys nothing: {open}"
    );
    assert!(lumen.shows("Sidechain source, Key, Post mixer"));
}

#[test]
fn a_track_that_the_voice_feeds_cannot_key_it() {
    let mut project = song(None);
    let group = Id(70);
    let mut bus = Track::new(group, "Bus", TrackKind::Group, Color(0x4F81BD));
    bus.id = group;
    project.tracks.push(std::sync::Arc::new(bus));
    let voice = std::sync::Arc::make_mut(&mut project.tracks[1]);
    voice.parent = Some(group);
    project.tracks.swap(1, 2);
    let mut lumen = open("feedback", &project);
    lumen.click(SOURCE);
    lumen.frames(5);
    lumen.click("Bus");
    lumen.frames(5);
    assert_eq!(
        keyed_from(&lumen),
        None,
        "refused: the group is fed by the voice"
    );
}

#[test]
fn the_command_opens_the_menu_of_the_chosen_card() {
    let mut lumen = open("command", &song(None));
    lumen.click("Deterministic Test Effect on Voice");
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(eframe::egui::Event::Text(
        "Choose the sidechain of the effect".to_owned(),
    ));
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert!(
        lumen.shows("Post mixer"),
        "the menu is open under the chooser"
    );
}

#[test]
fn the_key_does_not_leak_into_the_keyed_track() {
    let mut project = song(Some(SidechainTap::PostFx));
    let voice = project
        .tracks
        .iter_mut()
        .find(|t| t.id == VOICE)
        .expect("the voice");
    for clip in std::sync::Arc::make_mut(voice).clips.iter_mut() {
        std::sync::Arc::make_mut(clip).content = lumen_project::model::ClipContent::midi();
    }
    let mut lumen = launch("leak", &project, &["Keyed"]);
    ready(&mut lumen);
    play(&mut lumen);
    lumen.frames(30);
    let heard = peak(&mut lumen);
    assert!(heard < 0.0001, "nothing of the key is heard: {heard}");
}

#[test]
fn a_key_whose_track_was_deleted_says_so_on_the_row_and_can_be_chosen_again() {
    let mut lumen = open("removed", &song(Some(SidechainTap::PostFx)));
    assert!(lumen.shows("Sidechain source, Key, Post FX"));
    lumen.app_mut().select_tracks(&[KEY]);
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(20);
    assert!(
        lumen.app().project().tracks.iter().all(|t| t.id != KEY),
        "the key track is gone"
    );
    lumen.app_mut().select_tracks(&[VOICE]);
    lumen.frames(20);
    assert!(
        lumen.shows("Sidechain source, removed"),
        "the row says its source is removed"
    );
    lumen.click("Sidechain source, removed");
    lumen.frames(5);
    lumen.click("None");
    lumen.frames(5);
    assert_eq!(keyed_from(&lumen), None);
    assert!(lumen.shows(SOURCE));
}
