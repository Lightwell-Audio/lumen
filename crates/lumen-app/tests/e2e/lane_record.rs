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

use super::Lumen;
use super::hosting::{PLUGIN, effect_device, instrument_device, launch, project, wait_for_state};
use super::lane_view::{near, peak, points, run_command, tone};

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
    for _ in 0..150 {
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(16));
        samples.push((
            start.elapsed().as_secs_f64() * 1000.0,
            lumen.app().playhead().0 as f64,
        ));
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
