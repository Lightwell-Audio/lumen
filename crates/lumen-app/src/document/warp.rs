use std::sync::Arc;

#[cfg(test)]
use lumen_core::warp::Marker;
use lumen_project::edit::warp::{self as edit, Change, Refused};
use lumen_project::edit::warp_markers::{Edited, MarkerEdit, edit_marker, fix_hit};
use lumen_project::model::{Clip, Id, Project};

use super::Document;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Warped {
    pub changed: usize,
    pub refused: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkerDone {
    pub index: Option<usize>,
    pub held: bool,
}

fn marker_edited(project: &Project, id: Id, change: MarkerEdit) -> Result<Edited, Refused> {
    let clip = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id == id && !c.locked)
        .ok_or(Refused::Unchanged)?;
    edit_marker(project, clip, change)
}

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

fn clip_in(project: &Project, id: Id) -> Option<&Clip> {
    project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id == id)
        .map(|c| &**c)
}

impl Document {
    #[cfg(test)]
    pub fn warp_clips(&mut self, ids: &[Id], original_bpm: f64) -> Warped {
        self.warp_clips_by(ids, &|_| original_bpm)
    }

    pub fn warp_clips_by(&mut self, ids: &[Id], original_bpm: &dyn Fn(&Clip) -> f64) -> Warped {
        let mut out = Warped::default();
        self.history.apply("Warp clips", None, |project| {
            out = each(project, ids, &|p, c| {
                let on = edit::enable(p, c, original_bpm(c))?;
                Ok(edit::fit_to_file(p, &on))
            });
        });
        out
    }

    pub fn unwarp_clips(&mut self, ids: &[Id]) -> Warped {
        let mut out = Warped::default();
        self.history.apply("Turn warp off", None, |project| {
            out = each(project, ids, &|_, c| edit::disable(c));
        });
        out
    }

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
        self.history.apply(label, merge, |project| {
            super::clips::replace_clip(project, id, |_| edited.clip.clone());
        });
        Ok(done)
    }

    pub fn fix_warp_hit(
        &mut self,
        id: Id,
        hit: f64,
        sides: (Option<f64>, Option<f64>),
    ) -> Result<MarkerDone, Refused> {
        let (marked, index) = {
            let project = self.project();
            let clip = project
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .find(|c| c.id == id && !c.locked)
                .ok_or(Refused::Unchanged)?;
            let fixed = fix_hit(clip, hit, sides)?;
            let marked = edit::set_markers(project, clip, fixed.markers)?;
            (marked, fixed.hit)
        };
        self.history.apply("Fix this hit", None, |project| {
            super::clips::replace_clip(project, id, |_| marked.clone());
        });
        Ok(MarkerDone { index, held: false })
    }

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
    fn warping_to_a_faster_tempo_than_the_file_cuts_the_clip_in_the_same_step() {
        let (mut doc, id) = doc_with_clip();
        let plain = doc.project().clone();
        let length = plain.tracks[0].clips[0].length;
        assert_eq!(doc.warp_clips(&[id], 30.0).changed, 1);
        let cut = doc.project().tracks[0].clips[0].length;
        assert!(
            cut < length,
            "the clip follows the sound: {cut:?} < {length:?}"
        );
        doc.undo();
        assert_eq!(doc.project(), &plain);
        assert_eq!(doc.warp_clips(&[id], 240.0).changed, 1);
        assert_eq!(doc.project().tracks[0].clips[0].length, length);
        assert_eq!(doc.scale_warp_tempo(&[id], 0.25).changed, 1);
        assert!(doc.project().tracks[0].clips[0].length < length);
        lumen_project::validate(doc.project()).unwrap();
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
        doc.set_volume(track, -6.0);
        doc.edit_warp_marker(&base, id, move_to(4), Some("warp marker"))
            .unwrap();
        assert_eq!(doc.project().tracks[0].gain_db, -6.0, "an ordinary update");
        assert_eq!(warp_of(&doc).unwrap().markers[index].at, Ticks::quarters(4));
        doc.edit_warp_marker(&base, id, move_to(2), Some("warp marker"))
            .unwrap();
        assert_eq!(doc.project().tracks[0].gain_db, -6.0, "the return");
        assert_eq!(
            doc.project().tracks[0].clips[0],
            base.tracks[0].clips[0],
            "the clip is as the gesture began"
        );
    }

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

    #[test]
    fn fixing_a_hit_makes_its_three_markers_in_one_step_and_undo_restores_the_map() {
        let (mut doc, id) = doc_with_clip();
        doc.warp_clips(&[id], 120.0);
        let map = vec![
            Marker::new(0.0, Ticks::ZERO),
            Marker::new(2.0, Ticks::quarters(4)),
        ];
        assert!(doc.set_warp_markers(id, map.clone()));
        let before = doc.project().clone();
        let beats = |markers: &[Marker], sources: &[f64]| -> Vec<Ticks> {
            sources
                .iter()
                .map(|s| lumen_core::warp::tick_at_source(markers, 120.0, *s))
                .collect()
        };
        let was = beats(&map, &[0.4, 0.7, 1.2]);
        let done = doc
            .fix_warp_hit(id, 0.7, (Some(0.4), Some(1.2)))
            .expect("the hit and its sides");
        assert_eq!(done.index, Some(2), "the hit's own marker");
        let fixed = warp_of(&doc).unwrap().markers;
        assert_eq!(fixed.len(), 5, "the hit, both sides and the two there were");
        assert_eq!(beats(&fixed, &[0.4, 0.7, 1.2]), was, "no hit moved");
        assert_eq!(doc.history().undo_label(), Some("Fix this hit"));
        assert_eq!(
            doc.fix_warp_hit(id, 0.7, (Some(0.4), Some(1.2))),
            Err(Refused::Unchanged)
        );
        assert_eq!(doc.history().undo_label(), Some("Fix this hit"));
        doc.undo();
        assert_eq!(doc.project(), &before, "one undo puts the whole map back");
        doc.set_clips_locked(&[id], true);
        assert_eq!(
            doc.fix_warp_hit(id, 0.7, (Some(0.4), Some(1.2))),
            Err(Refused::Unchanged)
        );
    }

    #[test]
    fn transposing_by_a_step_adds_to_the_clips_own_and_stops_at_four_octaves() {
        let (mut doc, id) = doc_with_clip();
        doc.warp_clips(&[id], 120.0);
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
