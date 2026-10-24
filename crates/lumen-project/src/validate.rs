//! Semantic checks after deserialization. A file that parses but describes
//! an impossible project is rejected with a precise message; loading never
//! panics.

use std::collections::{HashMap, HashSet};

use lumen_core::meter::MeterMap;
use lumen_core::time::{TempoMap, Ticks};

use crate::edit::maps::MOST_CHANGES;
use crate::model::{
    ClipContent, Device, Id, MAX_DEVICE_STATE, Output, PluginRef, Project, Track, TrackKind,
};

/// The furthest a position or length may reach: about 3 billion beats, past
/// any song, and far enough from `i64`'s limit that sums of a few never
/// overflow. A crafted file past it is refused rather than wrapping.
pub(crate) const LONGEST: Ticks = Ticks(1 << 52);

/// Whether `t` lies within [`LONGEST`] of the start, either side of it.
pub(crate) fn within(t: Ticks) -> bool {
    t.0.unsigned_abs() <= LONGEST.0.unsigned_abs()
}

/// The most audio output buses of a plugin an output track can play (the
/// host's own limit, less the main one).
const MOST_BUSES: u32 = 31;

/// The most parameters a plugin may list, and so the most a device holds
/// values for (the host's own limit).
const MOST_PARAMS: usize = 16_384;
/// The most parameters a card's strip may name for one device.
const MOST_SHOWN: usize = 256;
/// The most devices the view may hold a fold or a strip for.
const MOST_VIEWED: usize = 65_536;

/// What a device may hold: bounded state, and the VST3-only data only on a
/// VST3 device (format v17). Everything here comes from a file a person may
/// have damaged or crafted.
fn device_data(device: &Device) -> Result<(), String> {
    if device.state.len() > MAX_DEVICE_STATE || device.controller_state.len() > MAX_DEVICE_STATE {
        return Err("a device's saved state is too large".into());
    }
    match &device.plugin {
        PluginRef::Vst3 {
            class_id,
            name,
            vendor,
            ..
        } => {
            if class_id.len() != 32 || !class_id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("a plugin's class id is not 32 hex digits".into());
            }
            if name.is_empty() || name.chars().count() > 256 || vendor.chars().count() > 256 {
                return Err("a plugin's name or vendor is empty or too long".into());
            }
            if device.plugin_version.chars().count() > 64 {
                return Err("a plugin's version is too long".into());
            }
            if device.params.len() > MOST_PARAMS
                || device.params.values().any(|v| !(0.0..=1.0).contains(v))
            {
                return Err("a plugin's parameter values are out of range".into());
            }
            if device.sidechain.is_some() && device.is_instrument() {
                return Err("an instrument takes no sidechain".into());
            }
            if let Some(reported) = &device.reported
                && (!(8_000..=384_000).contains(&reported.rate) || reported.latency > 1 << 24)
            {
                return Err("a plugin's reported latency or rate is out of range".into());
            }
        }
        PluginRef::Builtin { .. } => {
            if !device.controller_state.is_empty()
                || !device.plugin_version.is_empty()
                || device.reported.is_some()
                || !device.params.is_empty()
                || device.sidechain.is_some()
            {
                return Err("a built-in device holds plugin-only data".into());
            }
        }
    }
    Ok(())
}

pub fn validate(project: &Project) -> Result<(), String> {
    if project.view.shown_params.len() > MOST_VIEWED
        || project.view.folded_devices.len() > MOST_VIEWED
    {
        return Err("the view names too many devices".into());
    }
    if project
        .view
        .shown_params
        .values()
        .any(|shown| shown.len() > MOST_SHOWN)
    {
        return Err("a device's strip names too many parameters".into());
    }
    if !(8_000..=384_000).contains(&project.sample_rate) {
        return Err(format!(
            "sample rate {} is out of range",
            project.sample_rate
        ));
    }
    TempoMap::new(project.tempo.clone()).map_err(|e| format!("tempo: {e}"))?;
    MeterMap::new(project.meter.clone()).map_err(|e| format!("meter: {e}"))?;
    if project.tempo.len() > MOST_CHANGES || project.meter.len() > MOST_CHANGES {
        return Err("the song has too many tempo or time-signature changes".into());
    }
    if project.tempo.iter().any(|p| !within(p.at)) {
        return Err("a tempo change is out of range".into());
    }
    if let Some(point) = project
        .meter
        .iter()
        .find(|p| !(1..=64).contains(&p.meter.numerator) || !within(p.at))
    {
        return Err(format!(
            "time signature {} at {} is out of range",
            point.meter, point.at.0
        ));
    }

    crate::mapping::check(&project.mappings)?;

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
    let mut outputs = HashSet::new();
    for (index, marker) in project.markers.iter().enumerate() {
        if !ids.insert(marker.id) {
            return Err(format!("marker {index} {:?}: duplicate id", marker.name));
        }
        if marker.at < Ticks::ZERO || !within(marker.at) {
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
        crate::automation::check(track).map_err(|e| format!("{}: {e}", at()))?;
        frozen(track, &media).map_err(|e| format!("{}: {e}", at()))?;
        if let Some(output) = &track.plugin_output {
            if track.kind != TrackKind::Audio {
                return Err(format!(
                    "{}: only an audio track plays a plugin's output",
                    at()
                ));
            }
            if output.bus == 0 || output.bus > MOST_BUSES {
                return Err(format!(
                    "{}: a plugin output must be bus 1 to {MOST_BUSES}",
                    at()
                ));
            }
            if !outputs.insert((output.device, output.bus)) {
                return Err(format!(
                    "{}: another track plays the same plugin output",
                    at()
                ));
            }
        }
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
        if !track.midi_from.is_default() {
            if track.kind != TrackKind::Midi {
                return Err(format!("{}: MIDI From on a track that is not MIDI", at()));
            }
            let from = &track.midi_from;
            if from.channel.is_some_and(|c| c > 15)
                || matches!(&from.source, crate::model::FromSource::Port(p) if p.is_empty())
            {
                return Err(format!(
                    "{}: MIDI From names no port or a bad channel",
                    at()
                ));
            }
        }
        let mut instruments = 0;
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
            // Where the instrument sits is an edit-time rule, not checked
            // here: a file written before v17 may hold a plugin before the
            // synth, and every older file must open.
            if device.is_instrument() {
                if track.kind != TrackKind::Midi {
                    return Err(format!(
                        "{}: an instrument on a track that is not MIDI",
                        at()
                    ));
                }
                instruments += 1;
                if instruments > 1 {
                    return Err(format!("{}: more than one instrument", at()));
                }
            }
            device_data(device).map_err(|why| format!("{}: {why}", at()))?;
            // A source that was deleted stays named: the row says so and the
            // engine plays no sidechain, so undoing the delete brings it back.
            if device.sidechain.is_some_and(|s| s.from == track.id) {
                return Err(format!("{}: its sidechain is its own track", at()));
            }
        }
        for (index, clip) in track.clips.iter().enumerate() {
            let at = || format!("{}, clip {index} {:?}", at(), clip.name);
            if clip.crossfade && !matches!(clip.content, ClipContent::Audio { .. }) {
                return Err(format!("{}: a MIDI clip cannot request a crossfade", at()));
            }
            if !ids.insert(clip.id) {
                return Err(format!("{}: duplicate clip id", at()));
            }
            if clip.start < Ticks::ZERO
                || clip.length <= Ticks::ZERO
                || !within(clip.start)
                || !within(clip.length)
            {
                return Err(format!("{}: invalid position or length", at()));
            }
            if !clip.audio_phase_seconds.is_finite() {
                return Err(format!("{}: invalid audio source phase", at()));
            }
            if clip.fade_in.length < Ticks::ZERO
                || clip.fade_out.length < Ticks::ZERO
                || !within(clip.fade_in.length)
                || !within(clip.fade_out.length)
                || clip.fade_in.length + clip.fade_out.length > clip.length
            {
                return Err(format!("{}: fades are longer than the clip", at()));
            }
            for (edge, fade) in [("fade in", &clip.fade_in), ("fade out", &clip.fade_out)] {
                let Some(end) = fade.offset.0.checked_add(fade.length.0) else {
                    return Err(format!("{}: {edge} window overflows", at()));
                };
                if fade.offset < Ticks::ZERO
                    || fade.span < Ticks::ZERO
                    || !within(fade.offset)
                    || !within(fade.span)
                    || (fade.span == Ticks::ZERO && fade.offset != Ticks::ZERO)
                    || (fade.span > Ticks::ZERO && end > fade.span.0)
                {
                    return Err(format!("{}: invalid {edge} window", at()));
                }
            }
            if !clip.fade_in.curve.is_valid() || !clip.fade_out.curve.is_valid() {
                return Err(format!("{}: a fade curve leaves its square", at()));
            }
            if clip
                .loop_length
                .is_some_and(|l| l <= Ticks::ZERO || !within(l))
            {
                return Err(format!("{}: invalid loop length", at()));
            }
            match clip.loop_length {
                Some(_) if clip.loop_phase < Ticks::ZERO || !within(clip.loop_phase) => {
                    return Err(format!("{}: invalid loop phase", at()));
                }
                None if clip.loop_phase != Ticks::ZERO => {
                    return Err(format!("{}: loop phase without a loop", at()));
                }
                _ => {}
            }
            crate::envelope::check(clip).map_err(|e| format!("{}: {e}", at()))?;
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
                    let source_rate = project
                        .media
                        .iter()
                        .find(|m| m.id == *id)
                        .map_or(project.sample_rate, |m| m.sample_rate)
                        .max(1);
                    let source_seconds =
                        *offset as f64 / f64::from(source_rate) + clip.audio_phase_seconds;
                    if source_seconds < 0.0 {
                        return Err(format!("{}: audio source phase precedes the file", at()));
                    }
                    if clip.audio_phase_seconds != 0.0
                        && let Some(file) = project.media.iter().find(|m| m.id == *id)
                        && source_seconds > file.frames as f64 / f64::from(source_rate)
                    {
                        return Err(format!("{}: audio source phase exceeds the file", at()));
                    }
                }
                ClipContent::Midi { notes, events } => {
                    if clip.audio_phase_seconds != 0.0 {
                        return Err(format!("{}: audio source phase on MIDI clip", at()));
                    }
                    if clip.reversed || clip.inverted {
                        return Err(format!("{}: reverse or polarity on MIDI clip", at()));
                    }
                    // Notes and events may lie outside the clip's window,
                    // before its start or past its end (format v15).
                    for (n, event) in events.iter().enumerate() {
                        if !event.kind.in_range() || !within(event.at) {
                            return Err(format!("{}: event {n} is out of range", at()));
                        }
                    }
                    for (n, note) in notes.iter().enumerate() {
                        if note.pitch() > 127
                            || !(1..=127).contains(&note.velocity())
                            || note.length() <= Ticks::ZERO
                            || !within(note.start())
                            || !within(note.length())
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
/// A frozen track's file: only an audio or MIDI track is frozen, its file is
/// in the media and it lies within the song.
fn frozen(track: &Track, media: &HashSet<Id>) -> Result<(), &'static str> {
    let Some(frozen) = &track.frozen else {
        return Ok(());
    };
    if !matches!(track.kind, TrackKind::Audio | TrackKind::Midi) {
        return Err("only an audio or MIDI track is frozen");
    }
    if !media.contains(&frozen.media) {
        return Err("its frozen file is missing from the media");
    }
    if frozen.length <= Ticks::ZERO
        || !within(frozen.start)
        || !within(frozen.start + frozen.length)
    {
        return Err("its frozen file lies outside the song");
    }
    Ok(())
}

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

    /// Review M5 M3: positions near `i64`'s limit are refused, never added
    /// up into an overflow.
    #[test]
    fn positions_past_the_longest_song_are_refused() {
        let far = Ticks(i64::MAX - 1);
        assert!(with_first_clip(|c| c.start = far).is_err());
        assert!(with_first_clip(|c| c.length = far).is_err());
        assert!(
            with_first_clip(|c| {
                c.fade_in.length = far;
                c.fade_out.length = far;
            })
            .is_err()
        );
        assert!(with_first_clip(|c| c.loop_length = Some(far)).is_err());
        let note_at = |start: Ticks, length: Ticks| {
            with_first_clip(|c| {
                c.content = ClipContent::Midi {
                    notes: vec![crate::model::Note::new(start, length, 60, 100)],
                    events: Vec::new(),
                };
            })
        };
        assert_eq!(note_at(Ticks(-100), Ticks(100)), Ok(()));
        assert!(note_at(far, Ticks(100)).is_err());
        assert!(note_at(Ticks(-far.0), Ticks(100)).is_err());
        assert!(note_at(Ticks::ZERO, far).is_err());
        assert_eq!(with_first_clip(|c| c.start = LONGEST), Ok(()));
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
