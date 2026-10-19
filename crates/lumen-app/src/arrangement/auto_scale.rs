//! Where a lane's values sit on screen (M7 slice A1): the height of a value
//! in the lane, the value a height means, how a value reads in words and
//! what a target is called. Pure, so the drawing and the pointer share one
//! mapping and the tests can check it.
//!
//! Levels (volume and sends) put silence on the lane's floor, the bottom
//! `SILENCE_BAND` of it, and −60 dB to +6 dB above that in a straight line,
//! as the volume slider does. Trim, pan and plugin parameters run in a
//! straight line across their whole range. Mute has two heights.

use lumen_core::automation::FLOOR_DB;
use lumen_project::automation::Target;
use lumen_project::model::{Id, Project, Track};

use crate::widgets::SliderKind;

/// The bottom of the lane that means silence on a level: a point drawn
/// there takes the floor, which the engine plays as silence.
const SILENCE_BAND: f32 = 0.04;
/// Where the volume slider's own scale starts, in dB.
const LEVEL_LOW: f32 = -60.0;
const LEVEL_HIGH: f32 = 6.0;

/// The slider that shows `target`'s values, if one fits.
fn slider(target: Target) -> Option<SliderKind> {
    match target {
        Target::Volume | Target::Send { .. } => Some(SliderKind::Volume),
        Target::Trim => Some(SliderKind::Trim),
        Target::Pan => Some(SliderKind::Pan),
        Target::Mute | Target::Device { .. } => None,
    }
}

/// How high `value` sits in a lane, 0 at the floor and 1 at the top.
pub(crate) fn fraction(target: Target, value: f32) -> f32 {
    match target {
        Target::Volume | Target::Send { .. } => {
            let v = value.clamp(FLOOR_DB, LEVEL_HIGH);
            if v >= LEVEL_LOW {
                SILENCE_BAND + (1.0 - SILENCE_BAND) * (v - LEVEL_LOW) / (LEVEL_HIGH - LEVEL_LOW)
            } else {
                SILENCE_BAND * (v - FLOOR_DB) / (LEVEL_LOW - FLOOR_DB)
            }
        }
        _ => {
            let (low, high) = target.range();
            ((value - low) / (high - low)).clamp(0.0, 1.0)
        }
    }
}

/// The value at height `fraction` of the lane: the inverse of
/// [`fraction`]. A mute lane takes its nearer end; a level taken to the
/// floor band is silence.
pub(crate) fn value(target: Target, fraction: f32) -> f32 {
    let f = fraction.clamp(0.0, 1.0);
    match target {
        Target::Volume | Target::Send { .. } => {
            if f <= SILENCE_BAND {
                if f <= SILENCE_BAND * 0.5 {
                    FLOOR_DB
                } else {
                    LEVEL_LOW + (FLOOR_DB - LEVEL_LOW) * (1.0 - f / SILENCE_BAND)
                }
            } else {
                LEVEL_LOW + (LEVEL_HIGH - LEVEL_LOW) * (f - SILENCE_BAND) / (1.0 - SILENCE_BAND)
            }
        }
        Target::Mute => {
            if f >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
        _ => {
            let (low, high) = target.range();
            low + (high - low) * f
        }
    }
}

/// What a click on empty lane takes for a value when the lane has none to
/// continue: the control's own rest value.
pub(crate) fn default_value(target: Target) -> f32 {
    match target {
        Target::Volume | Target::Send { .. } | Target::Trim | Target::Pan | Target::Mute => 0.0,
        Target::Device { .. } => 0.5,
    }
}

/// The heights (0 to 1) where a value likes to rest: the ends of the
/// control and where it is neutral (unity, centre), so a point dragged near
/// one lands on it. Few, because every one makes the lane stickier. Mute has
/// only its two ends, which it already snaps to.
fn detents(target: Target) -> Vec<f32> {
    let values: &[f32] = match target {
        Target::Volume | Target::Send { .. } => &[FLOOR_DB, 0.0, LEVEL_HIGH],
        Target::Trim => &[-24.0, 0.0, 24.0],
        Target::Pan => &[-1.0, 0.0, 1.0],
        Target::Device { .. } => &[0.0, 0.5, 1.0],
        Target::Mute => &[],
    };
    values.iter().map(|&v| fraction(target, v)).collect()
}

/// `height` taken to the nearest resting place within `reach` of it (both
/// as shares of the lane's height), and whether it was.
pub(crate) fn snap_height(target: Target, height: f32, reach: f32) -> (f32, bool) {
    detents(target)
        .into_iter()
        .map(|d| (d, (d - height).abs()))
        .filter(|(_, away)| *away <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or((height, false), |(d, _)| (d, true))
}

/// The value as a person reads it: "−6.0 dB", "L 30", "Muted".
pub(crate) fn text(target: Target, value: f32) -> String {
    match (target, slider(target)) {
        (_, Some(kind)) => {
            if matches!(target, Target::Volume | Target::Send { .. }) && value <= FLOOR_DB + 0.5 {
                "\u{2212}\u{221E} dB".to_owned()
            } else {
                kind.text(value)
            }
        }
        (Target::Mute, _) => if value >= 0.5 { "Muted" } else { "Heard" }.to_owned(),
        _ => format!("{:.0}%", value * 100.0),
    }
}

/// What the lane is called: the chip's word and the chooser's row.
pub(crate) fn name(project: &Project, target: Target) -> String {
    match target {
        Target::Volume => "Volume".to_owned(),
        Target::Pan => "Pan".to_owned(),
        Target::Trim => "Trim".to_owned(),
        Target::Mute => "Mute".to_owned(),
        Target::Send { to } => format!("Send to {}", track_name(project, to)),
        Target::Device { param, .. } => format!("Parameter {}", param + 1),
    }
}

fn track_name(project: &Project, id: Id) -> String {
    project
        .tracks
        .iter()
        .find(|t| t.id == id)
        .map_or_else(|| "a return".to_owned(), |t| t.name.clone())
}

/// The targets a track offers in the chooser, in order: its own values,
/// then one send for each return it sends to.
pub(crate) fn offered(track: &Track) -> Vec<Target> {
    let mut targets = vec![Target::Volume, Target::Pan, Target::Trim, Target::Mute];
    targets.extend(track.sends.iter().map(|s| Target::Send { to: s.to }));
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_run_from_the_floor_to_the_top() {
        assert_eq!(fraction(Target::Volume, FLOOR_DB), 0.0);
        assert_eq!(fraction(Target::Volume, LEVEL_HIGH), 1.0);
        assert!((fraction(Target::Volume, LEVEL_LOW) - SILENCE_BAND).abs() < 1e-6);
        assert_eq!(fraction(Target::Pan, -1.0), 0.0);
        assert_eq!(fraction(Target::Pan, 0.0), 0.5);
        assert_eq!(fraction(Target::Mute, 1.0), 1.0);
        assert_eq!(fraction(Target::Trim, 0.0), 0.5);
    }

    #[test]
    fn a_height_and_its_value_agree_both_ways() {
        for target in [Target::Volume, Target::Pan, Target::Trim] {
            for step in 0..=20 {
                let f = step as f32 / 20.0;
                // Heights inside the silence band all mean the floor, so only
                // the ones above it come back as they were.
                if matches!(target, Target::Volume) && f < SILENCE_BAND {
                    continue;
                }
                let back = fraction(target, value(target, f));
                assert!((back - f).abs() < 1e-4, "{target:?} {f} came back {back}");
            }
        }
        assert_eq!(value(Target::Trim, 1.0), 24.0);
        assert_eq!(value(Target::Trim, 0.0), -24.0);
    }

    #[test]
    fn the_lowest_part_of_a_level_lane_is_silence() {
        assert_eq!(value(Target::Volume, 0.0), FLOOR_DB);
        assert_eq!(value(Target::Volume, SILENCE_BAND * 0.4), FLOOR_DB);
        assert!(value(Target::Volume, SILENCE_BAND * 0.9) > FLOOR_DB);
        assert_eq!(value(Target::Volume, 1.0), LEVEL_HIGH);
        // Nothing drawn can leave the range a lane may hold.
        for f in [-1.0, 0.0, 0.3, 0.99, 1.0, 5.0] {
            let v = value(Target::Volume, f);
            assert_eq!(Target::Volume.clamp(v), v);
        }
    }

    #[test]
    fn a_height_near_a_resting_place_lands_on_it_and_one_far_from_all_does_not() {
        let zero = fraction(Target::Volume, 0.0);
        let (at, snapped) = snap_height(Target::Volume, zero + 0.004, 0.01);
        assert!(snapped);
        assert_eq!(at, zero);
        assert_eq!(value(Target::Volume, at), 0.0, "exactly 0 dB, not near it");
        let (same, snapped) = snap_height(Target::Volume, zero + 0.03, 0.01);
        assert!(!snapped);
        assert_eq!(same, zero + 0.03);
        // Pan rests in the middle, and nothing rests on a mute lane.
        assert_eq!(snap_height(Target::Pan, 0.505, 0.01), (0.5, true));
        assert!(!snap_height(Target::Mute, 0.5, 0.2).1);
        // The nearer of two wins.
        let (at, _) = snap_height(Target::Pan, 0.76, 0.5);
        assert_eq!(at, 1.0);
        let (at, _) = snap_height(Target::Pan, 0.74, 0.5);
        assert_eq!(at, 0.5);
    }

    #[test]
    fn a_mute_lane_has_two_heights() {
        assert_eq!(value(Target::Mute, 0.2), 0.0);
        assert_eq!(value(Target::Mute, 0.5), 1.0);
        assert_eq!(value(Target::Mute, 0.9), 1.0);
    }

    #[test]
    fn values_read_in_the_units_of_their_controls() {
        assert_eq!(text(Target::Volume, 0.0), "0.0 dB");
        assert_eq!(text(Target::Volume, -6.0), "\u{2212}6.0 dB");
        assert_eq!(text(Target::Volume, FLOOR_DB), "\u{2212}\u{221E} dB");
        assert_eq!(text(Target::Pan, 0.0), "C");
        assert_eq!(text(Target::Mute, 1.0), "Muted");
        assert_eq!(text(Target::Mute, 0.0), "Heard");
        assert_eq!(
            text(
                Target::Device {
                    device: Id(1),
                    param: 0
                },
                0.25
            ),
            "25%"
        );
    }

    #[test]
    fn a_track_offers_its_own_values_and_one_send_per_return() {
        let mut track = Track::new(
            Id(1),
            "T".to_owned(),
            lumen_project::model::TrackKind::Audio,
            lumen_project::model::Color(0),
        );
        assert_eq!(
            offered(&track),
            [Target::Volume, Target::Pan, Target::Trim, Target::Mute]
        );
        track.sends.push(lumen_project::model::Send {
            to: Id(9),
            level: lumen_project::model::Level(0.0),
            pre_fader: false,
        });
        assert_eq!(offered(&track).last(), Some(&Target::Send { to: Id(9) }));
    }
}
