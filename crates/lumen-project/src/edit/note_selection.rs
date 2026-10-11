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
