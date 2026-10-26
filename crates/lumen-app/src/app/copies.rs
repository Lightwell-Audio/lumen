//! Save a copy and Collect and save (M9 slice R4, ADR-0105). Both copy every
//! audio file the project uses into a project folder, whether or not the
//! copy-imported preference is on, on a worker (`files::collect`) with progress
//! in the toast and Esc to stop. Save a copy writes a new project and leaves
//! the open one exactly as it is (its file, its unsaved mark, the recent list,
//! the autosave); Collect and save is a Save that also gathers the outside
//! files. The project file is written after the last copy, so a stop or a full
//! disk changes nothing in the project, and each file is written under a
//! temporary name and renamed when whole.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use crate::app::projects::{self, FileDialog};
use crate::{App, files, trace};

/// What a save is for. Saving waits for the plugins' states to be read
/// whichever it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Save,
    Copy,
    Collect,
    /// A copy kept in the templates folder, to start new projects from.
    Template,
    /// The unsaved version put away before Revert reads the saved file back.
    Revert,
}

/// A save waiting for the running plugins' states to be read.
pub(crate) struct Waiting {
    pub(crate) purpose: Purpose,
    pub(crate) path: PathBuf,
    /// Resolve the path as Save As does, into a project folder.
    pub(crate) normalize: bool,
    pub(crate) since: Instant,
}

/// A copy or a collect under way.
pub(crate) struct Running {
    purpose: Purpose,
    /// The document's change mark when the copy began: Revert only goes on
    /// when it is still the same.
    mark: u64,
    control: files::Control,
    result: Receiver<Result<files::Saved, files::Failure>>,
}

impl App {
    /// A copy or collect is possible now, or why not.
    pub(crate) fn copy_unavailable(&self) -> Result<(), &'static str> {
        if self.collecting.is_some() {
            Err("A copy is under way")
        } else if self.saving.is_some() || self.save_waiting.is_some() {
            Err("A save is under way")
        } else if self.dialog.is_some() {
            Err("A file dialog is open")
        } else {
            Ok(())
        }
    }

    /// Save a copy: asks where, then copies the project and its audio there.
    pub(crate) fn save_copy(&mut self, ctx: &egui::Context) {
        if self.copy_unavailable().is_err() {
            return;
        }
        let name = format!("{} copy.{}", self.document.name(), files::EXTENSION);
        let start = self.dialog_start();
        let owner = self.owner;
        let show = move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Save a copy")
                .add_filter(format!("{PRODUCT_NAME} project"), &[files::EXTENSION])
                .set_file_name(name);
            if let Some(dir) = start {
                dialog = dialog.set_directory(projects::made(dir));
            }
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            FileDialog::SaveCopy(dialog.save_file())
        };
        self.file_dialog(ctx, show, |paths| {
            FileDialog::SaveCopy(paths.into_iter().next())
        });
    }

    /// Collect and save: into this project's folder, or, for a project that
    /// has none yet, asks where first.
    pub(crate) fn collect_and_save(&mut self, ctx: &egui::Context) {
        if self.copy_unavailable().is_err() {
            return;
        }
        match self.document.path.clone() {
            Some(path) => self.start_save_for(Purpose::Collect, path, false),
            None => {
                let name = format!("{}.{}", self.document.name(), files::EXTENSION);
                let start = self.dialog_start();
                let owner = self.owner;
                let show = move || {
                    let mut dialog = rfd::FileDialog::new()
                        .set_title("Collect and save")
                        .add_filter(format!("{PRODUCT_NAME} project"), &[files::EXTENSION])
                        .set_file_name(name);
                    if let Some(dir) = start {
                        dialog = dialog.set_directory(projects::made(dir));
                    }
                    if let Some(owner) = &owner {
                        dialog = dialog.set_parent(owner);
                    }
                    FileDialog::Collect(dialog.save_file())
                };
                self.file_dialog(ctx, show, |paths| {
                    FileDialog::Collect(paths.into_iter().next())
                });
            }
        }
    }

    /// Starts the worker for a copy or a collect, with the plugins' states as
    /// they were last read.
    pub(crate) fn write_copy(&mut self, purpose: Purpose, path: PathBuf, normalize: bool) {
        trace::log(|| format!("files: {purpose:?} to {}", path.display()));
        // A copy, a template and what Revert puts away are new projects: the
        // open one is not saved.
        let copying = purpose != Purpose::Collect;
        if !copying {
            self.document.begin_save();
        }
        let mark = self.document.change_mark();
        let control = files::Control::default();
        let result = files::collect(
            files::SaveRequest {
                project: self.stamped(),
                path,
                normalize_project_path: normalize,
                media_files: self.document.media_files.clone(),
                // What Revert puts away points at the audio where it is (it
                // is only a way back); everything else holds its own.
                copy_imported: purpose != Purpose::Revert,
                renders: Some(self.renders_dir()),
                recordings: Some(self.recordings_dir()),
                templates: Some(self.templates_dir()),
                keep_backups: lumen_project::SaveOptions::default().keep_backups,
                state: self.document.state(),
                // A copy is a new project: not the one that is open.
                not_over: if copying {
                    self.document.path.clone()
                } else {
                    None
                },
            },
            control.clone(),
        );
        self.collecting = Some(Running {
            purpose,
            mark,
            control,
            result,
        });
        self.say(format!("{} 0% \u{b7} Esc to stop", verb(purpose)));
    }

    /// Progress, Esc, and the result of the copy or collect under way. Call
    /// every frame.
    pub(crate) fn poll_collecting(&mut self, ctx: &egui::Context) {
        let Some(running) = &self.collecting else {
            return;
        };
        let (purpose, mark) = (running.purpose, running.mark);
        if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            // The worker stops at its next piece and answers; its answer is
            // taken here, like any other.
            running.control.stop();
        }
        let progress = running.control.progress();
        match running.result.try_recv() {
            Err(TryRecvError::Empty) => {
                let shown = (progress * 100.0).floor() as u32;
                self.say(format!("{} {shown}% \u{b7} Esc to stop", verb(purpose)));
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(TryRecvError::Disconnected) => {
                self.collecting = None;
                self.file_notice = Some(format!(
                    "{} did not finish. Please try again.",
                    what(purpose)
                ));
            }
            Ok(Err(files::Failure::Stopped)) => {
                self.collecting = None;
                self.say(format!(
                    "{} stopped. Nothing in the project changed.",
                    what(purpose)
                ));
            }
            Ok(Err(files::Failure::Failed(why))) => {
                self.collecting = None;
                self.file_notice = Some(format!("{} failed. {why}", what(purpose)));
            }
            Ok(Ok(saved)) => {
                self.collecting = None;
                self.copy_done(ctx, purpose, mark, saved);
            }
        }
    }

    fn copy_done(&mut self, ctx: &egui::Context, purpose: Purpose, mark: u64, saved: files::Saved) {
        if purpose == Purpose::Revert {
            // Reading the saved file back reports what is missing.
            self.reload_saved(saved.path, mark);
            return;
        }
        let files::Saved {
            path,
            moved,
            state,
            copied,
            bytes,
            skipped,
        } = saved;
        let audio = match copied {
            0 => String::new(),
            1 => format!("1 audio file, {}", files::describe_size(bytes)),
            n => format!("{n} audio files, {}", files::describe_size(bytes)),
        };
        let called = path
            .file_stem()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match purpose {
            Purpose::Collect => {
                self.document.mark_saved_at(state);
                self.document.path = Some(path.clone());
                self.document.media_files.extend(moved);
                self.synced = None;
                files::remember(&mut self.settings.projects.recent, &path);
                self.settings_changed = Some(Instant::now());
                self.file_notice = None;
                let name = self.document.name();
                self.say(if audio.is_empty() {
                    format!("Everything is already in {name}'s folder. Saved")
                } else {
                    format!("Collected {audio} into {name} and saved")
                });
            }
            Purpose::Template => {
                self.templates.listing.ask(ctx, self.templates_dir(), true);
                self.say(if audio.is_empty() {
                    format!("Saved the template {called}")
                } else {
                    format!("Saved the template {called} ({audio})")
                });
            }
            // The open project is untouched: not its file, its unsaved mark,
            // its media, the recent list, its notices or the autosave.
            _ => self.say(if audio.is_empty() {
                format!("Saved a copy as {called}")
            } else {
                format!("Saved a copy as {called} ({audio})")
            }),
        }
        match skipped.as_slice() {
            [] => {}
            [only] => {
                self.file_notice = Some(format!(
                    "1 audio file could not be found and was left as it was: {only}"
                ));
            }
            [first, ..] => {
                self.file_notice = Some(format!(
                    "{} audio files could not be found and were left as they were, first {first}",
                    skipped.len()
                ));
            }
        }
    }
}

fn verb(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Collect => "Collecting",
        Purpose::Template => "Saving the template",
        Purpose::Revert => "Putting your changes away",
        Purpose::Save | Purpose::Copy => "Copying",
    }
}

fn what(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Collect => "Collect and save",
        Purpose::Template => "Save as template",
        Purpose::Revert => "Revert to saved",
        Purpose::Save | Purpose::Copy => "Save a copy",
    }
}
