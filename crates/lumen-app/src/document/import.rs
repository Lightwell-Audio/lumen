//! Importing files (M3 slices 3 and 6e): loaded audio becomes clips on new
//! tracks, or on the track it was dropped on, as one undo step.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent, Id, Media, Project, Track, TrackKind};

use super::{Document, clips, seconds_at, ticks_at_seconds, track_color};

/// An audio file that finished loading, ready to become a track.
#[derive(Debug, Clone)]
pub struct Imported {
    pub media: Id,
    pub path: PathBuf,
    pub name: String,
    /// The file's own format.
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub size_bytes: u64,
    /// Where the clip starts.
    pub at: Ticks,
    /// Where it goes (slice 6e).
    pub into: DropInto,
}

/// Where dropped files go (M3 slice 6e).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropInto {
    /// Each file on a new audio track, before the track `before` (after
    /// the last one when `None`), inside `group` (M4 slice 3; `None`: at the
    /// top). A place outside the group moves to the end of the group.
    NewTracks {
        before: Option<Id>,
        group: Option<Id>,
    },
    /// Onto an audio track, one after another, cutting away what they
    /// cover (Ableton's overlap rule).
    Track(Id),
}

/// Where Import audio puts files (M3 review H1): onto the chosen audio
/// track, the last selected track or the track of a selected clip; when
/// that is not an audio track, on new tracks right after it (and what is
/// inside it), in its group, or inside it when it is a group; with nothing
/// chosen, on new tracks at the end. `chosen` is the last selected track,
/// or the track of the first selected clip.
pub fn import_place(project: &Project, chosen: Option<Id>) -> DropInto {
    let Some(i) = chosen.and_then(|id| project.tracks.iter().position(|t| t.id == id)) else {
        return DropInto::NewTracks {
            before: None,
            group: None,
        };
    };
    let track = &project.tracks[i];
    let end = lumen_project::groups::span(&project.tracks, i).end;
    match track.kind {
        TrackKind::Audio => DropInto::Track(track.id),
        kind => DropInto::NewTracks {
            before: project.tracks.get(end).map(|t| t.id),
            group: if kind == TrackKind::Group {
                Some(track.id)
            } else {
                track.parent
            },
        },
    }
}

impl Document {
    /// Adds loaded files as one undo step: each on a new audio track, or
    /// onto the audio track it was dropped on (slice 6e). Files dropped
    /// together on one track follow one another from where they were
    /// dropped. A track that has gone, or is not an audio track, gets new
    /// tracks instead.
    pub fn import(&mut self, files: Vec<Imported>) {
        if files.is_empty() {
            return;
        }
        let label = match files.as_slice() {
            [one] => format!("Import {}", one.name),
            many => format!("Import {} files", many.len()),
        };
        let mut additions = Vec::new();
        for file in files {
            let (track, clip) = (self.fresh_id(), self.fresh_id());
            self.media_files.insert(file.media, file.path.clone());
            additions.push((track, clip, file));
        }
        let next = &mut self.next_id;
        self.history.apply(&label, None, |project| {
            let mut placed = Vec::new();
            let mut next_at: HashMap<Id, Ticks> = HashMap::new();
            for (track, clip, file) in additions {
                let target = match file.into {
                    DropInto::Track(id) => project
                        .tracks
                        .iter()
                        .position(|t| t.id == id && t.kind == TrackKind::Audio),
                    DropInto::NewTracks { .. } => None,
                };
                let at = match target {
                    Some(i) => next_at
                        .get(&project.tracks[i].id)
                        .map_or(file.at, |&after| after.max(file.at)),
                    None => file.at,
                };
                let start_seconds = seconds_at(project, at);
                let seconds = file.frames as f64 / f64::from(file.sample_rate.max(1));
                let end = ticks_at_seconds(project, start_seconds + seconds);
                project.media.push(Media {
                    id: file.media,
                    path: file.path.to_string_lossy().into_owned(),
                    original_path: None,
                    frames: file.frames,
                    channels: file.channels,
                    sample_rate: file.sample_rate,
                    size_bytes: file.size_bytes,
                });
                let new_clip = Arc::new(Clip {
                    id: clip,
                    name: file.name.clone(),
                    start: at,
                    length: (end - at).max(Ticks(1)),
                    muted: false,
                    color: None,
                    locked: false,
                    fade_in: Default::default(),
                    fade_out: Default::default(),
                    loop_length: None,
                    content: ClipContent::Audio {
                        media: file.media,
                        offset: 0,
                        gain_db: 0.0,
                    },
                });
                if let Some(i) = target {
                    let t = Arc::make_mut(&mut project.tracks[i]);
                    next_at.insert(t.id, at + new_clip.length);
                    t.clips.push(new_clip);
                    t.clips.sort_by_key(|c| c.start);
                    placed.push(clip);
                    continue;
                }
                let color = track_color(project.tracks.len());
                let (before, group) = match file.into {
                    DropInto::NewTracks { before, group } => (before, group),
                    DropInto::Track(_) => (None, None),
                };
                let group = group.filter(|g| {
                    project
                        .tracks
                        .iter()
                        .any(|t| t.id == *g && t.kind == TrackKind::Group)
                });
                let new_track = Arc::new(Track {
                    clips: vec![new_clip],
                    parent: group,
                    ..Track::new(track, file.name, TrackKind::Audio, color)
                });
                let before = before.and_then(|id| project.tracks.iter().position(|t| t.id == id));
                match before {
                    Some(i) => project.tracks.insert(i, new_track),
                    None => project.tracks.push(new_track),
                }
            }
            lumen_project::groups::normalize(&mut project.tracks);
            lumen_project::edit::overwrite(project, &placed, &mut clips::fresh_ids(next));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::tests::file;

    #[test]
    fn import_goes_onto_the_chosen_audio_track_or_new_tracks_after_it() {
        let mut doc = Document::new(crate::empty_project());
        let audio = doc.add_track(TrackKind::Audio, None);
        let midi = doc.add_track(TrackKind::Midi, Some(audio));
        let last = doc.add_track(TrackKind::Audio, Some(midi));
        let project = doc.project();
        assert_eq!(import_place(project, Some(audio)), DropInto::Track(audio));
        assert_eq!(
            import_place(project, Some(midi)),
            DropInto::NewTracks {
                before: Some(last),
                group: None
            },
            "right after the MIDI track"
        );
        let end = DropInto::NewTracks {
            before: None,
            group: None,
        };
        assert_eq!(import_place(project, None), end, "nothing chosen");
        assert_eq!(import_place(project, Some(Id(9_999))), end, "a track gone");
        doc.delete_tracks(&[last]);
        assert_eq!(
            import_place(doc.project(), Some(midi)),
            end,
            "the last track"
        );
    }

    #[test]
    fn files_land_on_the_track_or_between_the_tracks_they_were_dropped_on() {
        let mut doc = Document::new(crate::empty_project());
        let ids: Vec<Id> = (0..5).map(|_| doc.fresh_id()).collect();
        doc.import(vec![file(ids[0], "Kick", Ticks::ZERO)]);
        let kick = doc.project().tracks[0].id;
        let keys = doc.add_track(TrackKind::Midi, Some(kick));
        // At 120 BPM each two-second file lasts four quarters.
        let four = Ticks::quarters(4);

        // Two files dropped together on the kick's track, at beat 3: they
        // follow one another, and the first cuts away the kick's tail.
        let onto = |media, name, at| Imported {
            into: DropInto::Track(kick),
            ..file(media, name, at)
        };
        let steps = doc.history().steps().0.len();
        doc.import(vec![
            onto(ids[1], "Snare", Ticks::quarters(2)),
            onto(ids[2], "Hat", Ticks::quarters(2)),
        ]);
        assert_eq!(doc.history().steps().0.len(), steps + 1, "one step");
        let clips: Vec<(String, Ticks, Ticks)> = doc.project().tracks[0]
            .clips
            .iter()
            .map(|c| (c.name.clone(), c.start, c.length))
            .collect();
        let q = Ticks::quarters;
        assert_eq!(
            clips,
            [
                ("Kick".into(), Ticks::ZERO, q(2)),
                ("Snare".into(), q(2), four),
                ("Hat".into(), q(6), four),
            ]
        );

        // Onto a MIDI track: a new track instead. Between tracks: there.
        doc.import(vec![Imported {
            into: DropInto::Track(keys),
            ..file(ids[3], "Pad", Ticks::ZERO)
        }]);
        doc.import(vec![Imported {
            into: DropInto::NewTracks {
                before: Some(keys),
                group: None,
            },
            ..file(ids[4], "Bass", Ticks::ZERO)
        }]);
        let names: Vec<&str> = doc
            .project()
            .tracks
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names[0], "Kick");
        assert_eq!(names[1], "Bass", "before the MIDI track");
        assert_eq!(names[3], "Pad", "a MIDI track takes no audio");
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn files_go_into_groups_at_the_end_or_where_they_were_dropped() {
        let mut doc = Document::new(crate::empty_project());
        let kick = doc.add_track(TrackKind::Audio, None);
        let keys = doc.add_track(TrackKind::Midi, None);
        let bass = doc.add_track(TrackKind::Audio, None);
        let drums = doc.group_tracks(&[kick, keys]).unwrap();
        // Import audio with the group chosen: inside it, after its tracks.
        let into = import_place(doc.project(), Some(drums));
        assert_eq!(
            into,
            DropInto::NewTracks {
                before: Some(bass),
                group: Some(drums)
            }
        );
        // With the MIDI track inside it chosen: the same place.
        assert_eq!(import_place(doc.project(), Some(keys)), into);
        let media: Vec<Id> = (0..2).map(|_| doc.fresh_id()).collect();
        doc.import(vec![Imported {
            into,
            ..file(media[0], "Hat", Ticks::ZERO)
        }]);
        // Dropped on the group's lane: at the end of the group too.
        doc.import(vec![Imported {
            into: DropInto::NewTracks {
                before: None,
                group: Some(drums),
            },
            ..file(media[1], "Clap", Ticks::ZERO)
        }]);
        let tracks = &doc.project().tracks;
        let names: Vec<&str> = tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["Group 1", "Audio 1", "MIDI 1", "Hat", "Clap", "Audio 2"]
        );
        assert_eq!(
            (tracks[3].parent, tracks[4].parent),
            (Some(drums), Some(drums))
        );
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }
}
