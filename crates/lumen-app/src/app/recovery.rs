//! Autosave and the recovery banner after a crash (M3 slice 3d); the
//! autosave itself is `crate::autosave`.

use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use crate::widgets::raised;
use crate::{App, autosave, files, trace};

impl App {
    /// Writes an autosave a minute after unsaved changes, and removes it
    /// once they are saved.
    pub(crate) fn autosave(&mut self, ctx: &egui::Context) {
        let Some(mut autosave) = self.autosave.take() else {
            return;
        };
        let now = Instant::now();
        autosave.tick(
            self.document.is_dirty(),
            self.document.change_mark(),
            now,
            |file| files::SaveRequest {
                project: self.stamped(),
                path: file,
                normalize_project_path: false,
                media_files: self.document.media_files.clone(),
                copy_imported: false,
                renders: Some(self.renders_dir()),
                keep_backups: 0,
                state: self.document.change_mark(),
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
                if raised::primary_button(ui, "Recover").clicked() {
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
                self.opening = Some(files::open(leftover.file.clone()));
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
