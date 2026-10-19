//! The clipboard (M3 slice 6b): copied clips or a copied time range, and
//! pasting them over what is there.

use std::sync::Arc;

use lumen_core::time::Ticks;

use super::{clear, slice, sort_clips, tempo_of};
use crate::model::{Clip, ClipContent, Id, Project, TrackKind};

/// Copied clips, ready to paste: each with its track counted from the
/// topmost copied track, and its start from the copy's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Clipboard {
    pub clips: Vec<(usize, Clip)>,
    /// How long the copy lasts.
    pub length: Ticks,
    /// How many tracks it spans.
    pub tracks: usize,
    /// The tracks a copied range covers, counted from the topmost copied
    /// track. They skip tracks in between when the range was copied from a
    /// selection that is not contiguous. Empty for copied clips.
    pub rows: Vec<usize>,
    /// Copied from a time range: pasting clears the whole span on every
    /// track it lands on. Copied clips clear only under themselves.
    pub range: bool,
    /// The topmost track it was copied from (index), where it pastes back
    /// when nothing else is chosen.
    pub from_track: usize,
}

/// The selected clips `ids`, copied (locked ones too: copying changes
/// nothing).
pub fn copy_clips(project: &Project, ids: &[Id]) -> Option<Clipboard> {
    let found: Vec<(usize, &Clip)> = project
        .tracks
        .iter()
        .enumerate()
        .flat_map(|(i, t)| t.clips.iter().map(move |c| (i, &**c)))
        .filter(|(_, c)| ids.contains(&c.id))
        .collect();
    let top = found.iter().map(|(i, _)| *i).min()?;
    let bottom = found.iter().map(|(i, _)| *i).max()?;
    let origin = found.iter().map(|(_, c)| c.start).min()?;
    let end = found.iter().map(|(_, c)| c.start + c.length).max()?;
    let clips = found
        .into_iter()
        .map(|(i, c)| {
            let mut copy = c.clone();
            copy.start = c.start - origin;
            copy.locked = false;
            (i - top, copy)
        })
        .collect();
    Some(Clipboard {
        clips,
        length: end - origin,
        tracks: bottom - top + 1,
        rows: Vec::new(),
        range: false,
        from_track: top,
    })
}

/// The time range `[a, b)` on `tracks`, copied: clips cut at its edges.
pub fn copy_range(project: &Project, tracks: &[Id], a: Ticks, b: Ticks) -> Option<Clipboard> {
    let tempo = tempo_of(project);
    let rows: Vec<usize> = project
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| tracks.contains(&t.id))
        .map(|(i, _)| i)
        .collect();
    let top = *rows.first()?;
    let mut clips = Vec::new();
    for &i in &rows {
        for clip in &project.tracks[i].clips {
            if let Some(mut part) = slice(project, &tempo, clip, a, b) {
                part.start = part.start - a;
                part.locked = false;
                clips.push((i - top, part));
            }
        }
    }
    (a < b).then_some(Clipboard {
        clips,
        length: b - a,
        tracks: rows.last()? - top + 1,
        rows: rows.iter().map(|i| i - top).collect(),
        range: true,
        from_track: top,
    })
}

/// The ids of the tracks a copied range covers when pasted with its first
/// track at the index `track`: the copied ones only, so tracks the copy
/// skipped keep what they have. Tracks that do not exist are left out.
pub fn landing_tracks(project: &Project, clipboard: &Clipboard, track: usize) -> Vec<Id> {
    clipboard
        .rows
        .iter()
        .filter_map(|r| project.tracks.get(track + r).map(|t| t.id))
        .collect()
}

/// Pastes `clipboard` at `at`, its first track on the track with index
/// `track`, overwriting what is there. Clips land only on
/// tracks of their kind; the rest are left out. Returns the new clips'
/// ids.
pub fn paste(
    project: &mut Project,
    clipboard: &Clipboard,
    at: Ticks,
    track: usize,
    fresh: &mut impl FnMut() -> Id,
) -> Vec<Id> {
    let at = at.max(Ticks::ZERO);
    let fits = |row: usize, clip: &Clip| {
        project.tracks.get(row).is_some_and(|t| match clip.content {
            ClipContent::Audio { .. } => t.kind == TrackKind::Audio,
            ClipContent::Midi { .. } => t.kind == TrackKind::Midi,
        })
    };
    let landing: Vec<(usize, Clip)> = clipboard
        .clips
        .iter()
        .map(|(row, clip)| (track + row, clip))
        .filter(|(row, clip)| fits(*row, clip))
        .map(|(row, clip)| {
            let mut c = clip.clone();
            c.start = c.start + at;
            (row, c)
        })
        .collect();
    // Make room.
    if clipboard.range {
        let rows = landing_tracks(project, clipboard, track);
        clear(project, &rows, at, at + clipboard.length, fresh);
    } else {
        for (row, clip) in &landing {
            let id = project.tracks[*row].id;
            clear(project, &[id], clip.start, clip.start + clip.length, fresh);
        }
    }
    let mut ids = Vec::with_capacity(landing.len());
    for (row, mut clip) in landing {
        clip.id = fresh();
        ids.push(clip.id);
        Arc::make_mut(&mut project.tracks[row])
            .clips
            .push(Arc::new(clip));
    }
    sort_clips(project);
    ids
}

#[cfg(test)]
mod tests {
    use super::super::tests::{clips, ids, q, song};
    use super::*;

    #[test]
    fn a_copied_range_pastes_over_what_is_there() {
        let mut p = song();
        let copy = copy_range(&p, &[Id(1), Id(2)], q(4), q(8)).unwrap();
        assert_eq!((copy.length, copy.tracks), (q(4), 2));
        // Onto tracks 1 and 2 at quarter 2: track 1's clip is cut back.
        let new = paste(&mut p, &copy, q(2), 1, &mut ids());
        assert_eq!(new.len(), 2);
        assert_eq!(
            clips(&p, 1),
            [
                (20, 0, 2, 1_000),
                (new[0].0, 2, 4, 1_000),
                (1001, 6, 2, 145_000)
            ]
        );
        assert_eq!(
            p.tracks[2].clips.len(),
            1,
            "track 1's part of the copy lands on track 2"
        );
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn a_range_from_skipped_tracks_leaves_the_tracks_between_alone() {
        let mut p = song();
        // Tracks 1 and 3, not 2 (which holds clip 20): the copy covers rows
        // 0 and 2 only.
        let copy = copy_range(&p, &[Id(1), Id(3)], q(4), q(8)).unwrap();
        assert_eq!((copy.rows.clone(), copy.tracks), (vec![0, 2], 3));
        assert_eq!(landing_tracks(&p, &copy, 0), [Id(1), Id(3)]);
        paste(&mut p, &copy, q(2), 0, &mut ids());
        assert_eq!(clips(&p, 1), [(20, 0, 8, 1_000)], "untouched");
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn copied_clips_keep_their_layout_and_skip_tracks_of_another_kind() {
        let mut p = song();
        Arc::make_mut(&mut p.tracks[2]).kind = TrackKind::Midi;
        let copy = copy_clips(&p, &[Id(10), Id(20)]).unwrap();
        assert_eq!((copy.length, copy.tracks, copy.from_track), (q(12), 2, 0));
        let new = paste(&mut p, &copy, q(20), 1, &mut ids());
        assert_eq!(new.len(), 1, "the second would land on the MIDI track");
        assert_eq!(
            clips(&p, 1).last().unwrap().1,
            24,
            "kept its offset from the first"
        );
    }
}
