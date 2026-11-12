//! The "Open .lumen files" row (Preferences › Files, and the first-run
//! Ready step) and its command (M9 R8, FA A).
//!
//! The registry is read on a worker, never while drawing (sweep M4b): the
//! app draws the last answer and asks again when a window that shows the row
//! opens and after the person changes it.

use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use eframe::egui::{self, Ui};
use lumen_core::product::PRODUCT_NAME;

use crate::association::{Association, State, Status};
use crate::widgets::{hint, raised};
use crate::{App, widgets};

/// The association as a worker last read it (sweep M4b).
///
/// The hive reads behind the status (a lock and a few values, and on a real
/// start the registry) do not belong in a frame, so they are read on a
/// worker and the row draws this. `None` until the first answer: the row
/// then shows the switch disabled rather than a guess.
#[derive(Default)]
pub(crate) struct Reported {
    /// The last answer.
    status: Option<Status>,
    /// The worker reading it.
    pending: Option<Receiver<Status>>,
    /// A read has been asked for: a system that cannot start a thread is not
    /// asked again every frame.
    tried: bool,
    /// The answer held is out of date and a read is wanted.
    stale: bool,
    /// A window that shows the row was open on the last frame.
    shown: bool,
    /// A normal start has still to register this copy if nothing is.
    start_pending: bool,
    /// Why the last read could not start, until it is said.
    failure: Option<String>,
}

impl Reported {
    /// The status to draw, or `None` before the first answer: the registry
    /// is never read here.
    pub(crate) fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// The hive changed under the worker's feet (tests): this is the answer
    /// it would give, taken at once.
    pub(crate) fn set(&mut self, status: Status) {
        self.status = Some(status);
        // A read in flight predates what is known now.
        self.pending = None;
        self.stale = false;
    }

    /// A normal start: once the first answer is in, register this copy when
    /// nothing is (M9 R8, ADR-0117).
    pub(crate) fn start_registers(&mut self, yes: bool) {
        self.start_pending = yes;
    }

    /// Takes the start's registration wish; true once, for the first answer.
    fn take_start(&mut self) -> bool {
        std::mem::take(&mut self.start_pending)
    }

    /// Whether a read is wanted now: the first one (asked once, so a system
    /// that cannot start a thread is not asked again every frame), a stale
    /// answer, or a window that shows the row having just opened.
    fn wants(&self, shown: bool) -> bool {
        (!self.tried && self.status.is_none()) || self.stale || (shown && !self.shown)
    }

    /// Asks a worker for the status; nothing happens while one is reading.
    fn ask(&mut self, ctx: &egui::Context, association: &Association) {
        if self.pending.is_some() {
            return;
        }
        self.stale = false;
        self.tried = true;
        let (tx, rx) = mpsc::channel();
        let (hive, exe, ctx) = (association.hive(), association.exe.clone(), ctx.clone());
        let spawned = crate::spawn::spawn_named("lumen-association", move || {
            let status = match hive.lock() {
                Ok(hive) => crate::association::status(hive.as_ref(), &exe),
                Err(_) => crate::association::unreadable(),
            };
            let _ = tx.send(status);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.pending = Some(rx),
            Err(error) => {
                self.failure = Some(crate::spawn::start_failed(
                    "the check of which program opens project files",
                    &error,
                ));
            }
        }
    }

    /// The worker's answer, once it is in.
    fn poll(&mut self) -> Option<Status> {
        let status = self.pending.as_ref()?.try_recv().ok()?;
        self.pending = None;
        self.status = Some(status.clone());
        Some(status)
    }

    /// The app wrote the association: the row and its words follow at once
    /// (`None`: the write failed, and nothing changed), and the worker
    /// confirms it.
    fn wrote(&mut self, written: Option<bool>) {
        let Some(on) = written else {
            return;
        };
        let chosen_elsewhere = self
            .status
            .as_ref()
            .and_then(|status| status.chosen_elsewhere.clone());
        self.status = Some(Status {
            state: if on { State::This } else { State::Off },
            chosen_elsewhere,
            will_ask: false,
        });
        // A read in flight read the registry before the write.
        self.pending = None;
        self.stale = true;
    }

    /// Why a read could not start, once.
    fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}

impl App {
    /// The association as last read, for the windows and the commands:
    /// drawing never reads the registry (sweep M4b).
    pub(crate) fn association_status(&self) -> Option<&Status> {
        self.association_status.status()
    }

    /// Reads who opens `.lumen` files on a worker: at the first frame, when a
    /// window that shows it opens, and after the person changes it. Call
    /// every frame.
    pub(crate) fn poll_association(&mut self, ctx: &egui::Context) {
        let shown = self.preferences.open || self.welcome.open;
        // The registry changes while Lumen runs (the person changes it in
        // Default apps), so a window opening looks again; so does the first
        // frame, which start-up does not wait for.
        if self.association_status.wants(shown) {
            self.association_status.ask(ctx, &self.association);
        }
        self.association_status.shown = shown;
        if self.association_status.poll().is_some() && self.association_status.take_start() {
            self.associate_at_start();
        }
        if let Some(why) = self.association_status.take_failure() {
            self.say(why);
        }
    }

    /// Registers this copy or removes its registration, and remembers the
    /// wish.
    pub(crate) fn set_association(&mut self, on: bool) {
        self.association.set(on);
        self.association_status
            .wrote(self.association.error.is_none().then_some(on));
        self.settings.projects.associate_files = on;
        self.settings_changed = Some(Instant::now());
    }

    /// A normal start (owner, 2026-11-03): registers this copy when no
    /// copy of the app is registered and the setting wants it, as it does
    /// by default. Another copy that owns it, the setting turned off and
    /// the person's own "Open with" choice are left as they are.
    pub(crate) fn associate_at_start(&mut self) {
        let Some(state) = self.association_status().map(|status| status.state.clone()) else {
            return;
        };
        if !Association::supported()
            || !self.settings.projects.associate_files
            || state != State::Off
        {
            return;
        }
        self.set_association(true);
        let error = self.association.error.clone();
        crate::trace::log(|| match error {
            None => "association: registered this copy at start".to_owned(),
            Some(e) => format!("association: not registered at start ({e})"),
        });
    }

    /// The command: on when this copy is not the one registered.
    pub(crate) fn toggle_association(&mut self) {
        let on = self
            .association_status()
            .is_none_or(|status| status.state != State::This);
        self.set_association(on);
        let words = match (&self.association.error, on) {
            (Some(e), _) => e.clone(),
            (None, true) => format!("Projects now open with {PRODUCT_NAME} from File Explorer"),
            (None, false) => format!("Projects no longer open with this {PRODUCT_NAME}"),
        };
        self.say(words);
    }

    /// The switch and what it says about who opens projects now.
    pub(crate) fn association_row(&mut self, ui: &mut Ui) {
        let supported = Association::supported();
        let status = self.association_status().cloned();
        let mut on = status
            .as_ref()
            .is_some_and(|status| status.state == State::This);
        let label = format!("Open .lumen files with {PRODUCT_NAME}");
        // Read on a worker: until its answer is in, the switch shows off and
        // takes no click rather than guess.
        let known = status.is_some();
        let changed = widgets::switch(ui, &mut on, &label, supported && known)
            .on_hover_text("Projects open here from File Explorer, each with the project icon.")
            .changed();
        if changed {
            self.set_association(on);
        }
        if !supported {
            hint(
                ui,
                "On this system, project files are set up when the app is installed.",
            );
            return;
        }
        if let Some(error) = self.association.error.clone() {
            hint(ui, &error);
        }
        let Some(status) = status else {
            return;
        };
        match &status.state {
            State::Other(path) => {
                hint(
                    ui,
                    &format!(
                        "Another copy of {PRODUCT_NAME} opens them now: {}.",
                        path.display()
                    ),
                );
                if raised::button(ui, "Use this copy").clicked() {
                    self.set_association(true);
                }
            }
            State::This if status.chosen_elsewhere.is_none() => hint(
                ui,
                "Double-click a project in File Explorer to open it here.",
            ),
            _ => {}
        }
        if status.chosen_elsewhere.is_some() {
            hint(
                ui,
                "You chose another program for .lumen files in Windows, and Windows keeps that \
                 choice. Change it in Default apps.",
            );
            if raised::button(ui, "Open Default apps").clicked() {
                self.association.open_default_apps();
            }
        } else if status.will_ask {
            hint(
                ui,
                "Windows will ask which program to use the next time a project is opened.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::association::{MapHive, State};

    use super::*;

    fn association(registered: bool) -> Association {
        let exe = PathBuf::from(r"C:\Apps\Lumen\lumen.exe");
        let mut association = Association::with(Box::new(MapHive::default()), exe);
        if registered {
            association.set(true);
        }
        association
    }

    #[test]
    fn the_status_arrives_from_a_worker_and_until_then_nothing_is_drawn() {
        let ctx = egui::Context::default();
        let association = association(true);
        let mut reported = Reported::default();
        reported.ask(&ctx, &association);
        // Nothing is read where the frame is: the answer is collected from
        // the worker.
        assert!(reported.status().is_none(), "not read yet");
        let status = lumen_testkit::wait_for("the worker's status", || reported.poll());
        assert_eq!(status.state, State::This);
        assert_eq!(
            reported.status().map(|status| status.state.clone()),
            Some(State::This),
            "and kept for the frames that follow"
        );
    }

    #[test]
    fn a_change_shows_at_once_and_is_confirmed_by_a_read() {
        let mut reported = Reported::default();
        reported.wrote(Some(true));
        assert_eq!(
            reported.status().map(|status| status.state.clone()),
            Some(State::This)
        );
        assert!(reported.stale, "the worker looks again");
        // A write that failed changes nothing.
        let before = reported.status().cloned();
        reported.wrote(None);
        assert_eq!(reported.status().cloned(), before);
    }

    #[test]
    fn a_read_is_wanted_once_until_a_window_opens_or_the_answer_is_stale() {
        let mut reported = Reported::default();
        assert!(reported.wants(false), "the first look");
        reported.tried = true;
        assert!(!reported.wants(false), "not asked again every frame");
        assert!(reported.wants(true), "a window showing the row opened");
        reported.shown = true;
        assert!(!reported.wants(true), "and it stays open");
        reported.stale = true;
        assert!(reported.wants(true), "a change asks again");
    }

    #[test]
    fn a_seeded_hive_drops_a_read_that_predates_it() {
        let ctx = egui::Context::default();
        let association = association(false);
        let mut reported = Reported::default();
        reported.ask(&ctx, &association);
        reported.set(crate::association::Status {
            state: State::Other(PathBuf::from(r"D:\Old\lumen.exe")),
            chosen_elsewhere: None,
            will_ask: false,
        });
        // The read in flight is let go: what is known now wins, and nothing
        // stale arrives later to undo it.
        assert!(!reported.stale);
        assert!(reported.pending.is_none());
        assert!(matches!(
            reported.status().map(|status| &status.state),
            Some(State::Other(_))
        ));
    }
}
