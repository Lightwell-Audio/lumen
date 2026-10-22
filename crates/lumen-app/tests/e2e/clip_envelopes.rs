//! Envelopes on audio clips (M7 slice A7, the owner's E1 A and E2): the E key
//! shows the selected clip's gain envelope over it, a click over the clip adds
//! a point kept from the clip's start, the chip switches to pan, the envelope
//! is heard, moves with the clip, survives a save and a reopen, and every edit
//! is one undo step.

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::envelope::Kind;
use lumen_project::model::Id;

use super::lane_view::{click_at, near, peak, point_pos, spot, tone};
use super::{Lumen, TempDir};

/// The test tone's peak (`wav`): 3,000 of 32,768.
const TONE: f32 = 3_000.0 / 32_768.0;

/// The first clip of the first track, selected, with the track's clips left as
/// they are.
fn picked(lumen: &mut Lumen) -> (Id, Id) {
    let track = lumen.app().project().tracks[0].id;
    let clip = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    lumen.frames(5);
    (track, clip)
}

fn envelope(lumen: &Lumen, kind: Kind) -> Vec<(Ticks, f32)> {
    lumen.app().project().tracks[0].clips[0]
        .envelopes
        .get(kind)
        .map(|l| l.points().iter().map(|p| (p.at, p.value)).collect())
        .unwrap_or_default()
}

#[test]
fn the_e_key_shows_and_hides_the_selected_clips_envelope() {
    let (mut lumen, track) = tone("env-key");
    // Nothing selected: nothing happens, and the track shows no lane.
    lumen.app_mut().select_tracks(&[]);
    lumen.key(Modifiers::NONE, Key::E);
    assert_eq!(lumen.app().automation_shown(track), None);
    let (track, clip) = picked(&mut lumen);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    assert_eq!(
        lumen.app().automation_shown(track),
        Some(Target::ClipGain { clip })
    );
    assert!(
        lumen.shows("Automation lane of tone: Clip gain \u{00B7} tone"),
        "the chip names the envelope and its clip"
    );
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(5);
    assert_eq!(
        lumen.app().automation_shown(track),
        None,
        "E again hides it"
    );
}

#[test]
fn a_click_over_the_clip_adds_a_point_and_it_is_kept_from_the_clips_start() {
    let (mut lumen, _) = tone("env-click");
    let (track, clip) = picked(&mut lumen);
    // The clip starts at quarter 4, so its points are measured from there.
    lumen.app_mut().move_clip_to(clip, Ticks::quarters(4));
    lumen.frames(5);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    let at = spot(&lumen, track, 7.2, -20.0);
    click_at(&mut lumen, at);
    let held = envelope(&lumen, Kind::Gain);
    assert_eq!(held.len(), 1, "one point: {held:?}");
    assert_eq!(
        held[0].0,
        Ticks::quarters(3),
        "three quarters in, on the beat"
    );
    assert!(
        (held[0].1 + 20.0).abs() < 1.5,
        "about −20 dB: {}",
        held[0].1
    );
    assert_eq!(lumen.app().automation_picked(), Some((track, vec![0])));
    assert!(lumen.app().is_dirty());
    // The track has no lane of its own for it.
    assert!(lumen.app().project().tracks[0].automation.is_empty());
    // One undo step takes the envelope with it.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(envelope(&lumen, Kind::Gain).is_empty());
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(envelope(&lumen, Kind::Gain).len(), 1);
}

#[test]
fn a_click_beside_the_clip_is_not_the_envelopes() {
    let (mut lumen, _) = tone("env-beside");
    let (track, clip) = picked(&mut lumen);
    // Shorten the clip to two quarters so there is track beside it.
    lumen
        .app_mut()
        .trim_clip_end_to(clip, Ticks::quarters(2), false);
    lumen.frames(5);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    let beside = spot(&lumen, track, 6.0, 0.0);
    click_at(&mut lumen, beside);
    assert!(
        envelope(&lumen, Kind::Gain).is_empty(),
        "the track beside the clip belongs to the clips"
    );
}

#[test]
fn a_gain_envelope_is_heard() {
    let (mut lumen, _) = tone("env-heard");
    let (track, _) = picked(&mut lumen);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    let at = spot(&lumen, track, 4.0, -20.0);
    click_at(&mut lumen, at);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let level = peak(&mut lumen);
    assert!(
        near(level, TONE * 0.1),
        "a tenth of the level at −20 dB: {level}"
    );
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn dragging_a_point_is_one_undo_step() {
    let (mut lumen, _) = tone("env-drag");
    let (track, _) = picked(&mut lumen);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    let add = spot(&lumen, track, 2.0, 0.0);
    click_at(&mut lumen, add);
    let from = point_pos(&lumen, track, 0);
    let to = spot(&lumen, track, 5.0, -12.0);
    lumen.drag(from, to, Modifiers::NONE);
    let held = envelope(&lumen, Kind::Gain);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].0, Ticks::quarters(5), "moved to the beat");
    lumen.key(Modifiers::COMMAND, Key::Z);
    let back = envelope(&lumen, Kind::Gain);
    assert_eq!(
        back.first().map(|p| p.0),
        Some(Ticks::quarters(2)),
        "the drag undoes in one step, to the point as it was added"
    );
}

#[test]
fn the_chip_switches_to_pan_and_back() {
    let (mut lumen, _) = tone("env-chip");
    let (track, clip) = picked(&mut lumen);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    lumen.click("Automation lane of tone: Clip gain \u{00B7} tone");
    lumen.click("Clip pan");
    lumen.frames(5);
    assert_eq!(
        lumen.app().automation_shown(track),
        Some(Target::ClipPan { clip })
    );
    let at = spot(&lumen, track, 3.0, 1.0);
    click_at(&mut lumen, at);
    let held = envelope(&lumen, Kind::Pan);
    assert_eq!(held.len(), 1);
    assert!(held[0].1 > 0.9, "hard right: {}", held[0].1);
    assert!(envelope(&lumen, Kind::Gain).is_empty());
    // Pan is heard: the left side goes quiet.
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let frames = lumen.listen(std::time::Duration::from_millis(150), 1_000);
    let left = frames.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
    let right = frames.iter().fold(0.0f32, |m, f| m.max(f[1].abs()));
    assert!(left < 1e-3 * 10.0 && right > left * 10.0, "{left} {right}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn the_envelope_moves_with_its_clip_and_survives_a_save_and_a_reopen() {
    let (mut lumen, _) = tone("env-save");
    let (track, clip) = picked(&mut lumen);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    let at = spot(&lumen, track, 3.0, -12.0);
    click_at(&mut lumen, at);
    lumen.app_mut().move_clip_to(clip, Ticks::quarters(8));
    lumen.frames(5);
    assert_eq!(
        envelope(&lumen, Kind::Gain)[0].0,
        Ticks::quarters(3),
        "a move carries the points"
    );
    let path = lumen.dir().join("Enveloped.lumen");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen.app().project().tracks.clone();
    let dir: TempDir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    assert_eq!(again.app().project().tracks, saved);
    assert_eq!(
        again.app().project().tracks[0].clips[0]
            .envelopes
            .gain
            .as_ref()
            .map(|l| l.len()),
        Some(1)
    );
}

#[test]
fn delete_takes_the_picked_point_and_the_last_one_takes_the_envelope() {
    let (mut lumen, _) = tone("env-delete");
    let (track, _) = picked(&mut lumen);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    let at = spot(&lumen, track, 3.0, -12.0);
    click_at(&mut lumen, at);
    assert_eq!(envelope(&lumen, Kind::Gain).len(), 1);
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(3);
    assert!(envelope(&lumen, Kind::Gain).is_empty());
    assert!(
        lumen.app().project().tracks[0].clips[0]
            .envelopes
            .is_empty(),
        "no empty envelope is kept"
    );
}

// ---- Lock envelopes (the owner's E3): the track's automation and its clips ----

use lumen_core::automation::{Lane, Point};

fn volume_lane(points: &[(i64, f32)]) -> Lane {
    Lane::new(
        points
            .iter()
            .map(|&(q, v)| Point::new(Ticks::quarters(q), v))
            .collect(),
    )
    .unwrap()
}

/// A track whose one clip is eight quarters long, with a volume lane that has
/// points at 2 and 6 (under the clip) and 12 (past it).
fn automated(name: &str) -> (Lumen, Id, Id) {
    let (mut lumen, _) = tone(name);
    let (track, clip) = picked(&mut lumen);
    lumen
        .app_mut()
        .trim_clip_end_to(clip, Ticks::quarters(8), false);
    lumen
        .app_mut()
        .give_lane(
            track,
            Target::Volume,
            volume_lane(&[(2, -6.0), (6, -12.0), (12, 0.0)]),
        )
        .unwrap();
    lumen.frames(5);
    (lumen, track, clip)
}

fn volume_points(lumen: &Lumen) -> Vec<i64> {
    lumen.app().project().tracks[0]
        .lane(Target::Volume)
        .map(|l| {
            l.lane
                .points()
                .iter()
                .map(|p| p.at.0 / Ticks::quarters(1).0)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_moved_clip_takes_the_automation_under_it_and_undo_gives_it_back() {
    let (mut lumen, _, clip) = automated("lock-move");
    lumen.app_mut().move_clip_to(clip, Ticks::quarters(16));
    lumen.frames(3);
    assert_eq!(
        volume_points(&lumen),
        vec![12, 18, 22],
        "2 and 6 went with it"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(volume_points(&lumen), vec![2, 6, 12], "one undo step");
}

#[test]
fn with_envelopes_locked_the_automation_stays_where_it_is() {
    let (mut lumen, _, clip) = automated("lock-on");
    super::lane_view::run_command(&mut lumen, "Lock envelopes");
    lumen.app_mut().move_clip_to(clip, Ticks::quarters(16));
    lumen.frames(3);
    assert_eq!(volume_points(&lumen), vec![2, 6, 12]);
    // The choice is the user's, kept in the settings.
    assert!(lumen.app().lock_envelopes());
    super::lane_view::run_command(&mut lumen, "Lock envelopes");
    assert!(!lumen.app().lock_envelopes());
}

#[test]
fn deleting_a_clip_removes_the_automation_under_it() {
    let (mut lumen, _, clip) = automated("lock-delete");
    lumen.app_mut().select_clip(clip);
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(3);
    assert_eq!(volume_points(&lumen), vec![12]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(volume_points(&lumen), vec![2, 6, 12]);
}

#[test]
fn duplicating_a_clip_duplicates_the_automation_under_it() {
    let (mut lumen, _, clip) = automated("lock-duplicate");
    lumen.app_mut().select_clip(clip);
    lumen.key(Modifiers::COMMAND, Key::D);
    lumen.frames(5);
    // The copy lands right after the clip, eight quarters on: its points are
    // 8 later, and the point at 12 is under it, so it is replaced by the copy.
    assert_eq!(volume_points(&lumen), vec![2, 6, 10, 14]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(volume_points(&lumen), vec![2, 6, 12]);
}
