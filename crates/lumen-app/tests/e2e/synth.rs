//! Flare, the built-in polysynth (M8 slice B4), in the app: its card and its
//! parameters, a value that changes the sound, a lane on the cutoff, and both
//! of them back after a save and a reopen. The synth's own sound is tested in
//! `lumen-devices`; what is here is the path from the interface and the
//! project to the audio thread.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;
use lumen_devices::synth::params;
use lumen_project::automation::Target;
use lumen_project::model::{Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::lane_view::{peak, run_command};

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

/// A MIDI track named "Keys" with Flare on Poly and one long note.
fn one_note() -> Project {
    project_of(
        "One note",
        vec![midi_track(
            1,
            "Keys",
            "poly",
            (0, 64),
            None,
            vec![Note::new(q(0), q(16), 48, 110)],
            vec![],
        )],
    )
}

fn launched(name: &str) -> (Lumen, Id, Id) {
    let dir = TempDir::new(&format!("e2e-synth-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&one_note(), &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.frames(30);
    let first = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[first]);
    lumen.show_whole_cards();
    let track = lumen.app().project().tracks[0].id;
    let device = lumen.app().project().tracks[0].devices[0].id;
    (lumen, track, device)
}

#[track_caller]
fn at(lumen: &Lumen, label: &str) -> Pos2 {
    // The card is drawn above the panel, so a control that appears in both
    // (the cutoff's knob and its row in All parameters) is the card's first.
    lumen
        .harness
        .get_all_by_label_contains(label)
        .next()
        .unwrap_or_else(|| panic!("no control labelled {label:?}"))
        .rect()
        .center()
}

fn param(lumen: &Lumen, device: Id, id: u32) -> Option<f64> {
    lumen
        .app()
        .device_params(device)
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, value)| *value)
}

/// The loudest frame of what plays now.
fn heard(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(150), 2_000)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()))
}

/// Puts the cutoff on the card's strip: thirty-four parameters do not fit
/// eight knobs, so one comes off first, the way a person chooses them (All
/// parameters, searched, with its star).
fn cutoff_on_the_strip(lumen: &mut Lumen) {
    lumen.click("All parameters (34)");
    lumen.frames(10);
    // The search field takes the keyboard when the panel opens.
    lumen.harness.event(Event::Text("detune".to_owned()));
    lumen.frames(10);
    lumen.click("Take Oscillator 2 detune off the strip");
    lumen.frames(5);
    lumen.click("Search parameters");
    lumen.frames(2);
    for _ in 0.."detune".len() {
        lumen.key(Modifiers::NONE, Key::Backspace);
    }
    lumen.frames(5);
    lumen.harness.event(Event::Text("cutoff".to_owned()));
    lumen.frames(10);
    lumen.click("Put Filter cutoff on the strip");
    lumen.frames(5);
}

#[test]
fn a_midi_track_s_synth_is_flare_with_its_own_knobs_and_words() {
    let (lumen, _, device) = launched("card");
    assert!(lumen.shows("Flare on Keys"), "the card's spoken name");
    // The table is the device's: 34 parameters, the first eight as its knobs.
    assert_eq!(
        lumen.app().strip_parameters(device),
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert!(
        lumen
            .harness
            .query_by_label("All parameters (34)")
            .is_some(),
        "every parameter is one button away"
    );
    // A value reads in the device's own words: a knob under its name, a
    // chooser as its own word.
    assert!(
        lumen.shows("Oscillator 1 detune, parameter: \u{2212}7 ct"),
        "the knob's own words"
    );
    assert!(lumen.shows("Oscillator 1 level, parameter: 100 %"));
    assert!(lumen.shows("Oscillator 1 pan, parameter: L35"));
    assert!(
        lumen.shows("Oscillator 1 shape, choice: Saw"),
        "the shape reads as its word"
    );
    // The round-2 labels are the description's data, and the plain names are
    // the table's (a drawn face reads them; the card shows the plain ones):
    // Shape, Cutoff, Reso, Attack and Release are the five that map.
    assert_eq!(params::short_label(params::OSC1_SHAPE), "Shape");
    assert_eq!(params::short_label(params::FILTER_CUTOFF), "Cutoff");
    assert_eq!(params::short_label(params::FILTER_RESO), "Reso");
    assert_eq!(params::short_label(params::AMP_ATTACK), "Attack");
    assert_eq!(params::short_label(params::AMP_RELEASE), "Release");
    assert_eq!(
        params::find(params::FILTER_CUTOFF).unwrap().name,
        "Filter cutoff"
    );
}

#[test]
fn closing_the_cutoff_makes_the_note_darker_and_undo_opens_it_again() {
    let (mut lumen, _, device) = launched("cutoff");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    lumen.wait_for("the note", |app| app.playhead() > q(1));
    let open = heard(&mut lumen);
    assert!(open > 0.01, "the note is heard: {open}");

    // The cutoff's knob is dragged down: the corner closes and the note gets
    // quieter and darker.
    cutoff_on_the_strip(&mut lumen);
    assert_eq!(
        lumen.app().strip_parameters(device)[7],
        params::FILTER_CUTOFF
    );
    let knob = at(&lumen, "Filter cutoff, parameter");
    lumen.drag(knob, knob + vec2(0.0, 90.0), Modifiers::NONE);
    lumen.frames(20);
    let closed = param(&lumen, device, params::FILTER_CUTOFF).expect("the cutoff moved");
    assert!(closed < 0.3, "dragged down: {closed}");
    let dark = heard(&mut lumen);
    assert!(dark < open * 0.8, "the sound is darker: {open} then {dark}");
    assert_eq!(lumen.app().undo_label(), Some("Change Filter cutoff"));

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(10);
    let back = param(&lumen, device, params::FILTER_CUTOFF).expect("the cutoff");
    assert!((back - 0.492_032_53).abs() < 1e-6, "one undo step: {back}");
}

#[test]
fn a_lane_on_the_cutoff_is_its_own_lane_and_plays() {
    let (mut lumen, track, device) = launched("lane");
    let target = Target::Device {
        device,
        param: params::FILTER_CUTOFF,
    };
    // A lane from Poly's corner down to a closed one over two quarters.
    let lane = Lane::new(vec![
        Point::new(Ticks::ZERO, 0.492_032_53),
        Point::new(q(2), 0.05),
    ])
    .unwrap();
    lumen.app_mut().give_lane(track, target, lane).unwrap();
    lumen.frames(20);
    // Its knob's menu shows the lane, named for the device and the parameter,
    // and opening it draws the lane.
    cutoff_on_the_strip(&mut lumen);
    lumen.right_click(at(&lumen, "Filter cutoff, parameter"));
    lumen.click("Automate");
    lumen.frames(30);
    assert!(
        lumen
            .harness
            .query_by_label_contains("Automation lane of Keys: Flare")
            .is_some(),
        "the lane names Flare and its parameter"
    );
    // It plays: the note starts open and ends shut.
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::ZERO);
    let early = peak(&mut lumen);
    lumen.wait_for("the lane's end", |app| app.playhead() > q(2));
    lumen.frames(10);
    let late = peak(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Space);
    assert!(
        early > 0.01 && late < early * 0.7,
        "the lane closes the corner: {early} then {late}"
    );
}

#[test]
fn the_values_and_the_lane_are_back_after_a_save_and_a_reopen() {
    let (mut lumen, track, device) = launched("save");
    cutoff_on_the_strip(&mut lumen);
    let knob = at(&lumen, "Filter cutoff, parameter");
    lumen.drag(knob, knob + vec2(0.0, 60.0), Modifiers::NONE);
    lumen.frames(10);
    let changed = param(&lumen, device, params::FILTER_CUTOFF).expect("the cutoff moved");
    assert!(changed < 0.45, "dragged down: {changed}");
    // A lane on the resonance too, so both a value and a lane are saved.
    let target = Target::Device {
        device,
        param: params::FILTER_RESO,
    };
    let lane = Lane::new(vec![Point::new(Ticks::ZERO, 0.25), Point::new(q(4), 0.9)]).unwrap();
    lumen.app_mut().give_lane(track, target, lane).unwrap();
    lumen.frames(20);

    let path = lumen_app::testing::project_file(lumen.dir(), "Flare song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());

    let dir: TempDir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(20);
    let reopened = again.app().project().tracks[0].devices[0].id;
    let back = param(&again, reopened, params::FILTER_CUTOFF).expect("the cutoff is saved");
    assert!(
        (back - changed).abs() < 1e-9,
        "the value is back exactly: {changed} then {back}"
    );
    assert_eq!(
        again.app().project().tracks[0].devices[0].preset(),
        Some("poly"),
        "the preset's name stays as the record of where the sound started"
    );
    // The lane is back, aimed at the synth's resonance.
    let lanes = &again.app().project().tracks[0].automation;
    assert_eq!(lanes.len(), 1, "one lane, saved");
    assert_eq!(
        lanes[0].target,
        Target::Device {
            device: reopened,
            param: params::FILTER_RESO,
        }
    );
    again.key(Modifiers::NONE, Key::Space);
    again.wait_for("playback", |app| app.is_playing());
    assert!(heard(&mut again) > 0.0, "it plays after the reopen");
    again.key(Modifiers::NONE, Key::Space);
}

#[test]
fn the_palette_chooses_a_preset_and_the_values_follow_it() {
    let (mut lumen, _, device) = launched("preset");
    run_command(&mut lumen, "Instrument: Pad");
    lumen.frames(20);
    for (id, value) in params::preset_values(lumen_devices::Preset::Pad) {
        let saved = param(&lumen, device, id).expect("every parameter is saved");
        assert!(
            (saved - f64::from(value)).abs() < 1e-6,
            "parameter {id} is Pad's: {saved} against {value}"
        );
    }
    assert_eq!(
        lumen.app().project().tracks[0].devices[0].preset(),
        Some("pad")
    );
    assert_eq!(lumen.app().undo_label(), Some("Instrument: Pad"));
}
