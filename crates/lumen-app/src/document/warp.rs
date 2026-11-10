//! Warp (M9.5 slice S4a): the document's edits of a clip's warp. Each is one
//! undo step; one that changes nothing adds none; a locked clip is left as it
//! is. The audio editor's Warp chip (S5a) is their control; the engine plays
//! a warped clip in Repitch or a stretch mode (ADR-0131, ADR-0132).

use std::sync::Arc;

#[cfg(test)]
use lumen_core::warp::Marker;
use lumen_project::edit::warp::{self as edit, Change, Refused};
use lumen_project::edit::warp_markers::{Edited, MarkerEdit, edit_marker};
use lumen_project::model::{Clip, Id, Project};

use super::Document;

/// What a warp edit did to the clips it was asked about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Warped {
    /// Clips changed.
    pub changed: usize,
    /// Unlocked clips that could not take it: MIDI, a file that is not in
    /// the project, or a setting out of range.
    pub refused: usize,
}

/// What a marker edit did: the marker added, moved or slid by its place in
/// time now, and whether a move or slide stopped at a speed limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkerDone {
    pub index: Option<usize>,
    pub held: bool,
}

/// The unlocked clip `id` of `project` with `change` made to its markers.
fn marker_edited(project: &Project, id: Id, change: MarkerEdit) -> Result<Edited, Refused> {
    let clip = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id == id && !c.locked)
        .ok_or(Refused::Unchanged)?;
    edit_marker(project, clip, change)
}

/// `project` with `change` made to the markers of clip `id`, and the place of
/// the marker it made: where a drag that first makes a marker begins, without
/// a step in the history. `None` when the change does nothing.
pub fn with_marker_edit(
    project: &Project,
    id: Id,
    change: MarkerEdit,
) -> Option<(Project, Option<usize>)> {
    let edited = marker_edited(project, id, change).ok()?;
    let mut out = project.clone();
    super::clips::replace_clip(&mut out, id, |_| edited.clip.clone());
    Some((out, edited.index))
}

/// Applies `make` to each unlocked clip of `ids`, as one step named `label`.
fn each(
    project: &mut Project,
    ids: &[Id],
    make: &dyn Fn(&Project, &Clip) -> Result<Clip, Refused>,
) -> Warped {
    let mut out = Warped::default();
    let base = project.clone();
    for track in 0..project.tracks.len() {
        for index in 0..project.tracks[track].clips.len() {
            let clip = Arc::clone(&project.tracks[track].clips[index]);
            if !ids.contains(&clip.id) || clip.locked {
                continue;
            }
            match make(&base, &clip) {
                Ok(next) => {
                    let track = Arc::make_mut(&mut project.tracks[track]);
                    track.clips[index] = Arc::new(next);
                    out.changed += 1;
                }
                Err(Refused::Unchanged) => {}
                Err(_) => out.refused += 1,
            }
        }
    }
    out
}

/// Clip `id` in `project`.
fn clip_in(project: &Project, id: Id) -> Option<&Clip> {
    project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id == id)
        .map(|c| &**c)
}

impl Document {
    /// Turns warp on for the selected audio clips, for files made at
    /// `original_bpm`. A clip at the file's own tempo plays as it did; any other
    /// tempo plays it faster or slower, the pitch moving with it (Repitch).
    #[cfg(test)]
    pub fn warp_clips(&mut self, ids: &[Id], original_bpm: f64) -> Warped {
        self.warp_clips_by(ids, &|_| original_bpm)
    }

    /// [`Document::warp_clips`] with the tempo each clip's file was made at
    /// chosen by `original_bpm` (clips of different files differ).
    pub fn warp_clips_by(&mut self, ids: &[Id], original_bpm: &dyn Fn(&Clip) -> f64) -> Warped {
        let mut out = Warped::default();
        self.history.apply("Warp clips", None, |project| {
            out = each(project, ids, &|p, c| edit::enable(p, c, original_bpm(c)));
        });
        out
    }

    /// Turns warp off: each clip plays from where it stands at the file's own
    /// speed.
    pub fn unwarp_clips(&mut self, ids: &[Id]) -> Warped {
        let mut out = Warped::default();
        self.history.apply("Turn warp off", None, |project| {
            out = each(project, ids, &|_, c| edit::disable(c));
        });
        out
    }

    /// Changes a warp setting of the selected warped clips (mode, the tempo
    /// the file was made at, transpose). A drag passes a `merge` key, so the
    /// whole drag is one undo step.
    pub fn set_warp(&mut self, ids: &[Id], change: &Change, merge: Option<&str>) -> Warped {
        let label = match change {
            Change::Mode(_) => "Set warp mode",
            Change::OriginalBpm(_) => "Set original tempo",
            Change::Transpose(_) => "Transpose clips",
        };
        let mut out = Warped::default();
        self.history.apply(label, merge, |project| {
            out = each(project, ids, &|p, c| edit::set(p, c, change));
        });
        out
    }

    /// Moves the transpose of each selected warped clip by `cents`, from the
    /// transpose it has (clips with different ones keep the difference), as
    /// one step. The result is held to the widest transpose a clip takes.
    pub fn transpose_warp(&mut self, ids: &[Id], cents: i32) -> Warped {
        use lumen_project::model::MAX_TRANSPOSE_CENTS;
        let mut out = Warped::default();
        self.history.apply("Transpose clips", None, |project| {
            out = each(project, ids, &|p, c| {
                let now = c.warp.as_ref().ok_or(Refused::Unchanged)?.transpose_cents;
                let next = now
                    .saturating_add(cents)
                    .clamp(-MAX_TRANSPOSE_CENTS, MAX_TRANSPOSE_CENTS);
                edit::set(p, c, &Change::Transpose(next))
            });
        });
        out
    }

    /// Multiplies the tempo each selected warped clip's file was made at by
    /// `factor` (2 and ½ correct a detection that found half or double), as
    /// one step.
    pub fn scale_warp_tempo(&mut self, ids: &[Id], factor: f64) -> Warped {
        let label = if factor > 1.0 {
            "Double clip tempo"
        } else {
            "Halve clip tempo"
        };
        let mut out = Warped::default();
        self.history.apply(label, None, |project| {
            out = each(project, ids, &|p, c| {
                let bpm = c.warp.as_ref().ok_or(Refused::Unchanged)?.original_bpm;
                edit::set(p, c, &Change::OriginalBpm(bpm * factor))
            });
        });
        out
    }

    /// "Warp from here": moves the audio that plays at `here` to the bar line
    /// `bar` in `ids`' first clip, warping it first (for a file made at
    /// `original_bpm`) if it is not, as one step. `snap` may move the second
    /// heard at `here` to a transient beside it.
    pub fn warp_from_here(
        &mut self,
        id: Id,
        original_bpm: f64,
        (here, bar): (lumen_core::time::Ticks, lumen_core::time::Ticks),
        snap: &dyn Fn(f64) -> Option<f64>,
    ) -> Warped {
        let mut out = Warped::default();
        self.history.apply("Warp from here", None, |project| {
            out = each(project, &[id], &|p, c| {
                edit::from_here(p, c, original_bpm, (here, bar), snap)
            });
        });
        out
    }

    /// One change to the warp markers of clip `id`, made from `base` (the
    /// warp strip, M9.5 S5b): a drag passes the project it began on and a
    /// merge key, so the whole gesture is one undo step. Only the clip is
    /// replaced, on the project as it is now.
    /// Says what was made, or why nothing was (a locked clip is
    /// [`Refused::Unchanged`]).
    pub fn edit_warp_marker(
        &mut self,
        base: &Project,
        id: Id,
        change: MarkerEdit,
        merge: Option<&str>,
    ) -> Result<MarkerDone, Refused> {
        let label = match change {
            MarkerEdit::Add(_) | MarkerEdit::AddOn(_) => "Add warp marker",
            MarkerEdit::Move { .. } => "Move warp marker",
            MarkerEdit::Slide { .. } => "Slide audio under warp marker",
            MarkerEdit::Remove(_) => "Delete warp marker",
            MarkerEdit::Clear => "Remove warp markers",
        };
        let edited = match marker_edited(base, id, change) {
            Ok(edited) => edited,
            // A drag brought back to where it began: the clip goes back to
            // what it was when the gesture began (the change would be a no-op
            // against `base`, but not against what the drag has made since).
            // Only this clip is put back: whatever else changed meanwhile (a
            // mapped controller moving a fader) stays.
            Err(Refused::Unchanged) if merge.is_some() => {
                let (Some(began), Some(now)) = (clip_in(base, id), clip_in(self.project(), id))
                else {
                    return Err(Refused::Unchanged);
                };
                if began == now {
                    return Err(Refused::Unchanged);
                }
                let began = began.clone();
                self.history.apply(label, merge, |project| {
                    super::clips::replace_clip(project, id, |_| began.clone());
                });
                let index = match change {
                    MarkerEdit::Move { index, .. } | MarkerEdit::Slide { index, .. } => Some(index),
                    _ => None,
                };
                return Ok(MarkerDone { index, held: false });
            }
            Err(refused) => return Err(refused),
        };
        let done = MarkerDone {
            index: edited.index,
            held: edited.held,
        };
        // The clip alone is replaced, on the project as it is now, so what
        // changed elsewhere during the drag is kept.
        self.history.apply(label, merge, |project| {
            super::clips::replace_clip(project, id, |_| edited.clip.clone());
        });
        Ok(done)
    }

    /// Replaces a warped clip's markers whole (tests of the map's limits).
    /// Returns whether the clip changed.
    #[cfg(test)]
    pub fn set_warp_markers(&mut self, id: Id, markers: Vec<Marker>) -> bool {
        let mut out = Warped::default();
        self.history.apply("Edit warp markers", None, |project| {
            out = each(project, &[id], &|p, c| {
                edit::set_markers(p, c, markers.clone())
            });
        });
        out.changed == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::tests::file;
    use lumen_core::time::Ticks;
    use lumen_project::model::WarpMode;

    fn doc_with_clip() -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![file(media, "Loop", Ticks::quarters(4))]);
        let clip = doc.project().tracks[0].clips[0].id;
        (doc, clip)
    }

    fn warp_of(doc: &Document) -> Option<lumen_project::model::Warp> {
        doc.project().tracks[0].clips[0].warp.clone()
    }

    #[test]
    fn warp_on_and_off_are_one_undo_step_each() {
        let (mut doc, id) = doc_with_clip();
        let plain = doc.project().clone();
        assert_eq!(doc.warp_clips(&[id], 120.0).changed, 1);
        assert!(warp_of(&doc).is_some());
        let warped = doc.project().clone();
        assert_eq!(doc.unwarp_clips(&[id]).changed, 1);
        assert_eq!(doc.project(), &plain, "off returns the clip");
        doc.undo();
        assert_eq!(doc.project(), &warped);
        doc.undo();
        assert_eq!(doc.project(), &plain);
        doc.redo();
        assert_eq!(doc.project(), &warped);
    }

    #[test]
    fn an_edit_that_changes_nothing_adds_no_step_and_a_locked_clip_is_left_alone() {
        let (mut doc, id) = doc_with_clip();
        assert_eq!(doc.unwarp_clips(&[id]), Warped::default(), "not warped");
        assert_ne!(doc.undo().as_deref(), Some("Turn warp off"));
        doc.redo();
        doc.set_clips_locked(&[id], true);
        assert_eq!(doc.warp_clips(&[id], 120.0), Warped::default());
        assert!(warp_of(&doc).is_none());
    }

    #[test]
    fn a_marker_drag_is_one_undo_step_from_where_it_began() {
        let (mut doc, id) = doc_with_clip();
        doc.warp_clips(&[id], 120.0);
        let before = doc.project().clone();
        // A drag that makes its marker first: the base it is redone from has
        // the marker, and the history has no step for that.
        let (base, index) =
            with_marker_edit(&before, id, MarkerEdit::Add(Ticks::quarters(2))).expect("a marker");
        let index = index.expect("its place");
        assert_eq!(doc.project(), &before);
        for to in [3, 1] {
            let done = doc
                .edit_warp_marker(
                    &base,
                    id,
                    MarkerEdit::Move {
                        index,
                        to: Ticks::quarters(to),
                    },
                    Some("warp marker"),
                )
                .expect("it moves");
            assert_eq!(done.index, Some(index));
        }
        let markers = warp_of(&doc).unwrap().markers;
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[1].at, Ticks::quarters(1));
        doc.undo();
        assert_eq!(doc.project(), &before, "one undo takes the whole drag back");
        // A locked clip takes no marker.
        doc.set_clips_locked(&[id], true);
        let locked = doc.project().clone();
        assert_eq!(
            doc.edit_warp_marker(&locked, id, MarkerEdit::Add(Ticks::quarters(2)), None),
            Err(Refused::Unchanged)
        );
    }

    #[test]
    fn a_drag_brought_back_to_where_it_began_puts_the_clip_back() {
        for slide in [false, true] {
            let (mut doc, id) = doc_with_clip();
            doc.warp_clips(&[id], 120.0);
            let before = doc.project().clone();
            let (base, index) = with_marker_edit(&before, id, MarkerEdit::Add(Ticks::quarters(2)))
                .expect("a marker");
            let index = index.expect("its place");
            let source = Document::new(base.clone()).project().tracks[0].clips[0]
                .warp
                .as_ref()
                .unwrap()
                .markers[index]
                .source;
            let (away, home) = if slide {
                (
                    MarkerEdit::Slide {
                        index,
                        source: source + 0.1,
                    },
                    MarkerEdit::Slide { index, source },
                )
            } else {
                (
                    MarkerEdit::Move {
                        index,
                        to: Ticks::quarters(3),
                    },
                    MarkerEdit::Move {
                        index,
                        to: Ticks::quarters(2),
                    },
                )
            };
            for edit in [away, home] {
                doc.edit_warp_marker(&base, id, edit, Some("warp marker"))
                    .expect("it follows the pointer");
            }
            assert_eq!(doc.project(), &base, "back where the gesture began");
            doc.undo();
            assert_eq!(doc.project(), &before, "and still one undo step");
        }
    }

    #[test]
    fn a_drag_keeps_what_changed_elsewhere_while_it_went_on() {
        let (mut doc, id) = doc_with_clip();
        doc.warp_clips(&[id], 120.0);
        let before = doc.project().clone();
        let (base, index) =
            with_marker_edit(&before, id, MarkerEdit::Add(Ticks::quarters(2))).expect("a marker");
        let index = index.expect("its place");
        let track = doc.project().tracks[0].id;
        let move_to = |to| MarkerEdit::Move {
            index,
            to: Ticks::quarters(to),
        };
        doc.edit_warp_marker(&base, id, move_to(3), Some("warp marker"))
            .unwrap();
        // A mapped controller moves a fader in the middle of the drag.
        doc.set_volume(track, -6.0);
        doc.edit_warp_marker(&base, id, move_to(4), Some("warp marker"))
            .unwrap();
        assert_eq!(doc.project().tracks[0].gain_db, -6.0, "an ordinary update");
        assert_eq!(warp_of(&doc).unwrap().markers[index].at, Ticks::quarters(4));
        // And a return to the start puts back the clip, not the fader.
        doc.edit_warp_marker(&base, id, move_to(2), Some("warp marker"))
            .unwrap();
        assert_eq!(doc.project().tracks[0].gain_db, -6.0, "the return");
        assert_eq!(
            doc.project().tracks[0].clips[0],
            base.tracks[0].clips[0],
            "the clip is as the gesture began"
        );
    }

    /// A drag of the transpose passes one merge key, so it is one undo step.
    #[test]
    fn a_drag_of_the_transpose_is_one_undo_step() {
        let (mut doc, id) = doc_with_clip();
        doc.warp_clips(&[id], 120.0);
        doc.set_warp(&[id], &Change::Mode(WarpMode::Melodic), None);
        for cents in [100, 200, 300] {
            doc.set_warp(&[id], &Change::Transpose(cents), Some("warp transpose"));
        }
        assert_eq!(warp_of(&doc).unwrap().transpose_cents, 300);
        doc.undo();
        assert_eq!(warp_of(&doc).unwrap().transpose_cents, 0);
        assert_eq!(warp_of(&doc).unwrap().mode, WarpMode::Melodic);
    }

    /// Moving the transpose keeps what is there: it adds to each clip's own,
    /// stops at four octaves, and leaves a clip that cannot take one.
    #[test]
    fn transposing_by_a_step_adds_to_the_clips_own_and_stops_at_four_octaves() {
        let (mut doc, id) = doc_with_clip();
        doc.warp_clips(&[id], 120.0);
        // Repitch cannot transpose: nothing changes and nothing is refused
        // (it is the same as before), so no step is made.
        let last = doc.history().undo_label().map(str::to_owned);
        assert_eq!(doc.transpose_warp(&[id], 100).changed, 0);
        assert_eq!(
            doc.history().undo_label().map(str::to_owned),
            last,
            "no step for nothing"
        );
        doc.set_warp(&[id], &Change::Mode(WarpMode::Melodic), None);
        doc.set_warp(&[id], &Change::Transpose(250), None);
        assert_eq!(doc.transpose_warp(&[id], 100).changed, 1);
        assert_eq!(warp_of(&doc).unwrap().transpose_cents, 350, "added to 250");
        doc.set_warp(&[id], &Change::Transpose(4_750), None);
        assert_eq!(doc.transpose_warp(&[id], 100).changed, 1);
        assert_eq!(
            warp_of(&doc).unwrap().transpose_cents,
            4_800,
            "held at four octaves"
        );
        assert_eq!(doc.transpose_warp(&[id], 100).changed, 0, "and no further");
        assert_eq!(doc.transpose_warp(&[id], -100).changed, 1);
        assert_eq!(warp_of(&doc).unwrap().transpose_cents, 4_700);
    }

    #[test]
    fn settings_markers_and_refusals() {
        let (mut doc, id) = doc_with_clip();
        doc.warp_clips(&[id], 120.0);
        assert_eq!(
            doc.set_warp(&[id], &Change::Mode(WarpMode::Melodic), None)
                .changed,
            1
        );
        assert_eq!(
            doc.set_warp(&[id], &Change::Transpose(300), None).changed,
            1
        );
        assert_eq!(warp_of(&doc).unwrap().transpose_cents, 300);
        // Repitch cannot transpose; a range out of bounds is refused.
        assert_eq!(
            doc.set_warp(&[id], &Change::OriginalBpm(5.0), None).refused,
            1
        );
        doc.set_warp(&[id], &Change::Mode(WarpMode::Repitch), None);
        assert_eq!(warp_of(&doc).unwrap().transpose_cents, 0);
        assert_eq!(
            doc.set_warp(&[id], &Change::Transpose(100), None).refused,
            1
        );
        // Markers: the same map changes nothing; a slowed one is made, and one
        // that would run the clip past its file is refused.
        let map = warp_of(&doc).unwrap().markers;
        assert!(!doc.set_warp_markers(id, map.clone()));
        let mut stretched = map.clone();
        stretched.push(Marker::new(
            map[0].source + 0.25,
            map[0].at + Ticks::quarters(1),
        ));
        let mut too_fast = map.clone();
        too_fast.push(Marker::new(
            map[0].source + 4.0,
            map[0].at + Ticks::quarters(1),
        ));
        assert!(
            !doc.set_warp_markers(id, too_fast),
            "past the end of its file"
        );
        assert!(doc.set_warp_markers(id, stretched.clone()));
        assert_eq!(warp_of(&doc).unwrap().markers, stretched);
        assert!(
            !doc.set_warp_markers(id, Vec::new()),
            "no markers is refused"
        );
        doc.undo();
        assert_eq!(warp_of(&doc).unwrap().markers, map);
        assert_eq!(doc.project().tracks[0].clips[0].warp, warp_of(&doc));
        lumen_project::validate(doc.project()).unwrap();
    }
}
