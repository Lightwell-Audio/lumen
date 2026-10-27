//! The media pool's edits (M9 slice R4b, ADR-0106): putting a file back where
//! the project lost it, and dropping the entries nothing plays.

use std::path::PathBuf;

use lumen_project::edit::media;
use lumen_project::model::Id;

use super::Document;

impl Document {
    /// Says that media `id` is the file at `path` on this machine. Nothing in
    /// the song changes, so it is not an undo step, but the project has
    /// unsaved changes: its next save writes the new path. The library loads
    /// the file on its own, and the clips that play it sound again.
    ///
    /// Returns false, changing nothing, when the project has no such media.
    pub fn relink(&mut self, id: Id, path: PathBuf) -> bool {
        if !self.history.project().media.iter().any(|m| m.id == id) {
            return false;
        }
        if self.media_files.get(&id) == Some(&path) {
            return true;
        }
        self.media_files.insert(id, path);
        self.mark_changed_inside();
        true
    }

    /// Drops from the project's media the entries of `ids` that no clip and no
    /// frozen track plays, as one step. The files are not touched. Returns how
    /// many were dropped; when none were, nothing is recorded.
    pub fn remove_unused_media(&mut self, ids: &[Id]) -> usize {
        let mut removed = 0;
        let label = match ids {
            [_] => "Remove unused file".to_owned(),
            _ => "Remove unused files".to_owned(),
        };
        self.history.apply(&label, None, |project| {
            removed = media::remove_unused(project, ids).len();
        });
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::model::Media;

    fn entry(id: u64) -> Media {
        Media {
            id: Id(id),
            path: format!("Samples/{id}.wav"),
            original_path: None,
            frames: 100,
            channels: 1,
            sample_rate: 48_000,
            size_bytes: 1,
        }
    }

    fn doc_with_unused(ids: &[u64]) -> Document {
        let mut project = lumen_project::testing::small_project(1);
        project.media = ids.iter().map(|&id| entry(id)).collect();
        Document::new(project)
    }

    #[test]
    fn relinking_is_an_unsaved_change_with_nothing_to_undo() {
        let mut doc = doc_with_unused(&[900]);
        assert!(!doc.is_dirty());
        let before = doc.change_mark();
        assert!(doc.relink(Id(900), PathBuf::from("D:/moved/900.wav")));
        assert_eq!(
            doc.media_files.get(&Id(900)),
            Some(&PathBuf::from("D:/moved/900.wav"))
        );
        assert!(doc.is_dirty(), "the next save must write the new path");
        assert_ne!(doc.change_mark(), before, "and an autosave must too");
        assert!(
            doc.undo().is_none(),
            "where a file is, not what the song is"
        );
    }

    #[test]
    fn relinking_what_the_project_does_not_hold_changes_nothing() {
        let mut doc = doc_with_unused(&[900]);
        assert!(!doc.relink(Id(5), PathBuf::from("x.wav")));
        assert!(doc.media_files.is_empty());
        assert!(!doc.is_dirty());
    }

    #[test]
    fn relinking_to_where_it_already_is_is_no_change() {
        let mut doc = doc_with_unused(&[900]);
        doc.relink(Id(900), PathBuf::from("a.wav"));
        let mark = doc.change_mark();
        assert!(doc.relink(Id(900), PathBuf::from("a.wav")));
        assert_eq!(doc.change_mark(), mark);
    }

    #[test]
    fn removing_unused_media_is_one_undo_step() {
        let mut doc = doc_with_unused(&[900, 901, 902]);
        let before = doc.project().clone();
        assert_eq!(doc.remove_unused_media(&[Id(900), Id(902)]), 2);
        let left: Vec<_> = doc.project().media.iter().map(|m| m.id).collect();
        assert_eq!(left, [Id(901)]);
        assert!(doc.undo().is_some());
        assert_eq!(doc.project(), &before);
        assert!(doc.undo().is_none(), "one step");
    }

    #[test]
    fn removing_nothing_records_nothing() {
        let mut doc = doc_with_unused(&[900]);
        assert_eq!(doc.remove_unused_media(&[Id(7)]), 0);
        assert!(doc.undo().is_none());
        assert!(!doc.is_dirty());
    }
}
