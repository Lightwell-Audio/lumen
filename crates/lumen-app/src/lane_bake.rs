//! Lanes on the device's sample clock (M7 slices A0 and A3, D3): a lane is
//! kept in ticks, in the units of its target, and the engine plays it in
//! samples, so the interface bakes it with the tempo map and the device's
//! rate whenever it builds a routing or a MIDI list. Pure, so it is tested
//! like the rest of the translation.

use lumen_core::automation::{Lane, Shape};
use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{AutoLane, AutoPoint, ParamLane};
use lumen_project::automation::Target;
use lumen_project::model::{Device, Id, Track};

use crate::document::Overrides;

/// Frames between the points a curved segment is resampled to when the tempo
/// changes along it (about 21 ms at 48 kHz).
const RESAMPLE: i64 = 1024;
/// The same along a tempo ramp, where the tick is a different number of frames
/// at each point of the segment (about 5 ms at 48 kHz).
const RAMP_RESAMPLE: i64 = 256;

/// `lane` on the sample clock of a device at `rate`, tempo as `tempo`: each
/// point at the sample the tempo puts it at. A segment is straight in music
/// time, so one that a change of tempo falls inside is not straight in samples:
/// a straight segment is split at the change (each part is straight again) and
/// a curved one is resampled to short straight parts.
pub(crate) fn bake(lane: &Lane, tempo: &TempoMap, rate: u32) -> Option<AutoLane> {
    let model = lane.points();
    let at = |t: Ticks| tempo.sample_at(t, rate);
    let mut baked = Vec::with_capacity(model.len());
    for (i, point) in model.iter().enumerate() {
        let next = model.get(i + 1);
        let changes: Vec<Ticks> = match next {
            Some(next) if point.shape != Shape::Step => tempo
                .points()
                .iter()
                .map(|t| t.at)
                .filter(|&t| t > point.at && t < next.at)
                .collect(),
            _ => Vec::new(),
        };
        // Along a ramp a tick is not a fixed number of samples either, so a
        // segment that runs over one is cut every so often as well.
        let ramped = next.is_some_and(|next| {
            point.shape != Shape::Step && tempo.ramps_between(point.at, next.at)
        });
        let shape = match point.shape {
            Shape::Curve(_) if !changes.is_empty() || ramped => Shape::Linear,
            shape => shape,
        };
        baked.push(AutoPoint {
            at: at(point.at),
            value: point.value,
            shape,
        });
        if let (Some(next), false) = (next, changes.is_empty() && !ramped) {
            let (from, to) = (at(point.at), at(next.at));
            // The parts are cut at each change of tempo, where the shape of the
            // segment in samples has a corner; a curve, or any segment over a
            // ramp, is cut every so often as well.
            let mut inside: Vec<i64> = changes.iter().map(|&t| at(t)).collect();
            if matches!(point.shape, Shape::Curve(_)) || ramped {
                let every = if ramped { RAMP_RESAMPLE } else { RESAMPLE };
                inside.extend((1..).map(|k| from + k * every).take_while(|&s| s < to));
                inside.sort_unstable();
                inside.dedup();
            }
            for sample in inside {
                let value = lane
                    .value_at(tempo.ticks_at_sample(sample, rate))
                    .unwrap_or(point.value);
                baked.push(AutoPoint {
                    at: sample,
                    value,
                    shape: Shape::Linear,
                });
            }
        }
    }
    AutoLane::new(baked)
}

/// `lane`, whose points are measured from `origin` (a clip's start), on the
/// sample clock that starts at `origin`: its first frame is frame 0.
pub(crate) fn bake_from(
    lane: &Lane,
    origin: Ticks,
    tempo: &TempoMap,
    rate: u32,
) -> Option<AutoLane> {
    let points = lane
        .points()
        .iter()
        .map(|p| lumen_core::automation::Point {
            at: p.at + origin,
            ..*p
        })
        .collect();
    let absolute = Lane::new(points).ok()?;
    let base = tempo.sample_at(origin, rate);
    let baked = bake(&absolute, tempo, rate)?;
    AutoLane::new(
        baked
            .points()
            .iter()
            .map(|p| AutoPoint {
                at: p.at - base,
                ..*p
            })
            .collect(),
    )
}

/// The lanes of the parameters of `track`'s devices that `wanted` picks,
/// baked: the device's id and the lane of one of its parameters. Only lanes
/// that play (are on and not taken over), for a device the track has.
pub(crate) fn device_lanes(
    track: &Track,
    over: &Overrides,
    tempo: &TempoMap,
    rate: u32,
    wanted: impl Fn(&Device) -> bool,
) -> Vec<(Id, ParamLane)> {
    track
        .automation
        .iter()
        .filter(|entry| over.plays(track, entry))
        .filter_map(|entry| {
            let Target::Device { device, param } = entry.target else {
                return None;
            };
            let held = track.devices.iter().find(|d| d.id == device)?;
            if !wanted(held) {
                return None;
            }
            let lane = bake(&entry.lane, tempo, rate)?;
            Some((device, ParamLane { param, lane }))
        })
        .collect()
}
