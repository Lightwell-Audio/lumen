//! Clip edits (M3 slice 6): moving, copying, trimming, slipping, fades and
//! gain, deleting, ranges and paste. Each is one undo step through the
//! history's `apply`; drags pass a merge key and redo their edit from the
//! project as the drag found it.

use std::sync::Arc;

use std::ops::RangeInclusive;

use lumen_core::curve::Curve;
use lumen_core::time::Ticks;
use lumen_project::edit::{self, Edge};
use lumen_project::model::{Clip, ClipContent, Color, Id, Project};

use super::{Document, plural};

/// Clip gain, in dB: from silence (shown as −∞) to +12 dB.
pub const CLIP_GAIN: RangeInclusive<f32> = -60.0..=12.0;

impl Document {
    /// Puts clips at new places (M3 slice 6): each [`ClipMove`] moves that
    /// clip to its start on its track, starting from `base`, the project as
    /// the drag found it, so every frame of a drag is the same edit redone
    /// (one undo step with `merge`). Moved clips cut away what they cover
    /// (owner, slice 6c). Locked clips, and moves to tracks
    /// that do not exist, are left out.
    pub fn move_clips(&mut self, base: &Project, moves: &[ClipMove], merge: Option<&str>) {
        let label = plural(moves.len(), "Move clip", "Move {} clips");
        self.place_moves(&label, base, moves, merge);
    }

    /// [`Document::move_clips`] under another step name (Nudge).
    pub(super) fn place_moves(
        &mut self,
        label: &str,
        base: &Project,
        moves: &[ClipMove],
        merge: Option<&str>,
    ) {
        let follow = !self.lock_envelopes;
        let next = &mut self.next_id;
        self.history.apply(label, merge, |project| {
            *project = base.clone();
            if follow {
                let carries = carries_of(base, moves.iter().map(|m| (m.clip, m.track, m.start)));
                edit::carry::carry(project, &carries, false);
            }
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
        let follow = !self.lock_envelopes;
        let next = &mut self.next_id;
        self.history.apply(&label, merge, |project| {
            *project = base.clone();
            if follow {
                let carries = carries_of(base, moves.iter().map(|m| (m.clip, m.track, m.start)));
                edit::carry::carry(project, &carries, true);
            }
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

    /// Sets the fade at `clip`'s `edge` to `length` (slice 6d), kept within
    /// what the clip's other fade leaves of it. A drag passes `merge`.
    pub fn set_fade_length(&mut self, clip: Id, edge: Edge, length: Ticks, merge: Option<&str>) {
        let label = match edge {
            Edge::Start => "Fade in",
            Edge::End => "Fade out",
        };
        self.history.apply(label, merge, |project| {
            replace_clip(project, clip, |c| {
                let mut c = c.clone();
                let room = |other: Ticks| length.clamp(Ticks::ZERO, c.length - other);
                match edge {
                    Edge::Start => {
                        c.fade_in.length = room(c.fade_out.length);
                        c.fade_in.offset = Ticks::ZERO;
                        c.fade_in.span = Ticks::ZERO;
                    }
                    Edge::End => {
                        c.fade_out.length = room(c.fade_in.length);
                        c.fade_out.offset = Ticks::ZERO;
                        c.fade_out.span = Ticks::ZERO;
                    }
                }
                c
            });
        });
    }

    /// Sets the curve of the fades at `edge` of `clips` (the bend dot, the
    /// right-click menu). Locked clips stay as they are, and a curve that
    /// leaves its square is refused.
    pub fn set_fade_curve(&mut self, clips: &[Id], edge: Edge, curve: Curve, merge: Option<&str>) {
        if !curve.is_valid() {
            return;
        }
        self.history.apply("Fade curve", merge, |project| {
            for &id in clips {
                replace_clip(project, id, |c| {
                    let mut c = c.clone();
                    match edge {
                        Edge::Start => {
                            c.fade_in.curve = curve;
                            c.fade_in.offset = Ticks::ZERO;
                            c.fade_in.span = Ticks::ZERO;
                        }
                        Edge::End => {
                            c.fade_out.curve = curve;
                            c.fade_out.offset = Ticks::ZERO;
                            c.fade_out.span = Ticks::ZERO;
                        }
                    }
                    c
                });
            }
        });
    }

    /// Sets the gain of the audio `clips` to `db`, within [`CLIP_GAIN`].
    pub fn set_clip_gain(&mut self, clips: &[Id], db: f32, merge: Option<&str>) {
        let db = db.clamp(*CLIP_GAIN.start(), *CLIP_GAIN.end());
        self.history.apply("Clip gain", merge, |project| {
            for &id in clips {
                replace_clip(project, id, |c| {
                    let mut c = c.clone();
                    if let ClipContent::Audio { gain_db, .. } = &mut c.content {
                        *gain_db = db;
                    }
                    c
                });
            }
        });
    }

    /// Mutes or unmutes clips (slice 6e; locked ones too: a lock keeps a
    /// clip in place, not silent).
    pub fn set_clips_muted(&mut self, ids: &[Id], muted: bool) {
        let label = match (muted, ids.len()) {
            (true, 1) => "Mute clip".to_owned(),
            (false, 1) => "Unmute clip".to_owned(),
            (true, n) => format!("Mute {n} clips"),
            (false, n) => format!("Unmute {n} clips"),
        };
        self.edit_clips(&label, ids, |c| c.muted = muted);
    }

    /// Locks or unlocks clips: locked clips cannot be moved, trimmed,
    /// faded or deleted.
    pub fn set_clips_locked(&mut self, ids: &[Id], locked: bool) {
        let label = match (locked, ids.len()) {
            (true, 1) => "Lock clip".to_owned(),
            (false, 1) => "Unlock clip".to_owned(),
            (true, n) => format!("Lock {n} clips"),
            (false, n) => format!("Unlock {n} clips"),
        };
        self.edit_clips(&label, ids, |c| c.locked = locked);
    }

    /// Renames a clip (empty names are ignored).
    pub fn rename_clip(&mut self, id: Id, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.edit_clips("Rename clip", &[id], |c| c.name = name.to_owned());
    }

    /// Gives clips their own colour, or, with `None`, the track's again.
    pub fn recolor_clips(&mut self, ids: &[Id], color: Option<Color>) {
        let label = plural(ids.len(), "Change clip colour", "Change {} clips' colour");
        self.edit_clips(&label, ids, |c| c.color = color);
    }

    /// Changes each of the clips `ids`, locked or not, as one step; clips
    /// the change leaves as they were are not touched.
    fn edit_clips(&mut self, label: &str, ids: &[Id], change: impl Fn(&mut Clip)) {
        self.history.apply(label, None, |project| {
            for track in &mut project.tracks {
                for i in 0..track.clips.len() {
                    if !ids.contains(&track.clips[i].id) {
                        continue;
                    }
                    let mut clip = (*track.clips[i]).clone();
                    change(&mut clip);
                    if clip != *track.clips[i] {
                        Arc::make_mut(track).clips[i] = Arc::new(clip);
                    }
                }
            }
        });
    }

    /// Deletes clips; locked ones stay. Their media stays in the project
    /// (the media pool, M9).
    pub fn delete_clips(&mut self, ids: &[Id]) {
        let label = plural(ids.len(), "Delete clip", "Delete {} clips");
        let follow = !self.lock_envelopes;
        self.history.apply(&label, None, |project| {
            if follow {
                let spans: Vec<(Id, Ticks, Ticks)> = project
                    .tracks
                    .iter()
                    .flat_map(|t| t.clips.iter().map(move |c| (t.id, c)))
                    .filter(|(_, c)| ids.contains(&c.id) && !c.locked)
                    .map(|(track, c)| (track, c.start, c.start + c.length))
                    .collect();
                edit::carry::remove(project, &spans);
            }
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

    /// Deletes `[a, b)` from the whole song (Delete Time).
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
        let follow = !self.lock_envelopes;
        let next = &mut self.next_id;
        let mut pasted = Vec::new();
        self.history.apply(label, None, |project| {
            pasted = edit::paste(project, clipboard, at, track, &mut fresh_ids(next));
            if follow {
                edit::paste_lanes(project, clipboard, at, track);
            }
        });
        pasted
    }

    /// Deletes clips as a Cut (the clipboard holds them).
    pub fn cut_clips(&mut self, ids: &[Id]) {
        let label = plural(ids.len(), "Cut clip", "Cut {} clips");
        let follow = !self.lock_envelopes;
        self.history.apply(&label, None, |project| {
            if follow {
                let spans: Vec<(Id, Ticks, Ticks)> = project
                    .tracks
                    .iter()
                    .flat_map(|t| t.clips.iter().map(move |c| (t.id, c)))
                    .filter(|(_, c)| ids.contains(&c.id) && !c.locked)
                    .map(|(track, c)| (track, c.start, c.start + c.length))
                    .collect();
                edit::carry::remove(project, &spans);
            }
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

/// What each clip that moves or is copied takes of its track's automation:
/// from where it is in `base` to the track and start it is going to. A clip
/// that does not move, is locked or goes to a track that is not there takes
/// nothing.
fn carries_of(
    base: &Project,
    goes: impl Iterator<Item = (Id, Id, Ticks)>,
) -> Vec<edit::carry::Carry> {
    goes.filter_map(|(clip, to, start)| {
        let (from, clip) = base
            .tracks
            .iter()
            .find_map(|t| t.clips.iter().find(|c| c.id == clip).map(|c| (t.id, c)))?;
        if clip.locked || base.tracks.iter().all(|t| t.id != to) {
            return None;
        }
        let by = start.max(Ticks::ZERO) - clip.start;
        (by != Ticks::ZERO || from != to).then_some(edit::carry::Carry {
            from,
            span: (clip.start, clip.start + clip.length),
            to,
            by,
        })
    })
    .collect()
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
pub(super) fn fresh_ids(next: &mut u64) -> impl FnMut() -> Id + '_ {
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

    /// The owner's request: a MIDI clip dropped on
    /// another cuts it, but only its window. Moving the top clip away and
    /// dragging the covered clip's end back brings its notes back intact.
    #[test]
    fn a_covered_midi_clip_keeps_its_notes_and_shows_them_when_extended() {
        use lumen_project::model::{ClipContent, Note};
        let q = Ticks::quarters;
        let mut keys = lumen_project::testing::midi_track(
            1,
            "Keys",
            "poly",
            (0, 8),
            None,
            vec![
                Note::new(q(1), q(1), 60, 100),
                Note::new(q(5), q(2), 62, 90),
            ],
            vec![],
        );
        let mut top = (*keys.clips[0]).clone();
        top.id = Id(50);
        top.start = q(8);
        top.length = q(4);
        top.content = ClipContent::Midi {
            notes: vec![Note::new(q(0), q(1), 70, 100)],
            events: vec![],
        };
        keys.clips.push(Arc::new(top));
        let project = lumen_project::testing::project_of("Cover", vec![keys]);
        let under = project.tracks[0].clips[0].clone();
        let original = under.content.clone();
        let mut doc = Document::new(project);
        let track = doc.project().tracks[0].id;
        let place = |doc: &mut Document, start| {
            let base = doc.project().clone();
            let to = ClipMove {
                clip: Id(50),
                track,
                start: q(start),
            };
            doc.move_clips(&base, &[to], None);
        };
        let clip = |doc: &Document, id| {
            (*doc.project().tracks[0]
                .clips
                .iter()
                .find(|c| c.id == id)
                .unwrap())
            .clone()
        };

        // Dropped over beats 4 to 8 of the clip under it: that clip now ends
        // at 4, and the note at 5 is hidden, not gone.
        place(&mut doc, 4);
        let cut = clip(&doc, under.id);
        assert_eq!(cut.length, q(4));
        assert_eq!(cut.content, original, "every note kept");
        assert_eq!(edit::shown_notes(&cut), vec![0], "the note at 5 hidden");
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));

        // The top clip moved away, then the old clip's end dragged back.
        place(&mut doc, 12);
        let base = doc.project().clone();
        doc.trim_clip(&base, under.id, edit::Edge::End, q(8), false, None);
        let back = clip(&doc, under.id);
        assert_eq!(back.length, q(8));
        assert_eq!(back.content, original, "intact");
        assert_eq!(edit::shown_notes(&back), vec![0, 1], "and it shows again");
    }

    #[test]
    fn fades_and_gain_are_single_steps_and_stay_in_range() {
        let mut doc = Document::new(crate::empty_project());
        let a = doc.fresh_id();
        doc.import(vec![file(a, "Kick", Ticks::ZERO)]);
        let clip = doc.project().tracks[0].clips[0].clone();
        let get = |doc: &Document| (*doc.project().tracks[0].clips[0]).clone();
        let steps = |doc: &Document| doc.history().steps().0.len();
        let before_edits = steps(&doc);

        // A fade drag: one step, and never past what the other fade leaves.
        for q in [1, 3, 99] {
            doc.set_fade_length(clip.id, Edge::End, Ticks::quarters(q), Some("clip fade"));
        }
        assert_eq!(
            get(&doc).fade_out.length,
            clip.length,
            "the whole clip at most"
        );
        doc.set_fade_length(clip.id, Edge::Start, Ticks::quarters(2), None);
        assert_eq!(get(&doc).fade_in.length, Ticks::ZERO, "no room left");
        doc.set_fade_length(clip.id, Edge::End, Ticks::quarters(1), None);
        doc.set_fade_length(clip.id, Edge::Start, Ticks(-Ticks::quarters(2).0), None);
        assert_eq!(get(&doc).fade_in.length, Ticks::ZERO, "never negative");
        doc.set_fade_length(clip.id, Edge::Start, Ticks::quarters(1), None);
        doc.set_fade_curve(&[clip.id], Edge::Start, Curve::S_CURVE, None);
        let faded = get(&doc);
        assert_eq!(faded.fade_in.length, Ticks::quarters(1));
        assert_eq!(faded.fade_in.curve, Curve::S_CURVE);
        assert_eq!(faded.fade_out.length, Ticks::quarters(1));
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));

        // A gain drag: one step, clamped.
        for db in [-3.0, -9.0, -400.0] {
            doc.set_clip_gain(&[clip.id], db, Some("clip gain"));
        }
        let gain = |doc: &Document| match get(doc).content {
            ClipContent::Audio { gain_db, .. } => gain_db,
            ClipContent::Midi { .. } => unreachable!("an audio clip"),
        };
        assert_eq!(gain(&doc), *CLIP_GAIN.start());
        doc.set_clip_gain(&[clip.id], 40.0, None);
        assert_eq!(gain(&doc), *CLIP_GAIN.end());
        // Two drags, four edits; the two that changed nothing left no step.
        assert_eq!(steps(&doc) - before_edits, 6);
        doc.undo();
        assert_eq!(gain(&doc), *CLIP_GAIN.start(), "undo takes back one edit");

        // Locked clips keep their fades and gain.
        let id = clip.id;
        doc.apply("Lock", None, |p| {
            let t = Arc::make_mut(&mut p.tracks[0]);
            Arc::make_mut(&mut t.clips[0]).locked = true;
        });
        let before = get(&doc);
        doc.set_fade_length(id, Edge::Start, Ticks::ZERO, None);
        doc.set_clip_gain(&[id], 0.0, None);
        doc.set_fade_curve(&[id], Edge::End, Curve::FAST, None);
        assert_eq!(get(&doc), before);
    }

    #[test]
    fn clip_properties_are_single_steps_and_work_on_locked_clips() {
        let mut doc = Document::new(crate::empty_project());
        let (a, b) = (doc.fresh_id(), doc.fresh_id());
        doc.import(vec![
            file(a, "Kick", Ticks::ZERO),
            file(b, "Snare", Ticks::quarters(4)),
        ]);
        let ids: Vec<Id> = doc.project().tracks.iter().map(|t| t.clips[0].id).collect();
        let clip = |doc: &Document, i: usize| (*doc.project().tracks[i].clips[0]).clone();
        let steps = |doc: &Document| doc.history().steps().0.len();
        let start = steps(&doc);

        doc.set_clips_locked(&ids[..1], true);
        doc.set_clips_muted(&ids, true);
        doc.rename_clip(ids[0], "  Big kick ");
        doc.recolor_clips(&ids, Some(Color(0x9E5A6A)));
        let (kick, snare) = (clip(&doc, 0), clip(&doc, 1));
        assert!(kick.locked && kick.muted && snare.muted && !snare.locked);
        assert_eq!(kick.name, "Big kick", "trimmed, and locked clips rename");
        assert_eq!(
            (kick.color, snare.color),
            (Some(Color(0x9E5A6A)), Some(Color(0x9E5A6A)))
        );
        assert_eq!(steps(&doc) - start, 4, "one step each");
        assert_eq!(
            doc.history().steps().0.last(),
            Some(&"Change 2 clips' colour")
        );

        // Nothing to change, nothing recorded; an empty name is refused.
        doc.set_clips_muted(&ids, true);
        doc.rename_clip(ids[1], "   ");
        assert_eq!(steps(&doc) - start, 4);
        assert_eq!(clip(&doc, 1).name, "Snare");

        // The track's colour again; then undo takes back one edit.
        doc.recolor_clips(&ids[1..], None);
        assert_eq!(clip(&doc, 1).color, None);
        doc.undo();
        assert_eq!(clip(&doc, 1).color, Some(Color(0x9E5A6A)));
        doc.set_clips_locked(&ids[..1], false);
        assert!(!clip(&doc, 0).locked);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }
}
