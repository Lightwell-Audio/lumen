//! Bounce (M7b slice E4), the pure parts. A track is rendered with its own
//! sound into a new audio file, and the file goes onto the track (Replace) or
//! onto a new audio track after it. The rendering is the app's (it runs on a
//! worker, with the track's plugins in hosts of its own); this module says
//! which tracks go, over what span, and what the project becomes.
//!
//! The span (owner's B4): a time range if there is one, else from the first
//! selected clip's start to the last one's end, else the whole track, from its
//! first clip's start to its last clip's end.
//!
//! With effects the file holds the track's devices, trim, polarity, fader, pan
//! and own lanes (B2). Replace then leaves them off the track so they do not
//! process the file a second time (B5), which is only possible when the span
//! holds everything the track plays; otherwise the file goes to a new track
//! and the part says so. A MIDI track cannot hold the audio: its file goes on
//! a new audio track too, and Replace mutes the clips it was made from.

use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::model::{Clip, ClipContent, Color, Id, Media, Project, Track, TrackKind};

use super::overwrite;

/// Where the file goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Onto the track, over what it covers; or, on a MIDI track, onto a new
    /// audio track with the clips it was made from muted.
    Replace,
    /// Onto a new audio track after the source, which is left as it is.
    NewTrack,
}

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub mode: Mode,
    /// The track's devices, trim, polarity, fader, pan and lanes are in the
    /// file. Off: its own sound, unprocessed.
    pub effects: bool,
    /// What rings on after the span is in the file.
    pub tail: bool,
}

/// What a bounce covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope<'a> {
    /// A time range (it is `start..end`).
    Range(Ticks, Ticks),
    /// The span of these clips, track by track.
    Clips(&'a [Id]),
    /// Everything on the track.
    Track,
}

/// One track's bounce.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub track: Id,
    /// The track as it was when the part was planned. A part is applied only
    /// if the track is still exactly this.
    pub planned: Arc<Track>,
    pub start: Ticks,
    pub end: Ticks,
    /// The new clip's name, and the new track's.
    pub name: String,
    pub color: Option<Color>,
    /// What the part really does, which may differ from what was asked for.
    pub mode: Mode,
    pub effects: bool,
    pub tail: bool,
    /// Replace was asked for but the file goes to a new track (why:
    /// [`Part::moved`]).
    pub moved: Option<Moved>,
    /// Replace on a track that holds everything in the span: its devices,
    /// trim, polarity, fader, pan and lanes go off, as they are in the file.
    pub bypass_track: bool,
    /// Replace on a MIDI track: the clips in the span are muted.
    pub mute_source: bool,
}

/// Why Replace became a new track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// A MIDI track holds no audio.
    Midi,
    /// The track plays more than the span, so its devices cannot be taken
    /// off for the file.
    PartOfTrack,
    /// A locked clip lies in the span.
    Locked,
}

impl Part {
    pub fn moved(&self) -> Option<Moved> {
        self.moved
    }
}

/// The parts for `tracks` (audio and MIDI tracks that play something in the
/// span, in project order). A MIDI track sent to a hardware output has no
/// sound of its own and is left out.
pub fn plan(project: &Project, tracks: &[Id], scope: Scope<'_>, options: Options) -> Vec<Part> {
    let mut parts = Vec::new();
    for track in &project.tracks {
        if !tracks.contains(&track.id) || !playable(track) {
            continue;
        }
        let Some((start, end)) = span(track, scope) else {
            continue;
        };
        let inside: Vec<&Arc<Clip>> = track
            .clips
            .iter()
            .filter(|c| c.start < end && c.start + c.length > start)
            .collect();
        if inside.is_empty() {
            continue;
        }
        let midi = track.kind == TrackKind::Midi;
        let covers_all = track
            .clips
            .iter()
            .all(|c| c.start >= start && c.start + c.length <= end);
        let locked = inside.iter().any(|c| c.locked);
        let moved = match options.mode {
            Mode::NewTrack => None,
            Mode::Replace if midi => Some(Moved::Midi),
            Mode::Replace if locked => Some(Moved::Locked),
            Mode::Replace if options.effects && !covers_all => Some(Moved::PartOfTrack),
            Mode::Replace => None,
        };
        // A MIDI track's audio always goes to a new track; it is muted only
        // when the user asked to replace.
        let mode = if moved.is_some() {
            Mode::NewTrack
        } else {
            options.mode
        };
        let replacing = options.mode == Mode::Replace;
        parts.push(Part {
            track: track.id,
            planned: Arc::clone(track),
            start,
            end,
            name: named(&inside[0].name, &track.name),
            color: inside[0].color,
            mode,
            effects: options.effects,
            tail: options.tail,
            moved,
            bypass_track: mode == Mode::Replace && options.effects,
            mute_source: midi && replacing && !locked,
        });
    }
    parts
}

/// Whether a bounce can make sound from the track.
fn playable(track: &Track) -> bool {
    match track.kind {
        TrackKind::Audio => true,
        TrackKind::Midi => track.midi_to.is_none(),
        _ => false,
    }
}

/// The span `scope` gives on `track`, if there is one.
fn span(track: &Track, scope: Scope<'_>) -> Option<(Ticks, Ticks)> {
    match scope {
        Scope::Range(start, end) => (end > start).then_some((start, end)),
        Scope::Clips(ids) => {
            let picked = track.clips.iter().filter(|c| ids.contains(&c.id));
            let start = picked.clone().map(|c| c.start).min()?;
            let end = picked.map(|c| c.start + c.length).max()?;
            Some((start, end))
        }
        Scope::Track => {
            let start = track.clips.iter().map(|c| c.start).min()?;
            let end = track.clips.iter().map(|c| c.start + c.length).max()?;
            Some((start, end))
        }
    }
}

/// The name of something made from `name`, or the track's when the clip has
/// none.
fn named(name: &str, track: &str) -> String {
    const SUFFIX: &str = " (bounced)";
    let base = if name.trim().is_empty() { track } else { name };
    if base.ends_with(SUFFIX) {
        base.to_owned()
    } else {
        format!("{base}{SUFFIX}")
    }
}

/// A part and the file rendered for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Baked {
    pub part: Part,
    pub media: Media,
    /// Where the file ends, with its tail: at least `part.end`.
    pub end: Ticks,
}

/// Whether the track of `part` is still exactly as it was planned.
pub fn is_current(project: &Project, part: &Part) -> bool {
    project
        .tracks
        .iter()
        .find(|t| t.id == part.track)
        .is_some_and(|t| Arc::ptr_eq(t, &part.planned) || **t == *part.planned)
}

/// What a bounce made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// The new clips, in the order of `baked`.
    pub clips: Vec<Id>,
    /// The new tracks, where the files went to one.
    pub tracks: Vec<Id>,
}

/// Puts each file into the project as its part says and registers it as
/// media. Parts whose track changed are skipped.
pub fn apply(project: &mut Project, baked: &[Baked], fresh: &mut impl FnMut() -> Id) -> Made {
    let mut made = Made {
        clips: Vec::new(),
        tracks: Vec::new(),
    };
    for Baked { part, media, end } in baked {
        if !is_current(project, part) {
            continue;
        }
        let clip_id = fresh();
        let clip = Arc::new(Clip {
            id: clip_id,
            name: part.name.clone(),
            start: part.start,
            length: (*end - part.start).max(part.end - part.start).max(Ticks(1)),
            muted: false,
            color: part.color,
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
            content: ClipContent::Audio {
                media: media.id,
                offset: 0,
                gain_db: 0.0,
            },
        });
        if !project.media.iter().any(|m| m.id == media.id) {
            project.media.push(media.clone());
        }
        match part.mode {
            Mode::Replace => {
                replace(project, part, clip, fresh);
                made.clips.push(clip_id);
            }
            Mode::NewTrack => {
                let track = fresh();
                put_on_new_track(project, part, track, clip);
                made.clips.push(clip_id);
                made.tracks.push(track);
            }
        }
        if part.mute_source {
            mute_clips(project, part);
        }
    }
    made
}

/// The file onto the source track, over what it covers. The tail stops at the
/// next clip: what was not bounced is not replaced.
fn replace(project: &mut Project, part: &Part, clip: Arc<Clip>, fresh: &mut impl FnMut() -> Id) {
    let id = clip.id;
    let Some(track) = project.tracks.iter_mut().find(|t| t.id == part.track) else {
        return;
    };
    let track = Arc::make_mut(track);
    let mut clip = clip;
    if let Some(next) = track
        .clips
        .iter()
        .filter(|c| c.start >= part.end)
        .map(|c| c.start)
        .min()
    {
        let room = (next - part.start).max(part.end - part.start);
        if clip.length > room {
            Arc::make_mut(&mut clip).length = room;
        }
    }
    track.clips.push(clip);
    track.clips.sort_by_key(|c| c.start);
    if part.bypass_track {
        track.trim_db = 0.0;
        track.invert = false;
        track.gain_db = 0.0;
        track.pan = 0.0;
        for device in &mut track.devices {
            device.bypass = true;
        }
        for lane in &mut track.automation {
            lane.enabled = false;
        }
    }
    overwrite(project, &[id], fresh);
}

/// A new audio track after the source (and its group's other tracks), in the
/// source's group, holding the file.
fn put_on_new_track(project: &mut Project, part: &Part, id: Id, clip: Arc<Clip>) {
    let Some(at) = project.tracks.iter().position(|t| t.id == part.track) else {
        return;
    };
    let source = Arc::clone(&project.tracks[at]);
    let index = crate::groups::span(&project.tracks, at).end;
    let mut track = Track::new(id, part.name.clone(), TrackKind::Audio, source.color);
    track.parent = source.parent;
    track.clips = vec![clip];
    project.tracks.insert(index, Arc::new(track));
    crate::groups::normalize(&mut project.tracks);
}

/// Mutes the source's clips that the span covers (a MIDI track's, once its
/// sound is on an audio track).
fn mute_clips(project: &mut Project, part: &Part) {
    let Some(track) = project.tracks.iter_mut().find(|t| t.id == part.track) else {
        return;
    };
    let track = Arc::make_mut(track);
    for clip in &mut track.clips {
        if clip.start < part.end && clip.start + clip.length > part.start && !clip.muted {
            Arc::make_mut(clip).muted = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{ids, q, song};

    fn media(id: u64) -> Media {
        Media {
            id: Id(id),
            path: "bounced.wav".into(),
            original_path: None,
            frames: 480_000,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 0,
        }
    }

    const REPLACE: Options = Options {
        mode: Mode::Replace,
        effects: false,
        tail: true,
    };

    fn new_track(effects: bool) -> Options {
        Options {
            mode: Mode::NewTrack,
            effects,
            tail: true,
        }
    }

    /// Track 0 with clips at 0..2, 3..5 and 8..10 (quarters).
    fn three_clips() -> Project {
        let mut project = song();
        let track = Arc::make_mut(&mut project.tracks[0]);
        let template = track.clips[0].clone();
        track.clips = [(1, 0, 2), (2, 3, 2), (3, 8, 2)]
            .into_iter()
            .map(|(id, start, length)| {
                let mut clip = (*template).clone();
                clip.id = Id(id);
                clip.start = q(start);
                clip.length = q(length);
                Arc::new(clip)
            })
            .collect();
        project
    }

    fn baked(part: Part, end: Ticks) -> Baked {
        Baked {
            part,
            media: media(777),
            end,
        }
    }

    fn all(project: &Project) -> Vec<Id> {
        project.tracks.iter().map(|t| t.id).collect()
    }

    #[test]
    fn the_scope_is_the_range_else_the_selected_clips_else_the_whole_track() {
        let project = three_clips();
        let track = project.tracks[0].id;
        let one = [track];
        let part = |scope| plan(&project, &one, scope, REPLACE).remove(0);
        let range = part(Scope::Range(q(1), q(4)));
        assert_eq!((range.start, range.end), (q(1), q(4)));
        let clips = part(Scope::Clips(&[Id(2), Id(3)]));
        assert_eq!((clips.start, clips.end), (q(3), q(10)));
        let whole = part(Scope::Track);
        assert_eq!((whole.start, whole.end), (q(0), q(10)));
        assert_eq!(whole.name, "c10 (bounced)");
    }

    #[test]
    fn a_track_with_nothing_in_the_span_or_that_cannot_sound_is_left_out() {
        let project = three_clips();
        let track = project.tracks[0].id;
        assert!(plan(&project, &[track], Scope::Range(q(5), q(8)), REPLACE).is_empty());
        assert!(plan(&project, &[track], Scope::Clips(&[Id(999)]), REPLACE).is_empty());
        assert!(plan(&project, &[], Scope::Track, REPLACE).is_empty());
        let groups = plan(&project, &all(&project), Scope::Track, REPLACE);
        assert!(
            groups.iter().all(|p| {
                let kind = project
                    .tracks
                    .iter()
                    .find(|t| t.id == p.track)
                    .unwrap()
                    .kind;
                matches!(kind, TrackKind::Audio | TrackKind::Midi)
            }),
            "only audio and MIDI tracks"
        );
    }

    #[test]
    fn replace_with_effects_needs_the_span_to_hold_everything_else_it_is_a_new_track() {
        let project = three_clips();
        let track = [project.tracks[0].id];
        let effects = Options {
            effects: true,
            ..REPLACE
        };
        let part = plan(&project, &track, Scope::Range(q(0), q(5)), effects).remove(0);
        assert_eq!(part.mode, Mode::NewTrack);
        assert_eq!(part.moved(), Some(Moved::PartOfTrack));
        assert!(!part.bypass_track);
        let whole = plan(&project, &track, Scope::Track, effects).remove(0);
        assert_eq!((whole.mode, whole.moved()), (Mode::Replace, None));
        assert!(whole.bypass_track);
        // Without effects a part of the track is replaced as it is.
        let plain = plan(&project, &track, Scope::Range(q(0), q(5)), REPLACE).remove(0);
        assert_eq!((plain.mode, plain.bypass_track), (Mode::Replace, false));
    }

    #[test]
    fn a_locked_clip_in_the_span_is_never_replaced() {
        let mut project = three_clips();
        let track = Arc::make_mut(&mut project.tracks[0]);
        let mut locked = (*track.clips[1]).clone();
        locked.locked = true;
        track.clips[1] = Arc::new(locked);
        let id = [project.tracks[0].id];
        let part = plan(&project, &id, Scope::Track, REPLACE).remove(0);
        assert_eq!(
            (part.mode, part.moved()),
            (Mode::NewTrack, Some(Moved::Locked))
        );
    }

    #[test]
    fn replacing_puts_one_clip_over_the_span_and_keeps_the_rest() {
        let mut project = three_clips();
        let id = [project.tracks[0].id];
        let part = plan(&project, &id, Scope::Range(q(0), q(5)), REPLACE).remove(0);
        let made = apply(&mut project, &[baked(part, q(5))], &mut ids());
        assert_eq!(made.clips.len(), 1);
        assert!(made.tracks.is_empty());
        let starts: Vec<_> = project.tracks[0]
            .clips
            .iter()
            .map(|c| (c.start, c.length))
            .collect();
        assert_eq!(starts, [(q(0), q(5)), (q(8), q(2))]);
        assert!(project.media.iter().any(|m| m.id == Id(777)));
        assert!(
            project.media.iter().any(|m| m.id == Id(900)),
            "the original stays"
        );
    }

    #[test]
    fn the_tail_stops_at_the_next_clip_when_replacing_and_runs_free_on_a_new_track() {
        let mut project = three_clips();
        let id = [project.tracks[0].id];
        // The span 0..2 has a tail to 4 quarters, past the clip at 3.
        let part = plan(&project, &id, Scope::Range(q(0), q(2)), REPLACE).remove(0);
        apply(&mut project, &[baked(part, q(4))], &mut ids());
        let lengths: Vec<_> = project.tracks[0]
            .clips
            .iter()
            .map(|c| (c.start, c.length))
            .collect();
        assert_eq!(lengths, [(q(0), q(3)), (q(3), q(2)), (q(8), q(2))]);

        let mut project = three_clips();
        let part = plan(&project, &id, Scope::Range(q(0), q(2)), new_track(false)).remove(0);
        let before = project.tracks[0].clone();
        let made = apply(&mut project, &[baked(part, q(4))], &mut ids());
        assert_eq!(project.tracks[0], before, "the source is left as it is");
        let new = project
            .tracks
            .iter()
            .find(|t| t.id == made.tracks[0])
            .unwrap();
        assert_eq!(new.kind, TrackKind::Audio);
        assert_eq!(new.clips[0].length, q(4), "the tail is all there");
        assert_eq!(new.clips[0].start, q(0));
        let at = project.tracks.iter().position(|t| t.id == new.id).unwrap();
        assert_eq!(project.tracks[at - 1].id, id[0], "right after the source");
    }

    #[test]
    fn replacing_with_effects_turns_the_tracks_own_processing_off() {
        let mut project = three_clips();
        let track = Arc::make_mut(&mut project.tracks[0]);
        track.gain_db = -6.0;
        track.pan = 0.5;
        track.trim_db = 3.0;
        track.invert = true;
        let id = [track.id];
        let effects = Options {
            effects: true,
            ..REPLACE
        };
        let part = plan(&project, &id, Scope::Track, effects).remove(0);
        apply(&mut project, &[baked(part, q(10))], &mut ids());
        let track = &project.tracks[0];
        assert_eq!(
            (track.gain_db, track.pan, track.trim_db, track.invert),
            (0.0, 0.0, 0.0, false)
        );
        assert_eq!(track.clips.len(), 1);
        // A new track leaves the source's processing alone.
        let mut project = three_clips();
        Arc::make_mut(&mut project.tracks[0]).gain_db = -6.0;
        let part = plan(&project, &id, Scope::Track, new_track(true)).remove(0);
        apply(&mut project, &[baked(part, q(10))], &mut ids());
        assert_eq!(project.tracks[0].gain_db, -6.0);
    }

    /// The song with a MIDI track (the empty third) holding two clips.
    fn with_midi() -> (Project, Id) {
        let mut project = song();
        let template = project.tracks[0].clips[0].clone();
        let track = Arc::make_mut(&mut project.tracks[2]);
        track.kind = TrackKind::Midi;
        track.clips = [(41, 0), (42, 4)]
            .into_iter()
            .map(|(id, start)| {
                let mut clip = (*template).clone();
                clip.id = Id(id);
                clip.start = q(start);
                clip.length = q(2);
                clip.content = ClipContent::midi();
                Arc::new(clip)
            })
            .collect();
        let id = track.id;
        (project, id)
    }

    #[test]
    fn a_midi_track_bounces_to_a_new_audio_track_and_replace_mutes_its_clips() {
        let (mut project, midi) = with_midi();
        let part = plan(&project, &[midi], Scope::Track, REPLACE).remove(0);
        assert_eq!(
            (part.mode, part.moved()),
            (Mode::NewTrack, Some(Moved::Midi))
        );
        assert!(part.mute_source);
        let end = part.end;
        let made = apply(&mut project, &[baked(part, end)], &mut ids());
        let source = project.tracks.iter().find(|t| t.id == midi).unwrap();
        assert!(source.clips.iter().all(|c| c.muted));
        assert_eq!(made.tracks.len(), 1);
        // Asking for a new track leaves the notes playing.
        let (mut project, midi) = with_midi();
        let part = plan(&project, &[midi], Scope::Track, new_track(true)).remove(0);
        assert!(!part.mute_source);
        let end = part.end;
        apply(&mut project, &[baked(part, end)], &mut ids());
        let source = project.tracks.iter().find(|t| t.id == midi).unwrap();
        assert!(source.clips.iter().all(|c| !c.muted));
    }

    #[test]
    fn a_part_is_skipped_when_its_track_changed() {
        let mut project = three_clips();
        let id = [project.tracks[0].id];
        let part = plan(&project, &id, Scope::Track, REPLACE).remove(0);
        Arc::make_mut(&mut project.tracks[0]).gain_db = -3.0;
        assert!(!is_current(&project, &part));
        let before = project.clone();
        let made = apply(&mut project, &[baked(part, q(10))], &mut ids());
        assert!(made.clips.is_empty());
        assert_eq!(project, before);
    }
}
