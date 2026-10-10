//! Semantic checks after deserialization. A file that parses but describes
//! an impossible project is rejected with a precise message; loading never
//! panics.

use std::collections::{HashMap, HashSet};

use lumen_core::time::{TempoMap, Ticks};

use crate::model::{ClipContent, Id, Output, Project, Track, TrackKind};

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
    if kinds.len() != project.tracks.len() || kinds.contains_key(&project.master.id) {
        return Err("two tracks share an id".into());
    }
    if project.tracks.iter().any(|t| t.kind == TrackKind::Master) {
        return Err("a master track among the tracks".into());
    }
    let master = &project.master;
    if master.kind != TrackKind::Master
        || master.parent.is_some()
        || !master.clips.is_empty()
        || !master.sends.is_empty()
        || matches!(master.output, Output::Master | Output::Track(_))
    {
        return Err("the master track is not a master".into());
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
    for track in crate::routing::all_tracks(project) {
        let at = || format!("track {:?}", track.name);
        // JSON cannot represent NaN or infinity; they must never reach a file.
        if !track.gain_db.is_finite()
            || !track.pan.is_finite()
            || !(-1.0..=1.0).contains(&track.pan)
            || !track.trim_db.is_finite()
        {
            return Err(format!("{}: gain or pan is not a valid number", at()));
        }
        routing(track, &kinds).map_err(|e| format!("{}: {e}", at()))?;
        if let Some(parent) = track.parent {
            if !crate::groups::can_be_grouped(track.kind) {
                return Err(format!("{}: a return or master inside a group", at()));
            }
            match kinds.get(&parent) {
                Some(TrackKind::Group) => {}
                Some(_) => return Err(format!("{}: parent is not a group track", at())),
                None => return Err(format!("{}: parent track does not exist", at())),
            }
        }
        if let Some(out) = &track.midi_to {
            if track.kind != TrackKind::Midi {
                return Err(format!("{}: MIDI To on a track that is not MIDI", at()));
            }
            if out.channel > 15 || out.port.is_empty() {
                return Err(format!("{}: MIDI To names no port or a bad channel", at()));
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
            if device.is_synth() {
                if track.kind != TrackKind::Midi {
                    return Err(format!("{}: a synth on a track that is not MIDI", at()));
                }
                if !device
                    .preset()
                    .is_some_and(|p| !p.is_empty() && p.len() <= 32)
                {
                    return Err(format!("{}: the synth's preset is unreadable", at()));
                }
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
            if clip.fade_in.length < Ticks::ZERO
                || clip.fade_out.length < Ticks::ZERO
                || clip.fade_in.length + clip.fade_out.length > clip.length
            {
                return Err(format!("{}: fades are longer than the clip", at()));
            }
            if !clip.fade_in.curve.is_valid() || !clip.fade_out.curve.is_valid() {
                return Err(format!("{}: a fade curve leaves its square", at()));
            }
            if clip.loop_length.is_some_and(|l| l <= Ticks::ZERO) {
                return Err(format!("{}: invalid loop length", at()));
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
                ClipContent::Midi { notes, events } => {
                    for (n, event) in events.iter().enumerate() {
                        if event.at < Ticks::ZERO || !event.kind.in_range() {
                            return Err(format!("{}: event {n} is out of range", at()));
                        }
                    }
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
    if !crate::groups::in_order(&project.tracks) {
        return Err("a group's tracks are not right under it".into());
    }
    if crate::routing::has_loop(project) {
        return Err("the routing feeds back into itself".into());
    }
    Ok(())
}

/// A track's output and sends go to tracks that can take them.
fn routing(track: &Track, kinds: &HashMap<Id, TrackKind>) -> Result<(), String> {
    if let Output::Track(to) = track.output {
        match kinds.get(&to) {
            _ if to == track.id => return Err("its output is itself".into()),
            Some(TrackKind::Group | TrackKind::Return) => {}
            Some(_) => return Err("its output is not a group or return".into()),
            None => return Err("its output track does not exist".into()),
        }
    }
    for (index, send) in track.sends.iter().enumerate() {
        match kinds.get(&send.to) {
            _ if send.to == track.id => return Err(format!("send {index} goes to itself")),
            Some(TrackKind::Return) => {}
            Some(_) => return Err(format!("send {index} is not to a return track")),
            None => return Err(format!("send {index} goes to a missing track")),
        }
        if !(send.level.is_silent() || send.level.0.is_finite()) {
            return Err(format!("send {index}: level is not a valid number"));
        }
        if track.sends[..index].iter().any(|s| s.to == send.to) {
            return Err(format!("send {index}: a second send to the same return"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::{Level, Send};
    use crate::testing::small_project;

    #[test]
    fn outputs_and_sends_must_go_where_audio_can_go() {
        let project = small_project(3);
        assert_eq!(validate(&project), Ok(()));
        let group = project.tracks[0].id;
        let midi = project.tracks[1].id;
        let check = |edit: &dyn Fn(&mut Project)| {
            let mut p = project.clone();
            edit(&mut p);
            validate(&p)
        };
        let set = |i: usize, output: Output| {
            move |p: &mut Project| Arc::make_mut(&mut p.tracks[i]).output = output
        };
        assert_eq!(check(&set(5, Output::Track(group))), Ok(()));
        assert!(
            check(&set(5, Output::Track(midi)))
                .unwrap_err()
                .contains("not a group")
        );
        assert!(
            check(&set(5, Output::Track(Id(999))))
                .unwrap_err()
                .contains("does not exist")
        );
        // A track in the group routed back into its own group's output loop.
        assert!(
            check(&set(0, Output::Track(group)))
                .unwrap_err()
                .contains("itself")
        );
        let send = |to| {
            move |p: &mut Project| {
                Arc::make_mut(&mut p.tracks[5]).sends.push(Send {
                    to,
                    level: Level::SILENT,
                    pre_fader: false,
                })
            }
        };
        assert!(check(&send(group)).unwrap_err().contains("not to a return"));
        let master_out = |p: &mut Project| Arc::make_mut(&mut p.master).output = Output::Master;
        assert!(check(&master_out).unwrap_err().contains("not a master"));
        let master_in_tracks = |p: &mut Project| {
            let master = p.master.clone();
            p.tracks.push(master);
        };
        assert!(check(&master_in_tracks).is_err());
    }

    fn with_first_clip(edit: impl FnOnce(&mut crate::model::Clip)) -> Result<(), String> {
        let mut project = small_project(3);
        let track = project
            .tracks
            .iter_mut()
            .find(|t| !t.clips.is_empty())
            .unwrap();
        edit(Arc::make_mut(&mut Arc::make_mut(track).clips[0]));
        validate(&project)
    }

    #[test]
    fn fades_must_fit_the_clip_and_loops_must_have_a_length() {
        assert_eq!(with_first_clip(|_| {}), Ok(()));
        let fades = with_first_clip(|c| {
            c.fade_in.length = c.length;
            c.fade_out.length = Ticks(1);
        });
        assert!(fades.unwrap_err().contains("fades are longer"));
        let looped = with_first_clip(|c| c.loop_length = Some(Ticks::ZERO));
        assert!(looped.unwrap_err().contains("invalid loop length"));
        assert_eq!(
            with_first_clip(|c| c.loop_length = Some(Ticks(c.length.0 / 3))),
            Ok(())
        );
    }
}
