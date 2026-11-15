use lumen_core::time::Ticks;
use lumen_project::model::{self, ClipContent};

use super::notes::{loop_pattern, run_notes};
use super::{Member, extent, runs};

pub(crate) fn sounding_keys(project: &model::Project, track: &model::Track, at: Ticks) -> Vec<u8> {
    let map = project.drum_map_of(track);
    let members = track
        .clips
        .iter()
        .filter(|c| !c.muted && c.length.0 > 0 && matches!(c.content, ClipContent::Midi { .. }))
        .map(|clip| Member::own(clip, clip.length, map));
    let mut keys = Vec::new();
    for run in runs(members) {
        for pitch in held_in(&run, at) {
            if !keys.contains(&pitch) {
                keys.push(pitch);
            }
        }
    }
    keys
}

fn held_in(run: &[Member<'_>], at: Ticks) -> Vec<u8> {
    let map = run[0].map;
    let (start, length, period) = extent(run);
    let local = (at - start).0;
    if local < 0 || local >= length.0 {
        return Vec::new();
    }
    let (notes, local) = if let Some(period) = period {
        (
            loop_pattern(run[0].clip, period, map, run[0].dest)
                .0
                .to_vec(),
            (run[0].clip.loop_phase.0 + local).rem_euclid(period.0),
        )
    } else {
        (run_notes(run), local)
    };
    let mut keys = Vec::new();
    for n in notes {
        let (from, to) = (n.start.0, n.start.0 + n.length.0.max(1));
        let to = period.map_or(to, |p| to.min(p.0));
        if (from..to.max(from + 1)).contains(&local) && !keys.contains(&n.pitch) {
            keys.push(n.pitch);
        }
    }
    keys
}

pub(crate) fn sounding_inputs(
    project: &model::Project,
    track: &model::Track,
    at: Ticks,
) -> Vec<u8> {
    let Some(map) = project.drum_map_of(track) else {
        return Vec::new();
    };
    let members = track
        .clips
        .iter()
        .filter(|c| !c.muted && c.length.0 > 0 && matches!(c.content, ClipContent::Midi { .. }))
        .map(|clip| Member::own(clip, clip.length, None));
    let mut inputs = Vec::new();
    for run in runs(members) {
        for pitch in held_in(&run, at) {
            let heard = map.sound(pitch).is_none_or(|sound| map.heard(sound));
            if heard && !inputs.contains(&pitch) {
                inputs.push(pitch);
            }
        }
    }
    inputs
}
