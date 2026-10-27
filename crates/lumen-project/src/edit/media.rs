//! Which media files anything plays (M9 slice R4b): the media pool lists the
//! rest as unused, and removing them is one undo step.
//!
//! A media entry is played by an audio clip, or by the file a frozen track
//! plays in place of its clips. The passes of a loop recording (`Takes`) are
//! windows on their clip's own media and name no file. The reversed twins of
//! a file are derived while playing and are not saved. A new place that
//! names a media id must be added to [`users`], whose matches have no
//! wildcard so that a new kind of clip content is a compiler error here.

use std::collections::{HashMap, HashSet};

use crate::model::{ClipContent, Frozen, Id, Project, Track};

/// Every media id a clip or a frozen track of `project` plays.
pub fn used(project: &Project) -> HashSet<Id> {
    counts(project).into_keys().collect()
}

/// How many clips (and frozen tracks) play each media id that something plays.
pub fn counts(project: &Project) -> HashMap<Id, usize> {
    let mut counts = HashMap::new();
    for track in project
        .tracks
        .iter()
        .chain(std::iter::once(&project.master))
    {
        users(track, &mut counts);
    }
    counts
}

fn users(track: &Track, used: &mut HashMap<Id, usize>) {
    for clip in &track.clips {
        match &clip.content {
            ClipContent::Audio { media, .. } => {
                *used.entry(*media).or_default() += 1;
            }
            ClipContent::Midi { .. } => {}
        }
    }
    if let Some(Frozen {
        id: _,
        media,
        start: _,
        length: _,
        mono: _,
    }) = &track.frozen
    {
        *used.entry(*media).or_default() += 1;
    }
}

/// The ids in `project.media` that nothing plays, in the media list's order.
pub fn unused(project: &Project) -> Vec<Id> {
    let used = used(project);
    project
        .media
        .iter()
        .map(|m| m.id)
        .filter(|id| !used.contains(id))
        .collect()
}

/// Removes the entries of `ids` that nothing plays from the media list, and
/// returns the ones it removed. An id that is played, or not in the list, is
/// left alone, so a stale list from the pool can never drop a file in use.
pub fn remove_unused(project: &mut Project, ids: &[Id]) -> Vec<Id> {
    let unused: HashSet<Id> = unused(project)
        .into_iter()
        .filter(|id| ids.contains(id))
        .collect();
    let mut removed = Vec::new();
    project.media.retain(|m| {
        let drop = unused.contains(&m.id);
        if drop {
            removed.push(m.id);
        }
        !drop
    });
    removed
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_core::time::Ticks;

    use super::*;
    use crate::model::{Clip, Color, Media, TrackKind};
    use crate::testing::project_of;

    fn audio_clip(id: u64, media: u64) -> Arc<Clip> {
        Arc::new(Clip {
            content: ClipContent::Audio {
                media: Id(media),
                offset: 0,
                gain_db: 0.0,
            },
            ..midi_clip(id)
        })
    }

    fn midi_clip(id: u64) -> Clip {
        Clip {
            id: Id(id),
            name: format!("C{id}"),
            start: Ticks::ZERO,
            length: Ticks::quarters(4),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Ticks::ZERO,
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            envelopes: Default::default(),
            takes: None,
            content: ClipContent::midi(),
        }
    }

    fn track(id: u64, kind: TrackKind, clips: Vec<Arc<Clip>>) -> Track {
        let mut t = Track::new(Id(id), format!("T{id}"), kind, Color(0));
        t.clips = clips;
        t
    }

    fn media(id: u64) -> Media {
        Media {
            id: Id(id),
            path: format!("Samples/{id}.wav"),
            original_path: None,
            frames: 48_000,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 0,
        }
    }

    fn project() -> Project {
        let mut p = project_of(
            "media",
            vec![
                track(1, TrackKind::Audio, vec![audio_clip(10, 900)]),
                track(2, TrackKind::Audio, Vec::new()),
            ],
        );
        p.media = vec![media(900), media(901), media(902)];
        p
    }

    #[test]
    fn a_clip_uses_its_media_and_nothing_else_does() {
        let p = project();
        assert_eq!(used(&p), HashSet::from([Id(900)]));
        assert_eq!(unused(&p), vec![Id(901), Id(902)]);
    }

    #[test]
    fn a_midi_clip_uses_no_media() {
        let mut p = project();
        let mut t = track(3, TrackKind::Midi, vec![Arc::new(midi_clip(30))]);
        t.frozen = None;
        p.tracks.push(Arc::new(t));
        assert_eq!(used(&p).len(), 1);
    }

    #[test]
    fn a_frozen_track_uses_its_file_even_with_no_clip_playing_it() {
        let mut p = project();
        let mut frozen = track(2, TrackKind::Audio, Vec::new());
        frozen.frozen = Some(Frozen {
            id: Id(20),
            media: Id(902),
            start: Ticks::ZERO,
            length: Ticks::quarters(4),
            mono: false,
        });
        p.tracks[1] = Arc::new(frozen);
        assert_eq!(used(&p), HashSet::from([Id(900), Id(902)]));
        assert_eq!(unused(&p), vec![Id(901)]);
    }

    #[test]
    fn a_track_folded_into_a_group_still_counts() {
        // Groups hold no clips of their own; a child's clip is on the child.
        let mut p = project();
        p.tracks
            .push(Arc::new(track(3, TrackKind::Group, Vec::new())));
        assert_eq!(used(&p), HashSet::from([Id(900)]));
    }

    #[test]
    fn a_deleted_clip_leaves_its_media_unused() {
        let mut p = project();
        Arc::make_mut(&mut p.tracks[0]).clips.clear();
        assert_eq!(unused(&p), vec![Id(900), Id(901), Id(902)]);
    }

    #[test]
    fn removing_drops_only_what_nothing_plays() {
        let mut p = project();
        // 900 is played, 901 is asked for, 777 is not in the project.
        let removed = remove_unused(&mut p, &[Id(900), Id(901), Id(777)]);
        assert_eq!(removed, vec![Id(901)]);
        let left: Vec<_> = p.media.iter().map(|m| m.id).collect();
        assert_eq!(left, vec![Id(900), Id(902)]);
    }
}
