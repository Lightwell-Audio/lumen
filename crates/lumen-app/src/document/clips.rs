//! Clip edits (M3 slice 6): moving, copying, trimming, slipping, fades and
//! gain, deleting, ranges and paste. Each is one undo step through
//! [`Document::apply`]; drags pass a merge key and redo their edit from the
//! project as the drag found it.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::edit;
use lumen_project::model::{Clip, Id, Project};

use super::{Document, plural};

impl Document {
    /// Puts clips at new places (M3 slice 6): each [`ClipMove`] moves that
    /// clip to its start on its track, starting from `base`, the project as
    /// the drag found it, so every frame of a drag is the same edit redone
    /// (one undo step with `merge`). Moved clips cut away what they cover,
    /// as in Ableton (owner, slice 6c). Locked clips, and moves to tracks
    /// that do not exist, are left out.
    pub fn move_clips(&mut self, base: &Project, moves: &[ClipMove], merge: Option<&str>) {
        let label = plural(moves.len(), "Move clip", "Move {} clips");
        let next = &mut self.next_id;
        self.history.apply(&label, merge, |project| {
            *project = base.clone();
            place_clips(project, moves);
            let placed: Vec<Id> = moves.iter().map(|m| m.clip).collect();
            edit::overwrite(project, &placed, &mut fresh_ids(next));
        });
    }

    /// Puts copies of clips at new places, as Ctrl+drag does: the copy of
    /// `moves[i].clip` gets the id `ids[i]`. Like [`Document::move_clips`],
    /// from `base`, and the copies cut away what they cover.
    pub fn copy_clips(
        &mut self,
        base: &Project,
        moves: &[ClipMove],
        ids: &[Id],
        merge: Option<&str>,
    ) {
        let label = plural(moves.len(), "Copy clip", "Copy {} clips");
        let next = &mut self.next_id;
        self.history.apply(&label, merge, |project| {
            *project = base.clone();
            let copies: Vec<(Clip, ClipMove)> = moves
                .iter()
                .zip(ids)
                .filter_map(|(m, &id)| {
                    let clip = project
                        .tracks
                        .iter()
                        .flat_map(|t| &t.clips)
                        .find(|c| c.id == m.clip)?;
                    let mut copy = (**clip).clone();
                    copy.id = id;
                    copy.locked = false;
                    copy.start = m.start.max(Ticks::ZERO);
                    Some((copy, *m))
                })
                .collect();
            for (copy, m) in copies {
                if let Some(t) = project.tracks.iter_mut().find(|t| t.id == m.track) {
                    let t = Arc::make_mut(t);
                    t.clips.push(Arc::new(copy));
                    t.clips.sort_by_key(|c| c.start);
                }
            }
            edit::overwrite(project, ids, &mut fresh_ids(next));
        });
    }

    /// Trims `clip`'s `edge` to `to` (slice 6c), from `base`; with
    /// `looping`, the end extends by repeating the clip. The trimmed clip
    /// cuts away what it now covers.
    pub fn trim_clip(
        &mut self,
        base: &Project,
        clip: Id,
        edge: edit::Edge,
        to: Ticks,
        looping: bool,
        merge: Option<&str>,
    ) {
        let label = if looping { "Loop clip" } else { "Trim clip" };
        let next = &mut self.next_id;
        self.history.apply(label, merge, |project| {
            *project = base.clone();
            let tempo = edit::tempo_of(base);
            replace_clip(project, clip, |c| {
                edit::trim(base, &tempo, c, edge, to, looping)
            });
            edit::overwrite(project, &[clip], &mut fresh_ids(next));
        });
    }

    /// Slides `clip`'s content `by` inside it, from `base` (Ctrl+Alt+drag).
    pub fn slip_clip(&mut self, base: &Project, clip: Id, by: Ticks, merge: Option<&str>) {
        self.history.apply("Slip clip", merge, |project| {
            *project = base.clone();
            let tempo = edit::tempo_of(base);
            replace_clip(project, clip, |c| edit::slip(base, &tempo, c, by));
        });
    }

    /// Deletes clips; locked ones stay. Their media stays in the project
    /// (the media pool, M9).
    pub fn delete_clips(&mut self, ids: &[Id]) {
        let label = plural(ids.len(), "Delete clip", "Delete {} clips");
        self.history.apply(&label, None, |project| {
            for track in &mut project.tracks {
                if track.clips.iter().any(|c| ids.contains(&c.id) && !c.locked) {
                    Arc::make_mut(track)
                        .clips
                        .retain(|c| c.locked || !ids.contains(&c.id));
                }
            }
        });
    }

    /// Empties `[a, b)` on `tracks` (a time range's Delete, or its Cut):
    /// clips are cut back at its edges or split around it.
    pub fn clear_range(&mut self, label: &str, tracks: &[Id], a: Ticks, b: Ticks) {
        let next = &mut self.next_id;
        self.history.apply(label, None, |project| {
            edit::clear(project, tracks, a, b, &mut fresh_ids(next));
        });
    }

    /// Deletes `[a, b)` from the whole song (Ableton's Delete Time).
    pub fn delete_time(&mut self, a: Ticks, b: Ticks) {
        let next = &mut self.next_id;
        self.history.apply("Delete time", None, |project| {
            edit::delete_time(project, a, b, &mut fresh_ids(next));
        });
    }

    /// Inserts `length` of silence at `at` across the whole song.
    pub fn insert_time(&mut self, at: Ticks, length: Ticks) {
        let next = &mut self.next_id;
        self.history.apply("Insert time", None, |project| {
            edit::insert_time(project, at, length, &mut fresh_ids(next));
        });
    }

    /// Pastes `clipboard` at `at` from the track with index `track`,
    /// overwriting what is there, as one step named `label` ("Paste",
    /// "Duplicate"). Returns the new clips' ids.
    pub fn paste(
        &mut self,
        label: &str,
        clipboard: &edit::Clipboard,
        at: Ticks,
        track: usize,
    ) -> Vec<Id> {
        let next = &mut self.next_id;
        let mut pasted = Vec::new();
        self.history.apply(label, None, |project| {
            pasted = edit::paste(project, clipboard, at, track, &mut fresh_ids(next));
        });
        pasted
    }

    /// Deletes clips as a Cut (the clipboard holds them).
    pub fn cut_clips(&mut self, ids: &[Id]) {
        let label = plural(ids.len(), "Cut clip", "Cut {} clips");
        self.history.apply(&label, None, |project| {
            for track in &mut project.tracks {
                if track.clips.iter().any(|c| ids.contains(&c.id) && !c.locked) {
                    Arc::make_mut(track)
                        .clips
                        .retain(|c| c.locked || !ids.contains(&c.id));
                }
            }
        });
    }
}

/// Replaces the unlocked clip `id` with `change` of it.
fn replace_clip(project: &mut Project, id: Id, change: impl FnOnce(&Clip) -> Clip) {
    for track in &mut project.tracks {
        if let Some(i) = track.clips.iter().position(|c| c.id == id && !c.locked) {
            let t = Arc::make_mut(track);
            t.clips[i] = Arc::new(change(&t.clips[i]));
            t.clips.sort_by_key(|c| c.start);
            return;
        }
    }
}

/// Where a clip goes: a track and a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipMove {
    pub clip: Id,
    pub track: Id,
    pub start: Ticks,
}

/// Moves each clip in `moves` to its track and start, keeping every
/// track's clips in time order. Locked clips, and moves to tracks that do
/// not exist, are left out, so no clip is ever lost.
fn place_clips(project: &mut Project, moves: &[ClipMove]) {
    let tracks: Vec<Id> = project.tracks.iter().map(|t| t.id).collect();
    let target = |c: &Arc<Clip>| {
        moves
            .iter()
            .find(|m| m.clip == c.id && !c.locked && tracks.contains(&m.track))
            .copied()
    };
    let mut taken = Vec::new();
    for track in &mut project.tracks {
        if track.clips.iter().any(|c| target(c).is_some()) {
            let t = Arc::make_mut(track);
            let (go, stay): (Vec<_>, Vec<_>) = t.clips.drain(..).partition(|c| target(c).is_some());
            t.clips = stay;
            taken.extend(go.into_iter().filter_map(|c| Some((target(&c)?, c))));
        }
    }
    for (m, clip) in taken {
        if let Some(t) = project.tracks.iter_mut().find(|t| t.id == m.track) {
            let mut clip = (*clip).clone();
            clip.start = m.start.max(Ticks::ZERO);
            Arc::make_mut(t).clips.push(Arc::new(clip));
        }
    }
    for track in &mut project.tracks {
        if !track.clips.is_sorted_by_key(|c| c.start) {
            Arc::make_mut(track).clips.sort_by_key(|c| c.start);
        }
    }
}

/// New ids from a document's counter, for edits that make clips.
fn fresh_ids(next: &mut u64) -> impl FnMut() -> Id + '_ {
    move || {
        let id = Id(*next);
        *next += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::tests::file;

    #[test]
    fn clips_move_and_copy_across_tracks_as_one_step_and_locked_ones_stay() {
        let mut doc = Document::new(crate::empty_project());
        let (a, b) = (doc.fresh_id(), doc.fresh_id());
        doc.import(vec![
            file(a, "Kick", Ticks::ZERO),
            file(b, "Snare", Ticks::quarters(4)),
        ]);
        let tracks: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
        let kick = doc.project().tracks[0].clips[0].id;
        // A drag: three frames, one step; the kick ends on the snare's track.
        let base = doc.project().clone();
        for q in [1, 2, 8] {
            let to = ClipMove {
                clip: kick,
                track: tracks[1],
                start: Ticks::quarters(q),
            };
            doc.move_clips(&base, &[to], Some("clip drag"));
        }
        let project = doc.project();
        assert!(project.tracks[0].clips.is_empty());
        let starts: Vec<_> = project.tracks[1].clips.iter().map(|c| c.start).collect();
        assert_eq!(
            starts,
            [Ticks::quarters(4), Ticks::quarters(8)],
            "in time order"
        );
        assert_eq!(lumen_project::validate(project), Ok(()));
        doc.undo();
        assert_eq!(
            doc.project().tracks[0].clips[0].start,
            Ticks::ZERO,
            "one step"
        );

        // A copy-drag: twice, still one copy.
        let copy = doc.fresh_id();
        let base = doc.project().clone();
        for q in [12, 16] {
            let to = ClipMove {
                clip: kick,
                track: tracks[0],
                start: Ticks::quarters(q),
            };
            doc.copy_clips(&base, &[to], &[copy], Some("clip copy"));
        }
        let kicks: Vec<_> = doc.project().tracks[0]
            .clips
            .iter()
            .map(|c| (c.id, c.start))
            .collect();
        assert_eq!(kicks, [(kick, Ticks::ZERO), (copy, Ticks::quarters(16))]);

        // Locked clips neither move nor go; a move to a missing track is
        // ignored rather than losing the clip.
        doc.apply("lock", None, |p| {
            Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).locked = true;
        });
        let away = ClipMove {
            clip: kick,
            track: tracks[1],
            start: Ticks::ZERO,
        };
        let base = doc.project().clone();
        doc.move_clips(&base, &[away], None);
        doc.delete_clips(&[kick, copy]);
        let left: Vec<_> = doc.project().tracks[0].clips.iter().map(|c| c.id).collect();
        assert_eq!(left, [kick]);
        let lost = ClipMove {
            clip: kick,
            track: Id(999_999),
            start: Ticks::ZERO,
        };
        doc.apply("unlock", None, |p| {
            Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).locked = false;
        });
        let base = doc.project().clone();
        doc.move_clips(&base, &[lost], None);
        assert_eq!(doc.project().tracks[0].clips[0].id, kick);
    }
}
