//! Automation lanes (M7 slice A0): a project that holds lanes plays them, a
//! lane that is switched off plays nothing, and the lanes survive a save and
//! a reopen. There is no lane interface until slice A1, so the scenarios give
//! the tracks their lanes through `App::give_lane`.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::Id;
use lumen_project::testing::TempDir;

use lumen_app::testing::project_file;

use super::{Lumen, wav};

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

/// A project with one track holding a long tone; returns the track's id.
fn tone(name: &str) -> (Lumen, Id) {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let track = lumen.app().project().tracks[0].id;
    (lumen, track)
}

fn play(lumen: &mut Lumen, from: i64) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(from));
}

/// The loudest left and right sample of `ms` of audio, once any glide is over.
fn peaks(lumen: &mut Lumen, ms: u64) -> (f32, f32) {
    lumen
        .listen(Duration::from_millis(ms), 1_000)
        .iter()
        .fold((0.0f32, 0.0f32), |(l, r), f| {
            (l.max(f[0].abs()), r.max(f[1].abs()))
        })
}

fn near(got: f32, want: f32) -> bool {
    (got - want).abs() <= want.max(1e-3) * 0.1
}

#[test]
fn a_fader_lane_in_the_project_is_heard() {
    let (mut lumen, track) = tone("auto-fader");
    // 0 dB for the first quarter note, then down to −40 dB by the fifth.
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            lane(&[(1, 0.0, Shape::Linear), (5, -40.0, Shape::Linear)]),
        )
        .unwrap();
    lumen.frames(3);
    play(&mut lumen, 0);
    let (left, _) = peaks(&mut lumen, 100);
    assert!(
        near(left, TONE),
        "full level before the lane starts: {left}"
    );
    lumen.wait_for("the lane's middle", |app| {
        app.playhead() > Ticks::quarters(3)
    });
    // Halfway down: −20 dB, a tenth of the level.
    let (left, _) = peaks(&mut lumen, 20);
    assert!(left < TONE * 0.5 && left > TONE * 0.01, "{left}");
    lumen.wait_for("the lane's end", |app| app.playhead() > Ticks::quarters(7));
    let (left, right) = peaks(&mut lumen, 100);
    let want = TONE * 0.01;
    assert!(near(left, want) && near(right, want), "{left} {right}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_lane_that_is_switched_off_plays_nothing_and_on_again_plays() {
    let (mut lumen, track) = tone("auto-switch");
    lumen
        .app_mut()
        .give_lane(track, Target::Volume, lane(&[(0, -40.0, Shape::Step)]))
        .unwrap();
    lumen.frames(3);
    play(&mut lumen, 1);
    let (left, _) = peaks(&mut lumen, 150);
    assert!(near(left, TONE * 0.01), "the lane holds −40 dB: {left}");
    lumen
        .app_mut()
        .switch_lane(track, Target::Volume, false)
        .unwrap();
    lumen.frames(3);
    let (left, _) = peaks(&mut lumen, 150);
    assert!(
        near(left, TONE),
        "off, the fader is the track's own: {left}"
    );
    lumen
        .app_mut()
        .switch_lane(track, Target::Volume, true)
        .unwrap();
    lumen.frames(3);
    let (left, _) = peaks(&mut lumen, 150);
    assert!(near(left, TONE * 0.01), "on again: {left}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_mute_lane_silences_the_track_from_its_step_on() {
    let (mut lumen, track) = tone("auto-mute");
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Mute,
            lane(&[(0, 0.0, Shape::Step), (4, 1.0, Shape::Step)]),
        )
        .unwrap();
    lumen.frames(3);
    play(&mut lumen, 0);
    let (left, _) = peaks(&mut lumen, 100);
    assert!(near(left, TONE), "heard before the step: {left}");
    lumen.wait_for("the step", |app| app.playhead() > Ticks::quarters(5));
    let (left, right) = peaks(&mut lumen, 100);
    assert!(left < 1e-4 && right < 1e-4, "{left} {right}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_pan_lane_moves_the_sound_across() {
    let (mut lumen, track) = tone("auto-pan");
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Pan,
            lane(&[(0, -1.0, Shape::Step), (8, 1.0, Shape::Step)]),
        )
        .unwrap();
    lumen.frames(3);
    play(&mut lumen, 1);
    let (left, right) = peaks(&mut lumen, 100);
    assert!(
        near(left, TONE) && right < 1e-4,
        "hard left: {left} {right}"
    );
    lumen.wait_for("the second half", |app| app.playhead() > Ticks::quarters(9));
    let (left, right) = peaks(&mut lumen, 100);
    assert!(
        left < 1e-4 && near(right, TONE),
        "hard right: {left} {right}"
    );
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn lanes_survive_a_save_and_a_reopen_and_still_play() {
    let (mut lumen, track) = tone("auto-save");
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            lane(&[(0, -40.0, Shape::Step), (4, -6.0, Shape::Linear)]),
        )
        .unwrap();
    lumen.frames(3);
    assert!(lumen.app().is_dirty());
    let path = project_file(lumen.dir(), "Automated");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen.app().project().clone();
    assert_eq!(saved.tracks[0].automation.len(), 1);

    let dir: TempDir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    assert_eq!(again.app().project().tracks, saved.tracks);
    // The reopened project plays its lane: −6 dB once past the fourth quarter.
    play(&mut again, 5);
    let (left, _) = peaks(&mut again, 150);
    assert!(near(left, TONE * 0.5012), "{left}");
    again.key(Modifiers::NONE, Key::Space);
}
