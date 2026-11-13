//! The results of dialogs, saves and opens, and the banner that says a
//! file could not be saved or opened.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;

use super::dialogs::FileDialog;
use super::open::Next;
use super::save::PLUGIN_READ_WAIT;
use crate::App;
use crate::app::copies::{Purpose, Waiting};
use crate::app::handover::{interrupted, when_done};
use crate::app::view::{notice_card, notice_panel_frame};
use crate::widgets::raised;
use crate::{arrangement, document, files};

impl App {
    /// Results of dialogs, saves and opens; and the window's close button.
    pub(crate) fn poll_files(&mut self, ctx: &egui::Context) {
        if let Some(waiting) = &self.save_waiting {
            if !self.hosting.captures_pending() || waiting.since.elapsed() >= PLUGIN_READ_WAIT {
                if let Some(Waiting {
                    purpose,
                    path,
                    normalize,
                    ..
                }) = self.save_waiting.take()
                {
                    self.write(purpose, path, normalize);
                }
            } else {
                ctx.request_repaint_after(crate::motion::WORK_FRAME);
            }
        }
        if let Some(result) = self.dialog.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.dialog = None;
            self.save_dialog = false;
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
                FileDialog::ExportAudio(path) => self.export_destination_chosen(path, ctx),
                FileDialog::SaveCopy(Some(chosen)) => {
                    self.start_save_for(Purpose::Copy, chosen, true)
                }
                FileDialog::Collect(Some(chosen)) => {
                    self.start_save_for(Purpose::Collect, chosen, true)
                }
                FileDialog::Locate(path) => self.located(ctx, path),
                FileDialog::RelinkFolder(Some(folder)) => self.searching_folder(ctx, folder),
                FileDialog::CacheFolder(Some(folder)) => self.cache_folder_chosen(Some(folder)),
                FileDialog::ProjectsFolder(Some(folder)) => {
                    self.projects_folder_chosen(Some(folder))
                }
                FileDialog::SampleFolder(Some(folder)) => self.add_sample_folder(&folder),
                FileDialog::Place(Some(folder)) => self.place_chosen(folder),
                FileDialog::Place(None) => self.browser.files.finding = None,
                FileDialog::DrumMapExport(path) => self.drum_map_export_chosen(path),
                FileDialog::DrumMapImport(path) => self.drum_map_import_chosen(path),
                FileDialog::SamplerFiles { paths, device } => {
                    if !paths.is_empty() {
                        self.load_sample_files(device, paths);
                    }
                }
                // Cancelled: whatever was waiting on it is off too.
                FileDialog::SaveAs(None) | FileDialog::Open(None) => self.after_save = None,
                FileDialog::PluginFolder(None)
                | FileDialog::SaveCopy(None)
                | FileDialog::Collect(None)
                | FileDialog::RelinkFolder(None)
                | FileDialog::CacheFolder(None)
                | FileDialog::ProjectsFolder(None)
                | FileDialog::SampleFolder(None) => {}
            }
        }
        if let Some(result) = self.saving.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.saving = None;
            crate::recording::take_notes::delete_later(
                self.record.audio.notes.save_done(result.is_ok()),
            );
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
                    // The save committed, but the previous version could not
                    // be kept: say what is safe (sweep M1).
                    if let Some(why) = &saved.backup_error {
                        self.file_notice =
                            Some(format!("Saved, but {why}. Your project itself was saved."));
                    }
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
            let template = std::mem::take(&mut self.templates.opening);
            let reverting = self.revert.reloading.take();
            // A recording or an export that began, or an edit made, while
            // the file was read: nothing is replaced (M9 R8, ADR-0117).
            let busy = self.busy_for_open();
            let changed = self.document.change_mark() != self.opening_mark;
            match result {
                Ok(_) if (busy.is_some() || changed) && reverting.is_some() => {
                    let kept = reverting.unwrap_or_default();
                    self.file_notice = Some(format!(
                        "{}, so it was not reverted. The earlier changes are in {}. Choose \
                         Revert to saved again{}.",
                        interrupted(busy, "the saved file"),
                        kept.display(),
                        when_done(busy),
                    ));
                }
                Ok(_) if (busy.is_some() || changed) && self.recovering.is_some() => {
                    self.leftover = self.recovering.take();
                    self.file_notice = Some(format!(
                        "{}, so nothing was recovered. Choose Recover again{}.",
                        interrupted(busy, "the autosave"),
                        when_done(busy),
                    ));
                }
                // An open or a template waits again while the work goes on,
                // and asks again if the project changed.
                Ok(opened) if busy.is_some() || changed => {
                    let next = if template {
                        Next::Template(opened.path)
                    } else {
                        Next::Open(Some(opened.path))
                    };
                    if busy.is_some() {
                        let mark = self.opening_mark;
                        self.hold(&next, true, mark);
                    } else {
                        self.request(ctx, next);
                    }
                }
                Ok(opened) => {
                    let view = opened.project.view.clone();
                    let file = opened.path.clone();
                    // A template is a way to start, not a project to come
                    // back to: it is not remembered.
                    if !template {
                        files::remember(&mut self.settings.projects.recent, &opened.path);
                        self.settings_changed = Some(Instant::now());
                    }
                    // Missing audio is answered by its own banner, which
                    // opens the Project media tab (M9 slice R4b).
                    self.file_notice = None;
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
                            self.recover_takes(&leftover.session);
                        }
                        None if template => self.replace_document(
                            document::Document::from_template(opened.project, opened.media_files),
                        ),
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
                    if template {
                        self.said_new_from(&file);
                    } else if let Some(kept) = reverting {
                        // The message that offers the dropped changes.
                        self.revert.kept = Some(kept);
                        self.say(format!("Reverted to the saved {}", self.document.name()));
                    } else {
                        let what = if recovered {
                            "Recovered unsaved changes to"
                        } else {
                            "Opened"
                        };
                        let mut words = format!("{what} {}", self.document.name());
                        if let Some((_, left)) =
                            self.left_out.take().filter(|(dropped, _)| *dropped == file)
                        {
                            words =
                                format!("{words}. {}", crate::app::handover::left_out_words(left));
                        }
                        self.toast = Some((words, Instant::now()));
                    }
                }
                Err(e) => {
                    self.recovering = None;
                    self.file_notice = Some(match reverting {
                        Some(kept) => format!(
                            "Could not read the saved project back, so it was not reverted. \
                             Your changes are still open, and also kept in {}. {e}",
                            kept.display()
                        ),
                        None => format!(
                            "Could not open the {}. {e}",
                            if template { "template" } else { "project" }
                        ),
                    });
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

    /// A problem saving or opening, with a way forward.
    pub(crate) fn file_banner(&mut self, ui: &mut egui::Ui) {
        let Some(message) = self.file_notice.clone() else {
            return;
        };
        let ctx = ui.ctx().clone();
        egui::Panel::top("file banner")
            .frame(notice_panel_frame())
            .show(ui, |ui| {
                notice_card(ui, |ui| {
                    // A notice is read in full: it wraps rather than being cut
                    // short or widening the window.
                    ui.add(egui::Label::new(&message).wrap());
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
