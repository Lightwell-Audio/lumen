//! Semantic checks after deserialization. A file that parses but describes
//! an impossible project is rejected with a precise message; loading never
//! panics.

use std::collections::{HashMap, HashSet};

use lumen_core::time::{TempoMap, Ticks};

use crate::model::{ClipContent, Id, Project, TrackKind};

pub fn validate(project: &Project) -> Result<(), String> {
    if !(8_000..=384_000).contains(&project.sample_rate) {
        return Err(format!(
            "sample rate {} is out of range",
            project.sample_rate
        ));
    }
    TempoMap::new(project.tempo.clone()).map_err(|e| format!("tempo: {e}"))?;
    let signature = project.time_signature;
    if !(1..=64).contains(&signature.numerator)
        || !signature.denominator.is_power_of_two()
        || signature.denominator > 64
    {
        return Err(format!(
            "time signature {}/{} is invalid",
            signature.numerator, signature.denominator
        ));
    }

    let media: HashSet<Id> = project.media.iter().map(|m| m.id).collect();
    if media.len() != project.media.len() {
        return Err("two media entries share an id".into());
    }
    let kinds: HashMap<Id, TrackKind> = project.tracks.iter().map(|t| (t.id, t.kind)).collect();
    if kinds.len() != project.tracks.len() {
        return Err("two tracks share an id".into());
    }
    if project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Master)
        .count()
        > 1
    {
        return Err("more than one master track".into());
    }

    let mut ids = HashSet::new();
    for (index, marker) in project.markers.iter().enumerate() {
        if !ids.insert(marker.id) {
            return Err(format!("marker {index} {:?}: duplicate id", marker.name));
        }
        if marker.at < Ticks::ZERO {
            return Err(format!(
                "marker {index} {:?}: before the start",
                marker.name
            ));
        }
    }
    for track in &project.tracks {
        let at = || format!("track {:?}", track.name);
        // JSON cannot represent NaN or infinity; they must never reach a file.
        if !track.gain_db.is_finite()
            || !track.pan.is_finite()
            || !(-1.0..=1.0).contains(&track.pan)
        {
            return Err(format!("{}: gain or pan is not a valid number", at()));
        }
        if let Some(parent) = track.parent {
            match kinds.get(&parent) {
                Some(TrackKind::Group) => {}
                Some(_) => return Err(format!("{}: parent is not a group track", at())),
                None => return Err(format!("{}: parent track does not exist", at())),
            }
        }
        for device in &track.devices {
            if !ids.insert(device.id) {
                return Err(format!(
                    "{}: duplicate device id {:016x}",
                    at(),
                    device.id.0
                ));
            }
        }
        for (index, clip) in track.clips.iter().enumerate() {
            let at = || format!("{}, clip {index} {:?}", at(), clip.name);
            if !ids.insert(clip.id) {
                return Err(format!("{}: duplicate clip id", at()));
            }
            if clip.start < Ticks::ZERO || clip.length <= Ticks::ZERO {
                return Err(format!("{}: invalid position or length", at()));
            }
            match &clip.content {
                ClipContent::Audio {
                    media: id,
                    offset,
                    gain_db,
                } => {
                    if !gain_db.is_finite() {
                        return Err(format!("{}: gain is not a valid number", at()));
                    }
                    if !media.contains(id) {
                        return Err(format!("{}: refers to missing media", at()));
                    }
                    if *offset < 0 {
                        return Err(format!("{}: negative media offset", at()));
                    }
                }
                ClipContent::Midi { notes } => {
                    for (n, note) in notes.iter().enumerate() {
                        if note.pitch() > 127
                            || !(1..=127).contains(&note.velocity())
                            || note.start() < Ticks::ZERO
                            || note.length() <= Ticks::ZERO
                        {
                            return Err(format!("{}: note {n} is out of range", at()));
                        }
                    }
                }
            }
        }
    }

    // Group nesting must be a forest: walking up from any track terminates.
    let parents: HashMap<Id, Id> = project
        .tracks
        .iter()
        .filter_map(|t| t.parent.map(|p| (t.id, p)))
        .collect();
    for track in &project.tracks {
        let mut seen = HashSet::from([track.id]);
        let mut current = track.id;
        while let Some(&parent) = parents.get(&current) {
            if !seen.insert(parent) {
                return Err(format!(
                    "track {:?}: group nesting forms a cycle",
                    track.name
                ));
            }
            current = parent;
        }
    }
    Ok(())
}
