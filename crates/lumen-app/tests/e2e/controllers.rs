//! MIDI learn (M7 slice A5): a control waits for a controller, the next one
//! moved is mapped to it, and from then on the controller moves the control as
//! a hand on it would: one undo step per sweep, taking its lane over, and
//! written into the lane by a track in Touch. The mapping is saved with the
//! project. The scenarios send controller messages through a fake port.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::automation::Target;
use lumen_project::model::Id;
use lumen_project::testing::TempDir;

use super::lane_view::{points, run_command};
use super::midi_from::switch_on_for_tracks;
use super::{Lumen, wav};

/// A driver stamp that keeps moving on, in microseconds.
static STAMP: AtomicU64 = AtomicU64::new(1_000);

/// A project with one track holding a tone, selected, and a controller "Pad"
/// that is switched on.
fn launch(name: &str) -> (Lumen, FakeControl, Id) {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Pad");
    let dir = TempDir::new(&format!("e2e-{name}"));
    let with = fake.clone();
    let mut lumen = Lumen::start_with(dir, move |l| l.with_midi(with));
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let track = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[track]);
    lumen.frames(40);
    switch_on_for_tracks(&mut lumen, &fake, "Pad");
    (lumen, fake, track)
}

/// Sends controller `number` on `channel` (0 to 15) at `value`.
pub(super) fn cc(fake: &FakeControl, channel: u8, number: u8, value: u8) {
    let stamp = STAMP.fetch_add(5_000, Ordering::Relaxed);
    assert!(fake.send_to_input("Pad", stamp, &[0xB0 | channel, number, value]));
}

fn gain(lumen: &Lumen) -> f32 {
    lumen.app().project().tracks[0].gain_db
}

/// Shows the volume lane and waits for a controller to map to it.
fn learn_volume(lumen: &mut Lumen, fake: &FakeControl, track: Id, channel: u8, number: u8) {
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(5);
    run_command(lumen, "Learn a controller for the shown lane");
    assert_eq!(
        lumen.app().controller_learning(),
        Some((track, Target::Volume)),
        "the shown lane's control waits"
    );
    cc(fake, channel, number, 100);
    lumen.wait_for("the mapping", |app| app.project().mappings.len() == 1);
    assert_eq!(lumen.app().controller_learning(), None);
}

/// Lets a controller's hold run out, so its moves are sealed into one step.
pub(super) fn let_go(lumen: &mut Lumen) {
    for _ in 0..8 {
        lumen.frames(2);
        std::thread::sleep(Duration::from_millis(60));
    }
}

#[test]
fn learning_maps_the_next_controller_without_moving_the_control() {
    let (mut lumen, fake, track) = launch("learn-maps");
    let before = gain(&lumen);
    learn_volume(&mut lumen, &fake, track, 2, 7);
    let m = lumen.app().project().mappings[0];
    assert_eq!((m.channel, m.number, m.track), (2, 7, track));
    assert_eq!(m.target, Target::Volume);
    assert_eq!(
        gain(&lumen),
        before,
        "the message that taught it is not a move"
    );
}

#[test]
fn a_mapped_controller_moves_the_control_and_a_sweep_is_one_undo_step() {
    let (mut lumen, fake, track) = launch("learn-moves");
    learn_volume(&mut lumen, &fake, track, 0, 7);
    assert_eq!(gain(&lumen), 0.0);
    for value in [10_u8, 40, 70, 100, 127] {
        cc(&fake, 0, 7, value);
        std::thread::sleep(Duration::from_millis(20));
        lumen.frames(2);
    }
    lumen.wait_for("the fader at the top", |app| {
        (app.project().tracks[0].gain_db - 6.0).abs() < 0.01
    });
    cc(&fake, 0, 7, 0);
    lumen.wait_for("the fader at the bottom", |app| {
        app.project().tracks[0].gain_db < -50.0
    });
    // Another controller number, or the same on another channel, is not it.
    cc(&fake, 0, 8, 127);
    cc(&fake, 1, 7, 127);
    lumen.frames(10);
    assert!(gain(&lumen) < -50.0);
    let_go(&mut lumen);
    // The whole sweep is one step: one undo returns to where it began.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(gain(&lumen), 0.0);
    assert_eq!(
        lumen.app().project().mappings.len(),
        1,
        "the mapping was a step of its own"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(lumen.app().project().mappings.is_empty());
}

#[test]
fn clearing_a_mapping_gives_the_controller_back() {
    let (mut lumen, fake, track) = launch("learn-clear");
    learn_volume(&mut lumen, &fake, track, 0, 7);
    run_command(&mut lumen, "Clear the controller mapping of the shown lane");
    assert!(lumen.app().project().mappings.is_empty());
    cc(&fake, 0, 7, 127);
    lumen.frames(10);
    std::thread::sleep(Duration::from_millis(100));
    lumen.frames(10);
    assert_eq!(gain(&lumen), 0.0, "nothing moves it now");
}

#[test]
fn escape_stops_waiting_for_a_controller() {
    let (mut lumen, fake, track) = launch("learn-escape");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(5);
    run_command(&mut lumen, "Learn a controller for the shown lane");
    assert!(lumen.app().controller_learning().is_some());
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert_eq!(lumen.app().controller_learning(), None);
    cc(&fake, 0, 7, 100);
    lumen.frames(10);
    assert!(lumen.app().project().mappings.is_empty());
    let _ = track;
}

#[test]
fn a_mapping_is_saved_with_the_project_and_comes_back() {
    let (mut lumen, fake, track) = launch("learn-saved");
    learn_volume(&mut lumen, &fake, track, 5, 74);
    let path = lumen.dir().join("mapped.lumen");
    lumen_project::save(lumen.app().project(), &path, &Default::default())
        .expect("a project with a mapping saves");
    let loaded = lumen_project::load(&path).expect("and opens");
    assert_eq!(loaded.mappings, lumen.app().project().mappings);
    assert_eq!(loaded.mappings[0].channel, 5);
    assert_eq!(loaded.mappings[0].number, 74);
}

#[test]
fn the_controls_right_click_menu_learns_and_clears() {
    let (mut lumen, fake, track) = launch("learn-menu");
    lumen.click("Fold tone");
    lumen.frames(40);
    let slider = lumen.harness.get_by_label_contains("Volume tone:").rect();
    lumen.right_click(slider.center());
    lumen.click("Learn a controller\u{2026}");
    lumen.frames(3);
    assert_eq!(
        lumen.app().controller_learning(),
        Some((track, Target::Volume))
    );
    cc(&fake, 0, 7, 50);
    lumen.wait_for("the mapping", |app| app.project().mappings.len() == 1);
    // Now the menu offers to clear it, and to open the list.
    lumen.right_click(slider.center());
    assert!(lumen.shows("Clear controller 7 on channel 1"));
    assert!(lumen.shows("Controller mappings\u{2026}"));
    lumen.click("Clear controller 7 on channel 1");
    lumen.frames(3);
    assert!(lumen.app().project().mappings.is_empty());
}

#[test]
fn the_mappings_window_lists_each_mapping_and_clears_it() {
    let (mut lumen, fake, track) = launch("learn-window");
    learn_volume(&mut lumen, &fake, track, 0, 7);
    run_command(&mut lumen, "Controller mappings");
    assert!(lumen.app().mappings_window_open());
    lumen.frames(3);
    assert!(lumen.shows("Controller 7 \u{b7} channel 1"));
    lumen.click("Clear the controller of tone \u{b7} Volume");
    lumen.frames(3);
    assert!(lumen.app().project().mappings.is_empty());
    assert!(
        lumen
            .harness
            .query_by_label_contains("No controller is mapped")
            .is_some()
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(!lumen.app().mappings_window_open());
}

#[test]
fn a_track_in_touch_writes_a_controller_sweep_into_its_lane() {
    let (mut lumen, fake, track) = launch("learn-touch");
    let lane = Lane::new(vec![
        Point::new(Ticks::ZERO, -20.0).with_shape(Shape::Linear),
    ])
    .unwrap();
    lumen
        .app_mut()
        .give_lane(track, Target::Volume, lane)
        .unwrap();
    learn_volume(&mut lumen, &fake, track, 0, 7);
    lumen.app_mut().set_automation_mode(track, "touch");
    let before = points(&lumen, track, Target::Volume);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    for step in 0..10_u8 {
        cc(&fake, 0, 7, 40 + step * 8);
        std::thread::sleep(Duration::from_millis(30));
        lumen.frames(2);
    }
    // Let go: the controller is quiet and the lane takes the pass.
    let_go(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(10);
    let written = points(&lumen, track, Target::Volume);
    assert!(
        written.len() > before.len() + 2,
        "the sweep became points: {written:?}"
    );
    let rising = written
        .windows(2)
        .filter(|w| w[1].0 > Ticks::quarters(1))
        .filter(|w| w[1].1 >= w[0].1 - 0.01)
        .count();
    assert!(
        rising >= 2,
        "and they rise with the controller: {written:?}"
    );
}

#[test]
fn a_take_and_a_controller_sweep_are_recorded_together() {
    use super::record::{
        Keys, beats, clips_of, keyboard, launch, placed, reach, record, stop, track,
    };
    use lumen_project::model::ClipContent;

    let fake = keyboard();
    let project = lumen_project::testing::project_of("Together", vec![track(None, true)]);
    let mut lumen = launch("together", &project, &fake);
    let keys = Keys::new(&fake);
    let id = Id(1);
    // The controller moves the track's volume, which Touch writes.
    let lane = Lane::new(vec![
        Point::new(Ticks::ZERO, -6.0).with_shape(Shape::Linear),
    ])
    .unwrap();
    lumen.app_mut().give_lane(id, Target::Volume, lane).unwrap();
    lumen
        .app_mut()
        .give_mapping(id, Target::Volume, 0, 7)
        .unwrap();
    assert_eq!(lumen.app().automation_mode(id), "Read");
    lumen.frames(5);
    let before = points(&lumen, id, Target::Volume);

    record(&mut lumen);
    assert_eq!(
        lumen.app().automation_mode(id),
        "Latch",
        "recording a track with a mapped control writes its moves"
    );
    reach(&mut lumen, beats(1.0));
    keys.on(60, 90);
    for step in 0..10_u8 {
        keys.controller(7, 30 + step * 10);
        std::thread::sleep(Duration::from_millis(30));
        lumen.frames(2);
    }
    keys.off(60);
    keys.on(64, 100);
    reach(&mut lumen, beats(2.5));
    keys.off(64);
    // Let the controller's hold run out before stopping.
    let_go(&mut lumen);
    stop(&mut lumen);
    assert_eq!(
        lumen.app().automation_mode(id),
        "Read",
        "and the mode goes back when the take ends"
    );

    let clips = clips_of(&lumen);
    assert_eq!(clips.len(), 1, "one clip for the take");
    let pitches: Vec<u8> = placed(&clips[0]).iter().map(|n| n.pitch).collect();
    assert_eq!(pitches, [60, 64], "the notes were recorded");
    let ClipContent::Midi { events, .. } = &clips[0].content else {
        panic!("a MIDI clip");
    };
    assert!(
        events.is_empty(),
        "the mapped controller moved the volume and was not recorded as MIDI: {events:?}"
    );
    let written = points(&lumen, id, Target::Volume);
    assert!(
        written.len() > before.len() + 2,
        "the sweep became automation: {written:?}"
    );
}

#[test]
fn recording_leaves_a_mode_the_user_chose_and_a_track_with_no_mapped_control_alone() {
    use super::record::{keyboard, launch, record, stop, track};

    let fake = keyboard();
    let project = lumen_project::testing::project_of("Modes", vec![track(None, true)]);
    let mut lumen = launch("modes", &project, &fake);
    let id = Id(1);
    // Armed, but nothing is mapped to it: Read stays Read.
    record(&mut lumen);
    assert_eq!(lumen.app().automation_mode(id), "Read");
    stop(&mut lumen);
    // Mapped, and the user chose Touch: it stays Touch, before and after.
    lumen
        .app_mut()
        .give_mapping(id, Target::Volume, 0, 7)
        .unwrap();
    lumen.app_mut().set_automation_mode(id, "touch");
    record(&mut lumen);
    assert_eq!(lumen.app().automation_mode(id), "Touch");
    stop(&mut lumen);
    assert_eq!(lumen.app().automation_mode(id), "Touch");
}

#[test]
fn a_take_with_a_controller_writes_a_lane_that_did_not_exist_yet() {
    use super::record::{Keys, beats, clips_of, keyboard, launch, reach, record, stop, track};

    let fake = keyboard();
    let project = lumen_project::testing::project_of("NewLane", vec![track(None, true)]);
    let mut lumen = launch("new-lane", &project, &fake);
    let keys = Keys::new(&fake);
    let id = Id(1);
    // Learned, but the track has no volume lane and none is shown.
    lumen
        .app_mut()
        .give_mapping(id, Target::Volume, 0, 7)
        .unwrap();
    assert!(points(&lumen, id, Target::Volume).is_empty());
    record(&mut lumen);
    reach(&mut lumen, beats(1.0));
    keys.on(60, 90);
    for step in 0..10_u8 {
        keys.controller(7, 30 + step * 10);
        std::thread::sleep(Duration::from_millis(30));
        lumen.frames(2);
    }
    keys.off(60);
    reach(&mut lumen, beats(2.0));
    let_go(&mut lumen);
    stop(&mut lumen);
    assert_eq!(clips_of(&lumen).len(), 1, "the take");
    let written = points(&lumen, id, Target::Volume);
    assert!(written.len() > 2, "the sweep made a lane: {written:?}");
    assert_eq!(
        lumen.app().automation_shown(id),
        Some(Target::Volume),
        "the new lane is shown, so it can be seen"
    );
}
