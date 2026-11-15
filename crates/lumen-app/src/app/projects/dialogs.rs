use std::cell::Cell;
use std::path::PathBuf;

use eframe::egui;
use lumen_core::time::Ticks;

use crate::App;
use crate::document::Places;

pub(crate) enum FileDialog {
    SaveAs(Option<PathBuf>),
    Open(Option<PathBuf>),
    Import {
        paths: Vec<PathBuf>,
        at: Ticks,
        into: Places,
    },
    ExportMidi {
        path: Option<PathBuf>,
        bytes: Vec<u8>,
    },
    PluginFolder(Option<PathBuf>),
    SaveCopy(Option<PathBuf>),
    Collect(Option<PathBuf>),
    ExportAudio(Option<PathBuf>),
    Locate(Option<PathBuf>),
    RelinkFolder(Option<PathBuf>),
    CacheFolder(Option<PathBuf>),
    ProjectsFolder(Option<PathBuf>),
    SampleFolder(Option<PathBuf>),
    Place(Option<PathBuf>),
    DrumMapExport(Option<PathBuf>),
    DrumMapImport(Option<PathBuf>),
    /// Audio files for the chosen Shutter (none when cancelled; M8 slice B5).
    SamplerFiles {
        paths: Vec<PathBuf>,
        device: lumen_project::model::Id,
    },
}

thread_local! {
    static DIALOG_THREAD: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn made(dir: PathBuf) -> PathBuf {
    debug_assert!(
        DIALOG_THREAD.with(Cell::get),
        "made() runs inside a file dialog's show closure, never on the interface's thread"
    );
    let _ = std::fs::create_dir_all(&dir);
    dir
}

impl App {
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
