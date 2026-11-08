//! Warp (M9.5 slice S4a): the document's edits of a clip's warp. Each is one
//! undo step; one that changes nothing adds none; a locked clip is left as it
//! is. There is no control for them yet (the warp editor is S5), and the
//! engine does not play a warp until S4b: these keep the model true and
//! undoable so that what comes after has something to stand on.

// Nothing calls these until S4b and S5 give warp a control; a test does. The
// expectation fails the build the day something does, so it gets removed.
#![cfg_attr(not(test), expect(dead_code, reason = "no control for warp until S5"))]

use std::sync::Arc;

use lumen_core::warp::Marker;
use lumen_project::edit::warp::{self as edit, Change, Refused};
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

impl Document {
    /// Turns warp on for the selected audio clips, for files made at
    /// `original_bpm`. Nothing is stretched: each clip plays as it did.
    pub fn warp_clips(&mut self, ids: &[Id], original_bpm: f64) -> Warped {
        let mut out = Warped::default();
        self.history.apply("Warp clips", None, |project| {
            out = each(project, ids, &|p, c| edit::enable(p, c, original_bpm));
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
    /// the file was made at, transpose).
    pub fn set_warp(&mut self, ids: &[Id], change: &Change) -> Warped {
        let label = match change {
            Change::Mode(_) => "Set warp mode",
            Change::OriginalBpm(_) => "Set original tempo",
            Change::Transpose(_) => "Transpose clips",
        };
        let mut out = Warped::default();
        self.history.apply(label, None, |project| {
            out = each(project, ids, &|p, c| edit::set(p, c, change));
        });
        out
    }

    /// Replaces a warped clip's markers (the warp editor's drag, from S5).
    /// Returns whether the clip changed.
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
    fn settings_markers_and_refusals() {
        let (mut doc, id) = doc_with_clip();
        doc.warp_clips(&[id], 120.0);
        assert_eq!(
            doc.set_warp(&[id], &Change::Mode(WarpMode::Melodic))
                .changed,
            1
        );
        assert_eq!(doc.set_warp(&[id], &Change::Transpose(300)).changed, 1);
        assert_eq!(warp_of(&doc).unwrap().transpose_cents, 300);
        // Repitch cannot transpose; a range out of bounds is refused.
        assert_eq!(doc.set_warp(&[id], &Change::OriginalBpm(5.0)).refused, 1);
        doc.set_warp(&[id], &Change::Mode(WarpMode::Repitch));
        assert_eq!(warp_of(&doc).unwrap().transpose_cents, 0);
        assert_eq!(doc.set_warp(&[id], &Change::Transpose(100)).refused, 1);
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
