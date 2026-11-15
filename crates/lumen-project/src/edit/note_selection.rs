use std::sync::Arc;

use crate::model::{Clip, ClipContent, Id, Project};

#[derive(Debug, Clone)]
pub struct NoteSelection {
    clip: Arc<Clip>,
    indices: Vec<usize>,
}

impl NoteSelection {
    pub fn new(clip: Arc<Clip>, indices: Vec<usize>) -> Self {
        Self { clip, indices }
    }

    pub fn in_clip(project: &Project, id: Id) -> Option<Self> {
        find(project, id).map(|clip| Self::new(clip, Vec::new()))
    }

    pub fn clip(&self) -> Id {
        self.clip.id
    }

    pub fn indices(&self) -> &[usize] {
        &self.indices
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

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

        let mixed = crate::testing::synthetic(7, 3, 1, 2, 0);
        let audio = mixed.tracks[2].clips[0].id;
        assert!(matches!(
            mixed.tracks[2].clips[0].content,
            ClipContent::Audio { .. }
        ));
        assert!(NoteSelection::in_clip(&mixed, audio).is_none());
    }
}
