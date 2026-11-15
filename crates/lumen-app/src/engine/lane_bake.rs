use lumen_core::automation::{Lane, Shape};
use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{AutoLane, AutoPoint, ParamLane};
use lumen_project::automation::Target;
use lumen_project::model::{Device, Id, Track};

use crate::document::Overrides;

const RESAMPLE: i64 = 1024;
const RAMP_RESAMPLE: i64 = 256;

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
