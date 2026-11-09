//! Importing files (M3 slices 3 and 6e): loaded audio becomes clips on new
//! tracks, or on the track it was dropped on, as one undo step.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::{PPQ, Ticks};
use lumen_media::tempo::FileTempo;
use lumen_project::edit::warp;
use lumen_project::model::{Clip, ClipContent, Id, Media, Project, Track, TrackKind, WarpMode};
use serde::{Deserialize, Serialize};

use super::{Document, clips, seconds_at, ticks_at_seconds, track_color};

/// Whether new audio clips follow the song's tempo (M9.5 S4d, the owner's
/// choice W A): the "New audio clips" preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowTempo {
    /// A clip recorded in Lumen, and an imported file that says it is a loop
    /// (a tempo in its tags or its name, not a one-shot, of 1 to 32 bars).
    /// Anything else plays at its own speed.
    #[default]
    Loops,
    /// Every new clip.
    Always,
    /// No new clip: warp is on only when the user turns it on.
    Never,
}

/// How a new clip that follows the tempo is stretched until the user chooses
/// (the mode that keeps pitch, and works on any material).
pub const FOLLOW_MODE: WarpMode = WarpMode::FullMix;

/// The longest a file may be to count as a loop: 32 bars of four beats.
const LONGEST_LOOP_BEATS: f64 = 128.0;

impl FollowTempo {
    /// How a clip recorded in Lumen is stretched; `None` leaves it unwarped.
    /// It was played to the song's tempo, so it is made at it.
    pub fn recorded(self) -> Option<WarpMode> {
        (self != Self::Never).then_some(FOLLOW_MODE)
    }

    /// The tempo an imported file of `seconds` was made at, when its clip
    /// follows the song: its own when it states one, else (under *Always*) the
    /// song's, `song_bpm`.
    pub fn imported(self, stated: Option<FileTempo>, seconds: f64, song_bpm: f64) -> Option<f64> {
        match (self, stated) {
            (Self::Never, _) => None,
            (Self::Always, stated) => Some(stated.map_or(song_bpm, |t| t.bpm)),
            (Self::Loops, Some(t)) => {
                let beats = seconds * t.bpm / 60.0;
                (!t.one_shot && (1.0..=LONGEST_LOOP_BEATS).contains(&beats)).then_some(t.bpm)
            }
            (Self::Loops, None) => None,
        }
    }
}

/// The longest file listened to for a loop's tempo, in seconds: 32 bars of
/// four beats at the slowest tempo that counts as a loop's (60 BPM), and a
/// little over. A longer file cannot be a loop of 1 to 32 bars at 60 BPM or
/// more, so it is not read.
const LONGEST_LISTENED: f64 = 130.0;

/// An imported clip that plays at its own speed because its file states no
/// tempo, while the app listens to the file for a loop's (M9.5 S5a, W A:
/// "confident detection of a file that lasts a whole number of bars").
#[derive(Debug, Clone, PartialEq)]
pub struct Awaiting {
    pub clip: Id,
    pub media: Id,
    /// The clip's length as imported; a clip edited since is left alone.
    pub length: Ticks,
    /// How long the file plays.
    pub seconds: f64,
}

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
    /// The tempo the file states (a tag or its name), read with the file.
    pub tempo: Option<FileTempo>,
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
    /// cover (the overlap rule).
    Track(Id),
}

/// Where a drop or an import puts each kind of file (M5 slice 7): audio
/// files land on audio tracks, MIDI files on MIDI tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Places {
    pub audio: DropInto,
    pub midi: DropInto,
}

impl Places {
    /// Both kinds on new tracks at the end.
    pub const END: Places = Places {
        audio: DropInto::END,
        midi: DropInto::END,
    };
}

impl DropInto {
    /// New tracks at the end, outside every group.
    pub const END: DropInto = DropInto::NewTracks {
        before: None,
        group: None,
    };
}

/// Where Import puts files that land on `lands_on` tracks (audio or MIDI;
/// M3 review H1, M5 slice 7): onto the chosen track of that kind, the last
/// selected track or the track of a selected clip; when it is another
/// kind, on new tracks right after it (and what is inside it), in its
/// group, or inside it when it is a group; with nothing chosen, on new
/// tracks at the end. `chosen` is the last selected track, or the track of
/// the first selected clip.
pub fn import_place(project: &Project, chosen: Option<Id>, lands_on: TrackKind) -> DropInto {
    let Some(i) = chosen.and_then(|id| project.tracks.iter().position(|t| t.id == id)) else {
        return DropInto::NewTracks {
            before: None,
            group: None,
        };
    };
    let track = &project.tracks[i];
    let end = lumen_project::groups::span(&project.tracks, i).end;
    match track.kind {
        kind if kind == lands_on => DropInto::Track(track.id),
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
        let mut waiting = Vec::new();
        for file in files {
            let (track, clip) = (self.fresh_id(), self.fresh_id());
            self.media_files.insert(file.media, file.path.clone());
            additions.push((track, clip, file));
        }
        let next = &mut self.next_id;
        let new_tracks = self.new_tracks;
        let follow = self.follow_tempo;
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
                let song_bpm = lumen_core::time::bpm_at(&project.tempo, at);
                // A clip that follows the tempo is as long in beats as the
                // file is at its own tempo; any other as long as it plays.
                let made_at = follow.imported(file.tempo, seconds, song_bpm);
                let end = match made_at {
                    Some(bpm) => at + Ticks((seconds * bpm / 60.0 * PPQ as f64).round() as i64),
                    None => ticks_at_seconds(project, start_seconds + seconds),
                };
                project.media.push(Media {
                    id: file.media,
                    path: file.path.to_string_lossy().into_owned(),
                    original_path: None,
                    frames: file.frames,
                    channels: file.channels,
                    sample_rate: file.sample_rate,
                    size_bytes: file.size_bytes,
                });
                let mut new_clip = Clip {
                    id: clip,
                    name: file.name.clone(),
                    start: at,
                    length: (end - at).max(Ticks(1)),
                    muted: false,
                    color: None,
                    locked: false,
                    fade_in: Default::default(),
                    crossfade: false,
                    fade_out: Default::default(),
                    loop_length: None,
                    loop_phase: Default::default(),
                    envelopes: Default::default(),
                    audio_phase_seconds: 0.0,
                    reversed: false,
                    inverted: false,
                    takes: None,
                    warp: None,
                    content: ClipContent::Audio {
                        media: file.media,
                        offset: 0,
                        gain_db: 0.0,
                    },
                };
                if let Some(bpm) = made_at
                    && let Ok(mut warped) = warp::enable(project, &new_clip, bpm)
                {
                    if let Some(on) = warped.warp.as_mut() {
                        on.mode = FOLLOW_MODE;
                    }
                    new_clip = warped;
                }
                if made_at.is_none()
                    && follow == FollowTempo::Loops
                    && file.tempo.is_none()
                    && (1.0..=LONGEST_LISTENED).contains(&seconds)
                {
                    waiting.push(Awaiting {
                        clip,
                        media: file.media,
                        length: new_clip.length,
                        seconds,
                    });
                }
                let new_clip = Arc::new(new_clip);
                if let Some(i) = target {
                    let t = Arc::make_mut(&mut project.tracks[i]);
                    next_at.insert(t.id, at + new_clip.length);
                    t.clips.push(new_clip);
                    t.clips.sort_by_key(|c| c.start);
                    placed.push(clip);
                    continue;
                }
                let color = track_color(project.new_track_color, project.tracks.len());
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
                let mut new_track = Track {
                    clips: vec![new_clip],
                    parent: group,
                    ..Track::new(track, file.name, TrackKind::Audio, color)
                };
                new_tracks.give(&mut new_track);
                let new_track = Arc::new(new_track);
                let before = before.and_then(|id| project.tracks.iter().position(|t| t.id == id));
                match before {
                    Some(i) => project.tracks.insert(i, new_track),
                    None => project.tracks.push(new_track),
                }
            }
            lumen_project::groups::normalize(&mut project.tracks);
            lumen_project::edit::overwrite(project, &placed, &mut clips::fresh_ids(next));
        });
        self.awaiting_follow.extend(waiting);
    }

    /// The imported clips waiting to be listened to, taken once.
    pub fn take_awaiting_follow(&mut self) -> Vec<Awaiting> {
        std::mem::take(&mut self.awaiting_follow)
    }

    /// The app has listened to `wait`'s file and found a loop of `beats`
    /// beats at `bpm`: the clip follows the song's tempo, as one undo step.
    /// A clip changed since it was imported (trimmed, moved into a take set,
    /// locked, warped, reversed, looped, slipped, given envelopes), one that
    /// is gone, and one that would then run into the next clip are left as
    /// they are. Returns whether the clip now follows.
    pub fn follow_detected(&mut self, wait: &Awaiting, bpm: f64) -> bool {
        let mut done = false;
        self.history
            .apply("Follow the song's tempo", None, |project| {
                done = follow_clip(project, wait, bpm);
            });
        done
    }
}

/// [`Document::follow_detected`] on a project; true when it changed the clip.
fn follow_clip(project: &mut Project, wait: &Awaiting, bpm: f64) -> bool {
    let Some((t, i)) = project.tracks.iter().enumerate().find_map(|(t, track)| {
        let i = track.clips.iter().position(|c| c.id == wait.clip)?;
        Some((t, i))
    }) else {
        return false;
    };
    let clip = &project.tracks[t].clips[i];
    let untouched = matches!(
        clip.content,
        ClipContent::Audio { media, offset: 0, .. } if media == wait.media
    ) && clip.length == wait.length
        && !clip.locked
        && clip.warp.is_none()
        && clip.takes.is_none()
        && !clip.reversed
        && clip.loop_length.is_none()
        && clip.audio_phase_seconds == 0.0
        && clip.envelopes == Default::default();
    if !untouched {
        return false;
    }
    let length = Ticks((wait.seconds * bpm / 60.0 * PPQ as f64).round() as i64).max(Ticks(1));
    let end = clip.start + length;
    let runs_into_another = project.tracks[t]
        .clips
        .iter()
        .any(|c| c.id != clip.id && c.start < end && c.start + c.length > clip.start);
    if runs_into_another {
        return false;
    }
    let mut next = (**clip).clone();
    next.length = length;
    let Ok(mut warped) = warp::enable(project, &next, bpm) else {
        return false;
    };
    if let Some(on) = warped.warp.as_mut() {
        on.mode = FOLLOW_MODE;
    }
    Arc::make_mut(&mut project.tracks[t]).clips[i] = Arc::new(warped);
    true
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
        assert_eq!(
            import_place(project, Some(audio), TrackKind::Audio),
            DropInto::Track(audio)
        );
        assert_eq!(
            import_place(project, Some(midi), TrackKind::Audio),
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
        assert_eq!(
            import_place(project, None, TrackKind::Audio),
            end,
            "nothing chosen"
        );
        assert_eq!(
            import_place(project, Some(Id(9_999)), TrackKind::Audio),
            end,
            "a track gone"
        );
        doc.delete_tracks(&[last]);
        assert_eq!(
            import_place(doc.project(), Some(midi), TrackKind::Audio),
            end,
            "the last track"
        );
        // MIDI files land on the MIDI track, and after the audio track.
        let project = doc.project();
        assert_eq!(
            import_place(project, Some(midi), TrackKind::Midi),
            DropInto::Track(midi)
        );
        assert_eq!(
            import_place(project, Some(audio), TrackKind::Midi),
            DropInto::NewTracks {
                before: Some(midi),
                group: None
            }
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
        let into = import_place(doc.project(), Some(drums), TrackKind::Audio);
        assert_eq!(
            into,
            DropInto::NewTracks {
                before: Some(bass),
                group: Some(drums)
            }
        );
        // With the MIDI track inside it chosen: the same place.
        assert_eq!(
            import_place(doc.project(), Some(keys), TrackKind::Audio),
            into
        );
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

    fn stated(bpm: f64) -> Option<FileTempo> {
        Some(FileTempo {
            bpm,
            one_shot: false,
        })
    }

    #[test]
    fn a_file_follows_the_tempo_by_the_preference_and_what_it_says_about_itself() {
        let one_shot = Some(FileTempo {
            bpm: 120.0,
            one_shot: true,
        });
        // (preference, what the file states, seconds, the answer); 120 BPM
        // is two beats a second.
        for (follow, file, seconds, want) in [
            (FollowTempo::Loops, stated(90.0), 4.0, Some(90.0)),
            (FollowTempo::Loops, None, 4.0, None),
            (FollowTempo::Loops, one_shot, 4.0, None),
            // Under a beat is a hit; over 32 bars is a song.
            (FollowTempo::Loops, stated(120.0), 0.4, None),
            (FollowTempo::Loops, stated(120.0), 0.5, Some(120.0)),
            (FollowTempo::Loops, stated(120.0), 64.0, Some(120.0)),
            (FollowTempo::Loops, stated(120.0), 64.5, None),
            (FollowTempo::Always, stated(90.0), 600.0, Some(90.0)),
            (FollowTempo::Always, one_shot, 0.1, Some(120.0)),
            (FollowTempo::Always, None, 4.0, Some(100.0)),
            (FollowTempo::Never, stated(90.0), 4.0, None),
        ] {
            assert_eq!(
                follow.imported(file, seconds, 100.0),
                want,
                "{follow:?} {file:?} {seconds}"
            );
        }
        assert_eq!(FollowTempo::Loops.recorded(), Some(FOLLOW_MODE));
        assert_eq!(FollowTempo::Always.recorded(), Some(FOLLOW_MODE));
        assert_eq!(FollowTempo::Never.recorded(), None);
    }

    fn imported_with(follow: FollowTempo, tempo: Option<FileTempo>) -> Document {
        let mut doc = Document::new(crate::empty_project());
        doc.set_follow_tempo(follow);
        let id = doc.fresh_id();
        // Two seconds of audio.
        doc.import(vec![Imported {
            tempo,
            ..file(id, "Loop", Ticks::ZERO)
        }]);
        doc
    }

    fn the_clip(doc: &Document) -> Arc<Clip> {
        Arc::clone(&doc.project().tracks[0].clips[0])
    }

    #[test]
    fn a_loop_is_as_long_in_beats_as_its_own_tempo_and_follows_the_song() {
        let doc = imported_with(FollowTempo::Loops, stated(90.0));
        let clip = the_clip(&doc);
        // Two seconds at 90 BPM are three beats, whatever the song's tempo.
        assert_eq!(clip.length, Ticks::quarters(3));
        let warp = clip.warp.as_ref().expect("a loop follows");
        assert_eq!((warp.mode, warp.original_bpm), (FOLLOW_MODE, 90.0));
        assert_eq!(warp.markers.len(), 1);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn a_file_that_does_not_follow_plays_at_its_own_speed() {
        for (follow, tempo) in [
            (FollowTempo::Never, stated(90.0)),
            (FollowTempo::Loops, None),
        ] {
            let doc = imported_with(follow, tempo);
            let clip = the_clip(&doc);
            assert!(clip.warp.is_none(), "{follow:?}");
            // Two seconds at the song's 120 BPM are four beats.
            assert_eq!(clip.length, Ticks::quarters(4));
        }
    }

    #[test]
    fn always_makes_a_file_without_a_tempo_at_the_songs_tempo() {
        let doc = imported_with(FollowTempo::Always, None);
        let clip = the_clip(&doc);
        let warp = clip.warp.as_ref().expect("always follows");
        assert_eq!(warp.original_bpm, 120.0);
        assert_eq!(clip.length, Ticks::quarters(4));
    }

    #[test]
    fn importing_a_loop_is_one_undo_step() {
        let mut doc = imported_with(FollowTempo::Loops, stated(90.0));
        assert!(doc.undo().is_some());
        assert!(doc.project().tracks.is_empty());
        assert!(doc.undo().is_none());
    }

    fn waiting(follow: FollowTempo, tempo: Option<FileTempo>, frames: u64) -> Vec<Awaiting> {
        let mut doc = Document::new(crate::empty_project());
        doc.set_follow_tempo(follow);
        let id = doc.fresh_id();
        doc.import(vec![Imported {
            tempo,
            frames,
            ..file(id, "Loop", Ticks::ZERO)
        }]);
        doc.take_awaiting_follow()
    }

    #[test]
    fn only_a_file_that_states_no_tempo_waits_to_be_listened_to_under_loops() {
        let two_seconds = 96_000;
        let waits = waiting(FollowTempo::Loops, None, two_seconds);
        assert_eq!(waits.len(), 1);
        assert_eq!(
            (waits[0].length, waits[0].seconds),
            (Ticks::quarters(4), 2.0)
        );
        for (follow, tempo, frames) in [
            (FollowTempo::Never, None, two_seconds),
            (FollowTempo::Always, None, two_seconds),
            (FollowTempo::Loops, stated(90.0), two_seconds),
            // A hit is not a loop, and a song is longer than 32 bars.
            (FollowTempo::Loops, None, 24_000),
            (FollowTempo::Loops, None, 48_000 * 131),
        ] {
            assert!(
                waiting(follow, tempo, frames).is_empty(),
                "{follow:?} {tempo:?} {frames}"
            );
        }
        let mut doc = Document::new(crate::empty_project());
        let id = doc.fresh_id();
        doc.import(vec![file(id, "Loop", Ticks::ZERO)]);
        assert_eq!(doc.take_awaiting_follow().len(), 1);
        assert!(doc.take_awaiting_follow().is_empty(), "taken once");
    }

    fn listened() -> (Document, Awaiting) {
        let mut doc = Document::new(crate::empty_project());
        let id = doc.fresh_id();
        doc.import(vec![file(id, "Loop", Ticks::ZERO)]);
        let wait = doc.take_awaiting_follow().remove(0);
        (doc, wait)
    }

    #[test]
    fn a_loop_found_in_the_file_makes_its_clip_follow_the_song_in_one_undo_step() {
        let (mut doc, wait) = listened();
        assert!(doc.follow_detected(&wait, 90.0));
        let clip = the_clip(&doc);
        // Two seconds at 90 BPM are three beats, as when the file says so.
        assert_eq!(clip.length, Ticks::quarters(3));
        let warp = clip.warp.as_ref().expect("it follows");
        assert_eq!((warp.mode, warp.original_bpm), (FOLLOW_MODE, 90.0));
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        assert!(doc.undo().is_some());
        let plain = the_clip(&doc);
        assert!(plain.warp.is_none());
        assert_eq!(plain.length, Ticks::quarters(4), "at its own speed again");
        assert_eq!(doc.project().tracks.len(), 1, "the import stays");
    }

    #[test]
    fn a_clip_changed_since_the_import_is_left_alone() {
        let id = |doc: &Document| the_clip(doc).id;
        // Locked.
        let (mut doc, wait) = listened();
        let clip = id(&doc);
        doc.set_clips_locked(&[clip], true);
        assert!(!doc.follow_detected(&wait, 90.0));
        // Warped by hand.
        let (mut doc, wait) = listened();
        let clip = id(&doc);
        doc.warp_clips(&[clip], 120.0);
        assert!(!doc.follow_detected(&wait, 90.0));
        // Trimmed.
        let (mut doc, wait) = listened();
        let base = doc.project().clone();
        let clip = id(&doc);
        doc.trim_clip(
            &base,
            clip,
            lumen_project::edit::Edge::End,
            Ticks::quarters(2),
            false,
            None,
        );
        assert!(!doc.follow_detected(&wait, 90.0));
        // Gone.
        let (mut doc, wait) = listened();
        let track = doc.project().tracks[0].id;
        doc.delete_tracks(&[track]);
        assert!(!doc.follow_detected(&wait, 90.0));
        // Not the clip of the file that was listened to.
        let (mut doc, mut wait) = listened();
        wait.media = Id(wait.media.0 + 1);
        assert!(!doc.follow_detected(&wait, 90.0));
        assert!(doc.undo().is_some(), "only the import is on the history");
        assert!(doc.project().tracks.is_empty());
    }

    #[test]
    fn a_loop_that_would_run_into_the_next_clip_is_left_alone() {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Audio, None);
        let (a, b) = (doc.fresh_id(), doc.fresh_id());
        let on = DropInto::Track(track);
        doc.import(vec![
            Imported {
                into: on,
                ..file(a, "One", Ticks::ZERO)
            },
            Imported {
                into: on,
                ..file(b, "Two", Ticks::ZERO)
            },
        ]);
        let waits = doc.take_awaiting_follow();
        assert_eq!(waits.len(), 2);
        // The first is four beats and the second follows it. At 60 BPM it
        // would be two beats shorter than at 120 BPM, which fits; at 240 BPM
        // eight beats would run into the second.
        assert!(!doc.follow_detected(&waits[0], 240.0));
        assert!(doc.follow_detected(&waits[0], 60.0));
    }
}
