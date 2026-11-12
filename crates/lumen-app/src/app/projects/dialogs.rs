//! The system file dialogs: what one returns, and the thread it runs on so a
//! frame never waits on the disk.

use std::cell::Cell;
use std::path::PathBuf;

use eframe::egui;
use lumen_core::time::Ticks;

use crate::App;
use crate::document::Places;

/// What a file dialog returned.
pub(crate) enum FileDialog {
    SaveAs(Option<PathBuf>),
    Open(Option<PathBuf>),
    /// Audio and MIDI files to import (none when cancelled), and where
    /// they go: the playhead and the chosen track when the dialog opened
    /// (M3 review H1, M5 slice 7).
    Import {
        paths: Vec<PathBuf>,
        at: Ticks,
        into: Places,
    },
    /// Where to export a MIDI file (none when cancelled), and its bytes,
    /// made when the command ran (M5 slice 7).
    ExportMidi {
        path: Option<PathBuf>,
        bytes: Vec<u8>,
    },
    /// A folder to search for plugins (none when cancelled; M6 slice 1).
    PluginFolder(Option<PathBuf>),
    /// Where to save a copy of the project (none when cancelled; M9 R4).
    SaveCopy(Option<PathBuf>),
    /// Where to collect an unsaved project (none when cancelled; M9 R4).
    Collect(Option<PathBuf>),
    /// The audio export file, or a folder for stems (M9 R5).
    ExportAudio(Option<PathBuf>),
    /// The file chosen for a missing item (none when cancelled; M9 R4b).
    Locate(Option<PathBuf>),
    /// The folder to search for the missing files (none when cancelled; M9 R4b).
    RelinkFolder(Option<PathBuf>),
    /// The folder the cache is kept in (none when cancelled; M9 R7).
    CacheFolder(Option<PathBuf>),
    /// The projects folder (none when cancelled; M9 R7).
    ProjectsFolder(Option<PathBuf>),
    /// A folder to add to Samples (none when cancelled; M9 R7c).
    SampleFolder(Option<PathBuf>),
    /// A folder to add as a place, or where a missing one went (none when
    /// cancelled; M9 R7c).
    Place(Option<PathBuf>),
    /// Where to write a drum map, or the file to read (none when
    /// cancelled; M8 D1).
    DrumMapExport(Option<PathBuf>),
    DrumMapImport(Option<PathBuf>),
}

thread_local! {
    /// Set on a file dialog's own thread by [`spawn_dialog`].
    static DIALOG_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// `dir`, made if it is not there yet, so a dialog can start in it.
///
/// Call it inside a dialog's `show` closure, which [`spawn_dialog`] runs on a
/// worker of its own: making the folder on the interface's thread would wait
/// on the disk in a frame (sweep M4c). A debug build says so when it is
/// called anywhere but a dialog's thread.
pub(crate) fn made(dir: PathBuf) -> PathBuf {
    debug_assert!(
        DIALOG_THREAD.with(Cell::get),
        "made() runs inside a file dialog's show closure, never on the interface's thread"
    );
    let _ = std::fs::create_dir_all(&dir);
    dir
}

impl App {
    /// Shows a system file dialog on its own thread (`show`). In a test it
    /// is answered at once instead, with the next answer the test gave or
    /// as if cancelled, through `answered`, which makes the result from the
    /// chosen paths: a test never opens a real dialog.
    pub(crate) fn file_dialog(
        &mut self,
        ctx: &egui::Context,
        show: impl FnOnce() -> FileDialog + Send + 'static,
        answered: impl FnOnce(Vec<PathBuf>) -> FileDialog,
    ) {
        self.dialog = match &mut self.dialog_answers {
            Some(answers) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = tx.send(answered(answers.pop_front().unwrap_or_default()));
                ctx.request_repaint();
                Some(rx)
            }
            None => match spawn_dialog(ctx, show) {
                Ok(rx) => Some(rx),
                Err(error) => {
                    // Nothing is waiting on a dialog that never opened.
                    self.save_dialog = false;
                    self.say(crate::spawn::start_failed("the file chooser", &error));
                    None
                }
            },
        };
    }
}

pub(crate) fn spawn_dialog(
    ctx: &egui::Context,
    show: impl FnOnce() -> FileDialog + Send + 'static,
) -> std::io::Result<std::sync::mpsc::Receiver<FileDialog>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    crate::spawn::spawn_named("lumen-file-dialog", move || {
        // The dialog's worker: where a folder it starts in is made
        // ([`made`], sweep M4c).
        DIALOG_THREAD.with(|own| own.set(true));
        let _ = tx.send(show());
        ctx.request_repaint();
    })?;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use lumen_project::testing::TempDir;

    use super::*;

    /// The folder a dialog starts in is made on the dialog's own thread, so
    /// the frame that opens it never waits on the disk (sweep M4c): a
    /// projects folder that is not there yet is made there, and the dialog
    /// is handed it.
    #[test]
    fn a_dialogs_folder_is_made_on_the_dialogs_thread() {
        let dir = TempDir::new("dialog-folder");
        let start = dir.path().join("Lumen").join("Projects");
        assert!(!start.exists(), "nothing has made it yet");
        let asked = start.clone();
        let caller = std::thread::current().id();
        let rx = spawn_dialog(&egui::Context::default(), move || {
            assert_ne!(
                std::thread::current().id(),
                caller,
                "the dialog runs on a thread of its own"
            );
            let folder = made(asked);
            FileDialog::SaveAs(folder.is_dir().then_some(folder))
        })
        .expect("the dialog's thread starts");
        let answer = rx
            .recv_timeout(lumen_testkit::patience())
            .expect("the dialog answers");
        assert!(start.is_dir(), "the folder the dialog starts in");
        assert!(matches!(answer, FileDialog::SaveAs(Some(_))));
    }
}
