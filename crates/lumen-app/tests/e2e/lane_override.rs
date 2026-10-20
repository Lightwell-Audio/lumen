//! Controls that follow their lane, and lanes the user takes over (M7 slice
//! A4a): a fader whose lane plays shows the lane's value at the playhead; moving
//! it takes the lane over, so the control's own value is heard; Re-enable (the
//! chip's button, the command) gives the lane back. The scenarios listen.

use eframe::egui::{Key, Modifiers, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::Id;

use super::Lumen;
use super::lane_view::{near, peak, run_command, tone};

/// The test tone's peak (`wav`): 3,000 of 32,768.
const TONE: f32 = 3_000.0 / 32_768.0;

fn lane(points: &[(i64, f32, Shape)]) -> Lane {
    Lane::new(
        points
            .iter()
            .map(|&(q, v, s)| Point::new(Ticks::quarters(q), v).with_shape(s))
            .collect(),
    )
    .unwrap()
}

/// The words a control says for itself ("Volume tone: -20.0 dB").
fn said(lumen: &Lumen, label: &str) -> String {
    lumen
        .harness
        .get_by_label_contains(label)
        .accesskit_node()
        .label()
        .unwrap_or_default()
        // The interface writes a minus as U+2212.
        .replace('\u{2212}', "-")
}

/// Moves the playhead to `quarters` by clicking the ruler.
fn locate(lumen: &mut Lumen, quarters: i64) {
    let ruler = lumen
        .harness
        .get_by_label("Ruler: click to move the playhead")
        .rect();
    let zoom = lumen.app().view().zoom as f32;
    let at = Pos2::new(ruler.left() + quarters as f32 * zoom, ruler.center().y);
    lumen.drag(at, at, Modifiers::NONE);
    let target = Ticks::quarters(quarters);
    lumen.wait_for("the playhead", |app| app.playhead() == target);
    lumen.frames(5);
}

/// The tone track with the header unfolded and a lane that holds -20 dB.
fn held(name: &str) -> (Lumen, Id) {
    let (mut lumen, track) = tone(name);
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            lane(&[(0, -20.0, Shape::Linear), (8, 0.0, Shape::Linear)]),
        )
        .unwrap();
    lumen.click("Fold tone");
    lumen.frames(40);
    (lumen, track)
}

#[test]
fn a_fader_shows_its_lane_at_the_playhead_stopped() {
    let (mut lumen, track) = held("override-follow");
    assert!(!lumen.app().lane_taken_over(track, Target::Volume));
    let start = said(&lumen, "Volume tone:");
    locate(&mut lumen, 4);
    let middle = said(&lumen, "Volume tone:");
    locate(&mut lumen, 8);
    let end = said(&lumen, "Volume tone:");
    assert!(start.contains("-20"), "{start}");
    assert!(middle.contains("-10"), "halfway up: {middle}");
    assert!(
        end.contains("0.0") && !end.contains("-"),
        "at the last point: {end}"
    );
    // The track's own volume was never touched.
    assert_eq!(lumen.app().project().tracks[0].gain_db, 0.0);
}

#[test]
fn moving_the_fader_under_a_lane_takes_the_lane_over_and_it_is_heard() {
    let (mut lumen, track) = held("override-heard");
    // The lane holds its first value, -20 dB, wherever the song is.
    lumen
        .app_mut()
        .give_lane(track, Target::Volume, lane(&[(0, -20.0, Shape::Linear)]))
        .unwrap();
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let under_lane = peak(&mut lumen);
    assert!(
        near(under_lane, TONE * 0.1),
        "the lane's -20 dB: {under_lane}"
    );

    // Grab the fader and drag it to the top of its travel.
    let slider = lumen.harness.get_by_label_contains("Volume tone:").rect();
    lumen.drag(
        slider.center(),
        Pos2::new(slider.right() - 1.0, slider.center().y),
        Modifiers::NONE,
    );
    assert!(lumen.app().lane_taken_over(track, Target::Volume));
    let own = peak(&mut lumen);
    assert!(
        own > under_lane * 4.0,
        "the control's own value is heard: {own} against {under_lane}"
    );
    assert!(lumen.shows("Automation lane of tone: Volume \u{00B7} Overridden"));
    // The way back is a button on the chip.
    lumen.click("Re-enable Volume of tone");
    assert!(!lumen.app().lane_taken_over(track, Target::Volume));
    let back = peak(&mut lumen);
    assert!(near(back, under_lane), "the lane again: {back}");
    assert!(!lumen.shows("Re-enable Volume of tone"));
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn the_command_and_the_all_command_re_enable() {
    let (mut lumen, track) = held("override-command");
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    lumen.app_mut().take_over_lane(track, Target::Volume);
    assert!(lumen.app().lane_taken_over(track, Target::Volume));
    run_command(&mut lumen, "Re-enable the automation lane");
    assert!(!lumen.app().lane_taken_over(track, Target::Volume));
    lumen.app_mut().take_over_lane(track, Target::Volume);
    run_command(&mut lumen, "Re-enable all automation lanes");
    assert!(!lumen.app().lane_taken_over(track, Target::Volume));
}

#[test]
fn a_lane_switched_off_or_removed_is_no_longer_taken_over() {
    let (mut lumen, track) = held("override-ends");
    lumen.app_mut().take_over_lane(track, Target::Volume);
    lumen
        .app_mut()
        .switch_lane(track, Target::Volume, false)
        .unwrap();
    assert!(!lumen.app().lane_taken_over(track, Target::Volume));
    lumen
        .app_mut()
        .switch_lane(track, Target::Volume, true)
        .unwrap();
    // It plays again from the first frame it is on: the fader follows it.
    lumen.frames(5);
    assert!(said(&lumen, "Volume tone:").contains("-20"));
}

#[test]
fn a_mute_lane_decides_what_the_m_button_shows_and_a_click_takes_it_over() {
    let (mut lumen, track) = tone("override-mute");
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Mute,
            lane(&[(0, 0.0, Shape::Step), (4, 1.0, Shape::Step)]),
        )
        .unwrap();
    lumen.frames(30);
    assert!(!lumen.is_on("Mute tone"), "heard at the start");
    locate(&mut lumen, 6);
    assert!(lumen.is_on("Mute tone"), "the lane has muted it by now");
    // A click says "unmute", and the control's own value is heard now.
    lumen.click("Mute tone");
    assert!(lumen.app().lane_taken_over(track, Target::Mute));
    assert!(!lumen.is_on("Mute tone"));
    assert!(!lumen.app().project().tracks[0].mute);
    lumen.app_mut().reenable_lane(track, Target::Mute);
    lumen.frames(5);
    assert!(lumen.is_on("Mute tone"), "the lane again");
}

#[test]
fn a_control_with_no_lane_is_never_taken_over() {
    let (mut lumen, track) = tone("override-no-lane");
    lumen.click("Fold tone");
    lumen.frames(40);
    let slider = lumen.harness.get_by_label_contains("Pan tone:").rect();
    lumen.drag(
        slider.center(),
        Pos2::new(slider.right() - 1.0, slider.center().y),
        Modifiers::NONE,
    );
    assert!(!lumen.app().lane_taken_over(track, Target::Pan), "no lane");
    assert!(lumen.app().project().tracks[0].pan > 0.0, "it moved");
}
