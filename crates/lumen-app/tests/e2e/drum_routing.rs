//! Per-sound routing (M8 slice D4): a drum sound leaving its kit track — out
//! of one of its instrument's output pairs, on a track made for it, or to
//! another instrument track — and never leaving a note sounding when its
//! route changes mid-note.
//!
//! The slice has no interface of its own yet (the Output column and the
//! header's sound icons come with the UI agent), so these drive it as the
//! column and the palette will: the two commands through the palette, and
//! the per-sound route through the seam the column calls.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::drums::presets::Preset;
use lumen_project::model::{Color, Device, Id, Note, Project, Route, Track, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::hosting::{instrument_device, launch, wait_for_state};
use super::midi_play::palette;

const KIT: Id = Id(1);
const PLUGIN: Id = Id(20);
const SUB: Id = Id(30);
/// The 808-style map the kit tracks read, in the saved project.
const MAP: Id = Id(500);

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

fn note(start: i64, length: i64, pitch: u8) -> Note {
    Note::new(q(start), q(length), pitch, 100)
}

/// The kit track's map, its sounds on General MIDI's 808 notes (36 kick, 38
/// snare, 42 closed hat), ready to be given routes.
fn map() -> lumen_project::model::DrumMap {
    Preset::EIGHT_OH_EIGHT.map(MAP)
}

/// Starts Lumen with `project` open, playing the built-in synth only.
fn start(name: &str, project: &Project) -> Lumen {
    let dir = TempDir::new(&format!("e2e-routing-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("save the test project");
    let tracks = project.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    lumen.frames(30);
    lumen
}

/// A kit track with the built-in synth, the 808-style map and a kick on
/// every beat, and a second MIDI track with no clips: where a sound is sent.
fn kit_and_sub() -> Project {
    let beats: Vec<Note> = (0..16).map(|beat| note(beat, 1, 36)).collect();
    let kit = midi_track(KIT.0, "Drums", "pluck", (0, 16), None, beats, vec![]);
    let mut sub = Track::new(SUB, "Sub", TrackKind::Midi, Color(0x9BBB59));
    sub.devices = vec![Device::synth(Id(SUB.0 + 1), "bass")];
    let mut project = project_of("Routing", vec![kit, sub]);
    let map = map();
    project.drum_maps = vec![map.clone()];
    let slot = std::sync::Arc::make_mut(&mut project.tracks[0]);
    slot.drum_map = Some(map.id);
    project
}

/// Plays, and gives the level the device reached.
fn play_and_hear(lumen: &mut Lumen, loud: bool, what: &str) -> f32 {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    let heard = loudest(lumen);
    if loud {
        until_loud(lumen, 0.01, what)
    } else {
        until_quiet(lumen, 1e-4, what);
        heard
    }
}

fn stop(lumen: &mut Lumen) {
    if lumen.app().is_playing() {
        lumen.key(Modifiers::NONE, Key::Space);
        lumen.wait_for("stop", |app| !app.is_playing());
    }
}

/// The loudest sample heard over a stretch, after letting it settle.
fn loudest(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(100), 2_000)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

/// Waits until what the device plays is over `loud`.
#[track_caller]
fn until_loud(lumen: &mut Lumen, loud: f32, what: &str) -> f32 {
    let deadline = lumen_testkit::Deadline::new();
    loop {
        let heard = loudest(lumen);
        if heard > loud {
            return heard;
        }
        deadline.check(format_args!("{what}; only {heard} heard"));
        std::thread::sleep(lumen_testkit::POLL);
    }
}

/// Waits until what the device plays is under `quiet`.
#[track_caller]
fn until_quiet(lumen: &mut Lumen, quiet: f32, what: &str) {
    let deadline = lumen_testkit::Deadline::new();
    loop {
        let heard = loudest(lumen);
        if heard < quiet {
            return;
        }
        deadline.check(format_args!("{what}; still heard at {heard}"));
        std::thread::sleep(lumen_testkit::POLL);
    }
}

fn names(lumen: &Lumen) -> Vec<String> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .map(|t| t.name.clone())
        .collect()
}

/// Where the sound at input note `input` of the saved map plays now.
fn route(app: &lumen_app::App, input: u8) -> Route {
    app.project()
        .drum_maps
        .iter()
        .find(|m| m.id == MAP)
        .and_then(|m| m.sound(input))
        .map(|s| s.route)
        .expect("the map's sound")
}

fn track_named(lumen: &Lumen, name: &str) -> Option<Id> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.id)
}

#[test]
fn a_sound_sent_to_another_track_plays_that_tracks_instrument_and_moving_it_back_hangs_nothing() {
    let mut lumen = start("send", &kit_and_sub());
    lumen.app_mut().select_tracks(&[KIT]);
    lumen.frames(3);

    // The kick is the kit's: heard with the kit alone.
    play_and_hear(&mut lumen, true, "the kit's kick");
    stop(&mut lumen);

    // Sent to the Sub track's instrument (the seam the Output column calls,
    // one undo step). With the kit muted, the kick is still heard: it plays
    // on Sub now, and only there.
    let map = MAP;
    assert!(
        lumen
            .app_mut()
            .route_sound(KIT, map, 36, &format!("track {}", SUB.0)),
        "an instrument track takes the sound"
    );
    assert_eq!(lumen.app().undo_label(), Some("Route the drum sound"));
    lumen.click("Mute Drums");
    lumen.frames(3);
    play_and_hear(&mut lumen, true, "the kick on the Sub track");
    // Muting Sub too leaves it silent: it is not hiding on the kit as well.
    lumen.click("Mute Sub");
    until_quiet(&mut lumen, 1e-4, "the kick muted with Sub");
    lumen.click("Mute Sub");
    until_loud(&mut lumen, 0.01, "Sub unmuted");

    // Moved back to the kit while a note sounds: Sub is now silent and the
    // kit hears it, so the note followed its route and did not stay behind.
    assert!(lumen.app_mut().route_sound(KIT, map, 36, "main"));
    lumen.click("Mute Sub");
    lumen.click("Mute Drums");
    until_loud(&mut lumen, 0.01, "the kick back on the kit");

    // And nothing is left sounding: the device falls silent on Stop with no
    // note held by either instrument (a hanging note would ring on).
    stop(&mut lumen);
    until_quiet(&mut lumen, 1e-4, "the devices to fall silent");
    assert_eq!(route(lumen.app(), 36), Route::Main, "it is the kit's again");
    assert_eq!(lumen_project::validate(lumen.app().project()), Ok(()));
}

#[test]
fn the_palette_routes_a_kits_sounds_to_the_tracks_of_its_pairs() {
    // A kit whose instrument has three outputs, with sound notes that the
    // test instrument sends to its first two (a note goes to the output its
    // pitch picks: 61 to 1, 62 to 2).
    let mut project = kit_and_sub();
    let mut kit = midi_track(
        KIT.0,
        "Drums",
        "poly",
        (0, 16),
        None,
        vec![note(0, 16, 36), note(0, 16, 37)],
        vec![],
    );
    kit.devices = vec![instrument_device(PLUGIN)];
    let map = {
        let mut map = map();
        for sound in &mut map.sounds {
            match sound.input {
                36 => sound.output = 61,
                37 => sound.output = 62,
                _ => {}
            }
        }
        map
    };
    kit.drum_map = Some(map.id);
    project.drum_maps = vec![map.clone()];
    project.tracks[0] = std::sync::Arc::new(kit);
    let mut lumen = launch("pairs", &project, &["Multi"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");

    lumen.app_mut().select_tracks(&[KIT]);
    lumen.frames(3);
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(
        lumen
            .app()
            .command_reason(&ctx, "Route all sounds to tracks"),
        None,
        "the instrument has pairs to give"
    );
    let before = names(&lumen);
    palette(&mut lumen, "Route all sounds to tracks");
    lumen.frames(5);
    assert_eq!(
        lumen.app().undo_label(),
        Some("Route the sounds to tracks"),
        "one undo step"
    );
    // A track for each of the instrument's pairs, named after its sound, in
    // a group named after the map, with the kit.
    let after = names(&lumen);
    assert_eq!(
        after.len(),
        before.len() + 3,
        "a track for each of the two pairs, and the group: {after:?}"
    );
    let kick = track_named(&lumen, "Kick").expect("a track for the kick");
    let rim = track_named(&lumen, "Rim").expect("a track for the rim");
    let group = lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Group)
        .map(|t| t.id)
        .expect("the sounds' tracks are grouped");
    for id in [kick, rim] {
        assert!(
            lumen
                .app()
                .project()
                .tracks
                .iter()
                .any(|t| t.id == id && t.parent == Some(group)),
            "the kit and its tracks are in the group"
        );
    }
    assert_eq!(
        lumen
            .app()
            .project()
            .tracks
            .iter()
            .find(|t| t.id == group)
            .map(|t| t.name.as_str()),
        Some("808 style")
    );

    // Each pair's track hears only its own sound: the instrument sends the
    // kick's note to output 1 and the rim's to output 2.
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    let deadline = lumen_testkit::Deadline::new();
    loop {
        let heard = lumen.listen(Duration::from_millis(100), 2_400);
        let _ = heard;
        if lumen.app().meter(kick).peak_db > -20.0 && lumen.app().meter(rim).peak_db > -20.0 {
            break;
        }
        deadline.check("both of the kit's pairs to play");
        std::thread::sleep(lumen_testkit::POLL);
    }
    assert!(
        lumen.app().meter(KIT).peak_db < -60.0,
        "the kit's own output plays neither: {} dB",
        lumen.app().meter(KIT).peak_db
    );
    stop(&mut lumen);

    // The palette sends them back: one step, the made tracks gone with it.
    lumen.app_mut().select_tracks(&[KIT]);
    palette(&mut lumen, "Send the sounds back to the kit track");
    lumen.frames(5);
    assert_eq!(
        lumen.app().undo_label(),
        Some("Send the sounds to the kit track")
    );
    assert_eq!(names(&lumen), before, "the tracks the pairs made are gone");
    assert_eq!(lumen_project::validate(lumen.app().project()), Ok(()));
    // Undo brings the whole routing back in one step.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(names(&lumen).len(), after.len());
}

#[test]
fn a_built_in_instrument_says_why_its_sounds_cannot_be_routed_to_pairs() {
    let mut lumen = start("no-pairs", &kit_and_sub());
    lumen.app_mut().select_tracks(&[Id(9_999)]);
    lumen.frames(3);
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(
        lumen
            .app()
            .command_reason(&ctx, "Route all sounds to tracks"),
        Some("Select a MIDI track with a drum map first")
    );
    lumen.app_mut().select_tracks(&[KIT]);
    lumen.frames(3);
    assert_eq!(
        lumen
            .app()
            .command_reason(&ctx, "Route all sounds to tracks"),
        Some("The instrument on this track has no separate outputs to route sounds to"),
        "the synth has one output"
    );
    // Nothing changed, and no step was taken.
    assert_eq!(lumen.app().undo_label(), None);
    palette(&mut lumen, "Route all sounds to tracks");
    lumen.frames(5);
    assert_eq!(lumen.app().undo_label(), None);
    assert_eq!(names(&lumen), ["Drums", "Sub"]);
}
