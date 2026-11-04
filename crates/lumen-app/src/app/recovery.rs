//! Autosave and the recovery banner after a crash (M3 slice 3d); the
//! autosave itself is `crate::autosave`.

use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use crate::recording::take_notes::delete_later;
use crate::widgets::raised;
use crate::{App, autosave, files, trace};

impl App {
    /// Writes an autosave a minute after unsaved changes, and removes it
    /// once they are saved. While a take is recorded, or a placed take is in
    /// no finished save yet, it autosaves a project with no unsaved changes
    /// too, so a crash always leaves recovery something to start from (M9
    /// review H3, H4).
    pub(crate) fn autosave(&mut self, ctx: &egui::Context) {
        let Some(mut autosave) = self.autosave.take() else {
            return;
        };
        let notes = &mut self.record.audio.notes;
        delete_later(notes.keep_only(self.document.session()));
        let keep = notes.any() || self.is_recording() || self.audio_takes_finishing();
        let now = Instant::now();
        let mut started = false;
        autosave.tick(
            self.document.is_dirty() || keep,
            self.document.change_mark(),
            now,
            |file| {
                started = true;
                files::SaveRequest {
                    project: self.stamped(),
                    path: file,
                    normalize_project_path: false,
                    media_files: self.document.media_files.clone(),
                    copy_imported: false,
                    renders: Some(self.renders_dir()),
                    recordings: Some(self.recordings_dir()),
                    // An autosave refers to a template's audio where it is, as it
                    // does to imported audio.
                    templates: None,
                    keep_backups: 0,
                    state: self.document.change_mark(),
                    not_over: None,
                }
            },
            || autosave::Note {
                project: self
                    .document
                    .path
                    .as_deref()
                    .map(|p| p.to_string_lossy().into_owned()),
                name: self.document.name(),
                written: autosave::now_seconds(),
            },
        );
        // The write that ended first, then the one that began: in this order,
        // a take placed after the earlier write began is not let go with it.
        if let Some(written) = autosave.take_finished() {
            delete_later(self.record.audio.notes.autosave_done(written));
        }
        if started {
            self.record.audio.notes.autosave_started();
        }
        if let Some(due) = autosave.due_in(now) {
            ctx.request_repaint_after(due);
        }
        self.autosave = Some(autosave);
    }

    /// After a crash: offer the autosave that was left behind.
    pub(crate) fn recovery_banner(&mut self, ui: &mut egui::Ui) {
        let Some(leftover) = self.leftover.clone() else {
            return;
        };
        let mut choice = None;
        egui::Panel::top("recovery banner")
            .frame(super::view::notice_panel_frame())
            .show(ui, |ui| {
            super::view::notice_card(ui, |ui| {
                ui.label(format!(
                    "{PRODUCT_NAME} did not close normally. Unsaved changes to {} from {} can be recovered.",
                    leftover.note.name,
                    autosave::ago(leftover.note.written)
                ));
                // Recovering replaces the open project: never over unsaved
                // changes, a recording or an export (M9 R8, ADR-0117).
                let wait = match self.busy_for_open() {
                    Some(busy) => Some(format!(
                        "Recovering replaces the open project, so it waits{}.",
                        super::handover::when_done(Some(busy))
                    )),
                    None if self.document.is_dirty() => Some(format!(
                        "Recovering replaces the open project: save your changes to {} first.",
                        self.document.name()
                    )),
                    None => None,
                };
                if let Some(wait) = &wait {
                    ui.label(wait.as_str());
                }
                if ui
                    .add_enabled_ui(wait.is_none(), |ui| raised::primary_button(ui, "Recover"))
                    .inner
                    .clicked()
                {
                    choice = Some(true);
                }
                if raised::button(ui, "Discard")
                    .on_hover_text("Delete the autosaved copy")
                    .clicked()
                {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) if self.opening.is_none() => {
                trace::log(|| format!("autosave: recovering {}", leftover.file.display()));
                self.begin_opening(leftover.file.clone());
                self.recovering = Some(leftover);
                self.leftover = None;
            }
            Some(false) => {
                autosave::discard(&leftover);
                self.leftover = self.next_leftover();
            }
            _ => {}
        }
    }

    /// A crash report from an earlier run: offered once, to show or
    /// dismiss (owner, M3 slice 6c).
    pub(crate) fn crash_banner(&mut self, ui: &mut egui::Ui) {
        let Some(report) = self.crash_report.clone() else {
            return;
        };
        let mut done = false;
        egui::Panel::top("crash banner")
            .frame(super::view::notice_panel_frame())
            .show(ui, |ui| {
                super::view::notice_card(ui, |ui| {
                    ui.label(format!(
                        "{PRODUCT_NAME} closed unexpectedly last time. A crash report was saved."
                    ));
                    if raised::primary_button(ui, "Show report")
                        .on_hover_text(report.display().to_string())
                        .clicked()
                    {
                        crate::crash::reveal(&report);
                        done = true;
                    }
                    if raised::button(ui, "Dismiss").clicked() {
                        done = true;
                    }
                });
            });
        if done {
            crate::crash::mark_seen(&report);
            self.crash_report = None;
        }
    }

    /// The newest autosave left by another session, if any.
    pub(crate) fn next_leftover(&self) -> Option<autosave::Leftover> {
        let autosave = self.autosave.as_ref()?;
        autosave::leftovers(autosave.dir()?, Some(autosave.session()))
            .into_iter()
            .next()
    }
}
