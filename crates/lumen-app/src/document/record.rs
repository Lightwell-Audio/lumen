//! Recording MIDI into the document (M5 slice 6b). A recording is one undo
//! step however many takes it commits: every [`Document::record_take`] of it
//! merges into the same step. The placing is `lumen_project::edit::record`,
//! whose documentation says what a take becomes.

use std::collections::HashMap;

use lumen_core::time::Ticks;
use lumen_project::edit::record::{self, Blocked, Mode, Placement, Take};
use lumen_project::model::{Id, Project};

use super::Document;
use super::clips::fresh_ids;

/// A recording in progress: the project as it was when it began, the merge
/// key that makes its takes one undo step, and the clip each track's take
/// goes into.
#[derive(Debug)]
pub struct Recording {
    base: Project,
    key: String,
    take_clips: HashMap<Id, Id>,
    /// The ids each track's takes gave to the pieces of clips they cut, so
    /// the next commit gives the same pieces the same ids: a clip that a
    /// take only shortens stays the clip it was.
    pieces: HashMap<Id, Vec<Id>>,
}

impl Recording {
    /// The clip a Replace take of `track` is, once it has been placed.
    pub fn take_clip(&self, track: Id) -> Option<Id> {
        self.take_clips.get(&track).copied()
    }
}

impl Document {
    /// Starts a recording on the project as it is now, on `tracks`. Every
    /// take of it is placed from this snapshot, and they are all one undo
    /// step. Each track's take clip has its id from the start, so the take
    /// drawn before the first commit is already that clip.
    pub fn begin_recording(&mut self, tracks: &[Id]) -> Recording {
        let key = format!("record-{}", self.fresh_id().0);
        let take_clips = tracks.iter().map(|&t| (t, self.fresh_id())).collect();
        Recording {
            base: self.project().clone(),
            key,
            take_clips,
            pieces: HashMap::new(),
        }
    }

    /// The project as it will be once `takes` (a take per track) are
    /// committed as part of `rec`: what the arrangement draws on every
    /// frame of a recording, between the commits. Nothing changes and no
    /// step is made. Pieces the commits have not cut yet are given ids past
    /// the document's counter, which are not used by anything. A track whose
    /// take a locked clip blocks is drawn as it was.
    pub fn preview_takes(
        &self,
        rec: &Recording,
        takes: &[(Id, Take)],
        mode: Mode,
        punch: Option<(Ticks, Ticks)>,
    ) -> Project {
        let mut next = self.project().clone();
        let mut spare = self.next_id;
        for (track, take) in takes {
            let Some(&take_clip) = rec.take_clips.get(track) else {
                continue;
            };
            let pieces = rec.pieces.get(track).map_or(&[][..], Vec::as_slice);
            let mut used = 0;
            let mut fresh = || {
                used += 1;
                pieces.get(used - 1).copied().unwrap_or_else(|| {
                    spare += 1;
                    Id(spare - 1)
                })
            };
            let placement = Placement {
                mode,
                punch,
                take_clip,
            };
            if record::place(&mut next, &rec.base, *track, take, placement, &mut fresh).is_err() {
                // Blocked: `place` left the track as the recording began;
                // draw it as the document has it.
                if let (Some(slot), Some(now)) = (
                    next.tracks.iter_mut().find(|t| t.id == *track),
                    self.project().tracks.iter().find(|t| t.id == *track),
                ) {
                    *slot = std::sync::Arc::clone(now);
                }
            }
        }
        next
    }

    /// Places `take` on the MIDI track `track` as part of `rec`, merged into
    /// the recording's undo step. Returns the ids of the clips made or
    /// changed. A locked clip in the way gives `Blocked`, and then nothing
    /// changes and no step is made.
    pub fn record_take(
        &mut self,
        rec: &mut Recording,
        track: Id,
        take: &Take,
        mode: Mode,
        punch: Option<(Ticks, Ticks)>,
    ) -> Result<Vec<Id>, Blocked> {
        let take_clip = *rec
            .take_clips
            .entry(track)
            .or_insert_with(|| self.fresh_id());
        // Placed on a copy first: a blocked take must not reach the history,
        // and an unchanged one makes no step.
        let mut next = self.project().clone();
        // Ids for the pieces a take cuts: the ones the last commit used, in
        // order, then new ones.
        let pieces = rec.pieces.entry(track).or_default();
        let mut used = 0;
        let mut minted = fresh_ids(&mut self.next_id);
        let mut fresh = || {
            used += 1;
            match pieces.get(used - 1) {
                Some(&id) => id,
                None => {
                    let id = minted();
                    pieces.push(id);
                    id
                }
            }
        };
        let placed = record::place(
            &mut next,
            &rec.base,
            track,
            take,
            Placement {
                mode,
                punch,
                take_clip,
            },
            &mut fresh,
        )?;
        self.history
            .apply_take("Record", Some(&rec.key), |project| *project = next);
        Ok(placed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_core::time::PPQ;
    use lumen_project::edit::record::TakeNote;
    use lumen_project::model::{ClipContent, Note, TrackKind};

    use super::super::midi_clips::NewClip;
    use super::*;

    fn q(n: i64) -> Ticks {
        Ticks(PPQ * n)
    }

    fn tn(start: i64, length: i64, pitch: u8) -> TakeNote {
        TakeNote {
            start: q(start),
            length: q(length),
            pitch,
            velocity: 100,
        }
    }

    fn take(start: i64, end: i64, notes: Vec<TakeNote>) -> Take {
        Take {
            start: q(start),
            end: q(end),
            notes,
            passes: Vec::new(),
            events: Vec::new(),
        }
    }

    /// The notes of clip `clip`, wherever it is.
    fn notes_of(doc: &Document, clip: Id) -> Vec<Note> {
        let found = doc
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == clip)
            .expect("the clip is in the project");
        match &found.content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            ClipContent::Audio { .. } => unreachable!("a MIDI clip"),
        }
    }

    #[test]
    fn a_recording_of_several_commits_on_two_tracks_is_one_undo_step() {
        let mut doc = Document::new(crate::empty_project());
        let one = doc.add_track(TrackKind::Midi, None);
        let two = doc.add_track(TrackKind::Midi, None);
        let before = doc.project().clone();
        let mut rec = doc.begin_recording(&[]);
        let growing = [
            take(0, 1, vec![tn(0, 1, 60)]),
            take(0, 2, vec![tn(0, 1, 60), tn(1, 1, 62)]),
            take(0, 3, vec![tn(0, 1, 60), tn(1, 1, 62), tn(2, 1, 64)]),
        ];
        for part in &growing {
            doc.record_take(&mut rec, one, part, Mode::Replace, None)
                .unwrap();
            doc.record_take(
                &mut rec,
                two,
                &take(0, 3, vec![tn(2, 1, 36)]),
                Mode::Replace,
                None,
            )
            .unwrap();
        }
        let done = doc.project().clone();
        assert_eq!(doc.history().undo_label(), Some("Record"));
        assert_eq!(lumen_project::validate(&done), Ok(()));
        assert_eq!(
            done.tracks[0].clips.len(),
            1,
            "one clip per take, not three"
        );
        assert_eq!(notes_of(&doc, done.tracks[0].clips[0].id).len(), 3);

        doc.undo();
        assert_eq!(*doc.project(), before, "the whole recording is one step");
        assert_eq!(doc.history().undo_label(), Some("Add MIDI track"));
        doc.redo();
        assert_eq!(*doc.project(), done, "redo brings the recording back");
    }

    #[test]
    fn a_clip_a_take_cuts_keeps_the_same_ids_at_every_commit() {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        doc.create_midi_clip(track, NewClip::Range(q(0), q(16)))
            .unwrap();
        let mut rec = doc.begin_recording(&[]);
        let mut layouts = Vec::new();
        for end in [6, 8, 10] {
            doc.record_take(
                &mut rec,
                track,
                &take(4, end, vec![tn(4, 1, 60)]),
                Mode::Replace,
                None,
            )
            .unwrap();
            let clips = &doc.project().tracks[0].clips;
            layouts.push(clips.iter().map(|c| c.id).collect::<Vec<_>>());
        }
        assert_eq!(layouts[0].len(), 3, "the left part, the take, the right");
        assert_eq!(layouts[0], layouts[1]);
        assert_eq!(layouts[1], layouts[2]);
    }

    #[test]
    fn overdub_is_a_new_clip_carrying_the_notes_of_the_clip_under_it() {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let clip = doc
            .create_midi_clip(track, NewClip::Range(q(0), q(4)))
            .unwrap();
        let old = Note::new(q(0), q(1), 48, 90);
        doc.apply("Fill", None, |p| {
            let t = Arc::make_mut(&mut p.tracks[0]);
            let c = Arc::make_mut(&mut t.clips[0]);
            if let ClipContent::Midi { notes, .. } = &mut c.content {
                *notes = vec![old];
            }
        });
        let mut rec = doc.begin_recording(&[track]);
        let placed = doc.record_take(
            &mut rec,
            track,
            &take(0, 3, vec![tn(2, 1, 64)]),
            Mode::Overdub,
            None,
        );
        let new = rec.take_clip(track).unwrap();
        assert_eq!(placed, Ok(vec![new]));
        assert_ne!(new, clip);
        assert_eq!(notes_of(&doc, new), [old, Note::new(q(2), q(1), 64, 100)]);
        let clips = &doc.project().tracks[0].clips;
        let spans: Vec<_> = clips.iter().map(|c| (c.id, c.start, c.length)).collect();
        assert_eq!(spans, [(new, q(0), q(3)), (clip, q(3), q(1))]);
        assert_eq!(doc.history().undo_label(), Some("Record"));
    }

    #[test]
    fn the_preview_is_what_the_commit_makes_and_changes_nothing() {
        for mode in [Mode::Replace, Mode::Overdub] {
            let mut doc = Document::new(crate::empty_project());
            let track = doc.add_track(TrackKind::Midi, None);
            doc.create_midi_clip(track, NewClip::Range(q(0), q(8)))
                .unwrap();
            let mut rec = doc.begin_recording(&[track]);
            let early = take(2, 3, vec![tn(2, 1, 60)]);
            doc.record_take(&mut rec, track, &early, mode, None)
                .unwrap();
            let state = doc.state();
            // Between commits: the take has grown.
            let grown = take(2, 5, vec![tn(2, 1, 60), tn(4, 1, 64)]);
            let preview = doc.preview_takes(&rec, &[(track, grown.clone())], mode, None);
            assert_eq!(doc.state(), state, "{mode:?}: no step");
            doc.record_take(&mut rec, track, &grown, mode, None)
                .unwrap();
            assert_eq!(&preview, doc.project(), "{mode:?}");
        }
    }

    #[test]
    fn an_empty_overdub_over_an_empty_lane_is_an_empty_clip_at_once() {
        // The clip shows from the start of recording, before any note.
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let mut rec = doc.begin_recording(&[track]);
        let placed = doc.record_take(&mut rec, track, &take(0, 4, vec![]), Mode::Overdub, None);
        assert_eq!(placed, Ok(vec![rec.take_clip(track).unwrap()]));
        let clips = &doc.project().tracks[0].clips;
        assert_eq!(
            (clips.len(), clips[0].start, clips[0].length),
            (1, q(0), q(4))
        );
    }

    #[test]
    fn a_locked_clip_in_the_way_is_refused_with_no_history_entry() {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let clip = doc
            .create_midi_clip(track, NewClip::Range(q(0), q(4)))
            .unwrap();
        doc.set_clips_locked(&[clip], true);
        let mut rec = doc.begin_recording(&[]);
        let state = doc.state();
        let label = doc.history().undo_label().map(str::to_owned);
        for mode in [Mode::Replace, Mode::Overdub] {
            let refused =
                doc.record_take(&mut rec, track, &take(1, 3, vec![tn(1, 1, 60)]), mode, None);
            assert_eq!(refused, Err(Blocked), "{mode:?}");
        }
        assert_eq!(doc.state(), state, "nothing changed");
        assert_eq!(doc.history().undo_label(), label.as_deref());

        // Beside the locked clip the take goes in, as one step.
        let placed = doc.record_take(
            &mut rec,
            track,
            &take(4, 6, vec![tn(4, 1, 60)]),
            Mode::Replace,
            None,
        );
        assert!(placed.is_ok());
        assert_eq!(doc.history().undo_label(), Some("Record"));
    }
}
