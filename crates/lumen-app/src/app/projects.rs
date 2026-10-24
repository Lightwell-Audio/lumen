//! New, open, save and save as; the unsaved-changes prompt and the file
//! banner (M3 slice 3c). Saving and opening run in the background
//! (`crate::files`); file dialogs run on their own thread, owned by the main
//! window (`crate::owner`).

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, RichText, pos2};
use lumen_core::product::PRODUCT_NAME;
use lumen_core::time::Ticks;
use lumen_project::model::{Project, TrackKind};

use crate::document::Places;
use crate::widgets::Dialog;
use crate::widgets::raised;
use crate::{
    App, arrangement, document, empty_project, files, lanes, media, midi_files, theme, trace,
};

/// What happens after unsaved changes are saved or discarded.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Next {
    New,
    /// Open this project, or ask which.
    Open(Option<PathBuf>),
    Quit,
}

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
}

/// Where file dialogs start when the project has no folder yet.
pub(crate) fn documents_folder() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join("Documents"))
}

/// How long a save waits for the plugins' states to be read. Each read has
/// its own deadline in the host; this is the whole wait.
const PLUGIN_READ_WAIT: std::time::Duration = std::time::Duration::from_secs(4);

impl App {
    /// The project as it should be saved: the document with the view
    /// stamped in (it is not an undo step).
    pub(crate) fn stamped(&self) -> Project {
        let mut project = self.document.project().clone();
        project.view = lumen_project::model::ViewState {
            left_beat: self.nav.view.left as f32,
            px_per_beat: self.nav.view.zoom as f32,
            browser_open: self.show_browser,
            detail_open: self.show_detail,
            browser_size: self.browser_size,
            detail_size: self.detail_size,
            grid: self.grid,
            ..project.view.clone()
        };
        self.arrangement.save_into(&mut project.view);
        project.view.folded_devices = self.chain.folded.iter().copied().collect();
        project.view.shown_params = self.chain.shown.clone();
        // What the running plugins report goes in with the view: it is not an
        // edit (M6 slice 2c, the owner's C2).
        self.hosting.stamp(&mut project);
        project
    }

    /// Save (Ctrl+S): to the project's file, or ask where (Save As).
    pub(crate) fn save(&mut self, ctx: &egui::Context, ask: bool) {
        if self.saving.is_some() || self.save_waiting.is_some() || self.dialog.is_some() {
            return;
        }
        match self.document.path.clone() {
            Some(path) if !ask => self.start_save(path),
            _ => {
                let name = format!("{}.{}", self.document.name(), files::EXTENSION);
                let start = self
                    .document
                    .path
                    .as_deref()
                    .and_then(|p| p.parent()?.parent().map(PathBuf::from))
                    .or_else(documents_folder);
                let owner = self.owner;
                let show = move || {
                    let mut dialog = rfd::FileDialog::new()
                        .set_title("Save project")
                        .add_filter(format!("{PRODUCT_NAME} project"), &[files::EXTENSION])
                        .set_file_name(name);
                    if let Some(dir) = start {
                        dialog = dialog.set_directory(dir);
                    }
                    if let Some(owner) = &owner {
                        dialog = dialog.set_parent(owner);
                    }
                    FileDialog::SaveAs(dialog.save_file())
                };
                self.file_dialog(ctx, show, |paths| {
                    FileDialog::SaveAs(paths.into_iter().next())
                });
            }
        }
    }

    /// Import audio or MIDI (M3 review H1, M5 slice 7): the system's file
    /// dialog, then the chosen files load as dropped files do, at the
    /// playhead, onto the selected track (or the track of the selected
    /// clip) when it takes them, else onto new tracks after the selection.
    pub(crate) fn import_files(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        let at = self.transport.state().ticks;
        let project = self.document.project();
        let chosen = self.arrangement.selected.last().copied().or_else(|| {
            let clip = self.first_selected_clip()?;
            let track = project
                .tracks
                .iter()
                .find(|t| t.clips.iter().any(|c| c.id == clip))?;
            Some(track.id)
        });
        let into = Places {
            audio: document::import_place(project, chosen, TrackKind::Audio),
            midi: document::import_place(project, chosen, TrackKind::Midi),
        };
        let owner = self.owner;
        let show = move || {
            let both: Vec<&str> = media::EXTENSIONS
                .iter()
                .chain(midi_files::EXTENSIONS)
                .copied()
                .collect();
            let mut dialog = rfd::FileDialog::new()
                .set_title("Import audio or MIDI")
                .add_filter("Audio and MIDI", &both)
                .add_filter("Audio", media::EXTENSIONS)
                .add_filter("MIDI", midi_files::EXTENSIONS);
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            let paths = dialog.pick_files().unwrap_or_default();
            FileDialog::Import { paths, at, into }
        };
        self.file_dialog(ctx, show, move |paths| FileDialog::Import {
            paths,
            at,
            into,
        });
    }

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
        self.dialog = Some(match &mut self.dialog_answers {
            Some(answers) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = tx.send(answered(answers.pop_front().unwrap_or_default()));
                ctx.request_repaint();
                rx
            }
            None => spawn_dialog(ctx, show),
        });
    }

    pub(crate) fn start_save(&mut self, path: PathBuf) {
        self.start_save_with(path, false);
    }

    pub(crate) fn start_save_as(&mut self, chosen: PathBuf) {
        self.start_save_with(chosen, true);
    }

    fn start_save_with(&mut self, path: PathBuf, normalize_project_path: bool) {
        // Every running plugin is read first, and the save is written when
        // the answers are in (or after a few seconds, with what there is).
        if self.hosting.request_captures() {
            trace::log(|| {
                format!(
                    "files: reading the plugins before saving {}",
                    path.display()
                )
            });
            self.save_waiting = Some((path, normalize_project_path, Instant::now()));
            return;
        }
        self.write_save(path, normalize_project_path);
    }

    /// Writes the save, with the plugins' states as they were last read.
    fn write_save(&mut self, path: PathBuf, normalize_project_path: bool) {
        trace::log(|| format!("files: saving {}", path.display()));
        self.document.begin_save();
        self.saving = Some(files::save(files::SaveRequest {
            project: self.stamped(),
            path,
            normalize_project_path,
            media_files: self.document.media_files.clone(),
            copy_imported: self.settings.projects.copy_imported,
            renders: Some(self.renders_dir()),
            keep_backups: lumen_project::SaveOptions::default().keep_backups,
            state: self.document.state(),
        }));
    }

    /// New, Open or Quit: first offer to save unsaved changes.
    pub(crate) fn request(&mut self, ctx: &egui::Context, next: Next) {
        if self.document.is_dirty() {
            self.prompt = Some(next);
        } else {
            self.proceed(ctx, next);
        }
    }

    pub(crate) fn proceed(&mut self, ctx: &egui::Context, next: Next) {
        match next {
            Next::New => {
                self.replace_document(document::Document::new(empty_project()));
                self.toast = Some(("New project".into(), Instant::now()));
            }
            Next::Open(Some(path)) => {
                if self.opening.is_none() {
                    trace::log(|| format!("files: opening {}", path.display()));
                    self.opening = Some(files::open(path));
                }
            }
            Next::Open(None) => {
                if self.dialog.is_some() {
                    return;
                }
                let start = self
                    .document
                    .path
                    .as_deref()
                    .and_then(|p| p.parent()?.parent().map(PathBuf::from))
                    .or_else(documents_folder);
                let owner = self.owner;
                let show = move || {
                    let mut dialog = rfd::FileDialog::new()
                        .set_title("Open project")
                        .add_filter(format!("{PRODUCT_NAME} project"), &[files::EXTENSION]);
                    if let Some(dir) = start {
                        dialog = dialog.set_directory(dir);
                    }
                    if let Some(owner) = &owner {
                        dialog = dialog.set_parent(owner);
                    }
                    FileDialog::Open(dialog.pick_file())
                };
                self.file_dialog(ctx, show, |paths| {
                    FileDialog::Open(paths.into_iter().next())
                });
            }
            Next::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Swaps in another project: its media load afresh, the playhead goes
    /// to the start, and the transport follows its tempo.
    pub(crate) fn replace_document(&mut self, document: document::Document) {
        self.document = document;
        self.lanes = lanes::Lanes::default();
        self.arrangement = arrangement::Arrangement::default();
        self.chain.folded.clear();
        self.chain.shown.clear();
        self.chain.panel = None;
        self.chain.selected = None;
        self.synced = None;
        self.normalizing = None;
        self.consolidating = None;
        self.bounce_waiting = None;
        self.bouncing = None;
        self.show_bounce = false;
        self.freeze_waiting = None;
        self.freezing = None;
        self.show_history = false;
        self.forget_passes();
        self.transport.locate(Ticks::ZERO);
    }

    /// Results of dialogs, saves and opens; and the window's close button.
    pub(crate) fn poll_files(&mut self, ctx: &egui::Context) {
        if let Some((_, _, since)) = &self.save_waiting {
            if !self.hosting.captures_pending() || since.elapsed() >= PLUGIN_READ_WAIT {
                if let Some((path, normalize, _)) = self.save_waiting.take() {
                    self.write_save(path, normalize);
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
            }
        }
        if let Some(result) = self.dialog.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.dialog = None;
            match result {
                FileDialog::SaveAs(Some(chosen)) => {
                    self.start_save_as(chosen);
                }
                FileDialog::Open(Some(path)) => self.proceed(ctx, Next::Open(Some(path))),
                FileDialog::Import { paths, at, into } => {
                    if !paths.is_empty() {
                        self.add_files(paths, at, into, ctx);
                    }
                }
                FileDialog::ExportMidi { path, bytes } => {
                    if let Some(path) = path {
                        self.write_midi(ctx, path, bytes);
                    }
                }
                FileDialog::PluginFolder(Some(folder)) => self.add_plugin_folder(ctx, &folder),
                // Cancelled: whatever was waiting on it is off too.
                FileDialog::SaveAs(None) | FileDialog::Open(None) => self.after_save = None,
                FileDialog::PluginFolder(None) => {}
            }
        }
        if let Some(result) = self.saving.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.saving = None;
            match result {
                Ok(saved) => {
                    self.document.mark_saved_at(saved.state);
                    self.document.path = Some(saved.path.clone());
                    self.document.media_files.extend(saved.moved);
                    self.synced = None;
                    files::remember(&mut self.settings.projects.recent, &saved.path);
                    self.settings_changed = Some(Instant::now());
                    let copied = match saved.copied {
                        0 => String::new(),
                        1 => " (1 audio file copied in)".into(),
                        n => format!(" ({n} audio files copied in)"),
                    };
                    // A plugin that changed and could not be read is saved
                    // with the state it was loaded with and the parameter
                    // values changed since: say so.
                    let unread = match self.hosting.unread() {
                        0 => String::new(),
                        1 => ". A plugin could not be read: only its parameter values are current"
                            .into(),
                        n => format!(
                            ". {n} plugins could not be read: only their parameter values are current"
                        ),
                    };
                    self.toast = Some((
                        format!("Saved {}{copied}{unread}", self.document.name()),
                        Instant::now(),
                    ));
                    self.file_notice = None;
                    if let Some(next) = self.after_save.take() {
                        self.proceed(ctx, next);
                    }
                }
                Err(e) => {
                    self.after_save = None;
                    self.file_notice = Some(format!("Not saved. {e}"));
                }
            }
        }
        if let Some(result) = self.opening.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.opening = None;
            match result {
                Ok(opened) => {
                    let view = opened.project.view.clone();
                    files::remember(&mut self.settings.projects.recent, &opened.path);
                    self.settings_changed = Some(Instant::now());
                    self.file_notice = match opened.missing.len() {
                        0 => None,
                        1 => Some(format!("1 audio file is missing: {}", opened.missing[0])),
                        n => Some(format!(
                            "{n} audio files are missing, first {}",
                            opened.missing[0]
                        )),
                    };
                    let recovered = self.recovering.is_some();
                    match self.recovering.take() {
                        Some(leftover) => {
                            if let Some(autosave) = &mut self.autosave {
                                autosave.adopt(&leftover);
                            }
                            self.replace_document(document::Document::recovered(
                                opened.project,
                                leftover.note.project.map(PathBuf::from),
                                opened.media_files,
                            ));
                        }
                        None => self.replace_document(document::Document::opened(
                            opened.project,
                            opened.path,
                            opened.media_files,
                        )),
                    }
                    self.arrangement = arrangement::Arrangement::restore(&view);
                    self.chain.folded = view.folded_devices.iter().copied().collect();
                    self.chain.shown = view.shown_params.clone();
                    self.nav.set(crate::timeview::TimeView {
                        left: f64::from(view.left_beat).max(0.0),
                        zoom: f64::from(view.px_per_beat).clamp(crate::timeview::MIN_ZOOM, 1e7),
                    });
                    self.grid = view.grid;
                    self.show_browser = view.browser_open;
                    self.show_detail = view.detail_open;
                    self.browser_size = view.browser_size.clamp(crate::BROWSER_MIN, 600.0);
                    self.detail_size = view.detail_size.clamp(80.0, 600.0);
                    let what = if recovered {
                        "Recovered unsaved changes to"
                    } else {
                        "Opened"
                    };
                    self.toast = Some((format!("{what} {}", self.document.name()), Instant::now()));
                }
                Err(e) => {
                    self.recovering = None;
                    self.file_notice = Some(format!("Could not open the project. {e}"));
                }
            }
        }
        // The window's close button: offer to save first.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quitting
            && self.document.is_dirty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.prompt = Some(Next::Quit);
        }
    }

    /// "Save changes to <name>?" with Save, Don't save and Cancel. Not a
    /// blocking dialog: the rest of the app keeps running behind it.
    pub(crate) fn prompt_window(&mut self, ctx: &egui::Context) {
        let Some(next) = self.prompt.clone() else {
            return;
        };
        let mut choice = None;
        let action = match next {
            Next::New => "starting a new project",
            Next::Open(_) => "opening another project",
            Next::Quit => "closing",
        };
        let title = format!("Save changes to {}?", self.document.name());
        Dialog::new("unsaved changes", &title, 380.0)
            .at(
                pos2(ctx.content_rect().center().x, 120.0),
                egui::Align2::CENTER_TOP,
            )
            .question()
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "There are unsaved changes. Save them before {action}?"
                    ))
                    .color(theme::p().text_secondary),
                );
                ui.add_space(theme::space::M);
                ui.horizontal(|ui| {
                    if raised::button(ui, "Save").clicked() {
                        choice = Some(0);
                    }
                    if raised::button(ui, "Don't save").clicked() {
                        choice = Some(1);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if raised::button(ui, "Cancel").clicked() {
                            choice = Some(2);
                        }
                    });
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(2);
        }
        match choice {
            Some(0) => {
                self.prompt = None;
                self.after_save = Some(next);
                self.save(ctx, false);
            }
            Some(1) => {
                self.prompt = None;
                self.proceed(ctx, next);
            }
            Some(_) => self.prompt = None,
            None => {}
        }
    }

    /// A problem saving or opening, with a way forward.
    pub(crate) fn file_banner(&mut self, ui: &mut egui::Ui) {
        let Some(message) = self.file_notice.clone() else {
            return;
        };
        let ctx = ui.ctx().clone();
        egui::Panel::top("file banner")
            .frame(super::view::notice_panel_frame())
            .show(ui, |ui| {
                super::view::notice_card(ui, |ui| {
                    ui.label(&message);
                    if message.starts_with("Not saved")
                        && raised::primary_button(ui, "Save as\u{2026}").clicked()
                    {
                        self.save(&ctx, true);
                    }
                    if raised::button(ui, "Dismiss").clicked() {
                        self.file_notice = None;
                    }
                });
            });
    }
}

pub(crate) fn spawn_dialog(
    ctx: &egui::Context,
    show: impl FnOnce() -> FileDialog + Send + 'static,
) -> std::sync::mpsc::Receiver<FileDialog> {
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("lumen-file-dialog".into())
        .spawn(move || {
            let _ = tx.send(show());
            ctx.request_repaint();
        })
        .expect("spawn file dialog");
    rx
}
