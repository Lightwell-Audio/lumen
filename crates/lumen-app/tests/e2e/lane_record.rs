//! Recording a lane (M7 slice A4b): a track in Touch, Latch or Write writes the
//! controls that move while the song plays into its lanes, one undo step for
//! each pass, and Read is left as it was. The scenarios hold a fader with the
//! pointer, listen, and read the lane the pass wrote.

use std::time::Duration;

use eframe::egui::{Key, Modifiers, Pos2, Rect};
use egui_kittest::kittest::Queryable;
use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::Id;

use super::hosting::{
    PLUGIN, effect_device, instrument_device, launch, project, timing_sensitive, wait_for_state,
};
use super::lane_view::{near, peak, points, run_command, tone};
use super::{Lumen, TempDir};

/// The test tone's peak (`wav`): 3,000 of 32,768.
const TONE: f32 = 3_000.0 / 32_768.0;

/// The tone track with its header unfolded and a volume lane that holds -20 dB,
/// in `mode`.
fn recording(name: &str, mode: &str) -> (Lumen, Id) {
    let (mut lumen, track) = tone(name);
    let lane = Lane::new(vec![
        Point::new(Ticks::ZERO, -20.0).with_shape(Shape::Linear),
    ])
    .unwrap();
    lumen
        .app_mut()
        .give_lane(track, Target::Volume, lane)
        .unwrap();
    lumen.app_mut().set_automation_mode(track, mode);
    lumen.click("Fold tone");
    lumen.frames(40);
    (lumen, track)
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
}

fn volume_slider(lumen: &Lumen) -> Rect {
    lumen.harness.get_by_label_contains("Volume tone:").rect()
}

/// Presses the fader and moves it right in steps a frame apart, as a hand
/// would, and keeps holding it.
pub(super) fn grab(lumen: &mut Lumen, slider: Rect) -> Pos2 {
    let from = slider.center();
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    let mut at = from;
    for step in 1..=8 {
        at = from + eframe::egui::vec2(step as f32 * 3.0, 0.0);
        lumen.harness.hover_at(at);
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(16));
    }
    at
}

pub(super) fn let_go(lumen: &mut Lumen, at: Pos2) {
    lumen.harness.drop_at(at);
    lumen.frames(3);
}

fn lane_points(lumen: &Lumen, track: Id) -> Vec<(Ticks, f32)> {
    points(lumen, track, Target::Volume)
}

#[test]
fn touch_writes_while_a_fader_is_held_and_hands_the_lane_back_when_it_is_let_go() {
    let (mut lumen, track) = recording("record-touch", "touch");
    let before = lane_points(&lumen, track);
    play(&mut lumen);
    let level = peak(&mut lumen);
    assert!(near(level, TONE * 0.1), "the lane's -20 dB: {level}");

    let slider = volume_slider(&lumen);
    let at = grab(&mut lumen, slider);
    assert!(lumen.app().recording_automation(track, Target::Volume));
    assert!(
        !lumen.app().shows_lane_overridden(track, Target::Volume),
        "a lane being written is not shown as overridden"
    );
    let held = peak(&mut lumen);
    assert!(held > level * 2.0, "the hand is heard: {held} over {level}");
    // Nothing is written until the hand lets go.
    assert_eq!(lane_points(&lumen, track), before);

    let_go(&mut lumen, at);
    lumen.frames(5);
    assert!(!lumen.app().recording_automation(track, Target::Volume));
    let written = lane_points(&lumen, track);
    assert!(written.len() >= 3, "{written:?}");
    let highest = written.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    assert!(highest > -20.0 + 1.0, "the move was written: {written:?}");
    assert_eq!(
        written.last().unwrap().1,
        -20.0,
        "the glide ends back on the lane: {written:?}"
    );
    // The lane plays again from the next block: the fader follows it and the
    // sound is the lane's.
    lumen.frames(20);
    let back = peak(&mut lumen);
    assert!(near(back, level), "the lane again: {back} against {level}");
    lumen.key(Modifiers::NONE, Key::Space);

    // One pass, one undo step.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(lane_points(&lumen, track), before);
}

#[test]
fn latch_goes_on_writing_after_the_hand_is_gone_until_stop() {
    let (mut lumen, track) = recording("record-latch", "latch");
    play(&mut lumen);
    let slider = volume_slider(&lumen);
    let at = grab(&mut lumen, slider);
    let_go(&mut lumen, at);
    assert!(
        lumen.app().recording_automation(track, Target::Volume),
        "still writing"
    );
    let released_at = lumen.app().playhead();
    lumen.wait_for("the song to go on", |app| {
        app.playhead() > released_at + Ticks::quarters(1)
    });
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(5);
    assert!(!lumen.app().recording_automation(track, Target::Volume));
    let written = lane_points(&lumen, track);
    let last = written.last().unwrap();
    assert!(last.0 > released_at, "written to the stop: {written:?}");
    assert!(
        last.1 > -20.0,
        "the level the fader was left at: {written:?}"
    );
}

#[test]
fn write_overwrites_the_lane_with_the_control_from_where_playing_began() {
    let (mut lumen, track) = recording("record-write", "write");
    // A pan lane the track also has, not the one its chip shows.
    let pan = Lane::new(vec![
        Point::new(Ticks::ZERO, -0.5),
        Point::new(Ticks::quarters(8), 0.5),
    ])
    .unwrap();
    lumen
        .app_mut()
        .give_lane(track, Target::Pan, pan.clone())
        .unwrap();
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    assert_eq!(lumen.app().automation_shown(track), Some(Target::Volume));
    let before = lane_points(&lumen, track);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(2));
    assert!(lumen.app().recording_automation(track, Target::Volume));
    // The lane is not heard against what is written: the fader's own 0 dB is.
    let heard = peak(&mut lumen);
    assert!(near(heard, TONE), "the control's own level: {heard}");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(5);
    assert!(!lumen.app().recording_automation(track, Target::Volume));
    let written = lane_points(&lumen, track);
    assert_eq!(
        written.iter().map(|p| p.1).fold(f32::MIN, f32::max),
        0.0,
        "{written:?}"
    );
    let value_at = |q: i64| {
        let lane = lumen
            .app()
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.lane(Target::Volume))
            .map(|l| l.lane.clone())
            .unwrap();
        lane.value_at(Ticks::quarters(q))
    };
    assert_eq!(value_at(1), Some(0.0), "overwritten");
    let pan_points: Vec<_> = pan.points().iter().map(|p| (p.at, p.value)).collect();
    assert_eq!(
        points(&lumen, track, Target::Pan),
        pan_points,
        "the lane the chip does not show is left alone"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(lane_points(&lumen, track), before, "one undo step");
}

#[test]
fn a_control_moved_while_stopped_is_not_recorded() {
    let (mut lumen, track) = recording("record-stopped", "touch");
    let before = lane_points(&lumen, track);
    let slider = volume_slider(&lumen);
    let at = grab(&mut lumen, slider);
    let_go(&mut lumen, at);
    lumen.frames(5);
    assert_eq!(lane_points(&lumen, track), before);
    assert!(!lumen.app().recording_automation(track, Target::Volume));
    // It is the control's own value, as in Read: the lane is taken over.
    assert!(lumen.app().lane_taken_over(track, Target::Volume));
}

#[test]
fn read_still_takes_the_lane_over_while_playing_and_writes_nothing() {
    let (mut lumen, track) = recording("record-read", "read");
    let before = lane_points(&lumen, track);
    play(&mut lumen);
    let slider = volume_slider(&lumen);
    let at = grab(&mut lumen, slider);
    let_go(&mut lumen, at);
    lumen.frames(5);
    assert!(lumen.app().lane_taken_over(track, Target::Volume));
    assert!(!lumen.app().recording_automation(track, Target::Volume));
    lumen.key(Modifiers::NONE, Key::Space);
    assert_eq!(lane_points(&lumen, track), before);
}

#[test]
fn the_mode_is_chosen_from_the_chip_and_from_the_palette() {
    let (mut lumen, track) = recording("record-mode", "read");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    assert_eq!(lumen.app().automation_mode(track), "Read");
    assert!(lumen.shows("Automation lane of tone: Volume"));
    lumen.click("Automation lane of tone: Volume");
    lumen.click("Touch");
    assert_eq!(lumen.app().automation_mode(track), "Touch");
    assert!(
        lumen.shows("Automation lane of tone: Volume \u{00B7} Touch"),
        "the chip says what the track does"
    );
    run_command(&mut lumen, "Automation mode: Latch");
    assert_eq!(lumen.app().automation_mode(track), "Latch");
    run_command(&mut lumen, "Automation mode: Write");
    assert_eq!(lumen.app().automation_mode(track), "Write");
    run_command(&mut lumen, "Automation mode: Read");
    assert_eq!(lumen.app().automation_mode(track), "Read");
    assert!(lumen.shows("Automation lane of tone: Volume"));
}

#[test]
fn a_plugin_gesture_is_recorded_from_its_begin_to_its_end() {
    const KEYS: Id = Id(1);
    const FX: Id = Id(21);
    const LEVEL: u32 = 0;
    let _turn = timing_sensitive();
    let song = project(|d| *d = vec![instrument_device(PLUGIN), effect_device(FX)]);
    let mut lumen = launch("record-gesture", &song, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    wait_for_state(&mut lumen, FX, "ready");
    let target = Target::Device {
        device: FX,
        param: LEVEL,
    };
    let lane = Lane::new(vec![Point::new(Ticks::ZERO, 0.5)]).unwrap();
    lumen.app_mut().give_lane(KEYS, target, lane).unwrap();
    lumen.app_mut().set_automation_mode(KEYS, "touch");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));

    lumen
        .app_mut()
        .pretend_plugin_edits(FX, &[(LEVEL, 0.3, "begin"), (LEVEL, 0.3, "perform")]);
    lumen.frames(2);
    assert!(lumen.app().recording_automation(KEYS, target), "held");
    for value in [0.4, 0.6, 0.8] {
        std::thread::sleep(Duration::from_millis(40));
        lumen
            .app_mut()
            .pretend_plugin_edits(FX, &[(LEVEL, value, "perform")]);
        lumen.frames(2);
    }
    assert!(
        lumen.app().recording_automation(KEYS, target),
        "no end yet: still held with no pointer down"
    );
    lumen
        .app_mut()
        .pretend_plugin_edits(FX, &[(LEVEL, 0.8, "end")]);
    lumen.frames(4);
    assert!(!lumen.app().recording_automation(KEYS, target));
    let written: Vec<f32> = lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == KEYS)
        .and_then(|t| t.lane(target))
        .map(|l| l.lane.points().iter().map(|p| p.value).collect())
        .unwrap();
    assert!(
        written.iter().any(|v| (v - 0.8).abs() < 0.01)
            && written.iter().any(|v| (v - 0.3).abs() < 0.01),
        "the gesture was written: {written:?}"
    );
    lumen.key(Modifiers::NONE, Key::Space);
}

/// How true to the clock the playhead is that stamps every recorded point
/// (M7 slice A4b: measured, not assumed). A move is stamped with the playhead
/// of the frame that sees it, so a point is late by at most one frame of the
/// interface on top of what this measures.
#[test]
fn the_playhead_that_stamps_the_points_follows_the_clock_to_within_a_frame() {
    let (mut lumen, _) = recording("record-timing", "touch");
    play(&mut lumen);
    let start = std::time::Instant::now();
    let mut samples: Vec<(f64, f64)> = Vec::new();
    while samples.len() < 150 {
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(16));
        // A sample is a playhead read and the clock at the same moment: when
        // this thread was held up between the two reads (a busy machine) the
        // pair says nothing about the playhead, so it is not kept.
        let before = start.elapsed();
        let at = lumen.app().playhead().0 as f64;
        let after = start.elapsed();
        if after - before < Duration::from_millis(2) {
            samples.push(((before + after).as_secs_f64() * 500.0, at));
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "no steady reads in a minute"
        );
    }
    lumen.key(Modifiers::NONE, Key::Space);
    // The best straight line through the playhead against the clock.
    let n = samples.len() as f64;
    let (mt, mp) = (
        samples.iter().map(|s| s.0).sum::<f64>() / n,
        samples.iter().map(|s| s.1).sum::<f64>() / n,
    );
    let slope = samples.iter().map(|s| (s.0 - mt) * (s.1 - mp)).sum::<f64>()
        / samples.iter().map(|s| (s.0 - mt).powi(2)).sum::<f64>();
    // The silent test device does not run at the speed of the clock, so what
    // is measured is how straight the playhead runs, in the time the fitted
    // speed takes for a tick.
    let ms_per_tick = 1.0 / slope;
    let worst = samples
        .iter()
        .map(|s| ((s.1 - (mp + slope * (s.0 - mt))) * ms_per_tick).abs())
        .fold(0.0, f64::max);
    println!(
        "playhead against a straight line: worst {worst:.2} ms over {} frames",
        samples.len()
    );
    assert!(worst < 20.0, "worst {worst} ms");
}

#[test]
fn a_touch_on_a_control_with_no_lane_yet_returns_to_where_the_control_rested() {
    let (mut lumen, track) = tone("record-first");
    lumen.app_mut().set_automation_mode(track, "touch");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.click("Fold tone");
    lumen.frames(40);
    assert!(lane_points(&lumen, track).is_empty());
    play(&mut lumen);
    let rest = peak(&mut lumen);
    let slider = volume_slider(&lumen);
    let at = grab(&mut lumen, slider);
    let held = peak(&mut lumen);
    assert!(held > rest * 1.2, "the hand is heard: {held} over {rest}");
    let_go(&mut lumen, at);
    lumen.frames(20);
    assert!(!lumen.app().recording_automation(track, Target::Volume));
    let written = lane_points(&lumen, track);
    assert_eq!(
        written.first(),
        Some(&(Ticks::ZERO, 0.0)),
        "the new lane says what the fader held before: {written:?}"
    );
    assert_eq!(
        written.last().map(|p| p.1),
        Some(0.0),
        "and the pass hands back to it: {written:?}"
    );
    let back = peak(&mut lumen);
    assert!(
        near(back, rest),
        "not left where it was let go: {back} against {rest}"
    );
}

/// The volume lane of `track` as the project holds it.
fn volume_of(app: &lumen_app::App, track: Id) -> Vec<(Ticks, f32)> {
    app.project()
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.lane(Target::Volume))
        .map(|l| l.lane.points().iter().map(|p| (p.at, p.value)).collect())
        .unwrap_or_default()
}

#[test]
fn a_long_pass_is_kept_in_the_project_as_it_goes_and_is_still_one_undo_step() {
    let (mut lumen, track) = recording("record-parts", "write");
    lumen
        .app_mut()
        .set_pass_pacing(Duration::from_millis(200), Duration::from_millis(50));
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let before = lane_points(&lumen, track);
    play(&mut lumen);
    lumen.wait_for("a part of the pass in the project", move |app| {
        volume_of(app, track) != before
    });
    // Still under way: the lane is being written and not played against the
    // hand, though the project already holds what was heard.
    assert!(lumen.app().recording_automation(track, Target::Volume));
    assert!(lumen.app().lane_taken_over(track, Target::Volume));
    let heard = peak(&mut lumen);
    assert!(near(heard, TONE), "the control's own level: {heard}");
    assert!(
        lumen.app().is_dirty(),
        "the part counts as an unsaved change"
    );

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(5);
    assert!(!lumen.app().recording_automation(track, Target::Volume));
    assert!(
        !lumen.app().lane_taken_over(track, Target::Volume),
        "the finished pass hands the lane back"
    );
    // However many parts were written, undo takes the pass back in one step.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(lane_points(&lumen, track), vec![(Ticks::ZERO, -20.0)]);
}

#[test]
fn a_pass_under_way_when_the_app_stops_is_found_in_the_autosave() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-record-crash"), |launch| {
        launch.autosaving_after(Duration::from_secs(60))
    });
    let file = super::wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let track = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[track]);
    lumen.frames(40);
    let lane = Lane::new(vec![
        Point::new(Ticks::ZERO, -20.0).with_shape(Shape::Linear),
    ])
    .unwrap();
    lumen
        .app_mut()
        .give_lane(track, Target::Volume, lane)
        .unwrap();
    lumen.app_mut().set_automation_mode(track, "write");
    lumen
        .app_mut()
        .set_pass_pacing(Duration::from_millis(200), Duration::from_millis(50));
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let autosaves = lumen.dir().join("autosave");
    play(&mut lumen);
    // An autosave written while the pass is still under way holds its first
    // part.
    lumen.wait_for("an autosave of the pass so far", |app| {
        app.playhead() > Ticks::quarters(1)
            && std::fs::read_dir(&autosaves).is_ok_and(|entries| {
                entries.flatten().any(|e| {
                    lumen_project::load(&e.path()).is_ok_and(|p| {
                        p.tracks[0]
                            .lane(Target::Volume)
                            .is_some_and(|l| l.lane.points().iter().any(|pt| pt.value == 0.0))
                    })
                })
            })
    });
    assert!(lumen.app().recording_automation(track, Target::Volume));

    // The app stops without closing: recovery offers the pass so far.
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch);
    again.click("Recover");
    again.wait_for("the recovered project", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
    let recovered = volume_of(again.app(), track);
    assert!(
        recovered.iter().any(|p| p.1 == 0.0),
        "the pass so far came back: {recovered:?}"
    );
}

#[test]
fn opening_another_project_leaves_a_pass_under_way_behind() {
    // The M7 review's H1: a pass under way, then another project opens. The
    // pass belongs to the old project's track ids; the new project has a
    // track with the same id and must not be given a lane it never had.
    let (mut lumen, track) = recording("record-open", "write");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    play(&mut lumen);
    lumen.wait_for("the pass", |app| {
        app.playhead() > Ticks::quarters(2) && app.recording_automation(track, Target::Volume)
    });
    let other = lumen_project::testing::project_of(
        "Other",
        vec![lumen_project::model::Track::new(
            track,
            "Other",
            lumen_project::model::TrackKind::Audio,
            lumen_project::model::Color(0x5FA89A),
        )],
    );
    let path = lumen.dir().join("other.lumen");
    lumen_project::save(&other, &path, &Default::default()).unwrap();
    lumen.app_mut().open_project_file(path);
    lumen.wait_for("the other project", |app| app.project().name == "Other");
    lumen.frames(60);
    assert!(
        lumen.app().project().tracks[0].automation.is_empty(),
        "the old project's pass was written into the new one"
    );
    assert!(
        !lumen.app().is_dirty(),
        "and the new project is as it was saved"
    );
    assert_eq!(lumen.app().undo_label(), None);
}

#[test]
fn opening_another_project_ends_a_control_waiting_for_a_controller() {
    let (mut lumen, track) = recording("record-open-learn", "read");
    lumen.app_mut().begin_learning(track, Target::Volume);
    assert_eq!(
        lumen.app().controller_learning(),
        Some((track, Target::Volume))
    );
    let other = lumen_project::testing::project_of(
        "Other",
        vec![lumen_project::model::Track::new(
            track,
            "Other",
            lumen_project::model::TrackKind::Audio,
            lumen_project::model::Color(0x5FA89A),
        )],
    );
    let path = lumen.dir().join("other.lumen");
    lumen_project::save(&other, &path, &Default::default()).unwrap();
    lumen.app_mut().open_project_file(path);
    lumen.wait_for("the other project", |app| app.project().name == "Other");
    assert_eq!(
        lumen.app().controller_learning(),
        None,
        "the next controller moved must not map the other project's control"
    );
}
