//! A plugin effect keyed from another track (M6 slice 6c): the project's
//! sidechain turns the plugin's sidechain input on, the source's sound
//! reaches it sample for sample, and taking the sidechain away lets the
//! effect through again. The plugins and the host program are the real ones,
//! built by the test.

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

/// A muted track whose instrument plays a full-scale square, and a track
/// whose instrument plays through the sidechain effect. With a sidechain at
/// `tap`, the effect is silenced wherever the first track is loud.
fn song(sidechain: Option<SidechainTap>) -> Project {
    let mut key = midi_track(KEY.0, "Key", "poly", (0, 64), None, held(127), vec![]);
    let mut instrument = instrument_device(KEY_PLUGIN);
    // The plugin's level at full: a square of height 1.
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

/// The quietest of several windows, once the plugin has been set up again
/// for its sidechain (it plays unkeyed until then).
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

    // The same song with no sidechain: the effect lets its track through.
    let mut lumen = launch("unkeyed", &song(None), &["Keyed"]);
    ready(&mut lumen);
    play(&mut lumen);
    let open = settled(&mut lumen, |p| p > 0.05);
    assert!(open > 0.05, "without a sidechain it plays: {open}");
}

#[test]
fn the_tap_decides_whether_the_muted_key_is_heard_by_the_effect() {
    // The key track is muted. Its mute comes after its devices, so the tap
    // before and after them still carries the sound; what the track plays
    // (post mixer) is silence, which keys nothing.
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

// The sidechain row on the card (the owner's B with the light of I1).

use eframe::egui::{Key, Modifiers};
use lumen_project::model::{Color, Track, TrackKind};

const SOURCE: &str = "Sidechain source, none";

/// The song with the voice track chosen and its Devices tab showing.
fn open(name: &str, project: &Project) -> Lumen {
    let mut lumen = launch(name, project, &["Keyed"]);
    ready(&mut lumen);
    lumen.app_mut().select_tracks(&[VOICE]);
    lumen
        .harness
        .get_all_by_label("Devices tab")
        .last()
        .expect("the detail panel's tabs")
        .click();
    lumen.frames(20);
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
    // The instrument beside it has no sidechain input.
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
    // The menu lists the other tracks by name; the voice's own track is not
    // offered.
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

    // The light follows what arrives.
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
    // What a muted track plays is nothing, so nothing keys the effect.
    let open = settled(&mut lumen, |p| p > 0.05);
    assert!(
        open > 0.05,
        "post mixer of a muted track keys nothing: {open}"
    );
    assert!(lumen.shows("Sidechain source, Key, Post mixer"));
}

#[test]
fn a_track_that_the_voice_feeds_cannot_key_it() {
    // The voice plays into a group; the group cannot key the voice's effect,
    // or the sound would go round.
    let mut project = song(None);
    let group = Id(70);
    let mut bus = Track::new(group, "Bus", TrackKind::Group, Color(0x4F81BD));
    bus.id = group;
    project.tracks.push(std::sync::Arc::new(bus));
    let voice = std::sync::Arc::make_mut(&mut project.tracks[1]);
    voice.parent = Some(group);
    // A group's tracks follow it in the list.
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
