//! When a take's sidecar may go (M9 review H3).
//!
//! A take's sidecar (`<file>.take`, `take_worker.rs`) is how recovery finds
//! the take after a crash. It must outlive the moment the take is placed:
//! until a save or an autosave that holds the clip has been written, the only
//! record of the take is the sidecar. So a placed take's sidecar is *held*
//! here, and *claimed* by the next save or autosave that starts; when that
//! write succeeds the sidecars it claimed are deleted (on a thread of their
//! own), and when it fails they are held again for the next one.
//!
//! A held sidecar belongs to one document. When the document is replaced
//! (another project opens, New), the user has saved it or chosen not to, so
//! its sidecars go. While any are held, or a take is being recorded, the app
//! autosaves even a project with no unsaved changes, so a crash always leaves
//! an autosave for recovery to start from (H4).

use std::path::PathBuf;

use crate::recording::take_worker::sidecar_path;

#[derive(Debug, Clone, PartialEq)]
struct Held {
    sidecar: PathBuf,
    document: u64,
}

/// The sidecars of placed takes that no finished save holds yet.
#[derive(Debug, Default)]
pub(crate) struct TakeNotes {
    held: Vec<Held>,
    autosaving: Vec<Held>,
    saving: Vec<Held>,
}

impl TakeNotes {
    /// The takes in `files` were placed in document `document`.
    pub(crate) fn hold(&mut self, document: u64, files: impl IntoIterator<Item = PathBuf>) {
        for file in files {
            let sidecar = sidecar_path(&file);
            if !self.all().any(|h| h.sidecar == sidecar) {
                self.held.push(Held { sidecar, document });
            }
        }
    }

    fn all(&self) -> impl Iterator<Item = &Held> {
        self.held.iter().chain(&self.autosaving).chain(&self.saving)
    }

    /// Some take is not yet in a finished save: autosave, however clean the
    /// project.
    pub(crate) fn any(&self) -> bool {
        self.all().next().is_some()
    }

    /// The document is now `document`: the sidecars held for any other are
    /// returned, to delete.
    pub(crate) fn keep_only(&mut self, document: u64) -> Vec<PathBuf> {
        let mut gone = Vec::new();
        for list in [&mut self.held, &mut self.autosaving, &mut self.saving] {
            list.retain(|h| {
                let keep = h.document == document;
                if !keep {
                    gone.push(h.sidecar.clone());
                }
                keep
            });
        }
        gone
    }

    /// An autosave has started: it holds every take placed so far.
    pub(crate) fn autosave_started(&mut self) {
        self.autosaving.append(&mut self.held);
    }

    /// The autosave finished: returns the sidecars to delete.
    pub(crate) fn autosave_done(&mut self, written: bool) -> Vec<PathBuf> {
        let claimed = std::mem::take(&mut self.autosaving);
        Self::settle(&mut self.held, claimed, written)
    }

    /// A save of the project has started: it holds every take placed so
    /// far.
    pub(crate) fn save_started(&mut self) {
        self.saving.append(&mut self.held);
    }

    /// The save finished: returns the sidecars to delete.
    pub(crate) fn save_done(&mut self, written: bool) -> Vec<PathBuf> {
        let claimed = std::mem::take(&mut self.saving);
        Self::settle(&mut self.held, claimed, written)
    }

    fn settle(held: &mut Vec<Held>, claimed: Vec<Held>, written: bool) -> Vec<PathBuf> {
        if written {
            claimed.into_iter().map(|h| h.sidecar).collect()
        } else {
            held.extend(claimed);
            Vec::new()
        }
    }

    /// A normal close: nothing is left to recover. Returns every sidecar.
    pub(crate) fn close(&mut self) -> Vec<PathBuf> {
        let all: Vec<PathBuf> = self.all().map(|h| h.sidecar.clone()).collect();
        *self = Self::default();
        all
    }
}

/// Deletes `files` on a thread of their own: the interface never waits on
/// the disk (H1).
pub(crate) fn delete_later(files: Vec<PathBuf>) {
    if files.is_empty() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("lumen-take-notes".into())
        .spawn(move || {
            for file in files {
                let _ = std::fs::remove_file(file);
            }
        });
    // A thread that cannot start leaves the sidecars: recovery skips a take
    // a project already holds, so a sidecar left over does no harm.
    drop(spawned);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(n: u32) -> PathBuf {
        PathBuf::from(format!("Vox {n}.wav"))
    }

    fn side(n: u32) -> PathBuf {
        sidecar_path(&take(n))
    }

    #[test]
    fn a_take_is_let_go_only_when_a_save_that_holds_it_is_written() {
        let mut notes = TakeNotes::default();
        assert!(!notes.any());
        notes.hold(1, [take(1)]);
        assert!(notes.any(), "placed, not yet saved: kept");
        notes.autosave_started();
        // Placed while that autosave is written: not in it.
        notes.hold(1, [take(2)]);
        assert_eq!(notes.autosave_done(true), vec![side(1)]);
        assert!(notes.any(), "the second take waits for the next save");
        notes.save_started();
        assert_eq!(notes.save_done(true), vec![side(2)]);
        assert!(!notes.any());
    }

    #[test]
    fn a_save_that_fails_keeps_them_for_the_next() {
        let mut notes = TakeNotes::default();
        notes.hold(1, [take(1)]);
        notes.save_started();
        assert!(notes.save_done(false).is_empty());
        notes.autosave_started();
        assert!(notes.autosave_done(false).is_empty());
        notes.autosave_started();
        assert_eq!(notes.autosave_done(true), vec![side(1)]);
    }

    #[test]
    fn another_document_lets_its_takes_go_and_a_take_is_held_once() {
        let mut notes = TakeNotes::default();
        notes.hold(1, [take(1), take(1)]);
        notes.autosave_started();
        notes.hold(1, [take(1)]);
        notes.hold(2, [take(2)]);
        assert_eq!(notes.keep_only(2), vec![side(1)]);
        assert_eq!(notes.close(), vec![side(2)]);
        assert!(!notes.any());
    }
}
