//! Autosave and the recovery banner after a crash (M3 slice 3d); the
//! autosave itself is `crate::autosave`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use crate::recording::take_notes::delete_later;
use crate::widgets::raised;
use crate::{App, autosave, files, trace};

/// The autosaves left by other sessions, read on a worker (sweep M4a).
///
/// The folder is read there, with its locks and notes, and the app polls the
/// answer each frame: neither start-up nor a click waits on the disk. Until
/// the first answer no banner is offered, and otherwise the banner offers the
/// newest leftover and then the next, as it always did.
#[derive(Default)]
pub(crate) struct Leftovers {
    /// The worker reading the folder.
    pending: Option<Receiver<Vec<autosave::Leftover>>>,
    /// An answer delivered without a worker (no thread could start).
    ready: Option<Vec<autosave::Leftover>>,
    /// The start's look has been asked for.
    seen: bool,
    /// The start recovers the newest leftover at once, instead of offering
    /// it (`--recover-latest`).
    recover_latest: bool,
    /// Why the last read could not start, until it is said.
    failure: Option<String>,
}

impl Leftovers {
    /// The start's look. With `recover_latest` the newest leftover is
    /// recovered at once; an explicit project on the command line turns that
    /// off, as it wins over it.
    pub(crate) fn new(recover_latest: bool) -> Self {
        Self {
            recover_latest,
            ..Self::default()
        }
    }

    /// Asks a worker for the autosaves in `dir` other than `session`'s own,
    /// once: the start's look.
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

    /// Deletes `leftover` and reads the folder again, on one worker: the
    /// deletion must finish before the read, or the same autosave would be
    /// offered all over again.
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
                // No thread to delete on: the click does it here, which is
                // the one place the interface waits on the disk, so that
                // Discard always discards. Logged rather than said: what the
                // person asked for happened.
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

    /// The worker's answer, once it is in.
    pub(crate) fn poll(&mut self) -> Option<Vec<autosave::Leftover>> {
        if let Some(found) = self.ready.take() {
            self.pending = None;
            return Some(found);
        }
        let found = self.pending.as_ref()?.try_recv().ok()?;
        self.pending = None;
        Some(found)
    }

    /// Whether a read is under way (tests wait for it).
    #[cfg(test)]
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether the start's look is still to be made.
    pub(crate) fn unseen(&self) -> bool {
        !self.seen
    }

    /// Whether the start's answer recovers the newest leftover at once; true
    /// once, for the first answer.
    pub(crate) fn take_recover_latest(&mut self) -> bool {
        std::mem::take(&mut self.recover_latest)
    }

    /// Why a read could not start, once.
    pub(crate) fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}

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
        // Crash protection is off if its worker cannot start: say so, and say
        // what is still safe (the project in memory).
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
                // Deleted and the folder read again on a worker: the banner
                // is hidden until its answer comes (sweep M4a).
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

    /// The leftover scan's answers, and its first look. Call every frame,
    /// before the banners draw: the scan runs on a worker, so start-up never
    /// waits on the disk and the banner is offered once its answer is in
    /// (sweep M4a).
    pub(crate) fn poll_leftovers(&mut self, ctx: &egui::Context) {
        if self.leftovers.unseen()
            && let Some((dir, session)) = self.autosave_place()
        {
            self.leftovers.start(ctx, &dir, &session);
        }
        if let Some(found) = self.leftovers.poll() {
            // `--recover-latest`: the newest one is opened at once, as a
            // start with it did before the scan moved to a worker.
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

    /// Where this session's autosaves go, and the session's own name: what
    /// a leftover scan leaves out.
    fn autosave_place(&self) -> Option<(PathBuf, String)> {
        let autosave = self.autosave.as_ref()?;
        Some((autosave.dir()?.to_path_buf(), autosave.session().to_owned()))
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::testing::TempDir;

    /// An autosave `session` left behind by a session that ended badly.
    fn crashed(dir: &Path, session: &str, written: u64) {
        let file = dir.join(format!("{session}.{}", files::EXTENSION));
        std::fs::write(&file, "{}").unwrap();
        let note = autosave::Note {
            project: None,
            name: session.into(),
            written,
        };
        let text = serde_json::to_string(&note).unwrap();
        std::fs::write(dir.join(format!("{session}.json")), text).unwrap();
    }

    /// The scan's answer, waiting on the worker with a hang guard.
    fn answered(scan: &mut Leftovers) -> Vec<autosave::Leftover> {
        lumen_testkit::wait_for("the leftover scan's answer", || scan.poll())
    }

    #[test]
    fn a_scan_is_read_on_a_worker_and_its_answer_arrives_through_polling() {
        let tmp = TempDir::new("recovery-scan");
        crashed(tmp.path(), "old", 100);
        crashed(tmp.path(), "new", 300);
        // This session's own autosave is never offered to itself.
        crashed(tmp.path(), "mine", 200);
        let ctx = egui::Context::default();
        let mut scan = Leftovers::default();
        scan.start(&ctx, tmp.path(), "mine");
        // Nothing was read where the start is: the folder is read on the
        // worker, and only `poll` collects its answer.
        assert!(scan.busy(), "the worker is looking");
        assert!(!scan.take_recover_latest(), "no flag: it is offered");
        let found = answered(&mut scan);
        assert!(!scan.busy(), "the answer is in");
        let names: Vec<&str> = found.iter().map(|l| l.note.name.as_str()).collect();
        assert_eq!(names, ["new", "old"], "newest first, this session left out");
        // Asked once: a second start adds no look.
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
