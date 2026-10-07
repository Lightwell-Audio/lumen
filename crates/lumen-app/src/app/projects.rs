//! New, open, save and save as; the unsaved-changes prompt and the file
//! banner (M3 slice 3c). Saving and opening run in the background
//! (`crate::files`); file dialogs run on their own thread, owned by the main
//! window (`crate::owner`).

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, RichText, pos2};
use lumen_core::product::PRODUCT_NAME;
use lumen_core::time::Ticks;
use lumen_project::model::Project;

use crate::{App, arrangement, document, empty_project, files, fonts, lanes, theme, trace};

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
}

/// Where file dialogs start when the project has no folder yet.
pub(crate) fn documents_folder() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join("Documents"))
}

impl App {
    /// The project as it should be saved: the document with the loop region
    /// and view stamped in (they are not undo steps).
    pub(crate) fn stamped(&self) -> Project {
        let mut project = self.document.project().clone();
        let looped = self.transport.loop_region;
        project.transport = lumen_project::model::TransportState {
            loop_start: looped.start,
            loop_end: looped.end,
            loop_enabled: looped.enabled,
        };
        project.view = lumen_project::model::ViewState {
            left_beat: self.left_beat,
            px_per_beat: self.px_per_beat,
            browser_open: self.show_browser,
            detail_open: self.show_detail,
            browser_size: self.browser_size,
            detail_size: self.detail_size,
            ..project.view.clone()
        };
        self.arrangement.save_into(&mut project.view);
        project
    }

    /// Save (Ctrl+S): to the project's file, or ask where (Save As).
    pub(crate) fn save(&mut self, ctx: &egui::Context, ask: bool) {
        if self.saving.is_some() || self.dialog.is_some() {
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
                self.dialog = Some(spawn_dialog(ctx, move || {
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
                }));
            }
        }
    }

    pub(crate) fn start_save(&mut self, path: PathBuf) {
        trace::log(|| format!("files: saving {}", path.display()));
        self.saving = Some(files::save(files::SaveRequest {
            project: self.stamped(),
            path,
            media_files: self.document.media_files.clone(),
            copy_imported: self.settings.projects.copy_imported,
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
                self.dialog = Some(spawn_dialog(ctx, move || {
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
                }));
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
        self.synced = None;
        self.show_history = false;
        self.transport.locate(Ticks::ZERO);
    }

    /// Results of dialogs, saves and opens; and the window's close button.
    pub(crate) fn poll_files(&mut self, ctx: &egui::Context) {
        if let Some(result) = self.dialog.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.dialog = None;
            match result {
                FileDialog::SaveAs(Some(chosen)) => {
                    self.start_save(files::project_file_for(&chosen));
                }
                FileDialog::Open(Some(path)) => self.proceed(ctx, Next::Open(Some(path))),
                // Cancelled: whatever was waiting on it is off too.
                FileDialog::SaveAs(None) | FileDialog::Open(None) => self.after_save = None,
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
                    self.toast = Some((
                        format!("Saved {}{copied}", self.document.name()),
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
                    let looped = opened.project.transport;
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
                    self.left_beat = view.left_beat.max(0.0);
                    self.px_per_beat = view.px_per_beat.clamp(2.0, 400.0);
                    self.show_browser = view.browser_open;
                    self.show_detail = view.detail_open;
                    self.browser_size = view.browser_size.clamp(120.0, 600.0);
                    self.detail_size = view.detail_size.clamp(80.0, 600.0);
                    self.transport.set_loop(lumen_engine::Region::new(
                        looped.loop_start,
                        looped.loop_end.max(looped.loop_start + Ticks(1)),
                        looped.loop_enabled,
                    ));
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

    /// File shortcuts: Ctrl+N, Ctrl+O, Ctrl+S and Ctrl+Shift+S.
    pub(crate) fn file_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let pressed = |modifiers, key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            self.save(ctx, true);
        } else if pressed(Modifiers::COMMAND, Key::S) {
            self.save(ctx, false);
        } else if pressed(Modifiers::COMMAND, Key::O) {
            self.request(ctx, Next::Open(None));
        } else if pressed(Modifiers::COMMAND, Key::N) {
            self.request(ctx, Next::New);
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
        egui::Window::new("Unsaved changes")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_TOP)
            .default_pos(pos2(ctx.content_rect().center().x, 120.0))
            .min_width(380.0)
            .max_width(380.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!("Save changes to {}?", self.document.name()))
                        .font(fonts::display(theme::size::TITLE)),
                );
                ui.add_space(theme::space::XS);
                ui.label(
                    RichText::new(format!(
                        "There are unsaved changes. Save them before {action}?"
                    ))
                    .color(theme::p().text_secondary),
                );
                ui.add_space(theme::space::M);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Don't save").clicked() {
                        choice = Some(1);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
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
        egui::Panel::top("file banner").show(ui, |ui| {
            ui.horizontal(|ui| {
                let warning = theme::p().warning;
                ui.label(RichText::new("\u{26A0}").color(warning));
                ui.label(RichText::new(&message).color(warning));
                if message.starts_with("Not saved") && ui.button("Save as\u{2026}").clicked() {
                    self.save(&ctx, true);
                }
                if ui.small_button("Dismiss").clicked() {
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
