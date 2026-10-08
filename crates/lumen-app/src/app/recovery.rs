//! Autosave and the recovery banner after a crash (M3 slice 3d); the
//! autosave itself is `crate::autosave`.

use std::time::Instant;

use eframe::egui::{self, RichText};
use lumen_core::product::PRODUCT_NAME;

use crate::{App, autosave, files, theme, trace};

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
            self.document.state(),
            now,
            |file| files::SaveRequest {
                project: self.stamped(),
                path: file,
                media_files: self.document.media_files.clone(),
                copy_imported: false,
                keep_backups: 0,
                state: self.document.state(),
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
        egui::Panel::top("recovery banner").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("\u{26A0}").color(theme::p().warning));
                ui.label(format!(
                    "{PRODUCT_NAME} did not close normally. Unsaved changes to {} from {} can be recovered.",
                    leftover.note.name,
                    autosave::ago(leftover.note.written)
                ));
                if ui.button("Recover").clicked() {
                    choice = Some(true);
                }
                if ui
                    .button("Discard")
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

    /// The newest autosave left by another session, if any.
    pub(crate) fn next_leftover(&self) -> Option<autosave::Leftover> {
        let autosave = self.autosave.as_ref()?;
        autosave::leftovers(autosave.dir()?, Some(autosave.session()))
            .into_iter()
            .next()
    }
}
