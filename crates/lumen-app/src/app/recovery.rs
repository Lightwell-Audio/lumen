use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use crate::recording::take_notes::delete_later;
use crate::widgets::raised;
use crate::{App, autosave, files, trace};

#[derive(Default)]
pub(crate) struct Leftovers {
    pending: Option<Receiver<Vec<autosave::Leftover>>>,
    ready: Option<Vec<autosave::Leftover>>,
    seen: bool,
    recover_latest: bool,
    failure: Option<String>,
}

impl Leftovers {
    pub(crate) fn new(recover_latest: bool) -> Self {
        Self {
            recover_latest,
            ..Self::default()
        }
    }

    pub(crate) fn start(&mut self, ctx: &egui::Context, dir: &Path, session: &str) {
        if self.seen {
            return;
        }
        self.seen = true;
        let (tx, rx) = mpsc::channel();
        let (dir, session, ctx) = (dir.to_path_buf(), session.to_owned(), ctx.clone());
        let spawned = crate::spawn::spawn_named("lumen-recovery-scan", move || {
            let _ = tx.send(autosave::leftovers(&dir, Some(&session)));
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.pending = Some(rx),
            Err(error) => {
                self.failure = Some(crate::spawn::start_failed("the recovery scan", &error))
            }
        }
    }

    pub(crate) fn discard(
        &mut self,
        ctx: &egui::Context,
        dir: &Path,
        session: &str,
        leftover: &autosave::Leftover,
    ) {
        let (tx, rx) = mpsc::channel();
        let (own_dir, own_session, gone, ctx) = (
            dir.to_path_buf(),
            session.to_owned(),
            leftover.clone(),
            ctx.clone(),
        );
        let spawned = crate::spawn::spawn_named("lumen-recovery-discard", move || {
            autosave::discard(&gone);
            let _ = tx.send(autosave::leftovers(&own_dir, Some(&own_session)));
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.pending = Some(rx),
            Err(error) => {
                trace::log(|| {
                    format!(
                        "recovery: {}",
                        crate::spawn::start_failed("the recovery clean-up", &error)
                    )
                });
                autosave::discard(leftover);
                self.ready = Some(autosave::leftovers(dir, Some(session)));
            }
        }
    }

    pub(crate) fn poll(&mut self) -> Option<Vec<autosave::Leftover>> {
        if let Some(found) = self.ready.take() {
            self.pending = None;
            return Some(found);
        }
        let found = self.pending.as_ref()?.try_recv().ok()?;
        self.pending = None;
        Some(found)
    }

    #[cfg(test)]
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn unseen(&self) -> bool {
        !self.seen
    }

    pub(crate) fn take_recover_latest(&mut self) -> bool {
        std::mem::take(&mut self.recover_latest)
    }

    pub(crate) fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}

impl App {
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
                    templates: None,
                    keep_backups: 0,
                    state: self.document.change_mark(),
                    not_over: None,
                }
            },
            || autosave::Note {
                version: autosave::VERSION,
                project: self
                    .document
                    .path
                    .as_deref()
                    .map(|p| p.to_string_lossy().into_owned()),
                name: self.document.name(),
                written: autosave::now_seconds(),
            },
        );
        if let Some(written) = autosave.take_finished() {
            delete_later(self.record.audio.notes.autosave_done(written));
        }
        if started {
            self.record.audio.notes.autosave_started();
        }
        if let Some(why) = autosave.take_failure() {
            self.file_notice = Some(format!(
                "Autosave could not start: {why}. Save your work yourself until this is fixed, \
                 and restart if it keeps happening."
            ));
        }
        if let Some(due) = autosave.due_in(now) {
            ctx.request_repaint_after(due);
        }
        self.autosave = Some(autosave);
    }

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
                if let Some((dir, session)) = self.autosave_place() {
                    self.leftovers.discard(ui.ctx(), &dir, &session, &leftover);
                } else {
                    autosave::discard(&leftover);
                }
                self.leftover = None;
            }
            _ => {}
        }
    }

    pub(crate) fn poll_leftovers(&mut self, ctx: &egui::Context) {
        if self.leftovers.unseen()
            && let Some((dir, session)) = self.autosave_place()
        {
            self.leftovers.start(ctx, &dir, &session);
        }
        if let Some(found) = self.leftovers.poll() {
            if self.leftovers.take_recover_latest()
                && let Some(first) = found.first()
            {
                trace::log(|| format!("autosave: recovering {}", first.file.display()));
                self.begin_opening(first.file.clone());
                self.recovering = Some(first.clone());
                return;
            }
            self.leftover = found.into_iter().next();
        }
        if let Some(why) = self.leftovers.take_failure() {
            self.say(why);
        }
    }

    fn autosave_place(&self) -> Option<(PathBuf, String)> {
        let autosave = self.autosave.as_ref()?;
        Some((autosave.dir()?.to_path_buf(), autosave.session().to_owned()))
    }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::testing::TempDir;

    fn crashed(dir: &Path, session: &str, written: u64) {
        let file = dir.join(format!("{session}.{}", files::EXTENSION));
        std::fs::write(&file, "{}").unwrap();
        let note = autosave::Note {
            version: autosave::VERSION,
            project: None,
            name: session.into(),
            written,
        };
        let text = serde_json::to_string(&note).unwrap();
        std::fs::write(dir.join(format!("{session}.json")), text).unwrap();
    }

    fn answered(scan: &mut Leftovers) -> Vec<autosave::Leftover> {
        lumen_testkit::wait_for("the leftover scan's answer", || scan.poll())
    }

    #[test]
    fn a_scan_is_read_on_a_worker_and_its_answer_arrives_through_polling() {
        let tmp = TempDir::new("recovery-scan");
        crashed(tmp.path(), "old", 100);
        crashed(tmp.path(), "new", 300);
        crashed(tmp.path(), "mine", 200);
        let ctx = egui::Context::default();
        let mut scan = Leftovers::default();
        scan.start(&ctx, tmp.path(), "mine");
        assert!(scan.busy(), "the worker is looking");
        assert!(!scan.take_recover_latest(), "no flag: it is offered");
        let mut answer = None;
        lumen_testkit::wait_until("the leftover scan's answer", || {
            answer = scan.poll();
            answer.is_some()
        });
        let found = answer.expect("the scan answered");
        assert!(!scan.busy(), "the answer is in");
        let names: Vec<&str> = found.iter().map(|l| l.note.name.as_str()).collect();
        assert_eq!(names, ["new", "old"], "newest first, this session left out");
        scan.start(&ctx, tmp.path(), "mine");
        assert!(!scan.busy());
    }

    #[test]
    fn discard_deletes_the_copy_and_offers_the_next_one_from_its_answer() {
        let tmp = TempDir::new("recovery-discard");
        crashed(tmp.path(), "old", 100);
        crashed(tmp.path(), "new", 300);
        let ctx = egui::Context::default();
        let mut scan = Leftovers::default();
        scan.start(&ctx, tmp.path(), "mine");
        let found = answered(&mut scan);
        let newest = found[0].clone();
        scan.discard(&ctx, tmp.path(), "mine", &newest);
        let found = answered(&mut scan);
        assert!(!newest.file.exists(), "the discarded copy is gone");
        let names: Vec<&str> = found.iter().map(|l| l.note.name.as_str()).collect();
        assert_eq!(names, ["old"], "the next one comes after the delete");
    }

    #[test]
    fn the_recover_latest_flag_is_taken_once_for_the_first_answer() {
        let mut offered = Leftovers::default();
        assert!(!offered.take_recover_latest());
        let mut at_once = Leftovers::new(true);
        assert!(at_once.take_recover_latest());
        assert!(!at_once.take_recover_latest());
    }
}
