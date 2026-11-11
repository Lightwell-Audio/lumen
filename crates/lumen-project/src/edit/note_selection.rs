//! A selection of notes that knows when it has gone stale (M5 slice 3a).
//!
//! Notes are picked by index (see `edit::notes`), and an index means
//! nothing once the list has changed. A [`NoteSelection`] remembers the
//! clip as it was when the picks were made; before using it, the editor
//! calls [`NoteSelection::refresh`] with the project. If the clip is the
//! very same one (or an edit elsewhere left its notes as they were: a
//! rename, a move, a recolour) the picks stand; if its notes changed (an
//! undo, a redo, another editor) they are cleared. After an edit of its
//! own, the editor stores the indices the edit returned with
//! [`NoteSelection::rebind`], so the selection follows its own edits.

use std::sync::Arc;

use crate::model::{Clip, ClipContent, Id, Project};

#[derive(Debug, Clone)]
pub struct NoteSelection {
    clip: Arc<Clip>,
    indices: Vec<usize>,
}

impl NoteSelection {
    /// `indices` of the notes of `clip`, as the clip is now.
    pub fn new(clip: Arc<Clip>, indices: Vec<usize>) -> Self {
        Self { clip, indices }
    }

    /// An empty selection in the clip with id `id` of `project`, or `None`
    /// if there is no such clip.
    pub fn in_clip(project: &Project, id: Id) -> Option<Self> {
        find(project, id).map(|clip| Self::new(clip, Vec::new()))
    }

    /// The clip the notes belong to.
    pub fn clip(&self) -> Id {
        self.clip.id
    }

    /// The picked indices, sorted. Use only after [`Self::refresh`] said the
    /// selection is current.
    pub fn indices(&self) -> &[usize] {
        &self.indices
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Replaces the picks (after an edit returned its resulting indices, or
    /// a click) for the clip as `project` has it now. Returns whether the
    /// clip still exists; if it does not the selection is emptied.
    pub fn rebind(&mut self, project: &Project, indices: Vec<usize>) -> bool {
        match find(project, self.clip.id) {
            Some(clip) => {
                let len = notes_of(&clip).len();
                self.clip = clip;
                self.indices = super::notes::picked(&indices, len);
                true
            }
            None => {
                self.indices.clear();
                false
            }
        }
    }

    /// Brings the selection up to date with `project` and says whether the
    /// picks were kept. Cheap when nothing changed (a pointer comparison);
    /// when the clip was replaced, its notes are compared, so edits that
    /// leave the notes alone keep the selection and anything else clears it.
    pub fn refresh(&mut self, project: &Project) -> bool {
        let Some(clip) = find(project, self.clip.id) else {
            self.indices.clear();
            return false;
        };
        if Arc::ptr_eq(&clip, &self.clip) {
            return true;
        }
        let same = notes_of(&clip) == notes_of(&self.clip);
        self.clip = clip;
        if !same {
            self.indices.clear();
        }
        same
    }
}

fn find(project: &Project, id: Id) -> Option<Arc<Clip>> {
    project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id && matches!(c.content, ClipContent::Midi { .. }))
        .cloned()
}

fn notes_of(clip: &Clip) -> &[crate::model::Note] {
    match &clip.content {
        ClipContent::Midi { notes, .. } => notes,
        ClipContent::Audio { .. } => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Note;
    use crate::testing::{midi_track, project_of};
    use lumen_core::time::{PPQ, Ticks};

    fn note(beat: i64, pitch: u8) -> Note {
        Note::new(Ticks::quarters(beat), Ticks(PPQ / 2), pitch, 100)
    }

    /// A project with one MIDI track whose clip holds `notes`, and the clip's
    /// id.
    fn project(notes: Vec<Note>) -> (Project, Id) {
        let track = midi_track(1, "Lead", "keys", (0, 8), None, notes, Vec::new());
        let id = track.clips[0].id;
        (project_of("Notes", vec![track]), id)
    }

    #[test]
    fn refresh_keeps_the_picks_when_only_the_clip_arc_changed() {
        let (mut project, clip) = project(vec![note(0, 60), note(2, 62), note(4, 64)]);
        let mut selection = NoteSelection::in_clip(&project, clip).unwrap();
        assert!(selection.refresh(&project), "nothing changed");
        selection.rebind(&project, vec![0, 2]);
        assert_eq!(selection.indices(), [0, 2]);

        // An edit elsewhere (a rename, a move) replaces the clip but leaves
        // its notes as they were: the picks stand.
        let track = Arc::make_mut(&mut project.tracks[0]);
        let edited = Arc::make_mut(&mut track.clips[0]);
        edited.name = "Verse".into();
        edited.start = Ticks::quarters(4);

        assert!(selection.refresh(&project), "the notes are the same");
        assert_eq!(selection.indices(), [0, 2], "the picks stand");
    }

    #[test]
    fn refresh_clears_the_picks_when_the_notes_change_or_the_clip_goes() {
        let (mut project, clip) = project(vec![note(0, 60), note(2, 62)]);
        let mut selection = NoteSelection::in_clip(&project, clip).unwrap();
        selection.rebind(&project, vec![1]);
        let track = Arc::make_mut(&mut project.tracks[0]);
        let edited = Arc::make_mut(&mut track.clips[0]);
        if let ClipContent::Midi { notes, .. } = &mut edited.content {
            notes[1] = note(3, 65);
        }
        assert!(!selection.refresh(&project));
        assert!(selection.is_empty(), "stale picks are dropped");

        // The clip going away clears them too.
        selection.rebind(&project, vec![0]);
        let track = Arc::make_mut(&mut project.tracks[0]);
        track.clips.clear();
        assert!(!selection.refresh(&project));
        assert!(selection.is_empty());
    }

    #[test]
    fn rebind_sorts_drops_repeats_and_ignores_indices_past_the_end() {
        let (project, clip) = project(vec![note(0, 60), note(2, 62), note(4, 64)]);
        let mut selection = NoteSelection::in_clip(&project, clip).unwrap();
        assert!(selection.rebind(&project, vec![2, 0, 2, 99]));
        assert_eq!(selection.indices(), [0, 2]);
        assert!(selection.refresh(&project), "the rebound clip is current");
        assert_eq!(selection.clip(), clip);
    }

    #[test]
    fn a_missing_or_non_midi_clip_leaves_no_selection() {
        let (project, clip) = project(vec![note(0, 60)]);
        assert!(NoteSelection::in_clip(&project, Id(999)).is_none());
        let mut gone = project.clone();
        gone.tracks.clear();
        let mut selection = NoteSelection::in_clip(&project, clip).unwrap();
        assert!(!selection.rebind(&gone, vec![0]), "the clip is not there");
        assert!(selection.is_empty());
        assert!(!selection.refresh(&gone));
        assert!(selection.is_empty());

        // An audio clip holds no notes to pick.
        let mixed = crate::testing::synthetic(7, 3, 1, 2, 0);
        let audio = mixed.tracks[2].clips[0].id;
        assert!(matches!(
            mixed.tracks[2].clips[0].content,
            ClipContent::Audio { .. }
        ));
        assert!(NoteSelection::in_clip(&mixed, audio).is_none());
    }
}
